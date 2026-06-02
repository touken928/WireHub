import {
  Button,
  Card,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Field,
  Input,
  Spinner,
  Text,
  Textarea,
  makeStyles,
  tokens,
} from '@fluentui/react-components';
import { ArrowDownloadRegular, ArrowResetRegular, SaveRegular } from '@fluentui/react-icons';
import { useCallback, useEffect, useState } from 'react';
import { api, clearToken } from '../api/client';
import type { HubSettings } from '../api/client';
import { textToUpstreamDns, upstreamDnsToText } from '../types/hubConfig';
import PageHeader from '../components/PageHeader';
import { downloadBlob } from '../utils/download';
import { usePageLayoutStyles } from '../styles/pageLayout';

const useStyles = makeStyles({
  card: {
    padding: '20px',
    display: 'flex',
    flexDirection: 'column',
    gap: '14px',
  },
  sectionTitle: {
    marginTop: '4px',
  },
  actions: {
    display: 'flex',
    gap: '8px',
    flexWrap: 'wrap',
    marginTop: '8px',
  },
  error: {
    color: tokens.colorPaletteRedForeground1,
  },
  success: {
    color: tokens.colorPaletteGreenForeground1,
  },
  mono: {
    fontFamily: tokens.fontFamilyMonospace,
  },
  dangerCard: {
    padding: '20px',
    display: 'flex',
    flexDirection: 'column',
    gap: '12px',
    border: `1px solid ${tokens.colorPaletteRedBorder2}`,
  },
});

