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
import { DNS_DOMAIN, hubFQDN } from '@/constants';
import { PageHeader } from '@/components/layout/PageHeader';
import { useConfirm } from '@/components/common/useConfirm';
import { validateForwardTargetHost } from '@/lib/forwardTarget';
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
  infoBanner: {
    padding: '12px 16px',
    borderRadius: tokens.borderRadiusMedium,
    backgroundColor: tokens.colorNeutralBackground3,
    color: tokens.colorNeutralForeground2,
    fontSize: tokens.fontSizeBase300,
    lineHeight: tokens.lineHeightBase300,
  },
  mono: {
    fontFamily: tokens.fontFamilyMonospace,
    fontSize: tokens.fontSizeBase300,
  },
  hint: {
    color: tokens.colorNeutralForeground3,
    fontSize: tokens.fontSizeBase300,
  },
  error: {
    color: tokens.colorPaletteRedForeground1,
    fontSize: tokens.fontSizeBase300,
  },
  tableWrap: {
    overflowX: 'auto',
  },
  table: {
    minWidth: '720px',
  },
  colActions: {
    width: '88px',
    textAlign: 'right',
  },
  colEnabled: {
    width: '72px',
  },
  colListen: {
    minWidth: '180px',
    paddingRight: tokens.spacingHorizontalL,
  },
  colProtocol: {
    width: '80px',
  },
  colTarget: {
    minWidth: '200px',
  },
  endpoint: {
    whiteSpace: 'nowrap',
  },
  reservedPorts: {
    display: 'inline-flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    gap: '6px',
  },
  actions: {
    display: 'flex',
    justifyContent: 'flex-end',
    gap: '4px',
  },
  dialogGrid: {
    display: 'grid',
    gridTemplateColumns: '1fr 1fr',
    gap: '12px',
    alignItems: 'start',
  },
  dialogFull: {
    gridColumn: '1 / -1',
  },
});

type ForwardForm = {
  name: string;
  listen_port: string;
  protocol: 'tcp' | 'udp';
  target_host: string;
  target_port: string;
};

const emptyForm = (): ForwardForm => ({
  name: '',
  listen_port: '',
  protocol: 'tcp',
  target_host: '',
  target_port: '',
});

function formatListen(port: number) {
  return `${hubFQDN()}:${port}`;
}

