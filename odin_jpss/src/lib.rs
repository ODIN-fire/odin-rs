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

use std::{os::raw, time::Duration};
use std::collections::{HashMap, VecDeque};
use std::collections::BTreeMap;
use std::vec::Vec;
use std::f64::consts::PI;
use chrono::{DateTime, NaiveDate, NaiveTime};
use geo::{GeodesicBearing, GeodesicDestination, Point};
use nav_types::{ECEF, WGS84};
use odin_common::{angle::{LatAngle, LonAngle}, geo::LatLon};
use orekit::{get_trajectory_point, OrbitalTrajectory, Overpass};
use serde::{Serialize, Deserialize};
use uom::si::f32::{Power,ThermodynamicTemperature};
use odin_common::fs::ensure_writable_dir;
use odin_build::define_load_config;
use reqwest;
use tokio;
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

mod jpss_geo;
use jpss_geo::*;


use crate::jpss_geo::Cartesian3D;


define_load_config!{}


/* #region VIIRS data structures  ***************************************************************************/

// raw viirs hotspot - used for easy parsing of CSV file
#[derive(Serialize,Deserialize,Debug,Clone)]
pub struct RawHotspot {
    latitude: LatAngle,
    longitude: LonAngle,
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
pub struct ViirsHotspot {
    date: DateTime<Utc>, // batched
    lat: LatAngle,
    lon: LonAngle,
    bright: ThermodynamicTemperature,
    frp: Power,
    confidence: char,
    version: String,
    bounds: Vec<LatLon>
}
impl ViirsHotspot {
    pub fn to_json_pretty (&self)->Result<String> {
        Ok(serde_json::to_string_pretty( &self )?)
    }
    pub fn to_json (&self)->Result<String> {
        Ok(serde_json::to_string( &self )?)
    }
    pub fn set_date (&mut self, date: DateTime<Utc>) {
        self.date = date;
    }
}

#[derive(Serialize,Deserialize,Debug,Clone)]
pub struct ViirsHotspots {
    satellite: u32,
    source: String,
    hotspots: Vec<ViirsHotspot>
}
impl ViirsHotspots {
    pub fn new (satellite: u32, source: String) -> Self {
        ViirsHotspots { satellite, source, hotspots: Vec::new() }
    }
    pub fn update_hotspots (&mut self, new_hotspots: ViirsHotspots) {
        // to do: more sophisticated update that removes hs older than max age and adds updated ones
        self.hotspots = new_hotspots.hotspots;
        // store by date, pair of (rounded) lat,lons,  
        // 
    }
    pub fn to_json_pretty (&self)->Result<String> {
        Ok(serde_json::to_string_pretty( &self )?)
    }
    pub fn to_json (&self)->Result<String> {
        Ok(serde_json::to_string( &self )?)
    }
}

pub struct HotspotMap {
    hs: HashMap<DateTime<Utc>,HashMap<LatLon, Vec<ViirsHotspot>> >
}
/* #endregion VIIRS data structure */

pub async fn get_latest_jpss(query_bounds: &String, data_dir: &PathBuf, url: &String, source: &String) -> Result<PathBuf> {
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
        Err(OdinJpssError::FileDownloadError(format!("download failed: {:?}", response.status())))
    } 
}

pub fn read_jpss(filename: &PathBuf) -> Result<RawHotspots> {
    let mut hs:Vec<RawHotspot> = Vec::new();
    let mut rdr = Reader::from_path(filename)?;
    let mut iter = rdr.deserialize();
    for result in iter {
        let record: RawHotspot = result?;
        println!("{:?}", record);
        hs.push(record);
    }
    Ok(RawHotspots{hotspots:hs})
}


pub fn get_query_bounds(bounds: &Vec<LatLon>) -> String {
    //w,s,e,n 
    let mut w: Option<f64> = None;
    let mut s: Option<f64> = None;
    let mut e: Option<f64> = None;
    let mut n: Option<f64> = None;
    for bound in bounds.iter() {
        let lat = bound.lat_deg;
        let lon = bound.lon_deg;
        if w.is_none() || lon < w.unwrap() {
            w = Some(lon);
        }
        if e.is_none() || lon > e.unwrap() {
            e = Some(lon);
        }
        if s.is_none() || lat < s.unwrap() {
            s = Some(lat);
        } 
        if n.is_none() || lat> n.unwrap() {
            n = Some(lat);
        }
    }
    format!("{:?},{:?},{:?},{:?}", w.unwrap(),s.unwrap(),e.unwrap(),n.unwrap())
}

/* #region hotspot parsing *************************************************************************************************/

