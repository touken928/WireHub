import {
  Button,
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
  makeStyles,
} from '@fluentui/react-components';
import { AddRegular } from '@fluentui/react-icons';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { api } from '@/api';
import type { PeerGroup, ServiceMap } from '@/api/types';
import { AllowedGroupsPicker } from '@/components/common/AllowedGroupsPicker';
import { RuleListSearchBar } from '@/components/common/RuleListSearchBar';
import { PageHeader } from '@/components/layout/PageHeader';
import { MapCard } from '@/components/maps/MapCard';
import { useConfirm } from '@/components/common/useConfirm';
import { ALLOWED_GROUPS_REQUIRED } from '@/lib/allowedGroups';
import { filterServiceMaps } from '@/lib/filterRules';
import { validateForwardTargetHost } from '@/lib/forwardTarget';
import { DNS_DOMAIN } from '@/constants';
import { usePageLayoutStyles } from '@/styles/pageLayout';
import { useRuleListPageStyles } from '@/styles/ruleListPage';

const useStyles = makeStyles({
  dialogGrid: { display: 'flex', flexDirection: 'column', gap: '12px' },
});

type FormState = {
  name: string;
  slug: string;
  target_host: string;
  allowed_group_ids: number[];
};

const emptyForm = (): FormState => ({
  name: '',
  slug: '',
  target_host: '',
  allowed_group_ids: [],
});

export default function MapsPage() {
  const styles = useStyles();
  const pageLayout = usePageLayoutStyles();
  const listPage = useRuleListPageStyles();
  const { confirm } = useConfirm();
  const [maps, setMaps] = useState<ServiceMap[]>([]);
  const [groups, setGroups] = useState<PeerGroup[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [dialogOpen, setDialogOpen] = useState(false);
  const [editing, setEditing] = useState<ServiceMap | null>(null);
  const [form, setForm] = useState<FormState>(emptyForm());
  const [formError, setFormError] = useState('');
  const [saving, setSaving] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');

  const filteredMaps = useMemo(
    () => filterServiceMaps(maps, groups, searchQuery),
    [maps, groups, searchQuery],
  );

  const load = useCallback(async () => {
    setLoading(true);
    setError('');
    try {
      const [mapRes, groupList] = await Promise.all([api.listMaps(), api.listGroups()]);
      setMaps(mapRes.maps ?? []);
      setGroups(groupList);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const openCreate = () => {
    setEditing(null);
    const defaultGroups = groups.length === 1 ? [groups[0].id] : [];
    setForm({ ...emptyForm(), allowed_group_ids: defaultGroups });
    setFormError('');
    setDialogOpen(true);
  };

  const openEdit = (r: ServiceMap) => {
    setEditing(r);
    setForm({
      name: r.name,
      slug: r.slug,
      target_host: r.target_host,
      allowed_group_ids: [...r.allowed_group_ids],
    });
    setFormError('');
    setDialogOpen(true);
  };

  const save = async () => {
    setFormError('');
    const slug = form.slug.trim().toLowerCase();
    if (!slug) {
      setFormError('DNS slug is required');
      return;
    }
    const targetErr = validateForwardTargetHost(form.target_host);
    if (targetErr) {
      setFormError(targetErr);
      return;
    }
    if (form.allowed_group_ids.length === 0) {
      setFormError(ALLOWED_GROUPS_REQUIRED);
      return;
    }
    setSaving(true);
    try {
      const body = {
        name: form.name.trim(),
        slug,
        target_host: form.target_host.trim(),
        allowed_group_ids: form.allowed_group_ids,
      };
      if (editing) {
        await api.updateMap(editing.id, body);
      } else {
        await api.createMap(body);
      }
      setDialogOpen(false);
      await load();
    } catch (err) {
      setFormError(err instanceof Error ? err.message : 'Save failed');
    } finally {
      setSaving(false);
    }
  };

  const remove = async (r: ServiceMap) => {
    const ok = await confirm({
      title: 'Delete map',
      message: `Delete ${r.slug}.${DNS_DOMAIN}?`,
      confirmLabel: 'Delete',
    });
    if (!ok) return;
    try {
      await api.deleteMap(r.id);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Delete failed');
    }
  };

  if (loading) return <Spinner label="Loading maps..." />;

  return (
    <div className={pageLayout.page}>
      <PageHeader
        title="Maps"
        description={`Map {slug}.${DNS_DOMAIN} to a virtual IP and target (same port, TCP/UDP). Allowed groups only; default deny.`}
        actions={(
          <Button appearance="primary" icon={<AddRegular />} onClick={openCreate}>
            Add map
          </Button>
        )}
      />

      {error && !dialogOpen && <Text className={listPage.error}>{error}</Text>}

      {maps.length === 0 ? (
        <div className={listPage.empty}>
          <Text>No maps yet. Click Add map to create one.</Text>
        </div>
      ) : (
        <>
          <RuleListSearchBar
            value={searchQuery}
            placeholder="Name, slug, DNS, VIP, target, group…"
            onChange={setSearchQuery}
          />
          <Text className={listPage.resultHint}>
            Showing {filteredMaps.length} of {maps.length} map{maps.length === 1 ? '' : 's'}
          </Text>
          {filteredMaps.length === 0 ? (
            <div className={listPage.empty}>
              <Text>No maps match the current search.</Text>
            </div>
          ) : (
            <div className={listPage.list}>
              {filteredMaps.map((m) => (
                <MapCard
                  key={m.id}
                  map={m}
                  groups={groups}
                  onEdit={openEdit}
                  onDelete={(r) => void remove(r)}
                />
              ))}
            </div>
          )}
        </>
      )}

      <Dialog open={dialogOpen} onOpenChange={(_, d) => setDialogOpen(d.open)}>
        <DialogSurface>
          <DialogBody>
            <DialogTitle>{editing ? 'Edit map' : 'Add map'}</DialogTitle>
            <DialogContent className={styles.dialogGrid}>
              <Field label="Display name">
                <Input value={form.name} onChange={(_, d) => setForm({ ...form, name: d.value })} placeholder="LAN DB" />
              </Field>
              <Field label="DNS slug" required hint={`Resolves to ${form.slug.trim() ? `${form.slug.trim().toLowerCase()}.${DNS_DOMAIN}` : `{slug}.${DNS_DOMAIN}`}`}>
                <Input
                  value={form.slug}
                  onChange={(_, d) => setForm({ ...form, slug: d.value })}
                  placeholder="db"
                />
              </Field>
              <Field label="Target host" required>
                <Input
                  value={form.target_host}
                  onChange={(_, d) => setForm({ ...form, target_host: d.value })}
                  placeholder="192.168.1.10 or app.example.com"
                />
              </Field>
              <AllowedGroupsPicker
                groups={groups}
                value={form.allowed_group_ids}
                onChange={(allowed_group_ids) => setForm((f) => ({ ...f, allowed_group_ids }))}
              />
              {formError && <Text className={listPage.error}>{formError}</Text>}
            </DialogContent>
            <DialogActions>
              <Button appearance="secondary" onClick={() => setDialogOpen(false)}>Cancel</Button>
              <Button appearance="primary" disabled={saving} onClick={() => void save()}>
                {editing ? 'Save' : 'Create'}
              </Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
    </div>
  );
}
