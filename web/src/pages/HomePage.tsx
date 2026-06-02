import {
  Subtitle2,
  Button,
  Table,
  TableHeader,
  TableRow,
  TableHeaderCell,
  TableBody,
  TableCell,
  Input,
  Dialog,
  DialogTrigger,
  DialogSurface,
  DialogBody,
  DialogTitle,
  DialogContent,
  DialogActions,
  Field,
  Textarea,
  Badge,
  Spinner,
  Text,
  Card,
  Switch,
  Tooltip,
  makeStyles,
  tokens,
} from '@fluentui/react-components';
import {
  AddRegular,
  DeleteRegular,
  EditRegular,
  ArrowDownloadRegular,
  PowerRegular,
  ArrowResetRegular,
} from '@fluentui/react-icons';
import { useCallback, useEffect, useState } from 'react';
import { api, clearToken, formatBytes, formatHandshake, DNS_DOMAIN } from '../api/client';
import type { PeerStatus, Settings } from '../api/client';
import ConfigDialog from '../components/ConfigDialog';
import NetworkUsageChart from '../components/NetworkUsageChart';

const useStyles = makeStyles({
  page: {
    width: 'min(1120px, calc(100% - 48px))',
    margin: '0 auto',
    padding: '32px 0 56px',
  },
  topBar: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    marginBottom: '20px',
    gap: '16px',
    flexWrap: 'wrap',
  },
  brandBlock: {
    display: 'flex',
    flexDirection: 'column',
    gap: '4px',
  },
  brand: {
    fontSize: tokens.fontSizeHero700,
    fontWeight: tokens.fontWeightSemibold,
    color: tokens.colorBrandForeground1,
    letterSpacing: '-0.02em',
  },
  tagline: {
    color: tokens.colorNeutralForeground3,
  },
  topActions: {
    display: 'flex',
    alignItems: 'center',
    gap: '12px',
  },
  surface: {
    display: 'flex',
    flexDirection: 'column',
    gap: '20px',
  },
  hubCard: {
    padding: '20px',
    display: 'flex',
    flexDirection: 'column',
    gap: '16px',
    borderRadius: tokens.borderRadiusXLarge,
    boxShadow: tokens.shadow4,
  },
  hubHeader: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    gap: '12px',
    flexWrap: 'wrap',
  },
  hubGrid: {
    display: 'grid',
    gridTemplateColumns: 'repeat(auto-fit, minmax(210px, 1fr))',
    gap: '12px',
  },
  infoTile: {
    padding: '12px 14px',
    borderRadius: tokens.borderRadiusLarge,
    backgroundColor: tokens.colorNeutralBackground2,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    display: 'flex',
    flexDirection: 'column',
    gap: '4px',
  },
  label: {
    color: tokens.colorNeutralForeground3,
  },
  usersCard: {
    padding: '20px',
    display: 'flex',
    flexDirection: 'column',
    gap: '16px',
    borderRadius: tokens.borderRadiusXLarge,
    boxShadow: tokens.shadow4,
  },
  tableWrap: {
    overflowX: 'auto',
  },
  hostnameCell: {
    display: 'flex',
    flexDirection: 'column',
    gap: '2px',
  },
  monoText: {
    fontFamily: tokens.fontFamilyMonospace,
  },
  statusCell: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    gap: '6px',
  },
  actionsHeaderCell: {
    textAlign: 'center',
    '& .fui-TableHeaderCell__button': {
      justifyContent: 'center',
    },
  },
  actionsBodyCell: {
    textAlign: 'center',
    verticalAlign: 'middle',
    paddingTop: tokens.spacingVerticalXS,
    paddingBottom: tokens.spacingVerticalXS,
  },
  actionsCell: {
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    gap: tokens.spacingHorizontalXXS,
    margin: '0 auto',
  },
  emptyState: {
    padding: '36px',
    textAlign: 'center',
    color: tokens.colorNeutralForeground3,
  },
  dialogContent: {
    display: 'flex',
    flexDirection: 'column',
    gap: '12px',
  },
  readonlyHostname: {
    opacity: 0.7,
  },
  errorText: {
    color: tokens.colorPaletteRedForeground1,
  },
});

const EXCLUDE_HINT =
  'One hostname pattern per line. Default is unrestricted. Use alice, server-*, or !bob to re-allow. Hostnames only — no domain suffix.';

function excludeToText(rules: string[] | undefined): string {
  return (rules ?? []).join('\n');
}

