use axum::http::HeaderMap;

/// The origin the browser actually used.
///
/// Behind the tunnel this server binds to a local address the browser cannot
/// reach, so redirecting there sends the user somewhere their cookies do not
/// exist. The Host header carries the name they typed, and a proxy in front moves
/// it to X-Forwarded-Host.
pub fn request_origin(headers: &HeaderMap) -> String {
    let header = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());
    let host = header("x-forwarded-host")
        .or_else(|| header("host"))
        .unwrap_or("localhost:3006");
    let protocol = header("x-forwarded-proto").unwrap_or("http");
    format!("{protocol}://{host}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_what_the_proxy_says_the_browser_used() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "0.0.0.0:3006".parse().unwrap());
        assert_eq!(request_origin(&headers), "http://0.0.0.0:3006");

        headers.insert("x-forwarded-host", "music.example.com".parse().unwrap());
        headers.insert("x-forwarded-proto", "https".parse().unwrap());
        assert_eq!(request_origin(&headers), "https://music.example.com");
    }
}
