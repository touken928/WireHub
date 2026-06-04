import { Badge, Button } from '@fluentui/react-components';
import { DeleteRegular, EditRegular } from '@fluentui/react-icons';
import type { PortForward } from '@/api/types';
import {
  MemberRowActions,
  MemberRowCard,
  MemberRowIdentity,
  MemberRowStat,
} from '@/components/common/MemberRowCard';
import { hubFQDN } from '@/constants';

type ForwardRuleCardProps = {
  rule: PortForward;
  onEdit: (rule: PortForward) => void;
  onDelete: (rule: PortForward) => void;
};

function formatListen(port: number) {
  return `${hubFQDN()}:${port}`;
}

export function ForwardRuleCard({ rule, onEdit, onDelete }: ForwardRuleCardProps) {
  const title = rule.name?.trim() || `Port ${rule.listen_port}`;
  const protocol = rule.protocol.toUpperCase();

  return (
    <MemberRowCard statColumns={3}>
      <MemberRowIdentity
        title={title}
        badge={(
          <Badge
            appearance="outline"
            size="small"
            color={rule.protocol === 'udp' ? 'warning' : 'informative'}
          >
            {protocol}
          </Badge>
        )}
      />
      <MemberRowStat label="Listen" value={formatListen(rule.listen_port)} mono />
      <MemberRowStat label="Target" value={rule.target_display} mono />
      <MemberRowStat label="Protocol" value={protocol} />
      <MemberRowActions>
        <Button
          size="small"
          appearance="subtle"
          icon={<EditRegular />}
          aria-label="Edit forward"
          onClick={() => onEdit(rule)}
        />
        <Button
          size="small"
          appearance="subtle"
          icon={<DeleteRegular />}
          aria-label="Delete forward"
          onClick={() => onDelete(rule)}
        />
      </MemberRowActions>
    </MemberRowCard>
  );
}
