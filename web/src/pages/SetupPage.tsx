import {
  Input,
  Button,
  Title1,
  Text,
  Field,
  Spinner,
  Textarea,
} from '@fluentui/react-components';
import { ArrowUploadRegular } from '@fluentui/react-icons';
import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, getToken, setToken } from '@/api';
import type { SetupDefaults } from '@/api/types';
import { useSetupStatus } from '@/app/setupStatusContext';
import { AuthLayout } from '@/components/layout/AuthLayout';
import { getErrorMessage } from '@/lib/error';
import { textToUpstreamDns } from '@/lib/hubConfig';
import { useAuthLayoutStyles } from '@/styles/authLayout';

export default function SetupPage() {
  const styles = useAuthLayoutStyles();
  const navigate = useNavigate();
  const { refresh } = useSetupStatus();
  const fileRef = useRef<HTMLInputElement>(null);
  const [defaults, setDefaults] = useState<SetupDefaults | null>(null);
  const [loadError, setLoadError] = useState('');
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
    let cancelled = false;
    refresh()
      .then((status) => {
        if (cancelled) return;
        if (status.configured) {
          navigate(getToken() ? '/' : '/login', { replace: true });
          return;
        }
        setDefaults(status.defaults);
        setSubnet(status.defaults.subnet);
        setAdminUsername(status.defaults.admin_username);
        setMtu(String(status.defaults.mtu));
        setStatusInterval(String(status.defaults.status_interval));
        setUpstreamDns(status.defaults.upstream_dns.join('\n'));
      })
      .catch((err) => {
        if (!cancelled) {
          setLoadError(getErrorMessage(err, 'Failed to load setup defaults'));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [navigate, refresh]);

  const handleImport = async (file: File) => {
    setImporting(true);
    setError('');
    setImportOk('');
    try {
      await api.importDatabase(file);
      await refresh();
      setImportOk('Database imported. Sign in with your existing admin account.');
      setTimeout(() => navigate('/login', { replace: true }), 800);
    } catch (err) {
      setError(getErrorMessage(err, 'Import failed'));
    } finally {
      setImporting(false);
    }
  };

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
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
      await refresh();
      navigate('/', { replace: true });
    } catch (err) {
      setError(getErrorMessage(err, 'Setup failed'));
    } finally {
      setLoading(false);
    }
  };

  if (loadError) {
    return (
      <div className={styles.page}>
        <Text className={styles.error}>{loadError}</Text>
        <Button appearance="primary" onClick={() => window.location.reload()}>
          Retry
        </Button>
      </div>
    );
  }

  if (!defaults) {
    return (
      <div className={styles.page}>
        <Spinner label="Loading setup..." />
      </div>
    );
  }

  return (
    <AuthLayout>
      <Title1>WireHub Setup</Title1>
      <Text>
        Import an existing <Text weight="semibold">wirehub.db</Text> backup, or configure a new hub below.
      </Text>
      <input
        ref={fileRef}
        type="file"
        accept=".db,application/octet-stream"
        hidden
        onChange={(event) => {
          const file = event.target.files?.[0];
          if (file) void handleImport(file);
          event.target.value = '';
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
            onChange={(_, data) => setEndpoint(data.value)}
          />
        </Field>
        <Field
          label="Client endpoint port"
          required
          hint="UDP port in peer .conf (Endpoint); use your public/NAT port if forwarded (default 8443). Hub listens on CLI --port."
        >
          <Input
            type="number"
            required
            value={listenPort}
            placeholder="8443"
            onChange={(_, data) => setListenPort(data.value)}
          />
        </Field>
        <Field
          label="VPN subnet"
          hint="CIDR for the WireGuard network; hub and DNS use the first host (.1)"
        >
          <Input value={subnet} onChange={(_, data) => setSubnet(data.value)} />
        </Field>
        <Field label="Admin username" hint="Web UI login username">
          <Input value={adminUsername} onChange={(_, data) => setAdminUsername(data.value)} />
        </Field>
        <Field label="Admin password" required hint="At least 8 characters">
          <Input
            type="password"
            value={adminPassword}
            onChange={(_, data) => setAdminPassword(data.value)}
          />
        </Field>
        <Field label="MTU" hint="WireGuard interface MTU (default 1420)">
          <Input
            type="number"
            value={mtu}
            onChange={(_, data) => setMtu(data.value)}
          />
        </Field>
        <Field
          label="Additional DNS servers"
          hint="Public resolvers in client configs after the hub DNS IP; one per line"
        >
          <Textarea
            value={upstreamDns}
            rows={3}
            onChange={(_, data) => setUpstreamDns(data.value)}
          />
        </Field>
        <Field label="Status interval (seconds)" hint="How often peer traffic stats are polled">
          <Input
            type="number"
            value={statusInterval}
            onChange={(_, data) => setStatusInterval(data.value)}
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
    </AuthLayout>
  );
}
