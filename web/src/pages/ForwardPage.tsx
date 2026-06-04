import {
  Button,
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
  Text,
  makeStyles,
} from '@fluentui/react-components';
import { AddRegular } from '@fluentui/react-icons';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { api } from '@/api';
import type { PortForward } from '@/api/types';
import { ForwardRuleCard } from '@/components/forward/ForwardRuleCard';
import { RuleListSearchBar } from '@/components/common/RuleListSearchBar';
import { PageHeader } from '@/components/layout/PageHeader';
import { useConfirm } from '@/components/common/useConfirm';
import { DNS_DOMAIN, hubFQDN } from '@/constants';
import { filterPortForwards } from '@/lib/filterRules';
import { validateForwardTargetHost } from '@/lib/forwardTarget';
import { usePageLayoutStyles } from '@/styles/pageLayout';
import { useRuleListPageStyles } from '@/styles/ruleListPage';

const useStyles = makeStyles({
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

export default function ForwardPage() {
  const styles = useStyles();
  const pageLayout = usePageLayoutStyles();
  const listPage = useRuleListPageStyles();
  const { confirm } = useConfirm();

  const [rules, setRules] = useState<PortForward[]>([]);
  const [hubPort, setHubPort] = useState(80);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [dialogOpen, setDialogOpen] = useState(false);
  const [editing, setEditing] = useState<PortForward | null>(null);
  const [form, setForm] = useState<ForwardForm>(emptyForm);
  const [formError, setFormError] = useState('');
  const [searchQuery, setSearchQuery] = useState('');

  const filteredRules = useMemo(
    () => filterPortForwards(rules, searchQuery),
    [rules, searchQuery],
  );

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

  if (loading) return <Spinner label="Loading forwards..." />;

  return (
    <div className={pageLayout.page}>
      <PageHeader
        title="Forward"
        description="Proxy hub VPN ports to internal hosts. Delete a rule to stop it."
        actions={(
          <Button appearance="primary" icon={<AddRegular />} onClick={openCreate}>
            Add rule
          </Button>
        )}
      />

      {error && !dialogOpen && <Text className={listPage.error}>{error}</Text>}

      {rules.length === 0 ? (
        <div className={listPage.empty}>
          <Text>No port forwards yet. Click Add rule to create one.</Text>
        </div>
      ) : (
        <>
          <RuleListSearchBar
            value={searchQuery}
            placeholder="Name, port, protocol, target…"
            onChange={setSearchQuery}
          />
          <Text className={listPage.resultHint}>
            Showing {filteredRules.length} of {rules.length} rule{rules.length === 1 ? '' : 's'} · dial {hubFQDN()}:port · reserved DNS :53, Web/API :{hubPort}
          </Text>
          {filteredRules.length === 0 ? (
            <div className={listPage.empty}>
              <Text>No forwards match the current search.</Text>
            </div>
          ) : (
            <div className={listPage.list}>
              {filteredRules.map((rule) => (
                <ForwardRuleCard
                  key={rule.id}
                  rule={rule}
                  onEdit={openEdit}
                  onDelete={(r) => void remove(r)}
                />
              ))}
            </div>
          )}
        </>
      )}

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
                <Text className={`${listPage.error} ${styles.dialogFull}`}>{formError}</Text>
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