export default function SettingsPage() {
  const styles = useStyles();
  const pageLayout = usePageLayoutStyles();
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');

  const [readOnly, setReadOnly] = useState<Pick<HubSettings, 'endpoint' | 'subnet' | 'admin_username'> | null>(null);
  const [listenPort, setListenPort] = useState('');
  const [mtu, setMtu] = useState('');
  const [statusInterval, setStatusInterval] = useState('');
  const [upstreamDns, setUpstreamDns] = useState('');
  const [currentPassword, setCurrentPassword] = useState('');
  const [newPassword, setNewPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [resetOpen, setResetOpen] = useState(false);
  const [resetPassword, setResetPassword] = useState('');
  const [resetError, setResetError] = useState('');
  const [resetting, setResetting] = useState(false);

  const load = useCallback(async () => {
    const s = await api.getSettings();
    setReadOnly({
      endpoint: s.endpoint,
      subnet: s.subnet,
      admin_username: s.admin_username,
    });
    setListenPort(String(s.listen_port));
    setMtu(String(s.mtu));
    setStatusInterval(String(s.status_interval));
    setUpstreamDns(upstreamDnsToText(s.upstream_dns));
    setLoading(false);
  }, []);

  useEffect(() => {
    load().catch((err) => {
      setError(err instanceof Error ? err.message : 'Failed to load settings');
      setLoading(false);
    });
  }, [load]);

  const handleSave = async () => {
    setSaving(true);
    setError('');
    setMessage('');
    try {
      if (newPassword || confirmPassword || currentPassword) {
        if (newPassword !== confirmPassword) {
          throw new Error('New passwords do not match');
        }
        if (!currentPassword) {
          throw new Error('Current password is required to set a new password');
        }
        await api.changePassword(currentPassword, newPassword);
        setCurrentPassword('');
        setNewPassword('');
        setConfirmPassword('');
      }

      const result = await api.updateSettings({
        listen_port: parseInt(listenPort, 10),
        mtu: parseInt(mtu, 10),
        status_interval: parseInt(statusInterval, 10),
        upstream_dns: textToUpstreamDns(upstreamDns),
      });

      if (result.restart_required) {
        setMessage('Settings saved. Network stack was restarted to apply changes.');
      } else {
        setMessage('Settings saved.');
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Save failed');
    } finally {
      setSaving(false);
    }
  };

  const closeResetDialog = () => {
    setResetOpen(false);
    setResetPassword('');
    setResetError('');
  };

  const handleReset = async () => {
    if (!resetPassword) {
      setResetError('Password is required');
      return;
    }
    setResetting(true);
    setResetError('');
    setError('');
    try {
      await api.reset(resetPassword);
      clearToken();
      window.location.href = '/setup';
    } catch (err) {
      setResetError(err instanceof Error ? err.message : 'Reset failed');
      setResetting(false);
    }
  };

  const handleExport = async () => {
    setError('');
    try {
      const blob = await api.exportDatabase();
      downloadBlob('wirehub.db', blob);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Export failed');
    }
  };

  if (loading) return <Spinner label="Loading settings..." />;

  return (
    <div className={pageLayout.page}>
      <PageHeader
        title="Settings"
        description="Endpoint, subnet, and admin name are fixed after setup. WireGuard port is used in client configs only. Export downloads the full wirehub.db SQLite backup."
      />

      <Card className={styles.card}>
        <Text weight="semibold" className={styles.sectionTitle}>Hub (read-only)</Text>
        <Field label="Public endpoint">
          <Input readOnly value={readOnly?.endpoint ?? ''} className={styles.mono} />
        </Field>
        <Field label="VPN subnet">
          <Input readOnly value={readOnly?.subnet ?? ''} className={styles.mono} />
        </Field>
        <Field label="Admin username">
          <Input readOnly value={readOnly?.admin_username ?? ''} />
        </Field>
      </Card>

      <Card className={styles.card}>
        <Text weight="semibold" className={styles.sectionTitle}>Editable</Text>
        <Field
          label="WireGuard port"
          hint="UDP port in peer configs (default 8443). Changing this restarts the hub network stack."
        >
          <Input type="number" value={listenPort} onChange={(_, d) => setListenPort(d.value)} />
        </Field>
        <Field label="MTU" hint="Changing MTU restarts the hub network stack">
          <Input type="number" value={mtu} onChange={(_, d) => setMtu(d.value)} />
        </Field>
        <Field label="Status interval (seconds)" hint="How often peer traffic stats are polled">
          <Input type="number" value={statusInterval} onChange={(_, d) => setStatusInterval(d.value)} />
        </Field>
        <Field
          label="Additional DNS servers"
          hint="Listed in client configs after the hub DNS IP; one address per line"
        >
          <Textarea rows={3} value={upstreamDns} onChange={(_, d) => setUpstreamDns(d.value)} />
        </Field>
      </Card>

      <Card className={styles.card}>
        <Text weight="semibold" className={styles.sectionTitle}>Change password</Text>
        <Field label="Current password">
          <Input
            type="password"
            value={currentPassword}
            onChange={(_, d) => setCurrentPassword(d.value)}
          />
        </Field>
        <Field label="New password" hint="At least 8 characters">
          <Input type="password" value={newPassword} onChange={(_, d) => setNewPassword(d.value)} />
        </Field>
        <Field label="Confirm new password">
          <Input
            type="password"
            value={confirmPassword}
            onChange={(_, d) => setConfirmPassword(d.value)}
          />
        </Field>
      </Card>

      <Card className={styles.dangerCard}>
        <Text weight="semibold">Danger zone</Text>
        <Text size={200} className={pageLayout.muted}>
          Reset permanently deletes all hub settings, groups, users, and admin credentials.
        </Text>
        <Button
          appearance="secondary"
          icon={<ArrowResetRegular />}
          onClick={() => {
            setResetError('');
            setResetPassword('');
            setResetOpen(true);
          }}
        >
          Reset WireHub
        </Button>
      </Card>

      {error && <Text className={styles.error}>{error}</Text>}
      {message && <Text className={styles.success}>{message}</Text>}

      <div className={styles.actions}>
        <Button
          appearance="primary"
          icon={<SaveRegular />}
          disabled={saving}
          onClick={() => void handleSave()}
        >
          {saving ? 'Saving...' : 'Save'}
        </Button>
        <Button icon={<ArrowDownloadRegular />} onClick={() => void handleExport()}>
          Export wirehub.db
        </Button>
      </div>

      <Dialog
        open={resetOpen}
        onOpenChange={(_, d) => {
          if (!d.open) closeResetDialog();
        }}
      >
        <DialogSurface>
          <DialogBody>
            <DialogTitle>Reset WireHub?</DialogTitle>
            <DialogContent style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
              <Text>
                This permanently deletes all hub settings, groups, users, and admin credentials.
              </Text>
              <Field label="Admin password" required>
                <Input
                  type="password"
                  value={resetPassword}
                  autoComplete="current-password"
                  onChange={(_, d) => setResetPassword(d.value)}
                />
              </Field>
              {resetError && <Text className={styles.error}>{resetError}</Text>}
            </DialogContent>
            <DialogActions>
              <Button onClick={closeResetDialog} disabled={resetting}>Cancel</Button>
              <Button
                appearance="primary"
                onClick={() => void handleReset()}
                disabled={resetting || !resetPassword}
              >
                {resetting ? 'Resetting...' : 'Reset'}
              </Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
    </div>
  );
}
