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

use std::{collections::VecDeque, fmt, time::{Duration as StdDuration,SystemTime}, path::{Path,PathBuf}, fs, sync::Arc};
use nalgebra::{ViewStorage,base::{Matrix,ArrayStorage,dimension::{Const,Dyn}}};
use chrono::{DateTime,Utc,TimeZone,Datelike,Timelike};
use satkit::{self,Instant,Duration,frametransform::qteme2itrf};
use static_init::constructor;
use serde::{Deserialize,Serialize};
use async_trait::async_trait;
use uom::si::{
    f64::{Length,ThermodynamicTemperature,Power},thermodynamic_temperature::kelvin, length::meter, power::megawatt, 
};
use bit_set::BitSet;
use lazy_static::lazy_static;

use odin_build::{define_load_config,define_load_asset, pkg_cache_dir};
use odin_common::{
    angle::{ser_rounded5_angle, ser_rounded_angle, Angle180, Angle90, Latitude, Longitude}, 
    cartesian3::{ser_rounded_cartesian3, Cartesian3}, 
    cartographic::Cartographic, collections::empty_vec, 
    datetime::{self, de_from_epoch_millis, ser_epoch_millis, days}, 
    fs::{ensure_writable_dir, get_modified_timestamp, set_modified_timestamp, set_filepath_contents}, 
    geo::{GeoPoint,GeoPolygon}, 
    json_writer::{JsonWritable, JsonWriter}, 
};
use odin_macro::public_struct;

lazy_static! {
    pub static ref PKG_CACHE_DIR: PathBuf = pkg_cache_dir!(); 
}

pub mod errors;
use errors::{OdinOrbitalError,Result,op_failed};

pub mod orbitinfo;
pub use orbitinfo::OrbitInfo;

pub mod overpass;
pub use overpass::Overpass;

pub mod tle_store;
pub use tle_store::TleStore;

pub mod firms;
use firms::ViirsHotspotImporter;

pub mod actor;
pub use actor::OrbitalHotspotActor;

pub mod hotspot_service;
pub use hotspot_service::OrbitalHotspotService;


define_load_config!{}
define_load_asset!{}

/// the general information about an orbital satellite
/// this includes satellite identification, satellite sensor and overpass/data constraints
#[derive(Debug,Clone,Serialize,Deserialize)]
#[public_struct]
pub struct OrbitalSatelliteInfo {
    sat_id: u32,
    name: String,

    instrument: String,
    max_scan_angle: Angle90,  

    /// average orbital height - used to determine z-bounds for given regions
    avg_height: Length, 

    /// average swath width (single side)
    avg_swath_width: Length,

    avg_orbit_duration: StdDuration,

    /// time step for propagating orbits
    time_step: StdDuration,

    /// number of past days we initially compute overpasses and retrieve data for
    /// this is also used as the basis for the max file age (after which we drop cache entries)
    back_days: usize,

    // number of upcoming days we compute overpasses for 
    forward_days: usize,

    /// max number of completed overpasses to keep
    max_completed: usize,  

    /// max number of future overpasses to compute
    max_upcoming: usize,

    /// max number of TLEs to keep
    max_tles: usize,
}

impl OrbitalSatelliteInfo {
    pub fn step_dur (&self)->Duration { Duration::from_seconds( self.time_step.as_secs_f64()) }

    pub fn write_basic_json_to (&self, w: &mut JsonWriter) {
        w.write_object(|w| {
            w.write_field("satId", self.sat_id);
            w.write_field("name", &self.name);
            w.write_field("instrument", &self.instrument);
            w.write_field( "maxScanAngle", self.max_scan_angle.degrees());
            w.write_field("avgHeight", self.avg_height.get::<meter>());
            w.write_field( "avgSwathWidth", self.avg_swath_width.get::<meter>());
            w.write_field( "avgOrbitDuration", self.avg_orbit_duration.as_secs_f64() / 60.0)
        });
    }

    pub fn from_filenames (fnames: &Vec<&str>)->Result<Vec<Arc<OrbitalSatelliteInfo>>> {
        let mut sat_infos: Vec<Arc<OrbitalSatelliteInfo>> =Vec::with_capacity(fnames.len());

        for fname in fnames {
            let sat_info: OrbitalSatelliteInfo = load_config(fname)?;
            sat_infos.push( Arc::new(sat_info));
        }

        Ok( sat_infos )
    }
}

