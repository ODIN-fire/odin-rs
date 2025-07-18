/*
 * Copyright © 2024, United States Government, as represented by the Administrator of 
 * the National Aeronautics and Space Administration. All rights reserved.
 *
 * The “ODIN” software is licensed under the Apache License, Version 2.0 (the "License"); 
 * you may not use this file except in compliance with the License. You may obtain a copy 
 * of the License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software distributed under
 * the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,
 * either express or implied. See the License for the specific language governing permissions
 * and limitations under the License.
 */
#![allow(unused)]

/// test utility to send WsCmd messages to the Delphire server, supporting command line message arg or interactive modes.
/// This uses the same sentinel.ron config as the other crate executables
/// 
/// example: 
/// ```
/// {"event": "trigger-alert", "data":{ "deviceIds": ["roo7gd1dldn3"]}}
/// ```

use odin_actor::errors::op_failed;
use tokio::{io,time::timeout};
use tokio_util::codec::{FramedRead, LinesCodec};
use tokio_tungstenite::tungstenite::{protocol::Message,error::Error};
use futures::{SinkExt,StreamExt};
use std::{io::{stdin,stdout,Write},time::Duration,fmt::Debug};
use strum::EnumString;

use odin_build;
use odin_common::{if_let,define_cli,check_cli};
use odin_sentinel::{
    get_device_list_from_config,get_http_client,
    OdinSentinelError, SentinelConfig, Result,
    ws::{connect, expect_connected_response, WsCmd, WsMsg, WsStream},
    load_config,
};

#[derive(Debug,EnumString)]
#[strum(serialize_all="snake_case")]
enum InputFormat { Raw, Ron, Json }

define_cli! { ARGS [about="Delphire Sentinel websocket command tool"] = 
    execute: Vec<String>  [help="send command in non-interactive mode", short,long],
    format: InputFormat   [help="input format for commands (raw,ron,json)", short,long,default_value="raw"],
    show_ping: bool       [help="show ping/pong messages",long]
}

#[tokio::main]
async fn main()->anyhow::Result<()> {
    odin_build::set_bin_context!();
    check_cli!(ARGS);

    let config: SentinelConfig = load_config( "sentinel.ron")?;
    if ARGS.execute.is_empty() {
        print_prolog(&config).await?;
    }

    let (mut ws,_) = connect(&config).await?;
    expect_connected_response(&mut ws).await?;

    if ARGS.execute.is_empty() {
        if let Some(interval) = config.ping_interval {
            run_interactive_ping(&mut ws, interval).await
        } else {
            run_interactive(&mut ws).await
        }
    } else {
        exec_cmds(&mut ws).await?
    }

    Ok(())
}

async fn print_prolog (config: &SentinelConfig)->Result<()> {
    let http_client = get_http_client()?;
    let device_list = get_device_list_from_config( &http_client, &config).await?;
    println!("interactive mode monitoring devices: {:?}", device_list.get_device_ids());
    if device_list.is_empty() { 
        Err(OdinSentinelError::NoDevicesError) 
    } else { 
        println!("enter commands as {:?} strings", ARGS.format);
        println!("terminate with ctrl-C or 'exit'");
        Ok(()) 
    }
}

async fn exec_cmds (ws: &mut WsStream)->Result<()> {
    for cmd in &ARGS.execute {
        match to_message_text(cmd) {
            Ok(cmd) => process_cmd( ws, cmd).await?,
            Err(e) => { eprintln!("ERROR invalid command input: {e:?}"); return Err(e) } 
        }
    }
    Ok(())
}

async fn run_interactive (ws: &mut WsStream) {
    let mut cmd = String::new();
    loop {
        show_prompt();
        if std::io::stdin().read_line(&mut cmd).is_err() || cmd == "exit"{ 
            break
        }
        process_cmd( ws, cmd.clone()).await;

        cmd.clear();
    }
}

async fn run_interactive_ping (ws: &mut WsStream, interval: Duration) {
    let stdin = io::stdin();
    let mut reader = FramedRead::new(stdin, LinesCodec::new());

    show_prompt();
    loop {
        match timeout(interval, reader.next()).await {
            Err(_) => { // interactive input timed out - ping
                if let Err(e) = process_ping(ws).await {
                    eprintln!("ping failure, terminating.");
                    break
                }
            }
            Ok(input) => {
                match input {
                    Some(Ok(line)) => {
                        if line == "exit" { break }
                        if let Ok(cmd) = to_message_text(&line) {
                            if let Err(e) = process_cmd(ws, cmd).await { break }
                        } else {
                            eprintln!("not a valid command");
                        }
                        show_prompt();
                    }
                    Some(Err(e)) => {
                        eprintln!("error reading input: {e:?}")
                    }
                    None => break // stream closed
                }
            }
        }
    }
}

fn show_prompt() {
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(b"> ");
    stdout.flush();
}

async fn process_ping (ws: &mut WsStream)->Result<()> {
    let ping = WsCmd::new_ping("ping");
    let cmd = serde_json::to_string(&ping)?;

    if_let! {
        Ok(_) = { ws.send( Message::text(cmd)).await } else |other| { handle_ws_error(other, "sending ping") },
        Some(response) = { ws.next().await } else { Err(OdinSentinelError::WsClosedError) },
        Ok(Message::Text(s)) = { response } else |other| { handle_ws_error(other, "unexpected Ping response") },
        Ok(WsMsg::Pong{..}) = { serde_json::from_str::<WsMsg>(&s) } else |other| { eprintln!("\nERROR parsing Ping response: {other:?}"); Ok(()) } => {
            // nothing to report here - we just ping to keep the websocket open
            Ok(()) 
        }
    }
}

async fn process_cmd (ws: &mut WsStream, cmd: String)->Result<()> {
    if_let! {
        Ok(_) = { ws.send( Message::text(cmd)).await } else |other| { handle_ws_error(other, "sending command") },
        Some(response) = { ws.next().await } else { Err(OdinSentinelError::WsClosedError) },
        Ok(Message::Text(s)) = { response } else |other| { handle_ws_error(other, "unexpected command response") } => {
            println!("{s}");
            Ok(())
        }
    }
}

/// translate cmd to WsCmd and return JSON serialization of it
fn to_message_text (cmd: &String)->Result<String> {
    match ARGS.format {
        InputFormat::Raw => Ok(cmd.clone()),
        InputFormat::Ron => {
            let ws_cmd: WsCmd = ron::from_str(cmd).map_err(|e| op_failed(format!("{e:?}")))?;
            Ok(serde_json::to_string(&ws_cmd)?)
        }
        InputFormat::Json => {
            let ws_cmd: WsCmd = serde_json::from_str(cmd).map_err(|e| op_failed(format!("{e:?}")))?;
            Ok(serde_json::to_string(&ws_cmd)?)
        }
    }
}

/// return an Error if the connection is closed. All other results are reported and return Ok(())
fn handle_ws_error<T> (res: std::result::Result<T,Error>, msg: &str)->Result<()> where T: Debug {
    match res {
        Err(Error::ConnectionClosed) | Err(Error::AlreadyClosed) => {
            Err(OdinSentinelError::WsClosedError)
        }
        other => { 
            eprintln!("\nERROR {msg}: {other:?}");
            Ok(())
        }
    }
}