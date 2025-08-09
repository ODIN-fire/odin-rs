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

use std::sync::{Arc,Mutex,atomic::{AtomicBool,AtomicI64,Ordering}};
use chrono::{DateTime, Utc, NaiveDate, NaiveTime, TimeZone};
use chrono_tz::Tz;
use tokio::{self,net::TcpStream, io::{BufReader, AsyncBufReadExt}};
use dashmap::DashMap;
use async_trait::async_trait;
use odin_actor::prelude::*;
use odin_common::{extract_fields, u8extractor::{CsvStr, CsvFieldExtractor, CsvExtractor, AsyncCsvExtractor}, datetime::EpochMillis};
use crate::errors::{Result, OdinAdsbError,parse_error};
use crate::{Aircraft, AircraftStore, adsb::{AdsbConnector, AdsbConfig, AdsbData, AdsbUpdate, Position, ignored}, actor::{AdsbActorMsg}};

pub struct SbsConnector {
    config: Arc<AdsbConfig>,
    timestamp: Arc<AtomicI64>,
    aircraft: Arc<DashMap<String,Aircraft>>,
    task: Option<JoinHandle<()>>,  // close to useless for blocking (native thread) tasks as we can't abort 
    keep_alive: Arc<AtomicBool> // used to signal input thread to terminate
}


#[async_trait]
impl AdsbConnector for SbsConnector {
    fn new (config: Arc<AdsbConfig>, timestamp: Arc<AtomicI64>, aircraft: Arc<DashMap<String,Aircraft>>)->Self {
        SbsConnector{ config, timestamp, aircraft, task: None, keep_alive: Arc::new(AtomicBool::new(true)) }
    }

    async fn start (&mut self, hself: ActorHandle<AdsbActorMsg>) -> Result<()> {
        let max_trace = self.config.max_trace;
        let url = self.config.url.clone();
        let aircraft = self.aircraft.clone();
        let timestamp = self.timestamp.clone();
        let keep_alive = self.keep_alive.clone();
        let tz = self.config.timezone.clone();

        let join_handle =  spawn_blocking( "sbs-task", move || { process_msgs(url, max_trace, timestamp, aircraft, keep_alive, tz); })?;
        self.task = Some(join_handle);

        Ok(())
    }

    fn terminate (&mut self) {
        if let Some(join_handle) = &self.task {
            //join_handle.abort(); // blocking tasks cannot be aborted !
            self.keep_alive.store( false, Ordering::Relaxed);
            self.task = None;
        }
    }
}

pub fn process_msgs (url: String, max_trace: usize, timestamp: Arc<AtomicI64>, aircraft: Arc<DashMap<String,Aircraft>>, keep_alive: Arc<AtomicBool>, source_tz: Tz)->Result<()> {
    let stream = std::net::TcpStream::connect( url)?;
    let mut reader = std::io::BufReader::with_capacity( 8192, stream);
    let mut csv = CsvExtractor::new(reader);

    // TODO - this works for ADS-B with frequent input but not for sources with a high temporal variation that might
    // get blocked for extended amounts of time. Those we probably have to move to regular tokio tasks
    while keep_alive.load(Ordering::Relaxed) && csv.next_line()? {
        process_next_line(&mut csv, &timestamp, &aircraft, max_trace, &source_tz)?
    }

    Ok(())
}

pub async fn async_process_msgs (url: &str, max_trace: usize, timestamp: Arc<AtomicI64>, aircraft: Arc<DashMap<String,Aircraft>>, source_tz: Tz)->Result<()> {
    let stream = TcpStream::connect( url).await?;
    let mut reader = BufReader::with_capacity( 8192, stream);
    let mut csv = AsyncCsvExtractor::new(reader);

    while csv.next_line().await? {
        process_next_line(&mut csv, &timestamp, &aircraft, max_trace, &source_tz)?
    }

    Ok(())
}

