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

//! tests for SBS connection handling (timeouts, EOF and reconnects) with a local TCP server

use std::{sync::{Arc, atomic::{AtomicI64, AtomicUsize, Ordering}}, time::Duration};
use dashmap::DashMap;
use tokio::{io::AsyncWriteExt, net::TcpListener, time::sleep};
use odin_adsb::{Aircraft, adsb::AdsbConfig, sbs::{process_msgs, run_connection}};

const SBS_MSG: &str = "MSG,3,1,1,A2E9A3,1,2025/07/28,15:00:27.393,2025/07/28,15:00:27.445,,22400,,,37.78436,-121.95081,,,0,,0,0\n";

fn config (url: String, max_reconnects: u32)->Arc<AdsbConfig> {
    Arc::new( AdsbConfig {
        source: "test".into(),
        timezone: "America/Los_Angeles".parse().unwrap(),
        url,
        update_interval: Duration::from_secs(60),
        max_trace: 8,
        drop_after: Duration::from_secs(30),
        connect_timeout: Duration::from_millis(500),
        read_timeout: Duration::from_millis(200),
        reconnect_delay: Duration::from_millis(50),
        max_reconnects,
    })
}

#[tokio::test]
async fn test_read_timeout () {
    // server sends two messages and then goes silent (like a dead connection after host sleep)
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = listener.local_addr().unwrap().to_string();
    tokio::spawn( async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        socket.write_all( SBS_MSG.as_bytes()).await.unwrap();
        socket.write_all( b"\n").await.unwrap(); // empty line used to panic in the extractor
        socket.write_all( SBS_MSG.as_bytes()).await.unwrap();
        sleep( Duration::from_secs(5)).await;
    });

    let cfg = config( url, 0);
    let aircraft = Arc::new( DashMap::<String,Aircraft>::new());
    let mut n_lines = 0;
    let res = process_msgs( &cfg, &Arc::new( AtomicI64::new(0)), &aircraft, &mut n_lines).await;

    assert!( res.is_err()); // read timeout
    assert_eq!( n_lines, 3);
    assert!( aircraft.contains_key("A2E9A3"));
}

#[tokio::test]
async fn test_eof () {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = listener.local_addr().unwrap().to_string();
    tokio::spawn( async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        socket.write_all( SBS_MSG.as_bytes()).await.unwrap();
    }); // socket is closed when the task ends

    let cfg = config( url, 0);
    let mut n_lines = 0;
    let res = process_msgs( &cfg, &Arc::new( AtomicI64::new(0)), &Arc::new( DashMap::new()), &mut n_lines).await;
    assert!( res.is_ok());
    assert_eq!( n_lines, 1);
}

#[tokio::test]
async fn test_max_reconnects () {
    // server accepts connections but never sends data, i.e. each connection times out
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = listener.local_addr().unwrap().to_string();
    let n_connections = Arc::new( AtomicUsize::new(0));
    tokio::spawn({
        let n_connections = n_connections.clone();
        async move {
            let mut sockets = Vec::new(); // keep them open
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                n_connections.fetch_add( 1, Ordering::Relaxed);
                sockets.push( socket);
            }
        }
    });

    // run_connection returns after the initial connection plus max_reconnects attempts
    run_connection( config( url, 2), Arc::new( AtomicI64::new(0)), Arc::new( DashMap::new())).await;
    assert_eq!( n_connections.load( Ordering::Relaxed), 3);
}
