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

use std::fmt;

use chrono::{DateTime,Utc};

/// "downlink format" (DF) - Mode S transponder message 
/// see `rs1090::decode` (mod.rs)
#[repr(u8)]
pub enum DownlinkFormat {
    ShortAirAirSurveillance = 0, // altitude
    SurveillanceAltitudeReply = 4, // altitude
    SurveillanceIdentityReply = 5,
    AllCallReply = 11,
    LongAirAirSurveillance = 16,   
    ExtendedSquitterADSB = 17,  // <<<< ADSB payload (see bds)
    ExtendedSquitterTisB = 18, 
    ExtendedSquitterMilitary = 19, 
    CommBAltitudeReply = 20,
    CommBIdentityReply = 21,   
    CommDExtended = 24,   
}

/// "binary data store" (BDS) - ADS-B message type
/// see `rs1090::decode::bds::*`
#[repr(u8)]
pub enum Bds {
    AirbornePosition = 5,  // (barometric or satellite altitude)
    SurfacePosition = 6,
    AircraftIdentification = 8,
    AirborneVelocity = 9,
    AircraftStatus = 61,
    TargetStateAndStatusInformation = 62,
    AircraftOperationStatus = 65,
}

#[derive(Debug)]
pub struct AdsbUpdate<'a> {
    // the common fields for valid data
    pub timestamp: DateTime<Utc>,
    pub icao24: &'a str, // might be empty if data is ignored

    // the variable part
    pub data: AdsbData<'a>
}

impl<'a> fmt::Display for AdsbUpdate<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!( f, "AdsbUpdate( timestamp:{}, icao24: {}, data: {})", self.timestamp, self.icao24, self.data)
    }
}

pub fn ignored<'a> (timestamp: DateTime<Utc>)->AdsbUpdate<'a> { AdsbUpdate{timestamp, icao24: "", data: AdsbData::Ignored} }

/// the payload data for ADS-B messages
#[derive(Debug)]
pub enum AdsbData<'a> {
    SurfacePosition{ latitude: f64, longitude: f64 },
    ShortAirAirSurveillance{ altitude: i64 },
    SurveillanceAltitudeReply{ altitude: i64 },
    SurveillanceId{ callsign: &'a str },
    AllCallReply{ capability: &'a str },
    AirbornePosition{ latitude: f64, longitude: f64, altitude: Option<i64> },
    AircraftIdentification{ callsign: &'a str },
    AirborneVelocity{ groundspeed: f64, heading: f64, vertical_rate: Option<i64> },
    AirToAir{ altitude: i64 },
    TargetStateAndStatus{ selected_altitude: Option<i64>, selected_heading: Option<f64> },
    Ignored
}

impl<'a> fmt::Display for AdsbData<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            AdsbData::SurfacePosition {latitude,longitude} => { write!( f, "SurfacePosition( latitude: {}, longitude: {} )", latitude, longitude ) }
            AdsbData::ShortAirAirSurveillance {altitude} => { write!( f, "ShortAirAirSurveillance( altitude: {} )", altitude) }
            AdsbData::SurveillanceAltitudeReply {altitude} => { write!( f, "SurveillanceAltitudeReply( altitude: {} )", altitude) }
            AdsbData::SurveillanceId {callsign} => { write!( f, "SurveillanceId( callsign: {} )", callsign) }
            AdsbData::AllCallReply {capability} => { write!( f, "AllCallReply( capability: {} )", capability) }
            AdsbData::AirbornePosition {altitude,latitude,longitude} => { write!( f, "AirbornePosition( altitude: {:?}, latitude: {:?}, longitude: {:?} )", altitude,latitude,longitude)}
            AdsbData::AircraftIdentification {callsign} => { write!( f, "AircraftIdentification( callsign: {} )", callsign) }
            AdsbData::AirborneVelocity {groundspeed,heading,vertical_rate} => { write!( f, "AirborneVelocity( groundspeed: {}, heading: {}, vertical_rate: {:?} )", groundspeed,heading,vertical_rate) }
            AdsbData::AirToAir { altitude } => { write!( f, "AirToAir( altitude: {} )", altitude) }
            AdsbData::TargetStateAndStatus {selected_altitude,selected_heading} => { write!( f, "TargetStateAndStatus( selected_altitude: {:?}, selected_heading: {:?} )", selected_altitude,selected_heading) }
            AdsbData::Ignored => { write!( f, "Ignored") }
        }
    }
}
