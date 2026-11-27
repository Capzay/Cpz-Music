use chrono::Utc;
use serde_json::{Value, json};

/// A Supabase session, kept as the JSON GoTrue returned. The browser reads this
/// same cookie and expects every field, so nothing is dropped on the way through.
#[derive(Clone, Debug)]
pub struct Session(Value);

impl Session {
    pub fn from_value(mut value: Value) -> Option<Self> {
        value.get("access_token")?.as_str()?;
        value.get("refresh_token")?.as_str()?;
        if value.get("expires_at").and_then(Value::as_i64).is_none() {
            let ttl = value.get("expires_in").and_then(Value::as_i64).unwrap_or(0);
            value["expires_at"] = json!(Utc::now().timestamp() + ttl);
        }
        Some(Self(value))
    }

    pub fn access_token(&self) -> &str {
        self.0["access_token"].as_str().unwrap_or_default()
    }

    pub fn refresh_token(&self) -> &str {
        self.0["refresh_token"].as_str().unwrap_or_default()
    }

    pub fn expires_at(&self) -> i64 {
        self.0["expires_at"].as_i64().unwrap_or(0)
    }

    pub fn user(&self) -> &Value {
        &self.0["user"]
    }

    pub fn to_json(&self) -> String {
        self.0.to_string()
    }
}
