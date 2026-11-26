use std::sync::Arc;

use axum::Extension;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use chrono::{Datelike, Days, NaiveDate, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::FromRow;

use super::{Params, no_store, parse_body};
use crate::Shared;
use crate::auth::Resolved;
use crate::error::{ApiError, ApiResult};
use crate::listen_day::listen_day;
use crate::playlist_sync::js_integer;

/// A single track cannot plausibly contribute more than this in one report.
const MAX_LISTEN_SECS: f64 = 6.0 * 60.0 * 60.0;
const PERIODS: [&str; 4] = ["week", "month", "year", "all"];

fn unknown_track() -> ApiError {
    // Deleted since the listen was queued. Nothing to record, and the client must
    // not retry forever, so this is a permanent rejection.
    ApiError::Status(StatusCode::CONFLICT, "Unknown track")
}

pub async fn listen(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
    body: Bytes,
) -> ApiResult<StatusCode> {
    // Guests listen along but do not write to the owner's listening history.
    caller.require_host()?;

    let body = parse_body(&body);
    let track_id = body
        .get("trackId")
        .and_then(js_integer)
        .ok_or_else(ApiError::bad_request)?;
    let duration = body
        .get("durationSecs")
        .and_then(Value::as_f64)
        .filter(|secs| *secs >= 0.0)
        .ok_or_else(ApiError::bad_request)?;

    let track_id = i32::try_from(track_id).map_err(|_| unknown_track())?;
    let seconds = duration.round().min(MAX_LISTEN_SECS) as i32;
    let date = listen_day(
        body.get("at").and_then(Value::as_f64),
        Utc::now().timestamp_millis(),
    );

    let result = sqlx::query(
        r#"INSERT INTO "DailyListenStat" ("trackId", date, "playCount", "listenSecs")
           VALUES ($1, $2, 1, $3)
           ON CONFLICT ("trackId", date) DO UPDATE SET
               "playCount" = "DailyListenStat"."playCount" + 1,
               "listenSecs" = "DailyListenStat"."listenSecs" + EXCLUDED."listenSecs""#,
    )
    .bind(track_id)
    .bind(date)
    .bind(seconds)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(sqlx::Error::Database(err)) if err.is_foreign_key_violation() => Err(unknown_track()),
        // Anything else is an outage, not a bad listen. A 5xx is what tells the
        // client to keep it queued and try again.
        Err(err) => Err(err.into()),
    }
}

fn range_for(period: &str, today: NaiveDate) -> (NaiveDate, NaiveDate) {
    let end = today + Days::new(1);
    let start = match period {
        "week" => end - Days::new(7),
        "month" => today.with_day(1).expect("every month has a first day"),
        "all" => NaiveDate::from_ymd_opt(1970, 1, 1).expect("valid date"),
        _ => NaiveDate::from_ymd_opt(today.year(), 1, 1).expect("valid date"),
    };
    (start, end)
}

#[derive(FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
struct Totals {
    #[sqlx(rename = "listenSecs")]
    listen_secs: i64,
    #[sqlx(rename = "playCount")]
    play_count: i64,
    #[sqlx(rename = "uniqueTracks")]
    unique_tracks: i64,
    #[sqlx(rename = "uniqueArtists")]
    unique_artists: i64,
}

#[derive(FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
struct TopTrack {
    id: i32,
    title: String,
    #[sqlx(rename = "albumId")]
    album_id: i32,
    #[sqlx(rename = "hasArtwork")]
    has_artwork: bool,
    artist: String,
    #[sqlx(rename = "listenSecs")]
    listen_secs: i64,
    #[sqlx(rename = "playCount")]
    play_count: i64,
}

#[derive(FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
struct TopArtist {
    #[sqlx(rename = "artistId")]
    artist_id: i32,
    #[sqlx(rename = "artistName")]
    artist_name: String,
    #[sqlx(rename = "listenSecs")]
    listen_secs: i64,
}

pub async fn summary(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
    Query(params): Query<Params>,
) -> ApiResult<Response> {
    caller.require_host()?;

    let period = params
        .get("period")
        .map(String::as_str)
        .filter(|period| PERIODS.contains(period))
        .unwrap_or("year");
    let (start, end) = range_for(period, Utc::now().date_naive());

    let totals = sqlx::query_as::<_, Totals>(
        r#"SELECT COALESCE(SUM(s."listenSecs"), 0)::bigint AS "listenSecs",
                  COALESCE(SUM(s."playCount"), 0)::bigint AS "playCount",
                  COUNT(DISTINCT s."trackId") AS "uniqueTracks",
                  COUNT(DISTINCT t."artistId") AS "uniqueArtists"
           FROM "DailyListenStat" s
           JOIN "Track" t ON t.id = s."trackId"
           WHERE s.date >= $1 AND s.date < $2"#,
    )
    .bind(start)
    .bind(end)
    .fetch_one(&state.db);

    let top_tracks = sqlx::query_as::<_, TopTrack>(
        r#"SELECT t.id, t.title, al.id AS "albumId",
                  al."artworkPath" IS NOT NULL AS "hasArtwork", ar.name AS artist,
                  SUM(s."listenSecs")::bigint AS "listenSecs",
                  SUM(s."playCount")::bigint AS "playCount"
           FROM "DailyListenStat" s
           JOIN "Track" t ON t.id = s."trackId"
           JOIN "Album" al ON al.id = t."albumId"
           JOIN "Artist" ar ON ar.id = t."artistId"
           WHERE s.date >= $1 AND s.date < $2
           GROUP BY t.id, al.id, ar.id
           ORDER BY SUM(s."listenSecs") DESC
           LIMIT 10"#,
    )
    .bind(start)
    .bind(end)
    .fetch_all(&state.db);

    let top_artists = sqlx::query_as::<_, TopArtist>(
        r#"SELECT a.id AS "artistId", a.name AS "artistName",
                  SUM(s."listenSecs")::bigint AS "listenSecs"
           FROM "DailyListenStat" s
           JOIN "Track" t ON t.id = s."trackId"
           JOIN "Artist" a ON a.id = t."artistId"
           WHERE s.date >= $1 AND s.date < $2
           GROUP BY a.id, a.name
           ORDER BY SUM(s."listenSecs") DESC
           LIMIT 10"#,
    )
    .bind(start)
    .bind(end)
    .fetch_all(&state.db);

    let (totals, top_tracks, top_artists) = tokio::try_join!(totals, top_tracks, top_artists)?;

    Ok(no_store(json!({
        "period": period,
        "totals": totals,
        "topTracks": top_tracks,
        "topArtists": top_artists,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn periods_end_tomorrow_and_start_where_the_page_did() {
        let today = date(2026, 7, 30);
        assert_eq!(
            range_for("week", today),
            (date(2026, 7, 24), date(2026, 7, 31))
        );
        assert_eq!(
            range_for("month", today),
            (date(2026, 7, 1), date(2026, 7, 31))
        );
        assert_eq!(
            range_for("year", today),
            (date(2026, 1, 1), date(2026, 7, 31))
        );
        assert_eq!(
            range_for("all", today),
            (date(1970, 1, 1), date(2026, 7, 31))
        );
    }
}
