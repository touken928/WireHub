import {
  Button,
  Card,
  Field,
  Input,
  Text,
  makeStyles,
  tokens,
} from '@fluentui/react-components';
import { ArrowDownloadRegular, DeleteRegular, PowerRegular } from '@fluentui/react-icons';
import { DNS_DOMAIN } from '@/constants';
import type { GroupGraphNode } from '@/api/types';
import { formatBytes, formatHandshake } from '@/lib/format';
import { PeerStatusBadge } from '@/components/peers/PeerStatusBadge';
import type { EnrichedPeer } from '@/pages/groups/utils';

const useStyles = makeStyles({
  panel: {
    flex: '0 0 420px',
    display: 'grid',
    gridTemplateRows: 'auto minmax(0, 1fr) auto',
    minHeight: 0,
    alignSelf: 'stretch',
    borderRadius: tokens.borderRadiusXLarge,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    backgroundColor: tokens.colorNeutralBackground1,
    boxShadow: tokens.shadow4,
    overflow: 'hidden',
  },
  panelEmpty: {
    display: 'flex',
    gridTemplateRows: 'unset',
    alignItems: 'center',
    justifyContent: 'center',
  },
  header: {
    flexShrink: 0,
    display: 'flex',
    alignItems: 'center',
    gap: '10px',
    padding: '14px 16px',
    borderBottom: `1px solid ${tokens.colorNeutralStroke2}`,
  },
  memberList: {
    minHeight: 0,
    overflowY: 'auto',
    overflowX: 'hidden',
    overscrollBehavior: 'contain',
    WebkitOverflowScrolling: 'touch',
    padding: '12px 16px',
    display: 'flex',
    flexDirection: 'column',
    gap: '12px',
  },
  memberCard: {
    flexShrink: 0,
    padding: '14px',
    display: 'flex',
    flexDirection: 'column',
    gap: '10px',
    borderRadius: tokens.borderRadiusLarge,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    backgroundColor: tokens.colorNeutralBackground2,
  },
  memberTop: {
    display: 'flex',
    alignItems: 'center',
    gap: '8px',
    flexWrap: 'wrap',
  },
  memberMeta: {
    display: 'grid',
    gridTemplateColumns: '1fr 1fr',
    gap: '8px 12px',
  },
  metaItem: {
    display: 'flex',
    flexDirection: 'column',
    gap: '2px',
  },
  metaLabel: {
    color: tokens.colorNeutralForeground3,
    fontSize: tokens.fontSizeBase200,
  },
  mono: {
    fontFamily: tokens.fontFamilyMonospace,
    fontSize: tokens.fontSizeBase200,
  },
  memberActions: {
    display: 'flex',
    gap: '6px',
    flexWrap: 'wrap',
  },
  addSection: {
    flexShrink: 0,
    padding: '12px 16px 16px',
    display: 'flex',
    flexDirection: 'column',
    gap: '8px',
    borderTop: `1px solid ${tokens.colorNeutralStroke2}`,
    backgroundColor: tokens.colorNeutralBackground1,
  },
  panelEmptyText: {
    padding: '24px 16px',
    textAlign: 'center',
  },
  addRow: {
    display: 'flex',
    gap: '8px',
  },
  empty: {
    padding: '24px 8px',
    textAlign: 'center',
    color: tokens.colorNeutralForeground3,
  },
});

type GroupDetailPanelProps = {
  group: GroupGraphNode | null;
  peers: EnrichedPeer[];
  mutedClassName: string;
  newUserName: string;
  createUserError: string;
  onNewUserNameChange: (value: string) => void;
  onCreateUser: () => void;
  onShowConfig: (peerId: number) => void;
  onTogglePeer: (peerId: number) => void;
  onDeletePeer: (peerId: number, peerName: string) => void;
};

export function GroupDetailPanel({
  group,
  peers,
  mutedClassName,
  newUserName,
  createUserError,
  onNewUserNameChange,
  onCreateUser,
  onShowConfig,
  onTogglePeer,
  onDeletePeer,
}: GroupDetailPanelProps) {
  const styles = useStyles();

  if (!group) {
    return (
      <aside className={`${styles.panel} ${styles.panelEmpty}`}>
        <div className={styles.panelEmptyText}>
          <Text size={300}>No groups yet. Add a group to get started.</Text>
        </div>
      </aside>
    );
  }

  return (
    <aside className={styles.panel}>
      <div className={styles.header}>
        <div>
          <Text weight="semibold" block>{group.name}</Text>
          <Text size={200} className={mutedClassName}>{peers.length} member(s)</Text>
        </div>
      </div>
      <div className={styles.memberList}>
        {peers.length === 0 ? (
          <div className={styles.empty}>
            <Text size={300}>No users in this group yet.</Text>
          </div>
        ) : (
          peers.map((peer) => (
            <Card key={peer.id} className={styles.memberCard}>
              <div className={styles.memberTop}>
                <Text weight="semibold">{peer.name}</Text>
                <PeerStatusBadge enabled={peer.enabled} online={peer.online} />
              </div>
              <div className={styles.memberMeta}>
                <div className={styles.metaItem}>
                  <span className={styles.metaLabel}>WireGuard IP</span>
                  <span className={styles.mono}>{peer.wg_ip}</span>
                </div>
                <div className={styles.metaItem}>
                  <span className={styles.metaLabel}>DNS</span>
                  <span className={styles.mono}>{peer.fqdn || `${peer.name}.${DNS_DOMAIN}`}</span>
                </div>
                <div className={styles.metaItem}>
                  <span className={styles.metaLabel}>Last handshake</span>
                  <Text size={200}>{formatHandshake(peer.last_handshake ?? 0)}</Text>
                </div>
                <div className={styles.metaItem}>
                  <span className={styles.metaLabel}>Traffic</span>
                  <Text size={200}>
                    {formatBytes(peer.rx_bytes ?? 0)} / {formatBytes(peer.tx_bytes ?? 0)}
                  </Text>
                </div>
              </div>
              <div className={styles.memberActions}>
                <Button size="small" icon={<ArrowDownloadRegular />} onClick={() => onShowConfig(peer.id)}>
                  Config
                </Button>
                <Button size="small" icon={<PowerRegular />} onClick={() => onTogglePeer(peer.id)}>
                  Toggle
                </Button>
                <Button
                  size="small"
                  icon={<DeleteRegular />}
                  appearance="subtle"
                  onClick={() => onDeletePeer(peer.id, peer.name)}
                />
              </div>
            </Card>
          ))
        )}
      </div>
      <div className={styles.addSection}>
        <Text weight="semibold" size={300}>Add user</Text>
        <div className={styles.addRow}>
          <Field style={{ flex: 1 }}>
            <Input
              value={newUserName}
              placeholder="alice"
              onChange={(_, data) => onNewUserNameChange(data.value)}
            />
          </Field>
          <Button appearance="primary" onClick={onCreateUser} disabled={!newUserName.trim()}>
            Add
          </Button>
        </div>
        {createUserError && (
          <Text size={200} style={{ color: tokens.colorPaletteRedForeground1 }}>{createUserError}</Text>
        )}
      </div>
    </aside>
  );
}
