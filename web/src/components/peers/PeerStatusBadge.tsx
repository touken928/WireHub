import { Badge } from '@fluentui/react-components';

type PeerStatusBadgeProps = {
  enabled: boolean;
  online?: boolean;
};

export function PeerStatusBadge({ enabled, online = false }: PeerStatusBadgeProps) {
  const label = !enabled ? 'Disabled' : online ? 'Online' : 'Offline';
  const color = !enabled ? 'danger' : online ? 'success' : 'informative';

  return (
    <Badge
      size="small"
      appearance={enabled && online ? 'filled' : 'outline'}
      color={color}
    >
      {label}
    </Badge>
  );
}
