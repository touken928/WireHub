import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import {
  Activity, ArrowDownLeft, ArrowLeftRight, ArrowUpRight, Check, CircleHelp,
  Copy, Download, Gauge, GitBranch, KeyRound, LoaderCircle, LogOut,
  Menu, Network, Plus, Radio, RefreshCw, Search, Settings2, Shield, Trash2, X,
} from 'lucide-react'
import GroupsPage from './components/groups/GroupsPage'
import '@xyflow/react/dist/style.css'
import { ApiError, client, forwardsApi, rememberToken, setupApi, type Forward, type NetworkSettings, type SetupStatus } from './api/client'
import type { components } from './api/schema'

type Peer = components['schemas']['Peer']
type Group = components['schemas']['Group']
type Provision = components['schemas']['PeerProvision']
type Page = 'overview' | 'peers' | 'groups' | 'forwards' | 'settings'

const initials = (name: string) => name.slice(0, 1).toUpperCase()
const formatBytes = (n: number) => n > 1_000_000_000 ? `${(n / 1_000_000_000).toFixed(1)} GB` : n > 1_000_000 ? `${(n / 1_000_000).toFixed(1)} MB` : n > 1000 ? `${(n / 1000).toFixed(0)} KB` : `${n} B`
const timeAgo = (unix?: number | null) => {
  if (!unix) return 'Never'
  const seconds = Math.max(0, Math.floor(Date.now() / 1000 - unix))
  return seconds < 60 ? `${seconds} s ago` : seconds < 3600 ? `${Math.floor(seconds / 60)} m ago` : `${Math.floor(seconds / 3600)} h ago`
}
const mutationFailure = (error: unknown, fallback: string) => {
  const message = error instanceof Error ? error.message : ''
  if (/\b503\b|\b5\d\d\b|network|fetch failed|failed to fetch|networkerror/i.test(message)) return 'Result unconfirmed. Refresh to check the current state before retrying.'
  return message || fallback
}

