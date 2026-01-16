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

use std::{collections::VecDeque, fmt, sync::{atomic::{AtomicI64,Ordering}, Arc, Mutex}, time::Duration};
use uom::si::{length::{meter,foot}, velocity::{knot,foot_per_second},f64::{Length,Velocity}};
use dashmap::DashMap; // papaya or whirlwind can be an async alternatives (once whirlwind stabilizes)
use chrono::{DateTime,Utc};
use odin_build::{define_load_config,define_load_asset};
use odin_common::{
    angle::Angle360, cartesian3::Cartesian3, cartographic::Cartographic, collections::RingDeque,
    datetime::{self,EpochMillis}, geo::GeoPoint4,
    json_writer::{JsonWritable,JsonWriter, NumFormat}
};
use odin_server::{ws_service::ws_msg_from_json, spa::SpaService, errors::OdinServerResult};
use memchr;

pub mod adsb;
use adsb::Position;

use crate::errors::{OdinAdsbError,Result};

pub mod actor;

pub mod rs1090;
pub mod sbs;

pub mod adsb_service;
use adsb_service::AdsbService;

pub mod errors;

define_load_asset!{}
define_load_config!{}

/// the data we need to publish, plus the state that is required to determine what has to be published as update
/// Note that both `aircraft` and `timestamp` are shared between actor (reader) and connector (writer) and hence need
/// synced types
pub struct AircraftStore {
    source: String,
    last_update: EpochMillis, // the last published timestamp
    timestamp: Arc<AtomicI64>, // shared with and updated by connector
    aircraft: Arc<DashMap<String,Aircraft>>, // shared with and updated by connector
    dropped_list: Vec<Arc<String>>, // list of aircraft removed in last update cycle
}

impl AircraftStore {
    pub fn new (source: String)->Self {
        AircraftStore {
            source,
            last_update: EpochMillis::new(0),
            timestamp: Arc::new( AtomicI64::new(0)), // updated by connector
            aircraft: Arc::new( DashMap::new()), // updated by connector
            dropped_list: Vec::new(),
        }
    }

    // the external accessors
    pub fn source (&self)->&str { self.source.as_str() }
    pub fn timestamp (&self)->EpochMillis { EpochMillis::new( self.timestamp.load(Ordering::Relaxed)) }
    pub fn aircraft (&self)->&DashMap<String,Aircraft> { self.aircraft.as_ref() }

    pub fn dropped_list (&self)->&[Arc<String>] { self.dropped_list.as_slice() }
    pub fn clear_dropped_list (&mut self) { self.dropped_list.clear(); }

    pub fn set_dropped (&mut self, drop_after: Duration)->usize {
        let now = datetime::utc_now().timestamp_millis();
        let max_age = drop_after.as_millis() as i64;

        self.dropped_list.clear();
        for e in self.aircraft.iter() {
            let ac = e.value();
            let dt = now - ac.last_update.millis();
            if dt > max_age {
                self.dropped_list.push( ac.icao24.clone())
            }
        }

        self.dropped_list.len()
    }

    pub fn remove_stale (&mut self, drop_after: Duration)->usize {
        let n_dropped = self.set_dropped( drop_after);
        if n_dropped > 0 {
            for icao24 in &self.dropped_list {
                self.aircraft.remove(icao24.as_str());
            }
        }
        n_dropped
    }

    pub fn write_json_update_to (&self, w: &mut JsonWriter) {
        w.clear();

        w.write_object( |w| {
            w.write_field("source", self.source.as_str());
            w.write_array_field("updated", |w| {
                for e in self.aircraft.iter() {
                    let ac = e.value();
                    if let Some(p) = ac.last_position() {
                        if p.date > self.last_update {
                            ac.write_to( w, true);
                        }
                    }
                }
            });

            if (!self.dropped_list.is_empty()) {
                w.write_array_field("removed", |w| {
                    for icao24 in &self.dropped_list {
                        w.write_value(icao24.as_str());
                    }
                })
            }
        });
    }

    /// this happens frequently so we pass in a cached writer to avoid allocation
    pub fn get_json_update_msg (&self, w: &mut JsonWriter)->String {
        self.write_json_update_to( w);
        ws_msg_from_json( AdsbService::mod_path(), "update", w.as_str())
    }

    pub fn write_json_snapshot_to (&self, w: &mut JsonWriter) {
        w.clear();

        w.write_object( |w| {
            w.write_field("source", self.source.as_str());
            w.write_array_field("aircraft", |w| {
                for e in self.aircraft.iter() {
                    let ac = e.value();
                    if let Some(p) = ac.last_position() {
                        ac.write_to( w, false);
                    }
                }
            });
        });
    }

    /// this happens infrequently from a dyn action so we don't cache the writer (but for that save the clone)
    pub fn get_json_snapshot_msg (&self)->String {
        let mut w = JsonWriter::with_capacity(8192);
        self.write_json_snapshot_to(&mut w);
        ws_msg_from_json( AdsbService::mod_path(), "snapshot", w.as_str())
    }

    fn set_last_update (&mut self, last_update: EpochMillis) {
        self.last_update = last_update;
    }
}

/// the data model for a tracked aircraft
#[derive(Debug)]
pub struct Aircraft {
    pub icao24: Arc<String>, // we keep that in an Arc so that we can clone without heap allocation
    pub callsign: Option<String>,

