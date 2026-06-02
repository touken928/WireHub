import { Card, Subtitle2, Text, makeStyles, tokens } from '@fluentui/react-components';
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { api, formatRate } from '../api/client';
import type { PeerStatus } from '../api/client';

const POLL_MS = 1000;
const TICK_MS = 100;
const WINDOW_MS = 60_000;
const CHART_HEIGHT = 160;
const PAD = { top: 12, right: 12, bottom: 24, left: 52 };

export interface NetworkSample {
  t: number;
  rxBps: number;
  txBps: number;
}

function sumTraffic(peers: PeerStatus[]) {
  let rx = 0;
  let tx = 0;
  for (const p of peers) {
    rx += p.rx_bytes;
    tx += p.tx_bytes;
  }
  return { rx, tx };
}

function buildPath(
  samples: NetworkSample[],
  width: number,
  height: number,
  key: 'rxBps' | 'txBps',
  maxY: number,
  windowStart: number,
): string {
  if (samples.length === 0) return '';
  const innerW = width - PAD.left - PAD.right;
  const innerH = height - PAD.top - PAD.bottom;

  return samples
    .map((s, i) => {
      const x = PAD.left + ((s.t - windowStart) / WINDOW_MS) * innerW;
      const y = PAD.top + innerH - (maxY > 0 ? (s[key] / maxY) * innerH : innerH);
      return `${i === 0 ? 'M' : 'L'} ${x.toFixed(1)} ${y.toFixed(1)}`;
    })
    .join(' ');
}

const useStyles = makeStyles({
  card: {
    padding: '20px',
    borderRadius: tokens.borderRadiusXLarge,
    boxShadow: tokens.shadow4,
  },
  header: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'baseline',
    marginBottom: '12px',
    flexWrap: 'wrap',
    gap: '8px',
  },
  legend: {
    display: 'flex',
    gap: '16px',
  },
  legendItem: {
    display: 'flex',
    alignItems: 'center',
    gap: '6px',
  },
  dot: {
    width: '10px',
    height: '3px',
    borderRadius: '2px',
  },
  rxDot: {
    backgroundColor: tokens.colorBrandForeground1,
  },
  txDot: {
    backgroundColor: tokens.colorPaletteGreenForeground1,
  },
  chartWrap: {
    width: '100%',
    overflow: 'hidden',
    borderRadius: tokens.borderRadiusLarge,
    backgroundColor: tokens.colorNeutralBackground2,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
  },
  svg: {
    width: '100%',
    height: `${CHART_HEIGHT}px`,
    display: 'block',
  },
  axis: {
    stroke: tokens.colorNeutralStroke2,
    strokeWidth: 1,
  },
  grid: {
    stroke: tokens.colorNeutralStroke3,
    strokeWidth: 1,
    strokeDasharray: '4 4',
  },
  rxLine: {
    fill: 'none',
    stroke: tokens.colorBrandForeground1,
    strokeWidth: 2,
    strokeLinejoin: 'round',
    strokeLinecap: 'round',
  },
  txLine: {
    fill: 'none',
    stroke: tokens.colorPaletteGreenForeground1,
    strokeWidth: 2,
    strokeLinejoin: 'round',
    strokeLinecap: 'round',
  },
  axisLabel: {
    fill: tokens.colorNeutralForeground3,
    fontSize: '11px',
  },
});

