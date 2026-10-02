use std::sync::Arc;

use crate::{kernel::KernelHandle, model::*, storage::Store};
use axum::{
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use rand::RngCore;

mod forwards;
mod groups;
mod peers;
mod settings;

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

pub(crate) fn err(status: StatusCode, message: &str) -> impl IntoResponse {
    (status, message.to_owned())
}

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

pub(crate) async fn delete_result(
    result: Result<usize, rusqlite::Error>,
    state: &AppState,
    missing: &'static str,
) -> axum::response::Response {
    match result {
        Ok(1) if reload(state).await => StatusCode::NO_CONTENT.into_response(),
        Ok(1) => err(StatusCode::SERVICE_UNAVAILABLE, "runtime reload failed").into_response(),
        Ok(_) => err(StatusCode::NOT_FOUND, missing).into_response(),
        Err(error) => delete_error(error),
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
            NewForward
        ))
    )]
    struct Doc;
    Doc::openapi().to_pretty_json().unwrap()
}

#[cfg(test)]
#[path = "api/tests.rs"]
mod tests;
