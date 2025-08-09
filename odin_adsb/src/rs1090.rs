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

/// import from jet1090 (rs1090) TCP stream
/// as of 07/28/2025 jet1090 still takes about 3x CPU cycles compared to dump1090 and
/// uses 2x the bandwidth

use std::{io, sync::{Arc,atomic::{AtomicI64,Ordering}}, fmt::{self,Display,Formatter,Write}};
use tokio::{self,net::TcpStream, io::{BufReader,AsyncBufReadExt}};
use chrono::{DateTime,Utc};
use dashmap::DashMap;
use odin_common::{extract_all,extract_optional,u8extractor::{MemMemFinder,U8Readable}, datetime::EpochMillis};
use odin_macro::define_struct;

use crate::{Aircraft,AircraftStore};
use crate::adsb::{ignored,Position,AdsbData,AdsbUpdate};
use crate::errors::{parse_error,OdinAdsbError,Result};

// note that needle patterns have to include everything up to the first property value byte 
// (i.e. include ':' and opening '"' )
define_struct! {
    pub PropertyFinder =
        pub metadata: MemMemFinder<'static> = MemMemFinder::new(b"\"metadata\":"), // array (match used as a search bound)
        pub timestamp: MemMemFinder<'static> = MemMemFinder::new(b"\"timestamp\":"), // f64
        pub df: MemMemFinder<'static> = MemMemFinder::new(b"\"df\":\""), // str/u64
        pub icao24: MemMemFinder<'static> = MemMemFinder::new(b"\"icao24\":\""), // str
        pub bds: MemMemFinder<'static> = MemMemFinder::new(b"\"bds\":\""),  // str/u64
        pub groundspeed:  MemMemFinder<'static> = MemMemFinder::new(b"\"groundspeed\":"),  // f64
        pub altitude: MemMemFinder<'static> = MemMemFinder::new(b"\"altitude\":"), // u64
        pub track: MemMemFinder<'static> = MemMemFinder::new(b"\"track\":"), // f64
        pub heading: MemMemFinder<'static> = MemMemFinder::new(b"\"heading\":"), // f64
        pub vertical_rate: MemMemFinder<'static> = MemMemFinder::new(b"\"vertical_rate\":"), // i64
        pub longitude: MemMemFinder<'static> = MemMemFinder::new(b"\"longitude\":"), // f64
        pub latitude: MemMemFinder<'static> = MemMemFinder::new(b"\"latitude\":"), // f64
        pub callsign: MemMemFinder<'static> = MemMemFinder::new(b"\"callsign\":\""), // str
        pub selected_heading: MemMemFinder<'static> = MemMemFinder::new(b"\"selected_heading\":"), // f64
        pub selected_altitude: MemMemFinder<'static> = MemMemFinder::new(b"\"selected_altitude\":"), // u64
        pub capability: MemMemFinder<'static> = MemMemFinder::new(b"\"capability\":\""),

        pub comm_d_extended: MemMemFinder<'static> = MemMemFinder::new(b"\"df\":\"CommDExtended\"") // not clear if this is a bug
}

