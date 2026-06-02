import {
  Button,
  Card,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Dropdown,
  Field,
  Input,
  Option,
  Spinner,
  Switch,
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableHeaderCell,
  TableRow,
  Text,
  makeStyles,
  tokens,
} from '@fluentui/react-components';
import { AddRegular, DeleteRegular, EditRegular } from '@fluentui/react-icons';
import { useCallback, useEffect, useState } from 'react';
import { api } from '@/api';
import type { PortForward } from '@/api/types';
import { DNS_DOMAIN } from '@/constants';
import { PageHeader } from '@/components/layout/PageHeader';
import { useConfirm } from '@/components/common/useConfirm';
import { usePageLayoutStyles } from '@/styles/pageLayout';

const useStyles = makeStyles({
  card: {
    padding: '16px',
    borderRadius: tokens.borderRadiusXLarge,
  },
  hint: {
    color: tokens.colorNeutralForeground3,
    marginBottom: '16px',
  },
  actions: {
    display: 'flex',
    gap: '6px',
  },
  mono: {
    fontFamily: tokens.fontFamilyMonospace,
  },
});

type ForwardForm = {
  name: string;
  listen_port: string;
  protocol: 'tcp' | 'udp';
  target_host: string;
  target_port: string;
};

function displayTargetHost(host: string): string {
  const suffix = `.${DNS_DOMAIN}`;
  const lower = host.toLowerCase();
  if (lower.endsWith(suffix)) {
    const label = lower.slice(0, -suffix.length);
    if (label && !label.includes('.')) {
      return label;
    }
  }
  return host;
}

const emptyForm = (): ForwardForm => ({
  name: '',
  listen_port: '',
  protocol: 'tcp',
  target_host: '',
  target_port: '',
});

