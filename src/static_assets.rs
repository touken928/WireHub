use axum::{
    body::Body,
    http::{header, Uri},
    response::Response,
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "frontend/dist/"]
struct FrontendAssets;

const CONTENT_SECURITY_POLICY: &str = concat!(
    "default-src 'self'; ",
    "script-src 'self'; ",
    "style-src 'self' https://fonts.googleapis.com; ",
    "style-src-attr 'unsafe-inline'; ",
    "font-src 'self' https://fonts.gstatic.com; ",
    "img-src 'self' data:; ",
    "connect-src 'self'; ",
    "object-src 'none'; base-uri 'self'; frame-ancestors 'none'"
);

pub async fn handler(uri: Uri) -> Response<Body> {
    let path = uri.path();
    if path == "/api" || path.starts_with("/api/") {
        return not_found();
    }

    let relative_path = path.trim_start_matches('/');
    if let Some(asset) = FrontendAssets::get(relative_path) {
        let content_type = mime_guess::from_path(relative_path)
            .first_or_octet_stream()
            .essence_str()
            .to_owned();
        let cache_control = if relative_path == "index.html" {
            "no-cache"
        } else {
            "public, max-age=31536000, immutable"
        };
        return Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::CACHE_CONTROL, cache_control)
            .header(header::CONTENT_SECURITY_POLICY, CONTENT_SECURITY_POLICY)
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
            .body(Body::from(asset.data.into_owned()))
            .expect("valid static asset response headers");
    }

    // Missing files and asset-like URLs must not be mistaken for client-side routes.
    // Only extensionless paths outside /api are eligible for the SPA entry point.
    if path.starts_with("/assets/")
        || relative_path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.contains('.'))
    {
        return not_found();
    }

    serve_index()
}

fn serve_index() -> Response<Body> {
    let asset = FrontendAssets::get("index.html").expect("frontend/dist/index.html is embedded");
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONTENT_SECURITY_POLICY, CONTENT_SECURITY_POLICY)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Body::from(asset.data.into_owned()))
        .expect("valid SPA response headers")
}

fn not_found() -> Response<Body> {
    Response::builder()
        .status(404)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from("Not Found"))
        .expect("valid 404 response headers")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn serves_embedded_frontend_and_spa_routes() {
        let index = handler("/".parse().unwrap()).await;
        assert_eq!(index.status(), 200);
        assert_eq!(
            index.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        assert_eq!(index.headers()[header::CACHE_CONTROL], "no-cache");
        assert!(index
            .headers()
            .contains_key(header::CONTENT_SECURITY_POLICY));

        let javascript_asset = FrontendAssets::iter()
            .find(|path| path.starts_with("assets/") && path.ends_with(".js"))
            .expect("embedded frontend includes a JavaScript asset");
        let asset = handler(format!("/{javascript_asset}").parse().unwrap()).await;
        assert_eq!(asset.status(), 200);
        assert_eq!(asset.headers()[header::CONTENT_TYPE], "text/javascript");
        assert!(asset.headers()[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .contains("immutable"));

        let policy = index.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap();
        assert!(policy.contains("style-src-attr 'unsafe-inline'"));
        assert!(policy.contains("script-src 'self'"));

        assert_eq!(handler("/peers/new".parse().unwrap()).await.status(), 200);
    }

    #[tokio::test]
    async fn api_typos_and_missing_assets_are_not_spa_fallbacks() {
        assert_eq!(
            handler("/api/healthty".parse().unwrap()).await.status(),
            404
        );
        assert_eq!(
            handler("/assets/missing.js".parse().unwrap())
                .await
                .status(),
            404
        );
        assert_eq!(handler("/missing.css".parse().unwrap()).await.status(), 404);
    }
}
