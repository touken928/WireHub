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
import { ArrowUploadRegular } from '@fluentui/react-icons';
import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, setToken } from '../api/client';
import type { SetupDefaults } from '../api/client';
import { textToUpstreamDns } from '../types/hubConfig';
import { LAYOUT } from '../styles/layout';

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
    width: `min(${LAYOUT.authPanelWidth}, calc(100vw - 48px))`,
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
  divider: {
    borderTop: `1px solid ${tokens.colorNeutralStroke2}`,
    margin: '8px 0',
  },
  error: {
    color: tokens.colorPaletteRedForeground1,
  },
  success: {
    color: tokens.colorPaletteGreenForeground1,
  },
});

export default function SetupPage() {
  const styles = useStyles();
  const navigate = useNavigate();
  const fileRef = useRef<HTMLInputElement>(null);
  const [defaults, setDefaults] = useState<SetupDefaults | null>(null);
  const [endpoint, setEndpoint] = useState('');
  const [subnet, setSubnet] = useState('');
  const [adminUsername, setAdminUsername] = useState('');
  const [adminPassword, setAdminPassword] = useState('');
  const [mtu, setMtu] = useState('');
  const [statusInterval, setStatusInterval] = useState('');
  const [listenPort, setListenPort] = useState('');
  const [upstreamDns, setUpstreamDns] = useState('');
  const [error, setError] = useState('');
  const [importOk, setImportOk] = useState('');
  const [loading, setLoading] = useState(false);
  const [importing, setImporting] = useState(false);

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

  const handleImport = async (file: File) => {
    setImporting(true);
    setError('');
    setImportOk('');
    try {
      await api.importDatabase(file);
      setImportOk('Database imported. Sign in with your existing admin account.');
      setTimeout(() => navigate('/login', { replace: true }), 800);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Import failed');
    } finally {
      setImporting(false);
    }
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoading(true);
    setError('');
    setImportOk('');
    try {
      const { token } = await api.setup({
        endpoint: endpoint.trim(),
        subnet: subnet.trim() || defaults?.subnet,
        admin_username: adminUsername.trim() || defaults?.admin_username,
        admin_password: adminPassword,
        listen_port: parseInt(listenPort, 10),
        mtu: parseInt(mtu, 10) || defaults?.mtu,
        status_interval: parseInt(statusInterval, 10) || defaults?.status_interval,
        upstream_dns: textToUpstreamDns(upstreamDns),
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
          Import an existing <Text weight="semibold">wirehub.db</Text> backup, or configure a new hub below.
        </Text>
        <input
          ref={fileRef}
          type="file"
          accept=".db,application/octet-stream"
          hidden
          onChange={(e) => {
            const file = e.target.files?.[0];
            if (file) void handleImport(file);
            e.target.value = '';
          }}
        />
        <Button
          icon={<ArrowUploadRegular />}
          disabled={importing}
          onClick={() => fileRef.current?.click()}
        >
          {importing ? 'Importing...' : 'Import wirehub.db'}
        </Button>
        {importOk && <Text className={styles.success}>{importOk}</Text>}

        <div className={styles.divider} />
        <Text weight="semibold">New hub</Text>

        <form onSubmit={handleSubmit} className={styles.form}>
          <Field
            label="Public endpoint"
            required
            hint="IP or hostname clients use in WireGuard Endpoint (before the port)"
          >
            <Input
              value={endpoint}
              placeholder="example.com"
              onChange={(_, d) => setEndpoint(d.value)}
            />
          </Field>
          <Field
            label="WireGuard port"
            required
            hint="UDP port in client configs (default 8443)"
          >
            <Input
              type="number"
              required
              value={listenPort}
              placeholder="8443"
              onChange={(_, d) => setListenPort(d.value)}
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
          <Field label="Admin password" required hint="At least 8 characters">
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
            hint="Public resolvers in client configs after the hub DNS IP; one per line"
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
            disabled={loading || !endpoint.trim() || !listenPort.trim() || !adminPassword}
          >
            {loading ? 'Setting up...' : 'Complete setup'}
          </Button>
        </form>
      </Card>
    </div>
  );
}
