//! Plays local files through the default sound device, start to finish or for a
//! few seconds each. A quick check that a format decodes on this machine:
//!
//!     cargo run -p cpz-core --example play -- song.flac other.m4a
//!     PLAY_SECS=3 cargo run -p cpz-core --example play -- *.mp3

use std::time::Duration;

use cpz_core::api::Api;
use cpz_core::audio::{Engine, EngineEvent, Media, Output};
use cpz_core::auth::{Credentials, SupabaseAuth};
use cpz_core::types::ClientConfig;

#[tokio::main]
async fn main() {
    let secs: Option<u64> = std::env::var("PLAY_SECS").ok().and_then(|s| s.parse().ok());
    let http = cpz_core::http::client();
    let config = ClientConfig {
        supabase_url: "http://127.0.0.1:9".into(),
        anon_key: String::new(),
    };
    let credentials = Credentials::new(
        SupabaseAuth::new(http.clone(), config),
        None,
        Box::new(|_| {}),
    );
    let api = Api::new(http, "http://127.0.0.1:9", credentials);
    let (engine, events) = Engine::start(api, std::env::temp_dir(), Output::Device);

    for path in std::env::args().skip(1) {
        let generation = engine.load(Media::File(path.clone().into()), 0.0, true);
        let deadline = secs.map(|s| std::time::Instant::now() + Duration::from_secs(s));
        loop {
            match events.recv_timeout(Duration::from_millis(200)) {
                Ok(EngineEvent::Loaded {
                    generation: g,
                    duration,
                }) if g == generation => {
                    println!("{path}: playing, {duration:.1?}s", duration = duration);
                }
                Ok(EngineEvent::Ended { generation: g }) if g == generation => {
                    println!("{path}: ended at {:.1}s", engine.status().position);
                    break;
                }
                Ok(EngineEvent::Failed {
                    generation: g,
                    message,
                }) if g == generation => {
                    println!("{path}: failed: {message}");
                    break;
                }
                _ => {}
            }
            if deadline.is_some_and(|d| std::time::Instant::now() > d) {
                println!("{path}: ok, at {:.1}s", engine.status().position);
                break;
            }
        }
    }
}