function textToExclude(text: string): string[] {
  return text
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
}

function hasAccessRules(rules: string[] | undefined): boolean {
  return (rules ?? []).length > 0;
}

function slugHostname(name: string): string {
  const slug = name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
  return slug || 'host';
}

function validateHostnameInput(name: string): string | null {
  const trimmed = name.trim();
  if (!trimmed) return 'Hostname is required';
  if (trimmed.length > 63) return 'Hostname too long (max 63 characters)';
  const slug = slugHostname(trimmed);
  if (slug === 'host' && trimmed.toLowerCase() !== 'host') {
    return 'Use letters, numbers, and hyphens only';
  }
  if (slug.startsWith('-') || slug.endsWith('-')) {
    return 'Hostname cannot start or end with a hyphen';
  }
  if (slug.includes('--')) {
    return 'Hostname cannot contain consecutive hyphens';
  }
  if (slug === 'hub' || slug === 'dns' || slug === 'www') {
    return `Hostname "${slug}" is reserved`;
  }
  return null;
}

interface HomePageProps {
  dark: boolean;
  onToggleTheme: () => void;
}

export default function HomePage({ dark, onToggleTheme }: HomePageProps) {
  const styles = useStyles();
  const [peers, setPeers] = useState<PeerStatus[]>([]);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [loading, setLoading] = useState(true);
  const [configOpen, setConfigOpen] = useState(false);
  const [configText, setConfigText] = useState('');
  const [configFile, setConfigFile] = useState('peer.conf');
  const [createOpen, setCreateOpen] = useState(false);
  const [resetOpen, setResetOpen] = useState(false);
  const [resetting, setResetting] = useState(false);
  const [name, setName] = useState('');
  const [nameError, setNameError] = useState<string | null>(null);
  const [createExclude, setCreateExclude] = useState('');
  const [createError, setCreateError] = useState('');
  const [editOpen, setEditOpen] = useState(false);
  const [editPeer, setEditPeer] = useState<PeerStatus | null>(null);
  const [editExclude, setEditExclude] = useState('');
  const [editError, setEditError] = useState('');

  const load = useCallback(() => {
    api.getStatus().then((d) => {
      setPeers(d.peers ?? []);
      setSettings(d.settings);
      setLoading(false);
    }).catch(() => setLoading(false));
  }, []);

  useEffect(() => {
    load();
    const t = setInterval(load, 5000);
    return () => clearInterval(t);
  }, [load]);

  const hostnamePreview = name.trim()
    ? `${slugHostname(name)}.${DNS_DOMAIN}`
    : '';

  const showConfig = async (id: number) => {
    const { config, filename } = await api.getPeerConfig(id);
    setConfigText(config);
    setConfigFile(filename);
    setConfigOpen(true);
  };

  const openEdit = (peer: PeerStatus) => {
    setEditPeer(peer);
    setEditExclude(excludeToText(peer.access_exclude));
    setEditError('');
    setEditOpen(true);
  };

  const handleCreate = async () => {
    const err = validateHostnameInput(name);
    if (err) {
      setNameError(err);
      return;
    }
    setCreateError('');
    try {
      await api.createPeer({
        name: name.trim(),
        access_exclude: textToExclude(createExclude),
      });
      setCreateOpen(false);
      setName('');
      setCreateExclude('');
      setNameError(null);
      load();
    } catch (e) {
      setCreateError(e instanceof Error ? e.message : 'Create failed');
    }
  };

  const handleEdit = async () => {
    if (!editPeer) return;
    setEditError('');
    try {
      await api.updatePeer(editPeer.id, {
        access_exclude: textToExclude(editExclude),
      });
      setEditOpen(false);
      setEditPeer(null);
      load();
    } catch (e) {
      setEditError(e instanceof Error ? e.message : 'Update failed');
    }
  };

  const handleReset = async () => {
    setResetting(true);
    try {
      await api.reset();
      clearToken();
      window.location.href = '/setup';
    } catch {
      setResetting(false);
    }
  };

  if (loading) return <Spinner label="Loading..." />;

  return (
    <div className={styles.page}>
      <div className={styles.topBar}>
        <div className={styles.brandBlock}>
          <Text className={styles.brand}>WireHub</Text>
          <Text size={200} className={styles.tagline}>
            Private WireGuard hub, DNS, and userspace networking
          </Text>
        </div>
        <div className={styles.topActions}>
          <Switch
            label={dark ? 'Dark' : 'Light'}
            checked={dark}
            onChange={onToggleTheme}
          />
          <Tooltip content="Reset hub configuration" relationship="label">
            <Button
              appearance="subtle"
              icon={<ArrowResetRegular />}
              onClick={() => setResetOpen(true)}
            >
              Reset
            </Button>
          </Tooltip>
          <Tooltip content="Sign out" relationship="label">
            <Button
              appearance="subtle"
              onClick={() => {
                clearToken();
                window.location.href = '/login';
              }}
            >
              Logout
            </Button>
          </Tooltip>
        </div>
      </div>

      <div className={styles.surface}>
        {settings && (
          <Card className={styles.hubCard}>
            <div className={styles.hubHeader}>
              <Subtitle2>Hub</Subtitle2>
              <Badge appearance="tint" color="success">Ready</Badge>
            </div>
            <div className={styles.hubGrid}>
              <div className={styles.infoTile}>
                <Text size={200} className={styles.label}>Subnet</Text>
                <Text className={styles.monoText}>{settings.wg_subnet}</Text>
              </div>
              <div className={styles.infoTile}>
                <Text size={200} className={styles.label}>Endpoint</Text>
                <Text className={styles.monoText}>{settings.endpoint}:{settings.listen_port}</Text>
              </div>
              <div className={styles.infoTile}>
                <Text size={200} className={styles.label}>Client DNS</Text>
                <Text className={styles.monoText}>
                  {[settings.dns_ip, ...(settings.upstream_dns ?? [])].join(', ')}
                </Text>
              </div>
              <div className={styles.infoTile}>
                <Text size={200} className={styles.label}>Web UI</Text>
                <Text className={styles.monoText}>http://{settings.dns_suffix || DNS_DOMAIN}:{settings.listen_port}</Text>
              </div>
            </div>
          </Card>
        )}

        <NetworkUsageChart />

        <Card className={styles.usersCard}>
          <div className={styles.hubHeader}>
            <Subtitle2>Users</Subtitle2>
            <Dialog
              open={createOpen}
              onOpenChange={(_, d) => {
                setCreateOpen(d.open);
                if (!d.open) {
                  setName('');
                  setCreateExclude('');
                  setNameError(null);
                  setCreateError('');
                }
              }}
            >
              <DialogTrigger disableButtonEnhancement>
                <Button appearance="primary" icon={<AddRegular />} onClick={() => setCreateOpen(true)}>
                  Add User
                </Button>
              </DialogTrigger>
              <DialogSurface>
                <DialogBody>
                  <DialogTitle>Add User</DialogTitle>
                  <DialogContent className={styles.dialogContent}>
                    <Field label="Hostname" required hint="Cannot be changed after creation" validationMessage={nameError ?? undefined}>
                      <Input
                        value={name}
                        placeholder="alice"
                        onChange={(_, d) => {
                          setName(d.value);
                          setNameError(validateHostnameInput(d.value));
                        }}
                      />
                    </Field>
                    {hostnamePreview && !nameError && (
                      <Text size={200} className={styles.monoText}>DNS: {hostnamePreview}</Text>
                    )}
                    <Field label="Hostname access" hint={EXCLUDE_HINT}>
                      <Textarea
                        value={createExclude}
                        placeholder={'# block all server-* except server-01\nserver-*\n!server-01'}
                        rows={5}
                        onChange={(_, d) => setCreateExclude(d.value)}
                      />
                    </Field>
                    {createError && <Text className={styles.errorText}>{createError}</Text>}
                  </DialogContent>
                  <DialogActions>
                    <Button onClick={() => setCreateOpen(false)}>Cancel</Button>
                    <Button
                      appearance="primary"
                      onClick={handleCreate}
                      disabled={!name.trim() || !!nameError}
                    >
                      Create
                    </Button>
                  </DialogActions>
                </DialogBody>
              </DialogSurface>
            </Dialog>
          </div>
          <Text size={200} className={styles.label}>
            Hostnames are immutable. Configure per-user hostname reachability with exclude rules.
          </Text>

          <div className={styles.tableWrap}>
            <Table>
              <colgroup>
                <col />
                <col style={{ width: '140px' }} />
                <col style={{ width: '128px' }} />
                <col style={{ width: '168px' }} />
                <col style={{ width: '168px' }} />
              </colgroup>
              <TableHeader>
                <TableRow>
                  <TableHeaderCell>Hostname</TableHeaderCell>
                  <TableHeaderCell>WireGuard IP</TableHeaderCell>
                  <TableHeaderCell>Status</TableHeaderCell>
                  <TableHeaderCell>Traffic</TableHeaderCell>
                  <TableHeaderCell className={styles.actionsHeaderCell}>Actions</TableHeaderCell>
                </TableRow>
              </TableHeader>
              <TableBody>
                {peers.map((p) => (
                  <TableRow key={p.id}>
                    <TableCell>
                      <div className={styles.hostnameCell}>
                        <Text weight="semibold">{p.name}</Text>
                        <Text size={200} className={styles.label}>{formatHandshake(p.last_handshake)}</Text>
                      </div>
                    </TableCell>
                    <TableCell>
                      <Text className={styles.monoText}>{p.wg_ip}</Text>
                    </TableCell>
                    <TableCell>
                      <div className={styles.statusCell}>
                        <Badge
                          size="small"
                          appearance={p.enabled && p.online ? 'filled' : 'outline'}
                          color={!p.enabled ? 'danger' : p.online ? 'success' : 'informative'}
                        >
                          {!p.enabled ? 'Disabled' : p.online ? 'Online' : 'Offline'}
                        </Badge>
                        {hasAccessRules(p.access_exclude) && (
                          <Badge size="small" appearance="outline" color="warning">
                            Filtered
                          </Badge>
                        )}
                      </div>
                    </TableCell>
                    <TableCell>{formatBytes(p.rx_bytes)} / {formatBytes(p.tx_bytes)}</TableCell>
                    <TableCell className={styles.actionsBodyCell}>
                      <div className={styles.actionsCell}>
                        <Button size="small" appearance="subtle" icon={<EditRegular />} onClick={() => openEdit(p)} title="Access rules" />
                        <Button size="small" appearance="subtle" icon={<ArrowDownloadRegular />} onClick={() => showConfig(p.id)} title="Config" />
                        <Button size="small" appearance="subtle" icon={<PowerRegular />} onClick={() => api.togglePeer(p.id).then(load)} title="Toggle" />
                        <Button size="small" appearance="subtle" icon={<DeleteRegular />} onClick={() => api.deletePeer(p.id).then(load)} title="Delete" />
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
            {peers.length === 0 && (
              <div className={styles.emptyState}>
                <Text>No users yet. Add the first hostname to generate a WireGuard client config.</Text>
              </div>
            )}
          </div>
        </Card>
      </div>

      <ConfigDialog open={configOpen} config={configText} filename={configFile} onClose={() => setConfigOpen(false)} />

      <Dialog open={editOpen} onOpenChange={(_, d) => {
        setEditOpen(d.open);
        if (!d.open) {
          setEditPeer(null);
          setEditError('');
        }
      }}>
        <DialogSurface>
          <DialogBody>
            <DialogTitle>Hostname access</DialogTitle>
            <DialogContent className={styles.dialogContent}>
              <Field label="User">
                <Input value={editPeer?.name ?? ''} readOnly className={styles.readonlyHostname} />
              </Field>
              <Field label="Exclude rules" hint={EXCLUDE_HINT}>
                <Textarea
                  value={editExclude}
                  rows={6}
                  onChange={(_, d) => setEditExclude(d.value)}
                />
              </Field>
              {editError && <Text className={styles.errorText}>{editError}</Text>}
            </DialogContent>
            <DialogActions>
              <Button onClick={() => setEditOpen(false)}>Cancel</Button>
              <Button appearance="primary" onClick={handleEdit}>Save</Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>

      <Dialog open={resetOpen} onOpenChange={(_, d) => setResetOpen(d.open)}>
        <DialogSurface>
          <DialogBody>
            <DialogTitle>Reset WireHub?</DialogTitle>
            <DialogContent>
              <Text>
                This permanently deletes all hub settings, users, DNS records, and admin credentials.
                WireGuard and DNS will stop until you complete setup again. Process flags (port, bind, data directory) are unchanged.
              </Text>
            </DialogContent>
            <DialogActions>
              <Button onClick={() => setResetOpen(false)} disabled={resetting}>Cancel</Button>
              <Button appearance="primary" onClick={handleReset} disabled={resetting}>
                {resetting ? 'Resetting...' : 'Reset and return to setup'}
              </Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
    </div>
  );
}
