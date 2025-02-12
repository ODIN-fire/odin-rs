/*
 * Copyright © 2024, United States Government, as represented by the Administrator of 
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

use std::time::Duration;
use std::collections::HashMap;
use std::collections::BTreeMap;
use std::vec::Vec;
use chrono::TimeDelta;
use chrono::TimeZone;
use chrono::{DateTime, NaiveDate, NaiveTime};
use geo::{Geodesic, Bearing, Destination};
use nav_types::{ECEF, WGS84};
use odin_common::angle::Latitude;
use odin_common::angle::Longitude;
use odin_common::geo::GeoPoint;
use odin_common::geo::GeoPolygon;
use odin_common::geo::GeoRect;
use orekit::get_trajectory_point;
use serde::{Serialize, Deserialize};
use uom::si::f32::{Power,ThermodynamicTemperature};
use odin_common::fs::ensure_writable_dir;
use odin_build::{define_load_config, define_load_asset};
use reqwest;
use chrono::Utc;
use std::{fs, path::PathBuf};
use tempfile;
use std::io::Write as IoWrite;
use csv::Reader;

mod errors;
use errors::*;

pub mod orekit;
use orekit::*;

pub mod actor;
use actor::*;

pub mod live_importer;

mod orbital_geo;
use orbital_geo::*;

pub mod orbital_service;
pub use orbital_service::*;


define_load_config!{}
define_load_asset!{}


/* #region VIIRS data structures  ***************************************************************************/

// raw viirs hotspot - used for easy parsing of CSV file
#[derive(Serialize,Deserialize,Debug,Clone)]
pub struct RawHotspot {
    latitude: Latitude,
    longitude: Longitude,
    #[serde(alias="brightness")] bright_ti4: ThermodynamicTemperature,
    scan: f64,
    track: f64,
    acq_date: NaiveDate,
    acq_time: u32, // hhmm
    satellite: String,
    instrument: String,
    confidence: char,
    version: String,
    #[serde(alias="bright_t31")] bright_ti5: ThermodynamicTemperature,
    frp: Power,
    daynight: char
}
impl RawHotspot {
    pub fn to_json_pretty (&self)->Result<String> {
        Ok(serde_json::to_string_pretty( &self )?)
    }
    pub fn to_json (&self)->Result<String> {
        Ok(serde_json::to_string( &self )?)
    }
    pub fn get_datetime(&self) -> DateTime<Utc> {
        let hrs = self.acq_time/100;
        let min = self.acq_time%100;
        let t = NaiveTime::from_hms_opt(hrs, min, 0).unwrap();
        let date = self.acq_date.clone();
        date.and_time(t).and_utc()
    }
}

#[derive(Serialize,Deserialize,Debug,Clone)]
pub struct RawHotspots {
    hotspots: Vec<RawHotspot>
}
impl RawHotspots {
    pub fn to_json_pretty (&self)->Result<String> {
        Ok(serde_json::to_string_pretty( &self )?)
    }
    pub fn to_json (&self)->Result<String> {
        Ok(serde_json::to_string( &self )?)
    }
}

// formatted viirs hotspot - has bounds, slightly different value set and variable names - alligns with RACE-ODIN json
#[derive(Serialize,Deserialize,Debug,Clone)]
#[serde(rename_all="camelCase")]
pub struct ViirsHotspot {
    pub date: i64, // batched
    pub lat: Latitude,
    pub lon: Longitude,
    pub bright: ThermodynamicTemperature,
    pub frp: Power,
    pub confidence: char,
    pub version: String,
    pub bounds: Vec<GeoPoint>
}
impl ViirsHotspot {
    pub fn to_json_pretty (&self)->Result<String> {
        Ok(serde_json::to_string_pretty( &self )?)
    }
    pub fn to_json (&self)->Result<String> {
        Ok(serde_json::to_string( &self )?)
    }
    pub fn set_date (&mut self, date: i64) {
        self.date = date;
    }
}

