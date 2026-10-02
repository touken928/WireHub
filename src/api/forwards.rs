use axum::{extract::{Path, State}, http::{HeaderMap, StatusCode}, response::IntoResponse, Json};
use crate::{api::{auth, creation_error, delete_result, err, reload, uuid, AppState}, model::{Forward, NewForward}};

#[utoipa::path(get, path = "/api/forwards", tag = "crate", responses((status = 200, body = [Forward])))]
pub async fn list_forwards(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if !auth(&headers, &state) { return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response(); }
    match state.store.forwards() {
        Ok(forwards) => Json(forwards).into_response(),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "database error").into_response(),
    }
}

#[utoipa::path(post, path = "/api/forwards", tag = "crate", request_body = NewForward, responses((status = 201, body = Forward)))]
pub async fn create_forward(State(state): State<AppState>, headers: HeaderMap, Json(new): Json<NewForward>) -> impl IntoResponse {
    if !auth(&headers, &state) { return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response(); }
    if !matches!(new.protocol.as_str(), "tcp" | "udp") || new.target_port == 0 {
        return err(StatusCode::BAD_REQUEST, "invalid protocol or port").into_response();
    }
    let mut forward = Forward {
        id: uuid(), name: new.name, target_peer_id: new.target_peer_id,
        protocol: new.protocol, target_port: new.target_port,
        allowed_group_ids: new.allowed_group_ids,
    };
    match state.store.create_forward(&mut forward) {
        Ok(()) if reload(&state).await => (StatusCode::CREATED, Json(forward)).into_response(),
        Ok(()) => err(StatusCode::SERVICE_UNAVAILABLE, "runtime reload failed").into_response(),
        Err(error) => creation_error(error, "forward conflict"),
    }
}

#[utoipa::path(delete, path = "/api/forwards/{id}", tag = "crate", responses((status = 204)))]
pub async fn delete_forward(State(state): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> impl IntoResponse {
    if !auth(&headers, &state) { return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response(); }
    delete_result(state.store.remove_forward(&id), &state, "forward not found").await
}