/// general confidence categories for hotspots
#[derive(Debug,Clone,Copy,Serialize,Deserialize)]
pub enum HotspotConfidence {
    Low, Medium, High
}
impl HotspotConfidence {
    pub fn index(&self) -> usize {
        *self as usize
    }
}

/// abstraction of hotspots measured by different instruments/satellites
/// we need this so that we don't have to use generic hotspot types (or associated item types in containers), which would 
/// either restrict us to homogenous instruments (with redundant JS modules) or to using lots of trait objects
/// - which would still impose an abstraction unless we resort to the use of Any and downcasting
/// Preserving the raw input type should best be left to the specific importers
/// Note that we keep some redundant information here in order to save re-computation on the client side 
#[derive(Debug,Serialize,Deserialize)]
#[public_struct]
struct Hotspot {
    pos: Cartesian3, // of hotspot, in ECEF
    lon: Longitude,  // geodetic hotspot coords
    lat: Latitude,
    area: [Cartesian3;4], // footprint of pixel on ellipsoid surface

    scan: Length,    // cross-scan length of pixel footprint in meters
    track: Length,   // along-track length of pixel footprint in meters
    rot: Angle180,   // rotation angle of pixel rect counting clockwise from north (bearing from pos to nearest overpass ground point)
    dist: Length,    // great-circle dist of hotspot from closest ground point 

    date: DateTime<Utc>,    
    conf: Option<HotspotConfidence>,

    //--- optionals (depending on instrument)
    temp: Option<ThermodynamicTemperature>,
    frp: Option<Power>,
}

impl JsonWritable for Hotspot {
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object( |w| {
            w.write_field_with("pos", |w| self.pos.write_json_to(w));
            w.write_fmt_field("lon", &format!("{:.5}", self.lon.degrees() ));
            w.write_fmt_field("lat", &format!("{:.5}", self.lat.degrees() ));

            w.write_array_field("area", |w| {
                self.area[0].write_json_to(w);
                self.area[1].write_json_to(w);
                self.area[2].write_json_to(w);
                self.area[3].write_json_to(w);            
            });

            w.write_field("scan", self.scan.get::<meter>().round() as i64);
            w.write_field("track", self.track.get::<meter>().round() as i64);
            w.write_field("rot", self.rot.degrees().round() as i64);
            w.write_field("dist", self.dist.get::<meter>().round() as i64);

            w.write_field("date", self.date.timestamp_millis());
            if let Some(conf) = self.conf { w.write_field("conf", conf.index() as u64) }

            if let Some(temp) = self.temp { w.write_field("temp", temp.get::<kelvin>().round() as i64) }
            if let Some(frp) = self.frp { w.write_field("frp", frp.get::<megawatt>().round() as i64) }
        });
    }
}

impl fmt::Display for Hotspot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = &self.date;
        write!( f, "{{ lon: {:.5}, lat: {:.5}", self.lon.degrees(), self.lat.degrees())?;
        write!( f, ", scan: {:.0} m, track: {:.0} m, rot: {:.0}, dist: {:.0}", 
                 self.scan.get::<meter>(), self.track.get::<meter>(), self.rot.degrees(), self.dist.get::<meter>())?;
        write!( f, ", date: {:04}-{:02}-{:02}T{:02}:{:02}", d.year(), d.month(), d.day(), d.hour(), d.minute())?;
        if let Some(conf) = self.conf {
            write!( f, ", conf: {:?}", conf)?;
        }
        if let Some(temp) = self.temp {
            write!( f, ", temp: {:.0} K", temp.get::<kelvin>())?;
        }
        if let Some(frp) = self.frp {
            write!( f, ", frp: {:.2} MW", frp.get::<megawatt>())?;
        }
        write!( f, " }}")
    }
}


/// overpass data containing hotspots 
/// note this structure can be used as a trampoline - if hotspots are empty the full data can be obtained from fname
#[derive(Debug,Serialize,Deserialize)]
#[public_struct]
pub struct HotspotList {
    sat_id: u32,
    start: DateTime<Utc>,
    end: DateTime<Utc>,

