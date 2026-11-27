use axum::http::{HeaderMap, HeaderValue, header};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use percent_encoding::percent_decode_str;
use serde_json::Value;

use super::session::Session;

/// Reads and writes the session cookies `@supabase/ssr` uses, in its exact format,
/// so sessions the old Next.js app wrote are still read. The session is
/// JSON, encoded as `base64-<base64url>`, and split into `<key>.0`, `<key>.1`, ...
/// once it outgrows a single cookie.
const BASE64_PREFIX: &str = "base64-";
const MAX_CHUNK_SIZE: usize = 3180;
const MAX_AGE: u64 = 400 * 24 * 60 * 60;

pub fn parse(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .map(|(name, value)| {
            let decoded = percent_decode_str(value.trim_matches('"')).decode_utf8_lossy();
            (name.to_owned(), decoded.into_owned())
        })
        .collect()
}

fn find<'a>(cookies: &'a [(String, String)], name: &str) -> Option<&'a str> {
    cookies
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

/// The value under `key`, reassembled from chunks if it was split.
fn combine(cookies: &[(String, String)], key: &str) -> Option<String> {
    if let Some(value) = find(cookies, key).filter(|value| !value.is_empty()) {
        return Some(value.to_owned());
    }
    let mut combined = String::new();
    for index in 0.. {
        match find(cookies, &format!("{key}.{index}")).filter(|value| !value.is_empty()) {
            Some(chunk) => combined.push_str(chunk),
            None => break,
        }
    }
    (!combined.is_empty()).then_some(combined)
}

fn decode(value: &str) -> Option<String> {
    match value.strip_prefix(BASE64_PREFIX) {
        Some(encoded) => String::from_utf8(URL_SAFE_NO_PAD.decode(encoded).ok()?).ok(),
        None => Some(value.to_owned()),
    }
}

/// A partial write can leave chunks of two generations in the browser. They fail
/// to decode or parse, and count as no session rather than as an error.
pub fn read_session(cookies: &[(String, String)], key: &str) -> Option<Session> {
    let json: Value = serde_json::from_str(&decode(&combine(cookies, key)?)?).ok()?;
    Session::from_value(json)
}

/// The PKCE verifier the browser stored when it started the sign-in. auth-js
/// appends `/recovery` for password resets, which is not part of the verifier.
pub fn read_code_verifier(cookies: &[(String, String)], key: &str) -> Option<String> {
    let name = format!("{key}-code-verifier");
    let json: Value = serde_json::from_str(&decode(&combine(cookies, &name)?)?).ok()?;
    let verifier = json.as_str()?.split('/').next()?;
    (!verifier.is_empty()).then(|| verifier.to_owned())
}

fn set_cookie(name: &str, value: &str, max_age: u64) -> HeaderValue {
    format!("{name}={value}; Max-Age={max_age}; Path=/; SameSite=Lax")
        .parse()
        .expect("cookie names and base64 values are valid header text")
}

fn is_chunk_of(name: &str, key: &str) -> bool {
    name == key
        || name
            .strip_prefix(key)
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|index| !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()))
}

/// `Set-Cookie` values storing `session`, expiring any chunk from the previous
/// session that the new one does not overwrite.
pub fn write_session(
    cookies: &[(String, String)],
    key: &str,
    session: &Session,
) -> Vec<HeaderValue> {
    let encoded = format!(
        "{BASE64_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(session.to_json())
    );

    let chunks: Vec<(String, &str)> = if encoded.len() <= MAX_CHUNK_SIZE {
        vec![(key.to_owned(), encoded.as_str())]
    } else {
        // Base64 text is ASCII, so any byte offset is a safe place to split.
        encoded
            .as_bytes()
            .chunks(MAX_CHUNK_SIZE)
            .enumerate()
            .map(|(i, chunk)| {
                (
                    format!("{key}.{i}"),
                    std::str::from_utf8(chunk).expect("base64 is ASCII"),
                )
            })
            .collect()
    };

    let stale = cookies
        .iter()
        .filter(|(name, _)| is_chunk_of(name, key) && !chunks.iter().any(|(new, _)| new == name))
        .map(|(name, _)| set_cookie(name, "", 0));

    stale
        .chain(
            chunks
                .iter()
                .map(|(name, value)| set_cookie(name, value, MAX_AGE)),
        )
        .collect()
}

