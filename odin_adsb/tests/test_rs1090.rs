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

use std::fs;
use odin_common::{u8extractor::U8Readable, fs::read_lines};
use odin_adsb::rs1090::{self,Timestamp};

//--- test data

//--- df 17
const DF_17_5_1: &'static str = r#"{"timestamp":1753227401.9872684,"frame":"8da66970581dc50902b58e659e5e","df":"17","icao24":"a66970","bds":"05","tc":11,"NUCp":7,"NICb":0,"altitude":4900,"source":"barometric","parity":"odd","lat_cpr":33921,"lon_cpr":46478,"metadata":[{"system_timestamp":1753227401.9872684,"rssi":-13.328466,"serial":14924845721654670821,"name":"rtlsdr"}]}"#;
const DF_17_5_2: &'static str = r#"{"timestamp":1753227402.4442508,"frame":"8da12e8058af84c0b159c9d44467","df":"17","icao24":"a12e80","bds":"05","tc":11,"NUCp":7,"NICb":0,"altitude":34000,"source":"barometric","parity":"odd","lat_cpr":24664,"lon_cpr":88521,"latitude":37.75833388506356,"longitude":-119.93195243503737,"metadata":[{"system_timestamp":1753227402.4442508,"rssi":-19.246555,"serial":14924845721654670821,"name":"rtlsdr"}]}"#;
const DF_17_9: &'static str   = r#"{"timestamp":1753227402.170955,"frame":"8da0b59d990849b660043ed519f7","df":"17","icao24":"a0b59d","bds":"09","NACv":1,"groundspeed":439.9318128983172,"track":170.58049974178377,"vrate_src":"barometric","vertical_rate":0,"geo_minus_baro":1525,"metadata":[{"system_timestamp":1753227402.170955,"rssi":-10.372777,"serial":14924845721654670821,"name":"rtlsdr"}]}"#;
const DF_17_61: &'static str  = r#"{"timestamp":1753227401.9872699,"frame":"8dac04c5e11a1a00000000a82488","df":"17","icao24":"ac04c5","bds":"61","subtype":"emergency_priority","emergency_state":"none","squawk":"3611","metadata":[{"system_timestamp":1753227401.9872699,"rssi":-2.878996,"serial":14924845721654670821,"name":"rtlsdr"}]}"#;
const DF_17_62: &'static str  = r#"{"timestamp":1753227402.0427268,"frame":"8da682efea4a5867a95c080d2065","df":"17","icao24":"a682ef","bds":"62","source":"MCP/FCU","selected_altitude":38000,"barometric_setting":1013.6,"selected_heading":329.0625,"NACp":10,"tcas_operational":true,"metadata":[{"system_timestamp":1753227402.0427268,"rssi":-13.692514,"serial":14924845721654670821,"name":"rtlsdr"}]}"#;
const DF_17_65: &'static str  = r#"{"timestamp":1753227401.987267,"frame":"8da0b59df8210002004ab836e650","df":"17","icao24":"a0b59d","bds":"65","version":"2","NICa":0,"NACp":10,"GVA":2,"SIL":3,"BAI":1,"HRD":0,"SILs":0,"metadata":[{"system_timestamp":1753227401.987267,"rssi":-10.0772085,"serial":14924845721654670821,"name":"rtlsdr"}]}"#;

//--- df 0
const DF_0: &'static str = r#"{"timestamp":1753227402.1709576,"frame":"02e196b6e9bedec8846a65447563","df":"0","altitude":35550,"icao24":"adf64e","metadata":[{"system_timestamp":1753227402.1709576,"rssi":-6.2500696,"serial":14924845721654670821,"name":"rtlsdr"}]}"#;

//--- df 4
const MSG_7: &'static str = r#"{"timestamp":1753227402.1709735,"frame":"20000ab4ecafe907b20d416a6f4a","df":"4","altitude":16300,"icao24":"06a128","metadata":[{"system_timestamp":1753227402.1709735,"rssi":-7.1229773,"serial":14924845721654670821,"name":"rtlsdr"}]}"#;

//--- df 11
const MSG_8: &'static str = r#"{"timestamp":1753227402.170953,"frame":"5d39b493781d44a592cc60759999","df":"11","capability":"airborne","icao24":"39b493","metadata":[{"system_timestamp":1753227402.170953,"rssi":-14.824957,"serial":14924845721654670821,"name":"rtlsdr"}]}"#;


// run with "cargo test test_parse -- --nocapture"

 #[test]
 fn test_parse_meta () {
    let finder = rs1090::PropertyFinder::new();
    println!("\n--- test_parse_meta in DF_17_5_1");
    if let Some(idx) = finder.metadata.find_key( DF_17_5_1.as_bytes()) {
        println!("idx = {idx}");
    } else {
        panic!("failed to find metadata");
    }
 }

 #[test]
 fn test_parse_time () {
    let finder = rs1090::PropertyFinder::new();

    let buf = DF_17_5_1.as_bytes();
    println!("\n--- test_parse_time in DF_17_5_1");
    if let Some(idx) = finder.timestamp.find_key( buf) {
        if let Some((date,i)) = Timestamp::from_u8( buf, idx+finder.timestamp.len()) {
            println!("{}\n", date.0.format("%Y-%m-%d %H:%M:%S%.3f"));
        } else {
            panic!("failed to parse date");
        }
    } else {
        panic!("failed to find timestamp");
    }
 }

 #[test]
 fn test_parse_msgs () {
    let finder = rs1090::PropertyFinder::new();

    if let Ok(lines) = read_lines("resources/jet1090-1000.log") {
        for line in lines.map_while(Result::ok) {
            let msg = line.as_bytes();
            
            match rs1090::parse_msg( msg, &finder) {
                Ok(update) => println!("{update}"),
                Err(e) => panic!("failed to parse message {line}")
            }
        }
    }
 }