//! Playback end to end against a stand-in server, with no sound card: the null
//! output reads samples at the speed a real device would.

mod common;

use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use common::{deps, serve, track, wait_until, wav};
use cpz_core::audio::{Engine, EngineEvent, Media, Output, Remote};
use cpz_core::player::{Command, Player};
use cpz_core::store::Store;
use cpz_core::types::RepeatMode;

fn wait_for_event(events: &std::sync::mpsc::Receiver<EngineEvent>, secs: u64) -> EngineEvent {
    events
        .recv_timeout(Duration::from_secs(secs))
        .expect("an engine event in time")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_remote_track_reads_back_exactly_including_after_a_far_seek() {
    let file = wav(200.0); // 1.6 MB, well past the jump-ahead distance
    let (fake, api) = serve(vec![file.clone()]).await;
    *fake.throttle.lock().unwrap() = Duration::from_millis(10);
    let dir = tempfile::tempdir().unwrap();

    let remote = Remote::open(api, 0, dir.path()).await.unwrap();
    assert_eq!(remote.len(), file.len() as u64);
    assert_eq!(remote.mime(), Some("audio/wav"));

    let bytes = tokio::task::spawn_blocking(move || {
        let mut remote = remote;
        let mut head = vec![0u8; 1000];
        remote.read_exact(&mut head).unwrap();

        // Far beyond what a throttled download has reached: the download jumps.
        let far = file.len() as u64 - 2000;
        remote.seek(SeekFrom::Start(far)).unwrap();
        let mut tail = Vec::new();
        remote.read_to_end(&mut tail).unwrap();

        remote.seek(SeekFrom::Start(0)).unwrap();
        let mut all = Vec::new();
        remote.read_to_end(&mut all).unwrap();
        (head, tail, all, file)
    })
    .await
    .unwrap();

    let (head, tail, all, file) = bytes;
    assert_eq!(head, file[..1000]);
    assert_eq!(tail, file[file.len() - 2000..]);
    assert_eq!(all, file);
    let starts = fake.range_starts.lock().unwrap().clone();
    assert_eq!(starts[0], 0);
    assert!(
        starts.len() >= 2,
        "the far seek should have started a second request: {starts:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_engine_plays_files_and_streams_to_the_end() {
    let (_fake, api) = serve(vec![wav(0.6)]).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("local.wav");
    std::fs::write(&path, wav(0.4)).unwrap();

    let (engine, events) = Engine::start(api, dir.path().join("cache"), Output::Null);

    let generation = engine.load(Media::File(path), 0.0, true);
    match wait_for_event(&events, 5) {
        EngineEvent::Loaded {
            generation: g,
            duration,
        } => {
            assert_eq!(g, generation);
            assert!((duration.unwrap() - 0.4).abs() < 0.05, "{duration:?}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        wait_for_event(&events, 5),
        EngineEvent::Ended { generation }
    );

    let generation = engine.load(Media::Stream(0), 0.0, true);
    assert!(
        matches!(wait_for_event(&events, 5), EngineEvent::Loaded { generation: g, .. } if g == generation)
    );
    let started = Instant::now();
    assert_eq!(
        wait_for_event(&events, 5),
        EngineEvent::Ended { generation }
    );
    // Played in real time, not skipped through.
    assert!(
        started.elapsed() > Duration::from_millis(350),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_engine_pauses_seeks_and_reports_failures() {
    let (_fake, api) = serve(vec![wav(3.0)]).await;
    let dir = tempfile::tempdir().unwrap();
    let (engine, events) = Engine::start(api, dir.path().to_owned(), Output::Null);

    let generation = engine.load(Media::Stream(0), 0.0, false);
    assert!(matches!(
        wait_for_event(&events, 5),
        EngineEvent::Loaded { .. }
    ));
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        engine.status().position < 0.05,
        "paused, so nothing should have played"
    );

    engine.seek(2.5);
    engine.play();
    std::thread::sleep(Duration::from_millis(300));
    let status = engine.status();
    assert!(status.playing);
    assert!(status.position >= 2.5, "{status:?}");
    assert_eq!(
        wait_for_event(&events, 5),
        EngineEvent::Ended { generation }
    );

    let missing = engine.load(Media::Stream(99), 0.0, true);
    assert!(
        matches!(wait_for_event(&events, 5), EngineEvent::Failed { generation, .. } if generation == missing)
    );

    // A load superseded before it opens never reports.
    let stale = engine.load(Media::Stream(0), 0.0, true);
    let fresh = engine.load(Media::Stream(0), 0.0, true);
    match wait_for_event(&events, 5) {
        EngineEvent::Loaded { generation, .. } => {
            assert_eq!(generation, fresh);
            assert_ne!(generation, stale);
        }
        other => panic!("{other:?}"),
    }
    engine.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_player_works_through_a_queue_and_records_listens() {
    let (fake, api) = serve(vec![wav(0.5), wav(0.5), wav(0.5)]).await;
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::in_memory().unwrap());
    let player = Player::start(deps(&api, &store, dir.path()));

    player.send(Command::SetQueue {
        tracks: vec![track(0), track(1), track(2)],
        start: 1,
    });
    let state = wait_until(&player, 5, |s| s.playing && !s.loading).await;
    assert_eq!(state.current().unwrap().id, 1);
    assert!(!state.local);

    let end = wait_until(&player, 6, |s| !s.playing).await;
    assert_eq!(
        end.queue.index, 2,
        "played track 1 then 2, then stopped at the end"
    );
    assert!(end.error.is_none());

    // Both finished tracks count, however short.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let ids: Vec<i64> = fake
        .listens
        .lock()
        .unwrap()
        .iter()
        .map(|l| l["trackId"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, vec![1, 2]);
    assert!(fake.now_playing.load(Ordering::SeqCst) > 0);

    // Play after the end starts the last track again.
    player.send(Command::Toggle);
    wait_until(&player, 5, |s| s.playing && !s.loading).await;
    player.send(Command::Pause);
    assert!(!wait_until(&player, 2, |s| !s.playing).await.playing);
}

#[tokio::test(flavor = "multi_thread")]
async fn repeat_one_and_repeat_all_keep_playing_a_single_track() {
    let (fake, api) = serve(vec![wav(0.3)]).await;
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::in_memory().unwrap());
    let player = Player::start(deps(&api, &store, dir.path()));

    player.send(Command::CycleRepeat);
    assert_eq!(
        wait_until(&player, 2, |s| s.queue.repeat == RepeatMode::All)
            .await
            .queue
            .repeat,
        RepeatMode::All
    );
    player.send(Command::SetQueue {
        tracks: vec![track(0)],
        start: 0,
    });

    let deadline = Instant::now() + Duration::from_secs(6);
    while fake.listens.lock().unwrap().len() < 3 {
        assert!(Instant::now() < deadline, "the single track should loop");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(player.state().playing);
}

#[tokio::test(flavor = "multi_thread")]
async fn downloaded_tracks_play_from_disk_and_the_queue_survives_a_restart() {
    let (fake, api) = serve(vec![wav(2.0)]).await;
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::in_memory().unwrap());
    let deps = deps(&api, &store, dir.path());
    deps.downloads.download(&api, &track(0)).await.unwrap();
    let requests = fake.requests.load(Ordering::SeqCst);

    {
        let player = Player::start(deps);
        player.send(Command::SetVolume { volume: 0.4 });
        player.send(Command::Play {
            track: Some(track(0)),
        });
        let state = wait_until(&player, 5, |s| s.playing && !s.loading && s.position > 0.3).await;
        assert!(state.local);
        assert!((state.duration.unwrap() - 2.0).abs() < 0.05);
        player.send(Command::Pause);
        wait_until(&player, 2, |s| !s.playing).await;
    }
    assert_eq!(
        fake.requests.load(Ordering::SeqCst),
        requests,
        "a download never touches the network"
    );

    // The player thread saves on the way out; give it a moment.
    tokio::time::sleep(Duration::from_millis(400)).await;
    let reopened = Player::start(self::deps(&api, &store, dir.path()));
    let state = wait_until(&reopened, 5, |s| !s.queue.tracks.is_empty() && !s.loading).await;
    assert_eq!(state.current().unwrap().id, 0);
    assert!(!state.playing, "reopens paused");
    assert!(state.position > 0.2, "{state:?}");
    assert!((state.volume - 0.4).abs() < f32::EPSILON);
}
