mod actions;
mod artwork;
mod layout;
mod media;
mod selection;

use actions::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use openaction::*;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::LazyLock, time::Duration};
use tokio::sync::RwLock;
use zbus::Connection;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Settings {
	pub show_status: bool,
	pub artwork_grid: u8,
	pub artwork_position: u8,
}
static SETTINGS: LazyLock<RwLock<HashMap<String, Settings>>> = LazyLock::new(Default::default);
const ACTIONS: &[&str] = &[
	ArtworkAction::UUID,
	PlayPauseAction::UUID,
	StopAction::UUID,
	PreviousAction::UUID,
	NextAction::UUID,
	RepeatAction::UUID,
	ShuffleAction::UUID,
	SeekBackwardsAction::UUID,
	SeekForwardsAction::UUID,
	VolumeUpAction::UUID,
	VolumeDownAction::UUID,
];

async fn remember(instance: &Instance, settings: &Settings) {
	SETTINGS
		.write()
		.await
		.insert(instance.instance_id.clone(), settings.clone());
}
async fn forget(instance: &Instance) {
	SETTINGS.write().await.remove(&instance.instance_id);
}
async fn press(instance: &Instance, _: &Settings, action: &str) {
	if action == "artwork" {
		return;
	}
	let Some(player) = selection::selected().await else {
		let _ = instance.show_alert().await;
		return;
	};
	let result = tokio::time::timeout(Duration::from_secs(4), async {
		let conn = Connection::session().await?;
		let proxy = media::select(&conn, &player).await?;
		media::command(&proxy, action).await
	})
	.await;
	match result {
		Ok(Ok(())) => {}
		other => {
			log::warn!("{action} for '{}': {other:?}", player);
			let _ = instance.show_alert().await;
		}
	}
}

#[derive(Clone, Debug, Default, PartialEq)]
struct State {
	available: bool,
	playback: String,
	repeat: u16,
	shuffle: u16,
	player: String,
	track: String,
	art_url: String,
}
async fn state(player: &str) -> anyhow::Result<State> {
	let conn = Connection::session().await?;
	let proxy = media::select(&conn, player).await?;
	let playback = proxy.get_property::<String>("PlaybackStatus").await?;
	let repeat = match proxy
		.get_property::<String>("LoopStatus")
		.await
		.unwrap_or_default()
		.as_str()
	{
		"Playlist" => 1,
		"Track" => 2,
		_ => 0,
	};
	let shuffle = proxy.get_property::<bool>("Shuffle").await.unwrap_or(false) as u16;
	let metadata = proxy
		.get_property::<HashMap<String, zbus::zvariant::OwnedValue>>("Metadata")
		.await
		.unwrap_or_default();
	let art_url = metadata
		.get("mpris:artUrl")
		.and_then(|v| <&str>::try_from(v).ok())
		.unwrap_or_default()
		.to_owned();
	let track = format!("{:?}", metadata.get("mpris:trackid"));
	Ok(State {
		player: proxy.destination().to_string(),
		track,
		art_url,
		available: true,
		playback,
		repeat,
		shuffle,
	})
}

// Existing OpenDeck profile buttons can retain old state definitions after upgrades.
// Send the actual current image as well as the state index to support those buttons.
static CONTROL_IMAGES: LazyLock<HashMap<&'static str, String>> = LazyLock::new(|| {
	[
		(
			"play",
			include_bytes!("../assets/icons/play.png").as_slice(),
		),
		(
			"pause",
			include_bytes!("../assets/icons/pause.png").as_slice(),
		),
		(
			"repeat_none",
			include_bytes!("../assets/icons/repeat_none.png").as_slice(),
		),
		(
			"repeat_playlist",
			include_bytes!("../assets/icons/repeat_playlist.png").as_slice(),
		),
		(
			"repeat_track",
			include_bytes!("../assets/icons/repeat_track.png").as_slice(),
		),
		(
			"shuffle_off",
			include_bytes!("../assets/icons/shuffle_off.png").as_slice(),
		),
		(
			"shuffle_on",
			include_bytes!("../assets/icons/shuffle_on.png").as_slice(),
		),
	]
	.into_iter()
	.map(|(key, bytes)| {
		(
			key,
			format!("data:image/png;base64,{}", STANDARD.encode(bytes)),
		)
	})
	.collect()
});

