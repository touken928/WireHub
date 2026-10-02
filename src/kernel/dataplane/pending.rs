use super::{IngressPacket, RouteStamp};
use std::collections::VecDeque;
use std::time::Instant;

pub(super) const PENDING_LIMIT: usize = 256;
pub(super) const PENDING_BYTES: usize = 1024 * 1024;
pub(super) struct PendingPacket {
    pub ingress: IngressPacket,
    pub stamp: RouteStamp,
    pub deadline: Instant,
}
#[derive(Default)]
pub(super) struct PendingQueue {
    pub entries: VecDeque<PendingPacket>,
    pub bytes: usize,
}
