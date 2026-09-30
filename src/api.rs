use std::{sync::Arc, time::Duration};
use axum::{extract::{Path,State}, http::{header, HeaderMap,StatusCode}, response::IntoResponse, Json};
use rand::{rngs::OsRng,RngCore};
use boringtun::x25519::{PublicKey,StaticSecret};
use crate::{model::*,storage::Store,transport::{ReloadCommand,RuntimeStats}};

#[derive(Clone)] pub struct AppState { pub store:Arc<Store>, pub token:Option<String>, pub hub_public:String, pub reload_tx:tokio::sync::mpsc::Sender<ReloadCommand>, pub runtime_stats:RuntimeStats }
fn auth(h:&HeaderMap,s:&AppState)->bool { s.token.as_ref().filter(|t|!t.trim().is_empty()).is_some_and(|t|h.get("authorization").and_then(|v|v.to_str().ok()).is_some_and(|v|v==format!("Bearer {t}"))) }
fn err(code:StatusCode,msg:&str)->impl IntoResponse {(code,msg.to_string())}
async fn reload(s:&AppState)->bool { let (ack,wait)=tokio::sync::oneshot::channel(); if tokio::time::timeout(Duration::from_secs(3),s.reload_tx.send(ReloadCommand{ack})).await.is_err(){return false} matches!(tokio::time::timeout(Duration::from_secs(3),wait).await,Ok(Ok(Ok(())))) }
fn is_input_error(error:&rusqlite::Error)->bool {matches!(error,rusqlite::Error::ToSqlConversionFailure(source) if source.downcast_ref::<std::io::Error>().is_some_and(|e|e.kind()==std::io::ErrorKind::InvalidInput))}
fn creation_error(error:rusqlite::Error,duplicate:&str)->axum::response::Response {if error.to_string().contains("network setup is required"){err(StatusCode::CONFLICT,"setup required").into_response()}else if is_input_error(&error){err(StatusCode::BAD_REQUEST,"invalid name").into_response()}else if matches!(error,rusqlite::Error::SqliteFailure(ref e,_) if e.code==rusqlite::ErrorCode::ConstraintViolation){err(StatusCode::CONFLICT,duplicate).into_response()}else{err(StatusCode::INTERNAL_SERVER_ERROR,"database error").into_response()}}
#[utoipa::path(get,path="/api/setup",responses((status=200,body=SetupStatus),(status=401)))]
pub async fn get_setup(State(s):State<AppState>,h:HeaderMap)->impl IntoResponse {if !auth(&h,&s){return err(StatusCode::UNAUTHORIZED,"unauthorized").into_response()}match s.store.network_settings(){Ok(settings)=>Json(SetupStatus{configured:settings.is_some(),settings}).into_response(),Err(_)=>err(StatusCode::INTERNAL_SERVER_ERROR,"database error").into_response()}}
#[utoipa::path(post,path="/api/setup",request_body=SetupRequest,responses((status=200,body=NetworkSettings),(status=400),(status=409),(status=401)))]
pub async fn post_setup(State(s):State<AppState>,h:HeaderMap,Json(request):Json<SetupRequest>)->impl IntoResponse {if !auth(&h,&s){return err(StatusCode::UNAUTHORIZED,"unauthorized").into_response()}match s.store.setup(&request.subnet,&request.endpoint,request.persistent_keepalive){Ok(settings)=>{if reload(&s).await{Json(settings).into_response()}else{err(StatusCode::SERVICE_UNAVAILABLE,"settings saved, but runtime activation was not acknowledged; inspect setup status and restart the service before provisioning").into_response()}},Err(e)=>{if e.to_string().contains("already") {err(StatusCode::CONFLICT,"setup already completed").into_response()}else if is_input_error(&e){err(StatusCode::BAD_REQUEST,"invalid setup settings").into_response()}else{err(StatusCode::INTERNAL_SERVER_ERROR,"database error").into_response()}}}}
#[utoipa::path(put,path="/api/settings",request_body=SettingsRequest,responses((status=200,body=NetworkSettings),(status=400),(status=409),(status=401)))]
pub async fn put_settings(State(s):State<AppState>,h:HeaderMap,Json(request):Json<SettingsRequest>)->impl IntoResponse {if !auth(&h,&s){return err(StatusCode::UNAUTHORIZED,"unauthorized").into_response()}match s.store.update_settings(&request.endpoint,request.persistent_keepalive){Ok(settings)=>Json(settings).into_response(),Err(e)=>{if e.to_string().contains("network setup is required"){err(StatusCode::CONFLICT,"setup required").into_response()}else if is_input_error(&e){err(StatusCode::BAD_REQUEST,"invalid settings").into_response()}else{err(StatusCode::INTERNAL_SERVER_ERROR,"database error").into_response()}}}}
#[utoipa::path(get,path="/api/health",responses((status=200,body=Status)))]
pub async fn health()->Json<Status>{Json(Status{ok:true})}
#[utoipa::path(get,path="/api/groups",responses((status=200,body=[Group])))]
pub async fn list_groups(State(s):State<AppState>,h:HeaderMap)->impl IntoResponse {if !auth(&h,&s){return err(StatusCode::UNAUTHORIZED,"unauthorized").into_response()} match s.store.groups(){Ok(x)=>Json(x).into_response(),Err(_)=>err(StatusCode::INTERNAL_SERVER_ERROR,"database error").into_response()}}
#[utoipa::path(post,path="/api/groups",request_body=NewGroup,responses((status=201,body=Group)))]
pub async fn create_group(State(s):State<AppState>,h:HeaderMap,Json(n):Json<NewGroup>)->impl IntoResponse {if !auth(&h,&s){return err(StatusCode::UNAUTHORIZED,"unauthorized").into_response()} let g=Group{id:uuid(),name:n.name.trim().to_owned(),allowed_groups:vec![]};match s.store.add_group(&g){Ok(())=>if reload(&s).await{(StatusCode::CREATED,Json(g)).into_response()}else{err(StatusCode::SERVICE_UNAVAILABLE,"runtime reload failed").into_response()},Err(e)=>creation_error(e,"group conflict").into_response()}}
#[utoipa::path(delete,path="/api/groups/{id}",responses((status=204)))]
pub async fn delete_group(State(s):State<AppState>,h:HeaderMap,Path(id):Path<String>)->impl IntoResponse {if !auth(&h,&s){return err(StatusCode::UNAUTHORIZED,"unauthorized").into_response()} match s.store.remove_group(&id){Ok(1)=>if reload(&s).await{StatusCode::NO_CONTENT.into_response()}else{err(StatusCode::SERVICE_UNAVAILABLE,"runtime reload failed").into_response()},Ok(_)=>err(StatusCode::NOT_FOUND,"group not found").into_response(),Err(e)=>delete_error(e).into_response()}}
#[utoipa::path(put,path="/api/groups/{id}/acl",request_body=SetAcl,responses((status=200,body=Group)))]
pub async fn set_acl(State(s):State<AppState>,h:HeaderMap,Path(id):Path<String>,Json(a):Json<SetAcl>)->impl IntoResponse {if !auth(&h,&s){return err(StatusCode::UNAUTHORIZED,"unauthorized").into_response()} for g in &a.allowed_groups {match s.store.group(g){Ok(Some(_))=>{},Ok(None)=>return err(StatusCode::BAD_REQUEST,"unknown group").into_response(),Err(_)=>return err(StatusCode::INTERNAL_SERVER_ERROR,"database error").into_response()}} match s.store.set_acl(&id,&a.allowed_groups){Ok(1)=>if !reload(&s).await{err(StatusCode::SERVICE_UNAVAILABLE,"runtime reload failed").into_response()}else{match s.store.group(&id){Ok(Some(g))=>Json(g).into_response(),Ok(None)=>err(StatusCode::NOT_FOUND,"group not found").into_response(),Err(_)=>err(StatusCode::INTERNAL_SERVER_ERROR,"database error").into_response()}},Ok(_)=>err(StatusCode::NOT_FOUND,"group not found").into_response(),Err(_)=>err(StatusCode::INTERNAL_SERVER_ERROR,"database error").into_response()}}
#[utoipa::path(get,path="/api/peers",responses((status=200,body=[PeerStatus])))]
pub async fn list_peers(State(s):State<AppState>,h:HeaderMap)->impl IntoResponse {if !auth(&h,&s){return err(StatusCode::UNAUTHORIZED,"unauthorized").into_response()} match s.store.peers(){Ok(p)=>{let stats=s.runtime_stats.read().await;let values:Vec<_>=p.into_iter().map(|mut peer|{let mut last_data_unix=None;if let Some((rx,tx,handshake,activity))=stats.get(&peer.id){peer.received_bytes=*rx;peer.sent_bytes=*tx;peer.last_handshake_unix=*handshake;last_data_unix=*activity;}PeerStatus{id:peer.id,name:peer.name,public_key:peer.public_key,ipv4:peer.ipv4,group_id:peer.group_id,received_bytes:peer.received_bytes,sent_bytes:peer.sent_bytes,last_handshake_unix:peer.last_handshake_unix,last_data_unix}}).collect();Json(values).into_response()},Err(_)=>err(StatusCode::INTERNAL_SERVER_ERROR,"database error").into_response()}}
#[utoipa::path(post,path="/api/peers",request_body=NewPeer,responses((status=201,body=PeerProvision)))]
pub async fn create_peer(State(s):State<AppState>,h:HeaderMap,Json(n):Json<NewPeer>)->impl IntoResponse {if !auth(&h,&s){return err(StatusCode::UNAUTHORIZED,"unauthorized").into_response()} match s.store.network_settings(){Ok(Some(_))=>{},Ok(None)=>return err(StatusCode::CONFLICT,"setup required").into_response(),Err(_)=>return err(StatusCode::INTERNAL_SERVER_ERROR,"database error").into_response()} match s.store.group(&n.group_id){Ok(Some(_))=>{},Ok(None)=>return err(StatusCode::BAD_REQUEST,"unknown group").into_response(),Err(_)=>return err(StatusCode::INTERNAL_SERVER_ERROR,"database error").into_response()} let mut private=[0u8;32];OsRng.fill_bytes(&mut private);let secret=StaticSecret::from(private);let public=PublicKey::from(&secret);let key=base64::Engine::encode(&base64::engine::general_purpose::STANDARD,secret.to_bytes()); let pk=base64::Engine::encode(&base64::engine::general_purpose::STANDARD,public.as_bytes()); let mut p=Peer{id:uuid(),name:n.name,public_key:pk,ipv4:String::new(),group_id:n.group_id,received_bytes:0,sent_bytes:0,last_handshake_unix:None}; match s.store.create_peer_allocated(&mut p){Ok(true)=>{},Ok(false)=>return err(StatusCode::CONFLICT,"address pool exhausted").into_response(),Err(e)=>return creation_error(e,"peer conflict").into_response()} if !reload(&s).await{
    let cleanup=matches!(s.store.remove_peer(&p.id),Ok(1)) && reload(&s).await;
    let message=if cleanup {"runtime reload failed; peer removal and cleanup reload acknowledged"} else {"runtime reload failed; provisioning state is uncertain; inspect peer inventory before retrying"};
    let mut response=err(StatusCode::SERVICE_UNAVAILABLE,message).into_response();
    response.headers_mut().insert(header::CACHE_CONTROL,header::HeaderValue::from_static("no-store"));
    return response;
    } let settings=match s.store.network_settings(){Ok(Some(v))=>v, _=>return err(StatusCode::INTERNAL_SERVER_ERROR,"database error").into_response()}; let config=format!("[Interface]\nPrivateKey = {key}\nAddress = {}/32\n\n[Peer]\nPublicKey = {}\nEndpoint = {}\nAllowedIPs = {}\nPersistentKeepalive = {}\n", p.ipv4,s.hub_public,settings.endpoint,settings.subnet,settings.persistent_keepalive); let mut response=(StatusCode::CREATED,Json(PeerProvision{peer:p,config})).into_response();response.headers_mut().insert(header::CACHE_CONTROL,header::HeaderValue::from_static("no-store"));response }
