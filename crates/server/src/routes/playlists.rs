use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Context;
use axum::Extension;
use axum::body::Bytes;
use axum::extract::State;
use axum::response::Response;
use chrono::{DateTime, NaiveDateTime};
use serde::Serialize;
use sqlx::{FromRow, PgPool, Postgres, Row, Transaction};

use super::{no_store, parse_body};
use crate::Shared;
use crate::auth::Resolved;
use crate::db::{PlayerTrack, track_query};
use crate::error::{ApiError, ApiResult};
use crate::playlist_sync::{
    DirtyPlaylist, WriteDecision, decide_write, filter_known_track_ids, parse_dirty_playlists,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistSnapshot {
    uuid: String,
    id: i32,
    name: String,
    updated_at: i64,
    tracks: Vec<PlayerTrack>,
}

#[derive(Serialize)]
struct Snapshot {
    playlists: Vec<PlaylistSnapshot>,
}

/// Every playlist in two queries, however many there are.
async fn snapshot(db: &PgPool) -> Result<Snapshot, sqlx::Error> {
    let playlists = sqlx::query_as::<_, (i32, String, String, NaiveDateTime)>(
        r#"SELECT id, uuid, name, "updatedAt" FROM "Playlist" ORDER BY "updatedAt" DESC"#,
    )
    .fetch_all(db);

    let entries = sqlx::query(track_query!(
        r#", pt."playlistId""#,
        r#"JOIN "PlaylistTrack" pt ON pt."trackId" = t.id
           ORDER BY pt."playlistId", pt.position ASC"#
    ))
    .fetch_all(db);

    let (playlists, entries) = tokio::try_join!(playlists, entries)?;

    let mut tracks: HashMap<i32, Vec<PlayerTrack>> = HashMap::new();
    for row in &entries {
        tracks
            .entry(row.try_get("playlistId")?)
            .or_default()
            .push(PlayerTrack::from_row(row)?);
    }

    Ok(Snapshot {
        playlists: playlists
            .into_iter()
            .map(|(id, uuid, name, updated_at)| PlaylistSnapshot {
                uuid,
                id,
                name,
                updated_at: updated_at.and_utc().timestamp_millis(),
                tracks: tracks.remove(&id).unwrap_or_default(),
            })
            .collect(),
    })
}

pub async fn read(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
) -> ApiResult<Response> {
    caller.require_host()?;
    Ok(no_store(snapshot(&state.db).await?))
}

async fn insert_tracks(
    tx: &mut Transaction<'_, Postgres>,
    playlist_id: i32,
    track_ids: &[i32],
) -> Result<(), sqlx::Error> {
    let positions: Vec<i32> = (0..track_ids.len() as i32).collect();
    sqlx::query(
        r#"INSERT INTO "PlaylistTrack" ("playlistId", "trackId", position)
           SELECT $1, * FROM unnest($2::int[], $3::int[])"#,
    )
    .bind(playlist_id)
    .bind(track_ids)
    .bind(&positions)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Last write wins, per document. Existing rows and known tracks are read once for
/// the whole batch: a device coming back online can send a hundred documents, and
/// every query here is a round trip across the internet.
async fn apply(db: &PgPool, docs: &[DirtyPlaylist]) -> ApiResult<()> {
    if docs.is_empty() {
        return Ok(());
    }

    let uuids: Vec<&str> = docs.iter().map(|doc| doc.uuid.as_str()).collect();
    let ids: Vec<i32> = docs
        .iter()
        .flat_map(|doc| doc.track_ids.iter().copied())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let existing = sqlx::query_as::<_, (String, i32, NaiveDateTime)>(
        r#"SELECT uuid, id, "updatedAt" FROM "Playlist" WHERE uuid = ANY($1)"#,
    )
    .bind(&uuids)
    .fetch_all(db);
    let known = sqlx::query_scalar::<_, i32>(r#"SELECT id FROM "Track" WHERE id = ANY($1)"#)
        .bind(&ids)
        .fetch_all(db);
    let (existing, known) = tokio::try_join!(existing, known)?;

    let known: HashSet<i32> = known.into_iter().collect();
    let mut existing: HashMap<String, (i32, f64)> = existing
        .into_iter()
        .map(|(uuid, id, at)| (uuid, (id, at.and_utc().timestamp_millis() as f64)))
        .collect();

    let mut tx = db.begin().await?;

    for doc in docs {
        let current = existing.get(&doc.uuid).copied();
        let decision = decide_write(doc.updated_at, doc.deleted, current.map(|(_, at)| at));

        if decision == WriteDecision::Skip {
            continue;
        }
        if decision == WriteDecision::Delete {
            let (id, _) = current.context("delete is only decided for an existing row")?;
            sqlx::query(r#"DELETE FROM "Playlist" WHERE id = $1"#)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            existing.remove(&doc.uuid);
            continue;
        }

        let track_ids = filter_known_track_ids(&doc.track_ids, &known);
        let updated_at = DateTime::from_timestamp_millis(doc.updated_at as i64)
            .ok_or_else(ApiError::bad_request)?
            .naive_utc();

        let id = match current {
            Some((id, _)) => {
                sqlx::query(r#"UPDATE "Playlist" SET name = $2, "updatedAt" = $3 WHERE id = $1"#)
                    .bind(id)
                    .bind(&doc.name)
                    .bind(updated_at)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query(r#"DELETE FROM "PlaylistTrack" WHERE "playlistId" = $1"#)
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
                id
            }
            None => sqlx::query(
                r#"INSERT INTO "Playlist" (uuid, name, "updatedAt") VALUES ($1, $2, $3)
                       RETURNING id"#,
            )
            .bind(&doc.uuid)
            .bind(&doc.name)
            .bind(updated_at)
            .fetch_one(&mut *tx)
            .await?
            .try_get(0)?,
        };

        insert_tracks(&mut tx, id, &track_ids).await?;
        existing.insert(doc.uuid.clone(), (id, doc.updated_at));
    }

    tx.commit().await?;
    Ok(())
}

pub async fn sync(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
    body: Bytes,
) -> ApiResult<Response> {
    caller.require_host()?;

    let docs = parse_dirty_playlists(&parse_body(&body)).ok_or_else(ApiError::bad_request)?;
    apply(&state.db, &docs).await?;

    Ok(no_store(snapshot(&state.db).await?))
}
