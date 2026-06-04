import {
  Subtitle2,
  Text,
  Card,
  Badge,
  Spinner,
  makeStyles,
  tokens,
} from '@fluentui/react-components';
import { useMemo } from 'react';
import { useStatus } from '@/app/StatusProvider';
import { formatHandshake } from '@/api';
import { DNS_DOMAIN, hubFQDN } from '@/constants';
import NetworkUsageChart from '@/components/common/NetworkUsageChart';
import { PeerStatusBadge } from '@/components/peers/PeerStatusBadge';
import { PageHeader } from '@/components/layout/PageHeader';
import { usePageLayoutStyles } from '@/styles/pageLayout';
import type { PeerStatus } from '@/api/types';

const RECENT_PEER_LIMIT = 5;

function summarizePeers(peers: PeerStatus[]) {
  let online = 0;
  let offline = 0;
  let disabled = 0;
  for (const p of peers) {
    if (!p.enabled) {
      disabled += 1;
    } else if (p.online) {
      online += 1;
    } else {
      offline += 1;
    }
  }
  return { total: peers.length, online, offline, disabled };
}

function recentPeers(peers: PeerStatus[], limit: number): PeerStatus[] {
  return [...peers]
    .sort((a, b) => b.last_handshake - a.last_handshake)
    .slice(0, limit);
}

const useStyles = makeStyles({
  row: {
    display: 'grid',
    gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))',
    gap: '16px',
  },
  hubCard: {
    padding: '20px',
    display: 'flex',
    flexDirection: 'column',
    gap: '16px',
    borderRadius: tokens.borderRadiusXLarge,
    boxShadow: tokens.shadow4,
  },
  hubHeader: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
  },
  hubGrid: {
    display: 'grid',
    gridTemplateColumns: 'repeat(auto-fit, minmax(140px, 1fr))',
    gap: '12px',
  },
  infoTile: {
    padding: '12px 14px',
    borderRadius: tokens.borderRadiusLarge,
    backgroundColor: tokens.colorNeutralBackground2,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    display: 'flex',
    flexDirection: 'column',
    gap: '4px',
  },
  statValue: {
    fontSize: tokens.fontSizeBase500,
    fontWeight: tokens.fontWeightSemibold,
  },
  label: {
    color: tokens.colorNeutralForeground3,
  },
  monoText: {
    fontFamily: tokens.fontFamilyMonospace,
  },
  peerList: {
    display: 'flex',
    flexDirection: 'column',
    gap: '8px',
  },
  peerRow: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: '12px',
    padding: '10px 12px',
    borderRadius: tokens.borderRadiusLarge,
    backgroundColor: tokens.colorNeutralBackground2,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
  },
  peerIdentity: {
    display: 'flex',
    flexDirection: 'column',
    gap: '4px',
    minWidth: 0,
  },
  peerNameRow: {
    display: 'flex',
    alignItems: 'center',
    gap: '8px',
    flexWrap: 'wrap',
  },
  peerMeta: {
    color: tokens.colorNeutralForeground3,
    fontSize: tokens.fontSizeBase200,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  peerHandshake: {
    color: tokens.colorNeutralForeground3,
    fontSize: tokens.fontSizeBase200,
    flexShrink: 0,
    textAlign: 'right',
  },
  emptyHint: {
    color: tokens.colorNeutralForeground3,
    padding: '8px 4px',
  },
  cardHint: {
    color: tokens.colorNeutralForeground3,
    fontSize: tokens.fontSizeBase200,
  },
});

export default function DashboardPage() {
  const styles = useStyles();
  const pageLayout = usePageLayoutStyles();
  const { peers, settings, connected } = useStatus();

  const summary = useMemo(() => summarizePeers(peers), [peers]);
  const recent = useMemo(() => recentPeers(peers, RECENT_PEER_LIMIT), [peers]);

  if (!connected || !settings) {
    return <Spinner label="Connecting to hub status..." />;
  }

  return (
    <div className={pageLayout.page}>
      <PageHeader
        title="Dashboard"
        description="Hub status and live network traffic across all peers."
      />
      <Card className={styles.hubCard}>
        <div className={styles.hubHeader}>
          <Subtitle2>Hub</Subtitle2>
          <Badge appearance="tint" color="success">Live</Badge>
        </div>
        <div className={styles.hubGrid}>
          <div className={styles.infoTile}>
            <Text size={200} className={styles.label}>Subnet</Text>
            <Text className={styles.monoText}>{settings.wg_subnet}</Text>
          </div>
          <div className={styles.infoTile}>
            <Text size={200} className={styles.label}>WireGuard endpoint</Text>
            <Text className={styles.monoText}>{settings.endpoint}:{settings.listen_port}</Text>
          </div>
          <div className={styles.infoTile}>
            <Text size={200} className={styles.label}>Client DNS</Text>
            <Text className={styles.monoText}>{settings.dns_ip}</Text>
          </div>
          <div className={styles.infoTile}>
            <Text size={200} className={styles.label}>Upstream DNS</Text>
            <Text className={styles.monoText}>
              {(settings.upstream_dns ?? []).length > 0
                ? (settings.upstream_dns ?? []).join(', ')
                : '—'}
            </Text>
          </div>
          <div className={styles.infoTile}>
            <Text size={200} className={styles.label}>Web UI</Text>
            <Text className={styles.monoText}>
              {typeof window !== 'undefined' ? window.location.origin : `http://${hubFQDN(settings.dns_suffix || DNS_DOMAIN)}`}
            </Text>
          </div>
        </div>
      </Card>

      <div className={styles.row}>
        <Card className={styles.hubCard}>
          <Subtitle2>Peers</Subtitle2>
          <Text size={200} className={styles.cardHint}>
            Online / offline is reported by the hub from WireGuard handshakes.
          </Text>
          <div className={styles.hubGrid}>
            <div className={styles.infoTile}>
              <Text size={200} className={styles.label}>Total</Text>
              <Text className={styles.statValue}>{summary.total}</Text>
            </div>
            <div className={styles.infoTile}>
              <Text size={200} className={styles.label}>Online</Text>
              <Text className={styles.statValue}>{summary.online}</Text>
            </div>
            <div className={styles.infoTile}>
              <Text size={200} className={styles.label}>Offline</Text>
              <Text className={styles.statValue}>{summary.offline}</Text>
            </div>
            <div className={styles.infoTile}>
              <Text size={200} className={styles.label}>Disabled</Text>
              <Text className={styles.statValue}>{summary.disabled}</Text>
            </div>
          </div>
        </Card>

        <Card className={styles.hubCard}>
          <Subtitle2>Recently active</Subtitle2>
          {recent.length === 0 ? (
            <Text size={200} className={styles.emptyHint}>No peers yet.</Text>
          ) : (
            <div className={styles.peerList}>
              {recent.map((peer) => (
                <div key={peer.id} className={styles.peerRow}>
                  <div className={styles.peerIdentity}>
                    <div className={styles.peerNameRow}>
                      <Text weight="semibold">{peer.name}</Text>
                      <PeerStatusBadge enabled={peer.enabled} online={peer.online} />
                    </div>
                    <span className={styles.peerMeta}>
                      {peer.group_name || '—'} · {peer.wg_ip}
                    </span>
                  </div>
                  <span className={styles.peerHandshake}>
                    {formatHandshake(peer.last_handshake)}
                  </span>
                </div>
              ))}
            </div>
          )}
        </Card>
      </div>

      <NetworkUsageChart />
    </div>
  );
}