#[utoipa::path(delete,path="/api/peers/{id}",responses((status=204)))]
pub async fn delete_peer(State(s):State<AppState>,h:HeaderMap,Path(id):Path<String>)->impl IntoResponse {if !auth(&h,&s){return err(StatusCode::UNAUTHORIZED,"unauthorized").into_response()}match s.store.remove_peer(&id){Ok(1)=>if reload(&s).await{StatusCode::NO_CONTENT.into_response()}else{err(StatusCode::SERVICE_UNAVAILABLE,"runtime reload failed").into_response()},Ok(_)=>err(StatusCode::NOT_FOUND,"peer not found").into_response(),Err(e)=>delete_error(e).into_response()}}
#[utoipa::path(put,path="/api/peers/{id}/group",request_body=MovePeer,responses((status=200,body=Peer)))]
pub async fn move_peer(State(s):State<AppState>,h:HeaderMap,Path(id):Path<String>,Json(m):Json<MovePeer>)->impl IntoResponse {if !auth(&h,&s){return err(StatusCode::UNAUTHORIZED,"unauthorized").into_response()}match s.store.group(&m.group_id){Ok(Some(_))=>{},Ok(None)=>return err(StatusCode::BAD_REQUEST,"unknown group").into_response(),Err(_)=>return err(StatusCode::INTERNAL_SERVER_ERROR,"database error").into_response()}match s.store.move_peer(&id,&m.group_id){Ok(1)=>if !reload(&s).await{err(StatusCode::SERVICE_UNAVAILABLE,"runtime reload failed").into_response()}else{match s.store.peers(){Ok(peers)=>match peers.into_iter().find(|p|p.id==id){Some(p)=>Json(p).into_response(),None=>err(StatusCode::NOT_FOUND,"peer not found").into_response()},Err(_)=>err(StatusCode::INTERNAL_SERVER_ERROR,"database error").into_response()}},Ok(_)=>err(StatusCode::NOT_FOUND,"peer not found").into_response(),Err(_)=>err(StatusCode::INTERNAL_SERVER_ERROR,"database error").into_response()}}
#[utoipa::path(get,path="/api/forwards",responses((status=200,body=[Forward])))]
pub async fn list_forwards(State(s):State<AppState>,h:HeaderMap)->impl IntoResponse {if !auth(&h,&s){return err(StatusCode::UNAUTHORIZED,"unauthorized").into_response()}match s.store.forwards(){Ok(v)=>Json(v).into_response(),Err(_)=>err(StatusCode::INTERNAL_SERVER_ERROR,"database error").into_response()}}
#[utoipa::path(post,path="/api/forwards",request_body=NewForward,responses((status=201,body=Forward)))]
pub async fn create_forward(State(s):State<AppState>,h:HeaderMap,Json(n):Json<NewForward>)->impl IntoResponse {if !auth(&h,&s){return err(StatusCode::UNAUTHORIZED,"unauthorized").into_response()}match s.store.network_settings(){Ok(Some(_))=>{},Ok(None)=>return err(StatusCode::CONFLICT,"setup required").into_response(),Err(_)=>return err(StatusCode::INTERNAL_SERVER_ERROR,"database error").into_response()}if !matches!(n.protocol.as_str(),"tcp"|"udp")||n.target_port==0{return err(StatusCode::BAD_REQUEST,"invalid protocol or port").into_response()}let peers=match s.store.peers(){Ok(v)=>v,Err(_)=>return err(StatusCode::INTERNAL_SERVER_ERROR,"database error").into_response()};let Some(target)=peers.iter().find(|p|p.id==n.target_peer_id)else{return err(StatusCode::BAD_REQUEST,"unknown target peer").into_response()};for group in &n.allowed_group_ids{match s.store.group(group){Ok(Some(_))=>{},Ok(None)=>return err(StatusCode::BAD_REQUEST,"unknown allowed group").into_response(),Err(_)=>return err(StatusCode::INTERNAL_SERVER_ERROR,"database error").into_response()}}let mut f=Forward{id:uuid(),name:n.name,protocol:n.protocol,target_peer_id:target.id.clone(),target_port:n.target_port,allowed_group_ids:n.allowed_group_ids};match s.store.create_forward(&mut f){Ok(())=>if reload(&s).await{(StatusCode::CREATED,Json(f)).into_response()}else{err(StatusCode::SERVICE_UNAVAILABLE,"runtime reload failed").into_response()},Err(e)=>creation_error(e,"forward conflict").into_response()}}
#[utoipa::path(delete,path="/api/forwards/{id}",responses((status=204)))]
pub async fn delete_forward(State(s):State<AppState>,h:HeaderMap,Path(id):Path<String>)->impl IntoResponse {if !auth(&h,&s){return err(StatusCode::UNAUTHORIZED,"unauthorized").into_response()}match s.store.remove_forward(&id){Ok(1)=>if reload(&s).await{StatusCode::NO_CONTENT.into_response()}else{err(StatusCode::SERVICE_UNAVAILABLE,"runtime reload failed").into_response()},Ok(_)=>err(StatusCode::NOT_FOUND,"forward not found").into_response(),Err(e)=>delete_error(e).into_response()}}
fn delete_error(error:rusqlite::Error)->axum::response::Response {if matches!(error,rusqlite::Error::SqliteFailure(ref e,_) if e.code==rusqlite::ErrorCode::ConstraintViolation){err(StatusCode::CONFLICT,"resource is in use").into_response()}else{err(StatusCode::INTERNAL_SERVER_ERROR,"database error").into_response()}}
fn uuid()->String{let mut b=[0;16];OsRng.fill_bytes(&mut b);hex::encode(b)}
pub fn openapi()->String { use utoipa::OpenApi; #[derive(OpenApi)] #[openapi(paths(health,get_setup,post_setup,put_settings,list_groups,create_group,delete_group,set_acl,list_peers,create_peer,delete_peer,move_peer,list_forwards,create_forward,delete_forward),components(schemas(NetworkSettings,SetupStatus,SetupRequest,SettingsRequest,Group,Peer,PeerStatus,NewGroup,NewPeer,PeerProvision,MovePeer,SetAcl,Status,Forward,NewForward)))] struct Doc; Doc::openapi().to_pretty_json().unwrap() }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authentication_fails_closed_for_missing_or_empty_token() {
        let (reload_tx,_) = tokio::sync::mpsc::channel(1);let state=AppState{store:Arc::new(Store::open(":memory:").unwrap()),token:None,hub_public:String::new(),reload_tx,runtime_stats:RuntimeStats::default()};
        assert!(!auth(&HeaderMap::new(),&state));
        let state=AppState{token:Some(String::new()),..state};assert!(!auth(&HeaderMap::new(),&state));
    }
    #[test]
    fn authentication_requires_exact_bearer_token() {
        let (reload_tx,_) = tokio::sync::mpsc::channel(1);let state=AppState{store:Arc::new(Store::open(":memory:").unwrap()),token:Some("secret".into()),hub_public:String::new(),reload_tx,runtime_stats:RuntimeStats::default()};
        assert!(!auth(&HeaderMap::new(),&state));let mut headers=HeaderMap::new();headers.insert("authorization","Bearer secret".parse().unwrap());assert!(auth(&headers,&state));
    }
    #[tokio::test]
    async fn provisioning_uses_standard_wireguard_base64_without_persisting_private_key() {
        use base64::Engine;
        let store=Arc::new(Store::open(":memory:").unwrap());
        store.setup("192.168.44.0/24","hub.example:51820",25).unwrap();
        store.add_group(&Group{id:"g".into(),name:"group".into(),allowed_groups:vec![]}).unwrap();
        let (reload_tx,mut reload_rx)=tokio::sync::mpsc::channel::<ReloadCommand>(1);tokio::spawn(async move{if let Some(command)=reload_rx.recv().await{let _=command.ack.send(Ok(()));}});
        let state=AppState{store:store.clone(),token:Some("secret".into()),hub_public:"hub-public".into(),reload_tx,runtime_stats:RuntimeStats::default()};
        let mut headers=HeaderMap::new();headers.insert("authorization","Bearer secret".parse().unwrap());
        let response=create_peer(State(state),headers,Json(NewPeer{name:"client".into(),group_id:"g".into()})).await.into_response();
        assert_eq!(response.status(),StatusCode::CREATED);
        assert_eq!(response.headers().get(header::CACHE_CONTROL).unwrap(),"no-store");
        let body=axum::body::to_bytes(response.into_body(),usize::MAX).await.unwrap();
        #[derive(serde::Deserialize)] struct ProvisionBody { peer:Peer, config:String }
        let provision:ProvisionBody=serde_json::from_slice(&body).unwrap();
        assert_eq!(provision.peer.ipv4,"192.168.44.2");
        assert!(provision.config.contains("Address = 192.168.44.2/32\n"));
        assert!(provision.config.contains("Endpoint = hub.example:51820\n"));
        assert!(provision.config.contains("AllowedIPs = 192.168.44.0/24\n"));
        assert!(provision.config.contains("PersistentKeepalive = 25\n"));
        assert_eq!(provision.config.lines().filter(|line|line.starts_with("AllowedIPs = ")).count(),1);
        let encoded_public=provision.peer.public_key.clone();
        let public_bytes=base64::engine::general_purpose::STANDARD.decode(&encoded_public).unwrap();
        assert_eq!(public_bytes.len(),32);
        let stored=store.peers().unwrap();
        assert_eq!(stored.len(),1);
        assert_eq!(stored[0].public_key,encoded_public);
        let private_key=provision.config.lines().find_map(|line|line.strip_prefix("PrivateKey = ")).unwrap();
        let private_bytes=base64::engine::general_purpose::STANDARD.decode(private_key).unwrap();
        assert_eq!(private_bytes.len(),32);
        assert!(!serde_json::to_string(&stored).unwrap().contains(private_key));
        assert!(!stored[0].public_key.contains(private_key));
    }
    #[tokio::test]
    async fn failed_setup_activation_reports_persisted_settings() {
        let store=Arc::new(Store::open(":memory:").unwrap());
        let (reload_tx,mut reload_rx)=tokio::sync::mpsc::channel::<ReloadCommand>(1);
        tokio::spawn(async move { if let Some(command)=reload_rx.recv().await { let _=command.ack.send(Err(())); } });
        let state=AppState{store:store.clone(),token:Some("secret".into()),hub_public:String::new(),reload_tx,runtime_stats:RuntimeStats::default()};
        let mut headers=HeaderMap::new();headers.insert("authorization","Bearer secret".parse().unwrap());
        let response=post_setup(State(state),headers,Json(SetupRequest{subnet:"172.23.45.0/24".into(),endpoint:"hub.example:51820".into(),persistent_keepalive:25})).await.into_response();
        assert_eq!(response.status(),StatusCode::SERVICE_UNAVAILABLE);
        let body=axum::body::to_bytes(response.into_body(),usize::MAX).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("settings saved"));
        assert_eq!(store.network_settings().unwrap().unwrap().subnet,"172.23.45.0/24");
    }
}
