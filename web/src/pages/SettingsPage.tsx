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
import { api } from '@/api';
import { clearToken } from '@/api/auth';
import type { HubSettings } from '@/api/types';
import { PageHeader } from '@/components/layout/PageHeader';
import { getErrorMessage } from '@/lib/error';
import { textToUpstreamDns, upstreamDnsToText } from '@/lib/hubConfig';
import { downloadBlob } from '@/lib/download';
import { usePageLayoutStyles } from '@/styles/pageLayout';

const useStyles = makeStyles({
  stack: {
    display: 'flex',
    flexDirection: 'column',
    gap: '20px',
  },
  card: {
    padding: '20px',
    display: 'flex',
    flexDirection: 'column',
    gap: '16px',
    borderRadius: tokens.borderRadiusXLarge,
  },
  cardHeader: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'flex-start',
    gap: '16px',
    flexWrap: 'wrap',
  },
  cardTitle: {
    display: 'flex',
    flexDirection: 'column',
    gap: '4px',
  },
  section: {
    display: 'flex',
    flexDirection: 'column',
    gap: '14px',
  },
  sectionDivider: {
    borderTop: `1px solid ${tokens.colorNeutralStroke2}`,
    marginTop: '4px',
    paddingTop: '20px',
  },
  hint: {
    color: tokens.colorNeutralForeground3,
    fontSize: tokens.fontSizeBase300,
  },
  error: {
    color: tokens.colorPaletteRedForeground1,
    fontSize: tokens.fontSizeBase300,
  },
  success: {
    color: tokens.colorPaletteGreenForeground1,
    fontSize: tokens.fontSizeBase300,
  },
  mono: {
    fontFamily: tokens.fontFamilyMonospace,
    fontSize: tokens.fontSizeBase300,
  },
  actions: {
    display: 'flex',
    gap: '8px',
    flexWrap: 'wrap',
  },
  dangerHint: {
    color: tokens.colorPaletteRedForeground1,
    fontSize: tokens.fontSizeBase300,
  },
});

type ReadOnlySettings = Pick<
  HubSettings,
  'endpoint' | 'subnet' | 'admin_username' | 'listen_port'
>;

