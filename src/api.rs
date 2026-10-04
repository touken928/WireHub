use std::sync::Arc;

use crate::{
    kernel::{KernelHandle, ReloadPermit},
    model::*,
    storage::Store,
};
use axum::{
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use rand::RngCore;

mod configuration;
mod forwards;
mod groups;
mod peers;
mod settings;

pub use configuration::{get_config, get_status, put_policy};
pub use forwards::{create_forward, delete_forward, list_forwards};
pub use groups::{create_group, delete_group, list_groups, set_acl};
pub use peers::{create_peer, delete_peer, list_peers, move_peer};
pub use settings::{get_setup, post_setup, put_settings};

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    pub token: Option<String>,
    pub hub_public: String,
    pub kernel: KernelHandle,
}

pub(crate) fn auth(headers: &HeaderMap, state: &AppState) -> bool {
    state
        .token
        .as_ref()
        .filter(|token| !token.trim().is_empty())
        .is_some_and(|token| {
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value == format!("Bearer {token}"))
        })
}

pub(crate) fn err(status: StatusCode, message: &str) -> axum::response::Response {
    let code = match message {
        "address pool exhausted" => "address_pool_exhausted",
        "setup required" => "setup_required",
        "setup already completed" => "already_configured",
        "database error" => "database_error",
        "resource is in use" => "resource_in_use",
        "invalid reference" | "unknown group" => "invalid_reference",
        _ => match status {
            StatusCode::UNAUTHORIZED => "unauthorized",
            StatusCode::NOT_FOUND => "not_found",
            StatusCode::CONFLICT => "conflict",
            StatusCode::SERVICE_UNAVAILABLE => "runtime_unavailable",
            StatusCode::INTERNAL_SERVER_ERROR => "internal_error",
            StatusCode::PAYLOAD_TOO_LARGE => "payload_too_large",
            StatusCode::UNSUPPORTED_MEDIA_TYPE => "unsupported_media_type",
            _ => "invalid_request",
        },
    };
    configuration::failure(status, code, message, None)
}

async fn api_contract(
    axum::extract::State(state): axum::extract::State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let path = request.uri().path();
    let api = path == "/api" || path.starts_with("/api/");
    if api && !matches!(path, "/api/health" | "/api/ready") && !auth(request.headers(), &state) {
        return err(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let mut response = next.run(request).await;
    if api {
        let status = response.status();
        let json = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .is_some_and(|value| value.as_bytes().starts_with(b"application/json"));
        if (status.is_client_error() || status.is_server_error()) && !json {
            // Extractor/routing errors use the same contract without reflecting
            // untrusted JSON values or request credentials into the response.
            response = err(
                status,
                status.canonical_reason().unwrap_or("invalid request"),
            );
        }
        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            "no-store".parse().unwrap(),
        );
    }
    response
}

#[cfg(test)]
pub(crate) async fn reload(state: &AppState) -> bool {
    state.kernel.reload().await.is_ok()
}

pub(crate) fn is_input_error(error: &rusqlite::Error) -> bool {
    matches!(error, rusqlite::Error::ToSqlConversionFailure(source)
        if source.downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::InvalidInput))
}

pub(crate) fn creation_error(error: rusqlite::Error, duplicate: &str) -> axum::response::Response {
    use rusqlite::{Error, ErrorCode};
    let detail = error.to_string();
    let response = if detail.contains("network setup is required") {
        err(StatusCode::CONFLICT, "setup required")
    } else if detail.contains("invalid group reference")
        || detail.contains("invalid target peer reference")
    {
        err(StatusCode::BAD_REQUEST, "invalid reference")
    } else if detail.contains("name must not be empty") {
        err(StatusCode::BAD_REQUEST, "name must not be empty")
    } else if is_input_error(&error) {
        err(StatusCode::BAD_REQUEST, "invalid name")
    } else if matches!(error, Error::SqliteFailure(ref failure, _) if failure.code == ErrorCode::ConstraintViolation)
    {
        err(StatusCode::CONFLICT, duplicate)
    } else {
        err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
    };
    response.into_response()
}

pub(crate) fn delete_error(error: rusqlite::Error) -> axum::response::Response {
    if matches!(error, rusqlite::Error::SqliteFailure(ref failure, _)
        if failure.code == rusqlite::ErrorCode::ConstraintViolation)
    {
        err(StatusCode::CONFLICT, "resource is in use").into_response()
    } else {
        err(StatusCode::INTERNAL_SERVER_ERROR, "database error").into_response()
    }
}

pub(crate) fn activation_failure(store: &Store) -> axum::response::Response {
    configuration::failure(
        StatusCode::SERVICE_UNAVAILABLE,
        "activation_unconfirmed",
        "Configuration saved; runtime reload failed. Check configuration and runtime status before retrying.",
        store.revision().ok(),
    )
}