function NetworkUsageChart() {
  const styles = useStyles();
  const [samples, setSamples] = useState<NetworkSample[]>([]);
  const [now, setNow] = useState(() => Date.now());
  const [width, setWidth] = useState(800);
  const prevRef = useRef<{ t: number; rx: number; tx: number } | null>(null);
  const wrapRef = useRef<HTMLDivElement>(null);

  const recordSample = useCallback((peers: PeerStatus[]) => {
    const tick = Date.now();
    const { rx, tx } = sumTraffic(peers);
    const prev = prevRef.current;

    if (prev && tick > prev.t) {
      const dt = (tick - prev.t) / 1000;
      const rxDelta = rx >= prev.rx ? rx - prev.rx : 0;
      const txDelta = tx >= prev.tx ? tx - prev.tx : 0;
      const sample: NetworkSample = {
        t: tick,
        rxBps: rxDelta / dt,
        txBps: txDelta / dt,
      };
      const cutoff = tick - WINDOW_MS;
      setSamples((s) => [...s, sample].filter((p) => p.t >= cutoff));
    }

    prevRef.current = { t: tick, rx, tx };
    setNow(tick);
  }, []);

  useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => {
      setWidth(Math.max(320, Math.floor(entry.contentRect.width)));
    });
    ro.observe(el);
    setWidth(Math.max(320, el.clientWidth));
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    let cancelled = false;

    const poll = () => {
      api.getStatus()
        .then((d) => {
          if (!cancelled) recordSample(d.peers ?? []);
        })
        .catch(() => {});
    };

    poll();
    const pollId = setInterval(poll, POLL_MS);
    const tickId = setInterval(() => setNow(Date.now()), TICK_MS);

    return () => {
      cancelled = true;
      clearInterval(pollId);
      clearInterval(tickId);
    };
  }, [recordSample]);

  const windowStart = now - WINDOW_MS;
  const visible = useMemo(
    () => samples.filter((s) => s.t >= windowStart),
    [samples, windowStart],
  );

  const maxY = useMemo(
    () => Math.max(1024, ...visible.flatMap((s) => [s.rxBps, s.txBps])),
    [visible],
  );

  const innerH = CHART_HEIGHT - PAD.top - PAD.bottom;
  const yTicks = useMemo(() => [0, maxY * 0.5, maxY], [maxY]);
  const latest = visible.at(-1);

  const rxPath = useMemo(
    () => buildPath(visible, width, CHART_HEIGHT, 'rxBps', maxY, windowStart),
    [visible, width, maxY, windowStart],
  );
  const txPath = useMemo(
    () => buildPath(visible, width, CHART_HEIGHT, 'txBps', maxY, windowStart),
    [visible, width, maxY, windowStart],
  );

  return (
    <Card className={styles.card}>
      <div className={styles.header}>
        <Subtitle2>Network</Subtitle2>
        <div className={styles.legend}>
          <div className={styles.legendItem}>
            <span className={`${styles.dot} ${styles.rxDot}`} />
            <Text size={200}>Download {formatRate(latest?.rxBps ?? 0)}</Text>
          </div>
          <div className={styles.legendItem}>
            <span className={`${styles.dot} ${styles.txDot}`} />
            <Text size={200}>Upload {formatRate(latest?.txBps ?? 0)}</Text>
          </div>
        </div>
      </div>
      <div ref={wrapRef} className={styles.chartWrap}>
        <svg className={styles.svg} viewBox={`0 0 ${width} ${CHART_HEIGHT}`} preserveAspectRatio="none">
          {yTicks.map((v) => {
            const y = PAD.top + innerH - (maxY > 0 ? (v / maxY) * innerH : innerH);
            return (
              <g key={v}>
                <line x1={PAD.left} y1={y} x2={width - PAD.right} y2={y} className={styles.grid} />
                <text x={PAD.left - 8} y={y + 4} textAnchor="end" className={styles.axisLabel}>
                  {formatRate(v)}
                </text>
              </g>
            );
          })}
          <line x1={PAD.left} y1={PAD.top} x2={PAD.left} y2={CHART_HEIGHT - PAD.bottom} className={styles.axis} />
          <line
            x1={PAD.left}
            y1={CHART_HEIGHT - PAD.bottom}
            x2={width - PAD.right}
            y2={CHART_HEIGHT - PAD.bottom}
            className={styles.axis}
          />
          <text x={PAD.left} y={CHART_HEIGHT - 6} className={styles.axisLabel}>-60s</text>
          <text x={width - PAD.right} y={CHART_HEIGHT - 6} textAnchor="end" className={styles.axisLabel}>
            now
          </text>
          {rxPath && <path d={rxPath} className={styles.rxLine} />}
          {txPath && <path d={txPath} className={styles.txLine} />}
        </svg>
      </div>
    </Card>
  );
}

export default memo(NetworkUsageChart);
