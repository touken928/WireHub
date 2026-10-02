//! WireGuard session and UDP egress boundary.
//!
//! This module intentionally deals only in peer identities and encrypted
//! packets. Routing policy, flows, and accounting remain in the runtime and
//! dataplane respectively.
use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use boringtun::{
    noise::{handshake::parse_handshake_anon, rate_limiter::RateLimiter, Packet, Tunn, TunnResult},
    x25519::{PublicKey, StaticSecret},
};
use tokio::net::UdpSocket;

/// Stable authentication identity installed into a WireGuard session.
/// The IP is carried only so the runtime can bind authenticated plaintext to
/// the snapshot identity before passing it to the dataplane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PeerIdentity {
    pub(crate) id: String,
    pub(crate) key: [u8; 32],
    pub(crate) ip: std::net::Ipv4Addr,
}

/// WireGuard-owned per-peer tunnel state. No business routing configuration
/// is stored here.
pub(crate) struct Session {
    tunnel: Tunn,
    endpoint: Option<SocketAddr>,
    receiver_index: u32,
}

pub(crate) struct WireGuard {
    hub_private: [u8; 32],
    hub_secret: StaticSecret,
    hub_public: PublicKey,
    limiter: Arc<RateLimiter>,
    sessions: HashMap<String, SessionRecord>,
    indexes: HashMap<u32, String>,
    by_key: HashMap<[u8; 32], String>,
    next_index: u32,
}

struct SessionRecord {
    identity: PeerIdentity,
    session: Session,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransportOutcome {
    Delivered,
    NotReady,
    Failed,
}

pub(crate) struct IngressEvent {
    pub(crate) identity: PeerIdentity,
    pub(crate) packets: Vec<Vec<u8>>,
    pub(crate) authenticated: bool,
    pub(crate) handshake_authenticated: bool,
    pub(crate) data_authenticated: bool,
}

impl WireGuard {
    pub(crate) fn new(private: [u8; 32]) -> Self {
        let hub_secret = StaticSecret::from(private);
        let hub_public = PublicKey::from(&hub_secret);
        let limiter = Arc::new(RateLimiter::new(&hub_public, 100));
        Self {
            hub_private: private,
            hub_secret,
            hub_public,
            limiter,
            sessions: HashMap::new(),
            indexes: HashMap::new(),
            by_key: HashMap::new(),
            next_index: 1,
        }
    }

    /// Atomically reconcile transport identities, retaining the tunnel only
    /// when both stable ID and public key are unchanged.
    pub(crate) fn install(
        &mut self,
        identities: Vec<PeerIdentity>,
    ) -> Result<HashMap<String, bool>, ()> {
        let mut seen_ids = HashMap::new();
        let mut seen_keys = HashMap::new();
        for identity in &identities {
            if seen_ids.insert(identity.id.clone(), ()).is_some()
                || seen_keys.insert(identity.key, ()).is_some()
            {
                return Err(());
            }
        }
        let retained_indexes: HashMap<u32, String> = identities
            .iter()
            .filter_map(|identity| {
                self.sessions
                    .get(&identity.id)
                    .filter(|record| record.identity.key == identity.key)
                    .map(|record| (record.session.receiver_index, identity.id.clone()))
            })
            .collect();
        let mut indexes = HashMap::new();
        let mut reserved_indexes = retained_indexes.clone();
        let mut by_key = HashMap::new();
        let mut replacements = HashMap::new();
        let mut retained = HashMap::new();
        for identity in identities {
            let prior = self
                .sessions
                .remove(&identity.id)
                .filter(|record| record.identity.key == identity.key);
            // Preserve eager cursor movement for retained peers.
            let index = match prior.as_ref() {
                Some(record) => {
                    let _ = self.allocate_index(&reserved_indexes)?;
                    record.session.receiver_index
                }
                None => self.allocate_index(&reserved_indexes)?,
            };
            reserved_indexes.insert(index, identity.id.clone());
            let is_retained = prior.is_some();
            let record = if let Some(mut record) = prior {
                record.identity = identity.clone();
                record
            } else {
                SessionRecord {
                    session: Session::new(self.hub_private, &identity, index, self.limiter.clone()),
                    identity: identity.clone(),
                }
            };
            indexes.insert(index, identity.id.clone());
            by_key.insert(identity.key, identity.id.clone());
            replacements.insert(identity.id.clone(), record);
            retained.insert(identity.id, is_retained);
        }
        self.sessions = replacements;
        self.indexes = indexes;
        self.by_key = by_key;
        Ok(retained)
    }

