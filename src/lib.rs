//! Small crate for accessing Prologix GPIB-ETHERNET controllers

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, Instant};

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
}

/// Discover any Prologix GPIB-ETHERNET controllers on the network.
/// Returns a vector of IpAddr if any controllers was found. Returns a [Error::NotFound] if no
/// controllers was found.
///
/// # Arguments
///
/// * `duration` - A optional duration for how long it should try to discover new controllers.
///                Defaults to 500ms if set to None.
pub async fn discover(duration: Option<Duration>) -> Result<Vec<IpAddr>, Error> {
    let mut addresses = HashSet::new();
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
        let mut buf: Vec<u8> = vec![0; 100];
        if let Ok(Ok((len, _))) =
            timeout(Duration::from_millis(100), socket.recv_from(&mut buf)).await
        {
            if len >= 24 {
                let tmp = &buf[20..24];
                let host = IpAddr::V4(Ipv4Addr::new(tmp[0], tmp[1], tmp[2], tmp[3]));

                addresses.insert(host);
            }
        }
    }

    if addresses.is_empty() {
        Err(Error::NotFound)
    } else {
        Ok(addresses.into_iter().collect())
    }
}

fn build_discovery() -> Vec<u8> {
    const IDENTIFY_CMD: u8 = 0x00;
    let mac_addr: Vec<u8> = vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
    let mut rng = rand::thread_rng();
    let seq = rng.gen::<u16>();

    build_msg_header(IDENTIFY_CMD, seq, &mac_addr)
}

fn build_msg_header(id: u8, seq: u16, mac_addr: &[u8]) -> Vec<u8> {
    let mut header = vec![PROLOGIX_MAGIC, id];
    let seq = seq.to_le_bytes();

    header.extend_from_slice(&seq);
    header.extend_from_slice(mac_addr);
    header.push(0x00);
    header.push(0x00);

    header
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
