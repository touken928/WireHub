import { Button } from '@fluentui/react-components';
import { DeleteRegular, EditRegular } from '@fluentui/react-icons';
import type { PeerGroup, ServiceMap } from '@/api/types';
import { AllowedGroupsBadges } from '@/components/common/AllowedGroupsBadges';
import {
  MemberRowActions,
  MemberRowCard,
  MemberRowIdentity,
  MemberRowStat,
} from '@/components/common/MemberRowCard';

type MapCardProps = {
  map: ServiceMap;
  groups: PeerGroup[];
  onEdit: (map: ServiceMap) => void;
  onDelete: (map: ServiceMap) => void;
};

export function MapCard({ map, groups, onEdit, onDelete }: MapCardProps) {
  const title = map.name?.trim() || map.slug;
  const showSlug = Boolean(map.name?.trim()) && map.slug !== map.name;

  return (
    <MemberRowCard statColumns={4}>
      <MemberRowIdentity title={title} subtitle={showSlug ? map.slug : undefined} />
      <MemberRowStat label="DNS" value={map.fqdn} mono />
      <MemberRowStat label="Virtual IP" value={map.virtual_ip} mono />
      <MemberRowStat label="Target" value={map.target_display} mono />
      <MemberRowStat
        label="Allowed groups"
        value={<AllowedGroupsBadges groupIds={map.allowed_group_ids} groups={groups} />}
      />
      <MemberRowActions>
        <Button
          size="small"
          appearance="subtle"
          icon={<EditRegular />}
          aria-label="Edit map"
          onClick={() => onEdit(map)}
        />
        <Button
          size="small"
          appearance="subtle"
          icon={<DeleteRegular />}
          aria-label="Delete map"
          onClick={() => onDelete(map)}
        />
      </MemberRowActions>
    </MemberRowCard>
  );
}
