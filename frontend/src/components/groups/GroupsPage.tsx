import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  Background, BaseEdge, ConnectionMode, Controls, Handle, Panel, ReactFlow,
  getSmoothStepPath, useNodesInitialized, useNodesState, useReactFlow, useStore, type Connection, type EdgeProps, type NodeProps, type ReactFlowInstance,
} from '@xyflow/react'
import { ArrowLeftRight, ArrowRight, Check, GitBranch, LayoutGrid, LoaderCircle, Plus, RotateCcw, Shield, Trash2, X } from 'lucide-react'
import type { components } from '../../api/schema'
import { autoLayout, buildEdges, connectionEnds, connectRules, disconnectRules, readLayout, readRules, sameRules, saveLayout, setLinkDirection, SIDES, type GroupEdge, type GroupNode, type Rule } from './graph'

type Group = components['schemas']['Group']
type Peer = components['schemas']['Peer']
type Props = {
  groups: Group[]; peers: Peer[]; onCreate: () => void; onSaved: () => void
  onDelete: (group: Group) => void; onSaveAcl: (id: string, allowed: string[]) => Promise<void>
  onReload: () => Promise<void>; onDirtyChange: (dirty: boolean) => void
}
const GroupCard = memo(function GroupCard({ data, selected }: NodeProps<GroupNode>) {
  return <div className={`flow-node ${selected ? 'flow-node-selected' : ''}`}>
    {SIDES.map(side => <Handle key={side} id={side} type="source" position={side} aria-label={`${data.label} ${side} handle`} />)}
    <div className="flow-icon"><GitBranch size={17} /></div><strong title={data.label}>{data.label}</strong>
    <span>{data.members} peers</span>{data.internal && <Shield className="flow-internal" size={12} aria-label="Intra-group access allowed" />}
  </div>
})
function PolicyEdge({ id, sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, markerStart, markerEnd, style, data, selected }: EdgeProps<GroupEdge>) {
  const [path] = getSmoothStepPath({ sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, borderRadius: 14 })
  return <BaseEdge id={id} path={path} markerStart={markerStart} markerEnd={markerEnd} interactionWidth={24}
    style={{ ...style, stroke: selected ? '#2563eb' : '#8090a4', strokeWidth: selected ? 2 : 1.5, ...(data?.bidirectional ? {} : { strokeDasharray: '6 4', animation: 'policy-flow .7s linear infinite' }) }} />
}
const nodeTypes = { peerGroup: GroupCard }, edgeTypes = { groupLink: PolicyEdge }
const fitOptions = { padding: .35, maxZoom: 1 }
function FitCanvas({ groupIds }: { groupIds: string }) {
  const initialized = useNodesInitialized()
  const { fitView } = useReactFlow<GroupNode, GroupEdge>()
  const width = useStore(state => state.width), height = useStore(state => state.height)
  useEffect(() => {
    if (!initialized || !width || !height) return
    const frame = requestAnimationFrame(() => { void fitView(fitOptions) })
    return () => cancelAnimationFrame(frame)
  }, [initialized, width, height, groupIds, fitView])
  return null
}