async fn watch_players() {
	let mut last = HashMap::new();
	let mut art = artwork::Cache::new();
	let mut tiles = artwork::TileCache::default();
	let mut last_images = HashMap::new();
	loop {
		tokio::time::sleep(Duration::from_secs(1)).await;
		let settings = SETTINGS.read().await.clone();
		last.retain(|id, _| settings.contains_key(id));
		last_images.retain(|id, _| settings.contains_key(id));
		if settings.is_empty() {
			continue;
		}
		let Some(player) = selection::selected().await else {
			continue;
		};
		let current = match tokio::time::timeout(Duration::from_secs(3), state(&player)).await {
			Ok(Ok(state)) => state,
			_ => State::default(),
		};
		let artwork_instances = visible_instances(ArtworkAction::UUID).await;
		let mut by_device: HashMap<String, std::collections::HashSet<(u8, u8)>> = HashMap::new();
		for instance in &artwork_instances {
			let automatic = settings
				.get(&instance.instance_id)
				.is_none_or(|s| s.artwork_grid == 0);
			if automatic
				&& instance.controller == "Keypad"
				&& !instance.is_in_multi_action
				&& let Some(pos) = instance.coordinates
			{
				by_device
					.entry(instance.device_id.clone())
					.or_default()
					.insert((pos.row, pos.column));
			}
		}
		let detected: HashMap<_, _> = by_device
			.into_iter()
			.filter_map(|(device, coords)| layout::detect(&coords).map(|tiles| (device, tiles)))
			.collect();
		for action in ACTIONS {
			for instance in visible_instances(action).await {
				let Some(config) = settings.get(&instance.instance_id) else {
					continue;
				};
				let rendered = (current.clone(), config.clone());
				if last.get(&instance.instance_id) == Some(&rendered) {
					continue;
				}
				if *action == PlayPauseAction::UUID {
					let title = if config.show_status {
						if !current.available {
							"Offline"
						} else if current.playback == "Playing" {
							"Playing"
						} else {
							"Paused"
						}
					} else {
						""
					};
					// Explicitly clear titles in BOTH states, including older plugin titles.
					for index in 0..=1 {
						let _ = instance.set_title(Some(title), Some(index)).await;
					}
					let _ = instance
						.set_state((current.playback == "Playing") as u16)
						.await;
				} else if *action == RepeatAction::UUID || *action == ShuffleAction::UUID {
					// Clear labels cached in older OpenDeck profiles in every state.
					let count = if *action == RepeatAction::UUID { 3 } else { 2 };
					for index in 0..count {
						let _ = instance.set_title(Some(""), Some(index)).await;
					}
				} else if *action == ArtworkAction::UUID {
					let _ = instance.set_title(Some(""), None).await;
				} else {
					let _ = instance
						.set_title(
							if current.available {
								None
							} else {
								Some("Offline")
							},
							None,
						)
						.await;
				}
				if *action == RepeatAction::UUID {
					let _ = instance.set_state(current.repeat).await;
				}
				if *action == ShuffleAction::UUID {
					let _ = instance.set_state(current.shuffle).await;
				}
				let icon = if *action == PlayPauseAction::UUID {
					Some(if current.playback == "Playing" {
						"pause"
					} else {
						"play"
					})
				} else if *action == RepeatAction::UUID {
					Some(match current.repeat {
						1 => "repeat_playlist",
						2 => "repeat_track",
						_ => "repeat_none",
					})
				} else if *action == ShuffleAction::UUID {
					Some(if current.shuffle == 0 {
						"shuffle_off"
					} else {
						"shuffle_on"
					})
				} else {
					None
				};
				if let Some(icon) = icon {
					// No state parameter: update the visible state even on legacy one-state buttons.
					if let Err(error) = instance
						.set_image(Some(CONTROL_IMAGES[icon].clone()), None)
						.await
					{
						log::warn!("Failed to update control image: {error}");
						continue;
					}
					if *action == RepeatAction::UUID || *action == ShuffleAction::UUID {
						let _ = instance.set_title(Some(""), None).await;
					}
				}
				last.insert(instance.instance_id.clone(), rendered);
			}
		}
		let instances = artwork_instances;
		if !instances.is_empty() {
			let image = art
				.get(&current.player, &current.track, &current.art_url)
				.await;
			// Selection may change while artwork is loading; never display an old player's cover.
			if selection::selected().await.as_deref() != Some(&player) {
				continue;
			}
			tiles.update(image);
			for instance in instances {
				let Some(config) = settings.get(&instance.instance_id) else {
					continue;
				};
				let (grid, position) = if config.artwork_grid == 0 {
					instance
						.coordinates
						.and_then(|pos| {
							detected
								.get(&instance.device_id)?
								.get(&(pos.row, pos.column))
								.copied()
						})
						.unwrap_or((1, 0))
				} else {
					(config.artwork_grid, config.artwork_position)
				};
				let image = tiles.tile(grid, position).await;
				if last_images.get(&instance.instance_id) != Some(&image)
					&& instance.set_image(image.clone(), None).await.is_ok()
				{
					last_images.insert(instance.instance_id.clone(), image.clone());
				}
			}
		}
	}
}