fn process_next_line<'a, T: CsvFieldExtractor> (csv: &'a mut T, timestamp: &Arc<AtomicI64>, aircraft: &Arc<DashMap<String,Aircraft>>, max_trace: usize, source_tz: &Tz)->Result<()> {
    match parse_msg( csv, source_tz) {
        Ok(update) => {
            let update_timestamp = if let Some(mut ac) = aircraft.get_mut( update.icao24) {
                update.update( &mut ac)
            } else {
                let icao24 = update.icao24.to_string();
                let mut ac = Aircraft::new( icao24.clone(), update.timestamp, max_trace);
                let update_timestamp = update.update( &mut ac);
                aircraft.insert( icao24, ac);
                update_timestamp
            };

            // note that not all updates count towards a new timestamp
            if let Some(update_timestamp) = update_timestamp {
                timestamp.store( update_timestamp.millis(), Ordering::Relaxed); 
            }
        }
        Err(e) => eprintln!("PARSE ERROR for {}: {}", csv.line(), e)
    }
    Ok(())
}

/// SBS as documented on http://woodair.net/SBS/Article/Barebones42_Socket_Data.htm
/// 
/// Message examples:
///  MSG,1,111,11111,AA2BC2,111111,2016/03/11,13:07:16.663,2016/03/11,13:07:16.626,UAL814  ,,,,,,,,,,,0
///  MSG,3,111,11111,A04424,111111,2016/03/11,13:07:05.343,2016/03/11,13:07:05.288,,11025,,,37.17274,-122.03935,,,,,,0
///  MSG,4,111,11111,AC1FCC,111111,2016/03/11,13:07:07.777,2016/03/11,13:07:07.713,,,316,106,,,1536,,,,,0
/// 
/// fields:
///   0: message type (MSG, SEL, ID, AIR, STA, CLK)
///   1: transmission type (MSG only: 1-8, 3: ES Airborne Position Message)
///   2: DB session id   - '111' for dump1090 generated SBS
///   3: DB aircraft id  - '11111' for dump1090 generated SBS
///   4: ICAO 24 bit id (mode S transponder code)
///   5: DB flight id - '111111' for dump1090 generated SBS
///   6: date generated
///   7: time generated
///   8: date logged
///   9: time logged
///  10: callsign
///  11: mode-C altitude (relative to 1013.2mb (Flight Level), *not* AMSL)
///  12: ground speed
///  13: track (from vx,vy, *not* heading)
///  14: latitude
///  15: longitude
///  16: vertical rate (ft/min - 64ft resolution)
///  17: squawk (mode-A squawk code)
///  18: alert (flag indicating squawk has changed)
///  19: emergency (flag)
///  20: spi (flag, transponder ident activated)
///  21: on ground (flag)
/// 
/// see also http://mode-s.org/decode/
pub fn parse_msg<'a,T> (csv: &'a mut T, source_tz: &Tz)->Result<AdsbUpdate<'a>> where T: CsvFieldExtractor {
    extract_fields!{ csv ?
        // the common fields
        let msg_type: u64 = [1],
        let icao24: CsvStr = [4],
        let date: CsvStr = [6],
        let time: CsvStr = [7] => {
            let timestamp = EpochMillis::from( get_utc_datetime( date.as_str(), time.as_str(), source_tz)?);

            match msg_type {
                1 => parse_aircraft_identification( csv, timestamp, *icao24),
                2 => parse_surface_position( csv, timestamp, *icao24),
                3 => parse_airborne_position( csv, timestamp, *icao24),
                4 => parse_airborne_velocity( csv, timestamp, *icao24),
                5 => parse_surveillance_alt( csv, timestamp, *icao24),
                6 => parse_surveillance_id( csv, timestamp, *icao24),
                7 => parse_air_to_air( csv, timestamp, *icao24),
                8 => parse_all_call_reply( csv, timestamp, *icao24),
                _ => Ok( ignored(timestamp) ) 
            }
        } else {
            Err( parse_error!( "missing common fields in SBS message: {}", csv.line()) )
        }
    }
}

// note that dump1090 does report time in local timezone, i.e. we have to convert to UTC
fn get_utc_datetime (date: &str, time: &str, tz: &Tz)->Result<DateTime<Utc>> {
    let date = NaiveDate::parse_from_str( date, "%Y/%m/%d")?;
    let time = NaiveTime::parse_from_str( time, "%H:%M:%S%.3f")?;
    
    let dt = match tz.from_local_datetime( &date.and_time(time)) {
        chrono::offset::LocalResult::Single(dt) => dt,
        chrono::offset::LocalResult::Ambiguous(dt1, dt2) => dt2, // we don't care about that precision
        chrono::offset::LocalResult::None => return Err( OdinAdsbError::OpFailedError("forward time jump cannot be mapped to UTC".into())),
    };
    Ok( dt.with_timezone( &Utc) )
}