export default function App() {
  const [token, setToken] = useState('')
  const [connected, setConnected] = useState(false)
  const [page, setPage] = useState<Page>('overview')
  const [policyDirty, setPolicyDirty] = useState(false)
  const navigate = (next: Page) => { if (next !== page && policyDirty && !confirm('Discard unsaved policy changes?')) return; setPage(next); setMobileNav(false) }
  const [peers, setPeers] = useState<Peer[]>([])
  const [groups, setGroups] = useState<Group[]>([])
  const [forwards, setForwards] = useState<Forward[]>([])
  const [busy, setBusy] = useState(false)
  const [updatedAt, setUpdatedAt] = useState<Date | null>(null)
  const [pendingPeers, setPendingPeers] = useState<Set<string>>(() => new Set())
  const pendingPeersRef = useRef(new Set<string>())
  const [settingsSaving, setSettingsSaving] = useState(false)
  const [settingsError, setSettingsError] = useState('')
  const settingsPendingRef = useRef(false)
  const [error, setError] = useState('')
  const [forwardsError, setForwardsError] = useState('')
  const [query, setQuery] = useState('')
  const [modal, setModal] = useState<'peer' | 'group' | 'forward' | null>(null)
  const [provision, setProvision] = useState<Provision | null>(null)
  const [mobileNav, setMobileNav] = useState(false)
  const [toast, setToast] = useState('')
  const [loginError, setLoginError] = useState('')
  const [loginBusy, setLoginBusy] = useState(false)
  const [setup, setSetup] = useState<SetupStatus | null>(null)
  const sessionRef = useRef(0)
  // Reads share one freshness boundary, including policy recovery. ACL writes
  // invalidate at both start and settlement: a failed response may still commit,
  // and reads begun during a write cannot be trusted after it settles.
  const mutationEpochRef = useRef(0)
  const loadRef = useRef(0)
  const invalidateLoads = () => { ++mutationEpochRef.current }
  const beginPeerMutation = (id: string) => {
    if (pendingPeersRef.current.has(id)) return false
    pendingPeersRef.current.add(id); setPendingPeers(new Set(pendingPeersRef.current))
    return true
  }
  const finishPeerMutation = (id: string) => {
    pendingPeersRef.current.delete(id); setPendingPeers(new Set(pendingPeersRef.current))
  }

  const load = useCallback(async () => {
    if (!connected || !setup?.configured) return
    const session = sessionRef.current
    invalidateLoads()
    const epoch = mutationEpochRef.current, request = ++loadRef.current
    const isCurrent = () => session === sessionRef.current && epoch === mutationEpochRef.current && request === loadRef.current
    setBusy(true); setError('')
    try {
      const [p, g] = await Promise.all([client.GET('/api/peers'), client.GET('/api/groups')])
      if (!isCurrent()) return
      if (p.error || g.error) throw new Error('Unable to load data. Check your access token.')
      setPeers(p.data ?? []); setGroups(g.data ?? []); setUpdatedAt(new Date())
      try { const list = await forwardsApi.list(); if (isCurrent()) { setForwards(list); setForwardsError('') } }
      catch (e) { if (isCurrent()) setForwardsError(e instanceof Error ? e.message : 'Unable to load forwards.') }
    } catch (e) { if (isCurrent()) setError(e instanceof Error ? e.message : 'Connection failed. Check the server.') }
    // An invalidated read still owns its spinner until it settles; an older read
    // must never clear the busy state of a newer request or a different session.
    finally { if (session === sessionRef.current && request === loadRef.current) setBusy(false) }
  }, [connected, setup?.configured])
  useEffect(() => { void load() }, [load])
  useEffect(() => { if (!policyDirty) return; const guard = (e: BeforeUnloadEvent) => { e.preventDefault(); e.returnValue = '' }; window.addEventListener('beforeunload', guard); return () => window.removeEventListener('beforeunload', guard) }, [policyDirty])
  useEffect(() => { if (toast) { const id = window.setTimeout(() => setToast(''), 2800); return () => window.clearTimeout(id) } }, [toast])

  const enter = async (e: React.FormEvent) => {
    e.preventDefault()
    const candidate = token.trim()
    if (!candidate) return
    setLoginBusy(true); setLoginError('')
    const session = ++sessionRef.current
    rememberToken(candidate)
    try {
      const setupStatus = await setupApi.get()
      if (session !== sessionRef.current) return
      setSetup(setupStatus)
      if (setupStatus.configured) {
        const response = await client.GET('/api/peers')
        if (session !== sessionRef.current) return
        if (response.error || !response.data) throw new Error('Invalid token or insufficient access.')
        setPeers(response.data)
      }
      setConnected(true)
    } catch (e) {
      if (session !== sessionRef.current) return
      rememberToken('')
      setLoginError(e instanceof Error ? e.message : 'Connection failed. Check the server and token.')
    } finally { if (session === sessionRef.current) setLoginBusy(false) }
  }
  const signOut = () => { if (policyDirty && !confirm('Sign out and discard unsaved policy changes?')) return; ++sessionRef.current; setConnected(false); setToken(''); rememberToken(''); setPeers([]); setGroups([]); setForwards([]); setSetup(null); setModal(null); setProvision(null); setError(''); setForwardsError(''); setToast(''); setLoginError(''); setMobileNav(false); setQuery(''); setBusy(false); setUpdatedAt(null); pendingPeersRef.current.clear(); setPendingPeers(new Set()); settingsPendingRef.current = false; setSettingsSaving(false); setSettingsError(''); setPolicyDirty(false); setPage('overview') }
  const activeSession = sessionRef.current
  const filteredPeers = peers.filter(p => `${p.name} ${p.ipv4} ${groupName(groups, p.group_id)}`.toLowerCase().includes(query.toLowerCase()))
  const stats = useMemo(() => ({ online: peers.filter(p => Date.now() / 1000 - (p.last_handshake_unix ?? 0) < 180).length, sent: peers.reduce((s, p) => s + p.sent_bytes, 0), received: peers.reduce((s, p) => s + p.received_bytes, 0) }), [peers])

  if (!connected) return <Login token={token} setToken={setToken} onSubmit={enter} error={loginError} busy={loginBusy} />
  if (setup && !setup.configured) return <SetupWizard onConfigured={settings => { setSetup({ configured: true, settings }); setPage('overview') }} />
  const pageTitle = { overview: 'Overview', peers: 'Peers', groups: 'Groups', forwards: 'Forwards', settings: 'Settings' }[page]
  return <div className="app-shell">
    <aside className={`sidebar ${mobileNav ? 'sidebar-open' : ''}`}>
      <div className="brand"><div className="brand-mark"><Network size={19} strokeWidth={2.5} /></div><span>WireHub</span></div>
      <div className="workspace"><div className="workspace-icon"><Radio size={17} /></div><div><strong>Private network</strong><small className="mono">{setup?.settings?.subnet ?? 'WireGuard'}</small></div></div>

      <nav id="main-navigation" aria-label="Main navigation">
        <NavItem active={page === 'overview'} icon={<Gauge />} label="Overview" onClick={() => { navigate('overview') }} />
        <NavItem active={page === 'peers'} icon={<Network />} label="Peers" count={peers.length} onClick={() => { navigate('peers') }} />
        <NavItem active={page === 'groups'} icon={<GitBranch />} label="Groups" onClick={() => { navigate('groups') }} />
         <NavItem active={page === 'forwards'} icon={<ArrowLeftRight />} label="Forwards" count={forwards.length} onClick={() => { navigate('forwards') }} />
         <NavItem active={page === 'settings'} icon={<Settings2 />} label="Settings" onClick={() => { navigate('settings') }} />
      </nav>
      <div className="sidebar-bottom"><div className="network-status"><span className="pulse-dot" /><b>Authenticated</b></div><button className="profile" onClick={signOut}><div className="avatar admin-avatar">W</div><div><b>Administrator</b><small>Sign out</small></div><LogOut size={16} /></button></div>
    </aside>
    {mobileNav && <button aria-label="Close menu" className="mobile-scrim" onClick={() => setMobileNav(false)} />}
    <a className="skip-link" href="#main-content">Skip to content</a><main className="main-area">
      <header className="topbar"><button className="icon-button mobile-menu" aria-label="Open navigation" aria-controls="main-navigation" aria-expanded={mobileNav} onClick={() => setMobileNav(true)}><Menu size={19} /></button><div className="breadcrumbs"><span>WireHub</span><span className="crumb-slash">/</span><strong>{pageTitle}</strong></div><div className="top-actions"><div className="secure-label"><Shield size={14} /> Admin session</div><button className="icon-button" aria-label="Refresh data" onClick={() => void load()} disabled={busy}><RefreshCw size={17} className={busy ? 'spin' : ''} /></button><div className="top-avatar">W</div></div></header>
      <div className="content-wrap" id="main-content" tabIndex={-1}>
        {error && <div className="error-banner" role="alert"><CircleHelp size={18} /><span>{error}</span><button onClick={() => void load()}>Retry</button></div>}
        {page === 'overview' && <Overview peers={peers} groups={groups} forwards={forwards} stats={stats} busy={busy} settings={setup?.settings ?? null} updatedAt={updatedAt} setPage={navigate} />}
        {page === 'peers' && <PeersPage peers={filteredPeers} allCount={peers.length} groups={groups} query={query} setQuery={setQuery} onCreate={() => setModal('peer')} pendingPeers={pendingPeers} onCopied={() => setToast('IP copied')} onCopyError={() => setToast('Copy failed. Select the IP to copy it.')} onMove={async (peer, groupId) => {
          if (!beginPeerMutation(peer.id)) return
          const session = sessionRef.current; setError('')
          try {
            const r = await client.PUT('/api/peers/{id}/group', { params: { path: { id: peer.id } }, body: { group_id: groupId } })
            if (session !== sessionRef.current) return
            if (r.error || !r.data) throw new Error('Move unconfirmed. Refresh to check the group.')
            invalidateLoads(); setPeers(v => v.map(x => x.id === peer.id ? r.data! : x)); setToast('Group updated')
          } catch (e) { if (session === sessionRef.current) setError(mutationFailure(e, 'Unable to move peer.')) }
          finally { if (session === sessionRef.current) finishPeerMutation(peer.id) }
        }} onDelete={async peer => {
          if (pendingPeersRef.current.has(peer.id)) return
          if (!confirm(`Remove peer "${peer.name}"? Its configuration will stop working.`)) return
          if (!beginPeerMutation(peer.id)) return
          const session = sessionRef.current; setError('')
          try {
            const r = await client.DELETE('/api/peers/{id}', { params: { path: { id: peer.id } } })
            if (session !== sessionRef.current) return
            if (r.error) throw new Error('Deletion unconfirmed. Refresh to check the current state.')
            invalidateLoads(); setPeers(v => v.filter(x => x.id !== peer.id)); setForwards(v => v.filter(x => x.target_peer_id !== peer.id)); setToast('Peer removed')
          } catch (e) { if (session === sessionRef.current) setError(mutationFailure(e, 'Unable to delete peer.')) }
          finally { if (session === sessionRef.current) finishPeerMutation(peer.id) }
        }} />}
         {page === 'groups' && <GroupsPage onDirtyChange={setPolicyDirty} groups={groups} peers={peers} onCreate={() => setModal('group')} onSaved={() => { if (activeSession === sessionRef.current) setToast('Policy saved') }} onDelete={async g => { if (!confirm(`Delete group "${g.name}"?`)) return; try { const r = await client.DELETE('/api/groups/{id}', { params: { path: { id: g.id } } }); if (activeSession !== sessionRef.current) return; if (!r.error) { invalidateLoads(); setGroups(v => v.filter(x => x.id !== g.id).map(x => ({ ...x, allowed_groups: (x.allowed_groups ?? []).filter(id => id !== g.id) }))); setForwards(v => v.map(f => ({ ...f, allowed_group_ids: f.allowed_group_ids.filter(id => id !== g.id) }))); setToast('Group deleted') } else setError('Deletion unconfirmed. Refresh before retrying.') } catch (e) { if (activeSession === sessionRef.current) setError(mutationFailure(e, 'Unable to delete group.')) } }} onSaveAcl={async (id, allowed) => {
           if (activeSession !== sessionRef.current) throw new Error('Session ended.')
           invalidateLoads()
           try {
             const r = await client.PUT('/api/groups/{id}/acl', { params: { path: { id } }, body: { allowed_groups: allowed } })
             if (activeSession !== sessionRef.current) throw new Error('Session ended.')
             if (r.error || !r.data) throw new Error('Save unconfirmed. Reload the policy before retrying.')
             setGroups(v => v.map(g => g.id === id ? r.data! : g))
           } finally { if (activeSession === sessionRef.current) invalidateLoads() }
         }} onReload={async () => {
           if (activeSession !== sessionRef.current) throw new Error('Session ended.')
           invalidateLoads()
           const epoch = mutationEpochRef.current
           const r = await client.GET('/api/groups')
           if (activeSession !== sessionRef.current) throw new Error('Session ended.')
           if (epoch !== mutationEpochRef.current) throw new Error('Policy changed while reloading. Reload again.')
           if (r.error || !r.data) throw new Error('Unable to reload policy.')
           setGroups(r.data)
         }} />}
          {page === 'forwards' && <ForwardsPage forwards={forwards} subnet={setup?.settings?.subnet ?? ''} loadError={forwardsError} onRetry={() => void load()} peers={peers} groups={groups} onCreate={() => setModal('forward')} onDelete={async f => { if (!confirm(`Delete forward "${f.name}"?`)) return; const session = sessionRef.current; try { await forwardsApi.remove(f.id); if (session !== sessionRef.current) return; invalidateLoads(); setForwards(v => v.filter(x => x.id !== f.id)); setToast('Forward deleted') } catch (e) { if (session === sessionRef.current) setError(mutationFailure(e, 'Deletion unconfirmed. Refresh before retrying.')) } }} />}
          {page === 'settings' && setup?.settings && <SettingsPage settings={setup.settings} saving={settingsSaving} error={settingsError} onSave={async defaults => {
            if (activeSession !== sessionRef.current || settingsPendingRef.current) return
            settingsPendingRef.current = true; setSettingsSaving(true); setSettingsError('')
            try {
              const settings = await setupApi.update(defaults)
              if (activeSession !== sessionRef.current) return
              setSetup({ configured: true, settings }); setToast('Settings saved')
            } catch (e) { if (activeSession === sessionRef.current) setSettingsError(mutationFailure(e, 'Unable to save settings.')) }
            finally { if (activeSession === sessionRef.current) { settingsPendingRef.current = false; setSettingsSaving(false) } }
          }} />}
      </div>
    </main>
    {modal && <Modal kind={modal} session={sessionRef.current} isSessionCurrent={session => session === sessionRef.current} groups={groups} peers={peers} forwards={forwards} subnet={setup?.settings?.subnet ?? ''} onClose={() => setModal(null)} onCreated={async value => {
      if (activeSession !== sessionRef.current) return
      invalidateLoads()
      if (modal === 'peer') { setModal(null); setProvision(value as Provision); setPeers(v => [...v, (value as Provision).peer]) }
      if (modal === 'group') { setGroups(v => [...v, value as Group]); setModal(null); setToast('Group created') }
      if (modal === 'forward') { setForwards(v => [...v, value as Forward]); setModal(null); setToast('Forward created') }
    }} />}
    {provision && <ProvisionDialog data={provision} group={groupName(groups, provision.peer.group_id)} onClose={() => setProvision(null)} />}
    {toast && <div className="toast" role="status"><Check size={16} />{toast}</div>}
  </div>
}

