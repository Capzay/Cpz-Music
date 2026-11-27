mod support;

use std::sync::atomic::Ordering;

use futures_util::future::join_all;
use serde_json::{Value, json};
use support::{Harness, returned_cookies, write_wav};

const UUID_A: &str = "11111111-2222-4333-8444-555555555555";
const UUID_B: &str = "aaaaaaaa-2222-4333-8444-555555555555";

macro_rules! harness {
    () => {
        match Harness::start().await {
            Some(harness) => harness,
            None => return,
        }
    };
}

/// Two albums, one split over disc folders, one with a cover beside the tracks.
async fn library(h: &Harness) {
    write_wav(&h.music.join("Artist One/Second Album/CD1/01 Birch.wav"));
    write_wav(&h.music.join("Artist One/Second Album/CD2/01 Amber.wav"));
    write_wav(&h.music.join("Artist Two/First Album/Cedar.wav"));
    std::fs::write(
        h.music.join("Artist Two/First Album/cover.jpg"),
        b"fake jpeg",
    )
    .unwrap();
    std::fs::write(
        h.music.join("Artist Two/First Album/notes.txt"),
        b"not audio",
    )
    .unwrap();
    h.scan().await;
}

async fn track_id(h: &Harness, cookie: &str, title: &str) -> i64 {
    let body = h
        .json(
            &format!("/api/search?q={}", title.replace(' ', "%20")),
            cookie,
        )
        .await;
    body["tracks"][0]["id"].as_i64().unwrap()
}

#[tokio::test]
async fn refuses_anonymous_callers_and_keeps_public_routes_open() {
    let h = harness!();

    let health = h.get("/api/health", "").await;
    assert_eq!(health.status(), 200);
    assert_eq!(health.headers()["x-content-type-options"], "nosniff");
    assert_eq!(health.headers()["x-frame-options"], "DENY");
    assert_eq!(
        health.json::<Value>().await.unwrap(),
        json!({ "status": "ok" })
    );

    for path in [
        "/api/search?q=a",
        "/api/tracks/1/stream",
        "/api/playlists/sync",
        "/api/settings",
    ] {
        let response = h.get(path, "").await;
        assert_eq!(response.status(), 401, "{path}");
        assert_eq!(
            response.json::<Value>().await.unwrap(),
            json!({ "error": "Unauthorized" })
        );
    }

    assert_eq!(h.get("/api/artwork/1", "").await.status(), 404);
    assert_eq!(h.get("/nothing-here", "").await.status(), 404);
}

#[tokio::test]
async fn an_unknown_github_account_is_refused() {
    let h = harness!();
    let id = h.mock.add_user(json!({
        "id": "stranger",
        "identities": [{ "provider": "github", "identity_data": { "provider_id": "1" } }],
    }));
    let cookie = h.cookie_for(h.mock.session(&id, 3600));
    assert_eq!(h.get("/api/search?q=a", &cookie).await.status(), 401);
}

#[tokio::test]
async fn a_forged_cookie_is_refused() {
    let h = harness!();
    let mut session = h.mock.session(&h.owner_id, 3600);
    session["access_token"] = json!("made-up");
    // The owner's user object is in the cookie, and must not be believed.
    let cookie = h.cookie_for(session);
    assert_eq!(h.get("/api/search?q=a", &cookie).await.status(), 401);
}

