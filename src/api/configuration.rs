use crate::{
    api::{auth, err, AppState},
    kernel::ReloadPermit,
    model::{ErrorResponse, PolicyChange, PolicyRequest, PolicyResult, RuntimeStatus},
    storage::PolicyError,
};
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

pub(crate) fn failure(
    status: StatusCode,
    code: &str,
    message: &str,
    revision: Option<i64>,
) -> Response {
    let mut response = (
        status,
        Json(ErrorResponse {
            code: code.into(),
            message: message.into(),
            persisted_revision: revision,
        }),
    )
        .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}

pub(crate) fn policy_failure(error: PolicyError) -> Response {
    match error {
        PolicyError::Conflict { revision } => failure(
            StatusCode::CONFLICT,
            "revision_conflict",
            "Configuration changed. Reload and review before saving.",
            Some(revision),
        ),
        PolicyError::MissingGroup => failure(
            StatusCode::NOT_FOUND,
            "group_not_found",
            "group not found",
            None,
        ),
        PolicyError::Invalid(message) => {
            failure(StatusCode::BAD_REQUEST, "invalid_policy", &message, None)
        }
        PolicyError::Database(_) => failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            "database error",
            None,
        ),
    }
}

pub(crate) async fn apply_policy(
    state: &AppState,
    permit: ReloadPermit<'_>,
    expected: i64,
    changes: &[PolicyChange],
) -> Result<PolicyResult, Response> {
    let mut result = state
        .store
        .set_policy(expected, changes)
        .map_err(policy_failure)?;
    // Sending remains synchronous with the commit: request cancellation cannot
    // leave a persisted policy without its queued activation command.
    match permit.send_at(result.revision).wait_revision().await {
        Ok(revision) => { result.applied_revision = Some(revision); Ok(result) }
        Err(_) => Err(failure(StatusCode::SERVICE_UNAVAILABLE, "activation_unconfirmed", "Configuration saved; runtime activation is unconfirmed. Check runtime status before retrying.", Some(result.revision))),
    }
}

#[utoipa::path(get, path = "/api/config", responses((status = 200, body = Configuration), (status = 401), (status = 500, body = ErrorResponse)))]
pub async fn get_config(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !auth(&headers, &state) {
        return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    match state.store.configuration() {
        Ok(config) => {
            let revision = config.revision;
            let mut response = Json(config).into_response();
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
            response
                .headers_mut()
                .insert(header::ETAG, format!("\"{revision}\"").parse().unwrap());
            response
        }
        Err(_) => failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            "database error",
            None,
        ),
    }
}

#[utoipa::path(put, path = "/api/policy", request_body = PolicyRequest, responses((status = 200, body = PolicyResult), (status = 400, body = ErrorResponse), (status = 401), (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse), (status = 500, body = ErrorResponse), (status = 503, body = ErrorResponse)))]
pub async fn put_policy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PolicyRequest>,
) -> Response {
    if !auth(&headers, &state) {
        return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let permit = match state.kernel.reserve_reload().await {
        Ok(permit) => permit,
        Err(_) => {
            return failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "runtime_unavailable",
                "Runtime is unavailable; configuration was not changed.",
                None,
            )
        }
    };
    match apply_policy(&state, permit, request.expected_revision, &request.changes).await {
        Ok(result) => Json(result).into_response(),
        Err(response) => response,
    }
}

#[utoipa::path(get, path = "/api/status", responses((status = 200, body = RuntimeStatus), (status = 401), (status = 500, body = ErrorResponse)))]
pub async fn get_status(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !auth(&headers, &state) {
        return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let activation = state.kernel.activation().await;
    let ready = state.kernel.is_ready();
    let persisted_revision = match state.store.revision() {
        Ok(revision) => revision,
        Err(_) => {
            return failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "database error",
                None,
            )
        }
    };
    let mut response = Json(RuntimeStatus {
        ready,
        persisted_revision,
        applied_revision: activation.applied_revision,
        last_activation_error: activation.last_error,
    })
    .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}

pub(crate) enum RevisionPreconditionError {
    Missing,
    Invalid,
}
impl IntoResponse for RevisionPreconditionError {
    fn into_response(self) -> Response {
        match self {
            Self::Missing => failure(
                StatusCode::PRECONDITION_REQUIRED,
                "revision_required",
                "Read /api/config and supply its revision in If-Match.",
                None,
            ),
            Self::Invalid => failure(
                StatusCode::BAD_REQUEST,
                "invalid_revision",
                "If-Match must contain a quoted configuration revision.",
                None,
            ),
        }
    }
}
pub(crate) fn expected_revision(headers: &HeaderMap) -> Result<i64, RevisionPreconditionError> {
    let value = headers
        .get(header::IF_MATCH)
        .ok_or(RevisionPreconditionError::Missing)?;
    value
        .to_str()
        .ok()
        .and_then(|value| {
            value
                .strip_prefix('"')?
                .strip_suffix('"')?
                .parse::<i64>()
                .ok()
        })
        .filter(|revision| *revision >= 0)
        .ok_or(RevisionPreconditionError::Invalid)
}
