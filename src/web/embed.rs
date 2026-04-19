use axum::{
    body::Body,
    http::{Response, StatusCode, Uri, header},
    response::IntoResponse,
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "frontend/"]
struct Asset;

pub async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Asset::get(path) {
        Some(file) => Response::builder()
            .header(header::CONTENT_TYPE, mime_for(path))
            .body(Body::from(file.data.into_owned()))
            .unwrap(),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap(),
    }
}

fn mime_for(path: &str) -> &'static str {
    if path.ends_with(".html")               { "text/html; charset=utf-8" }
    else if path.ends_with(".css")           { "text/css; charset=utf-8" }
    else if path.ends_with(".js")            { "application/javascript; charset=utf-8" }
    else if path.ends_with(".svg")           { "image/svg+xml" }
    else if path.ends_with(".ico")           { "image/x-icon" }
    else if path.ends_with(".png")           { "image/png" }
    else if path.ends_with(".jpg") || path.ends_with(".jpeg") { "image/jpeg" }
    else                                     { "application/octet-stream" }
}