#[tokio::test]
async fn scans_the_library_and_serves_it() {
    let h = harness!();
    library(&h).await;
    let cookie = h.owner_cookie();

    let tracks = h.json("/api/library/tracks", &cookie).await;
    assert_eq!(tracks["tracks"].as_array().unwrap().len(), 3);
    assert!(!tracks.to_string().contains("filePath"));
    assert!(!tracks.to_string().contains(h.music.to_str().unwrap()));

    let albums = h.json("/api/library/albums", &cookie).await;
    let titles: Vec<_> = albums["albums"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| &a["title"])
        .collect();
    // Both discs folded into one album, named after the folder above them.
    assert_eq!(titles, [&json!("Second Album"), &json!("First Album")]);
    assert_eq!(albums["albums"][0]["artist"], "Artist One");
    assert_eq!(albums["albums"][1]["hasArtwork"], true);

    let artists = h.json("/api/library/artists", &cookie).await;
    assert_eq!(artists["artists"][0]["name"], "Artist One");
    assert_eq!(artists["artists"][0]["tracks"], 2);
    assert_eq!(artists["artists"][0]["albums"], 1);

    let album_id = albums["albums"][0]["id"].as_i64().unwrap();
    let album = h
        .json(&format!("/api/library/albums/{album_id}"), &cookie)
        .await;
    assert_eq!(album["tracks"].as_array().unwrap().len(), 2);
    assert_eq!(album["artist"]["name"], "Artist One");
    assert_eq!(
        h.get("/api/library/albums/999999", &cookie).await.status(),
        404
    );

    let artist_id = artists["artists"][1]["id"].as_i64().unwrap();
    let artist = h
        .json(&format!("/api/library/artists/{artist_id}"), &cookie)
        .await;
    assert_eq!(artist["name"], "Artist Two");
    assert_eq!(artist["albums"].as_array().unwrap().len(), 1);

    let found = h.json("/api/search?q=amb", &cookie).await;
    assert_eq!(found["tracks"][0]["title"], "01 Amber");
    assert_eq!(found["tracks"][0]["album"]["title"], "Second Album");
    assert_eq!(found["tracks"][0]["artist"]["name"], "Artist One");
    assert_eq!(found["tracks"][0]["discNumber"], 1);

    // Wildcards in a search box are literal characters.
    let wild = h.json("/api/search?q=%25", &cookie).await;
    assert_eq!(wild["tracks"], json!([]));

    assert_eq!(
        h.json("/api/search?q=%20", &cookie).await,
        json!({ "tracks": [], "albums": [], "artists": [] })
    );
    assert_eq!(h.get("/api/search?album=0", &cookie).await.status(), 400);
    assert_eq!(h.get("/api/search?artist=x", &cookie).await.status(), 400);
    let by_album = h
        .json(&format!("/api/search?album={album_id}"), &cookie)
        .await;
    assert_eq!(by_album["tracks"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn streams_with_range_support() {
    let h = harness!();
    library(&h).await;
    let cookie = h.owner_cookie();
    let id = track_id(&h, &cookie, "Amber").await;
    let path = format!("/api/tracks/{id}/stream");
    let file = std::fs::read(h.music.join("Artist One/Second Album/CD2/01 Amber.wav")).unwrap();
    let size = file.len();

    let whole = h.get(&path, &cookie).await;
    assert_eq!(whole.status(), 200);
    assert_eq!(whole.headers()["content-type"], "audio/wav");
    assert_eq!(whole.headers()["accept-ranges"], "bytes");
    assert_eq!(whole.headers()["content-length"], size.to_string());
    assert_eq!(
        whole.headers()["cache-control"],
        "private, max-age=31536000, immutable"
    );
    assert!(whole.headers().get("content-encoding").is_none());
    assert_eq!(whole.bytes().await.unwrap(), file);

    let ranged = |range: &'static str| {
        h.http
            .get(h.url(&path))
            .header("cookie", &cookie)
            .header("range", range)
            .send()
    };

    let part = ranged("bytes=4-11").await.unwrap();
    assert_eq!(part.status(), 206);
    assert_eq!(
        part.headers()["content-range"],
        format!("bytes 4-11/{size}")
    );
    assert_eq!(part.headers()["content-length"], "8");
    assert_eq!(part.bytes().await.unwrap(), file[4..12]);

    let tail = ranged("bytes=-10").await.unwrap();
    assert_eq!(tail.status(), 206);
    assert_eq!(tail.bytes().await.unwrap(), file[size - 10..]);

    let beyond = ranged("bytes=99999999-").await.unwrap();
    assert_eq!(beyond.status(), 416);
    assert_eq!(beyond.headers()["content-range"], format!("bytes */{size}"));

    let head = h
        .http
        .head(h.url(&path))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(head.status(), 200);
    assert_eq!(head.headers()["content-length"], size.to_string());

    assert_eq!(h.get("/api/tracks/abc/stream", &cookie).await.status(), 404);
    assert_eq!(
        h.get("/api/tracks/999999/stream", &cookie).await.status(),
        404
    );
}

#[tokio::test]
async fn refuses_to_serve_a_row_pointing_outside_the_library() {
    let h = harness!();
    library(&h).await;
    let cookie = h.owner_cookie();
    let id = track_id(&h, &cookie, "Amber").await;

    sqlx::query(r#"UPDATE "Track" SET "filePath" = '/etc/passwd' WHERE id = $1"#)
        .bind(id as i32)
        .execute(&h.state.db)
        .await
        .unwrap();
    h.state.media.invalidate();

    assert_eq!(
        h.get(&format!("/api/tracks/{id}/stream"), &cookie)
            .await
            .status(),
        404
    );
}

#[tokio::test]
async fn serves_cover_art_without_a_session() {
    let h = harness!();
    library(&h).await;
    let cookie = h.owner_cookie();
    let albums = h.json("/api/library/albums", &cookie).await;
    let id = albums["albums"][1]["id"].as_i64().unwrap();

    let cover = h.get(&format!("/api/artwork/{id}"), "").await;
    assert_eq!(cover.status(), 200);
    assert_eq!(cover.headers()["content-type"], "image/jpeg");
    assert_eq!(cover.headers()["cache-control"], "public, max-age=86400");
    assert_eq!(cover.bytes().await.unwrap().as_ref(), b"fake jpeg");
}

/// A WebP cover in a file named .jpg is saved and served as WebP, and a cover
/// file that goes missing comes back on the next scan even though no track
/// changed.
#[tokio::test]
async fn covers_keep_their_real_type_and_come_back_when_lost() {
    let h = harness!();
    let webp = b"RIFF\x10\x00\x00\x00WEBPVP8 fake";
    write_wav(&h.music.join("Artist/Album/01 One.wav"));
    std::fs::write(h.music.join("Artist/Album/cover.jpg"), webp).unwrap();
    h.scan().await;
    let cookie = h.owner_cookie();
    let id = h.json("/api/library/albums", &cookie).await["albums"][0]["id"]
        .as_i64()
        .unwrap();
    let saved = h.state.config.artwork_dir.join(format!("{id}.webp"));
    assert!(saved.exists());

    let cover = h.get(&format!("/api/artwork/{id}"), "").await;
    assert_eq!(cover.headers()["content-type"], "image/webp");
    assert_eq!(cover.bytes().await.unwrap().as_ref(), webp);

    std::fs::remove_file(&saved).unwrap();
    h.scan().await;
    assert!(saved.exists());
    assert_eq!(h.get(&format!("/api/artwork/{id}"), "").await.status(), 200);
}

#[tokio::test]
async fn a_rescan_touches_only_what_changed() {
    let h = harness!();
    library(&h).await;
    let cookie = h.owner_cookie();
    let id = track_id(&h, &cookie, "Amber").await;

    h.post(
        "/api/stats/listen",
        &cookie,
        json!({ "trackId": id, "durationSecs": 60 }),
    )
    .await;

    // Rows survive a rescan with the same ids, so stats and playlists stay attached.
    h.scan().await;
    assert_eq!(track_id(&h, &cookie, "Amber").await, id);

    std::fs::remove_file(h.music.join("Artist Two/First Album/Cedar.wav")).unwrap();
    h.scan().await;

    let tracks = h.json("/api/library/tracks", &cookie).await;
    assert_eq!(tracks["tracks"].as_array().unwrap().len(), 2);
    let albums = h.json("/api/library/albums", &cookie).await;
    assert_eq!(albums["albums"].as_array().unwrap().len(), 1);
    let artists = h.json("/api/library/artists", &cookie).await;
    assert_eq!(artists["artists"].as_array().unwrap().len(), 1);

    // The orphaned album took its cover with it.
    assert_eq!(
        std::fs::read_dir(&h.state.config.artwork_dir)
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn an_empty_library_never_wipes_the_database() {
    let h = harness!();
    library(&h).await;
    let cookie = h.owner_cookie();

    // What an unmounted disk looks like.
    std::fs::remove_dir_all(&h.music).unwrap();
    std::fs::create_dir_all(&h.music).unwrap();
    h.scan().await;
    let tracks = h.json("/api/library/tracks", &cookie).await;
    assert_eq!(tracks["tracks"].as_array().unwrap().len(), 3);

    // And one that will not open at all.
    std::fs::remove_dir_all(&h.music).unwrap();
    h.scan().await;
    let tracks = h.json("/api/library/tracks", &cookie).await;
    assert_eq!(tracks["tracks"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn the_watcher_follows_the_disk() {
    let h = harness!();
    library(&h).await;
    let cookie = h.owner_cookie();
    tokio::spawn(h.state.scanner().unwrap().watch_library());
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let count = |body: Value| body["tracks"].as_array().unwrap().len();
    let settle = std::time::Duration::from_millis(3000);

    write_wav(&h.music.join("Artist Two/First Album/Dune.wav"));
    tokio::time::sleep(settle).await;
    assert_eq!(count(h.json("/api/library/tracks", &cookie).await), 4);

    // A whole album moved away arrives as one event for the directory.
    let elsewhere = h.music.parent().unwrap().join("moved");
    std::fs::rename(h.music.join("Artist One"), &elsewhere).unwrap();
    tokio::time::sleep(settle).await;
    assert_eq!(count(h.json("/api/library/tracks", &cookie).await), 2);

    // And moved back, with no per-file event at all.
    std::fs::rename(&elsewhere, h.music.join("Artist One")).unwrap();
    tokio::time::sleep(settle).await;
    assert_eq!(count(h.json("/api/library/tracks", &cookie).await), 4);
}

#[tokio::test]
async fn now_playing_reaches_the_overlay() {
    let h = harness!();
    let cookie = h.owner_cookie();

    let track = json!({ "id": 1, "title": "Song", "duration": 100.0 });
    let posted = h
        .post(
            "/api/now-playing",
            &cookie,
            json!({ "track": track, "isPlaying": true, "currentTime": 10 }),
        )
        .await;
    assert_eq!(posted.status(), 204);

    let token = h.state.tokens.sign("obs", json!({}), 60).unwrap();
    let read = h.json(&format!("/api/now-playing?token={token}"), "").await;
    assert_eq!(read["track"]["title"], "Song");
    assert_eq!(read["isPlaying"], true);
    let position = read["currentTime"].as_f64().unwrap();
    assert!((10.0..11.0).contains(&position), "{position}");

    // The clock stops at the end of the track.
    h.post(
        "/api/now-playing",
        &cookie,
        json!({ "track": track, "isPlaying": true, "currentTime": 500 }),
    )
    .await;
    let read = h.json("/api/now-playing", &cookie).await;
    assert_eq!(read["currentTime"], 100.0);

    assert_eq!(h.get("/api/now-playing", "").await.status(), 401);
    assert_eq!(
        h.get("/api/now-playing?token=bad.token", "").await.status(),
        401
    );
    let invite = h.state.tokens.sign("jam-invite", json!({}), 60).unwrap();
    assert_eq!(
        h.get(&format!("/api/now-playing?token={invite}"), "")
            .await
            .status(),
        401
    );
    assert_eq!(
        h.post("/api/now-playing", "", json!({})).await.status(),
        401
    );
}

#[tokio::test]
async fn playlists_sync_with_last_write_wins() {
    let h = harness!();
    library(&h).await;
    let cookie = h.owner_cookie();
    let a = track_id(&h, &cookie, "Amber").await;
    let b = track_id(&h, &cookie, "Cedar").await;

    let sync = |docs: Value| h.post("/api/playlists/sync", &cookie, json!({ "playlists": docs }));

    let created = sync(json!([
        { "uuid": UUID_A, "name": " Mix ", "updatedAt": 1000, "trackIds": [b, 999999, a, b] },
        { "uuid": UUID_B, "name": "Other", "updatedAt": 2000, "trackIds": [] },
    ]))
    .await;
    assert_eq!(created.status(), 200);
    assert_eq!(created.headers()["cache-control"], "no-store");
    let body: Value = created.json().await.unwrap();

    // Newest first; unknown tracks dropped; order and repeats kept.
    assert_eq!(body["playlists"][0]["name"], "Other");
    let mix = &body["playlists"][1];
    assert_eq!(mix["uuid"], UUID_A);
    assert_eq!(mix["name"], "Mix");
    assert_eq!(mix["updatedAt"], 1000);
    let ids: Vec<_> = mix["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, [b, a, b]);

    // An older edit loses.
    let stale =
        sync(json!([{ "uuid": UUID_A, "name": "Old", "updatedAt": 500, "trackIds": [] }])).await;
    let body: Value = stale.json().await.unwrap();
    assert_eq!(body["playlists"][1]["name"], "Mix");

    // A newer one replaces the whole document.
    let newer =
        sync(json!([{ "uuid": UUID_A, "name": "New", "updatedAt": 3000, "trackIds": [a] }])).await;
    let body: Value = newer.json().await.unwrap();
    assert_eq!(body["playlists"][0]["name"], "New");
    assert_eq!(body["playlists"][0]["tracks"].as_array().unwrap().len(), 1);

    // The same doc twice in one batch: the second sees what the first did.
    let twice = sync(json!([
        { "uuid": "bbbbbbbb-2222-4333-8444-555555555555", "name": "One", "updatedAt": 10, "trackIds": [a] },
        { "uuid": "bbbbbbbb-2222-4333-8444-555555555555", "name": "Two", "updatedAt": 20, "trackIds": [b] },
    ]))
    .await;
    assert_eq!(twice.status(), 200);

    let deleted = sync(json!([{ "uuid": UUID_A, "updatedAt": 4000, "deleted": true }])).await;
    let body: Value = deleted.json().await.unwrap();
    assert!(
        body["playlists"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["uuid"] != UUID_A)
    );

    assert_eq!(
        h.post("/api/playlists/sync", &cookie, json!({}))
            .await
            .status(),
        400
    );
    let garbage = h
        .http
        .post(h.url("/api/playlists/sync"))
        .header("cookie", &cookie)
        .body("{")
        .send();
    assert_eq!(garbage.await.unwrap().status(), 400);
    assert_eq!(
        h.json("/api/playlists/sync", &cookie).await["playlists"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn listens_add_up_and_feed_the_stats() {
    let h = harness!();
    library(&h).await;
    let cookie = h.owner_cookie();
    let id = track_id(&h, &cookie, "Amber").await;

    let report = |body: Value| h.post("/api/stats/listen", &cookie, body);

    assert_eq!(
        report(json!({ "trackId": id, "durationSecs": 100 }))
            .await
            .status(),
        204
    );
    assert_eq!(
        report(json!({ "trackId": id, "durationSecs": 99999 }))
            .await
            .status(),
        204
    );

    let stats = h.json("/api/stats?period=week", &cookie).await;
    assert_eq!(stats["period"], "week");
    assert_eq!(stats["totals"]["playCount"], 2);
    // The second report is capped at six hours.
    assert_eq!(stats["totals"]["listenSecs"], 100 + 21600);
    assert_eq!(stats["totals"]["uniqueTracks"], 1);
    assert_eq!(stats["totals"]["uniqueArtists"], 1);
    assert_eq!(stats["topTracks"][0]["title"], "01 Amber");
    assert_eq!(stats["topArtists"][0]["artistName"], "Artist One");

    assert_eq!(
        h.json("/api/stats?period=nonsense", &cookie).await["period"],
        "year"
    );

    // A deleted track is a permanent rejection, so the client stops retrying.
    assert_eq!(
        report(json!({ "trackId": 999999, "durationSecs": 1 }))
            .await
            .status(),
        409
    );
    assert_eq!(
        report(json!({ "trackId": "1", "durationSecs": 1 }))
            .await
            .status(),
        400
    );
    assert_eq!(
        report(json!({ "trackId": id, "durationSecs": -1 }))
            .await
            .status(),
        400
    );
    assert_eq!(report(json!(null)).await.status(), 400);
}

#[tokio::test]
async fn an_outage_is_not_mistaken_for_a_deleted_track() {
    let h = harness!();
    let cookie = h.owner_cookie();
    h.state.db.close().await;
    let response = h
        .post(
            "/api/stats/listen",
            &cookie,
            json!({ "trackId": 1, "durationSecs": 1 }),
        )
        .await;
    // 5xx is what tells the client to keep the listen queued.
    assert_eq!(response.status(), 500);
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({ "error": "Internal Server Error" })
    );
}

/// Joins through an invite link as a brand new guest. Returns their cookie.
async fn join(h: &Harness, invite: &str) -> String {
    let response = h
        .http
        .get(format!("{invite}&name=Sam"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 307);
    assert!(
        response.headers()["location"]
            .to_str()
            .unwrap()
            .ends_with("/jam")
    );
    returned_cookies(&response)
}

async fn invite_url(h: &Harness, cookie: &str) -> String {
    let settings = h
        .http
        .get(h.url("/api/settings"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    settings["inviteUrl"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn a_guest_goes_from_invite_to_kicked() {
    let h = harness!();
    library(&h).await;
    let host = h.owner_cookie();
    let id = track_id(&h, &host, "Amber").await;
    let stream = format!("/api/tracks/{id}/stream");

    let settings = h.json("/api/settings", &host).await;
    assert_eq!(settings["jam"], Value::Null);
    assert_eq!(settings["secretMissing"], false);
    assert!(
        settings["overlayUrl"]
            .as_str()
            .unwrap()
            .contains("/obs?token=")
    );

    assert_eq!(
        h.post("/api/jam/start", &host, json!({ "hostName": " DJ " }))
            .await
            .status(),
        204
    );
    let invite = invite_url(&h, &host).await;

    let guest = join(&h, &invite).await;
    assert_eq!(h.mock.signups.load(Ordering::SeqCst), 1);

    // In the jam, but not let in: no audio, no queue, no library.
    let waiting = h.json("/api/jam/guest", &guest).await;
    assert_eq!(waiting, json!({ "status": "pending", "name": "Sam" }));
    assert_eq!(h.get(&stream, &guest).await.status(), 403);
    assert_eq!(
        h.post("/api/jam/add", &guest, json!({ "trackId": id }))
            .await
            .status(),
        403
    );

    // Never the host's endpoints, whatever the state.
    for path in [
        "/api/search?q=a",
        "/api/playlists/sync",
        "/api/library/tracks",
        "/api/settings",
    ] {
        assert_eq!(h.get(path, &guest).await.status(), 403, "{path}");
    }
    assert_eq!(
        h.post("/api/stats/listen", &guest, json!({}))
            .await
            .status(),
        403
    );
    assert_eq!(
        h.post("/api/jam/stop", &guest, json!({})).await.status(),
        403
    );

    let settings = h.json("/api/settings", &host).await;
    assert_eq!(settings["jam"]["hostName"], "DJ");
    assert_eq!(settings["participants"][0]["name"], "Sam");
    assert_eq!(settings["participants"][0]["status"], "pending");
    let pid = settings["participants"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let admit = format!("/api/jam/participants/{pid}/admit");
    assert_eq!(h.post(&admit, &host, json!({})).await.status(), 204);

    // Takes effect at once, despite the cached "no" from a moment ago.
    assert_eq!(h.get(&stream, &guest).await.status(), 200);
    let view = h.json("/api/jam/guest", &guest).await;
    assert_eq!(view["status"], "admitted");
    assert_eq!(view["hostName"], "DJ");
    assert_eq!(view["albums"].as_array().unwrap().len(), 2);
    assert_eq!(view["albums"][1]["tracks"][0]["title"], "01 Amber");
    assert_eq!(view["albums"][1]["tracks"].as_array().unwrap().len(), 2);
    assert_eq!(view["nowPlaying"]["track"], Value::Null);

    assert_eq!(
        h.post("/api/jam/add", &guest, json!({ "trackId": id }))
            .await
            .status(),
        204
    );
    assert_eq!(
        h.post("/api/jam/add", &guest, json!({ "trackId": 999999 }))
            .await
            .status(),
        404
    );
    assert_eq!(
        h.post("/api/jam/add", &guest, json!({ "trackId": "x" }))
            .await
            .status(),
        400
    );
    assert_eq!(
        h.post("/api/jam/add", &host, json!({ "trackId": id }))
            .await
            .status(),
        403
    );

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    {
        let broadcasts = h.mock.broadcasts.lock().unwrap();
        let add = broadcasts.iter().find(|m| m["event"] == "jam-add").unwrap();
        assert_eq!(add["topic"], "player");
        assert_eq!(add["private"], true);
        assert_eq!(add["payload"]["addedBy"], "Sam");
        assert_eq!(add["payload"]["track"]["id"], id);
    }

    // Five a minute, then slow down.
    assert_eq!(
        h.post("/api/jam/add", &guest, json!({ "trackId": id }))
            .await
            .status(),
        204
    );
    assert_eq!(
        h.post("/api/jam/add", &guest, json!({ "trackId": id }))
            .await
            .status(),
        429
    );

    let kick = format!("/api/jam/participants/{pid}/kick");
    assert_eq!(h.post(&kick, &host, json!({})).await.status(), 204);

    // Locked out on the very next request, with no wait for a cache to expire.
    assert_eq!(h.get(&stream, &guest).await.status(), 401);
    assert_eq!(
        h.json("/api/settings", &host).await["participants"],
        json!([])
    );
}

#[tokio::test]
async fn rotating_the_invite_stops_new_joins_only() {
    let h = harness!();
    let host = h.owner_cookie();
    h.post("/api/jam/start", &host, json!({})).await;
    let old = invite_url(&h, &host).await;

    let first = join(&h, &old).await;

    assert_eq!(
        h.post("/api/jam/invite", &host, json!({})).await.status(),
        204
    );

    let refused = h.http.get(&old).send().await.unwrap();
    assert!(
        refused.headers()["location"]
            .to_str()
            .unwrap()
            .ends_with("/jam?error=jam_closed")
    );

    // The guest already in keeps working, and re-opening the link reuses the account.
    let again = h
        .http
        .get(&old)
        .header("cookie", &first)
        .send()
        .await
        .unwrap();
    assert!(
        again.headers()["location"]
            .to_str()
            .unwrap()
            .ends_with("/jam")
    );
    assert_eq!(h.mock.signups.load(Ordering::SeqCst), 2);

    let fresh = invite_url(&h, &host).await;
    assert_ne!(fresh, old);
    join(&h, &fresh).await;
}

#[tokio::test]
async fn ending_a_jam_locks_guests_out() {
    let h = harness!();
    library(&h).await;
    let host = h.owner_cookie();
    let id = track_id(&h, &host, "Amber").await;
    h.post("/api/jam/start", &host, json!({})).await;
    let guest = join(&h, &invite_url(&h, &host).await).await;

    assert_eq!(h.json("/api/jam/guest", &guest).await["status"], "pending");
    assert_eq!(
        h.post("/api/jam/stop", &host, json!({})).await.status(),
        204
    );

    assert_eq!(h.get("/api/jam/guest", &guest).await.status(), 401);
    assert_eq!(
        h.get(&format!("/api/tracks/{id}/stream"), &guest)
            .await
            .status(),
        401
    );
    assert_eq!(h.json("/api/settings", &host).await["jam"], Value::Null);

    // The old link is dead too.
    let settings_invite =
        h.state
            .tokens
            .sign("jam-invite", json!({ "jamId": "gone", "epoch": 0 }), 60);
    let bad = h
        .http
        .get(h.url(&format!("/jam/join?token={}", settings_invite.unwrap())))
        .send()
        .await
        .unwrap();
    assert!(
        bad.headers()["location"]
            .to_str()
            .unwrap()
            .ends_with("/jam?error=jam_closed")
    );
}

#[tokio::test]
async fn leaving_clears_the_session() {
    let h = harness!();
    let host = h.owner_cookie();
    h.post("/api/jam/start", &host, json!({})).await;
    let guest = join(&h, &invite_url(&h, &host).await).await;

    let left = h.post("/api/jam/leave", &guest, json!({})).await;
    assert_eq!(left.status(), 204);
    let cleared = left.headers()["set-cookie"].to_str().unwrap();
    assert!(cleared.contains("Max-Age=0"), "{cleared}");

    assert_eq!(h.get("/api/jam/guest", &guest).await.status(), 401);
    assert_eq!(
        h.post("/api/jam/leave", &host, json!({})).await.status(),
        403
    );
}

#[tokio::test]
async fn bad_invites_bounce_with_a_reason() {
    let h = harness!();

    let bad = h.get("/jam/join?token=nonsense", "").await;
    assert_eq!(bad.status(), 307);
    assert!(
        bad.headers()["location"]
            .to_str()
            .unwrap()
            .ends_with("/jam?error=invalid_invite")
    );
    assert_eq!(h.mock.signups.load(Ordering::SeqCst), 0);

    // The owner opening their own link must not gain guest claims.
    let host = h.owner_cookie();
    h.post("/api/jam/start", &host, json!({})).await;
    let invite = invite_url(&h, &host).await;
    let own = h
        .http
        .get(&invite)
        .header("cookie", &host)
        .send()
        .await
        .unwrap();
    assert!(
        own.headers()["location"]
            .to_str()
            .unwrap()
            .ends_with("/jam?error=host")
    );
    assert!(h.mock.user("owner")["app_metadata"].get("jam").is_none());

    for _ in 0..12 {
        h.http
            .get(h.url("/jam/join"))
            .header("cf-connecting-ip", "9.9.9.9")
            .send()
            .await
            .unwrap();
    }
    let limited = h
        .http
        .get(h.url("/jam/join"))
        .header("cf-connecting-ip", "9.9.9.9")
        .send()
        .await
        .unwrap();
    assert!(
        limited.headers()["location"]
            .to_str()
            .unwrap()
            .ends_with("/jam?error=rate_limited")
    );
}

#[tokio::test]
async fn validates_a_session_once_rather_than_on_every_request() {
    let h = harness!();
    let cookie = h.owner_cookie();

    let requests = (0..50).map(|_| h.get("/api/search?q=%20", &cookie));
    for response in join_all(requests).await {
        assert_eq!(response.status(), 200);
    }
    assert_eq!(h.mock.user_lookups.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn refreshes_an_expired_session_once_for_a_burst_of_requests() {
    let h = harness!();
    let cookie = h.cookie_for(h.mock.session(&h.owner_id, 10));

    let requests = (0..20).map(|_| h.get("/api/search?q=%20", &cookie));
    let responses = join_all(requests).await;

    // Refresh tokens are single use. Spending it twice would log the owner out.
    assert_eq!(h.mock.refreshes.load(Ordering::SeqCst), 1);
    for response in &responses {
        assert_eq!(response.status(), 200);
        let set_cookie = response.headers()["set-cookie"].to_str().unwrap();
        assert!(set_cookie.contains("Max-Age=34560000"), "{set_cookie}");
        assert!(
            response.headers()["cache-control"]
                .to_str()
                .unwrap()
                .contains("no-store")
        );
    }

    // And the new cookie is good on its own.
    let fresh = returned_cookies(&responses[0]);
    assert_ne!(fresh, cookie);
    let response = h.get("/api/search?q=%20", &fresh).await;
    assert_eq!(response.status(), 200);
    assert!(response.headers().get("set-cookie").is_none());
}

fn verifier_cookie(h: &Harness) -> String {
    use base64::Engine;
    let value = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("\"the-verifier\"");
    format!(
        "{}-code-verifier=base64-{value}",
        h.state.auth.storage_key()
    )
}

#[tokio::test]
async fn the_sign_in_callback_completes_pkce() {
    let h = harness!();
    let verifier = verifier_cookie(&h);
    let proxied = |path: &str| {
        h.http
            .get(h.url(path))
            .header("cookie", &verifier)
            .header("x-forwarded-host", "music.example.com")
            .header("x-forwarded-proto", "https")
    };

    let location =
        |response: &reqwest::Response| response.headers()["location"].to_str().unwrap().to_owned();

    let missing = proxied("/auth/callback").send().await.unwrap();
    assert_eq!(
        location(&missing),
        "https://music.example.com/login?error=missing_code"
    );

    let good = proxied("/auth/callback?code=owner&next=/stats")
        .send()
        .await
        .unwrap();
    assert_eq!(good.status(), 307);
    assert_eq!(location(&good), "https://music.example.com/stats");

    let session = returned_cookies(&good);
    assert_eq!(h.get("/api/search?q=%20", &session).await.status(), 200);

    // ?next= cannot send anyone off-site.
    for next in ["//evil.example", "https://evil.example", "/%5Cevil.example"] {
        let response = proxied(&format!("/auth/callback?code=owner&next={next}"))
            .send()
            .await
            .unwrap();
        assert_eq!(location(&response), "https://music.example.com/", "{next}");
    }

    let no_verifier = h.get("/auth/callback?code=owner", "").await;
    assert!(location(&no_verifier).ends_with("/login?error=exchange_failed"));
}

#[tokio::test]
async fn the_owner_flag_follows_owner_github_id() {
    let h = harness!();
    let verifier = verifier_cookie(&h);

    let owner = h.mock.add_user(json!({
        "id": "new-owner",
        "app_metadata": {},
        "identities": [{ "provider": "github", "identity_data": { "provider_id": support::OWNER } }],
    }));
    let former = h.mock.add_user(json!({
        "id": "former-owner",
        "app_metadata": { "owner": true },
        "identities": [{ "provider": "github", "identity_data": { "provider_id": "1" } }],
    }));

    for id in [&owner, &former] {
        h.get(&format!("/auth/callback?code={id}"), &verifier).await;
    }

    assert_eq!(h.mock.user(&owner)["app_metadata"]["owner"], true);
    assert_eq!(h.mock.user(&former)["app_metadata"]["owner"], false);
    // Each stamp mints a fresh token, or the Realtime policy sees stale claims.
    assert_eq!(h.mock.refreshes.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn native_apps_authenticate_with_a_bearer_token() {
    let h = harness!();
    library(&h).await;

    let session = h.mock.session(&h.owner_id, 3600);
    let token = session["access_token"].as_str().unwrap();
    let search = |auth: String| {
        h.http
            .get(h.url("/api/search?q=Amb"))
            .header("authorization", auth)
            .send()
    };

    let ok = search(format!("Bearer {token}")).await.unwrap();
    assert_eq!(ok.status(), 200);
    assert!(ok.headers().get("set-cookie").is_none());
    let body: Value = ok.json().await.unwrap();
    assert_eq!(body["tracks"][0]["title"], "01 Amber");

    assert_eq!(
        search("Bearer not-a-token".into()).await.unwrap().status(),
        401
    );
    assert_eq!(
        search(format!("Basic {token}")).await.unwrap().status(),
        401
    );
}

#[tokio::test]
async fn native_sign_in_stamps_the_owner_flag_for_the_host_only() {
    let h = harness!();
    let host = h.mock.add_user(json!({
        "id": "fresh-host",
        "app_metadata": {},
        "identities": [{ "provider": "github", "identity_data": { "provider_id": support::OWNER } }],
    }));
    let token = h.mock.session(&host, 3600)["access_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let stamp = |token: String| {
        h.http
            .post(h.url("/api/auth/owner"))
            .bearer_auth(token)
            .send()
    };

    let first: Value = stamp(token.clone()).await.unwrap().json().await.unwrap();
    assert_eq!(first, json!({ "owner": true, "refresh": true }));
    assert_eq!(h.mock.user(&host)["app_metadata"]["owner"], true);
    let again: Value = stamp(token).await.unwrap().json().await.unwrap();
    assert_eq!(again["refresh"], false);

    let stranger = h
        .mock
        .add_user(json!({ "id": "stranger", "app_metadata": {} }));
    let token = h.mock.session(&stranger, 3600)["access_token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(stamp(token).await.unwrap().status(), 401);
    assert!(
        h.mock.user(&stranger)["app_metadata"]
            .get("owner")
            .is_none()
    );
}

#[tokio::test]
async fn hands_native_apps_the_public_supabase_settings() {
    let h = harness!();
    let response = h.get("/api/config", "").await;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["anonKey"], "anon");
    assert!(body["supabaseUrl"].as_str().unwrap().starts_with("http://"));
    assert_eq!(body["publicUrl"], "https://music.example.com");
    assert_eq!(h.mock.user_lookups.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn serves_the_latest_desktop_build_to_the_owner() {
    let h = harness!();
    let owner = h.owner_cookie();
    let dir = h.state.config.releases_dir.join("linux-x86_64");

    assert_eq!(
        h.get("/api/client/latest/linux-x86_64", &owner)
            .await
            .status(),
        404
    );

    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("cpz-music"), b"new build").unwrap();
    std::fs::write(
        dir.join("latest.json"),
        r#"{ "version": "0.2.0", "file": "cpz-music", "sha256": "abc" }"#,
    )
    .unwrap();

    assert_eq!(
        h.get("/api/client/latest/linux-x86_64", "").await.status(),
        401
    );
    assert_eq!(
        h.get("/api/client/download/linux-x86_64", "")
            .await
            .status(),
        401
    );

    let latest = h.json("/api/client/latest/linux-x86_64", &owner).await;
    assert_eq!(latest["version"], "0.2.0");
    assert_eq!(latest["sha256"], "abc");

    let download = h.get("/api/client/download/linux-x86_64", &owner).await;
    assert_eq!(download.status(), 200);
    assert_eq!(download.bytes().await.unwrap().as_ref(), b"new build");

    // A manifest pointing outside its folder is not followed.
    std::fs::write(
        dir.join("latest.json"),
        r#"{ "version": "9.0.0", "file": "../../secret", "sha256": "abc" }"#,
    )
    .unwrap();
    assert_eq!(
        h.get("/api/client/latest/linux-x86_64", &owner)
            .await
            .status(),
        404
    );
    assert_eq!(
        h.get("/api/client/download/linux-x86_64", &owner)
            .await
            .status(),
        404
    );
}

#[tokio::test]
async fn serves_the_jam_and_overlay_pages_with_their_own_policy() {
    let h = harness!();
    for path in ["/jam", "/obs?token=whatever"] {
        let page = h.get(path, "").await;
        assert_eq!(page.status(), 200, "{path}");
        let policy = page.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .to_owned();
        let nonce = policy
            .split("'nonce-")
            .nth(1)
            .and_then(|rest| rest.split('\'').next())
            .unwrap()
            .to_owned();
        let html = page.text().await.unwrap();
        assert!(
            html.contains(&format!("<script nonce=\"{nonce}\">")),
            "{path}"
        );
        assert!(!html.contains("{nonce}"), "{path}");
    }

    // An invite link without a name asks for one before anything else happens.
    let host = h.owner_cookie();
    h.post("/api/jam/start", &host, json!({})).await;
    let invite = invite_url(&h, &host).await;
    let ask = h.http.get(&invite).send().await.unwrap();
    assert_eq!(ask.status(), 200);
    assert!(ask.text().await.unwrap().contains("Join the jam"));
    assert_eq!(h.mock.signups.load(Ordering::SeqCst), 0);
}
