use super::*;
use super::delivery::*;
use super::snapshot::*;
use super::delivery::PendingDelivery;

/// Run the WireGuard router on an already-bound UDP socket. `hub_private` is
/// the hub's persisted static private key; it is never written by this module.
pub(super) async fn run_udp(socket: UdpSocket, store: Arc<Store>, hub_private: [u8; 32], mut commands: mpsc::Receiver<ReloadCommand>, stats: RuntimeStats, readiness:Readiness, startup: Option<oneshot::Sender<Result<(), ()>>>) -> Result<(),()> {
    let hub_secret = StaticSecret::from(hub_private);
    let hub_public = PublicKey::from(&hub_secret);
    let rate_limiter = Arc::new(RateLimiter::new(&hub_public, 100));
    let mut state = RouterState::default();
    let mut datagram = [0u8; 65535];
    let mut out = vec![0u8; 65535];
    let mut timers = time::interval(TIMER);
    state.next_index = 1;

    readiness.set(false);
    if let Err(error) = apply_snapshot(&store, hub_private, rate_limiter.clone(), &mut state, &stats, HashMap::new()).await {
        readiness.set(false);
        if let Some(ready) = startup { let _ = ready.send(Err(())); }
        return Err(error);
    }
    readiness.set(true);
    let mut retry = SnapshotRetry::new();
    let mut retry_at: Option<Instant> = None;
    if let Some(ready) = startup { let _ = ready.send(Ok(())); }

    loop {
        tokio::select! {
            biased;
            command = commands.recv() => {
                let Some(command) = command else { readiness.set(false); return Err(()) };
                // Snapshot and validate before touching the installed runtime. A failed
                // reload is fail-closed; a valid reload reconciles authenticated state.
                let old = std::mem::take(&mut state.peers);
                let result = apply_snapshot(&store, hub_private, rate_limiter.clone(), &mut state, &stats, old).await;
                if result.is_err() { eprintln!("runtime reload snapshot failed; runtime remains fail-closed");state.fail_closed(); readiness.set(false);retry.succeeded();retry_at=Some(retry.failed_at(Instant::now())); }
                else { readiness.set(true); retry.succeeded();retry_at=None; }
                publish_stats(&state.peers, &stats).await;
                let _ = command.ack.send(result);
            }
            _ = timers.tick(), if timers_enabled() => {
                rate_limiter.reset_count();
                state.flows.expire(Instant::now());
                for runtime in state.peers.values_mut() {
                    let result = runtime.tunnel.update_timers(&mut out);
                    if let (TunnResult::WriteToNetwork(packet), Some(endpoint)) = (result, runtime.endpoint) { let _ = socket.send_to(packet, endpoint).await; }
                }
                drain_pending(&socket, &mut state.pending, &mut state.pending_bytes, &mut state.peers, &state.forwards, state.hub_ip, &mut state.flows, &mut out).await;
                publish_stats(&state.peers, &stats).await;
                if retry_at.is_some_and(|at|Instant::now()>=at) {
                    match apply_snapshot(&store, hub_private, rate_limiter.clone(), &mut state, &stats, HashMap::new()).await {
                        Ok(())=>{ readiness.set(true);retry_at=None;retry.succeeded(); }
                        Err(())=>{ readiness.set(false);retry_at=Some(retry.failed_at(Instant::now()));eprintln!("runtime snapshot recovery failed; remaining fail-closed"); }
                    }
                }
            }
            received = socket.recv_from(&mut datagram) => {
                let (len, endpoint) = match classify_udp_receive(received, &readiness) {
                    Ok(Some(received)) => received,
                    Ok(None) => continue,
                    Err(()) => return Err(()),
                };
                let bytes = &datagram[..len];
                let verified = rate_limiter.verify_packet(Some(endpoint.ip()), bytes, &mut out);
                let parsed: Result<Packet<'_>, ()> = match verified {
                    Ok(packet) => Ok(packet),
                    Err(TunnResult::WriteToNetwork(cookie)) => { let _=socket.send_to(cookie,endpoint).await; continue; }
                    Err(_) => continue,
                };
                let packet_kind = parsed.as_ref().ok().map(|packet| match packet { Packet::HandshakeInit(_) => 1, Packet::HandshakeResponse(_) => 2, Packet::PacketCookieReply(_) => 3, Packet::PacketData(_) => 4 });
                let peer_id = match parsed {
                    Ok(Packet::HandshakeInit(init)) => {
                        // This lookup is only a demux hint. Tunn must verify the
                        // full initiation before any peer endpoint is changed.
                        #[cfg(test)]
                        ANON_PARSE_COUNT.with(|count| count.fetch_add(1, Ordering::SeqCst));
                        parse_handshake_anon(&hub_secret, &hub_public, &init).ok()
                            .and_then(|h| state.peers.iter().find(|(_, p)| decode_public_key(&p.peer.public_key).ok().as_ref() == Some(&h.peer_static_public)).map(|(id, _)| id.clone()))
                    }
                    Ok(Packet::HandshakeResponse(resp)) => state.indexes.get(&(resp.receiver_idx & 0xffff_ff00)).cloned(),
                    Ok(Packet::PacketCookieReply(cookie)) => state.indexes.get(&(cookie.receiver_idx & 0xffff_ff00)).cloned(),
                    Ok(Packet::PacketData(data)) => state.indexes.get(&(data.receiver_idx & 0xffff_ff00)).cloned(),
                    Err(_) => None,
                };
                let Some(id) = peer_id else { continue };
                let Some(runtime) = state.peers.get_mut(&id) else { continue };
                let result = runtime.tunnel.decapsulate(Some(endpoint.ip()), bytes, &mut out);
                let mut authenticated = false;
                let mut handshake_authenticated = false;
                let mut data_authenticated = false;
                let mut packet_queue = Vec::new();
                let initial_result = &result;
                let accepted_handshake_response = packet_kind == Some(2)
                    && matches!(initial_result, TunnResult::WriteToNetwork(packet)
                        if matches!(Tunn::parse_incoming_packet(packet), Ok(Packet::PacketData(_))));
                let mut pending = result;
                let mut first_result = true;
                loop {
                    match pending {
                        TunnResult::WriteToNetwork(packet) => {
                            let payload = packet.to_vec();
                            // A cookie response is not authentication. An
                            // authenticated initiation yields a handshake response.
                            if packet_kind == Some(1) && payload.len() >= 4 && u32::from_le_bytes(payload[..4].try_into().unwrap()) == 2 { authenticated = true; handshake_authenticated = true; }
                            // BoringTun accepts an authenticated handshake response
                            // by immediately emitting an encrypted transport keepalive.
                            // Require both the incoming response context and that
                            // exact initial output type; arbitrary network output is
                            // not proof that a response was accepted.
                            if first_result && accepted_handshake_response {
                                authenticated = true;
                                handshake_authenticated = true;
                            }
                            first_result = false;
                            let _ = socket.send_to(&payload, endpoint).await;
                            pending = runtime.tunnel.decapsulate(None, &[], &mut out);
                        }
                        TunnResult::WriteToTunnelV4(packet, _) => {
                            first_result = false;
                            authenticated = true;
                            data_authenticated = true;
                            if let Some(packet) = ipv4::validate(packet, &runtime.peer, runtime.group.as_ref()) {
                                packet_queue.push(packet);
                            }
                            pending = runtime.tunnel.decapsulate(None, &[], &mut out);
                        }
                        TunnResult::WriteToTunnelV6(_, _) => { first_result = false; data_authenticated = true; pending = runtime.tunnel.decapsulate(None, &[], &mut out); }
                        // BoringTun 0.7.1 returns Done for an authenticated
                        // empty transport packet (keepalive), but also for a
                        // cookie reply. Never infer response authentication
                        // from Done; it is recorded only for the expected
                        // transport keepalive emitted after response acceptance.
                        TunnResult::Done => { if packet_kind == Some(4) { authenticated = true; data_authenticated = true; } break; }
                        TunnResult::Err(_) => break,
                    }
                }
                if authenticated { runtime.endpoint = Some(endpoint); }
                if handshake_authenticated { runtime.peer.last_handshake_unix = Some(unix_now()); }
                if data_authenticated { runtime.last_data_unix = Some(unix_now()); }
                let _ = runtime;
                for packet in packet_queue {
                    let now = std::time::Instant::now();
                    // `id` came from the authenticated tunnel receiver index. Keep
                    // that identity; never infer a source peer from packet IP.
                    let Some(source) = state.peers.get(&id).map(|p| p.peer.clone()) else { continue };
                    let Some(source_group) = state.peers.get(&id).and_then(|p| p.group.clone()) else { continue };
                    let (plan, was_reply) = resolve_packet(&packet, &source, &source_group, &id, &state.peers, &state.forwards, state.hub_ip, &mut state.flows, now);
                    if let Some(mut plan) = plan {
                        let outcome = deliver_plan(&socket, &mut state.peers, &plan, &mut out).await;
                        complete_delivery(&mut state.flows, &mut state.peers, &mut plan, outcome);
                        if outcome == EgressOutcome::NotReady {
                            let target=state.peers.get(&plan.target_id);
                            let forward=plan.forward_id.as_ref().and_then(|id|state.forwards.iter().find(|f|&f.id==id));
                            enqueue_pending(&mut state.pending, &mut state.pending_bytes, PendingDelivery { source_id: id.clone(), source_key:source.public_key.clone(), source_ip:packet.src(), packet: packet.clone(), reply_only: was_reply, deadline: Instant::now() + PENDING_TTL, target_id: plan.target_id.clone(), target_key:target.map(|p|p.peer.public_key.clone()).unwrap_or_default(), target_ip:target.and_then(|p|p.peer.ipv4.parse().ok()).unwrap_or(Ipv4Addr::UNSPECIFIED), forward_id: plan.forward_id.clone(), forward_protocol:forward.map(|f|f.protocol.clone()), forward_target_port:forward.map(|f|f.target_port) });
                            #[cfg(test)]
                            QUEUE_OBSERVER.try_with(|observer| {
                                let queued = state.pending.iter().map(|item| (item.source_id.clone(), item.target_id.clone(), item.forward_id.clone(), item.packet.src(), item.packet.dst(), item.target_ip, item.packet.src_port().unwrap_or_default(), item.packet.protocol())).collect();
                                let counters = state.peers.get(&id).map(|peer| (peer.peer.received_bytes, peer.peer.sent_bytes)).unwrap_or_default();
                                let _ = observer.send(QueueObservation { queued, queued_bytes: state.pending_bytes, flow_counts: state.flows.test_state_counts(), source_counters: counters });
                            }).ok();
                        }
                    }
                }
                if authenticated { drain_pending(&socket, &mut state.pending, &mut state.pending_bytes, &mut state.peers, &state.forwards, state.hub_ip, &mut state.flows, &mut out).await; }
                publish_packet_stats(&state.peers, &stats, authenticated).await;
                #[cfg(test)]
                PACKET_RESULT_OBSERVER.try_with(|observer| { let _ = observer.send(authenticated); }).ok();
            }
        }
    }
}
