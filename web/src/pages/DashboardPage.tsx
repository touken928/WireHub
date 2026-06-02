import {
  Subtitle2,
  Text,
  Card,
  Badge,
  Spinner,
  makeStyles,
  tokens,
} from '@fluentui/react-components';
import { useCallback, useEffect, useState } from 'react';
import { api } from '@/api';
import type { Settings } from '@/api/types';
import { DNS_DOMAIN } from '@/constants';
import NetworkUsageChart from '@/components/common/NetworkUsageChart';
import { PageHeader } from '@/components/layout/PageHeader';
import { usePageLayoutStyles } from '@/styles/pageLayout';

const useStyles = makeStyles({
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
    gridTemplateColumns: 'repeat(auto-fit, minmax(210px, 1fr))',
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
  label: {
    color: tokens.colorNeutralForeground3,
  },
  monoText: {
    fontFamily: tokens.fontFamilyMonospace,
  },
});

export default function DashboardPage() {
  const styles = useStyles();
  const pageLayout = usePageLayoutStyles();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [loading, setLoading] = useState(true);

  const load = useCallback(() => {
    api.getStatus()
      .then((data) => {
        setSettings(data.settings);
        setLoading(false);
      })
      .catch(() => setLoading(false));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  if (loading) return <Spinner label="Loading dashboard..." />;

  return (
    <div className={pageLayout.page}>
      <PageHeader
        title="Dashboard"
        description="Hub status and live network traffic across all peers."
      />
      {settings && (
        <Card className={styles.hubCard}>
          <div className={styles.hubHeader}>
            <Subtitle2>Hub</Subtitle2>
            <Badge appearance="tint" color="success">Ready</Badge>
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
              <Text className={styles.monoText}>
                {[settings.dns_ip, ...(settings.upstream_dns ?? [])].join(', ')}
              </Text>
            </div>
            <div className={styles.infoTile}>
              <Text size={200} className={styles.label}>Web UI</Text>
              <Text className={styles.monoText}>
                {typeof window !== 'undefined' ? window.location.origin : `http://${settings.dns_suffix || DNS_DOMAIN}`}
              </Text>
            </div>
          </div>
        </Card>
      )}
      <NetworkUsageChart />
    </div>
  );
}