    high: usize,
    nominal: usize,
    low: usize,

    fname: String, // to retrieve the full hotspots if collapsed (hotspots are empty)
    hotspots: Vec<Hotspot>
}

impl HotspotList {
    pub fn new (op: &Overpass, hotspots: Vec<Hotspot>)->Self {
        let start = &op.start;
        let fname = format!("{}_{:4}-{:02}-{:02}_{:02}{:02}_{}_hotspots.json", 
                            op.sat_id, start.year(), start.month(), start.day(), start.hour(), start.minute(), (op.end-start).num_minutes());

        let mut high = 0;
        let mut nominal = 0;
        let mut low = 0;
        for h in &hotspots {
            match h.conf {
                Some(HotspotConfidence::High) => high += 1,
                Some(HotspotConfidence::Medium) => nominal += 1,
                Some(HotspotConfidence::Low) => low += 1,
                None => {}
            }
        }

        HotspotList { 
            sat_id: op.sat_id, 
            start: op.start,
            end: op.end,
            high, nominal, low,
            fname,
            hotspots
        }
    } 

    pub fn save_to (&self, dir: impl AsRef<Path>)->Result<()> {
        set_filepath_contents( dir, &self.fname, self.to_full_json().as_bytes())?;
        Ok(())
    }

    pub fn len (&self)->usize {
        self.hotspots.len()
    }

    pub fn collapse (&mut self) {
        self.hotspots = empty_vec();
    }

    pub fn is_collapsed (&self)->bool {
        self.hotspots.is_empty()
    }

    fn write_common_json_fields_to (&self, w: &mut JsonWriter) {
        w.write_field( "satId", self.sat_id);
        w.write_field( "start", self.start.timestamp_millis());
        w.write_field( "end", self.end.timestamp_millis());
        w.write_field( "high", self.high as u64);
        w.write_field( "nominal", self.nominal as u64);
        w.write_field( "low", self.low as u64);
        w.write_field( "fname", &self.fname);
    }

    pub fn write_collapsed_json_to (&self, w: &mut JsonWriter) {
        w.write_object( |w| self.write_common_json_fields_to(w));
    }

    pub fn to_collapsed_json (&self)->String {
        let mut w = JsonWriter::with_capacity(128);
        w.write_object( |w| self.write_common_json_fields_to(w));
        w.to_string()
    }

    // note this is still lossy as it formats/rounds values. Use serde_json::to_string() if required to be loss-less
    pub fn to_full_json (&self)->String {
        let mut w = JsonWriter::with_capacity(128 + self.len() * 128);
        w.write_object( |w|{
            self.write_common_json_fields_to(w);
            w.write_field_with( "hotspots", |w| self.hotspots.write_json_to(w));
        });
        w.to_string()
    }

    pub fn to_collapsed_json_array (hs: &Vec<&HotspotList>)->String {
        let mut w = JsonWriter::with_capacity(hs.len() * 128);
        w.write_array(|w| {
            for h in hs { h.write_collapsed_json_to(w); }
        });
        w.to_string()
    }
}

pub fn save_retrieved_hotspots_to (dir: impl AsRef<Path>, retrieved: &BitSet, completed: &VecDeque<CompletedOverpass<HotspotList>>)->Result<()> {
    for i in retrieved.iter() {
        if let Some(hs) = &completed[i].data {
            hs.save_to( &dir)?;
        }
    }
    Ok(())
}

/// the interface of importers used by OrbitalHotspotActor
#[async_trait]
pub trait HotspotImporter {
    /// import latest hotspots from current date with up to n_days of history and store in respective CompletedOverpasses
    async fn import_hotspots (&mut self, n_days: usize, cops: &mut VecDeque<CompletedOverpass<HotspotList>>) -> Result<BitSet>;
    
    /// what is the last date for which we got any hotspot data (might not be for our overpasses)
    fn last_reported (&self)->DateTime<Utc>;

    /// get the download time for the provided overpass end (might be either a const delay of depending on ground stations)
    fn get_download_schedule (&self, overpass_end: DateTime<Utc>) -> DateTime<Utc>;
}