#[derive(Serialize,Deserialize,Debug,Clone)]
#[serde(rename_all="camelCase")]
pub struct ViirsHotspotSet { //should also have dates
    date: i64,
    sat_id: u32,
    source: String,
    hotspots: Vec<ViirsHotspot>
}
impl ViirsHotspotSet {
    // pub fn new (satellite: u32, source: String) -> Self {
    //     ViirsHotspotSet { satellite, source, hotspots: Vec::new() }
    // }
    pub fn from_hotspots (date: i64, sat_id: u32, source: String, hotspots: Vec<ViirsHotspot>) -> Self {
        ViirsHotspotSet {date, sat_id, source, hotspots }
    }
    pub fn to_json_pretty (&self)->Result<String> {
        Ok(serde_json::to_string_pretty( &self )?)
    }
    pub fn to_json (&self)->Result<String> {
        Ok(serde_json::to_string( &self )?)
    }
}

#[derive(Serialize,Deserialize,Debug,Clone)]
pub struct ViirsHotspotStore {
    satellite: u32,
    source: String,
    hotspots: HashMap<DateTime<Utc>, HashMap<String, ViirsHotspot>> // we use a string for latlon comparison
}

impl ViirsHotspotStore {
    pub fn new (satellite: u32, source: String) -> Self {
        ViirsHotspotStore{satellite, source, hotspots: HashMap::new()}
    }
    pub fn update(&mut self, hs_set: ViirsHotspotSet, max_age: Duration) { // function to update the hotspot store primarily for handling duplicate hotspots/nrt corrections
        // --- step 1: identify hs that are past max_age
        let mut old_dates = vec![];
        for dt in self.hotspots.keys() {
            if Utc::now() > (dt.clone() + max_age) {
                old_dates.push(dt.to_owned());
            }
        }
        // --- step 2: remove hs that are past max_age
        for dt in old_dates.into_iter() {
            self.hotspots.remove(&dt);
        }
        // --- step 3: add new hotspots based on date, latlon - check for existing hs with latlon rounded to 4 decimal
        let hs_date = Utc.timestamp_millis_opt(hs_set.date).unwrap();
        if let Some(date_map) = self.hotspots.get_mut(&hs_date ) {
            // --- update pixel even if it is in the map, ensures we use most accurate hotspot data
            for hs in hs_set.hotspots.into_iter() {
                let hs_loc = format!("({:.4}, {:.4})", hs.lat.degrees(), hs.lon.degrees());
                date_map.insert(hs_loc, hs);
            }
            
        } else {
            // -- datetime not in map yet, need to create it
            let mut date_map: HashMap<String, ViirsHotspot> = HashMap::new();
            for hs in hs_set.hotspots.into_iter() {
                let hs_loc = format!("({:.4}, {:.4})", hs.lat.degrees(), hs.lon.degrees());
                date_map.insert(hs_loc, hs);
            }
            self.hotspots.insert(hs_date, date_map);
        }
    }   
    pub fn to_hotspots(&self) -> Vec<ViirsHotspotSet> { // this should be to a vec of hotspot sets
        let mut hs_sets = Vec::new(); 
        for dt in self.hotspots.keys(){
            let hs_map_op = self.hotspots.get(dt);
            if let Some(hs_map) = hs_map_op {
                let hs: Vec<ViirsHotspot> = hs_map.to_owned().into_iter().map(|(latlon, hs_val)| hs_val).collect();
                let hs_set: ViirsHotspotSet =  ViirsHotspotSet::from_hotspots(dt.timestamp_millis(), self.satellite, self.source.clone(), hs);
                hs_sets.push(hs_set);
            }
        }
        hs_sets
    }
}

/* #endregion VIIRS data structure */

