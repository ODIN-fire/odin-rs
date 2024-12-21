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

use nalgebra::{distance, Const, Dyn, Matrix, Rotation3, VecStorage, Vector3};
use chrono::{DateTime, Datelike, Duration, SubsecRound, TimeDelta, TimeZone, Timelike, Utc, NaiveDateTime, NaiveDate};
use odin_common::datetime::naive_utc_date_to_utc_datetime;
use odin_common::geo::LatLon;
use sgp4::{Constants, Elements};
use satkit::frametransform::{gmst, qteme2itrf};
use satkit::{AstroTime, TLE};
use satkit::ITRFCoord;
use satkit;
use nav_types::{WGS84, ECEF};
use serde_json::Value;
use core::f64;
use std::collections::HashMap;
use std::f64::consts::PI;
use std::vec::Vec;
use serde::{Deserialize,Serialize};
use uom::si::molar_radioactivity::disintegrations_per_minute_per_mole;
use uom::si::length::{kilometer, meter};
use uom::si::f64::Length;
use reqwest::Client;
use crate::jpss_geo::Cartesian3D;
use crate::errors::*;

/* #region overpass data structures  ***************************************************************************/

#[derive(Serialize,Deserialize,Debug,Clone)]
#[serde(rename_all="camelCase")]
pub struct Overpass {
    pub sat_id: i32,
    pub first_date: i64,//DateTime<Utc>,
    pub last_date: i64,
    pub coverage: f32,
    pub max_scan: f64,
    pub trajectory: Vec<Trajectory>
}

#[derive(Serialize,Deserialize,Debug,Clone)]
pub struct Trajectory {
    pub time: i64,
    pub x: f64,
    pub y: f64,
    pub z: f64
}

#[derive(Serialize,Deserialize,Debug,Clone)]
 pub struct OverpassList {
    pub overpasses: Vec<OrbitalTrajectory>
 }
 impl OverpassList {
    pub fn new() -> Self {
        OverpassList{ overpasses: Vec::new() }
    }

    pub fn from_orbital_trajectories(orbital_trajectories: Vec<OrbitalTrajectory>) -> Self {
        OverpassList{ overpasses: orbital_trajectories }
    }

    pub fn update(&mut self, overpass_list: OverpassList) {
        self.overpasses = overpass_list.overpasses;
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string( &self )?)
    }

    pub fn to_json_pretty(&self) -> Result<String>{
        Ok(serde_json::to_string_pretty( &self )?)
    }

    pub fn get_end_dates(&self) -> Vec<DateTime<Utc>> {
        let mut dates = Vec::new();
        for op in self.overpasses.iter() {
            dates.push(op.t_end.clone())
        }
        dates.sort();
        dates
    }

    pub fn get_start_dates(&self) -> Vec<DateTime<Utc>> {
        let mut dates = Vec::new();
        for op in self.overpasses.iter() {
            dates.push(op.t_start.clone())
        }
        dates.sort();
        dates
    }
    pub fn get_start(&self) -> Result<DateTime<Utc>> {
        let start_dates = self.get_start_dates();
        if start_dates.len() > 0 {
            return Ok(start_dates[0]);
        } else {
            Err(date_error(format!("No overpass dates")))
        }
    }

    pub fn get_end(&self) -> Result<DateTime<Utc>> {
        let end_dates = self.get_end_dates();
        if end_dates.len() > 0 {
            return Ok(end_dates[0]);
        } else {
            Err(date_error(format!("No overpass dates")))
        }
    }
 }

#[derive(Serialize,Deserialize,Debug,Clone)]
pub struct OrbitalTrajectory {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub z: Vec<f64>, //add start, tle, units, reference system
    pub t_end: DateTime<Utc>,
    pub t_start: DateTime<Utc>,
    pub length: usize,
    pub sat_id: i32, 
    pub swath_width: Length
}

