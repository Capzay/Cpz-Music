//! Playback end to end against a stand-in server, with no sound card: the null
//! output reads samples at the speed a real device would.

use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use cpz_core::api::Api;
use cpz_core::audio::{Engine, EngineEvent, Media, Output, Remote};
use cpz_core::auth::{Credentials, Session, SupabaseAuth};
use cpz_core::downloads::Downloads;
use cpz_core::listens::Listens;
use cpz_core::player::{Command, Player, PlayerDeps, PlayerState};
use cpz_core::store::Store;
use cpz_core::types::{ClientConfig, PlayerTrack, RepeatMode, TrackAlbum, TrackArtist};
use tokio::net::TcpListener;

/// A mono 8-bit WAV of `secs` seconds whose samples are a recognisable ramp.
fn wav(secs: f32) -> Vec<u8> {
    let rate: u32 = 8000;
    let data: Vec<u8> = (0..(rate as f32 * secs) as usize)
        .map(|i| (i % 251) as u8)
        .collect();
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&8u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

#[derive(Default)]
struct Fake {
    files: Mutex<Vec<Vec<u8>>>,
    /// Sleep this long between 16 KiB chunks, to make the download lag.
    throttle: Mutex<Duration>,
    requests: AtomicUsize,
    range_starts: Mutex<Vec<u64>>,
    listens: Mutex<Vec<serde_json::Value>>,
    now_playing: AtomicUsize,
}

async fn stream(
    State(fake): State<Arc<Fake>>,
    Path(id): Path<usize>,
    headers: HeaderMap,
) -> Response {
    fake.requests.fetch_add(1, Ordering::SeqCst);
    let Some(file) = fake.files.lock().unwrap().get(id).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let len = file.len() as u64;
    let start: u64 = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.trim_end_matches('-').parse().ok())
        .unwrap_or(0);
    fake.range_starts.lock().unwrap().push(start);

    let throttle = *fake.throttle.lock().unwrap();
    let chunks: Vec<Vec<u8>> = file[start as usize..]
        .chunks(16 * 1024)
        .map(<[u8]>::to_vec)
        .collect();
    let body = Body::from_stream(futures_util::stream::unfold(
        chunks.into_iter(),
        move |mut rest| async move {
            let chunk = rest.next()?;
            if !throttle.is_zero() {
                tokio::time::sleep(throttle).await;
            }
            Some((Ok::<_, std::io::Error>(chunk), rest))
        },
    ));
    Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header(header::CONTENT_TYPE, "audio/wav")
        .header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{}/{len}", len - 1),
        )
        .header(header::CONTENT_LENGTH, len - start)
        .body(body)
        .unwrap()
}

async fn listen(State(fake): State<Arc<Fake>>, body: axum::Json<serde_json::Value>) -> StatusCode {
    fake.listens.lock().unwrap().push(body.0);
    StatusCode::NO_CONTENT
}

async fn now_playing(State(fake): State<Arc<Fake>>) -> StatusCode {
    fake.now_playing.fetch_add(1, Ordering::SeqCst);
    StatusCode::NO_CONTENT
}

async fn serve(files: Vec<Vec<u8>>) -> (Arc<Fake>, Api) {
    let fake = Arc::new(Fake::default());
    *fake.files.lock().unwrap() = files;
    let router = Router::new()
        .route("/api/tracks/{id}/stream", get(stream))
        .route("/api/stats/listen", post(listen))
        .route("/api/now-playing", post(now_playing))
        .with_state(fake.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let http = cpz_core::http::client();
    let config = ClientConfig {
        supabase_url: "http://127.0.0.1:9".into(),
        anon_key: "anon".into(),
    };
    let session = Session {
        access_token: "token".into(),
        refresh_token: "refresh".into(),
        expires_at: i64::MAX / 2,
    };
    let credentials = Credentials::new(
        SupabaseAuth::new(http.clone(), config),
        Some(session),
        Box::new(|_| {}),
    );
    (fake, Api::new(http, &url, credentials))
}

fn track(id: i32) -> PlayerTrack {
    PlayerTrack {
        id,
        title: format!("Track {id}"),
        duration: Some(1.0),
        track_number: Some(id),
        disc_number: 1,
        artist: TrackArtist {
            id: 1,
            name: "A".into(),
        },
        album: TrackAlbum {
            id: 1,
            title: "B".into(),
            has_artwork: false,
        },
    }
}

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

async fn wait_until(
    player: &Player,
    secs: u64,
    check: impl Fn(&PlayerState) -> bool,
) -> PlayerState {
    let mut watch = player.subscribe();
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
            panic!("timed out; last state: {state:?}");
        }
    }
}

fn deps(api: &Api, store: &Arc<Store>, dir: &std::path::Path) -> PlayerDeps {
    PlayerDeps {
        api: api.clone(),
        store: store.clone(),
        downloads: Arc::new(Downloads::new(store.clone(), dir.join("downloads"))),
        listens: Arc::new(Listens::new(store.clone())),
        cache_dir: dir.join("cache"),
        output: Output::Null,
    }
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
