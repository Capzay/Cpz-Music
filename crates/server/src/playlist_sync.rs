use std::collections::HashSet;

use serde_json::Value;

/// Local-first playlists, one document per playlist.
///
/// One host, several devices: the whole playlist is the conflict unit. Concurrent
/// edits on two devices: later `updatedAt` wins. There is no CRDT and no per-track
/// op log.
pub const MAX_NAME: usize = 120;
pub const MAX_TRACKS: usize = 2000;
pub const MAX_SYNC_DOCS: usize = 100;

#[derive(Debug, PartialEq)]
pub struct DirtyPlaylist {
    pub uuid: String,
    pub name: String,
    pub updated_at: f64,
    pub track_ids: Vec<i32>,
    pub deleted: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum WriteDecision {
    Create,
    Replace,
    Delete,
    Skip,
}

pub fn decide_write(incoming_at: f64, deleted: bool, existing_at: Option<f64>) -> WriteDecision {
    match existing_at {
        None if deleted => WriteDecision::Skip,
        None => WriteDecision::Create,
        Some(at) if incoming_at < at => WriteDecision::Skip,
        Some(_) if deleted => WriteDecision::Delete,
        Some(_) => WriteDecision::Replace,
    }
}

/// Drops ids the library no longer has, keeping the incoming order and repeats.
pub fn filter_known_track_ids(track_ids: &[i32], known: &HashSet<i32>) -> Vec<i32> {
    track_ids
        .iter()
        .copied()
        .filter(|id| known.contains(id))
        .take(MAX_TRACKS)
        .collect()
}

pub fn is_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

/// JavaScript's `Number.isInteger`, which also accepts `5.0`.
pub fn js_integer(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| {
        value
            .as_f64()
            .filter(|n| n.fract() == 0.0 && n.abs() < 9e15)
            .map(|n| n as i64)
    })
}

fn clean_name(raw: Option<&Value>) -> String {
    let text = match raw {
        Some(Value::String(text)) => text.clone(),
        Some(value @ (Value::Number(_) | Value::Bool(_))) => value.to_string(),
        _ => String::new(),
    };
    text.trim().chars().take(MAX_NAME).collect()
}

pub fn parse_dirty_playlists(body: &Value) -> Option<Vec<DirtyPlaylist>> {
    let playlists = body.get("playlists")?.as_array()?;
    if playlists.len() > MAX_SYNC_DOCS {
        return None;
    }
    playlists.iter().map(parse_dirty_playlist).collect()
}

fn parse_dirty_playlist(raw: &Value) -> Option<DirtyPlaylist> {
    let uuid = raw
        .get("uuid")?
        .as_str()
        .filter(|value| is_uuid(value))?
        .to_owned();
    let updated_at = raw
        .get("updatedAt")?
        .as_f64()
        .filter(|at| at.is_finite() && *at >= 0.0)?;
    let name = clean_name(raw.get("name"));

    if raw.get("deleted") == Some(&Value::Bool(true)) {
        return Some(DirtyPlaylist {
            uuid,
            name,
            updated_at,
            track_ids: Vec::new(),
            deleted: true,
        });
    }

    if name.is_empty() {
        return None;
    }
    let ids = raw.get("trackIds")?.as_array()?;
    if ids.len() > MAX_TRACKS {
        return None;
    }
    let track_ids = ids
        .iter()
        .map(|id| js_integer(id).filter(|id| *id > 0))
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        // Past Int range cannot be a real track, so it filters out like any other
        // id the library does not have.
        .filter_map(|id| i32::try_from(id).ok())
        .collect();

    Some(DirtyPlaylist {
        uuid,
        name,
        updated_at,
        track_ids,
        deleted: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const UUID: &str = "11111111-2222-4333-8444-555555555555";

    #[test]
    fn decides_like_last_write_wins() {
        assert_eq!(decide_write(5.0, false, None), WriteDecision::Create);
        assert_eq!(decide_write(5.0, true, None), WriteDecision::Skip);
        assert_eq!(decide_write(5.0, false, Some(6.0)), WriteDecision::Skip);
        assert_eq!(decide_write(5.0, true, Some(6.0)), WriteDecision::Skip);
        assert_eq!(decide_write(6.0, false, Some(6.0)), WriteDecision::Replace);
        assert_eq!(decide_write(7.0, true, Some(6.0)), WriteDecision::Delete);
    }

    #[test]
    fn filters_unknown_ids_and_keeps_order_and_repeats() {
        let known = HashSet::from([1, 2]);
        assert_eq!(filter_known_track_ids(&[2, 9, 1, 2], &known), vec![2, 1, 2]);
    }

    #[test]
    fn parses_a_valid_doc() {
        let body = json!({ "playlists": [
            { "uuid": UUID, "name": "  Mix  ", "updatedAt": 5, "trackIds": [1, 2.0] }
        ]});
        let docs = parse_dirty_playlists(&body).unwrap();
        assert_eq!(docs[0].name, "Mix");
        assert_eq!(docs[0].track_ids, vec![1, 2]);
    }

    #[test]
    fn a_deleted_doc_needs_no_name_or_tracks() {
        let body = json!({ "playlists": [{ "uuid": UUID, "updatedAt": 5, "deleted": true }] });
        assert!(parse_dirty_playlists(&body).unwrap()[0].deleted);
    }

    #[test]
    fn rejects_bad_input() {
        let doc = |patch: Value| {
            let mut doc = json!({ "uuid": UUID, "name": "x", "updatedAt": 5, "trackIds": [] });
            for (key, value) in patch.as_object().unwrap() {
                doc[key] = value.clone();
            }
            json!({ "playlists": [doc] })
        };
        for patch in [
            json!({ "uuid": "nope" }),
            json!({ "name": "   " }),
            json!({ "updatedAt": -1 }),
            json!({ "updatedAt": "soon" }),
            json!({ "trackIds": [0] }),
            json!({ "trackIds": [1.5] }),
            json!({ "trackIds": "1" }),
        ] {
            assert!(
                parse_dirty_playlists(&doc(patch.clone())).is_none(),
                "{patch}"
            );
        }
        assert!(parse_dirty_playlists(&json!(null)).is_none());
        assert!(parse_dirty_playlists(&json!({})).is_none());
    }

    #[test]
    fn caps_the_batch_and_the_track_list() {
        let docs =
            vec![json!({ "uuid": UUID, "updatedAt": 1, "deleted": true }); MAX_SYNC_DOCS + 1];
        assert!(parse_dirty_playlists(&json!({ "playlists": docs })).is_none());

        let ids = vec![1; MAX_TRACKS + 1];
        let body = json!({ "playlists": [{ "uuid": UUID, "name": "x", "updatedAt": 1, "trackIds": ids }] });
        assert!(parse_dirty_playlists(&body).is_none());
    }
}
