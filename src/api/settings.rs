use crate::{
    api::{auth, err, is_input_error, AppState},
    model::{SettingsRequest, SetupRequest, SetupStatus},
};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};

#[utoipa::path(get, path = "/api/setup", tag = "crate", responses((status = 200, body = SetupStatus), (status = 401)))]
pub async fn get_setup(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if !auth(&headers, &state) {
        return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    match state.store.network_settings() {
        Ok(settings) => Json(SetupStatus {
            configured: settings.is_some(),
            settings,
        })
        .into_response(),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "database error").into_response(),
    }
}

#[utoipa::path(post, path = "/api/setup", tag = "crate", request_body = SetupRequest, responses((status = 200, body = NetworkSettings), (status = 400), (status = 409), (status = 401)))]
pub async fn post_setup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SetupRequest>,
) -> impl IntoResponse {
    if !auth(&headers, &state) {
        return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let permit = match state.kernel.reserve_reload().await {
        Ok(permit) => permit,
        Err(_) => {
            return err(StatusCode::SERVICE_UNAVAILABLE, "runtime reload failed").into_response()
        }
    };
    let (settings, revision) = match state.store.setup_versioned(
        &request.subnet,
        &request.endpoint,
        request.persistent_keepalive,
    ) {
        Ok(settings) => settings,
        Err(error) => {
            let detail = error.to_string();
            let response = if detail.contains("already") {
                err(StatusCode::CONFLICT, "setup already completed")
            } else if detail.contains("hub identity is required") {
                err(StatusCode::CONFLICT, "hub identity required")
            } else if is_input_error(&error) {
                err(StatusCode::BAD_REQUEST, "invalid setup settings")
            } else {
                err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
            };
            return response.into_response();
        }
    };

    let ack = permit.send_at(revision);
    if ack.wait().await.is_ok() {
        Json(settings).into_response()
    } else {
        super::configuration::failure(StatusCode::SERVICE_UNAVAILABLE, "activation_unconfirmed",
            "settings saved, but runtime activation was not acknowledged; inspect runtime status before provisioning", Some(revision))
            .into_response()
    }
}

#[utoipa::path(put, path = "/api/settings", tag = "crate", request_body = SettingsRequest, responses((status = 200, body = NetworkSettings), (status = 400), (status = 409), (status = 401)))]
pub async fn put_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SettingsRequest>,
) -> impl IntoResponse {
    if !auth(&headers, &state) {
        return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let permit = match state.kernel.reserve_reload().await {
        Ok(permit) => permit,
        Err(_) => {
            return err(StatusCode::SERVICE_UNAVAILABLE, "runtime reload failed").into_response()
        }
    };
    match state
        .store
        .update_settings_versioned(&request.endpoint, request.persistent_keepalive)
    {
        Ok((settings, revision)) => {
            if permit.send_at(revision).wait_revision().await.is_ok() {
                Json(settings).into_response()
            } else {
                super::configuration::failure(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "activation_unconfirmed",
                    "Settings saved; activation is unconfirmed.",
                    Some(revision),
                )
            }
        }
        Err(error) if error.to_string().contains("network setup is required") => {
            err(StatusCode::CONFLICT, "setup required").into_response()
        }
        Err(error) if is_input_error(&error) => {
            err(StatusCode::BAD_REQUEST, "invalid settings").into_response()
        }
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "database error").into_response(),
    }
}
