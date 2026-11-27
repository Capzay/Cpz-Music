//! The native apps' client against the real server: if a response shape drifts
//! on either side, this is where it shows.

#[allow(dead_code)]
mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use cpz_core::api::{Api, fetch_config};
use cpz_core::auth::{Credentials, Session, SupabaseAuth};
use cpz_core::downloads::Downloads;
use cpz_core::listens::Listens;
use cpz_core::playlists::Playlists;
use cpz_core::store::Store;
use cpz_core::types::{ClientConfig, StatsPeriod};
use support::{Harness, write_wav};

fn session_of(value: &serde_json::Value, expires_at: i64) -> Session {
    Session {
        access_token: value["access_token"].as_str().unwrap().into(),
        refresh_token: value["refresh_token"].as_str().unwrap().into(),
        expires_at,
    }
}

async fn client(h: &Harness, expires_at: Option<i64>) -> Api {
    let http = cpz_core::http::client();
    let config = fetch_config(&http, &h.url).await.unwrap();
    assert_eq!(
        config,
        ClientConfig {
            supabase_url: h.state.config.supabase_url.clone(),
            anon_key: "anon".into(),
        }
    );
    let raw = h.mock.session(&h.owner_id, 3600);
    let expires_at = expires_at.unwrap_or_else(|| raw["expires_at"].as_i64().unwrap());
    let credentials = Credentials::new(
        SupabaseAuth::new(http.clone(), config),
        Some(session_of(&raw, expires_at)),
        Box::new(|_| {}),
    );
    Api::new(http, &h.url, credentials)
}

#[tokio::test]
async fn the_native_client_reads_and_writes_everything_it_needs() {
    let Some(h) = Harness::start().await else {
        return;
    };
    write_wav(&h.music.join("Artist One/Second Album/CD1/01 Birch.wav"));
    write_wav(&h.music.join("Artist One/Second Album/CD2/01 Amber.wav"));
    write_wav(&h.music.join("Artist Two/First Album/Cedar.wav"));
    h.scan().await;

    let api = client(&h, None).await;
    api.claim_owner().await.unwrap();

    let albums = api.albums().await.unwrap();
    assert_eq!(albums.len(), 2);
    let wall = albums
        .iter()
        .find(|album| album.title == "Second Album")
        .unwrap();
    let detail = api.album(wall.id).await.unwrap();
    assert_eq!(detail.artist.name, "Artist One");
    assert_eq!(detail.tracks.len(), 2);

    let artists = api.artists().await.unwrap();
    assert_eq!(artists.len(), 2);
    let floyd = artists
        .iter()
        .find(|artist| artist.name == "Artist One")
        .unwrap();
    assert_eq!(api.artist(floyd.id).await.unwrap().tracks.len(), 2);
    assert_eq!(api.tracks().await.unwrap().len(), 3);

    let found = api.search("amber").await.unwrap();
    assert_eq!(found.tracks[0].title, "01 Amber");
    assert!(api.search("").await.unwrap().tracks.is_empty());
    let track = found.tracks[0].clone();

    // Listens, including a skip that must not count.
    let store = Arc::new(Store::in_memory().unwrap());
    let listens = Listens::new(store.clone());
    listens.report(&api, track.id, 5.0, false).await;
    listens.report(&api, track.id, 95.0, false).await;
    let stats = api.stats(StatsPeriod::All).await.unwrap();
    assert_eq!(stats.totals.play_count, 1);
    assert_eq!(stats.top_tracks[0].id, track.id);
    assert_eq!(listens.pending(), 0);

    // A listen queued while offline is replayed later.
    store
        .queue_listen(track.id, 40, cpz_core::http::now_ms())
        .unwrap();
    listens.flush(&api).await;
    assert_eq!(listens.pending(), 0);
    assert_eq!(
        api.stats(StatsPeriod::Week)
            .await
            .unwrap()
            .totals
            .play_count,
        2
    );

    // Playlists made on the device reach the server and come back with an id.
    let playlists = Playlists::load(store.clone());
    let uuid = playlists.create("Road trip").unwrap();
    playlists.add_tracks(&uuid, &detail.tracks);
    playlists.sync(&api).await.unwrap();
    let synced = playlists.find(&uuid).unwrap();
    assert!(!synced.dirty);
    assert!(synced.id.is_some());
    assert_eq!(synced.tracks.len(), 2);

    playlists.rename(&uuid, "Night drive");
    playlists.sync(&api).await.unwrap();
    assert_eq!(api.playlists().await.unwrap()[0].name, "Night drive");

    playlists.delete(&uuid);
    playlists.sync(&api).await.unwrap();
    assert!(api.playlists().await.unwrap().is_empty());
    assert!(playlists.list().is_empty());

    // Downloads land on disk whole, at the size the server sent.
    let dir = tempfile::tempdir().unwrap();
    let downloads = Downloads::new(store, dir.path().to_owned());
    assert!(
        downloads
            .download_all(&api, &detail.tracks)
            .await
            .is_empty()
    );
    let file = downloads.local_file(track.id).unwrap();
    assert_eq!(
        std::fs::metadata(&file).unwrap().len(),
        downloads.list()[0].bytes
    );
    assert_eq!(downloads.list().len(), 2);
    downloads.remove(track.id);
    assert!(!file.exists());
    assert!(!downloads.is_downloaded(track.id));

    let settings = api.settings().await.unwrap();
    assert!(settings.overlay_url.unwrap().contains("/obs?token="));
    assert_eq!(settings.jam, None);
    api.jam_action("/api/jam/start", &serde_json::json!({ "hostName": "DJ" }))
        .await
        .unwrap();
    let settings = api.settings().await.unwrap();
    assert_eq!(settings.jam.unwrap().host_name.as_deref(), Some("DJ"));
    assert!(settings.invite_url.unwrap().contains("/jam/join?token="));
}

#[tokio::test]
async fn a_stale_token_is_refreshed_before_it_is_used() {
    let Some(h) = Harness::start().await else {
        return;
    };
    let api = client(&h, Some(0)).await;
    assert!(api.albums().await.unwrap().is_empty());
    assert_eq!(h.mock.refreshes.load(Ordering::SeqCst), 1);
    assert!(api.artists().await.unwrap().is_empty());
    assert_eq!(h.mock.refreshes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_rejected_refresh_signs_the_device_out() {
    let Some(h) = Harness::start().await else {
        return;
    };
    let http = cpz_core::http::client();
    let config = fetch_config(&http, &h.url).await.unwrap();
    let signed_out = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = signed_out.clone();
    let credentials = Credentials::new(
        SupabaseAuth::new(http.clone(), config),
        Some(Session {
            access_token: "old".into(),
            refresh_token: "spent".into(),
            expires_at: 0,
        }),
        Box::new(move |session| flag.store(session.is_none(), Ordering::SeqCst)),
    );
    let api = Api::new(http, &h.url, credentials.clone());
    assert!(matches!(
        api.albums().await,
        Err(cpz_core::api::ApiError::SignedOut)
    ));
    assert!(signed_out.load(Ordering::SeqCst));
    assert!(!credentials.is_signed_in().await);
}
