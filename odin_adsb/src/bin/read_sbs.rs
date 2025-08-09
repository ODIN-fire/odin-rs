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
use chrono_tz::Tz;
use odin_common::{define_cli, u8extractor::{AsyncCsvExtractor,CsvFieldExtractor}};
use odin_adsb::sbs::parse_msg;

define_cli! { ARGS [about="ADS-B socket monitoring tool"] =
    url: String [help="URL from where to read ADS-B SBS messages"],
    tz: String [help="timezone of message source"]
}

#[tokio::main]
async fn main() -> Result<()> {
    let stream = TcpStream::connect( &ARGS.url).await?;
    let mut reader = BufReader::with_capacity( 4096, stream);
    let mut csv = AsyncCsvExtractor::new(reader);
    let tz: Tz = ARGS.tz.parse()?;

    while csv.next_line().await? {
        match parse_msg( &mut csv, &tz) {
            Ok(update) => println!("{update}"),
            Err(e) => println!("PARSE ERROR for {}", csv.line())
        }
    }
    Ok(())
}