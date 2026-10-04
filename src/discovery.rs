//! Finds other instances on the local network with mDNS. mDNS is link-local
//! multicast, so nothing here can reach beyond the LAN.
use crate::address::is_local;
use crate::model::InstanceId;
use anyhow::Context;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::net::{IpAddr, SocketAddr};
use std::sync::mpsc::Sender;

pub const SERVICE_TYPE: &str = "_projtransfer._tcp.local.";

#[derive(Clone, Debug, PartialEq)]
pub struct Discovered {
    pub id: InstanceId,
    pub name: String,
    pub addrs: Vec<SocketAddr>,
    pub version: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DiscoveryEvent {
    Found(Discovered),
    Lost(InstanceId),
}

pub struct Discovery {
    daemon: ServiceDaemon,
    me: InstanceId,
    port: u16,
}

/// Pure so it can be tested without a network. Returns None for records that
/// are missing a field, are malformed, or are our own announcement.
pub fn parse_txt(
    me: InstanceId,
    get: impl Fn(&str) -> Option<String>,
    ips: impl IntoIterator<Item = IpAddr>,
    port: u16,
) -> Option<Discovered> {
    let id: InstanceId = get("id")?.parse().ok()?;
    if id == me {
        return None;
    }
    let name = get("name")?;
    let version: u32 = get("v")?.parse().ok()?;
    // Anything on the multicast segment can claim to be a peer; only keep
    // addresses we would be willing to connect to.
    let addrs: Vec<SocketAddr> = ips
        .into_iter()
        .filter(|ip| is_local(*ip))
        .map(|ip| SocketAddr::new(ip, port))
        .collect();
    if addrs.is_empty() {
        return None;
    }
    Some(Discovered {
        id,
        name,
        addrs,
        version,
    })
}

impl Discovery {
    pub fn start(
        me: InstanceId,
        name: &str,
        port: u16,
        events: Sender<DiscoveryEvent>,
    ) -> anyhow::Result<Discovery> {
        let daemon = ServiceDaemon::new().context("could not start mDNS")?;
        let this = Discovery { daemon, me, port };
        this.register(name)?;
        let rx = this
            .daemon
            .browse(SERVICE_TYPE)
            .context("could not browse")?;
        // The daemon has its own thread; this one only translates its events.
        // It ends when shutdown closes the channel or the receiver is dropped.
        std::thread::spawn(move || {
            while let Ok(ev) = rx.recv() {
                let out = match ev {
                    ServiceEvent::ServiceResolved(r) => parse_txt(
                        me,
                        |k| r.get_property_val_str(k).map(str::to_string),
                        r.get_addresses().iter().map(|a| a.to_ip_addr()),
                        r.port,
                    )
                    .map(DiscoveryEvent::Found),
                    ServiceEvent::ServiceRemoved(_, full) => full
                        .split('.')
                        .next()
                        .and_then(|s| s.parse::<InstanceId>().ok())
                        .filter(|id| *id != me)
                        .map(DiscoveryEvent::Lost),
                    _ => None,
                };
                if let Some(out) = out
                    && events.send(out).is_err()
                {
                    break;
                }
            }
        });
        Ok(this)
    }

    fn register(&self, name: &str) -> anyhow::Result<()> {
        let id = self.me.to_string();
        let props = [
            ("id", id.as_str()),
            ("name", name),
            ("v", &crate::protocol::PROTOCOL_VERSION.to_string()),
        ];
        let info = ServiceInfo::new(
            SERVICE_TYPE,
            &id,
            &format!("{id}.local."),
            "",
            self.port,
            &props[..],
        )
        .context("could not describe this instance for mDNS")?
        .enable_addr_auto();
        self.daemon
            .register(info)
            .context("could not announce on mDNS")
    }

    /// Registering the same instance name again replaces the announcement.
    pub fn rename(&self, name: &str) -> anyhow::Result<()> {
        self.register(name)
    }

