/*
 * Copyright 2026 Peter Mehlitz
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://apache.org
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! tests for odin_common::net. Resolution of the local mDNS/bonjour name of the machine is only tested if
//! ODIN_TEST_MDNS_HOST is set (e.g. ODIN_TEST_MDNS_HOST=beast.local)

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use odin_common::net::ServerAddr;

#[derive(serde::Deserialize, serde::Serialize, Debug)]
struct Config { sock_addr: ServerAddr }

#[test]
fn test_server_addr_ip () {
    let a: ServerAddr = "127.0.0.1:9009".parse().unwrap();
    assert_eq!( a.socket_addr(), SocketAddr::new( IpAddr::V4( Ipv4Addr::LOCALHOST), 9009));
    assert!( a.ip().is_loopback()); // Deref to SocketAddr
    assert_eq!( a.port(), 9009);
    assert!( !a.is_named());
    assert_eq!( a.authority(), "127.0.0.1:9009");

    let a: ServerAddr = "[::1]:9009".parse().unwrap();
    assert_eq!( a.ip(), IpAddr::V6( Ipv6Addr::LOCALHOST));
    assert!( !a.is_named());
    assert_eq!( a.authority(), "[::1]:9009");

    let a: ServerAddr = "0.0.0.0:443".parse().unwrap();
    assert!( a.ip().is_unspecified());
}

#[test]
fn test_server_addr_name () {
    let a: ServerAddr = "localhost:9009".parse().unwrap();
    assert!( a.is_named());
    assert_eq!( a.host(), "localhost");
    assert_eq!( a.ip(), IpAddr::V4( Ipv4Addr::LOCALHOST)); // IPv4 is preferred
    assert!( a.is_loopback());
    assert_eq!( a.port(), 9009);
    assert_eq!( a.authority(), "localhost:9009");
    println!("{a}");
}

#[test]
fn test_server_addr_errors () {
    assert!( "localhost".parse::<ServerAddr>().is_err());          // no port
    assert!( ":9009".parse::<ServerAddr>().is_err());              // no host
    assert!( "localhost:99999".parse::<ServerAddr>().is_err());    // invalid port
    assert!( "no-such-host.invalid:9009".parse::<ServerAddr>().is_err()); // .invalid never resolves (RFC 6761)
}

#[test]
fn test_server_addr_serde () {
    let cfg: Config = ron::from_str( r#"Config( sock_addr: "localhost:9009" )"#).unwrap();
    assert!( cfg.sock_addr.ip().is_loopback());
    assert_eq!( ron::to_string( &cfg).unwrap(), r#"(sock_addr:"localhost:9009")"#); // keeps the name

    let cfg: Config = ron::from_str( r#"Config( sock_addr: "127.0.0.1:9009" )"#).unwrap();
    assert_eq!( cfg.sock_addr.port(), 9009);

    let res: Result<Config,_> = ron::from_str( r#"Config( sock_addr: "no-such-host.invalid:9009" )"#);
    assert!( res.is_err());
    println!("expected error: {}", res.unwrap_err());
}

#[test]
fn test_server_addr_mdns () {
    let Ok(host) = std::env::var("ODIN_TEST_MDNS_HOST") else { println!("ODIN_TEST_MDNS_HOST not set - skipped"); return };
    let a: ServerAddr = format!("{host}:9009").parse().unwrap();
    println!("{a}");
    assert!( a.is_named());
    assert!( !a.is_loopback()); // the machine name has to be reachable from the network
    assert!( a.ip().is_ipv4() && !a.ip().is_loopback()); // primary address is a non-loopback IPv4
    assert!( a.bind_addrs().iter().all( |a| match a.ip() { IpAddr::V6(ip) => !ip.is_unicast_link_local(), _ => true }));

    // all resolved addresses have to be bindable on this machine
    for addr in a.bind_addrs() {
        let listener = std::net::TcpListener::bind( SocketAddr::new( addr.ip(), 0)).unwrap();
        println!("bound to {}", listener.local_addr().unwrap());
    }
}
