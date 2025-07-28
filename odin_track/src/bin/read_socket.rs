/*
 * Copyright © 2025, United States Government, as represented by the Administrator of 
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

use std::io::{self};
use tokio::{self,net::TcpStream, io::{BufReader,AsyncBufReadExt}};
use anyhow::{Result};
use odin_common::define_cli;
use odin_track::rs1090::{PropertyFinder,parse_msg};

define_cli! { ARGS [about="ADS-B socket monitoring tool"] =
    url: String [help="URL from where to read ADS-B jet1090 messages"]
}

#[tokio::main]
async fn main() -> Result<()> {
    let finder = PropertyFinder::new();

    let stream = TcpStream::connect( &ARGS.url).await?;
    let mut reader = BufReader::new( stream);
    let mut line = String::with_capacity(1024);

    loop {
        match reader.read_line(&mut line).await {
            Ok(bytes_read) => {
                let msg = line.as_bytes();
                //println!("@@ got {} bytes: {}", msg.len(), line);
                
                match parse_msg( msg, &finder) {
                    Ok(update) => println!("{update}"),
                    Err(e) => println!("PARSE ERROR for {}", String::from_utf8_lossy(msg))
                }
            }
            Err(e) => {
                eprintln!("Error reading from stream: {}", e);
                break;
            }
        }
        line.clear();
    }
    Ok(())
}