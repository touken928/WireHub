import { useEffect, useRef, useState, type FormEvent } from 'react'
import { LoaderCircle } from 'lucide-react'
import { ApiError, configApi, forwardsApi, mutationFailure, type Forward, type NewForward } from '../../api/client'
import type { components } from '../../api/schema'
import Dialog from '../Dialog'

type Group = components['schemas']['Group']
type Peer = components['schemas']['Peer']
type Snapshot = { status: 'loading' | 'reload' } | { status: 'ready'; revision: number | null }
type Props = {
  forwardId?: string
  groups: Group[]
  peers: Peer[]
  forwards: Forward[]
  subnet: string
  session: number
  isSessionCurrent: (session: number) => boolean
  onMutationStart: () => void
  onMutationSettled: () => void
  onClose: () => void
  onSaved: (forward: Forward) => void
}

export default function ForwardDialog({ forwardId, groups, peers, forwards, subnet, session, isSessionCurrent, onMutationStart, onMutationSettled, onClose, onSaved }: Props) {
  const editing = forwardId !== undefined
  const [draft, setDraft] = useState<NewForward>(() => ({
    name: '', protocol: 'tcp', target_peer_id: peers[0]?.id ?? '', target_port: 0, allowed_group_ids: [],
  }))
  const [snapshot, setSnapshot] = useState<Snapshot>(editing ? { status: 'loading' } : { status: 'ready', revision: null })
  const [saving, setSaving] = useState(false)
  const savingRef = useRef(false)
  const [error, setError] = useState('')
  const [reloadVersion, setReloadVersion] = useState(0)

  useEffect(() => {
    if (!forwardId) return
    let active = true
    const current = () => active && isSessionCurrent(session)
    setSnapshot({ status: 'loading' }); setError('')
    void configApi.get().then(config => {
      if (!current()) return
      const forward = config.forwards.find(item => item.id === forwardId)
      if (!forward) throw new Error('Forward no longer exists. Close this dialog and refresh.')
      const { id: _, ...body } = forward
      setDraft(body)
      setSnapshot({ status: 'ready', revision: config.revision })
    }).catch(cause => {
      if (!current()) return
      setError(mutationFailure(cause, 'Unable to load forward. Close and reopen to retry.'))
      setSnapshot({ status: 'reload' })
    })
    return () => { active = false }
  }, [forwardId, reloadVersion, session, isSessionCurrent])

  const change = <K extends keyof NewForward,>(key: K, value: NewForward[K]) => setDraft(previous => ({ ...previous, [key]: value }))
  const duplicatePort = forwards.some(forward => forward.id !== forwardId && forward.protocol === draft.protocol && forward.target_port === draft.target_port)
  const loading = snapshot.status === 'loading'
  const locked = saving || snapshot.status !== 'ready'
  const entryAddress = subnet.replace(/\.0\/24$/, '.1')

  const submit = async (event: FormEvent) => {
    event.preventDefault()
    if (savingRef.current || snapshot.status !== 'ready') return
    if (!draft.name.trim()) { setError('Enter a name.'); return }
    if (duplicatePort) { setError(`${draft.protocol.toUpperCase()} port ${draft.target_port} is already in use.`); return }
    if (!Number.isInteger(draft.target_port) || draft.target_port < 1 || draft.target_port > 65535 || !draft.target_peer_id) return
    if (editing && snapshot.revision === null) return
    savingRef.current = true; setSaving(true); setError('')
    onMutationStart()
    try {
      const body = { ...draft, name: draft.name.trim() }
      let saved: Forward
      if (forwardId === undefined) {
        saved = await forwardsApi.create(body)
      } else {
        if (snapshot.revision === null) return
        saved = await forwardsApi.update(forwardId, snapshot.revision, body)
      }
      if (isSessionCurrent(session)) onSaved(saved)
    } catch (cause) {
      if (!isSessionCurrent(session)) return
      setError(mutationFailure(cause, 'Result unconfirmed. Refresh before retrying.'))
      if (editing && (!(cause instanceof ApiError) || cause.code === 'revision_conflict' || cause.status === 503 || cause.status === 404)) {
        setSnapshot({ status: 'reload' })
      }
    } finally {
      savingRef.current = false
      if (isSessionCurrent(session)) { onMutationSettled(); setSaving(false) }
    }
  }

  return <Dialog title={editing ? 'Edit forward' : 'New forward'} onClose={onClose} locked={saving}>
    <form className="stack-form" onSubmit={event => void submit(event)}>
      <fieldset className="form-fields" disabled={locked}>
        <label className="form-label">Name
          <input value={draft.name} onChange={event => change('name', event.target.value)} placeholder="Internal docs" required maxLength={128} />
        </label>
        <div className="forward-entry-preview"><span>Hub entry</span><b className="mono">{entryAddress}:{draft.target_port || 'port'}</b></div>
        <div className="form-row">
          <label className="form-label">Protocol
            <select aria-label="Protocol" value={draft.protocol} onChange={event => change('protocol', event.target.value)}><option value="tcp">TCP</option><option value="udp">UDP</option></select>
          </label>
          <label className="form-label">Port
            <input type="number" min="1" max="65535" step="1" value={draft.target_port || ''} onChange={event => change('target_port', Number(event.target.value))} placeholder="8080" required aria-invalid={duplicatePort} aria-describedby={duplicatePort ? 'forward-port-error' : undefined} />
          </label>
        </div>
        {duplicatePort && <div className="field-warning" id="forward-port-error" role="alert">{draft.protocol.toUpperCase()} port {draft.target_port} is already in use.</div>}
        <label className="form-label">Target peer
          <select aria-label="Target peer" value={draft.target_peer_id} onChange={event => change('target_peer_id', event.target.value)} required>
            <option value="" disabled>Select a peer</option>
            {peers.map(peer => <option value={peer.id} key={peer.id}>{peer.name} · {peer.ipv4}</option>)}
          </select>
        </label>
        <fieldset className="group-chooser"><legend>Allowed groups</legend>
          {groups.map(group => <label key={group.id} className="check-row">
            <input type="checkbox" checked={draft.allowed_group_ids.includes(group.id)} onChange={event => change('allowed_group_ids', event.target.checked ? [...draft.allowed_group_ids, group.id] : draft.allowed_group_ids.filter(id => id !== group.id))} />
            <span>{group.name}</span><small>{peers.filter(peer => peer.group_id === group.id).length} peers</small>
          </label>)}
        </fieldset>
        <p className="field-note">Internal access only. Hub and target use the same port. Allowed groups can access this service without direct access to the target’s group.</p>
        {!draft.allowed_group_ids.length && <p className="field-warning">No groups selected. This forward will block all access.</p>}
      </fieldset>
      {error && <div className="form-error" role="alert"><span>{error}</span>
        {editing && <button type="button" className="text-action" disabled={saving || loading} onClick={() => setReloadVersion(version => version + 1)}>Reload and discard edits</button>}
      </div>}
      <div className="dialog-actions">
        <button type="button" className="button button-quiet" disabled={saving} onClick={onClose}>Cancel</button>
        <button className="button button-primary" disabled={locked || !peers.length || duplicatePort}>
          {saving && <LoaderCircle size={15} className="spin" />}
          {loading ? 'Loading…' : saving ? (editing ? 'Saving…' : 'Creating…') : editing ? 'Save changes' : 'Create'}
        </button>
      </div>
    </form>
  </Dialog>
}
