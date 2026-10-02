//! Validated, typed configuration used by the runtime. Persistent/API strings
//! are parsed before installation, never on packet delivery paths.
use std::{collections::HashMap, net::Ipv4Addr};
use crate::{model::{Group, NetworkSnapshot}, network::Subnet24};

#[derive(Clone, Debug)]
pub(crate) struct PeerConfig {
    pub(crate) id: String,
    #[cfg(test)]
    pub(crate) public_key: String,
    pub(crate) key: [u8; 32],
    pub(crate) ip: Ipv4Addr,
    pub(crate) group_id: String,
    pub(crate) group: Option<Group>,
}
pub(crate) trait PeerConfigView {
    fn id(&self) -> &str;
    fn key_identity(&self) -> PeerKey;
    fn ip(&self) -> Ipv4Addr;
    fn group_id(&self) -> &str;
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum PeerKey { Compiled([u8;32]), #[cfg(test)] Invalid(String) }
impl PeerConfigView for PeerConfig {
    fn id(&self)->&str { &self.id }
    fn key_identity(&self)->PeerKey { PeerKey::Compiled(self.key) }
    fn ip(&self)->Ipv4Addr { self.ip }
    fn group_id(&self)->&str { &self.group_id }
}
#[cfg(test)]
impl PeerConfigView for crate::model::Peer {
    fn id(&self)->&str { &self.id }
    fn key_identity(&self)->PeerKey { decode_public_key(&self.public_key).map(PeerKey::Compiled).unwrap_or_else(|_|PeerKey::Invalid(self.public_key.clone())) }
    fn ip(&self)->Ipv4Addr { self.ipv4.parse().unwrap_or(Ipv4Addr::UNSPECIFIED) }
    fn group_id(&self)->&str { &self.group_id }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransportProtocol { Tcp, Udp }
impl TransportProtocol {
    pub(crate) fn parse(value: &str) -> Option<Self> { match value { "tcp" => Some(Self::Tcp), "udp" => Some(Self::Udp), _ => None } }
    pub(crate) const fn number(self) -> u8 { match self { Self::Tcp => 6, Self::Udp => 17 } }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ForwardConfig {
    pub(crate) id: String,
    pub(crate) protocol: TransportProtocol,
    pub(crate) target_peer_id: String,
    pub(crate) target_port: u16,
    pub(crate) allowed_group_ids: Vec<String>,
}
#[cfg(test)]
impl From<crate::model::Forward> for ForwardConfig {
    fn from(value: crate::model::Forward) -> Self {
        Self { id:value.id, protocol:TransportProtocol::parse(&value.protocol).expect("valid test fixture protocol"), target_peer_id:value.target_peer_id, target_port:value.target_port, allowed_group_ids:value.allowed_group_ids }
    }
}
impl From<&crate::model::Forward> for ForwardConfig {
    fn from(value: &crate::model::Forward) -> Self { Self { id:value.id.clone(), protocol:TransportProtocol::parse(&value.protocol).expect("validated forward protocol"), target_peer_id:value.target_peer_id.clone(), target_port:value.target_port, allowed_group_ids:value.allowed_group_ids.clone() } }
}
#[derive(Clone, Debug)]
pub(crate) struct CompiledSnapshot {
    pub(crate) forwards: Vec<ForwardConfig>,
    pub(crate) initial_stats: HashMap<String, (u64, u64, Option<i64>)>,
    pub(crate) hub_ip: Option<Ipv4Addr>,
    pub(crate) peers: HashMap<String, PeerConfig>,
    pub(crate) peer_by_key: HashMap<[u8; 32], String>,
    pub(crate) peer_by_ip: HashMap<Ipv4Addr, String>,
}
impl TryFrom<NetworkSnapshot> for CompiledSnapshot {
    type Error = ();
    fn try_from(raw: NetworkSnapshot) -> Result<Self, Self::Error> {
        let subnet = raw.settings.as_ref().map(|s| Subnet24::parse(&s.subnet).map_err(|_| ())).transpose()?;
        let hub_ip = subnet.map(Subnet24::hub_ip);
        let mut peers = HashMap::new(); let mut peer_by_key = HashMap::new(); let mut peer_by_ip = HashMap::new();
        let groups: HashMap<_,_> = raw.groups.iter().map(|g|(g.id.as_str(),g)).collect();
        for peer in &raw.peers {
            let key = decode_public_key(&peer.public_key)?;
            let ip: Ipv4Addr = peer.ipv4.parse().map_err(|_| ())?;
            if subnet.is_some_and(|s| s.peer_ip(ip.octets()[3]) != Some(ip)) { return Err(()); }
            let group=groups.get(peer.group_id.as_str()).ok_or(())?.to_owned().clone();
            if peer_by_key.insert(key, peer.id.clone()).is_some() || peer_by_ip.insert(ip, peer.id.clone()).is_some() { return Err(()); }
            peers.insert(peer.id.clone(), PeerConfig{id:peer.id.clone(),#[cfg(test)] public_key:peer.public_key.clone(),key,ip,group_id:peer.group_id.clone(),group:Some(group)});
        }
        let forwards = raw.forwards.iter().map(|forward| { let protocol=TransportProtocol::parse(&forward.protocol).ok_or(())?; if forward.target_port==0{return Err(())} Ok(ForwardConfig{id:forward.id.clone(),protocol,target_peer_id:forward.target_peer_id.clone(),target_port:forward.target_port,allowed_group_ids:forward.allowed_group_ids.clone()}) }).collect::<Result<Vec<_>, ()>>()?;
        let initial_stats=raw.peers.iter().map(|peer|(peer.id.clone(),(peer.received_bytes,peer.sent_bytes,peer.last_handshake_unix))).collect();
        if (!raw.peers.is_empty() || !raw.forwards.is_empty()) && subnet.is_none() { return Err(()); }
        Ok(Self { forwards, initial_stats, hub_ip, peers, peer_by_key, peer_by_ip })
    }
}
pub(crate) fn decode_public_key(encoded:&str)->Result<[u8;32],()> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|_| ())?.try_into().map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Forward, Group, NetworkSettings, Peer};
    #[test]
    fn compiles_peer_identity_ip_and_forward_protocol_indexes() {
        use base64::Engine;
        let key=base64::engine::general_purpose::STANDARD.encode([7u8;32]);
        let snapshot=NetworkSnapshot{settings:Some(NetworkSettings{subnet:"10.1.2.0/24".into(),endpoint:"hub.example:51820".into(),persistent_keepalive:25}),groups:vec![Group{id:"g".into(),name:"g".into(),allowed_groups:vec![]}],peers:vec![Peer{id:"p".into(),name:"p".into(),public_key:key,ipv4:"10.1.2.2".into(),group_id:"g".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}],forwards:vec![Forward{id:"f".into(),name:"f".into(),protocol:"udp".into(),target_peer_id:"p".into(),target_port:53,allowed_group_ids:vec![]}]};
        let compiled=CompiledSnapshot::try_from(snapshot).unwrap();
        assert_eq!(compiled.hub_ip,Some(Ipv4Addr::new(10,1,2,1)));
        assert_eq!(compiled.peer_by_key.get(&[7u8;32]).map(String::as_str),Some("p"));
        assert_eq!(compiled.peer_by_ip.get(&Ipv4Addr::new(10,1,2,2)).map(String::as_str),Some("p"));
        assert_eq!(compiled.forwards[0].protocol,TransportProtocol::Udp);
    }
}
