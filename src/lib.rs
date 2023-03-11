//! Small crate for accessing Prologix GPIB-ETHERNET controllers

use core::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr},
};

use rand::prelude::*;
use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::time::timeout;

const PROLOGIX_MAGIC: u8 = 0x5A;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Socket error")]
    Io(#[from] std::io::Error),
    #[error("No controller found")]
    NotFound,
    #[error("Failed parsing message")]
    ParseError { info: String },
}

#[derive(Debug)]
pub struct MacAddress {
    addr: [u8; 6],
}

impl MacAddress {
    pub fn addr(&self) -> &[u8] {
        &self.addr
    }
}

impl fmt::Display for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.addr[0], self.addr[1], self.addr[2], self.addr[3], self.addr[4], self.addr[5]
        )
    }
}

pub struct MsgHeader {
    magic: u8,
    id: u8,
    seq: u16,
    mac_addr: MacAddress,
}

impl MsgHeader {
    fn to_bytes(&self) -> Vec<u8> {
        let mut header = vec![self.magic, self.id];
        let seq = self.seq.to_be_bytes();

        header.extend_from_slice(&seq);
        header.extend_from_slice(self.mac_addr.addr());
        header.push(0x00);
        header.push(0x00);

        header
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        let mut seq = [0u8; 2];
        seq.copy_from_slice(&bytes[2..4]);
        let seq = u16::from_be_bytes(seq);

        let mut mac_addr = [0u8; 6];
        mac_addr.copy_from_slice(&bytes[4..10]);

        MsgHeader {
            magic: bytes[0],
            id: bytes[1],
            seq,
            mac_addr: MacAddress { addr: mac_addr },
        }
    }
}

#[derive(Debug)]
pub enum ControllerMode {
    BootLoader,
    Application,
}

impl From<u8> for ControllerMode {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::BootLoader,
            _ => Self::Application,
        }
    }
}

impl fmt::Display for ControllerMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug)]
pub enum ControllerAlert {
    Ok,
    Warning,
    Error,
}

impl From<u8> for ControllerAlert {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Ok,
            1 => Self::Warning,
            _ => Self::Error,
        }
    }
}

impl fmt::Display for ControllerAlert {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug)]
pub enum ControllerIpType {
    Dynamic,
    Static,
}

impl From<u8> for ControllerIpType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Dynamic,
            _ => Self::Static,
        }
    }
}

impl fmt::Display for ControllerIpType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug)]
pub struct ControllerNetmask {
    mask: [u8; 4],
}

impl ControllerNetmask {
    pub fn mask(&self) -> &[u8] {
        &self.mask
    }
}

impl fmt::Display for ControllerNetmask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{}.{}.{}",
            self.mask[0], self.mask[1], self.mask[2], self.mask[3]
        )
    }
}

#[derive(Debug)]
pub struct ControllerVersion {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
    pub bugfix: u8,
}

impl fmt::Display for ControllerVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{}.{}.{}",
            self.major, self.minor, self.patch, self.bugfix
        )
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct ControllerInfo {
    mac_addr: MacAddress,
    uptime: Duration,
    mode: ControllerMode,
    alert: ControllerAlert,
    ip_type: ControllerIpType,
    ip_addr: IpAddr,
    ip_netmask: ControllerNetmask,
    ip_gateway: IpAddr,
    app_version: ControllerVersion,
    boot_version: ControllerVersion,
    hardware_version: ControllerVersion,
    name: Vec<u8>,
}

