import { useState } from 'react'
import { ArrowUpNarrowWide, Copy, LoaderCircle, Network, Pencil, Plus, Search, Trash2, X } from 'lucide-react'
import type { components } from '../../api/schema'
import Dialog from '../Dialog'
import PeerTraffic from './PeerTraffic'

type Peer = components['schemas']['Peer']
type Group = components['schemas']['Group']
type Props = {
  peers: Peer[]; allCount: number; groups: Group[]; query: string
  setQuery: (value: string) => void; onCreate: () => void
  onMove: (peer: Peer, groupId: string) => void; onDelete: (peer: Peer) => void
  onRename: (peer: Peer, name: string) => Promise<boolean>; onToggle: (peer: Peer) => void
  pendingPeers: ReadonlySet<string>; onCopied: () => void; onCopyError: () => void
}
const handshakeAge = (unix?: number | null) => {
  if (!unix) return 'Never connected'
  const seconds = Math.max(0, Math.floor(Date.now() / 1000 - unix))
  return seconds < 60 ? `${seconds} s ago` : seconds < 3600 ? `${Math.floor(seconds / 60)} m ago` : `${Math.floor(seconds / 3600)} h ago`
}

export default function PeersPage({ peers, allCount, groups, query, setQuery, onCreate, onMove, onDelete, onRename, onToggle, pendingPeers, onCopied, onCopyError }: Props) {
  const [editing, setEditing] = useState<Peer | null>(null)
  const sorted = [...peers].sort((a, b) => a.ipv4.localeCompare(b.ipv4, 'en', { numeric: true }))
  return <div className="page-enter">
    <div className="page-heading"><div><h1>Peers</h1><span className="heading-count">{allCount} peers</span></div><button className="button button-primary" onClick={onCreate}><Plus size={16} />New peer</button></div>
    <div className="list-toolbar"><div className="search-box"><Search size={16} /><input aria-label="Search peers" value={query} onChange={e => setQuery(e.target.value)} placeholder="Search name, IP, or group" />{query && <button className="icon-button tiny" aria-label="Clear search" onClick={() => setQuery('')}><X size={14} /></button>}</div><div className="peer-list-summary"><span className="peer-sort"><ArrowUpNarrowWide size={14} />IP ascending</span><span className="result-count mono">{peers.length} / {allCount}</span></div></div>
    {sorted.length ? <div className="peer-list">
      <div className="peer-list-head" aria-hidden="true"><span>Peer / IP address</span><span>Group</span><span>Transfer totals</span><span>Access / actions</span></div>
      {sorted.map(peer => {
        const enabled = peer.enabled !== false, pending = pendingPeers.has(peer.id)
        const recent = enabled && Date.now() / 1000 - (peer.last_handshake_unix ?? 0) < 180
        return <article className={`peer-card${enabled ? '' : ' is-disabled'}`} key={peer.id} aria-label={`Peer ${peer.name}`} aria-busy={pending}>
          <div className="avatar" aria-hidden="true">{peer.name.slice(0, 1).toUpperCase()}</div>
          <div className="peer-identity"><h3 title={peer.name}>{peer.name}</h3>
            <div className="peer-address mono"><span>{peer.ipv4}</span><button className="icon-button tiny" aria-label={`Copy IP for ${peer.name}`} title="Copy IP address" onClick={async () => { try { await navigator.clipboard.writeText(peer.ipv4); onCopied() } catch { onCopyError() } }}><Copy size={13} /></button></div>
            <div className={`peer-card-state${enabled ? '' : ' is-disabled'}`}><span className={`connection-dot ${recent ? 'is-online' : ''}`} /><span>{!enabled ? 'Disabled' : recent ? 'Recent handshake' : 'No recent handshake'}</span><span className="peer-handshake-age" title={peer.last_handshake_unix ? new Date(peer.last_handshake_unix * 1000).toLocaleString('en-US') : 'No handshake recorded'}>{enabled ? handshakeAge(peer.last_handshake_unix) : 'Access paused'}</span></div>
          </div>
          <div className="peer-row-details"><label className="peer-meta-row"><span>Group</span><select disabled={pending} aria-label={`Group for ${peer.name}`} value={peer.group_id} onChange={e => onMove(peer, e.target.value)}>{groups.map(group => <option key={group.id} value={group.id}>{group.name}</option>)}</select></label>
            <PeerTraffic peer={peer} />
          </div>
          <div className="peer-actions"><button className={`peer-switch${enabled ? ' is-enabled' : ''}`} role="switch" aria-checked={enabled} aria-label={`Access for ${peer.name}`} title={`${enabled ? 'Disable' : 'Enable'} peer ${peer.name}`} disabled={pending} onClick={() => onToggle(peer)}><span>{pending && <LoaderCircle size={11} className="spin" />}</span></button><button className="icon-button" aria-label={`Rename peer ${peer.name}`} title="Rename peer" disabled={pending} onClick={() => setEditing(peer)}><Pencil size={15} /></button><button className="icon-button danger-on-hover peer-delete" disabled={pending} aria-label={`Delete peer ${peer.name}`} title="Delete peer" onClick={() => onDelete(peer)}><Trash2 size={15} /></button></div>
        </article>
      })}
    </div> : <div className="panel empty-panel"><div className="empty-state"><div className="empty-mark"><Network size={23} /></div><b>{query ? 'No matching peers' : 'No peers yet'}</b>{query ? <button className="text-action" onClick={() => setQuery('')}>Clear search</button> : <button className="button button-primary" onClick={onCreate}><Plus size={15} />New peer</button>}</div></div>}
    {editing && <RenamePeerDialog key={editing.id} peer={editing} pending={pendingPeers.has(editing.id)} onRename={onRename} onClose={() => setEditing(null)} />}
  </div>
}

function RenamePeerDialog({ peer, pending, onRename, onClose }: { peer: Peer; pending: boolean; onRename: Props['onRename']; onClose: () => void }) {
  const [name, setName] = useState(peer.name), [error, setError] = useState('')
  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    if (pending || !name.trim() || name.trim() === peer.name) return
    setError('')
    try { if (await onRename(peer, name.trim())) onClose() }
    catch (e) { setError(e instanceof Error ? e.message : 'Unable to rename peer.') }
  }
  return <Dialog title="Rename peer" onClose={onClose} locked={pending}>
    <form className="stack-form" onSubmit={event => void submit(event)}>
      <p className="field-note">Update the name for <span className="mono">{peer.ipv4}</span>.</p>
      <label className="form-label">Name<input value={name} onChange={event => setName(event.target.value)} required maxLength={128} disabled={pending} /></label>
      {error && <div className="form-error" role="alert">{error}</div>}
      <div className="dialog-actions"><button type="button" className="button button-quiet" onClick={onClose} disabled={pending}>Cancel</button><button className="button button-primary" disabled={pending || !name.trim() || name.trim() === peer.name}>{pending && <LoaderCircle size={15} className="spin" />}{pending ? 'Saving…' : 'Save changes'}</button></div>
    </form>
  </Dialog>
}
