//! Pure Rust async DNS resolver and client customizer using 1.1.1.1 and 8.8.8.8.
//!
//! Avoids ISP DNS poisoning, local router (192.168.18.1) rate limits, and network blocks
//! by querying Cloudflare (1.1.1.1 / 1.0.0.1) and Google (8.8.8.8) directly over UDP.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::timeout;

/// Public fallback DNS servers.
pub const DEFAULT_DNS_SERVERS: &[&str] = &[
    "1.1.1.1:53",
    "1.0.0.1:53",
    "8.8.8.8:53",
    "8.8.4.4:53",
];

/// Known upstream AI hosts that are pre-resolved through 1.1.1.1 to bypass local DNS filters.
pub const DEFAULT_UPSTREAM_DOMAINS: &[&str] = &[
    "api.groq.com",
    "integrate.api.nvidia.com",
    "api.nvidia.com",
    "assets.ngc.nvidia.com",
];

/// Build a standard RFC 1035 DNS Query packet.
pub fn build_dns_query(domain: &str, qtype: u16) -> anyhow::Result<Vec<u8>> {
    let mut packet = Vec::with_capacity(64);

    // Header: ID (0x4a12), Standard Query (0x0100), QDCOUNT=1, ANCOUNT=0, NSCOUNT=0, ARCOUNT=0
    packet.extend_from_slice(&[
        0x4a, 0x12, // ID
        0x01, 0x00, // Flags: Standard Query, Recursion Desired
        0x00, 0x01, // QDCOUNT = 1
        0x00, 0x00, // ANCOUNT = 0
        0x00, 0x00, // NSCOUNT = 0
        0x00, 0x00, // ARCOUNT = 0
    ]);

    // QNAME: labels length-prefixed, ending with 0x00
    for part in domain.trim_matches('.').split('.') {
        if part.is_empty() || part.len() > 63 {
            anyhow::bail!("invalid domain segment: {part}");
        }
        packet.push(part.len() as u8);
        packet.extend_from_slice(part.as_bytes());
    }
    packet.push(0x00);

    // QTYPE and QCLASS (IN = 1)
    packet.extend_from_slice(&qtype.to_be_bytes());
    packet.extend_from_slice(&1u16.to_be_bytes());

    Ok(packet)
}

/// Parse A (IPv4) records from an RFC 1035 DNS response packet.
pub fn parse_dns_a_response(buf: &[u8]) -> anyhow::Result<Vec<Ipv4Addr>> {
    if buf.len() < 12 {
        anyhow::bail!("DNS response too short: {} bytes", buf.len());
    }

    let ancount = u16::from_be_bytes([buf[6], buf[7]]);
    if ancount == 0 {
        return Ok(Vec::new());
    }

    let mut pos = 12;

    // Skip Question section
    while pos < buf.len() {
        let len = buf[pos] as usize;
        if len == 0 {
            pos += 1;
            break;
        }
        if len >= 192 {
            // Pointer in question
            pos += 2;
            break;
        }
        pos += 1 + len;
    }
    pos += 4; // Skip QTYPE and QCLASS

    let mut ips = Vec::new();

    // Parse Answer section
    for _ in 0..ancount {
        if pos >= buf.len() {
            break;
        }

        // Skip Name (handle compression pointer or uncompressed labels)
        if buf[pos] >= 192 {
            pos += 2;
        } else {
            while pos < buf.len() {
                let len = buf[pos] as usize;
                if len == 0 {
                    pos += 1;
                    break;
                }
                pos += 1 + len;
            }
        }

        if pos + 10 > buf.len() {
            break;
        }

        let atype = u16::from_be_bytes([buf[pos], buf[pos + 1]]);
        let _aclass = u16::from_be_bytes([buf[pos + 2], buf[pos + 3]]);
        let _ttl = u32::from_be_bytes([buf[pos + 4], buf[pos + 5], buf[pos + 6], buf[pos + 7]]);
        let rdlength = u16::from_be_bytes([buf[pos + 8], buf[pos + 9]]) as usize;
        pos += 10;

        if pos + rdlength > buf.len() {
            break;
        }

        if atype == 1 && rdlength == 4 {
            // Type A (IPv4)
            ips.push(Ipv4Addr::new(
                buf[pos],
                buf[pos + 1],
                buf[pos + 2],
                buf[pos + 3],
            ));
        }

        pos += rdlength;
    }

    Ok(ips)
}

/// Resolve IPv4 addresses for a domain using a specific upstream DNS server (default 1.1.1.1:53).
pub async fn resolve_domain_from_server(
    domain: &str,
    server: &str,
) -> anyhow::Result<Vec<Ipv4Addr>> {
    let server_addr: SocketAddr = if server.contains(':') {
        server.parse()?
    } else {
        format!("{server}:53").parse()?
    };

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    let packet = build_dns_query(domain, 1)?;
    socket.send_to(&packet, server_addr).await?;

    let mut buf = [0u8; 512];
    let (len, _) = timeout(Duration::from_millis(2000), socket.recv_from(&mut buf)).await??;

    parse_dns_a_response(&buf[..len])
}

/// Resolve domain trying multiple public DNS servers (1.1.1.1, 1.0.0.1, 8.8.8.8) with fallback.
pub async fn resolve_domain(domain: &str) -> anyhow::Result<Vec<Ipv4Addr>> {
    for server in DEFAULT_DNS_SERVERS {
        if let Ok(ips) = resolve_domain_from_server(domain, server).await
            && !ips.is_empty()
        {
            return Ok(ips);
        }
    }
    anyhow::bail!("failed to resolve domain {domain} via public DNS servers");
}

/// Build a high-performance `reqwest::Client` with direct DNS mappings from 1.1.1.1.
pub async fn build_dns_hardened_client(extra_domains: &[&str]) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120));

    let mut all_domains = DEFAULT_UPSTREAM_DOMAINS.to_vec();
    for d in extra_domains {
        if !all_domains.contains(d) {
            all_domains.push(d);
        }
    }

    for domain in all_domains {
        match resolve_domain(domain).await {
            Ok(ips) => {
                for ip in ips {
                    builder = builder.resolve(
                        domain,
                        SocketAddr::new(IpAddr::V4(ip), 443),
                    );
                }
            }
            Err(e) => {
                tracing::debug!(domain = domain, error = ?e, "could not pre-resolve domain via 1.1.1.1, falling back to system resolver");
            }
        }
    }

    builder.build().unwrap_or_else(|_| reqwest::Client::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_valid_dns_query() {
        let q = build_dns_query("ilmeee.com", 1).unwrap();
        assert_eq!(q[0], 0x4a);
        assert_eq!(q[1], 0x12);
        assert_eq!(q[12], 6); // "ilmeee" length
        assert_eq!(&q[13..19], b"ilmeee");
        assert_eq!(q[19], 3); // "com" length
        assert_eq!(&q[20..23], b"com");
        assert_eq!(q[23], 0); // null terminator
    }

    #[tokio::test]
    async fn resolves_domain_via_cloudflare_dns() {
        // Test query against 1.1.1.1
        if let Ok(ips) = resolve_domain_from_server("cloudflare.com", "1.1.1.1:53").await {
            assert!(!ips.is_empty(), "expected at least 1 IP for cloudflare.com");
        }
    }
}