impl OrbitalTrajectory{
    pub fn new(length: i32, start_t: DateTime<Utc>, d_t: TimeDelta, sat_id: i32, swath_width: Length) -> Self {
        OrbitalTrajectory {
            x: Vec::<f64>::with_capacity(length as usize),
            y: Vec::<f64>::with_capacity(length as usize),
            z: Vec::<f64>::with_capacity(length as usize),
            t_start: start_t.clone(),
            t_end: start_t + (d_t*length),
            length: length as usize,
            sat_id: sat_id,
            swath_width: swath_width
        }
    }

    pub fn from_overpass(op: &Overpass, margin: TimeDelta) -> Self {
        let x: Vec<f64> = op.trajectory.iter().map(|x| x.x).collect(); 
        let y: Vec<f64> = op.trajectory.iter().map(|x| x.y).collect(); 
        let z: Vec<f64> = op.trajectory.iter().map(|x| x.z).collect(); 
        let swath_width = get_swath_for_orbit(&x, &y, &z, op.max_scan);
        OrbitalTrajectory {
            t_end:  Utc.timestamp_millis_opt(op.last_date).unwrap() + margin,
            t_start:  Utc.timestamp_millis_opt(op.first_date).unwrap() - margin,
            length: op.trajectory.len(),
            x: x,
            y: y,
            z: z,
            sat_id: op.sat_id,
            swath_width: swath_width
        }
        
        
    }

    pub fn update(&mut self, i: usize, vec3:Vector3<f64>) {
        self.x[i] = vec3.x;
        self.y[i] = vec3.y;
        self.z[i] = vec3.z;
    }

    pub fn find_closest_ground_track_point(&self, p: &Cartesian3D) -> Cartesian3D {
        let i = self.find_closest_index(p);
        let p1 = Cartesian3D{x: self.x[i-1], y: self.y[i-1], z: self.z[i-1]};
        let p2 = Cartesian3D{x: self.x[i+1], y: self.y[i+1], z: self.z[i+1]};
        let mut gp = Cartesian3D::new();
        gp.set_to_intersection_with_plane(&p1, &p2, p); // set pt to intersection w/ plane
        gp.scale_to_earth_radius(); // scale to earth radius
        gp
    }

    pub fn find_closest_orbit_point(&self, p: &Cartesian3D) -> Cartesian3D {
        let i = self.find_closest_index(p);
        let p1 = Cartesian3D{x: self.x[i-1], y: self.y[i-1], z: self.z[i-1]};
        let p2 = Cartesian3D{x: self.x[i+1], y: self.y[i+1], z: self.z[i+1]};
        let mut gp = Cartesian3D::new();
        gp.set_to_intersection_with_plane(&p1, &p2, p); // set pt to intersection w/ plane
        gp
    }

    pub fn dist2(&self, i:usize, p: &Cartesian3D) -> f64 {
        ((self.x[i]-p.x).powf(2.0)) + ((self.y[i]-p.y).powf(2.0)) +((self.z[i]-p.z).powf(2.0))
    }
    
    pub fn find_closest_index(&self, p: &Cartesian3D) -> usize {
        let mut l = 1;
        let mut r = self.length-2;
        let mut i = r/2; // sets up binary search
        let mut dl = self.dist2(i, p) - self.dist2(i-1, p);
        let mut dr = self.dist2(i+1, p) - self.dist2(i, p);
        let mut di = 0.0;
        let mut i_last = i;

        while (dl.signum() == dr.signum()) {
            if (dr < 0.0) { // bisect right
                l = i;
            } else { // bisect left
                r = i
            }
            i = (l + r)/2;
            if (i == i_last) {
                return i;
            } else {
                i_last = i;
            }

            di = self.dist2(i, p);
            dl = di - self.dist2(i-1, p);
            dr = self.dist2(i+1, p) - di;
        }
        i
    }
}

