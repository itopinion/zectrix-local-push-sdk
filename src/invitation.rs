//! Device-initiated requests use a fixed listener; media keeps its own socket.
use crate::{device::Device, protocol::*, Result};
use std::{collections::VecDeque, net::SocketAddr};
use tokio::{
    net::UdpSocket,
    time::{Duration, Instant},
};

pub const CONNECTION_REQUEST_PORT: u16 = 32343;

/// Listen once per host. No discovery, pairing or automatic reconnect is performed.
/// Keep receiving while busy and discard requests your application cannot serve.
pub struct ConnectionRequestListener {
    socket: UdpSocket,
    seen: VecDeque<(SocketAddr, u64, Instant)>,
}

/// One explicit device request. Consuming it never substitutes a newer connection ID.
#[derive(Debug)]
pub struct ConnectionRequest {
    pub address: SocketAddr,
    pub connection_id: u64,
    received: Instant,
}
impl ConnectionRequestListener {
    /// Bind IPv4 UDP 32343. Port conflicts are returned to the caller.
    pub async fn bind() -> Result<Self> {
        Ok(Self {
            socket: UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, CONNECTION_REQUEST_PORT))
                .await?,
            seen: VecDeque::new(),
        })
    }
    /// Wait for a valid, previously unseen request. Safe to cancel while waiting.
    pub async fn next_request(&mut self) -> Result<ConnectionRequest> {
        let mut buffer = [0u8; 1201];
        loop {
            let (n, address) = self.socket.recv_from(&mut buffer).await?;
            let Some(packet) = Packet::decode(&buffer[..n]) else {
                continue;
            };
            if packet.kind != CONNECT_REQUEST
                || packet.connection == 0
                || packet.sequence != 1
                || !packet.body.is_empty()
            {
                continue;
            }
            let now = Instant::now();
            while self
                .seen
                .front()
                .is_some_and(|x| now.duration_since(x.2) >= Duration::from_secs(60))
            {
                self.seen.pop_front();
            }
            if self
                .seen
                .iter()
                .any(|x| x.0 == address && x.1 == packet.connection)
            {
                continue;
            }
            if self.seen.len() == 256 {
                self.seen.pop_front();
            }
            self.seen.push_back((address, packet.connection, now));
            return Ok(ConnectionRequest {
                address,
                connection_id: packet.connection,
                received: now,
            });
        }
    }
}
impl ConnectionRequest {
    /// Connect back using this request's ID. Failure is terminal for this request.
    pub async fn accept(self, manager_name: impl Into<String>) -> Result<Device> {
        if self.received.elapsed() >= Duration::from_secs(10) {
            return Err(crate::Error::Disconnected);
        }
        Device::open_requested(self.address, self.connection_id, manager_name.into()).await
    }
}
