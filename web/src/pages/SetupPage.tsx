import {
  Card,
  Input,
  Button,
  Title1,
  Text,
  Field,
  Spinner,
  Textarea,
  makeStyles,
  tokens,
} from '@fluentui/react-components';
import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, setToken } from '../api/client';
import type { SetupDefaults } from '../api/client';

const useStyles = makeStyles({
  page: {
    minHeight: '100vh',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    padding: '24px',
    background: `linear-gradient(135deg, ${tokens.colorBrandBackground2} 0%, ${tokens.colorNeutralBackground2} 100%)`,
  },
  card: {
    width: 'min(520px, 100%)',
    padding: '32px',
    display: 'flex',
    flexDirection: 'column',
    gap: '16px',
  },
  form: {
    display: 'flex',
    flexDirection: 'column',
    gap: '14px',
  },
  error: {
    color: tokens.colorPaletteRedForeground1,
  },
});

export default function SetupPage() {
  const styles = useStyles();
  const navigate = useNavigate();
  const [defaults, setDefaults] = useState<SetupDefaults | null>(null);
  const [endpoint, setEndpoint] = useState('');
  const [subnet, setSubnet] = useState('');
  const [adminUsername, setAdminUsername] = useState('');
  const [adminPassword, setAdminPassword] = useState('');
  const [mtu, setMtu] = useState('');
  const [statusInterval, setStatusInterval] = useState('');
  const [upstreamDns, setUpstreamDns] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    api.getSetupStatus().then((status) => {
      if (status.configured) {
        navigate('/login', { replace: true });
        return;
      }
      setDefaults(status.defaults);
      setSubnet(status.defaults.subnet);
      setAdminUsername(status.defaults.admin_username);
      setMtu(String(status.defaults.mtu));
      setStatusInterval(String(status.defaults.status_interval));
      setUpstreamDns(status.defaults.upstream_dns.join('\n'));
    }).catch((err) => {
      setError(err instanceof Error ? err.message : 'Failed to load setup defaults');
    });
  }, [navigate]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoading(true);
    setError('');
    try {
      const { token } = await api.setup({
        endpoint: endpoint.trim(),
        subnet: subnet.trim() || defaults?.subnet,
        admin_username: adminUsername.trim() || defaults?.admin_username,
        admin_password: adminPassword,
        mtu: parseInt(mtu, 10) || defaults?.mtu,
        status_interval: parseInt(statusInterval, 10) || defaults?.status_interval,
        upstream_dns: upstreamDns.split('\n').map((line) => line.trim()).filter(Boolean),
      });
      setToken(token);
      navigate('/', { replace: true });
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Setup failed');
    } finally {
      setLoading(false);
    }
  };

  if (!defaults) {
    return (
      <div className={styles.page}>
        <Spinner label="Loading setup..." />
      </div>
    );
  }

  return (
    <div className={styles.page}>
      <Card className={styles.card}>
        <Title1>WireHub Setup</Title1>
        <Text>
          Configure your hub once. These settings are stored in the database and cannot be changed from the UI later.
        </Text>
        <form onSubmit={handleSubmit} className={styles.form}>
          <Field
            label="Public endpoint"
            required
            hint="IP or hostname clients use to reach this hub (WireGuard Endpoint)"
          >
            <Input
              value={endpoint}
              placeholder="203.0.113.10"
              onChange={(_, d) => setEndpoint(d.value)}
            />
          </Field>
          <Field
            label="VPN subnet"
            hint="CIDR for the WireGuard network; hub and DNS use the first host (.1)"
          >
            <Input value={subnet} onChange={(_, d) => setSubnet(d.value)} />
          </Field>
          <Field label="Admin username" hint="Web UI login username">
            <Input value={adminUsername} onChange={(_, d) => setAdminUsername(d.value)} />
          </Field>
          <Field label="Admin password" required hint="Web UI login password">
            <Input
              type="password"
              value={adminPassword}
              onChange={(_, d) => setAdminPassword(d.value)}
            />
          </Field>
          <Field label="MTU" hint="WireGuard interface MTU (default 1420)">
            <Input
              type="number"
              value={mtu}
              onChange={(_, d) => setMtu(d.value)}
            />
          </Field>
          <Field
            label="Additional DNS servers"
            hint="Public resolvers listed in client configs after the hub DNS IP; one address per line (default 1.2.4.8, 1.1.1.1). External queries are forwarded through the hub."
          >
            <Textarea
              value={upstreamDns}
              rows={3}
              onChange={(_, d) => setUpstreamDns(d.value)}
            />
          </Field>
          <Field label="Status interval (seconds)" hint="How often peer traffic stats are polled">
            <Input
              type="number"
              value={statusInterval}
              onChange={(_, d) => setStatusInterval(d.value)}
            />
          </Field>
          {error && <Text className={styles.error}>{error}</Text>}
          <Button
            appearance="primary"
            type="submit"
            disabled={loading || !endpoint.trim() || !adminPassword}
          >
            {loading ? 'Setting up...' : 'Complete setup'}
          </Button>
        </form>
      </Card>
    </div>
  );
}