    fn allocate_index(&mut self, indexes: &HashMap<u32, String>) -> Result<u32, ()> {
        for _ in 0..(u16::MAX as usize) {
            let candidate = self.next_index.wrapping_add(1).max(1) & 0x00ff_ffff;
            self.next_index = candidate;
            let index = candidate << 8;
            if !indexes.contains_key(&index) {
                return Ok(index);
            }
        }
        Err(())
    }

    pub(crate) fn clear(&mut self) {
        self.sessions.clear();
        self.indexes.clear();
        self.by_key.clear();
    }
    #[cfg(test)]
    pub(crate) fn identity(&self, id: &str) -> Option<&PeerIdentity> {
        self.sessions.get(id).map(|r| &r.identity)
    }
    #[cfg(test)]
    pub(crate) fn endpoint(&self, id: &str) -> Option<SocketAddr> {
        self.sessions.get(id).and_then(|r| r.session.endpoint)
    }
    #[cfg(test)]
    pub(crate) fn set_endpoint(&mut self, id: &str, endpoint: Option<SocketAddr>) {
        if let Some(r) = self.sessions.get_mut(id) {
            r.session.endpoint = endpoint;
        }
    }
    #[cfg(test)]
    pub(crate) fn receiver_index(&self, id: &str) -> Option<u32> {
        self.sessions.get(id).map(|r| r.session.receiver_index)
    }

    /// Deliver application plaintext through the peer transport. A cold
    /// session may emit only its own handshake initiation; that is not a
    /// successful application delivery and plaintext is not passed to it.
    pub(crate) async fn deliver(
        &mut self,
        socket: &UdpSocket,
        target_id: &str,
        plaintext: &[u8],
        out: &mut [u8],
    ) -> TransportOutcome {
        let Some(record) = self.sessions.get_mut(target_id) else {
            return TransportOutcome::Failed;
        };
        let session = &mut record.session;
        if session.tunnel.time_since_last_handshake().is_none() {
            if let Some(endpoint) = session.endpoint {
                if let TunnResult::WriteToNetwork(initiation) =
                    session.tunnel.format_handshake_initiation(out, false)
                {
                    let _ = socket.send_to(initiation, endpoint).await;
                }
            }
            return TransportOutcome::NotReady;
        }
        let wire = match encapsulate(session, plaintext, out) {
            Ok(wire) => wire,
            Err(EncapsulationError::NotReady) => return TransportOutcome::NotReady,
            Err(EncapsulationError::Failed) => return TransportOutcome::Failed,
        };
        let Some(endpoint) = session.endpoint else {
            return TransportOutcome::Failed;
        };
        if socket.send_to(wire, endpoint).await.is_ok() {
            TransportOutcome::Delivered
        } else {
            TransportOutcome::Failed
        }
    }

