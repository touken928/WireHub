use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NetworkSettings { pub subnet: String, pub endpoint: String, pub persistent_keepalive: u16 }
#[derive(Serialize, ToSchema)]
pub struct SetupStatus { pub configured: bool, pub settings: Option<NetworkSettings> }
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetupRequest { pub subnet: String, pub endpoint: String, pub persistent_keepalive: u32 }
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsRequest { pub endpoint: String, pub persistent_keepalive: u32 }

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct Group { pub id: String, pub name: String, #[serde(default)] pub allowed_groups: Vec<String> }
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct Peer { pub id: String, pub name: String, pub public_key: String, pub ipv4: String, pub group_id: String, pub received_bytes: u64, pub sent_bytes: u64, pub last_handshake_unix: Option<i64> }
#[derive(Serialize, ToSchema)]
pub struct PeerStatus { pub id: String, pub name: String, pub public_key: String, pub ipv4: String, pub group_id: String, pub received_bytes: u64, pub sent_bytes: u64, pub last_handshake_unix: Option<i64>, pub last_data_unix: Option<i64> }
#[derive(Deserialize, ToSchema)]
pub struct NewGroup { pub name: String }
#[derive(Deserialize, ToSchema)]
pub struct NewPeer { pub name: String, pub group_id: String }
#[derive(Serialize, ToSchema)]
pub struct PeerProvision { pub peer: Peer, pub config: String }
#[derive(Deserialize, ToSchema)]
pub struct MovePeer { pub group_id: String }
#[derive(Deserialize, ToSchema)]
pub struct SetAcl { pub allowed_groups: Vec<String> }
#[derive(Serialize, ToSchema)]
pub struct Status { pub ok: bool }
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct Forward { pub id: String, pub name: String, pub protocol: String, pub target_peer_id: String, pub target_port: u16, pub allowed_group_ids: Vec<String> }
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewForward { pub name: String, pub protocol: String, pub target_peer_id: String, pub target_port: u16, pub allowed_group_ids: Vec<String> }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn peer_serialization_contains_no_private_key_field() {
        let peer=Peer{id:"p".into(),name:"peer".into(),public_key:"public".into(),ipv4:"10.77.0.2".into(),group_id:"g".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None};
        let json=serde_json::to_string(&peer).unwrap();
        assert!(!json.contains("private_key"));
        assert!(!json.contains("PrivateKey"));
    }
    #[test]
    fn new_forward_rejects_removed_and_unknown_fields() {
        let valid = r#"{"name":"web","protocol":"tcp","target_peer_id":"peer","target_port":443,"allowed_group_ids":[]}"#;
        assert!(serde_json::from_str::<NewForward>(valid).is_ok());
        let legacy = r#"{"name":"web","protocol":"tcp","listen_port":443,"virtual_ip":"10.0.0.128","target_peer_id":"peer","target_port":443,"allowed_group_ids":[]}"#;
        assert!(serde_json::from_str::<NewForward>(legacy).is_err());
    }
}
