#![no_main]
//! The rate-limit key behind trusted proxies: the socket peer and every
//! `X-Forwarded-For` line. The answer is always the socket or a hop the
//! chain named.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use libfuzzer_sys::fuzz_target;
use turbomcp_transport_http::fuzzing;

fuzz_target!(|input: (bool, u128, u16, Vec<Vec<u8>>)| {
    let (v6, raw, port, forwarded) = input;
    let ip = if v6 {
        IpAddr::V6(Ipv6Addr::from(raw))
    } else {
        IpAddr::V4(Ipv4Addr::from(raw as u32))
    };
    let trusted = ["10.0.0.0/8", "127.0.0.1/32", "2001:db8::/32", "0.0.0.0/1"];
    fuzzing::client_ip(SocketAddr::new(ip, port), &forwarded, &trusted);
});