export default function GroupsPage({ groups, peers, onCreate, onSaved, onDelete, onSaveAcl, onReload, onDirtyChange }: Props) {
  const serverRules = useMemo(() => readRules(groups), [groups])
  const [draft, setDraft] = useState<Rule[] | null>(null)
  const rules = draft ?? serverRules
  const dirty = draft !== null && !sameRules(draft, serverRules)
  const [nodes, setNodes, onNodesChange] = useNodesState<GroupNode>([])
  const [both, setBoth] = useState(true)
  const [selectedGroupId, setSelectedGroupId] = useState<string | null>(null)
  const [selectedEdges, setSelectedEdges] = useState<string[]>([])
  const [saving, setSaving] = useState(false), [uncertain, setUncertain] = useState(false), [error, setError] = useState('')
  const startNode = useRef<string | null>(null), flow = useRef<ReactFlowInstance<GroupNode, GroupEdge> | null>(null)
  const positions = useRef(readLayout())
  const selectedGroup = groups.find(g => g.id === selectedGroupId)
  // Rebuild nearest-side handles from live positions during every drag.
  const edges = useMemo(() => buildEdges(rules, nodes).map(edge => ({ ...edge, selected: selectedEdges.includes(edge.id) })), [rules, nodes, selectedEdges])
  const selectedEdge = selectedEdges.length === 1 ? edges.find(e => e.id === selectedEdges[0]) : undefined
  const locked = saving || uncertain
  useEffect(() => { onDirtyChange(dirty || uncertain) }, [dirty, uncertain, onDirtyChange])
  useEffect(() => () => onDirtyChange(false), [onDirtyChange])
  useEffect(() => {
    const layout = autoLayout(groups.map(g => g.id))
    setNodes(current => groups.map(g => {
      const existing = current.find(node => node.id === g.id)
      let position = existing?.position ?? positions.current[g.id] ?? layout.get(g.id)!
      if (!existing && current.some(n => Math.abs(n.position.x - position.x) < 190 && Math.abs(n.position.y - position.y) < 90)) {
        position = { x: Math.max(...current.map(n => n.position.x)) + 240, y: position.y }
      }
      return { ...existing, id: g.id, type: 'peerGroup', position, deletable: false, selected: selectedGroupId === g.id,
        data: { label: g.name, members: peers.filter(p => p.group_id === g.id).length, internal: rules.some(r => r.from === g.id && r.to === g.id) } }
    }))
  }, [groups, peers, rules, selectedGroupId, setNodes])
  const groupIds = groups.map(g => g.id).join(',')
  const edit = useCallback((next: Rule[]) => { setDraft(next); setError('') }, [])
  const onConnect = (connection: Connection) => {
    const { from, to } = connectionEnds(connection, startNode.current)
    startNode.current = null
    if (locked || !from || !to || from === to) return
    edit(connectRules(rules, from, to, both)); setSelectedEdges([])
  }
  const removeSelected = () => {
    if (locked) return
    edit(disconnectRules(rules, edges.filter(edge => selectedEdges.includes(edge.id)))); setSelectedEdges([])
  }
  const reload = async () => {
    setSaving(true)
    try { await onReload(); setDraft(null); setUncertain(false); setSelectedEdges([]); setError('') }
    catch { setError('Unable to reload policy. The canvas may differ from the server.') }
    finally { setSaving(false) }
  }
  const save = async () => {
    setSaving(true); setError('')
    const changed = groups.filter(g => !sameRules(serverRules.filter(r => r.from === g.id), rules.filter(r => r.from === g.id)))
    try {
      const results = await Promise.allSettled(changed.map(g => onSaveAcl(g.id, rules.filter(r => r.from === g.id).map(r => r.to))))
      if (results.some(result => result.status === 'rejected')) {
        try { await onReload(); setDraft(null); setUncertain(false); setSelectedEdges([]); setError('Save incomplete. Server policy reloaded; review before editing.') }
        catch { setUncertain(true); setError('Save unconfirmed. Reload server policy to continue.') }
        return
      }
      setDraft(null); onSaved()
    } finally { setSaving(false) }
  }
  const arrange = () => {
    const layout = autoLayout(groups.map(g => g.id))
    const next = nodes.map(n => ({ ...n, position: layout.get(n.id)! }))
    setNodes(next); saveLayout(next)
    requestAnimationFrame(() => { void flow.current?.fitView(fitOptions) })
  }
  const selectGroup = (id: string) => { setSelectedGroupId(id); setSelectedEdges([]) }
  const clearSelection = () => {
    setSelectedGroupId(null); setSelectedEdges([])
    setNodes(current => current.map(node => ({ ...node, selected: false })))
  }
  const groupName = (id: string) => groups.find(g => g.id === id)?.name ?? 'Removed group'
  const outgoing = selectedGroup ? rules.filter(r => r.from === selectedGroup.id && r.to !== selectedGroup.id) : []
  const incoming = selectedGroup ? rules.filter(r => r.to === selectedGroup.id && r.from !== selectedGroup.id) : []
  const members = selectedGroup ? peers.filter(p => p.group_id === selectedGroup.id) : []
  const selfAllowed = selectedGroup ? rules.some(r => r.from === selectedGroup.id && r.to === selectedGroup.id) : false
  return <div className="page-enter policy-page"><div className="page-heading"><div><h1>Groups</h1><span className="heading-count">{groups.length} groups</span></div><button className="button button-primary" onClick={onCreate} disabled={locked}><Plus size={16} />New group</button></div>
    {error && <div className="error-banner" role="alert"><span>{error}</span>{uncertain && <button onClick={() => void reload()} disabled={saving}>Reload policy</button>}</div>}
    <div className="policy-workspace"><section className="graph-panel"><div className="graph-toolbar"><div className="graph-status"><span className={`status-dot ${dirty || uncertain ? 'status-pending' : ''}`} /><span>{uncertain ? 'Unconfirmed' : dirty ? 'Unsaved changes' : 'Policy canvas'}</span></div><div className="graph-actions"><button className="icon-button" title="Auto layout" aria-label="Auto layout" disabled={locked || !groups.length} onClick={arrange}><LayoutGrid size={16} /></button>{dirty && !uncertain && <button className="icon-button" title="Discard changes" aria-label="Discard changes" disabled={saving} onClick={() => { setDraft(null); setSelectedEdges([]); setError('') }}><RotateCcw size={16} /></button>}<button className="button button-primary compact" disabled={!dirty || locked} onClick={() => void save()}>{saving ? <LoaderCircle size={14} className="spin" /> : <Check size={14} />}{saving ? 'Saving' : 'Save'}</button></div></div>
      <div className="graph-canvas"><ReactFlow<GroupNode, GroupEdge> nodes={nodes} edges={edges} nodeTypes={nodeTypes} edgeTypes={edgeTypes}
        onInit={instance => { flow.current = instance }} onNodesChange={changes => {
          onNodesChange(changes)
          let selectedId: string | null = null
          for (const change of changes) if (change.type === 'select' && change.selected) selectedId = change.id
          if (selectedId) { setSelectedGroupId(selectedId); setSelectedEdges([]) }
          else if (changes.some(change => change.type === 'select' && !change.selected && change.id === selectedGroupId)) setSelectedGroupId(null)
        }}
        onEdgesChange={changes => {
          if (changes.some(change => change.type === 'select' && change.selected)) setSelectedGroupId(null)
          setSelectedEdges(current => {
            const ids = new Set(current)
            for (const change of changes) if (change.type === 'select') { if (change.selected) ids.add(change.id); else ids.delete(change.id) }
            return [...ids]
          })
        }} onConnectStart={(_, params) => { startNode.current = params.nodeId }} onConnect={onConnect} onConnectEnd={() => { startNode.current = null }}
        onNodeDragStop={(_, node) => saveLayout(nodes.map(n => n.id === node.id ? node : n))}
        connectionMode={ConnectionMode.Loose} nodesDraggable={!locked} nodesConnectable={!locked} edgesReconnectable={false}
        isValidConnection={connection => { const { from, to } = connectionEnds(connection, startNode.current); return !locked && from !== to && !sameRules(rules, connectRules(rules, from, to, both)) }}
        onNodeClick={(_, node) => selectGroup(node.id)} onPaneClick={clearSelection}
        onEdgeClick={(_, edge) => { setSelectedEdges([edge.id]); setSelectedGroupId(null) }}
        onBeforeDelete={async ({ edges: toRemove }) => { if (!locked && toRemove.length) { edit(disconnectRules(rules, toRemove)); setSelectedEdges([]) } return false }}
        deleteKeyCode={['Backspace', 'Delete']} fitView fitViewOptions={fitOptions} minZoom={.15} maxZoom={1.5} aria-label="Group access graph" proOptions={{ hideAttribution: true }}
        ariaLabelConfig={{ 'controls.zoomIn.ariaLabel': 'Zoom in', 'controls.zoomOut.ariaLabel': 'Zoom out', 'controls.fitView.ariaLabel': 'Fit view', 'node.a11yDescription.default': 'Press Enter to select. Use arrow keys to move. Press Escape to cancel.', 'edge.a11yDescription.default': 'Press Enter to select. Press Delete to disconnect.' }}>
        <FitCanvas groupIds={groupIds} /><Background color="#dce2ea" gap={22} size={1} /><Controls showInteractive={false} />
        <Panel position="top-left"><span className="canvas-badge"><Shield size={12} />Default deny</span></Panel>
        {selectedEdges.length > 0 && <Panel position="top-right"><button className="button button-quiet compact" disabled={locked} onClick={removeSelected}><X size={14} />Disconnect</button></Panel>}
        <Panel position="bottom-right"><div className="link-modes" role="group" aria-label="New link direction"><button aria-pressed={!both} disabled={locked} onClick={() => setBoth(false)} title="Drag from source to target"><ArrowRight size={16} />One way</button><button aria-pressed={both} disabled={locked} onClick={() => setBoth(true)}><ArrowLeftRight size={16} />Both ways</button></div></Panel>
      </ReactFlow>{!groups.length && <div className="graph-empty"><GitBranch size={26} /><b>No groups yet</b><button className="button button-primary" onClick={onCreate}><Plus size={15} />New group</button></div>}</div>
      <div className="graph-foot"><span>Drag a handle to connect</span><span className="mono">{edges.length} links</span></div>
    </section><aside className="panel group-detail">{selectedGroup ? <><div className="panel-head"><div><span className="eyebrow">GROUP</span><h2>{selectedGroup.name}</h2></div><button className="icon-button" aria-label="Close group details" onClick={clearSelection}><X size={16} /></button></div><label className="intra-group-toggle"><span>Intra-group access</span><input role="switch" type="checkbox" checked={selfAllowed} disabled={locked} onChange={e => edit(e.target.checked ? [...rules, { from: selectedGroup.id, to: selectedGroup.id }] : rules.filter(r => !(r.from === selectedGroup.id && r.to === selectedGroup.id)))} /></label>
      <div className="detail-section"><h3>Outbound <span>{outgoing.length}</span></h3>{outgoing.length ? outgoing.map(r => <div className="detail-rule" key={r.to}><ArrowRight size={14} /><span>{groupName(r.to)}</span><button className="icon-button tiny" aria-label={`Remove access to ${groupName(r.to)}`} disabled={locked} onClick={() => edit(rules.filter(x => !(x.from === r.from && x.to === r.to)))}><X size={13} /></button></div>) : <p>No access</p>}</div>
      <div className="detail-section"><h3>Inbound <span>{incoming.length}</span></h3>{incoming.length ? incoming.map(r => <div className="detail-rule" key={r.from}><ArrowRight size={14} /><span>{groupName(r.from)}</span></div>) : <p>No access</p>}</div>
      <div className="detail-section"><h3>Peers <span>{members.length}</span></h3>{members.length ? members.map(p => <div className="detail-member" key={p.id}><b>{p.name}</b><span className="mono">{p.ipv4}</span></div>) : <p>No peers</p>}</div><button className="button button-quiet detail-delete" disabled={locked || dirty} onClick={() => onDelete(selectedGroup)}><Trash2 size={14} />Delete group</button></> : selectedEdge ? <><div className="panel-head"><div><span className="eyebrow">LINK</span><h2>Access direction</h2></div></div><div className="selected-link"><b>{groupName(selectedEdge.source)}</b>{selectedEdge.data?.bidirectional ? <ArrowLeftRight size={20} /> : <ArrowRight size={20} />}<b>{groupName(selectedEdge.target)}</b></div><div className="edge-direction-options"><button className="button button-quiet" aria-pressed={!selectedEdge.data?.bidirectional} disabled={locked} onClick={() => edit(setLinkDirection(rules, selectedEdge, 'forward'))}><ArrowRight size={16} />One way</button><button className="button button-quiet" aria-pressed={!!selectedEdge.data?.bidirectional} disabled={locked} onClick={() => edit(setLinkDirection(rules, selectedEdge, 'both'))}><ArrowLeftRight size={16} />Both ways</button><button className="button button-quiet" disabled={locked} onClick={() => edit(setLinkDirection(rules, selectedEdge, 'reverse'))}><RotateCcw size={15} />Reverse</button></div><button className="button button-quiet detail-delete" disabled={locked} onClick={removeSelected}><X size={14} />Disconnect</button></> : <><div className="panel-head"><h2>Groups</h2><span className="badge mono">{groups.length}</span></div><div className="detail-group-list">{groups.map(g => <button key={g.id} onClick={() => selectGroup(g.id)}><GitBranch size={16} /><b>{g.name}</b><span className="mono">{peers.filter(p => p.group_id === g.id).length}</span></button>)}</div><div className="detail-placeholder"><GitBranch size={23} /><p>Select a group or link</p></div></>}</aside></div>
  </div>
}
