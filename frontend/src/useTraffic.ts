import { useCallback, useRef, useState } from 'react'

type Counter = { id: string; received_bytes: number }
export type TrafficSample = { at: number; rate: number | null }
export type Traffic = { rate: number | null; samples: TrafficSample[] }
const WINDOW_MS = 5 * 60_000

export function useTraffic() {
  const previous = useRef<{ at: number; counters: Map<string, number> } | null>(null)
  const [traffic, setTraffic] = useState<Traffic>({ rate: null, samples: [] })
  const sample = useCallback((peers: Counter[]) => {
    const at = performance.now()
    const counters = new Map(peers.map(peer => [peer.id, peer.received_bytes]))
    const before = previous.current
    let rate: number | null = null
    // Rebaseline after membership changes or counter resets: neither is traffic.
    if (before && at > before.at && counters.size === before.counters.size) {
      let bytes = 0, valid = true
      for (const [id, value] of counters) {
        const old = before.counters.get(id)
        if (old === undefined || value < old) { valid = false; break }
        bytes += value - old
      }
      if (valid) rate = bytes / ((at - before.at) / 1000)
    }
    previous.current = { at, counters }
    setTraffic(current => ({ rate, samples: [...current.samples.filter(point => point.at > at - WINDOW_MS), { at, rate }] }))
  }, [])
  const reset = useCallback((clear = false) => {
    previous.current = null
    setTraffic(current => ({ rate: null, samples: clear ? [] : current.samples }))
  }, [])
  return { traffic, sample, reset }
}