fn parse_aircraft_identification<'a,T> (csv: &'a T, timestamp: EpochMillis, icao24: &'a str)-> Result<AdsbUpdate<'a>>
     where T: CsvFieldExtractor
{
    if let Some(cs) = csv.field::<CsvStr>(10) {
        let callsign: &'a str = cs.trim();

        let data = AdsbData::AircraftIdentification{callsign};
        Ok( AdsbUpdate{ timestamp, icao24, data } )
    } else { 
        Err( parse_error!( "missing callsign in AircraftIdentification message: {}", csv.line()) )
    }
}

fn parse_surface_position<'a,T> (csv: &'a T, timestamp: EpochMillis, icao24: &'a str)-> Result<AdsbUpdate<'a>>
     where T: CsvFieldExtractor
{
    extract_fields!{ csv ?
        let latitude: f64 = [14],
        let longitude: f64 = [15] => {
            let position = Position{latitude,longitude};
            let data = AdsbData::SurfacePosition{ position };
            Ok( AdsbUpdate{ timestamp, icao24, data } )
        } else {
            Ok( ignored(timestamp) ) // dump1090 only reports groundspeed, which is not very helpful
        }
    }
}

fn parse_airborne_position<'a,T> (csv: &'a T, timestamp: EpochMillis, icao24: &'a str)-> Result<AdsbUpdate<'a>>
     where T: CsvFieldExtractor
{
    let altitude = csv.field::<i64>(11);

    extract_fields!{ csv ?
        let latitude: f64 = [14],
        let longitude: f64 = [15] => {
            let position = Some(Position{latitude, longitude});
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

fn parse_airborne_velocity<'a,T> (csv: &'a T, timestamp: EpochMillis, icao24: &'a str)-> Result<AdsbUpdate<'a>>
     where T: CsvFieldExtractor
{
    let groundspeed: Option<f64> = csv.field(12);
    let heading: Option<f64> = csv.field(13);
    let vertical_rate: Option<i64> = csv.field(16);

    if (groundspeed.is_some() || heading.is_some() || vertical_rate.is_some()) {
        let data = AdsbData::AirborneVelocity{ groundspeed, heading, vertical_rate };
        Ok( AdsbUpdate{ timestamp, icao24, data } )
    } else {
        Err( parse_error!( "empty AirborneVelocity message: {}", csv.line()) )
    }
}

fn parse_surveillance_alt<'a,T> (csv: &'a T, timestamp: EpochMillis, icao24: &'a str)-> Result<AdsbUpdate<'a>>
     where T: CsvFieldExtractor
{
    if let Some(altitude) = csv.field::<i64>(11) {
        let data = AdsbData::SurveillanceAltitudeReply{altitude};
        Ok( AdsbUpdate{ timestamp, icao24, data } )
    } else { 
        Ok( ignored(timestamp) )
        //Err( parse_error!( "missing altitude in SurveillanceAltitudeReply message: {}", csv.line()) )
    }
}

fn parse_surveillance_id<'a,T> (csv: &'a T, timestamp: EpochMillis, icao24: &'a str)-> Result<AdsbUpdate<'a>>
     where T: CsvFieldExtractor
{
    if let Some(cs) = csv.field::<CsvStr>(10) {
        let callsign: &'a str = *cs;
        let data = AdsbData::SurveillanceId{callsign};
        Ok( AdsbUpdate{ timestamp, icao24, data } )
    } else { 
        Ok( ignored(timestamp) ) // dump1090 only reports groundspeed, which is not very helpful
    }
}

fn parse_air_to_air<'a,T> (csv: &'a T, timestamp: EpochMillis, icao24: &'a str)-> Result<AdsbUpdate<'a>>
     where T: CsvFieldExtractor
{
    if let Some(altitude) = csv.field::<i64>(11) {
        let data = AdsbData::AirToAir{altitude};
        Ok( AdsbUpdate{ timestamp, icao24, data } )
    } else { 
        Ok( ignored(timestamp) )
        //Err( parse_error!( "missing altitude in AirToAir message: {}", csv.line()) )
    }
}

fn parse_all_call_reply<'a,T> (csv: &'a T, timestamp: EpochMillis, icao24: &'a str)-> Result<AdsbUpdate<'a>>
     where T: CsvFieldExtractor
{
    Ok( ignored(timestamp) ) // dump1090 only reports onground flag (differs from jet1090)
}