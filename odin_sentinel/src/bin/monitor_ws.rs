/*
 * Copyright (c) 2024, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The ODIN - Open Data Integration Framework is licensed under the
 * Apache License, Version 2.0 (the "License"); you may not use this file
 * except in compliance with the License. You may obtain a copy of the
 * License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */
#![allow(unused)]

#[macro_use]
extern crate lazy_static;

use std::{process::Output, path::PathBuf, str::FromStr, fmt::{Display,Formatter}, fs::File, io::Write, time::Duration};
use anyhow::Result;
use structopt::StructOpt;
use displaydoc::Display;
use tokio;
use tokio_tungstenite::{tungstenite::protocol::Message};
use reqwest;
use futures::{pin_mut,select,Stream,stream::{self,StreamExt},SinkExt};
use async_stream::stream;
use strum::EnumString;
use chrono::prelude::*;
use odin_sentinel::{SentinelConfig,get_device_list_from_config};
use odin_sentinel::ws::{connect,read_next_ws_msg,request_join,WsStream,WsMsg,WsCmd};
use odin_config::prelude::*;

use_config!();

/// command line arg: {0}
#[derive(Debug,Display)]
struct ParseArgError(String);


#[derive(Debug,EnumString)]
#[strum(serialize_all="snake_case")]
enum OutputFormat { Rust, Ron, Json }


#[derive(StructOpt)]
#[structopt(about = "Delphire Sentinel websocket monitoring tool")]
struct CliOpts {
    /// run verbose
    #[structopt(short)]
    verbose: bool,

    /// output time
    #[structopt(short)]
    time: bool,

    /// produce formatted output
    #[structopt(short)]
    pretty: bool,

    /// output format (rust,ron,json)
    #[structopt(short,long,default_value="rust")]
    format: OutputFormat,

    //.. and more to follow
}

lazy_static! {
    static ref ARGS: CliOpts = CliOpts::from_args();
}

#[tokio::main]
async fn main()->Result<()> {
    let config: SentinelConfig = config_for!( "sentinel")?;
    let http_client = reqwest::Client::new();
    let mut msg_id = 0;

    let device_list = get_device_list_from_config( &http_client, &config).await?;
    let device_ids = device_list.get_device_ids();
    println!("monitoring devices: {:?}", device_ids);

    let (mut ws,_) = connect( &config).await?;
    let resp = read_next_ws_msg(&mut ws).await?;
    log_ws_msg(&resp);

    request_join( &mut ws, device_ids, get_next_msg_id(&mut msg_id)).await?;
    let resp = read_next_ws_msg(&mut ws).await?;
    log_ws_msg(&resp);

    if let Some(ping_interval) = config.ping_interval {
        run_with_ping(&mut ws, ping_interval).await;
    } else {
        run_without_ping(&mut ws).await;
    }

    Ok(())
}

async fn run_without_ping (ws: &mut WsStream) {
    loop {
        if let Ok(msg) = read_next_ws_msg(ws).await {
            log_ws_msg(&msg);
        }
    }
}

async fn run_with_ping (ws: &mut WsStream, interval: Duration) {
    let (mut write,read_stream) = ws.split();
    let msg_stream = read_stream.fuse();
    let ping_stream = new_ping_stream(interval).fuse();

    pin_mut!( msg_stream, ping_stream);

    loop {
        select! {
            msg = msg_stream.next() => {
                if let Some(msg) = msg {
                    if let Ok(msg) = msg {
                        match msg {
                            Message::Text(msg) => {
                                if let Ok(msg) = serde_json::from_str( msg.as_str()) {
                                    log_ws_msg(&msg);
                                }
                            }
                            other => {
                                println!("ignoring non-text message {:?}", other);
                            }
                        }
                    }
                }
            },
            ping = ping_stream.next() => {
                if let Some(ping) = ping {
                    log_ws_cmd(&ping);
                    if let Ok(json) = serde_json::to_string(&ping) {
                        let ws_msg = Message::Text(json);
                        write.send(ws_msg).await;
                    }
                }
            } 
        }
    }
}

fn new_ping_stream (interval: Duration)->impl Stream<Item=WsCmd> {
    stream::unfold( 0, move |cycle| async move {
        tokio::time::sleep( interval).await;
        let cmd = WsCmd::new_ping( format!("ping-{}", cycle));
        Some( (cmd, cycle+1) )
    })
}

fn get_next_msg_id (msg_id: &mut usize)->String {
    *msg_id += 1;
    msg_id.to_string()
}

fn log_ws_msg (msg: &WsMsg)->Result<()> {
    let mut stdout = std::io::stdout().lock();
    if ARGS.time {
        let now = Local::now();
        let ts = format!("[{}]", now.format("%H:%M:%S%.3f"));
        stdout.write_all(ts.as_bytes());
    }

    stdout.write(b"> ");
    match ARGS.format {
        OutputFormat::Json => {
            let s = if ARGS.pretty {
                serde_json::to_string_pretty(msg)?
            } else {
                serde_json::to_string(msg)?
            };
            stdout.write_all( s.as_bytes());
            stdout.write(b"\n");
        }
        OutputFormat::Rust => {
            if ARGS.pretty {
                writeln!( stdout, "{:#?}", msg);
            } else {
                writeln!( stdout, "{:?}", msg);
            };
        }
        OutputFormat::Ron => {
            let s = if ARGS.pretty {
                ron::ser::to_string_pretty(msg, ron::ser::PrettyConfig::default())?
            } else {
                ron::to_string(msg)?
            };
            stdout.write_all( s.as_bytes());
            stdout.write(b"\n");
        }
    }
    Ok(())
}

fn log_ws_cmd (cmd: &WsCmd)->Result<()> {
    let mut stdout = std::io::stdout().lock();
    if ARGS.time {
        let now = Local::now();
        let ts = format!("[{}]", now.format("%H:%M:%S%.3f"));
        stdout.write_all(ts.as_bytes());
    }
    stdout.write(b"< ");
    writeln!( stdout, "{:?}", cmd);

    Ok(())
}