    pub positions: VecDeque<GeoPoint4>, // used as a ringbuffer to keep trace

    pub groundspeed: Option<Velocity>,
    pub vertical_rate: Option<Velocity>,
    pub hdg: Option<Angle360>,
    pub altitude: Option<Length>, // ADS-B has messages that only contain altitude (without position)

    pub sel_hdg: Option<Angle360>,
    pub sel_alt: Option<Length>,

    pub last_update:  EpochMillis,
    //... and possibly more to follow
}

impl<'a> fmt::Display for Aircraft {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!( f, "Aircraft( icao24: {}", self.icao24);
        if let Some(cs) = &self.callsign { write!( f, ", cs: \"{cs}\""); }
        if let Some(p) = self.last_position() { write!( f, ", pos: {}", p.location); }
        if self.positions.len() > 0 { write!( f, ", n_pos: {}", self.positions.len()); }
        if let Some(gs) = self.groundspeed { write!( f, ", spd: {:.3}", gs.get::<knot>()); }
        if let Some(vr) = self.vertical_rate { write!( f, ", vr: {:.1}", vr.get::<foot_per_second>()); }
        if let Some(hdg) = self.hdg { write!( f, ", hdg: {:.0}", hdg.degrees()); }
        if let Some(alt) = self.altitude { write!( f, ", alt: {:.0}", alt.get::<foot>()); }
        if let Some(hdg) = self.sel_hdg { write!( f, ", sel_hdg: {:.0}", hdg.degrees()); }
        if let Some(alt) = self.sel_alt { write!( f, ", sel_alt: {:.0}", alt.get::<foot>()); }
        write!( f, ", time: {}", self.last_update);
        write!(f, ")");
        Ok(())
    }
}

impl Aircraft {
    pub fn new (icao24: String, last_update: EpochMillis, max_pos: usize)->Self {
        Aircraft {
            icao24: Arc::new(icao24),
            callsign: None,
            positions: VecDeque::with_capacity(max_pos),
            groundspeed: None,
            vertical_rate: None,
            hdg: None,
            altitude: None,
            sel_hdg: None,
            sel_alt: None,
            last_update
        }
    }

    // use this to find out if we should report
    pub fn last_position (&self)->Option<&GeoPoint4> { self.positions.back() }

    // note that ADS-B reports altitude in ft
    pub fn push_position (&mut self, pos: &Position, altitude: &Option<i64>, timestamp: EpochMillis) {
        let ts_millis = timestamp.millis();

        let alt_m = if let Some(alt) = altitude {
            let alt = Length::new::<foot>( (*alt) as f64);
            alt.get::<meter>()
        } else {
            if let Some(alt) = self.altitude { alt.get::<meter>() } else { 0.0 } // TODO - should we really use 0.0 as default or just skip
        };

        let p4 = GeoPoint4::from_lon_lat_degrees_alt_meters_epoch_millis( pos.longitude, pos.latitude, alt_m, ts_millis);

        if let Some(p_last) = self.positions.back() {
            let dt = ts_millis - p_last.date.millis();
            if  dt < 800 { // replace the previous last position, don't fill up ringbuffer with (almost) duplicates
                self.positions.pop_back();

            } else if dt < 2000 {
                // TODO - should we set actual hdg here if last_pos was recent? It would be overwritten by airborne_velocity messages
                self.hdg = Some(p4.location.bearing_from( &p_last.location));
            }
        }
        self.positions.push_to_ringbuffer(p4);
    }

    /// this serializes an Aircraft object into the JSON format processed by the odin_adsb.js JS module
    fn write_to (&self, w: &mut JsonWriter, is_update: bool) {
        w.write_object( |w| {
            w.write_field("icao24", self.icao24.as_str());
            if let Some(cs) = &self.callsign { w.write_field("callsign", cs.as_str()); }

            if let Some(p) = self.last_position() {
                w.write_field("date", p.epoch_millis().millis()); // if there are positions use the last position timestamp

                if is_update {
                    w.write_object_field( "position", |w| { write_ecef_fields_to( w, p); }); // should we add lon/lat/height ?
                } else {
                    w.write_array_field( "trace", |w| {
                        for i in 0..self.positions.len() {
                            let p = &self.positions[i];
                            w.write_object( |w| { write_ecef_fields_to( w, p); });
                        }
                    });
                }
            } else {
                w.write_field("date", self.last_update.millis()); // without positions we use the last update
            }

            if let Some(alt) = self.altitude { w.write_f64_field("alt", alt.get::<foot>(), NumFormat::Fp0); }
            if let Some(hdg) = self.hdg { w.write_f64_field("hdg", hdg.degrees(), NumFormat::Fp0); }
            if let Some(spd) = self.groundspeed { w.write_f64_field("spd", spd.get::<knot>(), NumFormat::Fp3); }
            if let Some(vrate) = self.vertical_rate { w.write_f64_field("vrate", vrate.get::<foot_per_second>(), NumFormat::Fp1); }
            //... possibly more to follow
        });
    }
}

fn write_ecef_fields_to (w: &mut JsonWriter, p: &GeoPoint4) {
    let p = p.location.to_cartesian3();
    w.write_f64_field("x", p.x, NumFormat::Fp0);
    w.write_f64_field("y", p.y, NumFormat::Fp0);
    w.write_f64_field("z", p.z, NumFormat::Fp0);
}
