use crate::media;
use openaction::{global_events::*, *};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{sync::LazyLock, time::Duration};
use tokio::sync::RwLock;
use zbus::Connection;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GlobalSettings {
	pub player: String,
}
impl Default for GlobalSettings {
	fn default() -> Self {
		Self {
			player: "strawberry".into(),
		}
	}
}
static GLOBAL: LazyLock<RwLock<Option<GlobalSettings>>> = LazyLock::new(Default::default);
pub async fn selected() -> Option<String> {
	GLOBAL.read().await.as_ref().map(|s| s.player.clone())
}

pub struct Handler;
#[async_trait]
impl GlobalEventHandler for Handler {
	async fn plugin_ready(&self) -> OpenActionResult<()> {
		get_global_settings().await
	}
	async fn did_receive_global_settings(
		&self,
		event: DidReceiveGlobalSettingsEvent,
	) -> OpenActionResult<()> {
		let mut settings =
			serde_json::from_value::<GlobalSettings>(event.payload.settings).unwrap_or_default();
		if settings.player.trim().is_empty() {
			settings.player = "strawberry".into();
		}
		*GLOBAL.write().await = Some(settings);
		Ok(())
	}
}

pub async fn inspector(instance: &Instance, payload: &Value) -> OpenActionResult<()> {
	if payload["command"] == "selectPlayer"
		&& let Some(player) = payload["player"].as_str().filter(|s| !s.trim().is_empty())
	{
		let settings = GlobalSettings {
			player: player.trim().to_owned(),
		};
		// Persist through the host so selection survives plugin and OpenDeck restarts.
		set_global_settings(&settings).await?;
		*GLOBAL.write().await = Some(settings);
	}
	let players = tokio::time::timeout(Duration::from_secs(3), async {
		let conn = Connection::session().await?;
		let mut players = Vec::new();
		for name in media::names(&conn).await? {
			let root = zbus::Proxy::new(
				&conn,
				name.as_str(),
				"/org/mpris/MediaPlayer2",
				"org.mpris.MediaPlayer2",
			)
			.await?;
			let label = root
				.get_property::<String>("Identity")
				.await
				.unwrap_or_else(|_| {
					name.trim_start_matches("org.mpris.MediaPlayer2.")
						.to_owned()
				});
			players.push(json!({"value":name, "label":label}));
		}
		Ok::<_, anyhow::Error>(players)
	})
	.await;
	let (players, error) = match players {
		Ok(Ok(players)) => (players, None),
		_ => (
			Vec::new(),
			Some("Could not discover players. Check the desktop session bus."),
		),
	};
	instance
		.send_to_property_inspector(
			json!({"players":players,"selected":selected().await,"error":error}),
		)
		.await
}
