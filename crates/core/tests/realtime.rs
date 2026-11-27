//! Several devices sharing one player over a stand-in for Supabase Realtime.

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::{Fake, client, deps, serve, track, wav};
use cpz_core::api::Api;
use cpz_core::player::{Command, Player};
use cpz_core::realtime::{Channel, ChannelConfig, Hub, HubState, Identity, PLAYER_TOPIC};
use cpz_core::store::Store;
use cpz_core::types::ClientConfig;
use serde_json::json;

struct Device {
    hub: Hub,
    _dir: tempfile::TempDir,
}

fn device(api: &Api, id: &str) -> Device {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::in_memory().unwrap());
    let player = Player::start(deps(api, &store, dir.path()));
    let config = ChannelConfig {
        supabase: ClientConfig {
            supabase_url: api.server().to_owned(),
            anon_key: "anon".into(),
            public_url: None,
        },
        topic: PLAYER_TOPIC.into(),
        presence_key: id.into(),
    };
    let channel = Channel::spawn(config, api.credentials().clone());
    let identity = Identity {
        id: id.into(),
        name: format!("Device {id}"),
        platform: "Linux".into(),
    };
    Device {
        hub: Hub::start(player, Some(channel), identity, store),
        _dir: dir,
    }
}

async fn wait(
    device: &Device,
    secs: u64,
    what: &str,
    check: impl Fn(&HubState) -> bool,
) -> HubState {
    let mut watch = device.hub.subscribe();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let state = watch.borrow_and_update().clone();
        if check(&state) {
            return state;
        }
        if tokio::time::timeout_at(deadline, watch.changed())
            .await
            .is_err()
        {
            panic!("timed out waiting for {what}; last state: {state:#?}");
        }
    }
}

fn current(state: &HubState) -> Option<i32> {
    state.player.current().map(|t| t.id)
}

