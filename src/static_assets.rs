use axum::{
    body::Body,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "web/out"]
struct Assets;

pub async fn serve(uri: Uri) -> Response {
    let candidates = asset_candidates(uri.path());

    for (candidate, status) in candidates {
        if let Some(file) = Assets::get(&candidate) {
            let mime = mime_guess::from_path(&candidate).first_or_octet_stream();
            return Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(file.data.into_owned()))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
        }
    }

    StatusCode::NOT_FOUND.into_response()
}

fn asset_candidates(path: &str) -> Vec<(String, StatusCode)> {
    let path = path.trim_matches('/');
    if path.is_empty() {
        return vec![("index.html".to_string(), StatusCode::OK)];
    }

    vec![
        (path.to_string(), StatusCode::OK),
        (format!("{path}.html"), StatusCode::OK),
        (format!("{path}/index.html"), StatusCode::OK),
        ("404.html".to_string(), StatusCode::NOT_FOUND),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_slash_routes_resolve_to_exported_index() {
        assert_eq!(
            asset_candidates("/dashboard/")[2],
            ("dashboard/index.html".to_string(), StatusCode::OK)
        );
        assert_eq!(
            asset_candidates("/claim/")[2],
            ("claim/index.html".to_string(), StatusCode::OK)
        );
    }
}