pub fn get_trajectory_point(point: &Cartesian3D, date:&DateTime<Utc>, overpass_list: &OverpassList) -> Option<Cartesian3D> {
    let date = date.clone();
    let mut tp:Option<Cartesian3D> = None;
    for overpass in overpass_list.overpasses.iter() {
        if (overpass.t_end >= date) & (overpass.t_start <= date) {
            tp = Some(overpass.find_closest_ground_track_point(point));
        } else {
            println!("hs date: {:?}; overpass dates: {:?}, {:?}", date, overpass.t_end, overpass.t_start )
        }
    }
    tp
}

/* #endregion overpass data structure */

/* #region TLE import functions */

pub async fn get_tles_celestrak_sgp4(sat_id: u32) -> Result<Vec<Elements>>{
    let client = Client::new();
    let sat_id_str = sat_id.clone().to_string();
    let query = vec![("CATNR", sat_id_str.as_str()),("FORMAT", "json")];
    let response = client.get("https://celestrak.com/NORAD/elements/gp.php")
            .query(&query).send().await?;
    if response.status().is_success() { 
        let tles: Vec<Elements> = response.json::<Vec<Elements>>().await?;
        Ok(tles)
    } else {
        Err(OdinJpssError::FileDownloadError(format!("TLE download failed: {:?}", response.status())))
    } 
}

pub async fn get_tles_celestrak(sat_id: u32) -> Result<TLE>{
    let client = Client::new();
    let sat_id_str = sat_id.clone().to_string();
    let query = vec![("CATNR", sat_id_str.as_str()),("FORMAT", "txt")];
    let response = client.get("https://celestrak.com/NORAD/elements/gp.php")
            .query(&query).send().await?;
    if response.status().is_success() { 
        let raw_lines =  response.text().await?;
        let lines: Vec<&str> = raw_lines.lines().collect();
        if lines.len() == 2 {
            let tle_res =  TLE::load_2line(lines[0], lines[1]);
            match tle_res {
                Ok(tle) => {
                    Ok(tle)
                }
                Err(err) => {
                    Err(OdinJpssError::TleError(format!("Satkit TLE import failed {:?}", err)))
                }
            }
        } else if lines.len() == 3 {
            let tle_res =  TLE::load_3line(lines[0], lines[1], lines[2]);
            match tle_res {
                Ok(tle) => {
                    Ok(tle)
                }
                Err(err) => {
                    Err(OdinJpssError::TleError(format!("Satkit TLE import failed {:?}", err)))
                }
            }
        } else { Err(OdinJpssError::TleError(format!("Inncorrect TLE lines {:?}", lines.len())))}
    } else {
        Err(OdinJpssError::FileDownloadError(format!("TLE download failed: {:?}", response.status())))
    } 
}

pub async fn get_tles_spacetrack_sgp4(sat_id: u32, username: &str, password:&str) -> Result<Vec<Elements>>{
    let client = Client::new();
    let mut form = HashMap::new();
    form.insert("identity", username);
    form.insert("password", password);
   
    // let query = vec![("identity", username),("password", password)];
    let url = format!("https://www.space-track.org/basicspacedata/query/class/gp/NORAD_CAT_ID/{}/format/json", sat_id);
    form.insert("query", url.as_str());
    let response = client.post("https://www.space-track.org/ajaxauth/login").form(&form).send().await?;
    if response.status().is_success() { 
        let tles: Vec<Elements> = response.json::<Vec<Elements>>().await?;
        Ok(tles)
    } else {
        Err(OdinJpssError::FileDownloadError(format!("TLE download failed: {:?}", response.status())))
    } 
}

