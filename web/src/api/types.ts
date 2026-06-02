export interface SetupDefaults {
  subnet: string;
  admin_username: string;
  listen_port: number;
  mtu: number;
  status_interval: number;
  upstream_dns: string[];
}

export interface SetupStatus {
  configured: boolean;
  defaults: SetupDefaults;
}

export interface Settings {
  server_public_key: string;
  endpoint: string;
  listen_port: number;
  wg_subnet: string;
  hub_ip: string;
  dns_ip: string;
  dns_suffix: string;
  upstream_dns?: string[];
}

export interface HubSettings {
  endpoint: string;
  subnet: string;
  admin_username: string;
  hub_ip: string;
  dns_ip: string;
  dns_suffix: string;
  listen_port: number;
  server_public_key: string;
  mtu: number;
  status_interval: number;
  upstream_dns: string[];
}

export interface PeerGroup {
  id: number;
  name: string;
  pos_x: number;
  pos_y: number;
  member_count: number;
}

export interface GroupLink {
  id: number;
  from_group_id: number;
  to_group_id: number;
}

export interface GroupGraphPeer {
  id: number;
  name: string;
  wg_ip: string;
  group_id: number;
  enabled: boolean;
  fqdn: string;
}

export interface GroupGraphNode {
  id: number;
  name: string;
  pos_x: number;
  pos_y: number;
  member_count: number;
  peers: GroupGraphPeer[];
}

export interface GroupGraph {
  groups: GroupGraphNode[];
  links: GroupLink[];
}

export interface Peer {
  id: number;
  name: string;
  public_key: string;
  wg_ip: string;
  group_id: number;
  group_name?: string;
  enabled: boolean;
  fqdn: string;
  last_handshake: number;
  rx_bytes: number;
  tx_bytes: number;
  created_at: string;
}

export interface PortForward {
  id: number;
  name: string;
  listen_port: number;
  protocol: 'tcp' | 'udp';
  target_host: string;
  target_port: number;
  enabled: boolean;
  target_display: string;
}

export interface PortForwardDMZ {
  id: number;
  target_host: string;
  enabled: boolean;
  target_display: string;
}

export interface PortForwardList {
  rules: PortForward[];
  dmz: PortForwardDMZ;
  hub_ip: string;
  hub_port: number;
}

export interface PeerStatus {
  id: number;
  name: string;
  fqdn: string;
  wg_ip: string;
  group_id: number;
  group_name: string;
  enabled: boolean;
  last_handshake: number;
  rx_bytes: number;
  tx_bytes: number;
  online: boolean;
}
