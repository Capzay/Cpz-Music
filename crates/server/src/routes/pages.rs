//! The only two web pages this server has: the jam guest page, opened from an
//! invite link on a guest's phone, and the OBS overlay, added as a browser
//! source. Everything else is the native app.
//!
//! Each is one static HTML file with its script and style inline. They get a
//! fresh nonce per response, so the policy can still refuse any other script.

use axum::http::{HeaderValue, header};
use axum::response::{Html, IntoResponse, Response};
use uuid::Uuid;

const JAM: &str = include_str!("../../pages/jam.html");
const OBS: &str = include_str!("../../pages/obs.html");

fn page(template: &str) -> Response {
    let nonce = Uuid::new_v4().simple().to_string();
    let html = template.replace("{nonce}", &nonce);
    let policy = format!(
        "default-src 'none'; script-src 'nonce-{nonce}'; style-src 'nonce-{nonce}'; \
         img-src 'self'; connect-src 'self'; form-action 'self'; base-uri 'none'; \
         frame-ancestors 'none'"
    );
    let mut response = Html(html).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&policy).expect("the policy is ASCII"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Shows whatever state the guest is in by asking `/api/jam/guest`, so it needs
/// nothing from the request. Also the name step of an invite link.
pub async fn jam() -> Response {
    page(JAM)
}

/// The token is checked by `/api/now-playing`, which the page polls; without a
/// valid one it says so instead of showing a track.
pub async fn obs() -> Response {
    page(OBS)
}
