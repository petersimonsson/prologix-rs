//! This crate can be used to discover and configure Prologix GPIB-ETHERNET controllers

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

const IDENTIFY_CMD: u8 = 0x00;
const REBOOT_CMD: u8 = 0x12;

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
    pub fn new(addr: [u8; 6]) -> Self {
        MacAddress { addr }
    }

    pub fn addr(&self) -> &[u8] {
        &self.addr
    }
}

impl Default for MacAddress {
    fn default() -> Self {
        MacAddress {
            addr: [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
        }
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
    fn new(magic: u8, id: u8, seq: u16, mac_addr: MacAddress) -> Self {
        MsgHeader {
            magic,
            id,
            seq,
            mac_addr,
        }
    }

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
pub struct ControllerVersion {
    major: u8,
    minor: u8,
    patch: u8,
    bugfix: u8,
}

impl ControllerVersion {
    pub fn new(major: u8, minor: u8, patch: u8, bugfix: u8) -> Self {
        ControllerVersion {
            major,
            minor,
            patch,
            bugfix,
        }
    }

    pub fn major(&self) -> u8 {
        self.major
    }

    pub fn minor(&self) -> u8 {
        self.minor
    }

    pub fn patch(&self) -> u8 {
        self.patch
    }

    pub fn bugfix(&self) -> u8 {
        self.bugfix
    }
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
    ip_netmask: IpAddr,
    ip_gateway: IpAddr,
    app_version: ControllerVersion,
    boot_version: ControllerVersion,
    hardware_version: ControllerVersion,
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

        Ok(ControllerInfo {
            mac_addr: header.mac_addr,
            uptime: Duration::from_secs(
                uptime_days + msg[14] as u64 * 3600 + msg[15] as u64 * 60 + msg[16] as u64,
            ),
            mode: ControllerMode::from(msg[17]),
            alert: ControllerAlert::from(msg[18]),
            ip_type: ControllerIpType::from(msg[19]),
            ip_addr: IpAddr::V4(Ipv4Addr::new(msg[20], msg[21], msg[22], msg[23])),
            ip_netmask: IpAddr::V4(Ipv4Addr::new(msg[24], msg[25], msg[26], msg[27])),
            ip_gateway: IpAddr::V4(Ipv4Addr::new(msg[28], msg[29], msg[30], msg[31])),
            app_version: ControllerVersion::new(msg[32], msg[33], msg[34], msg[35]),
            boot_version: ControllerVersion::new(msg[36], msg[37], msg[38], msg[39]),
            hardware_version: ControllerVersion::new(msg[40], msg[41], msg[42], msg[43]),
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

    pub fn ip_netmask(&self) -> &IpAddr {
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
/// Returns a Vec of [ControllerInfo] if any controllers was found.
/// Returns a [Error::NotFound] if no controllers was found.
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
    let mut rng = rand::thread_rng();
    let seq = rng.gen::<u16>();
    let header = MsgHeader::new(PROLOGIX_MAGIC, IDENTIFY_CMD, seq, MacAddress::default());

    header.to_bytes()
}

/// Send reboot message to the Prologix GPIB-ETHERNET controller at `addr`.
pub async fn reboot(addr: &IpAddr) -> Result<(), Error> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket
        .send_to(
            &build_reboot(&RebootType::Reset),
            addr.to_string() + ":3040",
        )
        .await?;
    Ok(())
}

pub enum RebootType {
    Bootloader,
    Reset,
}

impl RebootType {
    fn to_u8(&self) -> u8 {
        match self {
            Self::Bootloader => 0,
            Self::Reset => 1,
        }
    }
}

fn build_reboot(reboot_type: &RebootType) -> Vec<u8> {
    let mut rng = rand::thread_rng();
    let seq = rng.gen::<u16>();
    let header = MsgHeader::new(PROLOGIX_MAGIC, REBOOT_CMD, seq, MacAddress::default());
    let mut bytes = header.to_bytes();
    bytes.push(reboot_type.to_u8());
    bytes.push(0);
    bytes.push(0);
    bytes.push(0);

    bytes
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
