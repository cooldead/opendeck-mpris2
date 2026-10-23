use anyhow::{Result, bail};
use zbus::{Connection, Proxy};

const PREFIX: &str = "org.mpris.MediaPlayer2.";

pub fn matches_player(name: &str, requested: &str) -> bool {
	let requested = requested.trim();
	let full = if requested.starts_with(PREFIX) {
		requested.to_owned()
	} else {
		format!("{PREFIX}{requested}")
	};
	name == full || name.starts_with(&format!("{full}."))
}

pub async fn player_proxy(conn: &Connection, name: String) -> Result<Proxy<'static>> {
	Ok(Proxy::new_owned(
		conn.clone(),
		name,
		"/org/mpris/MediaPlayer2",
		"org.mpris.MediaPlayer2.Player",
	)
	.await?)
}

pub async fn names(conn: &Connection) -> Result<Vec<String>> {
	let dbus = zbus::fdo::DBusProxy::new(conn).await?;
	let mut names: Vec<String> = dbus
		.list_names()
		.await?
		.into_iter()
		.map(|n| n.to_string())
		.filter(|n| n.starts_with(PREFIX))
		.collect();
	names.sort();
	Ok(names)
}

pub async fn select(conn: &Connection, requested: &str) -> Result<Proxy<'static>> {
	let names = names(conn).await?;
	let requested = requested.trim();
	if requested != "auto" {
		let requested = if requested.is_empty() {
			"strawberry"
		} else {
			requested
		};
		let name = names
			.into_iter()
			.find(|n| matches_player(n, requested))
			.ok_or_else(|| {
				anyhow::anyhow!("Player '{requested}' is unavailable; start it and enable MPRIS2")
			})?;
		return player_proxy(conn, name).await;
	}
	let mut statuses = Vec::new();
	for name in names {
		let proxy = player_proxy(conn, name.clone()).await?;
		// A player that doesn't answer quickly (throttled in the background) keeps its old status.
		match tokio::time::timeout(
			std::time::Duration::from_millis(800),
			proxy.get_property::<String>("PlaybackStatus"),
		)
		.await
		{
			Ok(Ok(status)) => statuses.push((name, status)),
			_ => {
				let known = AUTO.lock().unwrap().seen.get(&name).cloned();
				statuses.push((name, known.unwrap_or_default()));
			}
		}
	}
	let name = AUTO
		.lock()
		.unwrap()
		.choose(&statuses)
		.ok_or_else(|| anyhow::anyhow!("No MPRIS2 players found"))?;
	player_proxy(conn, name).await
}

/// "auto" player choice. Follows the player that most recently *started* playing and stays
/// with it while it is paused, instead of jumping to whichever player sorts first (a paused
/// music app in the background used to lose its tiles to an idle browser).
#[derive(Default)]
pub struct Auto {
	current: Option<String>,
	seen: std::collections::HashMap<String, String>,
}
static AUTO: std::sync::LazyLock<std::sync::Mutex<Auto>> = std::sync::LazyLock::new(Default::default);

impl Auto {
	pub fn choose(&mut self, statuses: &[(String, String)]) -> Option<String> {
		let rank = |name: &String| (!matches_player(name, "strawberry"), name.clone());
		let playing = |(_, s): &&(String, String)| s == "Playing";
		let started = statuses
			.iter()
			.filter(playing)
			.filter(|(n, _)| self.seen.get(n).is_some_and(|old| old != "Playing"))
			.map(|(n, _)| n)
			.min_by_key(|n| rank(n));
		let present = |n: &String| statuses.iter().any(|(m, _)| m == n);
		let current = self.current.clone().filter(present);
		let current_playing = current
			.as_ref()
			.is_some_and(|c| statuses.iter().any(|(n, s)| n == c && s == "Playing"));
		let any_playing = statuses.iter().filter(playing).map(|(n, _)| n).min_by_key(|n| rank(n));
		let choice = if let Some(n) = started {
			Some(n.clone())
		} else if current_playing {
			current
		} else if let Some(n) = any_playing {
			Some(n.clone())
		} else if current.is_some() {
			current
		} else {
			statuses.iter().map(|(n, _)| n).min_by_key(|n| rank(n)).cloned()
		};
		self.seen = statuses.iter().cloned().collect();
		self.current = choice.clone();
		choice
	}
}

pub fn next_repeat(current: &str) -> &'static str {
	match current {
		"None" => "Playlist",
		"Playlist" => "Track",
		_ => "None",
	}
}