/// `Set-Cookie` values removing the session.
pub fn clear_session(cookies: &[(String, String)], key: &str) -> Vec<HeaderValue> {
    cookies
        .iter()
        .filter(|(name, _)| is_chunk_of(name, key))
        .map(|(name, _)| set_cookie(name, "", 0))
        .collect()
}

pub fn clear_code_verifier(key: &str) -> HeaderValue {
    set_cookie(&format!("{key}-code-verifier"), "", 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const KEY: &str = "sb-abc-auth-token";

    fn session(padding: usize) -> Session {
        Session::from_value(json!({
            "access_token": "access",
            "refresh_token": "refresh",
            "expires_at": 4102444800u64,
            "user": { "id": "u1", "padding": "x".repeat(padding) },
        }))
        .unwrap()
    }

    /// What a browser would send back after receiving these `Set-Cookie` values.
    fn stored(headers: &[HeaderValue]) -> Vec<(String, String)> {
        headers
            .iter()
            .map(|value| value.to_str().unwrap().split(';').next().unwrap())
            .map(|pair| pair.split_once('=').unwrap())
            .filter(|(_, value)| !value.is_empty())
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect()
    }

    #[test]
    fn round_trips_a_small_session_in_one_cookie() {
        let headers = write_session(&[], KEY, &session(0));
        assert_eq!(headers.len(), 1);
        let text = headers[0].to_str().unwrap();
        assert!(text.starts_with("sb-abc-auth-token=base64-"), "{text}");
        assert!(
            text.ends_with("; Max-Age=34560000; Path=/; SameSite=Lax"),
            "{text}"
        );
        assert_eq!(
            read_session(&stored(&headers), KEY).unwrap().access_token(),
            "access"
        );
    }

    #[test]
    fn splits_a_large_session_and_reassembles_it() {
        let headers = write_session(&[], KEY, &session(5000));
        let cookies = stored(&headers);
        assert!(cookies.len() >= 2);
        assert_eq!(cookies[0].0, "sb-abc-auth-token.0");
        assert!(
            cookies
                .iter()
                .all(|(_, value)| value.len() <= MAX_CHUNK_SIZE)
        );
        assert_eq!(
            read_session(&cookies, KEY).unwrap().refresh_token(),
            "refresh"
        );
    }

    #[test]
    fn expires_chunks_the_new_session_does_not_overwrite() {
        let old = stored(&write_session(&[], KEY, &session(5000)));
        let headers = write_session(&old, KEY, &session(0));
        let expired = headers
            .iter()
            .filter(|h| h.to_str().unwrap().contains("Max-Age=0"));
        assert_eq!(expired.count(), old.len());
    }

    #[test]
    fn leaves_lookalike_cookies_alone() {
        let cookies = vec![(format!("{KEY}-code-verifier"), "x".to_owned())];
        assert!(clear_session(&cookies, KEY).is_empty());
    }

    #[test]
    fn treats_corrupt_chunks_as_no_session() {
        for value in ["base64-!!!", "base64-bm90IGpzb24", "{}"] {
            let cookies = vec![(KEY.to_owned(), value.to_owned())];
            assert!(read_session(&cookies, KEY).is_none(), "{value}");
        }
    }

    #[test]
    fn reads_the_pkce_verifier_without_its_recovery_suffix() {
        let value = format!("base64-{}", URL_SAFE_NO_PAD.encode("\"abc123/recovery\""));
        let cookies = vec![(format!("{KEY}-code-verifier"), value)];
        assert_eq!(read_code_verifier(&cookies, KEY).as_deref(), Some("abc123"));
    }

    #[test]
    fn parses_a_cookie_header() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, "a=1; b=two%20words".parse().unwrap());
        assert_eq!(find(&parse(&headers), "b"), Some("two words"));
    }
}
