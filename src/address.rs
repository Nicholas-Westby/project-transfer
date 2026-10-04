//! Only private-network peers are ever contacted or accepted.

use std::net::{IpAddr, SocketAddr};

/// The port an instance listens on unless it is taken; also what "Add by
/// address" assumes when no port is given.
pub const DEFAULT_PORT: u16 = 47820;

pub const NOT_LOCAL: &str = "That address isn't on your local network. Project Transfer only \
                             connects to computers on the same network.";

/// Reads what someone typed into "Add by address": `ip`, `ip:port`, a bare
/// IPv6 address or `[v6]:port`. Refuses addresses outside the local network.
pub fn parse_address(text: &str) -> Result<SocketAddr, String> {
    let text = text.trim();
    let addr = match text.parse::<SocketAddr>() {
        Ok(a) => Some(a),
        Err(_) => text
            .parse::<IpAddr>()
            .ok()
            .map(|ip| SocketAddr::new(ip, DEFAULT_PORT)),
    };
    let addr = addr.filter(|a| a.port() != 0).ok_or_else(|| {
        format!(
            "Enter an address like 192.168.1.20 or 192.168.1.20:{DEFAULT_PORT}. You can find \
             it in Settings on the other computer."
        )
    })?;
    if !is_local(addr.ip()) {
        return Err(NOT_LOCAL.to_string());
    }
    Ok(addr)
}

pub fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.is_loopback(),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_local(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

/// How other computers can reach this one: each local, non-loopback
/// address with `port`, IPv4 first. IPv6 link-local addresses are left out
/// because they need an interface suffix nobody types.
pub fn shareable(ips: impl IntoIterator<Item = IpAddr>, port: u16) -> Vec<String> {
    let mut ips: Vec<IpAddr> = ips
        .into_iter()
        .filter(|ip| is_local(*ip) && !ip.is_loopback())
        .filter(|ip| match ip {
            IpAddr::V6(v6) => (v6.segments()[0] & 0xffc0) != 0xfe80,
            IpAddr::V4(_) => true,
        })
        .collect();
    ips.sort_by_key(|ip| (ip.is_ipv6(), *ip));
    ips.dedup();
    ips.into_iter()
        .map(|ip| SocketAddr::new(ip, port).to_string())
        .collect()
}

/// This computer's addresses as `shareable` gives them; empty when the
/// interfaces can't be read.
pub fn this_computer(port: u16) -> Vec<String> {
    let ips = if_addrs::get_if_addrs()
        .map(|list| list.into_iter().map(|i| i.ip()).collect::<Vec<_>>())
        .unwrap_or_default();
    shareable(ips, port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shareable_addresses_skip_loopback_public_and_link_local_v6() {
        let ips = [
            "127.0.0.1",
            "8.8.8.8",
            "fe80::1",
            "fd00::5",
            "192.168.1.20",
            "10.0.0.5",
        ]
        .map(|s| s.parse::<IpAddr>().unwrap());
        assert_eq!(
            shareable(ips, 47820),
            ["10.0.0.5:47820", "192.168.1.20:47820", "[fd00::5]:47820"]
        );
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn private_ranges_are_local() {
        for s in [
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.254",
            "192.168.1.5",
            "169.254.7.7",
            "127.0.0.1",
            "::1",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "febf::1",
        ] {
            assert!(is_local(ip(s)), "{s}");
        }
    }

    #[test]
    fn parses_typed_addresses_with_and_without_a_port() {
        let ok = |s: &str| parse_address(s).unwrap();
        assert_eq!(ok("192.168.1.20"), "192.168.1.20:47820".parse().unwrap());
        assert_eq!(ok(" 10.0.0.5:5000 "), "10.0.0.5:5000".parse().unwrap());
        assert_eq!(ok("fe80::1"), "[fe80::1]:47820".parse().unwrap());
        assert_eq!(ok("[::1]:6000"), "[::1]:6000".parse().unwrap());
    }

    #[test]
    fn refuses_public_and_malformed_addresses() {
        assert_eq!(parse_address("8.8.8.8:53").unwrap_err(), NOT_LOCAL);
        assert_eq!(parse_address("2001:4860::8888").unwrap_err(), NOT_LOCAL);
        for bad in ["", "desk.local", "10.0.0.5:http", "10.0.0.5:0", "300.1.1.1"] {
            let e = parse_address(bad).unwrap_err();
            assert!(e.starts_with("Enter an address like"), "{bad}: {e}");
        }
    }

    #[test]
    fn public_addresses_are_not_local() {
        for s in [
            "8.8.8.8",
            "172.32.0.1",
            "172.15.0.1",
            "11.0.0.1",
            "2001:4860::8888",
            "fec0::1",
        ] {
            assert!(!is_local(ip(s)), "{s}");
        }
    }
}