pub async fn get_latest_hotspot_download(data_dir: &PathBuf, url: &String, source: &String) -> Result<PathBuf> {
    let request_date = Utc::now();
    let filename = data_dir.join(format!("{}_{}.csv", source, request_date.format("%Y-%m-%d_H-%M-%S")));
    let mut file = tempfile::NamedTempFile::new().unwrap(); // don't use path yet as that would expose partial downloads to the world
    let mut response = reqwest::get(url).await.unwrap();
    while let Some(chunk) = response.chunk().await.unwrap() {
        file.write_all(&chunk).unwrap();
    }

    if response.status().is_success() {
        let file_len = fs::metadata(file.path()).unwrap().len();
        if file_len > 0 {
            fs::rename(file.path(), &filename)?; // now make it visible to the world as a permanent file
        }
        Ok(filename) 
    } else {
        Err(OdinOrbitalSatError::FileDownloadError(format!("download failed: {:?}", response.status())))
    } 
}

pub fn read_hotspots(filename: &PathBuf) -> Result<RawHotspots> {
    let mut hs:Vec<RawHotspot> = Vec::new();
    let mut rdr = Reader::from_path(filename)?;
    let iter = rdr.deserialize();
    for result in iter {
        let record: RawHotspot = result?;
        hs.push(record);
    }
    Ok(RawHotspots{hotspots:hs})
}


pub fn get_query_bounds(bounds: &GeoRect) -> String {
    //w,s,e,n 
    let w = bounds.west().degrees();
    let s = bounds.south().degrees();
    let e = bounds.east().degrees();
    let n = bounds.north().degrees();
    format!("{:?},{:?},{:?},{:?}", w, s, e, n)
}

/* #region hotspot parsing *************************************************************************************************/

pub fn get_hs_bounds(hs: &RawHotspot, overpass_list: &OverpassList) -> Result<Vec<GeoPoint>> {
    // transform lat lon to ECEF
    let hs_loc = WGS84::from_degrees_and_meters(hs.latitude.degrees(), hs.longitude.degrees(), 0.0);
    let hs_loc_ecef = Cartesian3D::from_ecef(ECEF::from(hs_loc));
    // get trajectory point - get overpass for corresponding date, get closest ground track
    let ground_point = get_trajectory_point(&hs_loc_ecef, &hs.get_datetime(), overpass_list)?;
    if let Some(gp) = ground_point {
        // covert traj point to lat lon
        let sat_pos = gp.to_wgs84();
        let sat_pos_geo = geo::geometry::Point::new(sat_pos.longitude_degrees(), sat_pos.latitude_degrees());
        let hs_loc_geo = geo::geometry::Point::new(hs_loc.longitude_degrees(), hs_loc.latitude_degrees());
        let scan_dist = (hs.scan*1000.0)/2.0; // needs to be in m
        let track_dist = (hs.track*1000.0)/2.0; // needs to be in m
        let bearing = Geodesic::bearing(sat_pos_geo, hs_loc_geo);
        let track_bearing = bearing + 90.0; // satellite track (perp to scanAngle)
        let opp_track_bearing = track_bearing + 180.0; // oppositional satellite track
        // get dist from center to edges using scan/track
        let p_scan_0 = Geodesic::destination(hs_loc_geo, bearing, scan_dist);// towards satellite on GC
        let p_scan_1 = Geodesic::destination(hs_loc_geo, bearing+180.0, scan_dist);// away from satellite on GC
        // get coords using https://docs.rs/geo/latest/geo/algorithm/geodesic_distance/trait.GeodesicDistance.html
        let bounds = vec![ Geodesic::destination(p_scan_0, track_bearing, track_dist),
                                    Geodesic::destination(p_scan_1,track_bearing, track_dist),
                                    Geodesic::destination(p_scan_1,opp_track_bearing, track_dist),
                                    Geodesic::destination(p_scan_0,opp_track_bearing, track_dist)];
        let b:Vec<GeoPoint> = bounds.into_iter().map(|x| GeoPoint::from_point(x)).collect();
        Ok(b)
    } else {
        let overpass_first_dates: Vec<i64> = overpass_list.overpasses.iter().map(|x| x.first_date).collect();
        let overpass_last_dates: Vec<i64> = overpass_list.overpasses.iter().map(|x| x.last_date).collect();
         Err(OdinOrbitalSatError::BoundsError(String::from("Error: no trajectory ground point")))
    }   
}