pub async fn get_tles_spacetrack(sat_id: u32, username: &str, password:&str) -> Result<TLE>{
    let client = Client::new();
    let mut form = HashMap::new();
    form.insert("identity", username);
    form.insert("password", password);
   
    // let query = vec![("identity", username),("password", password)];
    let url = format!("https://www.space-track.org/basicspacedata/query/class/gp/NORAD_CAT_ID/{}/format/json", sat_id);
    form.insert("query", url.as_str());
    let response = client.post("https://www.space-track.org/ajaxauth/login").form(&form).send().await?;
    if response.status().is_success() { 
        let json_res: Value = serde_json::from_str(response.text().await?.as_str())?;
        let mut lines = vec![];
        if let Some(line0) = json_res[0].get("TLE_LINE0") {
            lines.push(line0.as_str().unwrap());
        }
        if let Some(line1) = json_res[0].get("TLE_LINE1") {
            lines.push(line1.as_str().unwrap());
        }
        if let Some(line2) = json_res[0].get("TLE_LINE2") {
            lines.push(line2.as_str().unwrap());
        }
        if lines.len() == 2 {
            let tle_res =  TLE::load_2line(lines[0], lines[1]);
            match tle_res {
                Ok(tle) => {
                    Ok(tle)
                }
                Err(err) => {
                    Err(OdinJpssError::TleError(format!("Satkit TLE import failed {:?}", err)))
                }
            }
        } else if lines.len() == 3 {
            let tle_res =  TLE::load_3line(lines[0], lines[1], lines[2]);
            match tle_res {
                Ok(tle) => {
                    Ok(tle)
                }
                Err(err) => {
                    Err(OdinJpssError::TleError(format!("Satkit TLE import failed {:?}", err)))
                }
            }
        } else { Err(OdinJpssError::TleError(format!("Inncorrect TLE lines {:?}", lines.len())))}
    } else {
        Err(OdinJpssError::FileDownloadError(format!("TLE download failed: {:?}", response.status())))
    } 
}
/* #endregion TLE import functions */

/* #region overpass calculation functions  ***************************************************************************/

// fn compute_overpass_periods(tle: Vec<sgp4::Elements>, start_date: DateTime<Utc>, duration: TimeDelta, region:Vec<LatLon>, scan_angle:f64) {

// }

pub fn compute_full_orbits_sgp4(tle: &Elements, max_scan: f64) -> Result<OverpassList>{ //todo: take in region
    let constants = Constants::from_elements(tle)?;
    let mut preds: Vec<[f64;3]> = vec![];
    let times = get_time_vector(); 
    for time in times.iter() { 
        let t = tle.datetime_to_minutes_since_epoch(&time.naive_utc())?;
        let prediction = constants.propagate(t)?;
        //let position: [f64;3]  = prediction.position.map(|f| f*1000.0); // slows it way down
        // if in_region(region, prediction.position) {

        // }
        preds.push(prediction.position);
    }
    let overpass = format_prediction_sgp4(preds, times, tle, max_scan)?;
    Ok(overpass)
}

pub fn compute_full_orbits(mut tle: TLE, max_scan: f64) -> Result<OverpassList> {
    let times = get_time_vector(); 
    let ats: Vec<AstroTime> = times.iter().map(|x| utc_to_astrotime(x)).collect();
    let (pred_teme, _, _) = satkit::sgp4::sgp4(&mut tle, &ats[..]);
    let overpass = format_prediction(pred_teme, times, tle, max_scan)?;
    Ok(overpass)
}

pub fn get_init_times_vector(history: Duration) -> Vec<DateTime<Utc>> {
    let now = Utc::now();
    // start = now - history
    let start = now - TimeDelta::seconds(history.num_seconds());
    let total_steps = history.num_seconds() + TimeDelta::hours(24).num_seconds();
    let mut times:Vec<DateTime<Utc>> = vec![];
    let mut now_mut = start.round_subsecs(0).clone();
    for i in 1..total_steps {
        now_mut = now_mut + TimeDelta::seconds(1);
        times.push(now_mut);
    }
    times
}

pub fn compute_initial_orbits(mut tle: TLE, max_scan: f64, history: Duration) -> Result<OverpassList> {
    let times = get_init_times_vector(history); 
    let ats: Vec<AstroTime> = times.iter().map(|x| utc_to_astrotime(x)).collect();
    let (pred_teme, _, _) = satkit::sgp4::sgp4(&mut tle, &ats[..]);
    let overpass = format_prediction(pred_teme, times, tle, max_scan)?;
    Ok(overpass)
}

