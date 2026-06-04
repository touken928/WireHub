import type { PeerGroup, PortForward, ServiceMap } from '@/api/types';
import { hubFQDN } from '@/constants';

function includesQuery(haystack: string, query: string): boolean {
  return haystack.toLowerCase().includes(query);
}

export function filterPortForwards(rules: PortForward[], query: string): PortForward[] {
  const q = query.trim().toLowerCase();
  if (!q) return rules;
  return rules.filter((rule) => {
    const text = [
      rule.name,
      rule.protocol,
      String(rule.listen_port),
      String(rule.target_port),
      rule.target_host,
      rule.target_display,
      `${hubFQDN()}:${rule.listen_port}`,
    ].join(' ').toLowerCase();
    return includesQuery(text, q);
  });
}

export function filterServiceMaps(
  maps: ServiceMap[],
  groups: PeerGroup[],
  query: string,
): ServiceMap[] {
  const q = query.trim().toLowerCase();
  if (!q) return maps;
  const groupNameById = new Map(groups.map((g) => [g.id, g.name]));
  return maps.filter((map) => {
    const allowedNames = map.allowed_group_ids
      .map((id) => groupNameById.get(id) ?? '')
      .join(' ');
    const text = [
      map.name,
      map.slug,
      map.fqdn,
      map.virtual_ip,
      map.target_host,
      map.target_display,
      allowedNames,
    ].join(' ').toLowerCase();
    return includesQuery(text, q);
  });
}