pub fn process_hotspot(raw_hs: &RawHotspot, overpass_list: &OverpassList) -> Result<ViirsHotspot> {
    let bounds = get_hs_bounds(&raw_hs, overpass_list)?;
    Ok(ViirsHotspot { // add bounds as optional?
        date: raw_hs.get_datetime().timestamp_millis(),
        lat: raw_hs.latitude,
        lon: raw_hs.longitude,
        bright: raw_hs.bright_ti4,
        frp: raw_hs.frp,
        confidence: raw_hs.confidence,
        version: raw_hs.version.clone(),
        bounds: bounds
    })
}
pub fn process_hotspots(raw_hotspots: RawHotspots, overpass_list: &OverpassList, satellite: u32, source: String) -> Result<Vec<ViirsHotspotSet>> {
    let hotspot_res: Vec<Result<ViirsHotspot>> = raw_hotspots.hotspots.iter().map(|x| process_hotspot(x, overpass_list)).collect();
    let hs_res: Result<Vec<ViirsHotspot>> = hotspot_res.into_iter().collect();
    let hs = hs_res?;
    let sorted_hs = partition_hotspots(hs, overpass_list, satellite, source.clone())?;
    Ok(sorted_hs)
}

pub fn partition_hotspots(hotspots: Vec<ViirsHotspot>, overpass_list: &OverpassList, satellite: u32, source: String) -> Result<Vec<ViirsHotspotSet>>{
    let mut parts: BTreeMap<DateTime<Utc>, Vec<ViirsHotspot>> = BTreeMap::new();
    //--- step 1: sort into bins
    for hs in hotspots.into_iter() {
        let hs_d = Utc.timestamp_millis_opt(hs.date).unwrap();
        let overpass = get_overpass_for_date(&hs_d, overpass_list)?;
        let overpass_d = Utc.timestamp_millis_opt(overpass.last_date).unwrap();
        match parts.get_mut(&overpass_d) {
            Some(hs_vec) => {hs_vec.push(hs)}
            None => {parts.insert(overpass_d, vec![hs]);}
        }
    }
    //--- step 2: merge bins according to maxOverpassDuration - i dont think we need this? overpasses should be unique, only possible issue for hotspots that were not cleanly mapped into an overpass

    //--- step 3: turn into date sorted ViirsHotspotSet sequence
    let mut sorted_hotspot_sets = Vec::new();
    for (od, hs_vec) in parts.into_iter() {
        let mut sorted_hotspots = Vec::new();
        for mut hs in hs_vec.into_iter() {
            hs.set_date(od.timestamp_millis());
            sorted_hotspots.push(hs) 
        }
        sorted_hotspot_sets.push(ViirsHotspotSet::from_hotspots(od.timestamp_millis(), satellite, source.clone(), sorted_hotspots));

    }
    
    Ok(sorted_hotspot_sets)
}

// pub fn get_overpass(d: &DateTime<Utc>, start: &DateTime<Utc>, end: &DateTime<Utc>, overpasses: &Vec<DateTime<Utc>>) -> DateTime<Utc> {
//     let overpass = get_overpass_for_date(d, overpasses)?;

//     let mut date = d.clone();
//     if (d >= start) & (d <= end) {
//         for overpass_d in overpasses.iter() {
//             if overpass_d > d {
//                 date = overpass_d.clone();
//             }
//         }
//     } 
//     date 
// }

/* #endregion hotspot parsing */

