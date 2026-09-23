use crate::{Settings, forget, press, remember};
use openaction::*;

macro_rules! media_action {
	($name:ident, $suffix:literal) => {
		pub struct $name;
		#[async_trait]
		impl Action for $name {
			const UUID: ActionUuid = concat!("com.cooldead.mpris2.", $suffix);
			type Settings = Settings;
			async fn will_appear(
				&self,
				instance: &Instance,
				settings: &Settings,
			) -> OpenActionResult<()> {
				remember(instance, settings).await;
				Ok(())
			}
			async fn did_receive_settings(
				&self,
				instance: &Instance,
				settings: &Settings,
			) -> OpenActionResult<()> {
				remember(instance, settings).await;
				Ok(())
			}
			async fn will_disappear(
				&self,
				instance: &Instance,
				_: &Settings,
			) -> OpenActionResult<()> {
				forget(instance).await;
				Ok(())
			}
			async fn send_to_plugin(
				&self,
				instance: &Instance,
				_: &Settings,
				payload: &serde_json::Value,
			) -> OpenActionResult<()> {
				crate::selection::inspector(instance, payload).await
			}
			async fn key_up(
				&self,
				instance: &Instance,
				settings: &Settings,
			) -> OpenActionResult<()> {
				press(instance, settings, $suffix).await;
				Ok(())
			}
		}
	};
}
media_action!(PlayPauseAction, "playpause");
media_action!(StopAction, "stop");
media_action!(PreviousAction, "previous");
media_action!(NextAction, "next");
media_action!(RepeatAction, "repeat");
media_action!(ShuffleAction, "shuffle");
media_action!(SeekBackwardsAction, "seekbackwards");
media_action!(SeekForwardsAction, "seekforwards");
media_action!(VolumeUpAction, "volumeup");
media_action!(VolumeDownAction, "volumedown");

media_action!(ArtworkAction, "artwork");
