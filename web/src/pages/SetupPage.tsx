import { Button, Spinner } from '@fluentui/react-components';
import { ArrowUploadRegular } from '@fluentui/react-icons';
import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, getToken, setToken } from '@/api';
import type { SetupDefaults } from '@/api/types';
import { useSetupStatus } from '@/app/setupStatusContext';
import { AuthField } from '@/components/auth/AuthField';
import { LoginLayout } from '@/components/layout/LoginLayout';
import { getErrorMessage } from '@/lib/error';
import { textToUpstreamDns } from '@/lib/hubConfig';
import { useLoginPageStyles } from '@/styles/loginPage';

export default function SetupPage() {
  const styles = useLoginPageStyles();
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
        setListenPort(String(status.defaults.listen_port));
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
      <LoginLayout wide scroll heroTitle="Almost there." heroSubtitle="We could not load setup defaults. Retry to continue configuring your hub.">
        <div>
          <h2 className={styles.formTitle}>Setup unavailable</h2>
          <p className={styles.formSubtitle}>Check that the hub is running and try again.</p>
        </div>
        <div className={`${styles.errorBanner} login-animate-scale-in`} role="alert">
          {loadError}
        </div>
        <Button appearance="primary" className={styles.submitButton} onClick={() => window.location.reload()}>
          Retry
        </Button>
      </LoginLayout>
    );
  }

  if (!defaults) {
    return (
      <LoginLayout wide scroll heroTitle="Set up your hub." heroSubtitle="Import a backup or configure WireGuard, DNS, and admin access in a few steps.">
        <div className={styles.loadingState}>
          <Spinner size="medium" />
          <span>Loading setup…</span>
        </div>
      </LoginLayout>
    );
  }

  return (
    <LoginLayout
      wide
      scroll
      heroTitle="Set up your hub."
      heroSubtitle="Import a backup or configure WireGuard, DNS, and admin access in a few steps."
    >
      <div>
        <h2 className={styles.formTitle}>WireHub setup</h2>
        <p className={styles.formSubtitle}>
          Import an existing <strong>wirehub.db</strong> backup, or create a new hub below.
        </p>
      </div>

      <div className={styles.importBlock}>
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
          className={styles.secondaryButton}
          icon={<ArrowUploadRegular />}
          disabled={importing}
          onClick={() => fileRef.current?.click()}
        >
          {importing ? 'Importing…' : 'Import wirehub.db'}
        </Button>
        {importOk ? (
          <div className={`${styles.successBanner} login-animate-scale-in`} role="status">
            {importOk}
          </div>
        ) : null}
      </div>

      <hr className={styles.sectionDivider} />

      <form onSubmit={handleSubmit} className={styles.form}>
        <section className={styles.formSection}>
          <h3 className={styles.sectionTitle}>Hub</h3>
          <AuthField
            id="setup-endpoint"
            label="Public endpoint"
            required
            hint="IP or hostname clients use in WireGuard Endpoint (before the port)"
            placeholder="example.com"
            value={endpoint}
            onChange={setEndpoint}
          />
          <AuthField
            id="setup-listen-port"
            label="Client endpoint port"
            required
            hint="UDP port in peer .conf; use your public/NAT port if forwarded (default 8443)"
            type="number"
            value={listenPort}
            onChange={setListenPort}
          />
          <AuthField
            id="setup-subnet"
            label="VPN subnet"
            hint="CIDR for the WireGuard network; hub and DNS use the first host (.1)"
            value={subnet}
            onChange={setSubnet}
          />
        </section>

        <section className={styles.formSection}>
          <h3 className={styles.sectionTitle}>Admin</h3>
          <AuthField
            id="setup-admin-user"
            label="Admin username"
            hint="Web UI login username"
            value={adminUsername}
            onChange={setAdminUsername}
          />
          <AuthField
            id="setup-admin-password"
            label="Admin password"
            required
            hint="At least 8 characters"
            type="password"
            value={adminPassword}
            onChange={setAdminPassword}
          />
        </section>

        <section className={styles.formSection}>
          <h3 className={styles.sectionTitle}>Advanced</h3>
          <AuthField
            id="setup-mtu"
            label="MTU"
            hint="WireGuard interface MTU (default 1420)"
            type="number"
            value={mtu}
            onChange={setMtu}
          />
          <AuthField
            id="setup-upstream-dns"
            label="Upstream DNS servers"
            multiline
            rows={3}
            hint="Optional. Hub resolvers for non-wirehub names (server-side only). Leave empty to resolve *.wirehub only; e.g. 1.2.4.8"
            value={upstreamDns}
            onChange={setUpstreamDns}
          />
          <AuthField
            id="setup-status-interval"
            label="Status interval (seconds)"
            hint="How often peer traffic stats are polled"
            type="number"
            value={statusInterval}
            onChange={setStatusInterval}
          />
        </section>

        {error ? (
          <div className={`${styles.errorBanner} login-animate-scale-in`} role="alert">
            {error}
          </div>
        ) : null}

        <Button
          appearance="primary"
          type="submit"
          disabled={loading || !endpoint.trim() || !listenPort.trim() || !adminPassword}
          className={styles.submitButton}
        >
          {loading ? 'Setting up…' : 'Complete setup'}
        </Button>
      </form>
    </LoginLayout>
  );
}