pub async fn process_msgs (url: &str, max_trace: usize, timestamp: Arc<AtomicI64>, aircraft: Arc<DashMap<String,Aircraft>>)->Result<()> {
    let finder = PropertyFinder::new();

    let stream = TcpStream::connect( url).await?;
    let mut reader = BufReader::new( stream);
    let mut line = String::with_capacity(1024);

    loop {
        match reader.read_line(&mut line).await {
            Ok(bytes_read) => {
                let msg = line.as_bytes();
                //println!("@@ got {} bytes: {}", msg.len(), line);
                
                match parse_msg( msg, &finder) {
                    Ok(update) => {
                        // TBD - add to aircraft and call update with changed aircraft ref
                    }
                    Err(e) => {
                        eprintln!("PARSE ERROR for {}", line)
                    }
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

/// this is the toplevel sync parser
pub fn parse_msg<'a> (msg: &'a [u8], finder: &PropertyFinder)->Result<AdsbUpdate<'a>> {
    if let Some(timestamp) = extract_optional!( msg, finder.timestamp, Timestamp) {
        let timestamp: EpochMillis = timestamp.0.into();
        if let Some(df) = extract_optional!( msg, finder.df, u64) {
            match df as u8 {
                0 => parse_short_air_air_surveillance( msg, finder, timestamp),  // DownlinkFormat::ShortAirAirSurveillance
                4 => parse_surveillance_altitude_reply( msg, finder, timestamp),  // DownlinkFormat::SurveillanceAltitudeReply
                11 => parse_all_call_reply( msg, finder, timestamp), // DownlinkFormat::AllCallReply
                17 => parse_extended_squitter_adsb( msg, finder, timestamp), // DownlinkFormat::ExtendedSquitterADSB
                _ => Ok(ignored(timestamp)) // ignore
            }
        } else { // no valid/known "df" tag
            if finder.comm_d_extended.find_key(msg).is_some() { // TODO - not sure if this is a rs1090/jet1090 bug
                Ok(ignored(timestamp))
            } else {
                Err(parse_error!( "not a valid Mode-S message"))
            }
        }
    } else { // no timestamp
         Err(parse_error!( "not a valid Mode-S message")) // malformed message
    }
}

pub fn parse_short_air_air_surveillance<'a> (msg: &'a[u8], finder: &PropertyFinder, timestamp: EpochMillis)->Result<AdsbUpdate<'a>> {
    extract_all! { msg ?
        let altitude: i64 = finder.altitude,
        let icao24: &str = finder.icao24 => {
            let data = AdsbData::ShortAirAirSurveillance{altitude};
            Ok( AdsbUpdate{ timestamp, icao24, data } )
        } else {
            Err( parse_error!( "failed to parse ShortAirAirSurveillance message: {}", String::from_utf8_lossy(msg)) )
        }
    }
}

pub fn parse_surveillance_altitude_reply<'a> (msg: &'a[u8], finder: &PropertyFinder, timestamp: EpochMillis)->Result<AdsbUpdate<'a>> {
    extract_all! { msg ?
        let altitude: i64 = finder.altitude,
        let icao24: &str = finder.icao24 => {
            let data = AdsbData::SurveillanceAltitudeReply{altitude};
            Ok( AdsbUpdate{ timestamp, icao24, data } )
        } else {
            Err( parse_error!( "failed to parse SurveillanceAltitudeReply message: {}", String::from_utf8_lossy(msg)) )
        }
    }
}

pub fn parse_all_call_reply<'a> (msg: &'a[u8], finder: &PropertyFinder, timestamp: EpochMillis)->Result<AdsbUpdate<'a>> {
    extract_all! { msg ?
        let capability: &str = finder.capability,
        let icao24: &str = finder.icao24 => {
            let data = AdsbData::AllCallReply{capability};
            Ok( AdsbUpdate{ timestamp, icao24, data } )
        } else {
            Err( parse_error!( "failed to parse AllCallReply message: {}", String::from_utf8_lossy(msg)) )
        }
    }
}

pub fn parse_extended_squitter_adsb<'a> (msg: &'a[u8], finder: &PropertyFinder, timestamp: EpochMillis)->Result<AdsbUpdate<'a>> {
    extract_all! { msg ?
        let icao24: &str = finder.icao24,
        let bds: u64 = finder.bds => {
            match bds as u8 {
                5 => parse_airborne_position( msg, finder, timestamp, icao24),  // Bds::AirbornePosition
                8 => parse_aircraft_identification( msg, finder, timestamp, icao24),  // Bds::AircraftIdentification
                9 => parse_airborne_velocity( msg, finder, timestamp, icao24),  // Bds::AirborneVelocity
                61 => parse_aircraft_status( msg, finder, timestamp, icao24), // Bds::AircraftStatus
                62 => parse_target_state_and_status( msg, finder, timestamp, icao24), // Bds::TargetStateAndStatusInformation
                65 => parse_aircraft_operation_status( msg, finder, timestamp, icao24), // Bds::AircraftOperationStatus
                _ => Ok(ignored(timestamp))  // Bds::ignore
            }
        } else {
            Err( parse_error!( "failed to parse ExtendedSquitterAdsb message: {}", String::from_utf8_lossy(msg)) )
        }
    }
}

