//! The embedded web UI (`apps/desktop/dist-web`) with SPA fallback and cache rules.

use axum::body::Body;
use axum::http::{HeaderValue, Method, StatusCode, Uri, header};
use axum::response::Response;
use include_dir::{Dir, include_dir};

static DIST: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../../apps/desktop/dist-web");

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Cache rules: hashed build assets never change; the shell and service worker must revalidate.
fn cache_control(path: &str) -> &'static str {
    if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

fn respond(path: &str, data: &'static [u8], head: bool) -> Response {
    let mut res = Response::new(if head { Body::empty() } else { Body::from(data) });
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime(path)));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache_control(path)));
    res
}

/// Serve a file from the bundle; paths without an extension fall back to `index.html` (client routing).
pub fn serve(method: &Method, uri: &Uri) -> Response {
    let head = method == Method::HEAD;
    if method != Method::GET && !head {
        let mut res = Response::new(Body::empty());
        *res.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
        return res;
    }
    let path = uri.path().trim_start_matches('/');
    // Path traversal is impossible inside an embedded Dir, but refuse odd paths anyway.
    if path.split('/').any(|s| s == ".." || s.contains('\\')) {
        let mut res = Response::new(Body::empty());
        *res.status_mut() = StatusCode::BAD_REQUEST;
        return res;
    }
    let path = if path.is_empty() { "index.html" } else { path };
    if let Some(file) = DIST.get_file(path) {
        return respond(path, file.contents(), head);
    }
    let last = path.rsplit('/').next().unwrap_or(path);
    if !last.contains('.') {
        if let Some(index) = DIST.get_file("index.html") {
            return respond("index.html", index.contents(), head);
        }
    }
    let mut res = Response::new(Body::from("not found"));
    *res.status_mut() = StatusCode::NOT_FOUND;
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_serves_index_and_unknown_routes_fall_back() {
        let ok = serve(&Method::GET, &Uri::from_static("/"));
        assert_eq!(ok.status(), StatusCode::OK);
        let spa = serve(&Method::GET, &Uri::from_static("/chat/abc"));
        assert_eq!(spa.status(), StatusCode::OK);
        let missing = serve(&Method::GET, &Uri::from_static("/nope.js"));
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn cache_rules() {
        assert!(cache_control("assets/app-1a2b.js").contains("immutable"));
        assert_eq!(cache_control("index.html"), "no-cache");
        assert_eq!(cache_control("sw.js"), "no-cache");
    }
}