export default function ForwardPage() {
  const styles = useStyles();
  const pageLayout = usePageLayoutStyles();
  const { confirm } = useConfirm();

  const [rules, setRules] = useState<PortForward[]>([]);
  const [hubPort, setHubPort] = useState(80);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [togglingId, setTogglingId] = useState<number | null>(null);
  const [error, setError] = useState('');
  const [dialogOpen, setDialogOpen] = useState(false);
  const [editing, setEditing] = useState<PortForward | null>(null);
  const [form, setForm] = useState<ForwardForm>(emptyForm);
  const [formError, setFormError] = useState('');

  const load = useCallback(async () => {
    setLoading(true);
    setError('');
    try {
      const data = await api.listPortForwards();
      setRules(data.rules);
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
    setFormError('');
    setDialogOpen(true);
  };

  const openEdit = (rule: PortForward) => {
    setEditing(rule);
    setForm({
      name: rule.name,
      listen_port: String(rule.listen_port),
      protocol: rule.protocol,
      target_host: rule.target_host,
      target_port: String(rule.target_port),
    });
    setFormError('');
    setDialogOpen(true);
  };

  const validateForm = (): boolean => {
    const hostErr = validateForwardTargetHost(form.target_host);
    if (hostErr) {
      setFormError(hostErr);
      return false;
    }
    const listen = Number(form.listen_port);
    const target = Number(form.target_port);
    if (!Number.isInteger(listen) || listen < 1 || listen > 65535) {
      setFormError('Listen port must be between 1 and 65535');
      return false;
    }
    if (!Number.isInteger(target) || target < 1 || target > 65535) {
      setFormError('Target port must be between 1 and 65535');
      return false;
    }
    setFormError('');
    return true;
  };

  const submit = async () => {
    if (!validateForm()) return;
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
      setFormError(e instanceof Error ? e.message : 'Save failed');
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

  const targetHostHint = `FQDN or IPv4 (e.g. peer.${DNS_DOMAIN}, 10.0.0.2)`;

  return (
    <div className={`${pageLayout.page} ${styles.stack}`}>
      <PageHeader
        title="Forward"
        description="Proxy hub VPN ports to internal hosts on the hub VPN address."
      />

      {!loading && (
        <div className={styles.infoBanner}>
          Clients dial <span className={styles.mono}>{hubFQDN()}:&lt;port&gt;</span> on the hub VPN
          address. Reserved:{' '}
          <span className={styles.reservedPorts}>
            <span className={styles.mono}>DNS :53</span>
            <span className={styles.mono}>Web/API :{hubPort}</span>
          </span>
          .
        </div>
      )}

      {error && !dialogOpen && <Text className={styles.error}>{error}</Text>}

      <Card className={styles.card}>
        <div className={styles.cardHeader}>
          <div className={styles.cardTitle}>
            <Text weight="semibold" size={400}>
              Port forwards
            </Text>
            <Text className={styles.hint}>Per-port TCP/UDP proxy rules on the hub VPN IP.</Text>
          </div>
          <Button appearance="primary" icon={<AddRegular />} onClick={openCreate}>
            Add rule
          </Button>
        </div>

        {loading ? (
          <Spinner label="Loading…" />
        ) : rules.length === 0 ? (
          <Text className={styles.hint}>No rules yet. Add one to get started.</Text>
        ) : (
          <div className={styles.tableWrap}>
            <Table aria-label="Port forwards" className={styles.table}>
              <TableHeader>
                <TableRow>
                  <TableHeaderCell>Name</TableHeaderCell>
                  <TableHeaderCell className={styles.colListen}>Listen</TableHeaderCell>
                  <TableHeaderCell className={styles.colTarget}>Target</TableHeaderCell>
                  <TableHeaderCell className={styles.colProtocol}>Protocol</TableHeaderCell>
                  <TableHeaderCell className={styles.colEnabled}>On</TableHeaderCell>
                  <TableHeaderCell className={styles.colActions} />
                </TableRow>
              </TableHeader>
              <TableBody>
                {rules.map((rule) => (
                  <TableRow key={rule.id}>
                    <TableCell>
                      <Text>{rule.name || '—'}</Text>
                    </TableCell>
                    <TableCell className={styles.colListen}>
                      <Text className={`${styles.mono} ${styles.endpoint}`}>
                        {formatListen(rule.listen_port)}
                      </Text>
                    </TableCell>
                    <TableCell className={styles.colTarget}>
                      <Text className={`${styles.mono} ${styles.endpoint}`}>
                        {rule.target_display}
                      </Text>
                    </TableCell>
                    <TableCell className={styles.colProtocol}>
                      <Text>{rule.protocol.toUpperCase()}</Text>
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
                          aria-label="Edit"
                          onClick={() => openEdit(rule)}
                        />
                        <Button
                          size="small"
                          appearance="subtle"
                          icon={<DeleteRegular />}
                          aria-label="Delete"
                          onClick={() => void remove(rule)}
                        />
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}
      </Card>

      <Dialog
        open={dialogOpen}
        onOpenChange={(_, data) => {
          setDialogOpen(data.open);
          if (!data.open) setFormError('');
        }}
      >
        <DialogSurface style={{ maxWidth: 480 }}>
          <DialogBody>
            <DialogTitle>{editing ? 'Edit rule' : 'Add rule'}</DialogTitle>
            <DialogContent className={styles.dialogGrid}>
              <Field className={styles.dialogFull} label="Name" hint="Optional label">
                <Input
                  value={form.name}
                  onChange={(_, d) => setForm((f) => ({ ...f, name: d.value }))}
                />
              </Field>
              <Field label="Listen port" required hint={`On ${hubFQDN()}`}>
                <Input
                  type="number"
                  min={1}
                  max={65535}
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
              <Field className={styles.dialogFull} label="Target host" required hint={targetHostHint}>
                <Input
                  value={form.target_host}
                  placeholder={`peer.${DNS_DOMAIN}`}
                  onChange={(_, d) => setForm((f) => ({ ...f, target_host: d.value }))}
                />
              </Field>
              <Field className={styles.dialogFull} label="Target port" required>
                <Input
                  type="number"
                  min={1}
                  max={65535}
                  value={form.target_port}
                  onChange={(_, d) => setForm((f) => ({ ...f, target_port: d.value }))}
                />
              </Field>
              {formError && (
                <Text className={`${styles.error} ${styles.dialogFull}`}>{formError}</Text>
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