impl ControllerInfo {
    fn from_bytes(msg: &[u8]) -> Result<Self, Error> {
        if msg.len() < 76 {
            return Err(Error::ParseError {
                info: "Failed to parse ControllerInfo".to_string(),
            });
        }

        let header = MsgHeader::from_bytes(&msg[0..12]);

        if header.magic != PROLOGIX_MAGIC {
            return Err(Error::ParseError {
                info: "Incorrect magic number at start of message".to_string(),
            });
        }

        let mut uptime_days = [0u8; 2];
        uptime_days.copy_from_slice(&msg[12..14]);
        let uptime_days = u16::from_be_bytes(uptime_days) as u64 * 24 * 3600;

        let mut ip_netmask = [0u8; 4];
        ip_netmask.copy_from_slice(&msg[24..28]);

        Ok(ControllerInfo {
            mac_addr: header.mac_addr,
            uptime: Duration::from_secs(
                uptime_days + msg[14] as u64 * 3600 + msg[15] as u64 * 60 + msg[16] as u64,
            ),
            mode: ControllerMode::from(msg[17]),
            alert: ControllerAlert::from(msg[18]),
            ip_type: ControllerIpType::from(msg[19]),
            ip_addr: IpAddr::V4(Ipv4Addr::new(msg[20], msg[21], msg[22], msg[23])),
            ip_netmask: ControllerNetmask { mask: ip_netmask },
            ip_gateway: IpAddr::V4(Ipv4Addr::new(msg[28], msg[29], msg[30], msg[31])),
            app_version: ControllerVersion {
                major: msg[32],
                minor: msg[33],
                patch: msg[34],
                bugfix: msg[35],
            },
            boot_version: ControllerVersion {
                major: msg[36],
                minor: msg[37],
                patch: msg[38],
                bugfix: msg[39],
            },
            hardware_version: ControllerVersion {
                major: msg[40],
                minor: msg[41],
                patch: msg[42],
                bugfix: msg[43],
            },
            name: msg[44..76].to_vec(),
        })
    }

    pub fn mac_addr(&self) -> &MacAddress {
        &self.mac_addr
    }

    pub fn uptime(&self) -> &Duration {
        &self.uptime
    }

    pub fn mode(&self) -> &ControllerMode {
        &self.mode
    }

    pub fn alert(&self) -> &ControllerAlert {
        &self.alert
    }

    pub fn ip_type(&self) -> &ControllerIpType {
        &self.ip_type
    }

    pub fn ip_addr(&self) -> &IpAddr {
        &self.ip_addr
    }

    pub fn ip_netmask(&self) -> &ControllerNetmask {
        &self.ip_netmask
    }

    pub fn ip_gateway(&self) -> &IpAddr {
        &self.ip_gateway
    }

    pub fn app_verion(&self) -> &ControllerVersion {
        &self.app_version
    }

    pub fn boot_verion(&self) -> &ControllerVersion {
        &self.boot_version
    }

    pub fn hardware_version(&self) -> &ControllerVersion {
        &self.hardware_version
    }
}

/// Discover any Prologix GPIB-ETHERNET controllers on the network.
/// Returns a vector of IpAddr if any controllers was found. Returns a [Error::NotFound] if no
/// controllers was found.
///
/// # Arguments
///
/// * `duration` - A optional duration for how long it should try to discover new controllers.
///                Defaults to 500ms if set to None.
pub async fn discover(duration: Option<Duration>) -> Result<Vec<Arc<ControllerInfo>>, Error> {
    let mut addresses = HashMap::new();
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.set_broadcast(true)?;

    socket
        .send_to(&build_discovery(), "255.255.255.255:3040")
        .await?;

    let now = Instant::now();
    let max_duration = match duration {
        Some(duration) => duration,
        None => Duration::from_millis(500),
    };

    while Instant::now().duration_since(now) < max_duration {
        let mut buf: Vec<u8> = vec![0; 76];
        if let Ok(Ok((len, _))) =
            timeout(Duration::from_millis(100), socket.recv_from(&mut buf)).await
        {
            if len >= 24 {
                let controller = Arc::new(ControllerInfo::from_bytes(&buf)?);
                addresses.insert(controller.ip_addr, controller);
            }
        }
    }

    if addresses.is_empty() {
        Err(Error::NotFound)
    } else {
        Ok(addresses.into_iter().map(|(_, info)| info).collect())
    }
}

fn build_discovery() -> Vec<u8> {
    const IDENTIFY_CMD: u8 = 0x00;
    let mac_addr: [u8; 6] = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
    let mut rng = rand::thread_rng();
    let seq = rng.gen::<u16>();
    let header = MsgHeader {
        magic: PROLOGIX_MAGIC,
        id: IDENTIFY_CMD,
        seq,
        mac_addr: MacAddress { addr: mac_addr },
    };

    header.to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_build_discover() {
        let result = build_discovery();
        assert!(result.starts_with(&[0x5A, 0x00]));
        assert!(result.ends_with(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00]))
    }
}
