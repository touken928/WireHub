use super::*;
use super::delivery::{publish_stats, retain_pending};

pub(super) async fn apply_snapshot(loader:&SnapshotLoader,key:[u8;32],limiter:Arc<RateLimiter>,state:&mut RouterState,stats:&RuntimeStats,old:HashMap<String,RuntimePeer>)->Result<(),()> {
    let compiled = crate::kernel::snapshot::CompiledSnapshot::try_from(loader()?)?;
    let peer_by_key=compiled.peer_by_key.clone(); let peer_by_ip=compiled.peer_by_ip.clone(); let protocols=compiled.protocols.clone();
    let snapshot = compiled.raw;
    let updated_hub_ip=compiled.hub_ip;
    let updated_forwards=snapshot.forwards;
    install_peers(snapshot.peers,&compiled.peers,key,limiter,state,old)?;
    state.keys=peer_by_key; state.ips=peer_by_ip; state.protocols=protocols;
    state.flows.reconcile(state.hub_ip,updated_hub_ip,&state.forwards,&updated_forwards,|id| state.peers.get(id).map(RuntimePeer::policy));
    retain_pending(&mut state.pending,&mut state.pending_bytes,&state.peers,&updated_forwards,updated_hub_ip,&state.flows);
    state.forwards=updated_forwards;state.hub_ip=updated_hub_ip;
    publish_stats(&state.peers,stats).await;
    Ok(())
}

pub(super) fn install_peers(current:Vec<Peer>,compiled:&HashMap<String,crate::kernel::snapshot::PeerConfig>,key:[u8;32],limiter:Arc<RateLimiter>,state:&mut RouterState,mut old:HashMap<String,RuntimePeer>)->Result<(),()> {
    let mut replacements=HashMap::new();let mut new_indexes=HashMap::new();
    for (id, prior) in &old { if compiled.get(id).is_some_and(|p|p.key==prior.config.key) { new_indexes.insert(prior.session.receiver_index,id.clone()); } }
    for peer in current {
        let config=compiled.get(&peer.id).ok_or(())?.clone(); let public=config.key;
        let prior=old.remove(&peer.id).filter(|p|p.config.key==config.key);
        // Keep the eager allocation behavior of unwrap_or: receiver cursor
        // advancement for retained peers is part of existing runtime behavior.
        let index=prior.as_ref().map(|p|p.session.receiver_index).unwrap_or(allocate_index(&mut state.next_index,&new_indexes)?);
        let runtime=if let Some(mut prior)=prior { prior.peer=peer;prior.config=config;prior } else { let stats=PeerRuntimeStats{received_bytes:peer.received_bytes,sent_bytes:peer.sent_bytes,last_handshake_unix:peer.last_handshake_unix,last_data_unix:None};RuntimePeer{peer,config,session:PeerSession{tunnel:Tunn::new(StaticSecret::from(key),PublicKey::from(public),None,None,index>>8,Some(limiter.clone())),endpoint:None,receiver_index:index},stats} };
        new_indexes.insert(index,runtime.peer.id.clone());
        replacements.insert(runtime.peer.id.clone(),runtime);
    }
    state.peers=replacements;state.indexes=new_indexes;Ok(())
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
