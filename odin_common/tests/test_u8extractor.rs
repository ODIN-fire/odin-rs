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

use std::fmt::Debug;
use chrono::{DateTime,Utc,NaiveDate,NaiveTime,TimeZone};
use std::io::Cursor;
use anyhow::Result;
use odin_common::{
    extract_all, extract_fields, extract_ordered, 
    u8extractor::{AsyncCsvExtractor, CsvFieldExtractor, CsvStr, MemMemFinder, SimpleU8Finder,
                  U8Readable}
};

const DF_17_9: &'static str   = r#"{"timestamp":1753227402.170955,"frame":"8da0b59d990849b660043ed519f7","df":"17","icao24":"a0b59d","bds":"09","NACv":1,"groundspeed":439.9318128983172,"track":170.58049974178377,"vrate_src":"barometric","vertical_rate":0,"geo_minus_baro":1525,"metadata":[{"system_timestamp":1753227402.170955,"rssi":-10.372777,"serial":14924845721654670821,"name":"rtlsdr"}]}"#;

// run with  cargo test --test test_u8extractor -- test_memem --nocapture

#[test]
fn test_memmem() {
    println!("\n--- test_memem");
    let buf: &[u8] = DF_17_9.as_bytes();

    let find_df = MemMemFinder::new( b"\"df\":\"");
    let find_icao24 = MemMemFinder::new( b"\"icao24\":\"");
    let find_groundspeed = MemMemFinder::new( b"\"groundspeed\":");

    extract_all! { buf ?
        let df: u64 = find_df,
        let icao24: &str = find_icao24,
        let groundspeed: f64 = find_groundspeed => {
            println!("df = {df}, icao24= {icao24}, groundspeed = {groundspeed}");
        }
    }
}

#[test]
fn test_ordered() {
    println!("\n--- test_ordered");
    let buf: &[u8] = DF_17_9.as_bytes();

    let find_df = MemMemFinder::new( b"\"df\":\"");
    let find_icao24 = MemMemFinder::new( b"\"icao24\":\"");
    let find_groundspeed = MemMemFinder::new( b"\"groundspeed\":");

    extract_ordered! { buf ?
        let df: u64 = find_df,
        let icao24: &str = find_icao24,
        let groundspeed: f64 = find_groundspeed => {
            println!("df = {df}, icao24= {icao24}, groundspeed = {groundspeed}");
        }
    }
}

#[test]
fn test_failed_ordered() {
    println!("\n--- test_failed_ordered");
    let buf: &[u8] = DF_17_9.as_bytes();

    let find_df = MemMemFinder::new( b"\"df\":\"");
    let find_icao24 = MemMemFinder::new( b"\"icao24\":\"");
    let find_groundspeed = MemMemFinder::new( b"\"groundspeed\":");


    let success = extract_ordered! { buf ?
        let df: u64 = find_df,
        let groundspeed: f64 = find_groundspeed, // << out of order
        let icao24: &str = find_icao24 => {
            println!("df = {df}, icao24= {icao24}, groundspeed = {groundspeed}");
            true
        } else {
            println!("ordered parsing failed as expected");
            false
        }
    };

    if success { panic!( "ordered parsing should have failed") }
}

#[test]
 fn test_simple() {
    println!("\n--- test_simple");
    let buf: &[u8] = DF_17_9.as_bytes();

    let find_df = SimpleU8Finder::new( b"\"df\":\"");
    let find_icao24 = SimpleU8Finder::new( b"\"icao24\":\"");
    let find_groundspeed = SimpleU8Finder::new( b"\"groundspeed\":");

    extract_all! { buf ?
        let df: u64 = find_df,
        let icao24: &str = find_icao24,
        let groundspeed: f64 = find_groundspeed => {
            println!("df = {df}, icao24= {icao24}, groundspeed = {groundspeed}");
        }
    }
 }

 // example custom type U8Readable impl for DateTime<Utc> from fractional epoch secs

 // note that either the trait or the impl type have to be in this module - we have to newtype here
#[derive(Debug)]
struct Timestamp(DateTime<Utc>);

 impl<'a> U8Readable<'a,Timestamp> for Timestamp {
    fn from_u8 (buf: &'a[u8], i0: usize)->Option<(Timestamp,usize)> {
        let mut secs: i64 = 0;
        let mut frac: i64 = 0;
        let mut a: &mut i64 = &mut secs;
        let mut di = 0;

        let mut i = i0;

        loop {
            let b: u8 = buf[i];
            if b >= b'0' && b <= b'9' {
                *a = *a * 10 + (b as i64 - 48);
            } else if b == b'.' {
                a = &mut frac;
                di = i;
            } else {
                let nsecs = (((frac as f64) / 10.0f64.powi((i - di - 1) as i32)) * 1000000000.0) as u32;
                if let Some(date) = DateTime::from_timestamp( secs, nsecs) {
                    return Some((Timestamp(date),i-i0));
                } else {
                    return None;
                }
            }

            i += 1;
            if i >= buf.len() {
                return None;
            }
        }
    }
}

 #[test]
 fn test_timestamp() {
    println!("\n--- test_datetime");
    let buf: &[u8] = DF_17_9.as_bytes();

    let find_timestamp = MemMemFinder::new( b"\"timestamp\":");
    let find_icao24 = MemMemFinder::new( b"\"icao24\":\"");

    extract_all! { buf ?
        let timestamp: Timestamp = find_timestamp,
        let icao24: &str = find_icao24 => {
            println!("timestamp = {timestamp:?}, icao24= {icao24}");
        }
    }
 }

 #[tokio::test]
 async fn test_async_csv()->Result<()> {
    let mut data=String::new();
    data.push_str("MSG,3,1,1,A2E9A3,1,2025/07/28,15:00:27.393,2025/07/28,15:00:27.445,,22400,,,37.78436,-121.95081,,,0,,0,0\n");
    data.push_str("MSG,4,1,1,A29C41,1,2025/07/28,15:00:27.391,2025/07/28,15:00:27.445,,,122,132,,,-128,,,,,0");
    let cursor = Cursor::new(data.as_bytes());

    let mut csv = AsyncCsvExtractor::new(cursor);
    while csv.next_line().await? {
        println!("{}", csv.line());

        extract_fields! { csv ?
            let msg_type: u64 = [1],
            let icao24: CsvStr = [4],
            let date: CsvStr = [6],
            let time: CsvStr = [7] => {
                let date = NaiveDate::parse_from_str( date.as_str(), "%Y/%m/%d")?;
                let time = NaiveTime::parse_from_str( time.as_str(), "%H:%M:%S%.3f")?;
                let d = Utc.from_utc_datetime( &date.and_time(time));
                
                println!("  => msg={msg_type}, icao24={}, date={}", *icao24, d);
            }
        }
    }

    Ok(())
 }