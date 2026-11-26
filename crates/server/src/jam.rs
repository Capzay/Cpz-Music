use chrono::NaiveDateTime;
use serde::{Serialize, Serializer};
use serde_json::json;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::tokens::Tokens;

const INVITE_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;

fn iso_date<S: Serializer>(value: &NaiveDateTime, serializer: S) -> Result<S::Ok, S::Error> {
    let text = value
        .and_utc()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    serializer.serialize_str(&text)
}

#[derive(Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Jam {
    pub id: String,
    #[sqlx(rename = "hostName")]
    pub host_name: Option<String>,
    #[sqlx(rename = "inviteEpoch")]
    #[serde(skip)]
    pub invite_epoch: i32,
    #[sqlx(rename = "requireApproval")]
    pub require_approval: bool,
    #[sqlx(rename = "createdAt")]
    #[serde(serialize_with = "iso_date")]
    pub created_at: NaiveDateTime,
}

#[derive(Debug, FromRow, Serialize)]
pub struct Participant {
    pub id: String,
    pub name: String,
    pub status: String,
}

#[derive(Debug, FromRow)]
pub struct Joined {
    pub id: String,
    #[sqlx(rename = "jamId")]
    pub jam_id: String,
    pub status: String,
}

pub struct InviteClaims {
    pub jam_id: String,
    pub epoch: f64,
}

pub fn sign_invite(tokens: &Tokens, jam_id: &str, epoch: i32) -> Option<String> {
    tokens.sign(
        "jam-invite",
        json!({ "jamId": jam_id, "epoch": epoch }),
        INVITE_TTL_SECONDS,
    )
}

pub fn read_invite(tokens: &Tokens, token: Option<&str>) -> Option<InviteClaims> {
    let claims = tokens.verify(token, "jam-invite")?;
    Some(InviteClaims {
        jam_id: claims.get("jamId")?.as_str()?.to_owned(),
        epoch: claims.get("epoch")?.as_f64()?,
    })
}

pub async fn active_jam(db: &PgPool) -> Result<Option<Jam>, sqlx::Error> {
    sqlx::query_as(
        r#"SELECT id, "hostName", "inviteEpoch", "requireApproval", "createdAt"
           FROM "Jam" WHERE active ORDER BY "createdAt" DESC LIMIT 1"#,
    )
    .fetch_optional(db)
    .await
}

/// One active jam at a time; starting a new one ends the old.
pub async fn create_jam(db: &PgPool, host_name: Option<&str>) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query(
        r#"UPDATE "Jam" SET active = false, "endedAt" = (now() AT TIME ZONE 'utc') WHERE active"#,
    )
    .execute(&mut *tx)
    .await?;
    // Prisma generated this id on the client, so the column has no default.
    sqlx::query(r#"INSERT INTO "Jam" (id, "hostName") VALUES ($1, $2)"#)
        .bind(Uuid::new_v4().to_string())
        .bind(host_name)
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

pub async fn end_jam(db: &PgPool, jam_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE "Jam" SET active = false, "endedAt" = (now() AT TIME ZONE 'utc') WHERE id = $1"#,
    )
    .bind(jam_id)
    .execute(db)
    .await?;
    Ok(())
}

/// Bumps the epoch, which invalidates every outstanding invite link at once.
pub async fn rotate_invite(db: &PgPool, jam_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query(r#"UPDATE "Jam" SET "inviteEpoch" = "inviteEpoch" + 1 WHERE id = $1"#)
        .bind(jam_id)
        .execute(db)
        .await?;
    Ok(())
}

/// Attaches a Supabase user to a jam, creating the participant row on first use.
///
/// Returns None when the jam is over or the link has been rotated. Existing
/// participants keep working after a rotation; only new joins are refused, which
/// is what makes "revoke the link" usable mid-party.
pub async fn join_jam(
    db: &PgPool,
    claims: &InviteClaims,
    user_id: &str,
    name: &str,
) -> Result<Option<Joined>, sqlx::Error> {
    let jam: Option<(String, i32, bool)> = sqlx::query_as(
        r#"SELECT id, "inviteEpoch", "requireApproval" FROM "Jam" WHERE id = $1 AND active"#,
    )
    .bind(&claims.jam_id)
    .fetch_optional(db)
    .await?;
    let Some((jam_id, invite_epoch, require_approval)) = jam else {
        return Ok(None);
    };

    let existing: Option<Joined> =
        sqlx::query_as(r#"SELECT id, "jamId", status FROM "JamParticipant" WHERE "userId" = $1"#)
            .bind(user_id)
            .fetch_optional(db)
            .await?;

    if let Some(existing) = existing {
        if existing.jam_id != jam_id {
            return Ok(None);
        }
        if existing.status == "kicked" {
            return Ok(Some(existing));
        }
        sqlx::query(r#"UPDATE "JamParticipant" SET name = $2 WHERE id = $1"#)
            .bind(&existing.id)
            .bind(name)
            .execute(db)
            .await?;
        return Ok(Some(existing));
    }

    if claims.epoch != f64::from(invite_epoch) {
        return Ok(None);
    }

    let joined = sqlx::query_as(
        r#"INSERT INTO "JamParticipant" (id, "jamId", "userId", name, status, "admittedAt")
           VALUES ($1, $2, $3, $4, $5,
                   CASE WHEN $6 THEN now() AT TIME ZONE 'utc' END)
           RETURNING id, "jamId", status"#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&jam_id)
    .bind(user_id)
    .bind(name)
    .bind(if require_approval {
        "pending"
    } else {
        "admitted"
    })
    .bind(!require_approval)
    .fetch_one(db)
    .await?;
    Ok(Some(joined))
}

pub async fn participant_status(db: &PgPool, pid: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(r#"SELECT status FROM "JamParticipant" WHERE id = $1"#)
        .bind(pid)
        .fetch_optional(db)
        .await
}

pub async fn list_participants(db: &PgPool, jam_id: &str) -> Result<Vec<Participant>, sqlx::Error> {
    sqlx::query_as(
        r#"SELECT id, name, status FROM "JamParticipant"
           WHERE "jamId" = $1 AND status IN ('pending', 'admitted')
           ORDER BY "joinedAt" ASC"#,
    )
    .bind(jam_id)
    .fetch_all(db)
    .await
}

/// Returns the participant's auth user, if they belong to this jam.
pub async fn set_participant_status(
    db: &PgPool,
    jam_id: &str,
    pid: &str,
    status: &str,
) -> Result<Option<Option<String>>, sqlx::Error> {
    sqlx::query_scalar(
        r#"UPDATE "JamParticipant"
           SET status = $3,
               "admittedAt" = CASE WHEN $3 = 'admitted' THEN now() AT TIME ZONE 'utc'
                                   ELSE "admittedAt" END
           WHERE id = $1 AND "jamId" = $2
           RETURNING "userId""#,
    )
    .bind(pid)
    .bind(jam_id)
    .bind(status)
    .fetch_optional(db)
    .await
}

pub async fn participant_user_ids(db: &PgPool, jam_id: &str) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        r#"SELECT "userId" FROM "JamParticipant" WHERE "jamId" = $1 AND "userId" IS NOT NULL"#,
    )
    .bind(jam_id)
    .fetch_all(db)
    .await
}