    pub(crate) async fn receive(
        &mut self,
        socket: &UdpSocket,
        endpoint: SocketAddr,
        bytes: &[u8],
        out: &mut [u8],
    ) -> Option<IngressEvent> {
        let verified = self.limiter.verify_packet(Some(endpoint.ip()), bytes, out);
        let packet = match verified {
            Ok(packet) => packet,
            Err(TunnResult::WriteToNetwork(cookie)) => {
                let _ = socket.send_to(cookie, endpoint).await;
                return None;
            }
            Err(_) => return None,
        };
        let packet_kind = match packet {
            Packet::HandshakeInit(_) => 1,
            Packet::HandshakeResponse(_) => 2,
            Packet::PacketCookieReply(_) => 3,
            Packet::PacketData(_) => 4,
        };
        let id = match packet {
            Packet::HandshakeInit(init) => {
                #[cfg(test)]
                ANON_PARSE_COUNT
                    .with(|count| count.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
                parse_handshake_anon(&self.hub_secret, &self.hub_public, &init)
                    .ok()
                    .and_then(|h| self.by_key.get(&h.peer_static_public).cloned())
            }
            Packet::HandshakeResponse(resp) => self
                .indexes
                .get(&(resp.receiver_idx & 0xffff_ff00))
                .cloned(),
            Packet::PacketCookieReply(cookie) => self
                .indexes
                .get(&(cookie.receiver_idx & 0xffff_ff00))
                .cloned(),
            Packet::PacketData(data) => self
                .indexes
                .get(&(data.receiver_idx & 0xffff_ff00))
                .cloned(),
        }?;
        let record = self.sessions.get_mut(&id)?;
        let identity = record.identity.clone();
        let session = &mut record.session;
        let result = session.tunnel.decapsulate(Some(endpoint.ip()), bytes, out);
        let accepted_handshake_response = packet_kind == 2
            && matches!(&result,
            TunnResult::WriteToNetwork(packet) if matches!(Tunn::parse_incoming_packet(packet), Ok(Packet::PacketData(_))));
        let mut authenticated = false;
        let mut handshake_authenticated = false;
        let mut data_authenticated = false;
        let mut packets = Vec::new();
        let mut pending = result;
        let mut first_result = true;
        loop {
            match pending {
                TunnResult::WriteToNetwork(packet) => {
                    let payload = packet.to_vec();
                    if packet_kind == 1
                        && payload.len() >= 4
                        && u32::from_le_bytes(payload[..4].try_into().unwrap()) == 2
                    {
                        authenticated = true;
                        handshake_authenticated = true;
                    }
                    if first_result && accepted_handshake_response {
                        authenticated = true;
                        handshake_authenticated = true;
                    }
                    first_result = false;
                    let _ = socket.send_to(&payload, endpoint).await;
                    pending = session.tunnel.decapsulate(None, &[], out);
                }
                TunnResult::WriteToTunnelV4(packet, _) => {
                    first_result = false;
                    authenticated = true;
                    data_authenticated = true;
                    packets.push(packet.to_vec());
                    pending = session.tunnel.decapsulate(None, &[], out);
                }
                TunnResult::WriteToTunnelV6(_, _) => {
                    first_result = false;
                    data_authenticated = true;
                    pending = session.tunnel.decapsulate(None, &[], out);
                }
                TunnResult::Done => {
                    if packet_kind == 4 {
                        authenticated = true;
                        data_authenticated = true;
                    }
                    break;
                }
                TunnResult::Err(_) => break,
            }
        }
        if authenticated {
            session.endpoint = Some(endpoint);
        }
        Some(IngressEvent {
            identity,
            packets,
            authenticated,
            handshake_authenticated,
            data_authenticated,
        })
    }

    pub(crate) async fn tick(&mut self, socket: &UdpSocket, out: &mut [u8]) {
        self.limiter.reset_count();
        for record in self.sessions.values_mut() {
            let session = &mut record.session;
            let result = session.tunnel.update_timers(out);
            if let (TunnResult::WriteToNetwork(packet), Some(endpoint)) = (result, session.endpoint)
            {
                let _ = socket.send_to(packet, endpoint).await;
            }
        }
    }
}

#[cfg(test)]
thread_local! { pub(crate) static ANON_PARSE_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0); }

impl Session {
    pub(crate) fn new(
        hub_key: [u8; 32],
        identity: &PeerIdentity,
        receiver_index: u32,
        limiter: std::sync::Arc<boringtun::noise::rate_limiter::RateLimiter>,
    ) -> Self {
        Self {
            tunnel: Tunn::new(
                StaticSecret::from(hub_key),
                PublicKey::from(identity.key),
                None,
                None,
                receiver_index >> 8,
                Some(limiter),
            ),
            endpoint: None,
            receiver_index,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boringtun::noise::rate_limiter::RateLimiter;
    use tokio::{
        net::UdpSocket,
        time::{timeout, Duration},
    };

    fn identity(id: &str, key: u8) -> PeerIdentity {
        PeerIdentity {
            id: id.into(),
            key: *PublicKey::from(&StaticSecret::from([key; 32])).as_bytes(),
            ip: "10.0.0.2".parse().unwrap(),
        }
    }

    #[test]
    fn install_retained_peer_eagerly_consumes_cursor_skips_collision_and_keeps_indexes_demultiplexed(
    ) {
        let mut wg = WireGuard::new([218; 32]);
        wg.install(vec![identity("retained", 31), identity("removed", 32)])
            .unwrap();
        let retained_index = wg.receiver_index("retained").unwrap();
        wg.next_index = 1;
        let retained = wg
            .install(vec![identity("retained", 31), identity("new", 33)])
            .unwrap();
        assert_eq!(retained.get("retained"), Some(&true));
        assert_eq!(retained.get("new"), Some(&false));
        assert_eq!(wg.receiver_index("retained"), Some(retained_index));
        assert_eq!(wg.receiver_index("new"),Some(4<<8),"eager retained allocation consumes a candidate, skips its reserved-index collision, then allocates a unique fresh index");
        assert_ne!(wg.receiver_index("new"), Some(retained_index));
        assert_eq!(
            wg.indexes.get(&retained_index).map(String::as_str),
            Some("retained")
        );
        assert_eq!(
            wg.indexes
                .get(&wg.receiver_index("new").unwrap())
                .map(String::as_str),
            Some("new")
        );
        assert_eq!(
            wg.indexes.len(),
            2,
            "removed peer index is not retained in the demultiplexer"
        );
    }

    #[tokio::test]
    async fn receive_cookie_reply_is_not_authenticated_and_cannot_migrate_endpoint() {
        let hub_private = [41; 32];
        let client_secret = StaticSecret::from([42; 32]);
        let mut wg = WireGuard::new(hub_private);
        wg.install(vec![PeerIdentity {
            id: "p".into(),
            key: *PublicKey::from(&client_secret).as_bytes(),
            ip: "10.0.0.2".parse().unwrap(),
        }])
        .unwrap();
        let hub_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let client_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let attacker = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let old_endpoint = client_socket.local_addr().unwrap();
        wg.set_endpoint("p", Some(old_endpoint));
        let hub_public = PublicKey::from(&StaticSecret::from(hub_private));
        let limiter = Arc::new(RateLimiter::new(&PublicKey::from(&client_secret), 0));
        let mut client = Tunn::new(client_secret, hub_public, None, None, 99, Some(limiter));
        let mut out = vec![0; 65535];
        let plaintext = b"must not become authenticated application traffic";
        assert_eq!(
            wg.deliver(&hub_socket, "p", plaintext, &mut out).await,
            TransportOutcome::NotReady
        );
        let (n, _) = timeout(Duration::from_secs(1), client_socket.recv_from(&mut out))
            .await
            .unwrap()
            .unwrap();
        let mut client_out = vec![0; 65535];
        let TunnResult::WriteToNetwork(cookie) = client.decapsulate(
            Some(hub_socket.local_addr().unwrap().ip()),
            &out[..n],
            &mut client_out,
        ) else {
            panic!("overloaded real peer limiter must issue a cookie reply")
        };
        let cookie = cookie.to_vec();
        attacker
            .send_to(&cookie, hub_socket.local_addr().unwrap())
            .await
            .unwrap();
        let event = wg
            .receive(
                &hub_socket,
                attacker.local_addr().unwrap(),
                &cookie,
                &mut out,
            )
            .await
            .expect("indexed cookie reply reaches its tunnel");
        assert!(!event.authenticated);
        assert!(!event.handshake_authenticated);
        assert!(!event.data_authenticated);
        assert!(event.packets.is_empty());
        assert_eq!(
            wg.endpoint("p"),
            Some(old_endpoint),
            "cookie-reply processing must not migrate the authenticated endpoint"
        );
    }

    #[tokio::test]
    async fn receive_handshake_response_authenticates_only_after_packet_data_and_application_retry()
    {
        let hub_private = [51; 32];
        let client_secret = StaticSecret::from([52; 32]);
        let mut wg = WireGuard::new(hub_private);
        wg.install(vec![PeerIdentity {
            id: "p".into(),
            key: *PublicKey::from(&client_secret).as_bytes(),
            ip: "10.0.0.2".parse().unwrap(),
        }])
        .unwrap();
        let hub_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let client_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let attacker = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let old_endpoint = "127.0.0.1:9".parse().unwrap();
        wg.set_endpoint("p", Some(client_socket.local_addr().unwrap()));
        let mut client = Tunn::new(
            client_secret,
            PublicKey::from(&StaticSecret::from(hub_private)),
            None,
            None,
            101,
            None,
        );
        let mut out = vec![0; 65535];
        let mut plaintext = vec![0; 20];
        plaintext[0] = 0x45;
        plaintext[2..4].copy_from_slice(&20u16.to_be_bytes());
        plaintext[8] = 64;
        assert_eq!(
            wg.deliver(&hub_socket, "p", &plaintext, &mut out).await,
            TransportOutcome::NotReady
        );
        let (n, _) = timeout(Duration::from_secs(1), client_socket.recv_from(&mut out))
            .await
            .unwrap()
            .unwrap();
        let mut client_out = vec![0; 65535];
        let TunnResult::WriteToNetwork(response) = client.decapsulate(
            Some(hub_socket.local_addr().unwrap().ip()),
            &out[..n],
            &mut client_out,
        ) else {
            panic!("real initiation must produce a real handshake response")
        };
        let response = response.to_vec();

        let mut invalid = response.clone();
        invalid[12] ^= 1; // Corrupt the encrypted Noise response body, not the optional MAC2 field.
        attacker
            .send_to(&invalid, hub_socket.local_addr().unwrap())
            .await
            .unwrap();
        assert!(
            wg.receive(
                &hub_socket,
                attacker.local_addr().unwrap(),
                &invalid,
                &mut out
            )
            .await
            .is_none(),
            "invalid response MAC is rejected before authentication"
        );
        assert_eq!(
            wg.endpoint("p"),
            Some(client_socket.local_addr().unwrap()),
            "invalid response cannot migrate endpoint"
        );

        attacker
            .send_to(&response, hub_socket.local_addr().unwrap())
            .await
            .unwrap();
        let event = wg
            .receive(
                &hub_socket,
                attacker.local_addr().unwrap(),
                &response,
                &mut out,
            )
            .await
            .unwrap();
        assert!(event.authenticated);
        assert!(event.handshake_authenticated);
        assert!(!event.data_authenticated);
        assert!(event.packets.is_empty(),"BoringTun's response-triggered PacketData is transport control, not business plaintext");
        assert_eq!(wg.endpoint("p"), Some(attacker.local_addr().unwrap()));
        let (n, _) = timeout(Duration::from_secs(1), attacker.recv_from(&mut out))
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(
                Tunn::parse_incoming_packet(&out[..n]),
                Ok(Packet::PacketData(_))
            ),
            "first authenticated response output is a real PacketData keepalive"
        );

        assert_eq!(
            wg.deliver(&hub_socket, "p", &plaintext, &mut out).await,
            TransportOutcome::Delivered,
            "application retry is delivered only after handshake readiness"
        );
        let (n, _) = timeout(Duration::from_secs(1), attacker.recv_from(&mut out))
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            Tunn::parse_incoming_packet(&out[..n]),
            Ok(Packet::PacketData(_))
        ));
        let mut client_out = vec![0; 65535];
        assert!(
            matches!(client.decapsulate(Some(attacker.local_addr().unwrap().ip()),&out[..n],&mut client_out),TunnResult::WriteToTunnelV4(packet,_) if packet==plaintext)
        );
        assert_ne!(wg.endpoint("p"), Some(old_endpoint));
    }
}

/// Encapsulate plaintext for one session. Only transport packets are eligible
/// for UDP delivery; BoringTun's queued-handshake output is never considered a
/// completed delivery.
pub(crate) fn encapsulate<'a>(
    session: &mut Session,
    plaintext: &[u8],
    out: &'a mut [u8],
) -> Result<&'a [u8], EncapsulationError> {
    match session.tunnel.encapsulate(plaintext, out) {
        TunnResult::WriteToNetwork(packet)
            if matches!(
                Tunn::parse_incoming_packet(packet),
                Ok(Packet::PacketData(_))
            ) =>
        {
            Ok(packet)
        }
        TunnResult::WriteToNetwork(_) => Err(EncapsulationError::NotReady),
        _ => Err(EncapsulationError::Failed),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EncapsulationError {
    NotReady,
    Failed,
}