/// this is a dummy HotspotImporter that does not import anything but does so right away
/// Useful to hook up new satellites in order to see their orbits/coverage
pub struct NoHotspotImporter {}

#[async_trait]
impl HotspotImporter for NoHotspotImporter {
    async fn import_hotspots (&mut self, n_days: usize, cops: &mut VecDeque<CompletedOverpass<HotspotList>>) -> Result<BitSet> {
        Ok(BitSet::with_capacity(0))
    }

    fn last_reported (&self)->DateTime<Utc> {
        datetime::utc_now() + datetime::days(1000) // make sure we never reschedule any "unavailable" data
    }

    fn get_download_schedule (&self, overpass_end: DateTime<Utc>) -> DateTime<Utc> {
        overpass_end
    }
}

/// aggregation of orbit segment and resulting instrument data (to be filled in once such data becomes available)
#[public_struct]
pub struct CompletedOverpass<T> {
    overpass: Overpass, // the time & trajectory 
    data: Option<T>     // e.g. hotspots received for this overpass - optional since we might not have received data yet
}

impl<T> CompletedOverpass<T> {
    pub fn new (overpass: Overpass)->Self { CompletedOverpass { overpass, data: None } }
}

//--- general utility functions and types

pub fn instant_now()->Instant {
    // TODO - this should use sim time
    Instant::now()
}

pub fn instant_from_datetime<Z> (dt: DateTime<Z>)->Instant where Z:TimeZone {
    Instant::from_unixtime( dt.timestamp_millis() as f64 / 1000.0)
}

pub fn duration_std (dur: StdDuration) -> Duration { Duration::from_seconds( dur.as_secs_f64()) }
pub fn duration_secs_f64 (secs: f64) -> Duration { Duration::from_seconds(secs) }
pub fn duration_minutes (minutes: usize) -> Duration { Duration::from_minutes(minutes as f64) }
pub fn duration_hours (hours: usize) -> Duration { Duration::from_hours(hours as f64) }
pub fn duration_days (days: usize) -> Duration { Duration::from_days(days as f64) }

pub fn instant_from_datetime_spec (ds: &str) -> Result<Instant> {
    datetime::parse_datetime(ds).ok_or( op_failed!("invalid datetime spec {}", ds)).map( |dt| instant_from_datetime(dt))
}

pub fn get_time_vec (orbit_duration: Duration, time_step: Duration, start_time: Instant)->Vec<Instant> {
    let n = (orbit_duration.as_seconds() / time_step.as_seconds()).ceil() as usize + 5; // the TLE mean_motion is just that - mean
    let mut t = start_time;

    let mut tv: Vec<Instant> = Vec::with_capacity(n);
    for i in 0..n {
        tv.push(t);
        t += time_step;
    }

    tv
}

pub type ColumnVec<'a> = Matrix<f64, Const<3>, Const<1>, ViewStorage<'a, f64, Const<3>, Const<1>, Const<1>, Const<3>>>;


#[constructor(0)]
pub extern "C" fn init_orbital_data () {
    let dir = pkg_cache_dir!().join("satkit");
    println!("using emphemeris from {:?}", dir);
    ensure_writable_dir(&dir).expect("failed to create satkit data dir");
    satkit::utils::set_datadir(&dir).expect("failed to set satkit data dir");

    update_orbital_data().expect("failed to update satkit ephemeris");
}

// in long running servers this should be called periodically (once per day)
pub fn update_orbital_data ()->Result<()> {
    let dir = satkit::utils::datadir().map_err(|_| op_failed!("no satkit data dir set"))?;
    let last_mod = get_modified_timestamp(&dir).ok_or( op_failed!( "no modified timestamp of satkit data dir"))?;
    let now = SystemTime::now();
    let elapsed = now.duration_since(last_mod)
        .map_err(|e| op_failed!("invalid modification timestamp of data dir {dir:?}: {e}"))?;

    if elapsed > days(1) {
        satkit::utils::update_datafiles( None, false).map_err( |e| op_failed!("failed to update satkit data dir: {}", e))?;
        set_modified_timestamp(&dir, now)?;
    } else {
        println!("satkit data up-to-date");
    }

    Ok(())
}