export default function SettingsPage() {
  const styles = useStyles();
  const pageLayout = usePageLayoutStyles();

  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');

  const [readOnly, setReadOnly] = useState<ReadOnlySettings | null>(null);
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
    setLoading(true);
    setError('');
    try {
      const settings = await api.getSettings();
      setReadOnly({
        endpoint: settings.endpoint,
        subnet: settings.subnet,
        admin_username: settings.admin_username,
        listen_port: settings.listen_port,
      });
      setMtu(String(settings.mtu));
      setStatusInterval(String(settings.status_interval));
      setUpstreamDns(upstreamDnsToText(settings.upstream_dns));
    } catch (err) {
      setError(getErrorMessage(err, 'Failed to load settings'));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
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
        mtu: parseInt(mtu, 10),
        status_interval: parseInt(statusInterval, 10),
        upstream_dns: textToUpstreamDns(upstreamDns),
      });

      setMessage(
        result.restart_required
          ? 'Settings saved. Network stack was restarted to apply changes.'
          : 'Settings saved.',
      );
    } catch (err) {
      setError(getErrorMessage(err, 'Save failed'));
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
      setResetError(getErrorMessage(err, 'Reset failed'));
      setResetting(false);
    }
  };

  const handleExport = async () => {
    setError('');
    try {
      const blob = await api.exportDatabase();
      downloadBlob('wirehub.db', blob);
    } catch (err) {
      setError(getErrorMessage(err, 'Export failed'));
    }
  };

  return (
    <div className={`${pageLayout.page} ${styles.stack}`}>
      <PageHeader
        title="Settings"
        description="Endpoint, subnet, admin name, and client endpoint port are fixed after setup. Export downloads the full wirehub.db SQLite backup."
      />

      {error && <Text className={styles.error}>{error}</Text>}
      {message && <Text className={styles.success}>{message}</Text>}

      <Card className={styles.card}>
        <div className={styles.cardHeader}>
          <div className={styles.cardTitle}>
            <Text weight="semibold" size={400}>
              Hub configuration
            </Text>
            <Text className={styles.hint}>
              Read-only values from setup; editable network and account options below.
            </Text>
          </div>
          <div className={styles.actions}>
            <Button
              appearance="primary"
              icon={<SaveRegular />}
              disabled={saving || loading}
              onClick={() => void handleSave()}
            >
              {saving ? 'Saving…' : 'Save'}
            </Button>
            <Button icon={<ArrowDownloadRegular />} disabled={loading} onClick={() => void handleExport()}>
              Export wirehub.db
            </Button>
          </div>
        </div>

        {loading ? (
          <Spinner label="Loading settings…" />
        ) : (
          <>
            <div className={styles.section}>
              <Text weight="semibold">Hub (read-only)</Text>
              <Field label="Public endpoint">
                <Input readOnly value={readOnly?.endpoint ?? ''} className={styles.mono} />
              </Field>
              <Field label="VPN subnet">
                <Input readOnly value={readOnly?.subnet ?? ''} className={styles.mono} />
              </Field>
              <Field label="Admin username">
                <Input readOnly value={readOnly?.admin_username ?? ''} />
              </Field>
              <Field
                label="Client endpoint port"
                hint="UDP port in peer .conf (Endpoint); set at setup only. May differ from hub --port when using port forwarding."
              >
                <Input
                  readOnly
                  value={readOnly ? String(readOnly.listen_port) : ''}
                  className={styles.mono}
                />
              </Field>
            </div>

            <div className={`${styles.section} ${styles.sectionDivider}`}>
              <Text weight="semibold">Editable</Text>
              <Field label="MTU" hint="Changing MTU restarts the hub network stack">
                <Input type="number" value={mtu} onChange={(_, data) => setMtu(data.value)} />
              </Field>
              <Field label="Status interval (seconds)" hint="How often peer traffic stats are polled">
                <Input
                  type="number"
                  value={statusInterval}
                  onChange={(_, data) => setStatusInterval(data.value)}
                />
              </Field>
              <Field
                label="Upstream DNS servers"
                hint="Optional. Hub resolvers for non-wirehub names (server-side only). Leave empty to resolve *.wirehub only; one IP per line, e.g. 1.2.4.8"
              >
                <Textarea
                  value={upstreamDns}
                  rows={3}
                  onChange={(_, data) => setUpstreamDns(data.value)}
                />
              </Field>
            </div>

            <div className={`${styles.section} ${styles.sectionDivider}`}>
              <Text weight="semibold">Change password</Text>
              <Field label="Current password">
                <Input
                  type="password"
                  value={currentPassword}
                  onChange={(_, data) => setCurrentPassword(data.value)}
                />
              </Field>
              <Field label="New password" hint="At least 8 characters">
                <Input
                  type="password"
                  value={newPassword}
                  onChange={(_, data) => setNewPassword(data.value)}
                />
              </Field>
              <Field label="Confirm new password">
                <Input
                  type="password"
                  value={confirmPassword}
                  onChange={(_, data) => setConfirmPassword(data.value)}
                />
              </Field>
            </div>

            <div className={`${styles.section} ${styles.sectionDivider}`}>
              <Text weight="semibold">Danger zone</Text>
              <Text className={styles.dangerHint}>
                Reset permanently deletes all hub settings, groups, peers, and admin credentials.
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
            </div>
          </>
        )}
      </Card>

      <Dialog
        open={resetOpen}
        onOpenChange={(_, data) => {
          if (!data.open) closeResetDialog();
        }}
      >
        <DialogSurface>
          <DialogBody>
            <DialogTitle>Reset WireHub?</DialogTitle>
            <DialogContent style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
              <Text>
                This permanently deletes all hub settings, groups, peers, and admin credentials.
              </Text>
              <Field label="Admin password" required>
                <Input
                  type="password"
                  value={resetPassword}
                  autoComplete="current-password"
                  onChange={(_, data) => setResetPassword(data.value)}
                />
              </Field>
              {resetError && <Text className={styles.error}>{resetError}</Text>}
            </DialogContent>
            <DialogActions>
              <Button onClick={closeResetDialog} disabled={resetting}>
                Cancel
              </Button>
              <Button
                appearance="primary"
                onClick={() => void handleReset()}
                disabled={resetting || !resetPassword}
              >
                {resetting ? 'Resetting…' : 'Reset'}
              </Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
    </div>
  );
}
