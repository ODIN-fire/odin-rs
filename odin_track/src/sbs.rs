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

use std::{sync::{Arc,RwLock},collections::HashMap};
use chrono::{DateTime, Utc, NaiveDate, NaiveTime, TimeZone};
use tokio::{self,net::TcpStream, io::{BufReader,AsyncBufReadExt}};
use odin_common::{extract_fields, u8extractor::{CsvStr,CsvFieldExtractor,AsyncCsvExtractor}};
use crate::errors::{Result,OdinTrackError,parse_error};
use crate::{Aircraft,adsb::{AdsbData, AdsbUpdate, ignored}};

pub async fn process_msgs<F: FnMut(&Aircraft)> (url: &str, aircraft: Arc<RwLock<HashMap<String,Aircraft>>>, f: F)->Result<()> {
    let stream = TcpStream::connect( url).await?;
    let mut reader = BufReader::with_capacity( 8192, stream);
    let mut csv = AsyncCsvExtractor::new(reader);

    while csv.next_line().await? {
        match parse_msg( &mut csv) {
            Ok(update) => {
                // TBD
            }
            Err(e) => eprintln!("PARSE ERROR for {}: {}", csv.line(), e)
        }
    }

    Ok(())
}

/// see http://woodair.net/sbs/article/Barebones42_Socket_Data.htm for SBS BaseStation socket data format
/// (which is also used by dump1090)
pub fn parse_msg<'a,T> (csv: &'a mut T)->Result<AdsbUpdate<'a>> where T: CsvFieldExtractor {
    extract_fields!{ csv ?
        // the common fields
        let msg_type: u64 = [1],
        let icao24: CsvStr = [4],
        let date: CsvStr = [6],
        let time: CsvStr = [7] => {
            let timestamp = get_utc_datetime( date.as_str(), time.as_str())?;

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

fn get_utc_datetime (date: &str, time: &str)->Result<DateTime<Utc>> {
    let date = NaiveDate::parse_from_str( date, "%Y/%m/%d")?;
    let time = NaiveTime::parse_from_str( time, "%H:%M:%S%.3f")?;
    Ok( Utc.from_utc_datetime( &date.and_time(time)) )
}

fn parse_aircraft_identification<'a,T> (csv: &'a T, timestamp: DateTime<Utc>, icao24: &'a str)-> Result<AdsbUpdate<'a>>
     where T: CsvFieldExtractor
{
    if let Some(cs) = csv.field::<CsvStr>(10) {
        let callsign: &'a str = *cs;
        let data = AdsbData::AircraftIdentification{callsign};
        Ok( AdsbUpdate{ timestamp, icao24, data } )
    } else { 
        Err( parse_error!( "missing callsign in AircraftIdentification message: {}", csv.line()) )
    }
}

fn parse_surface_position<'a,T> (csv: &'a T, timestamp: DateTime<Utc>, icao24: &'a str)-> Result<AdsbUpdate<'a>>
     where T: CsvFieldExtractor
{
    extract_fields!{ csv ?
        let latitude: f64 = [14],
        let longitude: f64 = [15] => {
            let data = AdsbData::SurfacePosition{ latitude, longitude };
            Ok( AdsbUpdate{ timestamp, icao24, data } )
        } else {
            Ok( ignored(timestamp) ) // dump1090 only reports groundspeed, which is not very helpful
        }
    }
}

fn parse_airborne_position<'a,T> (csv: &'a T, timestamp: DateTime<Utc>, icao24: &'a str)-> Result<AdsbUpdate<'a>>
     where T: CsvFieldExtractor
{
    extract_fields!{ csv ?
        let latitude: f64 = [14],
        let longitude: f64 = [15] => {
            let altitude = csv.field::<i64>(11);
            let data = AdsbData::AirbornePosition{ latitude, longitude, altitude };
            Ok( AdsbUpdate{ timestamp, icao24, data } )
        } else {
            Err( parse_error!( "missing position in AirbornePosition message: {}", csv.line()) )
        }
    }
}

fn parse_airborne_velocity<'a,T> (csv: &'a T, timestamp: DateTime<Utc>, icao24: &'a str)-> Result<AdsbUpdate<'a>>
     where T: CsvFieldExtractor
{
    extract_fields!{ csv ?
        let groundspeed: f64 = [12],
        let heading: f64 = [13] => { // that is not entirely true as this is 'track', i.e. computed from e/w and n/s velocities
            let vertical_rate = csv.field::<i64>(16);
            let data = AdsbData::AirborneVelocity{ groundspeed, heading, vertical_rate };
            Ok( AdsbUpdate{ timestamp, icao24, data } )
        } else {
           Err( parse_error!( "missing speed/track in AirborneVelocity message: {}", csv.line()) )
        }
    }
}

fn parse_surveillance_alt<'a,T> (csv: &'a T, timestamp: DateTime<Utc>, icao24: &'a str)-> Result<AdsbUpdate<'a>>
     where T: CsvFieldExtractor
{
    if let Some(altitude) = csv.field::<i64>(11) {
        let data = AdsbData::SurveillanceAltitudeReply{altitude};
        Ok( AdsbUpdate{ timestamp, icao24, data } )
    } else { 
        Err( parse_error!( "missing altitude in SurveillanceAltitudeReply message: {}", csv.line()) )
    }
}

fn parse_surveillance_id<'a,T> (csv: &'a T, timestamp: DateTime<Utc>, icao24: &'a str)-> Result<AdsbUpdate<'a>>
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

fn parse_air_to_air<'a,T> (csv: &'a T, timestamp: DateTime<Utc>, icao24: &'a str)-> Result<AdsbUpdate<'a>>
     where T: CsvFieldExtractor
{
    if let Some(altitude) = csv.field::<i64>(11) {
        let data = AdsbData::AirToAir{altitude};
        Ok( AdsbUpdate{ timestamp, icao24, data } )
    } else { 
        Err( parse_error!( "missing altitude in AirToAir message: {}", csv.line()) )
    }
}

fn parse_all_call_reply<'a,T> (csv: &'a T, timestamp: DateTime<Utc>, icao24: &'a str)-> Result<AdsbUpdate<'a>>
     where T: CsvFieldExtractor
{
    Ok( ignored(timestamp) ) // dump1090 only reports onground flag (differs from jet1090)
}