export default function ForwardPage() {
  const styles = useStyles();
  const pageLayout = usePageLayoutStyles();
  const { confirm } = useConfirm();

  const [rules, setRules] = useState<PortForward[]>([]);
  const [hubIP, setHubIP] = useState('');
  const [hubPort, setHubPort] = useState(8443);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [togglingId, setTogglingId] = useState<number | null>(null);
  const [error, setError] = useState('');
  const [dialogOpen, setDialogOpen] = useState(false);
  const [editing, setEditing] = useState<PortForward | null>(null);
  const [form, setForm] = useState<ForwardForm>(emptyForm);

  const load = useCallback(async () => {
    setLoading(true);
    setError('');
    try {
      const data = await api.listPortForwards();
      setRules(data.rules);
      setHubIP(data.hub_ip);
      setHubPort(data.hub_port);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load forwards');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const openCreate = () => {
    setEditing(null);
    setForm(emptyForm());
    setDialogOpen(true);
  };

  const openEdit = (rule: PortForward) => {
    setEditing(rule);
    setForm({
      name: rule.name,
      listen_port: String(rule.listen_port),
      protocol: rule.protocol,
      target_host: displayTargetHost(rule.target_host),
      target_port: String(rule.target_port),
    });
    setDialogOpen(true);
  };

  const submit = async () => {
    setSaving(true);
    setError('');
    try {
      const body = {
        name: form.name.trim(),
        listen_port: Number(form.listen_port),
        protocol: form.protocol,
        target_host: form.target_host.trim(),
        target_port: Number(form.target_port),
        enabled: editing ? editing.enabled : true,
      };
      if (editing) {
        await api.updatePortForward(editing.id, body);
      } else {
        await api.createPortForward(body);
      }
      setDialogOpen(false);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Save failed');
    } finally {
      setSaving(false);
    }
  };

  const toggleEnabled = async (rule: PortForward, enabled: boolean) => {
    setTogglingId(rule.id);
    setError('');
    try {
      await api.updatePortForward(rule.id, {
        name: rule.name,
        listen_port: rule.listen_port,
        protocol: rule.protocol,
        target_host: rule.target_host,
        target_port: rule.target_port,
        enabled,
      });
      setRules((prev) => prev.map((r) => (r.id === rule.id ? { ...r, enabled } : r)));
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Update failed');
    } finally {
      setTogglingId(null);
    }
  };

  const remove = async (rule: PortForward) => {
    const ok = await confirm({
      title: 'Delete forward?',
      message: `Remove ${rule.protocol.toUpperCase()} :${rule.listen_port} on the hub?`,
      confirmLabel: 'Delete',
    });
    if (!ok) return;
    try {
      await api.deletePortForward(rule.id);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Delete failed');
    }
  };

  return (
    <div className={pageLayout.page}>
      <PageHeader
        title="Forward"
        description="Expose TCP/UDP ports on the hub VPN address and proxy traffic to a peer hostname or external host."
      />
      <div style={{ marginBottom: 16 }}>
        <Button appearance="primary" icon={<AddRegular />} onClick={openCreate}>
          Add forward
        </Button>
      </div>

      {hubIP && (
        <Text className={styles.hint}>
          Peers connect to <span className={styles.mono}>{hubIP}:&lt;listen port&gt;</span> (hub VPN IP).
          Web UI uses port <span className={styles.mono}>{hubPort}</span>; DNS uses <span className={styles.mono}>53</span>.
          Target host can be a peer name (e.g. <span className={styles.mono}>alice</span>),{' '}
          <span className={styles.mono}>alice.{DNS_DOMAIN}</span>, or an external hostname.
        </Text>
      )}

      {error && !dialogOpen && (
        <Text style={{ color: tokens.colorPaletteRedForeground1 }}>{error}</Text>
      )}

      <Card className={styles.card}>
        {loading ? (
          <Spinner label="Loading forwards..." />
        ) : rules.length === 0 ? (
          <Text className={styles.hint}>No port forwards configured.</Text>
        ) : (
          <Table aria-label="Port forwards">
            <TableHeader>
              <TableRow>
                <TableHeaderCell>Name</TableHeaderCell>
                <TableHeaderCell>Listen</TableHeaderCell>
                <TableHeaderCell>Target</TableHeaderCell>
                <TableHeaderCell>Enabled</TableHeaderCell>
                <TableHeaderCell />
              </TableRow>
            </TableHeader>
            <TableBody>
              {rules.map((rule) => (
                <TableRow key={rule.id}>
                  <TableCell>
                    <Text>{rule.name || '—'}</Text>
                  </TableCell>
                  <TableCell>
                    <Text className={styles.mono}>
                      {rule.protocol.toUpperCase()} :{rule.listen_port}
                    </Text>
                  </TableCell>
                  <TableCell>
                    <Text className={styles.mono}>{rule.target_display}</Text>
                  </TableCell>
                  <TableCell>
                    <Switch
                      checked={rule.enabled}
                      disabled={togglingId === rule.id}
                      onChange={(_, d) => void toggleEnabled(rule, d.checked)}
                    />
                  </TableCell>
                  <TableCell>
                    <div className={styles.actions}>
                      <Button
                        size="small"
                        appearance="subtle"
                        icon={<EditRegular />}
                        onClick={() => openEdit(rule)}
                      />
                      <Button
                        size="small"
                        appearance="subtle"
                        icon={<DeleteRegular />}
                        onClick={() => void remove(rule)}
                      />
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </Card>

      <Dialog open={dialogOpen} onOpenChange={(_, data) => setDialogOpen(data.open)}>
        <DialogSurface>
          <DialogBody>
            <DialogTitle>{editing ? 'Edit forward' : 'Add forward'}</DialogTitle>
            <DialogContent style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <Field label="Name" hint="Optional label">
                <Input
                  value={form.name}
                  onChange={(_, d) => setForm((f) => ({ ...f, name: d.value }))}
                />
              </Field>
              <Field label="Listen port" required hint={`Hub VPN IP (${hubIP || 'hub'})`}>
                <Input
                  type="number"
                  value={form.listen_port}
                  onChange={(_, d) => setForm((f) => ({ ...f, listen_port: d.value }))}
                />
              </Field>
              <Field label="Protocol" required>
                <Dropdown
                  selectedOptions={[form.protocol]}
                  value={form.protocol.toUpperCase()}
                  onOptionSelect={(_, d) =>
                    setForm((f) => ({ ...f, protocol: (d.optionValue as 'tcp' | 'udp') || 'tcp' }))
                  }
                >
                  <Option value="tcp">TCP</Option>
                  <Option value="udp">UDP</Option>
                </Dropdown>
              </Field>
              <Field label="Target host" required hint={`Peer name or hostname (e.g. peer.${DNS_DOMAIN})`}>
                <Input
                  value={form.target_host}
                  onChange={(_, d) => setForm((f) => ({ ...f, target_host: d.value }))}
                />
              </Field>
              <Field label="Target port" required>
                <Input
                  type="number"
                  value={form.target_port}
                  onChange={(_, d) => setForm((f) => ({ ...f, target_port: d.value }))}
                />
              </Field>
              {error && dialogOpen && (
                <Text style={{ color: tokens.colorPaletteRedForeground1 }}>{error}</Text>
              )}
            </DialogContent>
            <DialogActions>
              <Button appearance="secondary" onClick={() => setDialogOpen(false)}>
                Cancel
              </Button>
              <Button appearance="primary" disabled={saving} onClick={() => void submit()}>
                {saving ? 'Saving…' : 'Save'}
              </Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
    </div>
  );
}
