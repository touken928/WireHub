use std::{net::Ipv4Addr, str::FromStr};
pub use crate::model::NetworkSettings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Subnet24 {
    network: Ipv4Addr,
}

impl Subnet24 {
    pub fn parse(value: &str) -> Result<Self, String> {
        let (address, prefix) = value.split_once('/').ok_or("subnet must be an IPv4 /24")?;
        if prefix != "24" { return Err("subnet must be an IPv4 /24".into()); }
        let address = Ipv4Addr::from_str(address).map_err(|_| "subnet must be an IPv4 /24")?;
        let octets = address.octets();
        if !(octets[0] == 10 || (octets[0] == 172 && (16..=31).contains(&octets[1])) || (octets[0] == 192 && octets[1] == 168)) {
            return Err("subnet must be within RFC1918 private address space".into());
        }
        if octets[3] != 0 { return Err("subnet must use the canonical network address (last octet .0)".into()); }
        Ok(Self { network: address })
    }

    pub fn canonical(self) -> String { format!("{}/24", self.network) }
    pub fn hub_ip(self) -> String { self.address(1) }
    pub fn peer_ip(self, offset: u8) -> Option<String> { (2..=254).contains(&offset).then(|| self.address(offset)) }
    fn address(self, offset: u8) -> String { let mut octets=self.network.octets(); octets[3]=offset; Ipv4Addr::from(octets).to_string() }
}

pub fn validate_endpoint(value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim()!=value || value.chars().any(|c| c.is_control() || c.is_whitespace() || matches!(c, '/'|'\\'|'@'|'#'|'?')) {
        return Err("endpoint must be a DNS name or IPv4 address followed by :port".into());
    }
    let (host, port) = value.rsplit_once(':').ok_or("endpoint must include a port")?;
    let port: u16 = port.parse().map_err(|_| "endpoint port must be an integer from 1 through 65535")?;
    if port == 0 { return Err("endpoint port must be nonzero".into()); }
    if host.parse::<Ipv4Addr>().is_ok() { return Ok(()); }
    if host.contains(':') || host.len()>253 || host.is_empty() || !host.split('.').all(|label| !label.is_empty() && label.len()<=63 && label.bytes().all(|b| b.is_ascii_alphanumeric() || b==b'-') && !label.starts_with('-') && !label.ends_with('-')) {
        return Err("endpoint host must be a DNS name or IPv4 address (IPv6 is unsupported)".into());
    }
    Ok(())
}

pub fn validate_settings(subnet: &str, endpoint: &str, keepalive: u32) -> Result<NetworkSettings, String> {
    let subnet=Subnet24::parse(subnet)?.canonical();
    validate_endpoint(endpoint)?;
    if keepalive > u16::MAX as u32 { return Err("persistent keepalive must be between 0 and 65535".into()); }
    Ok(NetworkSettings{subnet,endpoint:endpoint.into(),persistent_keepalive:keepalive as u16})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn rfc1918_canonical_subnets_and_boundaries() {
        for s in ["10.1.2.0/24","172.16.0.0/24","172.31.255.0/24","192.168.1.0/24"] { assert!(Subnet24::parse(s).is_ok(),"{s}"); }
        for s in ["9.0.0.0/24","172.15.0.0/24","172.32.0.0/24","192.167.1.0/24","192.169.1.0/24","10.1.2.1/24","10.1.2.0/23","2001:db8::/24"] { assert!(Subnet24::parse(s).is_err(),"{s}"); }
        let subnet=Subnet24::parse("192.168.5.0/24").unwrap(); assert_eq!(subnet.hub_ip(),"192.168.5.1"); assert_eq!(subnet.peer_ip(2).as_deref(),Some("192.168.5.2")); assert_eq!(subnet.peer_ip(254).as_deref(),Some("192.168.5.254")); assert_eq!(subnet.peer_ip(1),None); assert_eq!(subnet.peer_ip(255),None);
    }
    #[test] fn endpoint_is_host_port_without_injection_or_ipv6() {
        for e in ["vpn.example:51820","192.0.2.1:65535"] { assert!(validate_endpoint(e).is_ok()); }
        for e in ["vpn.example","vpn.example:0","vpn.example:65536","[::1]:51820","2001:db8::1:51820","host:2\\nAllowedIPs=0.0.0.0/0","host/path:22"] { assert!(validate_endpoint(e).is_err(),"{e}"); }
    }
}
