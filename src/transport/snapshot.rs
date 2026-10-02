use super::*;
use super::delivery::{publish_stats, retain_pending};

pub(super) async fn apply_snapshot(store:&Store,key:[u8;32],limiter:Arc<RateLimiter>,state:&mut RouterState,stats:&RuntimeStats,old:HashMap<String,RuntimePeer>)->Result<(),()> {
    let snapshot=store.runtime_snapshot().map_err(|_|())?;
    validate_persisted_addresses(&snapshot)?;
    let updated_hub_ip=snapshot.settings.as_ref().map(|settings|Subnet24::parse(&settings.subnet).map_err(|_|())?.hub_ip().parse::<Ipv4Addr>().map_err(|_|())).transpose()?;
    let updated_forwards=snapshot.forwards;
    install_peers(&snapshot.groups,snapshot.peers,key,limiter,state,old)?;
    state.flows.reconcile(state.hub_ip,updated_hub_ip,&state.forwards,&updated_forwards,&state.peers);
    retain_pending(&mut state.pending,&mut state.pending_bytes,&state.peers,&updated_forwards,updated_hub_ip,&state.flows);
    state.forwards=updated_forwards;state.hub_ip=updated_hub_ip;
    publish_stats(&state.peers,stats).await;
    Ok(())
}

pub(super) fn install_peers(groups:&[Group],current:Vec<Peer>,key:[u8;32],limiter:Arc<RateLimiter>,state:&mut RouterState,mut old:HashMap<String,RuntimePeer>)->Result<(),()> {
    let group_map:HashMap<_,_>=groups.iter().cloned().map(|g|(g.id.clone(),g)).collect();
    let mut replacements=HashMap::new();let mut new_indexes=HashMap::new();
    for (id, prior) in &old { if current.iter().any(|p|p.id==*id && p.public_key==prior.peer.public_key) { new_indexes.insert(prior.receiver_index,id.clone()); } }
    for peer in current {
        let group=group_map.get(&peer.group_id).cloned().ok_or(())?;let public=decode_public_key(&peer.public_key)?;
        let prior=old.remove(&peer.id).filter(|p|p.peer.public_key==peer.public_key);
        // Keep the eager allocation behavior of unwrap_or: receiver cursor
        // advancement for retained peers is part of existing runtime behavior.
        let index=prior.as_ref().map(|p|p.receiver_index).unwrap_or(allocate_index(&mut state.next_index,&new_indexes)?);
        let mut peer=peer;
        let runtime=if let Some(mut prior)=prior { peer.received_bytes=prior.peer.received_bytes;peer.sent_bytes=prior.peer.sent_bytes;peer.last_handshake_unix=prior.peer.last_handshake_unix;prior.peer=peer;prior.group=Some(group);prior } else { RuntimePeer{peer,group:Some(group),tunnel:Tunn::new(StaticSecret::from(key),PublicKey::from(public),None,None,index>>8,Some(limiter.clone())),endpoint:None,last_data_unix:None,receiver_index:index} };
        new_indexes.insert(index,runtime.peer.id.clone());
        replacements.insert(runtime.peer.id.clone(),runtime);
    }
    state.peers=replacements;state.indexes=new_indexes;Ok(())
}

pub(super) fn validate_persisted_addresses(snapshot:&crate::storage::RuntimeSnapshot)->Result<(),()> {
    let peers=&snapshot.peers;
    let forwards=&snapshot.forwards;
    if peers.is_empty()&&forwards.is_empty() { return Ok(()); }
    let settings=snapshot.settings.as_ref().ok_or(())?;let subnet=Subnet24::parse(&settings.subnet).map_err(|_|())?;
    for peer in peers {let ip:Ipv4Addr=peer.ipv4.parse().map_err(|_|())?;if subnet.peer_ip(ip.octets()[3]).as_deref()!=Some(peer.ipv4.as_str()){return Err(())}}
    for forward in forwards {if !matches!(forward.protocol.as_str(),"tcp"|"udp")||forward.target_port==0{return Err(())}}
    Ok(())
}

pub(super) fn allocate_index(next: &mut u32, indexes: &HashMap<u32, String>) -> Result<u32, ()> {
    for _ in 0..(u16::MAX as usize) {
        let candidate = (*next).wrapping_add(1).max(1) & 0x00ff_ffff;
        *next = candidate;
        let wire_index = candidate << 8;
        if !indexes.contains_key(&wire_index) { return Ok(wire_index); }
    }
    Err(())
}

pub(super) fn decode_public_key(encoded: &str) -> Result<[u8; 32], ()> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|_| ())?;
    bytes.try_into().map_err(|_| ())
}
