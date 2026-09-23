use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
	collections::HashMap,
	process::Command,
	sync::{Arc, Mutex},
	time::Duration,
};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{WebSocketStream, accept_async, tungstenite::Message};

struct Child(std::process::Child);
impl Drop for Child {
	fn drop(&mut self) {
		let _ = self.0.kill();
		let _ = self.0.wait();
	}
}
#[derive(Default)]
struct Data {
	playing: bool,
	next: usize,
	artwork: String,
}
struct Player(Arc<Mutex<Data>>);
#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
	fn play_pause(&self) {
		let mut d = self.0.lock().unwrap();
		d.playing = !d.playing;
	}
	fn next(&self) {
		self.0.lock().unwrap().next += 1;
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
	fn playback_status(&self) -> String {
		if self.0.lock().unwrap().playing {
			"Playing"
		} else {
			"Paused"
		}
		.into()
	}
	#[zbus(property)]
	fn loop_status(&self) -> String {
		"None".into()
	}
	#[zbus(property)]
	fn shuffle(&self) -> bool {
		false
	}
	#[zbus(property)]
	fn metadata(&self) -> HashMap<String, zbus::zvariant::OwnedValue> {
		HashMap::from([(
			"mpris:artUrl".into(),
			zbus::zvariant::Value::from(self.0.lock().unwrap().artwork.clone())
				.try_to_owned()
				.unwrap(),
		)])
	}
}
async fn serve(name: &str, data: Arc<Mutex<Data>>) -> zbus::Connection {
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
type Socket = WebSocketStream<TcpStream>;
async fn send(ws: &mut Socket, value: Value) {
	ws.send(Message::Text(value.to_string().into()))
		.await
		.unwrap();
}
async fn until(ws: &mut Socket, predicate: impl Fn(&Value) -> bool) -> Value {
	tokio::time::timeout(Duration::from_secs(8), async {
		loop {
			let message = ws.next().await.unwrap().unwrap();
			if let Ok(text) = message.to_text() {
				let value: Value = serde_json::from_str(text).unwrap();
				if predicate(&value) {
					return value;
				}
			}
		}
	})
	.await
	.expect("Expected host event timed out")
}
fn event(kind: &str, action: &str, id: &str, settings: Value) -> Value {
	json!({"event":kind,"action":format!("com.cooldead.mpris2.{action}"),"context":id,"device":"test","payload":{"settings":settings,"controller":"Keypad","coordinates":{"row":0,"column":0},"isInMultiAction":false,"state":0}})
}

#[tokio::test]
#[ignore = "Requires local sockets; run under dbus-run-session"]
async fn global_selection_artwork_and_playback_states() {
	let first = Arc::new(Mutex::new(Data {
		playing: true,
		artwork: format!(
			"file://{}/assets/icons/playpause.png",
			env!("CARGO_MANIFEST_DIR")
		),
		..Default::default()
	}));
	let second = Arc::new(Mutex::new(Data::default()));
	let _first = serve("org.mpris.MediaPlayer2.testfirst", first.clone()).await;
	let _second = serve("org.mpris.MediaPlayer2.testsecond", second.clone()).await;
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let port = listener.local_addr().unwrap().port().to_string();
	let _child = Child(
		Command::new(env!("CARGO_BIN_EXE_opendeck-mpris2"))
			.args([
				"-port",
				&port,
				"-pluginUUID",
				"test-plugin",
				"-registerEvent",
				"registerPlugin",
				"-info",
				r#"{"devices":[]}"#,
			])
			.spawn()
			.unwrap(),
	);
	let (stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
		.await
		.unwrap()
		.unwrap();
	let mut ws = accept_async(stream).await.unwrap();
	until(&mut ws, |v| v["event"] == "registerPlugin").await;
	until(&mut ws, |v| v["event"] == "getGlobalSettings").await;
	send(
		&mut ws,
		json!({"event":"didReceiveGlobalSettings","payload":{"settings":{"player":"testfirst"}}}),
	)
	.await;
	// Old per-button player values must not override restored global selection.
	send(
		&mut ws,
		event(
			"willAppear",
			"playpause",
			"play",
			json!({"player":"testsecond"}),
		),
	)
	.await;
	send(&mut ws, event("willAppear", "artwork", "art", json!({}))).await;
	send(
		&mut ws,
		event("willAppear", "next", "next", json!({"player":"testsecond"})),
	)
	.await;
	let image = until(&mut ws, |v| {
		v["event"] == "setImage" && v["context"] == "art"
	})
	.await;
	assert!(
		image["payload"]["image"]
			.as_str()
			.unwrap()
			.starts_with("data:image/png;base64,")
	);
	// Multiple artwork instances must receive different slices of the same cover.
	send(
		&mut ws,
		event(
			"willAppear",
			"artwork",
			"tile2",
			json!({"artwork_grid":2,"artwork_position":0}),
		),
	)
	.await;
	let tile2 = until(&mut ws, |v| {
		v["event"] == "setImage" && v["context"] == "tile2"
	})
	.await;
	send(
		&mut ws,
		event(
			"willAppear",
			"artwork",
			"tile3",
			json!({"artwork_grid":3,"artwork_position":8}),
		),
	)
	.await;
	let tile3 = until(&mut ws, |v| {
		v["event"] == "setImage" && v["context"] == "tile3"
	})
	.await;
	assert_ne!(tile2["payload"]["image"], image["payload"]["image"]);
	assert_ne!(tile2["payload"]["image"], tile3["payload"]["image"]);
	send(
		&mut ws,
		event(
			"didReceiveSettings",
			"artwork",
			"tile2",
			json!({"artwork_grid":2,"artwork_position":3}),
		),
	)
	.await;
	let moved = until(&mut ws, |v| {
		v["event"] == "setImage" && v["context"] == "tile2"
	})
	.await;
	assert_ne!(moved["payload"]["image"], tile2["payload"]["image"]);
	send(
		&mut ws,
		event("keyUp", "next", "next", json!({"player":"testsecond"})),
	)
	.await;
	send(&mut ws, event("keyUp", "playpause", "play", json!({}))).await;
	until(&mut ws, |v| {
		v["event"] == "setTitle" && v["context"] == "play" && v["payload"]["title"] == ""
	})
	.await;
	until(&mut ws, |v| {
		v["event"] == "setState" && v["context"] == "play" && v["payload"]["state"] == 0
	})
	.await;
	let play_image = until(&mut ws, |v| {
		v["event"] == "setImage" && v["context"] == "play"
	})
	.await;
	use base64::Engine;
	assert!(play_image["payload"]["state"].is_null());
	assert_eq!(
		play_image["payload"]["image"],
		format!(
			"data:image/png;base64,{}",
			base64::engine::general_purpose::STANDARD
				.encode(include_bytes!("../assets/icons/play.png"))
		)
	);
	assert!(!first.lock().unwrap().playing);
	assert_eq!(first.lock().unwrap().next, 1);
	assert_eq!(second.lock().unwrap().next, 0);
	send(&mut ws,json!({"event":"sendToPlugin","action":"com.cooldead.mpris2.artwork","context":"art","payload":{"command":"selectPlayer","player":"testsecond"}})).await;
	let persisted = until(&mut ws, |v| v["event"] == "setGlobalSettings").await;
	assert_eq!(persisted["payload"]["player"], "testsecond");
	let dropdown = until(&mut ws, |v| v["event"] == "sendToPropertyInspector").await;
	assert_eq!(dropdown["payload"]["selected"], "testsecond");
	assert_eq!(dropdown["payload"]["players"].as_array().unwrap().len(), 2);
	until(&mut ws, |v| {
		v["event"] == "setImage" && v["context"] == "art" && v["payload"]["image"].is_null()
	})
	.await;
	send(
		&mut ws,
		event("keyUp", "next", "next", json!({"player":"testfirst"})),
	)
	.await;
	send(&mut ws, event("keyUp", "playpause", "play", json!({}))).await;
	until(&mut ws, |v| {
		v["event"] == "setState" && v["context"] == "play" && v["payload"]["state"] == 1
	})
	.await;
	let pause_image = until(&mut ws, |v| {
		v["event"] == "setImage" && v["context"] == "play"
	})
	.await;
	assert!(pause_image["payload"]["state"].is_null());
	assert_eq!(
		pause_image["payload"]["image"],
		format!(
			"data:image/png;base64,{}",
			base64::engine::general_purpose::STANDARD
				.encode(include_bytes!("../assets/icons/pause.png"))
		)
	);
	assert_eq!(second.lock().unwrap().next, 1);
	send(
		&mut ws,
		event(
			"didReceiveSettings",
			"playpause",
			"play",
			json!({"show_status":true}),
		),
	)
	.await;
	until(&mut ws, |v| {
		v["event"] == "setTitle" && v["context"] == "play" && v["payload"]["title"] == "Playing"
	})
	.await;
	send(
		&mut ws,
		event(
			"didReceiveSettings",
			"playpause",
			"play",
			json!({"show_status":false}),
		),
	)
	.await;
	until(&mut ws, |v| {
		v["event"] == "setTitle" && v["context"] == "play" && v["payload"]["title"] == ""
	})
	.await;
	send(
		&mut ws,
		json!({"event":"didReceiveGlobalSettings","payload":{"settings":{"player":"missing"}}}),
	)
	.await;
	send(&mut ws, event("keyUp", "playpause", "play", json!({}))).await;
	until(&mut ws, |v| v["event"] == "showAlert").await;
	ws.close(None).await.unwrap();
}