pub fn get_hs_bounds(hs: &RawHotspot, overpass_list: &OverpassList) -> Result<Vec<LatLon>> {
    // transform lat lon to ECEF
    let hs_loc = WGS84::from_degrees_and_meters(hs.latitude.degrees(), hs.longitude.degrees(), 0.0);
    let hs_loc_ecef = Cartesian3D::from_ecef(ECEF::from(hs_loc));
    // get trajectory point - get overpass for corresponding date, get closest ground track
    let ground_point = get_trajectory_point(&hs_loc_ecef, &hs.get_datetime(), overpass_list);
    if let Some(gp) = ground_point {
        // covert traj point to lat lon
        let sat_pos = gp.to_wgs84();
        let sat_pos_geo = geo::geometry::Point::new(sat_pos.longitude_degrees(), sat_pos.latitude_degrees());
        let hs_loc_geo = geo::geometry::Point::new(hs_loc.longitude_degrees(), hs_loc.latitude_degrees());
        let scan_dist = (hs.scan*1000.0)/2.0; // needs to be in m
        let track_dist = (hs.track*1000.0)/2.0; // needs to be in m
        let bearing = sat_pos_geo.geodesic_bearing(hs_loc_geo);
        let track_bearing = bearing + 90.0; // satellite track (perp to scanAngle)
        let opp_track_bearing = track_bearing + 180.0; // oppositional satellite track
        // get dist from center to edges using scan/track
        let p_scan_0 = hs_loc_geo.geodesic_destination(bearing, scan_dist);// towards satellite on GC
        let p_scan_1 = hs_loc_geo.geodesic_destination(bearing+180.0, scan_dist);// away from satellite on GC
        // get coords using https://docs.rs/geo/latest/geo/algorithm/geodesic_distance/trait.GeodesicDistance.html
        let bounds = vec![p_scan_0.geodesic_destination(track_bearing, track_dist),
                                    p_scan_1.geodesic_destination(track_bearing, track_dist),
                                    p_scan_1.geodesic_destination(opp_track_bearing, track_dist),
                                    p_scan_0.geodesic_destination(opp_track_bearing, track_dist)];
        let b:Vec<LatLon> = bounds.into_iter().map(|x| lat_lon_from_point(x)).collect();
        Ok(b)
    } else {
        Err(OdinJpssError::BoundsError(String::from("Error: no trajectory ground point")))
    }   
}

pub fn process_hotspot(raw_hs: &RawHotspot, overpass_list: &OverpassList) -> Result<ViirsHotspot> {
    let bounds = get_hs_bounds(&raw_hs, overpass_list)?;
    Ok(ViirsHotspot { // add bounds as optional?
        date: raw_hs.get_datetime(),
        lat: raw_hs.latitude,
        lon: raw_hs.longitude,
        bright: raw_hs.bright_ti4,
        frp: raw_hs.frp,
        confidence: raw_hs.confidence,
        version: raw_hs.version.clone(),
        bounds: bounds
    })
}
pub fn process_hotspots(raw_hotspots: RawHotspots, overpass_list: &OverpassList, satellite: u32, source: String) -> Result<ViirsHotspots> {
    let hotspot_res: Vec<Result<ViirsHotspot>> = raw_hotspots.hotspots.iter().map(|x| process_hotspot(x, overpass_list)).collect();
    let hs_res: Result<Vec<ViirsHotspot>> = hotspot_res.into_iter().collect();
    let hs = hs_res?;
    let sorted_hs = parition_hotspots(hs, overpass_list)?;
    Ok(ViirsHotspots {satellite, source, hotspots: sorted_hs})
}

pub fn parition_hotspots(hotspots: Vec<ViirsHotspot>, overpass_list: &OverpassList) -> Result<Vec<ViirsHotspot>>{
    let mut parts: BTreeMap<DateTime<Utc>, Vec<ViirsHotspot>> = BTreeMap::new();
    let start = overpass_list.get_start()?;
    let end = overpass_list.get_end()?;
    let overpasses = overpass_list.get_end_dates();
    //--- step 1: sort into bins
    for hs in hotspots.into_iter() {
        let hs_d = hs.date;
        let overpass_d = get_overpass(&hs_d, &start, &end, &overpasses);
        match parts.get_mut(&overpass_d) {
            Some(hs_vec) => {hs_vec.push(hs)}
            None => {parts.insert(overpass_d, vec![hs]);}
        }
    }
    //--- step 2: merge bins according to maxOverpassDuration - i dont think we need this? overpasses should be unique, only possible issue for hotspots that were not cleanly mapped into an overpass

    //--- step 3: turn into date sorted ViirsHotspots sequence
    let mut sorted_hotspots = Vec::new();
    for (od, hs_vec) in parts.into_iter() {
        for mut hs in hs_vec.into_iter() {
            hs.set_date(od);
            sorted_hotspots.push(hs) 
        }
    }
    Ok(sorted_hotspots)
}

pub fn get_overpass(d: &DateTime<Utc>, start: &DateTime<Utc>, end: &DateTime<Utc>, overpasses: &Vec<DateTime<Utc>>) -> DateTime<Utc> {
    let mut date = d.clone();
    if (d >= start) & (d <= end) {
        for overpass_d in overpasses.iter() {
            if overpass_d > d {
                date = overpass_d.clone();
            }
        }
    } 
    date
}

/* #endregion hotspot parsing */