#[tokio::main]
async fn main() -> OpenActionResult<()> {
	let _ = simplelog::SimpleLogger::init(log::LevelFilter::Info, simplelog::Config::default());
	if std::env::args().nth(1).as_deref() == Some("--list-players") {
		match Connection::session().await {
			Ok(conn) => match media::names(&conn).await {
				Ok(names) => {
					for name in names {
						println!("{name}");
					}
				}
				Err(e) => {
					eprintln!("{e}");
					std::process::exit(1);
				}
			},
			Err(e) => {
				eprintln!("Cannot access desktop session bus: {e}");
				std::process::exit(1);
			}
		}
		return Ok(());
	}
	if std::env::args().nth(1).as_deref() == Some("--diagnose") {
		let player = std::env::args()
			.nth(2)
			.unwrap_or_else(|| "strawberry".into());
		match tokio::time::timeout(Duration::from_secs(5), state(&player)).await {
			Ok(Ok(snapshot)) => {
				println!(
					"Player: {player}\nPlayback: {}\nRepeat state: {}\nShuffle: {}",
					snapshot.playback,
					snapshot.repeat,
					snapshot.shuffle != 0
				);
				let image = artwork::Cache::new()
					.get(&snapshot.player, &snapshot.track, &snapshot.art_url)
					.await;
				println!(
					"Artwork: {}",
					if image.is_some() {
						"loaded"
					} else {
						"unavailable"
					}
				);
			}

			result => {
				eprintln!("Unable to read player '{player}': {result:?}");
				std::process::exit(1);
			}
		}
		return Ok(());
	}
	global_events::set_global_event_handler(&selection::Handler);
	register_action(ArtworkAction).await;
	register_action(PlayPauseAction).await;
	register_action(StopAction).await;
	register_action(PreviousAction).await;
	register_action(NextAction).await;
	register_action(RepeatAction).await;
	register_action(ShuffleAction).await;
	register_action(SeekBackwardsAction).await;
	register_action(SeekForwardsAction).await;
	register_action(VolumeUpAction).await;
	register_action(VolumeDownAction).await;
	tokio::spawn(watch_players());
	run(std::env::args().collect()).await
}