pub(crate) fn delete_result<'a>(
    result: Result<usize, rusqlite::Error>,
    permit: ReloadPermit<'a>,
    store: &'a Store,
    missing: &'static str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = axum::response::Response> + Send + 'a>> {
    match result {
        Ok(1) => {
            let ack = permit.send();
            Box::pin(async move {
                if ack.wait().await.is_ok() {
                    StatusCode::NO_CONTENT.into_response()
                } else {
                    activation_failure(store)
                }
            })
        }
        Ok(_) => Box::pin(std::future::ready(
            err(StatusCode::NOT_FOUND, missing).into_response(),
        )),
        Err(error) => Box::pin(std::future::ready(delete_error(error))),
    }
}

#[utoipa::path(get, path = "/api/health", responses((status = 200, body = Status)))]
pub async fn health() -> axum::Json<Status> {
    axum::Json(Status { ok: true })
}

#[utoipa::path(get, path = "/api/ready", responses((status = 200, body = Status), (status = 503, body = Status)))]
pub async fn ready(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> impl IntoResponse {
    if state.kernel.is_ready() {
        (StatusCode::OK, axum::Json(Status { ok: true })).into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(Status { ok: false }),
        )
            .into_response()
    }
}

pub fn router(state: AppState) -> axum::Router {
    use axum::{
        routing::{delete, get, put},
        Router,
    };
    Router::new()
        .route("/api/health", get(health))
        .route("/api/ready", get(ready))
        .route("/api/config", get(get_config))
        .route("/api/status", get(get_status))
        .route("/api/policy", put(put_policy))
        .route("/api/setup", get(get_setup).post(post_setup))
        .route("/api/settings", put(put_settings))
        .route("/api/groups", get(list_groups).post(create_group))
        .route("/api/groups/:id", delete(delete_group))
        .route("/api/groups/:id/acl", put(set_acl))
        .route("/api/peers", get(list_peers).post(create_peer))
        .route("/api/peers/:id", delete(delete_peer))
        .route("/api/peers/:id/group", put(move_peer))
        .route("/api/forwards", get(list_forwards).post(create_forward))
        .route("/api/forwards/:id", delete(delete_forward))
        .fallback(crate::static_assets::handler)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            api_contract,
        ))
        .with_state(state)
}

pub(crate) fn uuid() -> String {
    let mut bytes = [0; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn openapi() -> String {
    use utoipa::OpenApi;
    #[derive(OpenApi)]
    #[openapi(
        paths(
            health,
            ready,
            configuration::get_config,
            configuration::get_status,
            configuration::put_policy,
            settings::get_setup,
            settings::post_setup,
            settings::put_settings,
            groups::list_groups,
            groups::create_group,
            groups::delete_group,
            groups::set_acl,
            peers::list_peers,
            peers::create_peer,
            peers::delete_peer,
            peers::move_peer,
            forwards::list_forwards,
            forwards::create_forward,
            forwards::delete_forward
        ),
        components(schemas(
            NetworkSettings,
            SetupStatus,
            SetupRequest,
            SettingsRequest,
            Group,
            Peer,
            PeerStatus,
            NewGroup,
            NewPeer,
            PeerProvision,
            MovePeer,
            SetAcl,
            Status,
            Forward,
            NewForward,
            Configuration,
            PolicyChange,
            PolicyRequest,
            PolicyResult,
            ErrorResponse,
            RuntimeStatus
        ))
    )]
    struct Doc;
    let mut document = serde_json::to_value(Doc::openapi()).unwrap();
    document["components"]["securitySchemes"] = serde_json::json!({
        "adminBearer": {"type":"http", "scheme":"bearer", "description":"Exact configured WIREHUB_ADMIN_TOKEN. Keep it outside URLs and logs."}
    });
    for (path, item) in document["paths"].as_object_mut().unwrap() {
        for (method, operation) in item.as_object_mut().unwrap() {
            if !matches!(method.as_str(), "get" | "post" | "put" | "delete") {
                continue;
            }
            if matches!(path.as_str(), "/api/health" | "/api/ready") {
                continue;
            }
            operation["security"] = serde_json::json!([{"adminBearer":[]}]);
            let mut errors = vec![
                (401, "Authentication required"),
                (500, "Persistence failure"),
            ];
            if method != "get" {
                errors.extend([
                    (400, "Invalid input"),
                    (409, "Conflict"),
                    (503, "Runtime unavailable or activation unconfirmed"),
                ]);
                if method != "delete" {
                    errors.extend([
                        (413, "Body too large"),
                        (415, "JSON content type required"),
                        (422, "Invalid JSON body"),
                    ]);
                }
                if path.contains("{id}") {
                    errors.push((404, "Resource missing"));
                }
            }
            if path.ends_with("/acl") {
                errors.push((428, "If-Match revision required"));
                operation["parameters"] = serde_json::json!([
                    {"name":"id","in":"path","required":true,"schema":{"type":"string"}},
                    {"name":"If-Match","in":"header","required":true,"description":"Quoted nonnegative configuration revision from GET /api/config. Stale revisions return 409.","schema":{"type":"string"}}
                ]);
            }
            for (status, description) in errors {
                operation["responses"][status.to_string()] = serde_json::json!({"description":description,"content":{"application/json":{"schema":{"$ref":"#/components/schemas/ErrorResponse"}}}});
            }
        }
    }
    serde_json::to_string_pretty(&document).unwrap()
}

#[cfg(test)]
#[path = "api/tests.rs"]
mod tests;
