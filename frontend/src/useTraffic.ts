import { useCallback, useRef, useState } from 'react'

type Counter = { id: string; received_bytes: number }
export type TrafficSample = { at: number; rate: number | null }
export type Traffic = { rate: number | null; samples: TrafficSample[]; status: 'connecting' | 'live' | 'paused' | 'reconnecting' }
export const TRAFFIC_INTERVAL_MS = 1000
const WINDOW_MS = 5 * 60_000
const MAX_SAMPLE_GAP_MS = 10_000
const initialTraffic = (): Traffic => ({ rate: null, samples: [], status: 'connecting' })

export function useTraffic() {
  const previous = useRef<{ at: number; counters: Map<string, number> } | null>(null)
  const [traffic, setTraffic] = useState<Traffic>(initialTraffic)
  const sample = useCallback((peers: Counter[]) => {
    const at = performance.now()
    const counters = new Map(peers.map(peer => [peer.id, peer.received_bytes]))
    const before = previous.current
    // Focus/online wakeups may land next to a scheduled sample. Keep its
    // baseline until enough time has elapsed to calculate a useful rate.
    if (before && at - before.at < TRAFFIC_INTERVAL_MS / 2) return
    let rate: number | null = null
    if (before && at > before.at && at - before.at <= MAX_SAMPLE_GAP_MS) {
      let bytes = 0, comparable = 0
      for (const [id, value] of counters) {
        const old = before.counters.get(id)
        // A new/reset peer establishes its own baseline. Existing peers keep
        // contributing without turning inventory edits into gaps or spikes.
        if (old !== undefined && value >= old) { bytes += value - old; ++comparable }
      }
      if (comparable || counters.size === 0) rate = bytes / ((at - before.at) / 1000)
    }
    previous.current = { at, counters }
    setTraffic(current => ({ rate, status: 'live', samples: [...current.samples.filter(point => point.at > at - WINDOW_MS), { at, rate }] }))
  }, [])
  const interrupt = useCallback((status: 'paused' | 'reconnecting' | 'connecting', rebaseline = false) => {
    if (rebaseline) previous.current = null
    const at = performance.now()
    setTraffic(current => ({ ...current, status, samples: current.samples.filter(point => point.at > at - WINDOW_MS) }))
  }, [])
  const reset = useCallback((clear = false) => {
    previous.current = null
    setTraffic(current => clear ? initialTraffic() : { ...current, rate: null, status: 'connecting' })
  }, [])
  return { traffic, sample, interrupt, reset }
}
