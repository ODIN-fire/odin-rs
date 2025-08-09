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

use std::{fmt, sync::{Arc,atomic::AtomicI64}, time::Duration};
use chrono::{DateTime,Utc};
use chrono_tz::Tz;
use serde::{Serialize,Deserialize};
use async_trait::async_trait;
use dashmap::DashMap;
use uom::{si::{f64::{Length, Velocity}, length::{foot, meter}, velocity::{foot_per_minute, knot}}, ConstZero};
use odin_common::{angle::Angle360, collections::RingDeque, datetime::EpochMillis, geo::GeoPoint4};
use odin_actor::prelude::*;

use crate::{actor::AdsbActorMsg, errors::Result, Aircraft, AircraftStore};

#[derive(Deserialize,Serialize,Debug)]
pub struct AdsbConfig {
    pub source: String, // the receiver station name 
    pub timezone: Tz, // timezone for receiver station (used to convert local SBS times)
    pub url: String, // of the socket from which to read ADS-B data
    pub update_interval: Duration, // interval in which we send out aircraft changes
    pub max_trace: usize, // max number of trace (last trajectory) points to keep
    pub drop_after: Duration, // duration after which un-changed aircraft will be dropped
    // and more to follow 
}

#[async_trait]
pub trait AdsbConnector {
    fn new (config: Arc<AdsbConfig>, timestamp: Arc<AtomicI64>, aircraft: Arc<DashMap<String,Aircraft>>)->Self;
    async fn start (&mut self, hself: ActorHandle<AdsbActorMsg>) -> Result<()>;
    fn terminate (&mut self);
}

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
    pub timestamp: EpochMillis,
    pub icao24: &'a str, // might be empty if data is ignored

    // the variable part
    pub data: AdsbData<'a>
}

impl <'a> AdsbUpdate<'a> {

    // return (optional) timestamp that should be recorded
    pub fn update (&self, ac: &mut Aircraft)->Option<EpochMillis> {
        ac.last_update = self.timestamp; // note this might not be a position

        self.data.update( ac, self.timestamp)
    }
}

impl<'a> fmt::Display for AdsbUpdate<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!( f, "AdsbUpdate( timestamp:{}, icao24: {}, data: {})", self.timestamp, self.icao24, self.data)
    }
}

pub fn ignored<'a> (timestamp: EpochMillis)->AdsbUpdate<'a> { AdsbUpdate{timestamp, icao24: "", data: AdsbData::Ignored} }

#[derive(Debug)]
pub struct Position { pub latitude: f64, pub longitude: f64 }

/// the payload data for ADS-B messages
#[derive(Debug)]
pub enum AdsbData<'a> {
    SurfacePosition{ position: Position },
    ShortAirAirSurveillance{ altitude: i64 },
    SurveillanceAltitudeReply{ altitude: i64 },
    SurveillanceId{ callsign: &'a str },
    AllCallReply{ capability: &'a str },
    AirbornePosition{ position: Option<Position>, altitude: Option<i64> },
    AircraftIdentification{ callsign: &'a str },
    AirborneVelocity{ groundspeed: Option<f64>, heading: Option<f64>, vertical_rate: Option<i64> },
    AirToAir{ altitude: i64 },
    TargetStateAndStatus{ selected_altitude: Option<i64>, selected_heading: Option<f64> },
    Ignored
}

impl<'a> AdsbData<'a> {

    // return timestamp that should be recorded as last update for the store
    pub fn update (&self, ac: &mut Aircraft, timestamp: EpochMillis)->Option<EpochMillis> {
        ac.last_update = timestamp;
        match self {
            AdsbData::SurfacePosition {position} => { 
                ac.push_position( position, &Some(0), timestamp);
                ac.altitude = Some( Length::ZERO);
                Some(timestamp) 
            }
            AdsbData::ShortAirAirSurveillance {altitude} => { ac.altitude = Some(Length::new::<foot>( (*altitude) as f64)); None }
            AdsbData::SurveillanceAltitudeReply {altitude} => { ac.altitude = Some(Length::new::<foot>( (*altitude) as f64)); None }
            AdsbData::SurveillanceId {callsign} => { ac.callsign = Some(callsign.to_string()); None }
            AdsbData::AllCallReply {capability} => { None }
            AdsbData::AirbornePosition {position,altitude} => { 
                if let Some(pos) = position { ac.push_position( pos, altitude, timestamp) }
                if let Some(alt) = altitude { ac.altitude = Some(Length::new::<foot>( (*alt) as f64)) }
                Some(timestamp)
            }
            AdsbData::AircraftIdentification {callsign} => { ac.callsign = Some(callsign.to_string()); None }
            AdsbData::AirborneVelocity {groundspeed,heading,vertical_rate} => { 
                if let Some(gs) = groundspeed { ac.groundspeed = Some( Velocity::new::<knot>( *gs) ); }
                if let Some(hdg) = heading { ac.hdg = Some(Angle360::from_degrees( *hdg)); }
                if let Some(vrate) = vertical_rate { ac.vertical_rate = Some(Velocity::new::<foot_per_minute>( (*vrate) as f64 )); }
                None
            }
            AdsbData::AirToAir { altitude } => { ac.altitude = Some( Length::new::<foot>( (*altitude) as f64)); None }
            AdsbData::TargetStateAndStatus { selected_altitude, selected_heading } => { 
                if let Some(alt) = selected_altitude { ac.sel_alt = Some( Length::new::<foot>( (*alt) as f64) ); }
                if let Some(hdg) = selected_heading { ac.sel_hdg = Some( Angle360::from_degrees( *hdg) ); }
                None
            }
            AdsbData::Ignored => { None }
        }
    }
}

impl<'a> fmt::Display for AdsbData<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AdsbData::SurfacePosition {position} => { write!( f, "SurfacePosition( position: {:?} )", position ) }
            AdsbData::ShortAirAirSurveillance {altitude} => { write!( f, "ShortAirAirSurveillance( altitude: {} )", altitude) }
            AdsbData::SurveillanceAltitudeReply {altitude} => { write!( f, "SurveillanceAltitudeReply( altitude: {} )", altitude) }
            AdsbData::SurveillanceId {callsign} => { write!( f, "SurveillanceId( callsign: {} )", callsign) }
            AdsbData::AllCallReply {capability} => { write!( f, "AllCallReply( capability: {} )", capability) }
            AdsbData::AirbornePosition {position,altitude} => { write!( f, "AirbornePosition( position: {:?}, altitude: {:?} )", position, altitude)}
            AdsbData::AircraftIdentification {callsign} => { write!( f, "AircraftIdentification( callsign: {} )", callsign) }
            AdsbData::AirborneVelocity {groundspeed,heading,vertical_rate} => { write!( f, "AirborneVelocity( groundspeed: {:?}, heading: {:?}, vertical_rate: {:?} )", groundspeed,heading,vertical_rate) }
            AdsbData::AirToAir { altitude } => { write!( f, "AirToAir( altitude: {} )", altitude) }
            AdsbData::TargetStateAndStatus {selected_altitude,selected_heading} => { write!( f, "TargetStateAndStatus( selected_altitude: {:?}, selected_heading: {:?} )", selected_altitude,selected_heading) }
            AdsbData::Ignored => { write!( f, "Ignored") }
        }
    }
}