// unfortunately we get some "altitude":null messages with valie latitude,longitude so we have to treat every field as optional
pub fn parse_airborne_position<'a> (msg: &'a[u8], finder: &PropertyFinder, timestamp: EpochMillis, icao24: &'a str)->Result<AdsbUpdate<'a>>  
{
    let altitude = extract_optional!( msg, finder.altitude, i64);

    // we only accept latitude/longitude in pairs
    extract_all! { msg ?
        let latitude: f64 = finder.latitude,
        let longitude: f64 = finder.longitude => {
            let position = Some( Position{latitude,longitude} );
            let data = AdsbData::AirbornePosition{ position, altitude };
            Ok( AdsbUpdate{ timestamp, icao24, data } )
        } else {
            if altitude.is_some() {
                let data = AdsbData::AirbornePosition{ position: None, altitude };
                Ok( AdsbUpdate{ timestamp, icao24, data } )
            } else {
                Ok( ignored(timestamp) )
                //Err( parse_error!( "missing position in AirbornePosition message: {}", csv.line()) )
            }
        }
    }
}

pub fn parse_aircraft_identification<'a> (msg: &'a [u8], finder: &PropertyFinder, timestamp: EpochMillis, icao24: &'a str)->Result<AdsbUpdate<'a>>  
{
    extract_all! { msg ?
        let callsign: &str = finder.callsign => {
            let data = AdsbData::AircraftIdentification{callsign};
            Ok( AdsbUpdate{ timestamp, icao24, data } )
        } else {
            Err( parse_error!( "failed to parse AircraftIdentification message: {}", String::from_utf8_lossy(msg)) )
        }
    }
}

/// extract groundspeed, track/heading and (optional) vertical_rate
/// this is sub-optimal - it is not clear if vrate is optional or a missing value indicates a malformed msg. Same goes
/// for the heading / track alternative
pub fn parse_airborne_velocity<'a> (msg: &'a [u8], finder: &PropertyFinder, timestamp: EpochMillis, icao24: &'a str)->Result<AdsbUpdate<'a>>  
{
    let groundspeed: Option<f64> = extract_optional!( msg, finder.groundspeed, f64);
    let heading: Option<f64>  = extract_optional!( msg, finder.track, f64).or_else( || extract_optional!( msg, finder.heading, f64));
    let vertical_rate: Option<i64> = extract_optional!( msg, finder.vertical_rate, i64);

    if (groundspeed.is_some() || heading.is_some() || vertical_rate.is_some()) {
        let data = AdsbData::AirborneVelocity{groundspeed,heading,vertical_rate};
        return Ok( AdsbUpdate{ timestamp, icao24, data } )
    } else {
        Err( parse_error!( "empty AirborneVelocity message: {}", String::from_utf8_lossy(msg)) )
    }
}

pub fn parse_aircraft_status<'a> (msg: &'a [u8], finder: &PropertyFinder, timestamp: EpochMillis, icao24: &'a str)->Result<AdsbUpdate<'a>>  
{
    Ok( ignored(timestamp) ) // not yet (we get emergency_type, emergency_status as &str)
}

pub fn parse_target_state_and_status<'a> (msg: &'a [u8], finder: &PropertyFinder, timestamp: EpochMillis, icao24: &'a str)->Result<AdsbUpdate<'a>>  
{
    let selected_altitude = extract_optional!( msg, finder.selected_altitude, i64);
    let selected_heading = extract_optional!( msg, finder.selected_heading, f64);

    let data = AdsbData::TargetStateAndStatus{selected_altitude,selected_heading};
    Ok( AdsbUpdate{ timestamp, icao24, data } )
}

pub fn parse_aircraft_operation_status<'a> (msg: &'a [u8], finder: &PropertyFinder, timestamp: EpochMillis, icao24: &'a str)->Result<AdsbUpdate<'a>>  
{
    Ok( ignored(timestamp) ) // not yet (has "NICa","NACp","GVA","SIL","BAI","HRD","SILs")
}

/* #region low level parsing ***************************************************************************/

/// wrapper for DateTime<Utc> - we need our own newtype here so that we can implement U8Readable
#[derive(Debug)]
pub struct Timestamp(pub DateTime<Utc>);

impl<'a> fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!( f, "{}", self.0.format("%Y-%m-%dT%H:%M:%S%.3f Z"))
    }
}

/// this reads DateTime<Utc> as fractional epoch seconds
impl<'a> U8Readable<'a,Timestamp> for Timestamp {
    fn from_u8 (buf: &'a[u8], i0: usize)->Option<(Timestamp,usize)> {
        let mut secs: i64 = 0;
        let mut frac: i64 = 0;
        let mut a: &mut i64 = &mut secs;
        let mut di = 0;

        let mut i = i0;

        loop {
            if i >= buf.len() { return None; }

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
        }
    }
}
/* #endregion low level parsing */