    pub fn stop(self) {
        let _ = self.daemon.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    fn txt(id: &str, name: &str, v: &str) -> HashMap<String, String> {
        HashMap::from([
            ("id".into(), id.into()),
            ("name".into(), name.into()),
            ("v".into(), v.into()),
        ])
    }

    fn parse(me: InstanceId, m: &HashMap<String, String>, ips: &[&str]) -> Option<Discovered> {
        parse_txt(
            me,
            |k| m.get(k).cloned(),
            ips.iter().map(|s| s.parse::<IpAddr>().unwrap()),
            4000,
        )
    }

    #[test]
    fn parses_a_valid_record() {
        let other = InstanceId::new_v4();
        let d = parse(
            InstanceId::new_v4(),
            &txt(&other.to_string(), "Desk", "1"),
            &["192.168.1.5"],
        )
        .unwrap();
        assert_eq!(d.id, other);
        assert_eq!(d.name, "Desk");
        assert_eq!(d.version, 1);
        assert_eq!(d.addrs, vec!["192.168.1.5:4000".parse().unwrap()]);
    }

    #[test]
    fn ignores_own_record() {
        let me = InstanceId::new_v4();
        assert!(parse(me, &txt(&me.to_string(), "Me", "1"), &["10.0.0.1"]).is_none());
    }

    #[test]
    fn drops_public_addresses_and_empty_results() {
        let other = InstanceId::new_v4().to_string();
        let m = txt(&other, "Far", "1");
        let d = parse(InstanceId::new_v4(), &m, &["8.8.8.8", "10.0.0.2"]).unwrap();
        assert_eq!(d.addrs, vec!["10.0.0.2:4000".parse().unwrap()]);
        assert!(parse(InstanceId::new_v4(), &m, &["8.8.8.8"]).is_none());
    }

    #[test]
    fn rejects_missing_or_malformed_fields() {
        let me = InstanceId::new_v4();
        let ok = InstanceId::new_v4().to_string();
        assert!(parse(me, &txt("not-a-uuid", "X", "1"), &["10.0.0.2"]).is_none());
        assert!(parse(me, &txt(&ok, "X", "one"), &["10.0.0.2"]).is_none());
        let mut no_name = txt(&ok, "X", "1");
        no_name.remove("name");
        assert!(parse(me, &no_name, &["10.0.0.2"]).is_none());
    }

    #[test]
    #[ignore = "needs multicast"]
    fn two_instances_find_each_other() {
        let (a, b) = (InstanceId::new_v4(), InstanceId::new_v4());
        let (txa, rxa) = mpsc::channel();
        let (txb, rxb) = mpsc::channel();
        let da = Discovery::start(a, "Alpha", 4001, txa).unwrap();
        let db = Discovery::start(b, "Beta", 4002, txb).unwrap();
        let wait = |rx: &mpsc::Receiver<DiscoveryEvent>, want: InstanceId, port: u16| {
            let end = Instant::now() + Duration::from_secs(10);
            while let Some(left) = end.checked_duration_since(Instant::now()) {
                if let Ok(DiscoveryEvent::Found(d)) = rx.recv_timeout(left) {
                    assert_ne!(d.id, if want == a { b } else { a }, "saw the wrong peer");
                    if d.id == want {
                        assert!(d.addrs.iter().all(|s| s.port() == port));
                        assert!(d.addrs.iter().all(|s| is_local(s.ip())));
                        return d;
                    }
                }
            }
            panic!("did not find {want} within 10 s");
        };
        let seen_by_a = wait(&rxa, b, 4002);
        let seen_by_b = wait(&rxb, a, 4001);
        assert_eq!(seen_by_a.name, "Beta");
        assert_eq!(seen_by_b.name, "Alpha");
        da.stop();
        db.stop();
    }

    #[test]
    #[ignore = "needs multicast"]
    fn peers_see_a_rename() {
        let (a, b) = (InstanceId::new_v4(), InstanceId::new_v4());
        let (txa, _rxa) = mpsc::channel();
        let (txb, rxb) = mpsc::channel();
        let da = Discovery::start(a, "Before", 4011, txa).unwrap();
        let db = Discovery::start(b, "Watcher", 4012, txb).unwrap();
        let wait_for = |name: &str| {
            let end = Instant::now() + Duration::from_secs(15);
            while let Some(left) = end.checked_duration_since(Instant::now()) {
                if let Ok(DiscoveryEvent::Found(d)) = rxb.recv_timeout(left)
                    && d.id == a
                    && d.name == name
                {
                    return;
                }
            }
            panic!("never saw {name}");
        };
        wait_for("Before");
        da.rename("After").unwrap();
        wait_for("After");
        da.stop();
        db.stop();
    }
}
