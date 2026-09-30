import { MarkerType, Position, type Connection, type Edge, type Node } from '@xyflow/react'
import type { components } from '../../api/schema'

type Group = components['schemas']['Group']
export type Rule = { from: string; to: string }
export type GroupNode = Node<{ label: string; members: number; internal: boolean }, 'peerGroup'>
export type GroupEdge = Edge<{ bidirectional: boolean }, 'groupLink'>
export const NODE_WIDTH = 184
export const NODE_HEIGHT = 80
export const SIDES = [Position.Top, Position.Right, Position.Bottom, Position.Left]
export const ruleKey = ({ from, to }: Rule) => JSON.stringify([from, to])
export function readRules(groups: Group[]): Rule[] {
  const ids = new Set(groups.map(g => g.id))
  return groups.flatMap(g => [...new Set(g.allowed_groups ?? [])].filter(to => ids.has(to)).map(to => ({ from: g.id, to })))
}
export function sameRules(a: Rule[], b: Rule[]) {
  const keys = new Set(a.map(ruleKey)), other = new Set(b.map(ruleKey))
  return keys.size === other.size && [...other].every(key => keys.has(key))
}
// Loose connections may report source/target by handle type. Policy follows the gesture.
export function connectionEnds(connection: Pick<Connection, 'source' | 'target'>, start: string | null) {
  const from = start ?? connection.source
  return { from, to: from === connection.source ? connection.target : connection.source }
}
export function connectRules(rules: Rule[], from: string, to: string, both: boolean): Rule[] {
  if (from === to) return rules
  const additions = both ? [{ from, to }, { from: to, to: from }] : [{ from, to }]
  return [...new Map([...rules, ...additions].map(rule => [ruleKey(rule), rule])).values()]
}
export function disconnectRules(rules: Rule[], edges: GroupEdge[]): Rule[] {
  return rules.filter(rule => !edges.some(edge =>
    (rule.from === edge.source && rule.to === edge.target) ||
    (edge.data?.bidirectional && rule.from === edge.target && rule.to === edge.source)))
}
export function setLinkDirection(rules: Rule[], edge: GroupEdge, mode: 'forward' | 'both' | 'reverse'): Rule[] {
  const remaining = rules.filter(r => !((r.from === edge.source && r.to === edge.target) || (r.from === edge.target && r.to === edge.source)))
  return mode === 'reverse' ? connectRules(remaining, edge.target, edge.source, false) : connectRules(remaining, edge.source, edge.target, mode === 'both')
}
export function autoLayout(ids: string[]): Map<string, { x: number; y: number }> {
  const radius = Math.max(170, ids.length * 34)
  return new Map(ids.map((id, i) => {
    const angle = 2 * Math.PI * i / ids.length - (ids.length === 2 ? 0 : Math.PI / 2)
    return [id, ids.length === 1 ? { x: 0, y: 0 } : { x: radius * Math.cos(angle) - NODE_WIDTH / 2, y: radius * Math.sin(angle) - NODE_HEIGHT / 2 }]
  }))
}
const LAYOUT_KEY = 'wirehub.group-layout.v1'
export function readLayout(): Record<string, { x: number; y: number }> {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(LAYOUT_KEY) ?? '{}')
    if (!value || typeof value !== 'object' || Array.isArray(value)) return {}
    return Object.fromEntries(Object.entries(value).filter(([, p]) => p && Number.isFinite(p.x) && Number.isFinite(p.y)))
  } catch { return {} }
}
export function saveLayout(nodes: GroupNode[]) {
  try { localStorage.setItem(LAYOUT_KEY, JSON.stringify(Object.fromEntries(nodes.map(n => [n.id, n.position])))) } catch { /* Layout remains usable when storage is unavailable. */ }
}
function nearestSide(node: GroupNode, toward: { x: number; y: number }) {
  const { x, y } = node.position
  const anchors = [
    { side: Position.Top, x: x + NODE_WIDTH / 2, y },
    { side: Position.Right, x: x + NODE_WIDTH, y: y + NODE_HEIGHT / 2 },
    { side: Position.Bottom, x: x + NODE_WIDTH / 2, y: y + NODE_HEIGHT },
    { side: Position.Left, x, y: y + NODE_HEIGHT / 2 },
  ]
  return anchors.sort((a, b) => ((a.x - toward.x) ** 2 + (a.y - toward.y) ** 2) - ((b.x - toward.x) ** 2 + (b.y - toward.y) ** 2))[0].side
}
export function buildEdges(rules: Rule[], nodes: GroupNode[]): GroupEdge[] {
  const keys = new Set(rules.map(ruleKey)), seen = new Set<string>()
  return rules.flatMap(({ from, to }) => {
    if (from === to) return [] // Self-access is a separate switch.
    const pair = JSON.stringify([from, to].sort())
    if (seen.has(pair)) return []
    seen.add(pair)
    const bidirectional = keys.has(ruleKey({ from: to, to: from }))
    const source = nodes.find(n => n.id === from), target = nodes.find(n => n.id === to)
    if (!source || !target) return []
    const arrow = { type: MarkerType.ArrowClosed, width: 16, height: 16, color: '#8090a4' }
    return [{
      id: pair, type: 'groupLink', source: from, target: to,
      sourceHandle: nearestSide(source, { x: target.position.x + NODE_WIDTH / 2, y: target.position.y + NODE_HEIGHT / 2 }),
      targetHandle: nearestSide(target, { x: source.position.x + NODE_WIDTH / 2, y: source.position.y + NODE_HEIGHT / 2 }),
      data: { bidirectional }, markerEnd: arrow, markerStart: bidirectional ? arrow : undefined,
      interactionWidth: 24, reconnectable: false, style: { stroke: '#8090a4', strokeWidth: 1.5 },
      ariaLabel: `${source.data.label} ${bidirectional ? '↔' : '→'} ${target.data.label}`,
    } satisfies GroupEdge]
  })
}
