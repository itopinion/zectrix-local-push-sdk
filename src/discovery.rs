use crate::{DeviceSummary, Error, Result};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
    time::Duration,
};
struct Browser(ServiceDaemon);
impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.0.stop_browse("_zectrix._udp.local.");
        let _ = self.0.shutdown();
    }
}
pub async fn targets(target: &Option<String>) -> Result<Vec<DeviceSummary>> {
    if let Some(t) = target {
        if let Ok(addr) = t.parse::<SocketAddr>() {
            if addr.is_ipv4() {
                return Ok(vec![direct(addr)]);
            }
        }
        if let Ok(IpAddr::V4(ip)) = t.parse::<IpAddr>() {
            return Ok(vec![direct(SocketAddr::new(ip.into(), 32342))]);
        }
        if t.len() != 12 || !t.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::InvalidOptions(
                "目标应为 IPv4、IPv4:端口或 12 位设备 ID".into(),
            ));
        }
    }
    let browser =
        Browser(ServiceDaemon::new().map_err(|e| Error::TransportFailure(e.to_string()))?);
    let rx = browser
        .0
        .browse("_zectrix._udp.local.")
        .map_err(|e| Error::TransportFailure(e.to_string()))?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut found = BTreeMap::new();
    while let Ok(Ok(event)) = tokio::time::timeout_at(deadline, rx.recv_async()).await {
        if let ServiceEvent::ServiceResolved(s) = event {
            if s.get_property_val_str("version") != Some("1") {
                continue;
            }
            let Some(id) = s.get_property_val_str("id") else {
                continue;
            };
            if target.as_ref().is_some_and(|t| !t.eq_ignore_ascii_case(id)) {
                continue;
            }
            for ip in s.get_addresses().iter().filter(|a| a.is_ipv4()) {
                let addr = SocketAddr::new(*ip, s.get_port());
                found.insert(
                    (id.to_string(), addr),
                    DeviceSummary {
                        id: id.into(),
                        name: s.get_fullname().into(),
                        address: addr.to_string(),
                        busy: false,
                    },
                );
            }
        }
    }
    if found.is_empty() {
        Err(Error::NoDevice)
    } else {
        Ok(found.into_values().collect())
    }
}
fn direct(addr: SocketAddr) -> DeviceSummary {
    DeviceSummary {
        id: String::new(),
        name: String::new(),
        address: addr.to_string(),
        busy: false,
    }
}
