import { useEffect, useRef } from 'react'

type Options = {
  enabled: boolean
  interval: number
  timeout?: number
  maxRetryDelay?: number
  poll: (signal: AbortSignal) => Promise<void>
  onError: (error: unknown) => void
  onPause?: () => void
  onResume?: () => void
}

// Each feed owns its request, deadline and retry timer. Lifecycle events can
// replace a suspended request without allowing its late result to publish.
export function usePolling(options: Options) {
  const latest = useRef(options)
  latest.current = options
  const { enabled, interval, timeout = 3000, maxRetryDelay = 5000 } = options
  useEffect(() => {
    if (!enabled) return
    let stopped = false, generation = 0, failures = 0, hidden = document.hidden
    let timer: number | undefined, deadline: number | undefined
    let active: AbortController | null = null
    const available = () => !stopped && !hidden && navigator.onLine
    const cancel = () => {
      ++generation
      window.clearTimeout(timer); window.clearTimeout(deadline)
      active?.abort(); active = null
    }
    const run = async () => {
      if (!available() || active) return
      window.clearTimeout(timer)
      const controller = new AbortController(), request = ++generation, started = performance.now()
      active = controller
      const current = () => !stopped && request === generation && !controller.signal.aborted
      try {
        const expired = new Promise<never>((_, reject) => {
          deadline = window.setTimeout(() => { controller.abort(); reject(new Error('Live update timed out. Retrying…')) }, timeout)
        })
        await Promise.race([latest.current.poll(controller.signal), expired])
        if (!current()) return
        failures = 0
      } catch (error) {
        if (stopped || request !== generation || !available()) return
        failures = Math.min(failures + 1, 8)
        latest.current.onError(error)
      } finally {
        if (!stopped && request === generation) {
          window.clearTimeout(deadline); active = null
          if (available()) {
            const delay = failures ? Math.min(maxRetryDelay, interval * 2 ** (failures - 1)) : Math.max(0, interval - (performance.now() - started))
            timer = window.setTimeout(() => void run(), delay)
          }
        }
      }
    }
    const pause = () => { cancel(); latest.current.onPause?.() }
    const resume = () => {
      if (!available()) return
      failures = 0
      latest.current.onResume?.()
      void run()
    }
    const visibility = () => {
      const next = document.hidden
      if (next === hidden) return
      hidden = next
      if (hidden) pause(); else resume()
    }
    const focus = () => { if (available() && !active) void run() }
    const online = () => { cancel(); resume() }
    const offline = () => { cancel(); latest.current.onError(new Error('Network offline. Waiting to reconnect…')) }
    const pagehide = () => { hidden = true; pause() }
    const pageshow = () => { if (!document.hidden) { hidden = false; if (!active) resume() } }
    document.addEventListener('visibilitychange', visibility)
    window.addEventListener('focus', focus)
    window.addEventListener('online', online)
    window.addEventListener('offline', offline)
    window.addEventListener('pagehide', pagehide)
    window.addEventListener('pageshow', pageshow)
    if (hidden) latest.current.onPause?.()
    else if (!navigator.onLine) offline()
    else void run()
    return () => {
      stopped = true; cancel()
      document.removeEventListener('visibilitychange', visibility)
      window.removeEventListener('focus', focus)
      window.removeEventListener('online', online)
      window.removeEventListener('offline', offline)
      window.removeEventListener('pagehide', pagehide)
      window.removeEventListener('pageshow', pageshow)
    }
  }, [enabled, interval, timeout, maxRetryDelay])
}
