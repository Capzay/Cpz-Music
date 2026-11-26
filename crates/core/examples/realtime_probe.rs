//! Joins the real player channel and prints what happens, to check the protocol
//! against Supabase itself. With only the anon key the join should be refused;
//! with an owner's access token it should join and list devices.
//!
//!     SUPABASE_URL=... SUPABASE_ANON_KEY=... [ACCESS_TOKEN=...] \
//!         cargo run -p cpz-core --example realtime_probe

use std::time::Duration;

use cpz_core::auth::{Credentials, Session, SupabaseAuth};
use cpz_core::realtime::{Channel, ChannelConfig, ChannelEvent, PLAYER_TOPIC};
use cpz_core::types::ClientConfig;

#[tokio::main]
async fn main() {
    let config = ClientConfig {
        supabase_url: std::env::var("SUPABASE_URL").expect("SUPABASE_URL"),
        anon_key: std::env::var("SUPABASE_ANON_KEY").expect("SUPABASE_ANON_KEY"),
    };
    let token = std::env::var("ACCESS_TOKEN").unwrap_or_else(|_| config.anon_key.clone());
    let session = Session {
        access_token: token,
        refresh_token: String::new(),
        expires_at: i64::MAX / 2,
    };
    let http = cpz_core::http::client();
    let credentials = Credentials::new(
        SupabaseAuth::new(http, config.clone()),
        Some(session),
        Box::new(|_| {}),
    );

    let (channel, mut events) = Channel::spawn(
        ChannelConfig {
            supabase: config,
            topic: PLAYER_TOPIC.into(),
            presence_key: "probe".into(),
        },
        credentials,
    );
    channel.track(serde_json::json!({ "deviceId": "probe", "name": "Probe", "platform": "CLI", "claimedAt": 0 }));

    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    while let Ok(Some(event)) = tokio::time::timeout_at(deadline, events.recv()).await {
        match event {
            ChannelEvent::Disconnected { reason } => {
                println!("disconnected: {reason}");
                break;
            }
            other => println!("{other:?}"),
        }
    }
}