fn compute_approximate_swath_width(altitude: Length, max_scan: f64) -> Length {
    let earth = Length::new::<meter>(6371000.0);
    let d = earth + altitude;
    let c0 = f64::sin(max_scan)/earth; 
    let c1 = earth.value.powf(2.0) - d.value.powf(2.0);
    // val c1 = squared(r) - squared(d)
    let c2 = d*f64::cos(max_scan);
    let a = c2.value - (c2.value.powf(2.0)+c1).sqrt();
    let alpha = (c0.value*a).asin();
    Length::new::<meter>(earth.value*alpha)
}

fn get_average_altitude(x: &Vec<f64>, y: &Vec<f64>, z: &Vec<f64>) -> f64{
    let p1 = Cartesian3D::from_ecef(ECEF::new(x[0], y[0], z[0])).to_wgs84();
    let p2_ind = x.len()-1;
    let p2 = Cartesian3D::from_ecef(ECEF::new(x[p2_ind], y[p2_ind], z[p2_ind])).to_wgs84();
    (p1.altitude() + p2.altitude()) / 2.0
}

fn get_swath_for_orbit(x: &Vec<f64>, y: &Vec<f64>, z: &Vec<f64>, max_scan: f64) -> Length {
    let altitude = get_average_altitude(x, y, z);
    compute_approximate_swath_width(Length::new::<meter>(altitude), max_scan)
}

pub fn get_time_vector() -> Vec<DateTime<Utc>> {
    let now = Utc::now();
    let now_round = now.round_subsecs(0);
    let now_naive = now_round.naive_utc();
    let future_naive = now_naive.clone() + TimeDelta::hours(24);
    let mut times:Vec<DateTime<Utc>> = vec![];
    let mut now_mut = now_round.clone();
    for i in 1..86400 {
        now_mut = now_mut + TimeDelta::seconds(1);
        times.push(now_mut);
    }
    times
}

pub fn utc_to_astrotime(time: &DateTime<Utc>) -> AstroTime{
    AstroTime::from_datetime(time.year(), time.month(), time.day(), time.hour(), time.minute(), time.second().into())
}