async fn long_tracks() -> (Arc<Fake>, Api) {
    serve(vec![wav(30.0), wav(30.0), wav(30.0)]).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_remote_drives_the_active_device_then_takes_over() {
    let (fake, api) = long_tracks().await;
    let a = device(&api, "a");
    wait(&a, 5, "a alone", |s| s.connected && s.devices.len() == 1).await;
    let b = device(&api, "b");
    for d in [&a, &b] {
        wait(d, 5, "both devices", |s| s.devices.len() == 2).await;
    }

    // Nobody has claimed, so the lowest id plays, and both agree on that.
    assert!(a.hub.state().is_active);
    assert!(!b.hub.state().is_active);
    assert_eq!(b.hub.state().active_id.as_deref(), Some("a"));

    // Commands from the remote play on the active device, and the remote mirrors.
    b.hub.send(Command::SetQueue {
        tracks: vec![track(0), track(1), track(2)],
        start: 0,
    });
    wait(&a, 5, "a playing track 0", |s| {
        s.player.playing && !s.player.loading && current(s) == Some(0)
    })
    .await;
    wait(&b, 5, "b mirroring", |s| {
        s.player.playing && current(s) == Some(0) && s.player.queue.tracks.len() == 3
    })
    .await;

    b.hub.send(Command::Next);
    wait(&a, 5, "a on track 1", |s| {
        current(s) == Some(1) && !s.player.loading
    })
    .await;
    wait(&b, 5, "b mirroring track 1", |s| current(s) == Some(1)).await;
    let mirrored = wait(&b, 5, "b's clock running", |s| s.player.position > 0.8).await;
    assert!(!mirrored.player.local);

    // Claiming moves playback here, carrying on from where the other device was.
    b.hub.claim();
    let taken = wait(&b, 5, "b playing", |s| {
        s.is_active && s.player.playing && !s.player.loading && current(s) == Some(1)
    })
    .await;
    assert!(
        taken.player.position > 0.5,
        "picked up mid-track: {}",
        taken.player.position
    );
    wait(&a, 5, "a a remote now", |s| {
        !s.is_active && s.active_id.as_deref() == Some("b")
    })
    .await;

    // A guest's pick, pushed by the server, plays next on whichever device plays.
    fake.realtime
        .server_broadcast("jam-add", json!({ "track": track(2) }));
    wait(&b, 5, "jam pick queued", |s| {
        s.player.queue.tracks.len() == 4 && s.player.queue.tracks[2].id == 2
    })
    .await;
    wait(&a, 5, "a mirroring the jam pick", |s| {
        s.player.queue.tracks.len() == 4
    })
    .await;
    fake.realtime.server_broadcast("jam", json!({}));
    wait(&a, 5, "jam bump on a", |s| s.jam_version == 1).await;
    wait(&b, 5, "jam bump on b", |s| s.jam_version == 1).await;

    // The playing device going away hands playback back without anyone asking.
    drop(b);
    let resumed = wait(&a, 8, "a took over", |s| {
        s.is_active && s.player.playing && !s.player.loading && current(s) == Some(1)
    })
    .await;
    assert_eq!(resumed.devices.len(), 1);
    assert_eq!(resumed.player.queue.tracks.len(), 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn opening_the_app_elsewhere_does_not_steal_playback() {
    let (_fake, api) = long_tracks().await;
    let a = device(&api, "m");
    wait(&a, 5, "m connected", |s| s.connected).await;
    a.hub.send(Command::SetQueue {
        tracks: vec![track(0)],
        start: 0,
    });
    wait(&a, 5, "m playing", |s| {
        s.player.playing && !s.player.loading
    })
    .await;

    // Sorts before "m" and has never claimed: it must not win on the tie-break.
    let newcomer = device(&api, "0-new");
    let seen = wait(&newcomer, 5, "newcomer sees m", |s| {
        s.devices.len() == 2 && s.active_id.is_some()
    })
    .await;
    assert_eq!(seen.active_id.as_deref(), Some("m"));
    assert!(!seen.is_active);
    assert!(a.hub.state().is_active);
    assert!(a.hub.state().player.playing);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_channel_still_plays_on_its_own() {
    let (fake, api) = long_tracks().await;
    let outsider = client(api.server(), "not-the-token");
    let lone = device(&outsider, "lone");
    while fake.realtime.refused.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let state = lone.hub.state();
    assert!(!state.connected);
    assert!(state.is_active);

    // Commands apply locally rather than going nowhere.
    lone.hub.send(Command::SetVolume { volume: 0.5 });
    wait(&lone, 2, "volume applied", |s| {
        (s.player.volume - 0.5).abs() < f32::EPSILON
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn devices_find_each_other_again_after_a_dropped_connection() {
    let (fake, api) = long_tracks().await;
    let a = device(&api, "a");
    let b = device(&api, "b");
    for d in [&a, &b] {
        wait(d, 5, "both devices", |s| s.devices.len() == 2).await;
    }
    a.hub.send(Command::SetQueue {
        tracks: vec![track(0)],
        start: 0,
    });
    wait(&b, 5, "b mirroring", |s| s.player.playing).await;

    fake.realtime.disconnect("b");
    // Cut off, the remote plays for itself but does not start making noise: the
    // other device is still playing.
    let alone = wait(&b, 5, "b on its own", |s| !s.connected).await;
    assert!(alone.is_active);
    assert!(a.hub.state().is_active);
    let quiet = wait(&b, 5, "b keeps the queue", |s| {
        current(s) == Some(0) && !s.player.loading
    })
    .await;
    assert!(!quiet.player.playing);

    for d in [&a, &b] {
        wait(d, 8, "back together", |s| {
            s.connected && s.devices.len() == 2
        })
        .await;
    }
    // a claimed on its first play, so it is still the one playing.
    let back = wait(&b, 5, "b a remote again", |s| !s.is_active).await;
    assert_eq!(back.active_id.as_deref(), Some("a"));
    assert!(a.hub.state().player.playing);
}
