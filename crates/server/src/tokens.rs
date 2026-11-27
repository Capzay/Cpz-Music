use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Utc;
use hmac::{Hmac, KeyInit, Mac};
use serde_json::{Map, Value, json};
use sha2::Sha256;

/// Small signed tokens for the two places a browser has to prove something
/// without a Supabase session: the OBS overlay (a browser source cannot complete
/// an OAuth flow) and jam invite links (handed to people with no account).
///
/// The same format the old Next.js app signed, so links it handed out keep
/// working. `purpose` is part of the signed payload, so an OBS token can
/// never be replayed as a jam invite.
#[derive(Clone)]
pub struct Tokens {
    secret: Option<Vec<u8>>,
}

fn hmac(secret: &[u8], body: &str) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC takes any key length");
    mac.update(body.as_bytes());
    mac
}

impl Tokens {
    pub fn new(secret: Option<&str>) -> Self {
        let secret = secret
            .filter(|s| s.chars().count() >= 32)
            .map(|s| s.as_bytes().to_vec());
        Self { secret }
    }

    pub fn can_sign(&self) -> bool {
        self.secret.is_some()
    }

    /// None when APP_SECRET is missing or too short to be trusted.
    pub fn sign(&self, purpose: &str, claims: Value, ttl_seconds: i64) -> Option<String> {
        let secret = self.secret.as_ref()?;

        let mut payload: Map<String, Value> = match claims {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        payload.insert("purpose".into(), json!(purpose));
        payload.insert("exp".into(), json!(Utc::now().timestamp() + ttl_seconds));

        let body = URL_SAFE_NO_PAD.encode(Value::Object(payload).to_string());
        let signature = URL_SAFE_NO_PAD.encode(hmac(secret, &body).finalize().into_bytes());
        Some(format!("{body}.{signature}"))
    }

    pub fn verify(&self, token: Option<&str>, purpose: &str) -> Option<Value> {
        let secret = self.secret.as_ref()?;
        let (body, signature) = token?.rsplit_once('.')?;
        if body.is_empty() {
            return None;
        }

        let provided = URL_SAFE_NO_PAD.decode(signature).ok()?;
        // Constant-time, so the signature cannot be found a byte at a time.
        hmac(secret, body).verify_slice(&provided).ok()?;

        let payload: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(body).ok()?).ok()?;
        if payload.get("purpose")?.as_str()? != purpose {
            return None;
        }
        if payload.get("exp")?.as_f64()? < Utc::now().timestamp_millis() as f64 / 1000.0 {
            return None;
        }
        Some(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens() -> Tokens {
        Tokens::new(Some(&"0".repeat(64)))
    }

    #[test]
    fn round_trips_claims() {
        let token = tokens().sign("obs", json!({ "scope": "overlay" }), 60);
        let claims = tokens().verify(token.as_deref(), "obs").unwrap();
        assert_eq!(claims["scope"], "overlay");
        assert_eq!(claims["purpose"], "obs");
    }

    #[test]
    fn rejects_a_token_signed_for_a_different_purpose() {
        let token = tokens().sign("obs", json!({}), 60);
        assert!(tokens().verify(token.as_deref(), "jam-invite").is_none());
    }

    #[test]
    fn rejects_a_tampered_payload() {
        let token = tokens()
            .sign("jam-invite", json!({ "jamId": "jam-1" }), 60)
            .unwrap();
        let (body, signature) = token.rsplit_once('.').unwrap();
        let forged = URL_SAFE_NO_PAD
            .encode(json!({ "jamId": "jam-2", "purpose": "jam-invite", "exp": 9e9 }).to_string());
        assert!(
            tokens()
                .verify(Some(&format!("{forged}.{signature}")), "jam-invite")
                .is_none()
        );
        assert!(
            tokens()
                .verify(Some(&format!("{body}.{signature}")), "jam-invite")
                .is_some()
        );
    }

    #[test]
    fn rejects_a_token_signed_with_a_different_secret() {
        let token = tokens().sign("obs", json!({}), 60);
        let other = Tokens::new(Some(&"1".repeat(64)));
        assert!(other.verify(token.as_deref(), "obs").is_none());
    }

    #[test]
    fn rejects_an_expired_token() {
        let token = tokens().sign("obs", json!({}), -1);
        assert!(tokens().verify(token.as_deref(), "obs").is_none());
    }

    #[test]
    fn rejects_malformed_input_rather_than_panicking() {
        for token in [
            None,
            Some(""),
            Some("nodot"),
            Some(".sig"),
            Some("not-base64.short"),
        ] {
            assert!(tokens().verify(token, "obs").is_none());
        }
    }

    #[test]
    fn refuses_to_sign_with_a_weak_secret() {
        assert!(
            Tokens::new(Some("tooshort"))
                .sign("obs", json!({}), 60)
                .is_none()
        );
        assert!(Tokens::new(None).sign("obs", json!({}), 60).is_none());
    }

    /// Generated by the Node implementation, so a change that breaks links
    /// already handed out fails here rather than in somebody's OBS scene.
    #[test]
    fn accepts_a_token_minted_by_the_next_app() {
        let token = "eyJqYW1JZCI6ImphbS0xIiwiZXBvY2giOjMsInB1cnBvc2UiOiJqYW0taW52aXRlIiwiZXhwIjo0MTAyNDQ0ODAwfQ.1yFNTSkYwxwrNuHaMP9VNi3WqBRUlCtu5dQ4Az4dW8w";
        let claims = tokens().verify(Some(token), "jam-invite").unwrap();
        assert_eq!(claims["jamId"], "jam-1");
        assert_eq!(claims["epoch"], 3);
    }
}