pub async fn command(proxy: &Proxy<'_>, action: &str) -> Result<()> {
	let capability = match action {
		"next" => "CanGoNext",
		"previous" => "CanGoPrevious",
		"seekbackwards" | "seekforwards" => "CanSeek",
		_ => "CanControl",
	};
	if !proxy.get_property::<bool>(capability).await? {
		bail!("Player does not currently support {action}");
	}
	match action {
		"playpause" | "stop" | "previous" | "next" => {
			let method = match action {
				"playpause" => "PlayPause",
				"stop" => "Stop",
				"previous" => "Previous",
				_ => "Next",
			};
			proxy.call_method(method, &()).await?;
		}
		"seekbackwards" | "seekforwards" => {
			let offset: i64 = if action == "seekbackwards" {
				-10_000_000
			} else {
				10_000_000
			};
			proxy.call_method("Seek", &(offset,)).await?;
		}
		"repeat" => {
			let current: String = proxy.get_property("LoopStatus").await?;
			proxy
				.set_property("LoopStatus", next_repeat(&current))
				.await?;
		}
		"shuffle" => {
			let current: bool = proxy.get_property("Shuffle").await?;
			proxy.set_property("Shuffle", !current).await?;
		}
		"volumeup" | "volumedown" => {
			let current: f64 = proxy.get_property("Volume").await?;
			let delta = if action == "volumeup" { 0.05 } else { -0.05 };
			proxy
				.set_property("Volume", (current + delta).clamp(0.0, 1.0))
				.await?;
		}
		_ => bail!("Unknown action: {action}"),
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn player_matching_handles_instances_without_matching_other_apps() {
		assert!(matches_player(
			"org.mpris.MediaPlayer2.strawberry",
			"strawberry"
		));
		assert!(matches_player(
			"org.mpris.MediaPlayer2.strawberry.instance42",
			"strawberry"
		));
		assert!(matches_player(
			"org.mpris.MediaPlayer2.strawberry",
			"org.mpris.MediaPlayer2.strawberry"
		));
		assert!(!matches_player(
			"org.mpris.MediaPlayer2.strawberry-other",
			"strawberry"
		));
		assert!(!matches_player("org.mpris.MediaPlayer2.vlc", "strawberry"));
	}
	fn st(list: &[(&str, &str)]) -> Vec<(String, String)> {
		list.iter()
			.map(|(n, s)| (format!("{PREFIX}{n}"), s.to_string()))
			.collect()
	}
	#[test]
	fn auto_stays_on_a_paused_player_instead_of_jumping_to_an_idle_one() {
		let mut auto = Auto::default();
		let pick = |a: &mut Auto, l: &[(&str, &str)]| a.choose(&st(l)).unwrap().replace(PREFIX, "");
		assert_eq!(pick(&mut auto, &[("brave", "Paused"), ("cider", "Playing")]), "cider");
		// Cider pauses in the background: keep showing Cider, not the alphabetically first Brave.
		assert_eq!(pick(&mut auto, &[("brave", "Paused"), ("cider", "Paused")]), "cider");
		assert_eq!(pick(&mut auto, &[("brave", "Stopped"), ("cider", "Paused")]), "cider");
		// Something else starts playing: follow it.
		assert_eq!(pick(&mut auto, &[("brave", "Playing"), ("cider", "Paused")]), "brave");
		// Cider starts again while Brave still plays: the newest one wins.
		assert_eq!(pick(&mut auto, &[("brave", "Playing"), ("cider", "Playing")]), "cider");
		assert_eq!(pick(&mut auto, &[("brave", "Playing"), ("cider", "Playing")]), "cider");
		// Cider quits: fall back to what is playing.
		assert_eq!(pick(&mut auto, &[("brave", "Playing")]), "brave");
		assert!(auto.choose(&[]).is_none());
	}
	#[test]
	fn auto_first_choice_prefers_playing_then_strawberry() {
		let mut auto = Auto::default();
		assert_eq!(
			auto.choose(&st(&[("a", "Paused"), ("strawberry", "Paused")])).unwrap(),
			format!("{PREFIX}strawberry")
		);
		let mut auto = Auto::default();
		assert_eq!(
			auto.choose(&st(&[("a", "Paused"), ("z", "Playing")])).unwrap(),
			format!("{PREFIX}z")
		);
	}
	#[test]
	fn repeat_cycles() {
		assert_eq!(next_repeat("None"), "Playlist");
		assert_eq!(next_repeat("Playlist"), "Track");
		assert_eq!(next_repeat("Track"), "None");
	}
}

#[cfg(test)]
mod integration_tests {
	use super::*;
	use std::sync::{Arc, Mutex};

	#[derive(Default)]
	struct Data {
		playing: bool,
		volume: f64,
		shuffle: bool,
		loop_status: String,
		calls: Vec<String>,
		seek: i64,
	}
	struct Player(Arc<Mutex<Data>>);
	#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
	impl Player {
		fn play_pause(&self) {
			let mut d = self.0.lock().unwrap();
			d.playing = !d.playing;
			d.calls.push("PlayPause".into());
		}
		fn stop(&self) {
			let mut d = self.0.lock().unwrap();
			d.playing = false;
			d.calls.push("Stop".into());
		}
		fn next(&self) {
			self.0.lock().unwrap().calls.push("Next".into());
		}
		fn previous(&self) {
			self.0.lock().unwrap().calls.push("Previous".into());
		}
		fn seek(&self, offset: i64) {
			self.0.lock().unwrap().seek += offset;
		}
		#[zbus(property)]
		fn can_control(&self) -> bool {
			true
		}
		#[zbus(property)]
		fn can_go_next(&self) -> bool {
			true
		}
		#[zbus(property)]
		fn can_go_previous(&self) -> bool {
			true
		}
		#[zbus(property)]
		fn can_seek(&self) -> bool {
			true
		}
		#[zbus(property)]
		fn playback_status(&self) -> String {
			if self.0.lock().unwrap().playing {
				"Playing"
			} else {
				"Paused"
			}
			.into()
		}
		#[zbus(property)]
		fn volume(&self) -> f64 {
			self.0.lock().unwrap().volume
		}
		#[zbus(property)]
		fn set_volume(&mut self, value: f64) {
			self.0.lock().unwrap().volume = value;
		}
		#[zbus(property)]
		fn shuffle(&self) -> bool {
			self.0.lock().unwrap().shuffle
		}
		#[zbus(property)]
		fn set_shuffle(&mut self, value: bool) {
			self.0.lock().unwrap().shuffle = value;
		}
		#[zbus(property)]
		fn loop_status(&self) -> String {
			self.0.lock().unwrap().loop_status.clone()
		}
		#[zbus(property)]
		fn set_loop_status(&mut self, value: String) {
			self.0.lock().unwrap().loop_status = value;
		}
	}
	async fn serve(name: &str, data: Arc<Mutex<Data>>) -> Connection {
		zbus::connection::Builder::session()
			.unwrap()
			.name(name)
			.unwrap()
			.serve_at("/org/mpris/MediaPlayer2", Player(data))
			.unwrap()
			.build()
			.await
			.unwrap()
	}
	#[tokio::test]
	#[ignore = "Run under dbus-run-session on an isolated bus"]
	async fn commands_selection_and_restart() {
		let conn = Connection::session().await.unwrap();
		assert!(
			names(&conn).await.unwrap().is_empty(),
			"Use an isolated bus, never the desktop bus"
		);
		let other = Arc::new(Mutex::new(Data {
			playing: true,
			..Default::default()
		}));
		let _other_service = serve("org.mpris.MediaPlayer2.browser", other.clone()).await;
		assert!(
			select(&conn, "strawberry").await.is_err(),
			"Explicit selection must never fall back"
		);
		let data = Arc::new(Mutex::new(Data {
			volume: 0.98,
			loop_status: "None".into(),
			..Default::default()
		}));
		let service = serve("org.mpris.MediaPlayer2.strawberry", data.clone()).await;
		assert_eq!(
			select(&conn, "auto").await.unwrap().destination().as_str(),
			"org.mpris.MediaPlayer2.browser"
		);
		for action in [
			"playpause",
			"next",
			"previous",
			"seekforwards",
			"volumeup",
			"shuffle",
			"repeat",
		] {
			command(&select(&conn, "strawberry").await.unwrap(), action)
				.await
				.unwrap();
		}
		{
			let d = data.lock().unwrap();
			assert!(d.playing);
			assert!(d.shuffle);
			assert_eq!(d.loop_status, "Playlist");
			assert_eq!(d.volume, 1.0);
			assert_eq!(d.seek, 10_000_000);
			assert_eq!(d.calls, ["PlayPause", "Next", "Previous"]);
		}
		assert_eq!(
			select(&conn, "auto").await.unwrap().destination().as_str(),
			"org.mpris.MediaPlayer2.strawberry"
		);
		for action in [
			"seekbackwards",
			"volumedown",
			"stop",
			"repeat",
			"repeat",
			"shuffle",
		] {
			command(&select(&conn, "strawberry").await.unwrap(), action)
				.await
				.unwrap();
		}
		{
			let d = data.lock().unwrap();
			assert!(!d.playing);
			assert!(!d.shuffle);
			assert_eq!(d.loop_status, "None");
			assert_eq!(d.volume, 0.95);
			assert_eq!(d.seek, 0);
		}
		assert!(other.lock().unwrap().calls.is_empty());
		service
			.release_name("org.mpris.MediaPlayer2.strawberry")
			.await
			.unwrap();
		assert!(select(&conn, "strawberry").await.is_err());
		let _restart = serve("org.mpris.MediaPlayer2.strawberry.instance42", data.clone()).await;
		assert!(select(&conn, "strawberry").await.is_ok());
		data.lock().unwrap().volume = 0.01;
		command(&select(&conn, "strawberry").await.unwrap(), "volumedown")
			.await
			.unwrap();
		assert_eq!(data.lock().unwrap().volume, 0.0);
	}
}
