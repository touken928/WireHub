import { ArrowDownLeft, ArrowUpRight } from 'lucide-react'
import type { components } from '../../api/schema'

type Counters = Pick<components['schemas']['Peer'], 'received_bytes' | 'sent_bytes'>
const formatBytes = (n: number) => n > 1_000_000_000 ? `${(n / 1_000_000_000).toFixed(1)} GB` : n > 1_000_000 ? `${(n / 1_000_000).toFixed(1)} MB` : n > 1000 ? `${(n / 1000).toFixed(0)} KB` : `${n} B`

export default function PeerTraffic({ peer, compact = false }: { peer: Counters; compact?: boolean }) {
  // API counters use the hub's perspective: received from / sent to this peer.
  const upload = formatBytes(peer.received_bytes), download = formatBytes(peer.sent_bytes)
  if (compact) return <span className="mini-traffic mono"><span title="Upload from this peer" aria-label={`Upload: ${upload}`}>↑ {upload}</span><i aria-hidden="true">/</i><span title="Download to this peer" aria-label={`Download: ${download}`}>↓ {download}</span></span>
  return <div className="peer-traffic"><div title="Upload from this peer"><span><ArrowUpRight size={13} />Upload</span><b className="mono">{upload}</b></div><div title="Download to this peer"><span><ArrowDownLeft size={13} />Download</span><b className="mono">{download}</b></div></div>
}
