use axum::{extract::{Path, State}, http::{HeaderMap, StatusCode}, response::IntoResponse, Json};
use crate::{api::{auth, creation_error, delete_result, err, is_input_error, uuid, AppState}, model::{Group, NewGroup, SetAcl}};

#[utoipa::path(get, path = "/api/groups", tag = "crate", responses((status = 200, body = [Group])))]
pub async fn list_groups(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if !auth(&headers, &state) { return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response(); }
    match state.store.groups() {
        Ok(groups) => Json(groups).into_response(),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "database error").into_response(),
    }
}

#[utoipa::path(post, path = "/api/groups", tag = "crate", request_body = NewGroup, responses((status = 201, body = Group)))]
pub async fn create_group(State(state): State<AppState>, headers: HeaderMap, Json(new): Json<NewGroup>) -> impl IntoResponse {
    if !auth(&headers, &state) { return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response(); }
    let permit = match state.kernel.reserve_reload().await {
        Ok(permit) => permit,
        Err(_) => return err(StatusCode::SERVICE_UNAVAILABLE, "runtime reload failed").into_response(),
    };
    let group = Group { id: uuid(), name: new.name.trim().to_owned(), allowed_groups: vec![] };
    match state.store.add_group(&group) {
        Ok(()) => {
            let ack = permit.send();
            if ack.wait().await.is_ok() { (StatusCode::CREATED, Json(group)).into_response() }
            else { err(StatusCode::SERVICE_UNAVAILABLE, "runtime reload failed").into_response() }
        }
        Err(error) => creation_error(error, "group conflict"),
    }
}

#[utoipa::path(delete, path = "/api/groups/{id}", tag = "crate", responses((status = 204)))]
pub async fn delete_group(State(state): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> impl IntoResponse {
    if !auth(&headers, &state) { return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response(); }
    let permit = match state.kernel.reserve_reload().await {
        Ok(permit) => permit,
        Err(_) => return err(StatusCode::SERVICE_UNAVAILABLE, "runtime reload failed").into_response(),
    };
    delete_result(state.store.remove_group(&id), permit, "group not found").await
}

#[utoipa::path(put, path = "/api/groups/{id}/acl", tag = "crate", request_body = SetAcl, responses((status = 200, body = Group)))]
pub async fn set_acl(State(state): State<AppState>, headers: HeaderMap, Path(id): Path<String>, Json(acl): Json<SetAcl>) -> impl IntoResponse {
    if !auth(&headers, &state) { return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response(); }
    let permit = match state.kernel.reserve_reload().await {
        Ok(permit) => permit,
        Err(_) => return err(StatusCode::SERVICE_UNAVAILABLE, "runtime reload failed").into_response(),
    };
    match state.store.set_acl(&id, &acl.allowed_groups) {
        Ok(1) => {
            let ack = permit.send();
            if ack.wait().await.is_err() { return err(StatusCode::SERVICE_UNAVAILABLE, "runtime reload failed").into_response(); }
            match state.store.group(&id) {
                Ok(Some(group)) => Json(group).into_response(),
                Ok(None) => err(StatusCode::NOT_FOUND, "group not found").into_response(),
                Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "database error").into_response(),
            }
        }
        Ok(_) => err(StatusCode::NOT_FOUND, "group not found").into_response(),
        Err(error) if is_input_error(&error) => err(StatusCode::BAD_REQUEST, "unknown group").into_response(),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "database error").into_response(),
    }
}