function groupName(groups: Group[], id: string) { return groups.find(g => g.id === id)?.name ?? 'Unassigned' }
function NavItem({ active, icon, label, count, onClick }: { active: boolean; icon: ReactNode; label: string; count?: number; onClick: () => void }) {
  return <button className={`nav-item ${active ? 'active' : ''}`} aria-current={active ? 'page' : undefined} onClick={onClick}>{icon}<span>{label}</span>{count !== undefined && <small>{count}</small>}</button>
}
function isPrivateSubnet(value: string) {
  const match = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.0\/24$/.exec(value.trim())
  if (!match || match.slice(1).some(part => Number(part) > 255 || String(Number(part)) !== part)) return false
  const [a, b] = match.slice(1).map(Number)
  return a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168)
}
function isEndpoint(value: string) {
  const match = /^([^:\s/]+):(\d{1,5})$/.exec(value.trim())
  if (!match || Number(match[2]) < 1 || Number(match[2]) > 65535) return false
  const host = match[1]
  if (/^[\d.]+$/.test(host)) return /^(\d{1,3}\.){3}\d{1,3}$/.test(host) && host.split('.').every(part => Number(part) <= 255)
  return host.length <= 253 && /^(?:[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)(?:\.(?:[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?))*$/.test(host)
}
function validateDefaults(endpoint: string, keepalive: string) {
  if (!isEndpoint(endpoint)) return 'Use a hostname or IPv4 address with a port.'
  const n = Number(keepalive)
  return !keepalive.trim() || !Number.isInteger(n) || n < 0 || n > 65535 ? 'Keepalive must be an integer from 0 to 65535.' : ''
}
function Brand() { return <div className="brand"><div className="brand-mark"><Network size={20} /></div><span>WireHub</span></div> }
function DefaultsFields({ endpoint, setEndpoint, keepalive, setKeepalive }: { endpoint: string; setEndpoint: (v: string) => void; keepalive: string; setKeepalive: (v: string) => void }) {
  return <><label className="form-label">Endpoint<input value={endpoint} onChange={e => setEndpoint(e.target.value)} placeholder="vpn.example.com:51820" required spellCheck={false} /></label><label className="form-label">Keepalive <span>seconds</span><input type="number" min="0" max="65535" step="1" value={keepalive} onChange={e => setKeepalive(e.target.value)} required /></label></>
}
function SetupWizard({ onConfigured }: { onConfigured: (settings: NetworkSettings) => void }) {
  const [subnet, setSubnet] = useState('10.10.10.0/24')
  const [endpoint, setEndpoint] = useState('')
  const [keepalive, setKeepalive] = useState('25')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState('')
  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    const invalid = !isPrivateSubnet(subnet) ? 'Use a private IPv4 /24, such as 10.10.10.0/24.' : validateDefaults(endpoint, keepalive)
    if (invalid) { setError(invalid); return }
    setSaving(true); setError('')
    try { onConfigured(await setupApi.create({ subnet: subnet.trim(), endpoint: endpoint.trim(), persistent_keepalive: Number(keepalive) })) }
    catch (e) {
      if (e instanceof ApiError && e.status === 409) {
        try { const status = await setupApi.get(); if (status.configured && status.settings) { onConfigured(status.settings); return } } catch { /* Preserve the conflict. */ }
        setError('Setup is already in progress. Refresh to check the network.')
      } else setError(e instanceof ApiError ? e.message : mutationFailure(e, 'Unable to save settings.'))
    } finally { setSaving(false) }
  }
  return <main className="auth-screen"><div className="auth-card setup-card"><Brand /><div className="auth-heading"><span className="eyebrow">INITIAL SETUP</span><h1>Create your network.</h1></div><form className="stack-form" onSubmit={e => void submit(e)}>
    <label className="form-label">Subnet <span>private IPv4 /24</span><input value={subnet} onChange={e => setSubnet(e.target.value)} placeholder="10.10.10.0/24" required spellCheck={false} /></label>
    <div className="address-map"><span>Hub <code>.1</code></span><span>Peers <code>.2–.254</code></span><span>Capacity <code>253</code></span></div>
    <p className="field-note">Subnet is permanent. Choose a range that does not overlap your LAN.</p>
    <DefaultsFields {...{ endpoint, setEndpoint, keepalive, setKeepalive }} />
    {error && <div className="form-error" role="alert">{error}</div>}
    <button className="button button-primary auth-submit" disabled={saving}>{saving ? <LoaderCircle size={16} className="spin" /> : <Plus size={16} />}{saving ? 'Creating…' : 'Create network'}</button>
  </form></div></main>
}
function SettingsPage({ settings, saving, error, onSave }: { settings: NetworkSettings; saving: boolean; error: string; onSave: (defaults: Pick<NetworkSettings, 'endpoint' | 'persistent_keepalive'>) => Promise<void> }) {
  const [endpoint, setEndpoint] = useState(settings.endpoint)
  const [keepalive, setKeepalive] = useState(String(settings.persistent_keepalive))
  const [validationError, setValidationError] = useState('')
  useEffect(() => { setEndpoint(settings.endpoint); setKeepalive(String(settings.persistent_keepalive)) }, [settings])
  const submit = async (e: React.FormEvent) => {
    e.preventDefault(); const invalid = validateDefaults(endpoint, keepalive)
    if (saving) return
    if (invalid) { setValidationError(invalid); return }
    setValidationError('')
    await onSave({ endpoint: endpoint.trim(), persistent_keepalive: Number(keepalive) })
  }
  return <div className="page-enter"><PageHeading title="Settings" /><section className="panel settings-panel"><div className="panel-head"><h2>Network</h2><span className="badge">Read only</span></div><label className="form-label">Subnet<input value={settings.subnet} readOnly /></label><div className="address-map"><span>Hub <code>{settings.subnet.replace(/\.0\/24$/, '.1')}</code></span><span>Peers <code>.2–.254</code></span></div><form className="stack-form settings-form" onSubmit={e => void submit(e)}><h2>Client defaults</h2><DefaultsFields {...{ endpoint, setEndpoint, keepalive, setKeepalive }} /><p className="field-note">Applies to new configurations. Update existing clients manually.</p>{(validationError || error) && <div className="form-error" role="alert">{validationError || error}</div>}<div className="dialog-actions"><button className="button button-primary" disabled={saving || (endpoint === settings.endpoint && keepalive === String(settings.persistent_keepalive))}>{saving ? <LoaderCircle size={15} className="spin" /> : <Check size={15} />}{saving ? 'Saving…' : 'Save changes'}</button></div></form></section></div>
}
function Login({ token, setToken, onSubmit, error, busy }: { token: string; setToken: (v: string) => void; onSubmit: (e: React.FormEvent) => void; error: string; busy: boolean }) {
  return <main className="auth-screen"><div className="auth-card"><Brand /><div className="auth-heading"><span className="eyebrow">NETWORK CONTROL</span><h1>Your network.<br /><span>Your rules.</span></h1></div><form onSubmit={onSubmit}><label className="form-label" htmlFor="token">Access token</label><div className="token-field"><KeyRound size={17} /><input id="token" type="password" autoComplete="off" placeholder="Enter admin token" value={token} onChange={e => setToken(e.target.value)} required aria-invalid={!!error} aria-describedby={error ? 'login-error' : 'token-note'} /></div>{error && <div className="form-error" id="login-error" role="alert">{error}</div>}<button className="button button-primary auth-submit" disabled={busy || !token.trim()}>{busy ? <LoaderCircle size={16} className="spin" /> : <ArrowUpRight size={16} />}{busy ? 'Connecting…' : 'Connect'}</button></form><p className="auth-note" id="token-note"><Shield size={13} /> Token stays in this session.</p></div><span className="auth-footer">WIREGUARD / PRIVATE NETWORK</span></main>
}
function PageHeading({ title, count, action }: { title: string; count?: string; action?: ReactNode }) {
  return <div className="page-heading"><div><h1>{title}</h1>{count && <span className="heading-count">{count}</span>}</div>{action}</div>
}
function Overview({ peers, groups, forwards, stats, busy, settings, updatedAt, setPage }: { peers: Peer[]; groups: Group[]; forwards: Forward[]; stats: { online: number; sent: number; received: number }; busy: boolean; settings: NetworkSettings | null; updatedAt: Date | null; setPage: (p: Page) => void }) {
  const total = stats.sent + stats.received
  return <div className="page-enter"><PageHeading title="Overview" action={<span className="update-time"><RefreshCw size={13} className={busy ? 'spin' : ''} />{busy ? 'Refreshing' : updatedAt ? updatedAt.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', hour12: false }) : 'Awaiting data'}</span>} />
    {settings && <section className="hub-summary"><div className="hub-identity"><Network size={22} /><div><b>Private network</b><span>WireGuard / IPv4</span></div></div><div><small>Subnet</small><b className="mono">{settings.subnet}</b></div><div><small>Endpoint</small><b className="mono">{settings.endpoint}</b></div><button className="icon-button" aria-label="Network settings" onClick={() => setPage('settings')}><ArrowUpRight size={18} /></button></section>}
    <section className="metric-grid" aria-label="Network metrics"><Metric icon={<Network />} label="Peers" value={String(peers.length).padStart(2, '0')} /><Metric icon={<Activity />} label="Recent handshakes" value={String(stats.online).padStart(2, '0')} foot="Last 3 minutes" /><Metric icon={<ArrowUpRight />} label="Sent" value={formatBytes(stats.sent)} /><Metric icon={<ArrowDownLeft />} label="Received" value={formatBytes(stats.received)} /></section>
    <div className="overview-grid"><section className="panel traffic-panel"><div className="panel-head"><h2>Traffic</h2><span className="badge">Cumulative</span></div><div className="traffic-value mono">{formatBytes(total)}</div><div className="traffic-breakdown"><div><span className="legend-dot" />Sent <b className="mono">{formatBytes(stats.sent)}</b></div><div><span className="legend-dot received" />Received <b className="mono">{formatBytes(stats.received)}</b></div></div><div className="composition-track" role="img" aria-label={`Cumulative traffic: sent ${formatBytes(stats.sent)}, received ${formatBytes(stats.received)}`}><span style={{ width: `${total ? stats.sent / total * 100 : 0}%` }} /><i style={{ width: `${total ? stats.received / total * 100 : 0}%` }} /></div><div className="composition-caption"><span>Sent {total ? Math.round(stats.sent / total * 100) : 0}%</span><span>Received {total ? Math.round(stats.received / total * 100) : 0}%</span></div></section>
    <section className="panel group-panel"><div className="panel-head"><h2>Groups</h2><button className="icon-button" aria-label="Open groups" onClick={() => setPage('groups')}><ArrowUpRight size={17} /></button></div>{busy ? <Loading /> : groups.length ? <div className="group-overview-list">{groups.slice(0, 5).map(g => <button className="group-overview-item" key={g.id} onClick={() => setPage('groups')}><span className="group-symbol"><GitBranch size={16} /></span><b>{g.name}</b><span className="mono">{peers.filter(p => p.group_id === g.id).length}<small> peers</small></span></button>)}</div> : <Empty title="No groups yet" action={<button className="text-action" onClick={() => setPage('groups')}>Create a group <ArrowUpRight size={14} /></button>} />}</section></div>
    <section className="panel recent-panel"><div className="panel-head"><h2>Peers</h2><button className="text-action" onClick={() => setPage('peers')}>View all <ArrowUpRight size={14} /></button></div>{peers.length ? <div className="mini-peer-list">{[...peers].sort((a, b) => (b.last_handshake_unix ?? 0) - (a.last_handshake_unix ?? 0)).slice(0, 4).map(p => <div className="mini-peer" key={p.id}><div className="avatar">{initials(p.name)}</div><div className="mini-peer-main"><b>{p.name}</b><small className="mono">{p.ipv4}</small></div><span className={`connection-dot ${Date.now() / 1000 - (p.last_handshake_unix ?? 0) < 180 ? 'is-online' : ''}`} /><span className="mini-status">{timeAgo(p.last_handshake_unix)}</span><span className="mini-traffic mono">↑ {formatBytes(p.sent_bytes)} <i>/</i> ↓ {formatBytes(p.received_bytes)}</span></div>)}</div> : <Empty title="No peers yet" action={<button className="text-action" onClick={() => setPage('peers')}>Add a peer <Plus size={14} /></button>} />}</section><div className="overview-foot"><span><Shield size={13} /> Default deny</span><span>{forwards.length} forwards</span></div>
  </div>
}
function Metric({ icon, label, value, foot }: { icon: ReactNode; label: string; value: string; foot?: string }) { return <div className="metric-card"><div className="metric-label">{label}{icon}</div><div className="metric-value mono">{value}</div>{foot && <div className="metric-foot">{foot}</div>}</div> }
function Empty({ title, action }: { title: string; action?: ReactNode }) { return <div className="empty-state"><div className="empty-mark"><Network size={23} /></div><b>{title}</b>{action}</div> }
function Loading() { return <div className="loading-state"><LoaderCircle size={18} className="spin" /> Loading…</div> }
function PeersPage({ peers, allCount, groups, query, setQuery, onCreate, onMove, onDelete, pendingPeers, onCopied, onCopyError }: { peers: Peer[]; allCount: number; groups: Group[]; query: string; setQuery: (v: string) => void; onCreate: () => void; onMove: (p: Peer, groupId: string) => void; onDelete: (p: Peer) => void; pendingPeers: ReadonlySet<string>; onCopied: () => void; onCopyError: () => void }) {
  return <div className="page-enter"><PageHeading title="Peers" count={`${allCount} peers`} action={<button className="button button-primary" onClick={onCreate}><Plus size={16} />New peer</button>} /><div className="list-toolbar"><div className="search-box"><Search size={16} /><input aria-label="Search peers" value={query} onChange={e => setQuery(e.target.value)} placeholder="Search name, IP, or group" />{query && <button className="icon-button tiny" aria-label="Clear search" onClick={() => setQuery('')}><X size={14} /></button>}</div><span className="result-count mono">{peers.length} / {allCount}</span></div>
    {peers.length ? <div className="peer-list">{peers.map(p => {
      const recent = Date.now() / 1000 - (p.last_handshake_unix ?? 0) < 180
      return <article className="peer-card" key={p.id}>
        <div className="avatar">{initials(p.name)}</div>
        <div className="peer-identity"><h3 title={p.name}>{p.name}</h3>
          <div className="peer-address mono">{p.ipv4}<button className="icon-button tiny" aria-label={`Copy IP for ${p.name}`} onClick={async () => { try { await navigator.clipboard.writeText(p.ipv4); onCopied() } catch { onCopyError() } }}><Copy size={13} /></button></div>
          <div className="peer-card-state"><span className={`connection-dot ${recent ? 'is-online' : ''}`} />{recent ? 'Recent handshake' : 'No recent handshake'}<span>{timeAgo(p.last_handshake_unix)}</span></div>
        </div>
        <div className="peer-row-details"><label className="peer-meta-row"><span>Group</span><select disabled={pendingPeers.has(p.id)} aria-label={`Group for ${p.name}`} value={p.group_id} onChange={e => onMove(p, e.target.value)}>{groups.map(g => <option key={g.id} value={g.id}>{g.name}</option>)}</select></label>
          <div className="peer-traffic mono"><span><ArrowUpRight size={13} />{formatBytes(p.sent_bytes)}</span><span><ArrowDownLeft size={13} />{formatBytes(p.received_bytes)}</span></div>
        </div>
        <button className="icon-button danger-on-hover peer-delete" disabled={pendingPeers.has(p.id)} aria-label={`Delete peer ${p.name}`} onClick={() => onDelete(p)}><Trash2 size={15} /></button>
      </article>
    })}</div> : <div className="panel empty-panel"><Empty title={query ? 'No matching peers' : 'No peers yet'} action={query ? <button className="text-action" onClick={() => setQuery('')}>Clear search</button> : <button className="button button-primary" onClick={onCreate}><Plus size={15} />New peer</button>} /></div>}
  </div>
}
function ForwardsPage({ forwards, subnet, peers, groups, onCreate, onDelete, loadError, onRetry }: { forwards: Forward[]; subnet: string; peers: Peer[]; groups: Group[]; onCreate: () => void; onDelete: (f: Forward) => void; loadError: string; onRetry: () => void }) {
  const hubAddress = subnet.replace(/\.0\/24$/, '.1')
  return <div className="page-enter"><PageHeading title="Forwards" count={`${forwards.length} forwards`} action={<button className="button button-primary" onClick={onCreate}><Plus size={16} />New forward</button>} /><div className="inline-banner"><Shield size={15} /><span>Internal only</span><code>{hubAddress}</code></div>{loadError ? <div className="error-banner" role="alert"><span>{loadError}</span><button onClick={onRetry}>Retry</button></div> : forwards.length ? <div className="forward-list">{forwards.map(f => { const target = peers.find(p => p.id === f.target_peer_id); return <article className="forward-card" key={f.id}><div className="forward-card-head"><span className="badge mono">{f.protocol.toUpperCase()}</span><span className="forward-active">Configured</span><button className="icon-button danger-on-hover" aria-label={`Delete forward ${f.name}`} onClick={() => onDelete(f)}><Trash2 size={15} /></button></div><h3>{f.name}</h3><div className="route-visual"><div><small>Hub</small><b className="mono">{hubAddress}:{f.target_port}</b></div><ArrowRightIcon /><div><small>{target?.name ?? 'Peer removed'}</small><b className="mono">{target?.ipv4 ?? '—'}:{f.target_port}</b></div></div><div className="forward-access"><Shield size={14} /><div className="allow-chips">{f.allowed_group_ids.length ? f.allowed_group_ids.map(id => <span key={id}>{groupName(groups, id)}</span>) : <span className="denied-label">No access</span>}</div></div></article> })}</div> : <div className="panel empty-panel"><Empty title="No forwards yet" action={<button className="button button-primary" onClick={onCreate}><Plus size={15} />New forward</button>} /></div>}</div>
}
function ArrowRightIcon() { return <ArrowUpRight className="route-arrow" size={18} /> }
function Dialog({ title, children, onClose, locked = false }: { title: string; children: ReactNode; onClose: () => void; locked?: boolean }) {
  const ref = useRef<HTMLElement>(null)
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null
    (ref.current?.querySelector<HTMLElement>('input, select') ?? ref.current?.querySelector<HTMLElement>('button'))?.focus()
    return () => previous?.focus()
  }, [])
  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === 'Escape' && !locked) { event.preventDefault(); onClose() }
    if (event.key !== 'Tab') return
    const focusable = [...(ref.current?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), summary, [tabindex="0"]') ?? [])]
    const first = focusable[0], last = focusable[focusable.length - 1]
    if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus() }
    else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus() }
  }
  return <div className="modal-backdrop" onMouseDown={e => { if (e.target === e.currentTarget && !locked) onClose() }}><section ref={ref} className="dialog" role="dialog" aria-modal="true" aria-labelledby="dialog-title" onKeyDown={onKeyDown}><div className="dialog-head"><h2 id="dialog-title">{title}</h2><button className="icon-button" aria-label="Close dialog" disabled={locked} onClick={onClose}><X size={18} /></button></div>{children}</section></div>
}
function Modal({ kind, groups, peers, forwards, subnet, session, isSessionCurrent, onClose, onCreated }: { kind: 'peer' | 'group' | 'forward'; groups: Group[]; peers: Peer[]; forwards: Forward[]; subnet: string; session: number; isSessionCurrent: (session: number) => boolean; onClose: () => void; onCreated: (value: Peer | Group | Provision | Forward) => void }) {
  const [name, setName] = useState(''), [groupId, setGroupId] = useState(groups[0]?.id ?? ''), [targetPeer, setTargetPeer] = useState(peers[0]?.id ?? '')
  const [protocol, setProtocol] = useState<'tcp' | 'udp'>('tcp'), [targetPort, setTargetPort] = useState(''), [allowed, setAllowed] = useState<string[]>([])
  const [saving, setSaving] = useState(false), [error, setError] = useState('')
  const title = { peer: 'New peer', group: 'New group', forward: 'New forward' }[kind]
  const portNumber = Number(targetPort)
  const duplicatePort = kind === 'forward' && forwards.some(f => f.protocol === protocol && f.target_port === portNumber)
  const entryAddress = subnet.replace(/\.0\/24$/, '.1')
  const submit = async (e: React.FormEvent) => {
    e.preventDefault(); if (!name.trim()) { setError('Enter a name.'); return }
    setSaving(true); setError('')
    try {
      if (kind === 'peer') {
        const r = await client.POST('/api/peers', { body: { name: name.trim(), group_id: groupId } })
        const status = r.response.status
        if (r.error || !r.data) throw new Error(status === 409 ? 'Peer name already exists.' : 'Creation unconfirmed. Refresh peers before retrying.')
        if (isSessionCurrent(session)) onCreated(r.data)
      }
      if (kind === 'group') {
        const r = await client.POST('/api/groups', { body: { name: name.trim() } })
        const status = r.response.status
        if (r.error || !r.data) throw new Error(status === 409 ? 'Group name already exists.' : 'Creation unconfirmed. Refresh groups before retrying.')
        if (isSessionCurrent(session)) onCreated(r.data)
      }
      if (kind === 'forward') {
        if (duplicatePort) throw new Error(`${protocol.toUpperCase()} port ${portNumber} is already in use.`)
        const created = await forwardsApi.create({ name: name.trim(), protocol, target_peer_id: targetPeer, target_port: portNumber, allowed_group_ids: allowed })
        if (isSessionCurrent(session)) onCreated(created)
      }
    } catch (e) { if (isSessionCurrent(session)) setError(mutationFailure(e, 'Result unconfirmed. Refresh before retrying.')) }
    finally { if (isSessionCurrent(session)) setSaving(false) }
  }
  return <Dialog title={title} onClose={onClose} locked={saving}><form className="stack-form" onSubmit={e => void submit(e)}><fieldset className="form-fields" disabled={saving}>
    <label className="form-label">Name<input value={name} onChange={e => setName(e.target.value)} placeholder={kind === 'peer' ? 'MacBook Pro' : kind === 'group' ? 'Engineering' : 'Internal docs'} required maxLength={128} /></label>
    {kind === 'peer' && <><label className="form-label">Group<select aria-label="Group" value={groupId} onChange={e => setGroupId(e.target.value)} required><option value="" disabled>Select a group</option>{groups.map(g => <option value={g.id} key={g.id}>{g.name}</option>)}</select></label><p className="field-note"><KeyRound size={14} /> Download the configuration after creation. It is shown once.</p>{!groups.length && <div className="field-warning">Create a group first.</div>}</>}
    {kind === 'forward' && <><div className="forward-entry-preview"><span>Hub entry</span><b className="mono">{entryAddress}:{targetPort || 'port'}</b></div><div className="form-row"><label className="form-label">Protocol<select aria-label="Protocol" value={protocol} onChange={e => setProtocol(e.target.value as 'tcp' | 'udp')}><option value="tcp">TCP</option><option value="udp">UDP</option></select></label><label className="form-label">Port<input type="number" min="1" max="65535" step="1" value={targetPort} onChange={e => setTargetPort(e.target.value)} placeholder="8080" required aria-invalid={duplicatePort} aria-describedby={duplicatePort ? 'forward-port-error' : undefined} /></label></div>{duplicatePort && <div className="field-warning" id="forward-port-error" role="alert">{protocol.toUpperCase()} port {portNumber} is already in use.</div>}<label className="form-label">Target peer<select aria-label="Target peer" value={targetPeer} onChange={e => setTargetPeer(e.target.value)} required><option value="" disabled>Select a peer</option>{peers.map(p => <option value={p.id} key={p.id}>{p.name} · {p.ipv4}</option>)}</select></label><fieldset className="group-chooser"><legend>Allowed groups</legend>{groups.map(g => <label key={g.id} className="check-row"><input type="checkbox" checked={allowed.includes(g.id)} onChange={e => setAllowed(e.target.checked ? [...allowed, g.id] : allowed.filter(id => id !== g.id))} /><span>{g.name}</span><small>{peers.filter(p => p.group_id === g.id).length} peers</small></label>)}</fieldset><p className="field-note">Internal access only. Hub and target use the same port.</p></>}
    </fieldset>{error && <div className="form-error" role="alert">{error}</div>}<div className="dialog-actions"><button type="button" className="button button-quiet" disabled={saving} onClick={onClose}>Cancel</button><button className="button button-primary" disabled={saving || (kind === 'peer' && !groups.length) || (kind === 'forward' && (!peers.length || duplicatePort))}>{saving && <LoaderCircle size={15} className="spin" />}{saving ? 'Creating…' : 'Create'}</button></div>
  </form></Dialog>
}
function ProvisionDialog({ data, group, onClose }: { data: Provision; group: string; onClose: () => void }) {
  const [copied, setCopied] = useState(false), [error, setError] = useState('')
  const download = () => {
    const a = document.createElement('a'), url = URL.createObjectURL(new Blob([data.config], { type: 'text/plain' }))
    a.href = url; a.download = `${data.peer.name.replace(/[^a-z0-9-_]/gi, '-')}.conf`; a.click(); window.setTimeout(() => URL.revokeObjectURL(url), 1000)
  }
  const copy = async () => { try { await navigator.clipboard.writeText(data.config); setCopied(true); setError('') } catch { setError('Copy failed. Download the configuration instead.') } }
  return <Dialog title="Peer ready" onClose={onClose}><div className="secret-warning"><KeyRound size={18} /><div><b>Save your configuration.</b><span>The private key is shown once and cannot be recovered.</span></div></div><div className="provision-details"><span>Peer</span><b>{data.peer.name}</b><span>Address</span><b className="mono">{data.peer.ipv4}/32</b><span>Group</span><b>{group}</b></div><details className="config-details"><summary>WireGuard configuration</summary><pre>{data.config}</pre></details>{error && <div className="form-error" role="alert">{error}</div>}<div className="dialog-actions"><button className="button button-quiet" onClick={() => void copy()}><Copy size={15} />{copied ? 'Copied' : 'Copy'}</button><button className="button button-primary" onClick={download}><Download size={15} />Download</button></div><button className="done-link" onClick={onClose}>Saved. Close.</button></Dialog>
}
