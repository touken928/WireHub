use axum::{extract::{Path, State}, http::{header, HeaderMap, StatusCode}, response::IntoResponse, Json};
use boringtun::x25519::{PublicKey, StaticSecret};
use rand::{rngs::OsRng, RngCore};
use crate::{api::{auth, creation_error, delete_result, err, is_input_error, reload, uuid, AppState}, model::{MovePeer, NewPeer, Peer, PeerProvision, PeerStatus}};

#[utoipa::path(get, path = "/api/peers", tag = "crate", responses((status = 200, body = [PeerStatus])))]
pub async fn list_peers(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if !auth(&headers, &state) { return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response(); }
    match state.store.peers() {
        Ok(peers) => {
            let stats = state.runtime_stats.read().await;
            let peers = peers.into_iter().map(|mut peer| {
                let mut last_data_unix = None;
                if let Some((received, sent, handshake, activity)) = stats.get(&peer.id) {
                    peer.received_bytes = *received;
                    peer.sent_bytes = *sent;
                    peer.last_handshake_unix = *handshake;
                    last_data_unix = *activity;
                }
                PeerStatus {
                    id: peer.id, name: peer.name, public_key: peer.public_key,
                    ipv4: peer.ipv4, group_id: peer.group_id,
                    received_bytes: peer.received_bytes, sent_bytes: peer.sent_bytes,
                    last_handshake_unix: peer.last_handshake_unix, last_data_unix,
                }
            }).collect::<Vec<_>>();
            Json(peers).into_response()
        }
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "database error").into_response(),
    }
}

fn render_config(private_key: &str, peer: &Peer, state: &AppState, settings: &crate::model::NetworkSettings) -> String {
    format!(
        "[Interface]\nPrivateKey = {private_key}\nAddress = {}/32\n\n[Peer]\nPublicKey = {}\nEndpoint = {}\nAllowedIPs = {}\nPersistentKeepalive = {}\n",
        peer.ipv4, state.hub_public, settings.endpoint, settings.subnet, settings.persistent_keepalive,
    )
}

async fn cleanup_failed_provision(state: &AppState, peer_id: &str) -> bool {
    matches!(state.store.remove_peer(peer_id), Ok(1)) && reload(state).await
}

#[utoipa::path(post, path = "/api/peers", tag = "crate", request_body = NewPeer, responses((status = 201, body = PeerProvision)))]
pub async fn create_peer(State(state): State<AppState>, headers: HeaderMap, Json(new): Json<NewPeer>) -> impl IntoResponse {
    if !auth(&headers, &state) { return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response(); }

    let mut private = [0u8; 32];
    OsRng.fill_bytes(&mut private);
    let secret = StaticSecret::from(private);
    let public = PublicKey::from(&secret);
    let private_key = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, secret.to_bytes());
    let public_key = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, public.as_bytes());
    let mut peer = Peer {
        id: uuid(), name: new.name, public_key, ipv4: String::new(), group_id: new.group_id,
        received_bytes: 0, sent_bytes: 0, last_handshake_unix: None,
    };
    match state.store.create_peer_allocated(&mut peer) {
        Ok(true) => {}
        Ok(false) => return err(StatusCode::CONFLICT, "address pool exhausted").into_response(),
        Err(error) => return creation_error(error, "peer conflict"),
    }

    if !reload(&state).await {
        let cleanup_succeeded = cleanup_failed_provision(&state, &peer.id).await;
        let message = if cleanup_succeeded {
            "runtime reload failed; peer removal and cleanup reload acknowledged"
        } else {
            "runtime reload failed; provisioning state is uncertain; inspect peer inventory before retrying"
        };
        let mut response = err(StatusCode::SERVICE_UNAVAILABLE, message).into_response();
        response.headers_mut().insert(header::CACHE_CONTROL, header::HeaderValue::from_static("no-store"));
        return response;
    }

    let settings = match state.store.network_settings() {
        Ok(Some(settings)) => settings,
        _ => return err(StatusCode::INTERNAL_SERVER_ERROR, "database error").into_response(),
    };
    let config = render_config(&private_key, &peer, &state, &settings);
    let mut response = (StatusCode::CREATED, Json(PeerProvision { peer, config })).into_response();
    response.headers_mut().insert(header::CACHE_CONTROL, header::HeaderValue::from_static("no-store"));
    response
}

#[utoipa::path(delete, path = "/api/peers/{id}", tag = "crate", responses((status = 204)))]
pub async fn delete_peer(State(state): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> impl IntoResponse {
    if !auth(&headers, &state) { return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response(); }
    delete_result(state.store.remove_peer(&id), &state, "peer not found").await
}

#[utoipa::path(put, path = "/api/peers/{id}/group", tag = "crate", request_body = MovePeer, responses((status = 200, body = Peer)))]
pub async fn move_peer(State(state): State<AppState>, headers: HeaderMap, Path(id): Path<String>, Json(movement): Json<MovePeer>) -> impl IntoResponse {
    if !auth(&headers, &state) { return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response(); }
    match state.store.move_peer(&id, &movement.group_id) {
        Ok(1) => {
            if !reload(&state).await { return err(StatusCode::SERVICE_UNAVAILABLE, "runtime reload failed").into_response(); }
            match state.store.peers() {
                Ok(peers) => match peers.into_iter().find(|peer| peer.id == id) {
                    Some(peer) => Json(peer).into_response(),
                    None => err(StatusCode::NOT_FOUND, "peer not found").into_response(),
                },
                Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "database error").into_response(),
            }
        }
        Ok(_) => err(StatusCode::NOT_FOUND, "peer not found").into_response(),
        Err(error) if is_input_error(&error) => err(StatusCode::BAD_REQUEST, "unknown group").into_response(),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "database error").into_response(),
    }
}
