import { useEffect, useState } from 'react'
import { TRAFFIC_INTERVAL_MS, type Traffic } from '../useTraffic'

export default function TrafficChart({ traffic, formatRate }: { traffic: Traffic; formatRate: (rate: number) => string }) {
  const [now, setNow] = useState(() => performance.now())
  useEffect(() => {
    const timer = window.setInterval(() => setNow(performance.now()), TRAFFIC_INTERVAL_MS)
    return () => window.clearInterval(timer)
  }, [])
  const end = Math.max(now, traffic.samples[traffic.samples.length - 1]?.at ?? 0)
  const start = end - 5 * 60_000
  const samples = traffic.samples.filter(point => point.at > start)
  const peak = Math.max(0, ...samples.map(point => point.rate ?? 0))
  const paths: string[] = []
  let path = ''
  for (const point of samples) {
    if (point.rate === null) { if (path) paths.push(path); path = ''; continue }
    const x = (point.at - start) / 300_000 * 600
    const y = 90 - (peak ? point.rate / peak * 80 : 0)
    path += `${path ? ' L' : 'M'}${x.toFixed(2)},${y.toFixed(2)}`
  }
  if (path) paths.push(path)
  return <div className="traffic-chart">
    <div className="traffic-chart-scale"><span>Transfer rate</span><span className="mono">Peak {formatRate(peak)}</span></div>
    <svg viewBox="0 0 600 100" preserveAspectRatio="none" role="img" aria-label={`Transfer rate over the last 5 minutes. Peak ${formatRate(peak)}.`}>
      {[10, 50, 90].map(y => <line key={y} x1="0" y1={y} x2="600" y2={y} className="traffic-grid-line" />)}
      {paths.map((d, i) => <path key={i} d={d} className="traffic-line" />)}
      {samples.filter(point => point.rate !== null).map(point => <circle key={point.at} cx={(point.at - start) / 300_000 * 600} cy={90 - (peak ? point.rate! / peak * 80 : 0)} r="2" className="traffic-point" />)}
    </svg>
    <div className="traffic-chart-caption"><span>5 minutes ago</span><span>Now</span></div>
  </div>
}