pub struct ECEFCoordinates {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub fn convert_pred(pred: [f64;3] , time: &DateTime<Utc>) -> ECEFCoordinates {
    let at = AstroTime::from_datetime(time.year(), time.month(), time.day(), time.hour(), time.minute(), time.second().into());
    let itrf = Rotation3::<f64>::from_matrix(qteme2itrf(&at).to_rotation_matrix().matrix()) *  Vector3::new(pred[0], pred[1], pred[2]);
    let itrf_coord = ITRFCoord::from_slice(&itrf.as_slice()).unwrap();
    ECEFCoordinates {x:itrf_coord.itrf[0], y:itrf_coord.itrf[1], z:itrf_coord.itrf[2]}
}

// pub fn get_orbits_from_trajectories(trajectories: Vec<Trajectory>) -> Vec<Vec<Trajectory>> {
//     // split long trajectory into multiple orbits
//     // define orbit end points as 85, -85 degrees
//     let max_z = Cartesian3D::from_latlon(LatLon { lat_deg: 85.0, lon_deg: 0.0 }).to_ecef().z();
//     let min_z = Cartesian3D::from_latlon(LatLon { lat_deg: -85.0, lon_deg: 0.0 }).to_ecef().z();
    
// }

fn coverable_region(region: &Vec<LatLon>, max_scan: f64) -> bool {
    // if distance between edges are within max_scan, then reurn true
    // sort by lat, then check the distance between the points if on the same lat
    true
}

pub fn filter_orbits(overpass_list: &OverpassList,  region: &Vec<LatLon>, max_scan: f64) -> OverpassList{
    let mut filtered_orbits: Vec<OrbitalTrajectory> = vec![];
    let coverable_region = coverable_region(region, max_scan);
    for overpass in overpass_list.overpasses.clone().into_iter(){
        if (coverable_region) {
            //println!("region covarable");
            if covers_region(&overpass, region, max_scan) {
                filtered_orbits.push(overpass);
            }
            // else {
            //     println!("region not covered");
            // }
        } else {
            //println!("region not covered");
            if covers_region_partial(&overpass, region, max_scan) {
                filtered_orbits.push(overpass);
            }
        }
        
    }
    OverpassList { overpasses: filtered_orbits }
}

pub fn covers_region(overpass: &OrbitalTrajectory, region: &Vec<LatLon>, max_scan: f64) -> bool {
    let mut covers = true;
    for vertex in region.iter(){
        let point = Cartesian3D::from_latlon(vertex.clone());
        let mut orbit_point = overpass.find_closest_orbit_point(&point);
        let dist_to_earth = orbit_point.z;
        let max_scan_m = scan_angle_to_meters(max_scan, dist_to_earth);
        orbit_point.scale_to_earth_radius();
        // let ground_track = overpass.find_closest_ground_track_point(&point);
        let distance = orbit_point.to_wgs84().distance(&point.to_wgs84()); // uses great circle distance
        if (distance <= (max_scan_m/2.0)) {
            covers = true;
            //ln!("covers distance:{}, max scan:{}", distance, max_scan_m/2.0);
        } else {
            //println!("distance:{}, max scan:{}", distance, max_scan_m/2.0);
            covers = false;
            break
        }
    }
    covers
}

pub fn scan_angle_to_meters(max_scan: f64, dist_to_earth: f64) -> f64{
    dist_to_earth * (f64::tan((max_scan/ 2.0)*PI/180.0) * 2.0)
}

pub fn covers_region_partial(overpass: &OrbitalTrajectory, region: &Vec<LatLon>, max_scan: f64) -> bool { // for cases when the region is too large to fully fit in a single overpass
    let mut covers = true; // update with interpolation
    for vertex in region.iter(){
        let point = Cartesian3D::from_latlon(vertex.clone());
        let mut orbit_point = overpass.find_closest_orbit_point(&point);
        let dist_to_earth = orbit_point.z;
        let max_scan_m = scan_angle_to_meters(max_scan, dist_to_earth);
        orbit_point.scale_to_earth_radius();
        // let ground_track = overpass.find_closest_ground_track_point(&point);
        let distance = orbit_point.to_wgs84().distance(&point.to_wgs84()); // uses great circle distance
        if (distance <= (max_scan_m/2.0)) {
            covers = true;
            break
        } else {
            covers = false;
            break
        }
    }
    covers
}

pub fn format_prediction_sgp4(preds: Vec<[f64;3]>, times: Vec<DateTime<Utc>>, tle: &Elements, max_scan: f64) -> Result<OverpassList> {
    let times = get_time_vector();
    let max_z = Cartesian3D::from_latlon(LatLon { lat_deg: 85.0, lon_deg: 0.0 }).to_ecef().z();
    let min_z = Cartesian3D::from_latlon(LatLon { lat_deg: -85.0, lon_deg: 0.0 }).to_ecef().z(); 
    let mut trajectories = vec![];
    let mut orbital_trajectories:Vec<OrbitalTrajectory> = vec![];
    let mut in_current_trajectory = false;
    for (pred, time) in preds.into_iter().zip(times.iter()) {
        let ecef = convert_pred(pred, time);
        if (ecef.z < max_z) & (ecef.z > min_z) {
            in_current_trajectory = true;
            // check if it is in region, if in region then save
            let traj = Trajectory{
                time: time.timestamp_millis(), 
                x: ecef.x,
                y: ecef.y,
                z: ecef.z
            };
            trajectories.push(traj);
        } else {
            if in_current_trajectory { // first point outside of trajectory
                // convert to overpass
                let overpass = Overpass {
                    sat_id: tle.norad_id as i32,
                    first_date:  times[0].timestamp_millis(),
                    last_date: times[times.len()-1].timestamp_millis(),
                    coverage: 0.0,
                    trajectory: trajectories, 
                    max_scan: max_scan
                };
                // convert trajectories to orbital trajectory
                let orbital_traj = OrbitalTrajectory::from_overpass(&overpass, TimeDelta::new(120, 0).unwrap());
                // save orbital trajectories
                orbital_trajectories.push(orbital_traj);
                // reset trajectories
                trajectories = vec![];

            }
            in_current_trajectory = false
        }
    }
    let overpasses = OverpassList::from_orbital_trajectories(orbital_trajectories);
    Ok(overpasses)
}

pub fn format_prediction(preds: Matrix<f64, Const<3>, Dyn, VecStorage<f64, Const<3>, Dyn>>, times: Vec<DateTime<Utc>>, tle: TLE, max_scan: f64) -> Result<OverpassList> {
    let times = get_time_vector();
    let max_z = Cartesian3D::from_latlon(LatLon { lat_deg: 85.0, lon_deg: 0.0 }).to_ecef().z();
    let min_z = Cartesian3D::from_latlon(LatLon { lat_deg: -85.0, lon_deg: 0.0 }).to_ecef().z(); 
    let mut trajectories = vec![];
    let mut orbital_trajectories:Vec<OrbitalTrajectory> = vec![];
    let mut in_current_trajectory = false;
    for (pred, time) in preds.column_iter().zip(times.iter()) {
        let temp_pred =[pred[0], pred[1], pred[2]];
        let ecef = convert_pred(temp_pred, time);
        if (ecef.z < max_z) & (ecef.z > min_z) {
            in_current_trajectory = true;
            // check if it is in region, if in region then save
            let traj = Trajectory{
                time: time.timestamp_millis(), 
                x: ecef.x,
                y: ecef.y,
                z: ecef.z
            };
            trajectories.push(traj);
        } else {
            if in_current_trajectory { // first point outside of trajectory
                // convert to overpass
                let overpass = Overpass {
                    sat_id: tle.sat_num,
                    first_date:  times[0].timestamp_millis(),
                    last_date: times[times.len()-1].timestamp_millis(),
                    coverage: 0.0,
                    trajectory: trajectories, 
                    max_scan: max_scan
                };
                // convert trajectories to orbital trajectory
                let orbital_traj = OrbitalTrajectory::from_overpass(&overpass, TimeDelta::new(120, 0).unwrap());
                // save orbital trajectories
                orbital_trajectories.push(orbital_traj);
                // reset trajectories
                trajectories = vec![];

            }
            in_current_trajectory = false
        }
    }
    let overpasses = OverpassList::from_orbital_trajectories(orbital_trajectories);
    // let overpass = Overpass {
    //     sat_id: tle.sat_num,
    //     first_date:  times[0].timestamp_millis(),
    //     last_date: times[times.len()-1].timestamp_millis(),
    //     coverage: 0.0,
    //     trajectory: trajectories
    // };
    Ok(overpasses)
}

pub async fn get_overpasses_for_region(region:&Vec<LatLon>, sat_id: u32, max_scan: f64) -> Result<OverpassList>{
    let tle = get_tles_celestrak(sat_id).await?;
    let all_overpasses = compute_full_orbits(tle, max_scan)?;
    let filtered_overpasses = filter_orbits(&all_overpasses, region, max_scan);
    Ok(filtered_overpasses)
}      

pub fn get_overpasses_for_small_region(region:&Vec<LatLon>, overpass_list: &OverpassList, max_scan: f64) -> OverpassList {
    let filtered_overpasses = filter_orbits(overpass_list, region, max_scan);
    filtered_overpasses
}
// idea: every day, for each satellite:
// X 1. pull tle
// X 2. calculate the full orbits for the next 24 hours using propagate and tle
// X 3. convert the trajectories into individual orbit segments for large area https://rhodesmill.org/skyfield/earth-satellites.html
// X 4. convert orbits into ECEF coordiates - could swap with 3
// 5. calculate swath of each orbit from scan angle  + orbit - store as an overpass list
// orbit trajectory request:
// 1. for each orbit segment, check if region falls inside swath - use same covers region function
// 2. store and return relevant orbit segments in overpasslist

/* #endregion overpass calculation functions */