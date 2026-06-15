import { Button, Spinner, Input, Field } from '@fluentui/react-components';
import { ArrowUploadRegular } from '@fluentui/react-icons';
import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, getToken, setToken } from '@/api';
import { getSetupToken, clearSetupToken } from '@/api/http';
import type { SetupDefaults } from '@/api/types';
import { useSetupStatus } from '@/app/setupStatusContext';
import { AuthField } from '@/components/auth/AuthField';
import { LoginLayout } from '@/components/layout/LoginLayout';
import { getErrorMessage } from '@/lib/error';
import { textToUpstreamDns } from '@/lib/hubConfig';
import { isSetupTokenRejectedError } from '@/lib/setupToken';
import { useLoginPageStyles } from '@/styles/loginPage';

const SETUP_TOKEN_KEY = 'wirehub_setup_token';

export default function SetupPage() {
  const styles = useLoginPageStyles();
  const navigate = useNavigate();
  const { refresh } = useSetupStatus();
  const fileRef = useRef<HTMLInputElement>(null);
  const [defaults, setDefaults] = useState<SetupDefaults | null>(null);
  const [loadError, setLoadError] = useState('');
  const [tokenInput, setTokenInput] = useState('');
  const [tokenError, setTokenError] = useState('');
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
  const [hasToken, setHasToken] = useState(!!getSetupToken());

  useEffect(() => {
    if (!hasToken) return;
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
          if (isSetupTokenRejectedError(err)) {
            clearSetupToken();
            navigate('/setup', { replace: true });
            setHasToken(false);
            setTokenInput('');
            setTokenError('That token did not work. Check the server logs and try again.');
            return;
          }
          setLoadError(getErrorMessage(err, 'Failed to load setup defaults'));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [navigate, refresh, hasToken]);

  const handleTokenSubmit = () => {
    const trimmed = tokenInput.trim();
    if (!trimmed) {
      setTokenError('Please enter the setup token from the server logs.');
      return;
    }
    sessionStorage.setItem(SETUP_TOKEN_KEY, trimmed);
    setTokenError('');
    setLoadError('');
    setHasToken(true);
  };

  const handleImport = async (file: File) => {
    setImporting(true);
    setError('');
    setImportOk('');
    try {
      await api.importDatabase(file);
      clearSetupToken(); // token no longer needed after import
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
      clearSetupToken(); // token no longer needed after hub is configured
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

  if (!hasToken) {
    return (
      <LoginLayout
        wide
        scroll
        heroTitle="Set up your hub."
        heroSubtitle="Enter the first-run setup token printed in the server logs to continue."
      >
        <div>
          <h2 className={styles.formTitle}>Setup token required</h2>
          <p className={styles.formSubtitle}>
            WireHub prints a one-time setup token in the server logs on startup. Use that token
            for first-time setup or after a reset, then paste it here.
          </p>
        </div>
        <Field
          label="Setup token"
          validationMessage={tokenError}
          validationState={tokenError ? 'error' : undefined}
        >
          <Input
            value={tokenInput}
            onChange={(_, data) => {
              setTokenInput(data.value);
              setTokenError('');
            }}
            placeholder="Paste the setup token here"
            onKeyDown={(e) => {
              if (e.key === 'Enter') handleTokenSubmit();
            }}
          />
        </Field>
        <div className={styles.importBlock}>
          <Button appearance="primary" className={styles.submitButton} onClick={handleTokenSubmit}>
            Continue
          </Button>
          <p className={styles.formSubtitle}>
            If you just reset the hub, this screen should open automatically from /setup?setup_token=...
          </p>
        </div>
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
            hint="Optional. Hub resolvers for non-wirehub names (server-side only). Leave empty to resolve *.wirehub only; e.g. 114.114.114.114"
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
