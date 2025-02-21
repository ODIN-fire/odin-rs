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

//#![allow(unused)]

use nalgebra::{distance, Const, Dyn, Matrix, Rotation3, VecStorage, Vector, Vector3};
use geo::{geodesic_destination, Contains, Destination, Geodesic, GeodesicArea, HausdorffDistance, Point, Polygon, Rect};
use geo::algorithm::BooleanOps;
use chrono::{DateTime, Datelike, Duration, SubsecRound, TimeDelta, TimeZone, Timelike, Utc, NaiveDateTime, NaiveDate};
use odin_common::angle::{Longitude, Latitude};
use odin_common::datetime::naive_utc_date_to_utc_datetime;
use odin_common::geo::{GeoCoord, GeoPoint, GeoPoint3, GeoPolygon, GeoRect};
use sgp4::{Constants, Elements};
use satkit::{Instant, TLE, ITRFCoord, frametransform::{gmst, qteme2itrf}};
use satkit;
use nav_types::{WGS84, ECEF};
use serde_json::Value;
use core::{f64, num};
use itertools::izip;
use std::collections::HashMap;
use std::f64::consts::PI;
use std::vec::Vec;
use serde::{Deserialize,Serialize};
use uom::si::length::meter;
use uom::si::f64::Length;
use reqwest::{Client, Response};
use argminmax::ArgMinMax;
use crate::orbital_geo::Cartesian3D;
use crate::errors::*;

/* #region overpass data structures  ***************************************************************************/

#[derive(Serialize,Deserialize,Debug,Clone)]
#[serde(rename_all="camelCase")]
pub struct Overpass {
    pub sat_id: i32,
    pub first_date: i64, //unix timestamp,
    pub last_date: i64, //unix timestamp,
    pub coverage: f32,
    pub swath: f64,
    pub max_scan: f64,
    pub trajectory: Vec<Trajectory>
}

impl Overpass {
    pub fn new(sat_id: i32, max_scan:f64, trajectory:Vec<Trajectory>) -> Self {
        let last_date = trajectory[trajectory.len()-1].time;
        let first_date = trajectory[0].time;
        let coverage = 0.0;
        let swath = get_swath_for_orbit(&trajectory, max_scan).value;
        Overpass {
            sat_id, first_date, last_date, coverage, swath, max_scan, trajectory
        }
    }

    pub fn set_coverage (&mut self, region: &GeoRect) {// calculate coverage of overpass over region
        // get polygon of overpass+swath
        let overpass = self.get_overpass_bounds().to_polygon();
        // get intersection of region and overpass+swath
        let intersections = &overpass.intersection(&region.to_polygon()).0;
        if intersections.len()>0 {
            let intersection = &overpass.intersection(&region.to_polygon()).0[0];
            // area of intersection/ area of region
            self.coverage = (((intersection.geodesic_area_unsigned()/ region.area().value)*100.0).round()/100.0) as f32;
        }
    }

    pub fn set_swath (&mut self) {
        let swath = get_swath_for_orbit(&self.trajectory, self.max_scan);
        self.swath = swath.value;
    }

    pub fn get_overpass_bounds(&self ) -> GeoRect {
        // n,s,e,w
        let first_geo_pt3 = self.trajectory[0].as_wgs84();
        let first_geo_pt = GeoPoint::from_lon_lat(first_geo_pt3.longitude(), first_geo_pt3.latitude());
        let n = self.trajectory[0].as_wgs84().latitude();
        let s =  self.trajectory[self.trajectory.len()-1].as_wgs84().latitude();
        let w = Longitude::from_degrees(Geodesic::destination(first_geo_pt.point().clone(), 270.0, self.swath).x());
        let e = Longitude::from_degrees(Geodesic::destination(first_geo_pt.point().clone(), 90.0, self.swath).x());
        GeoRect::from_wsen(w, s, e, n)
    }

    pub fn update(&mut self, i: usize, vec3:Vector3<f64>) {
        self.trajectory[i].x = vec3.x;
        self.trajectory[i].y = vec3.y;
        self.trajectory[i].z = vec3.z;
    }

    pub fn find_closest_ground_track_point(&self, p: &Cartesian3D) -> Cartesian3D {
        let mut gp = self.find_closest_orbit_point(p);
        gp.scale_to_earth_radius(); // scale to earth radius
        gp
    }

    pub fn find_closest_orbit_point(&self, p: &Cartesian3D) -> Cartesian3D {
        let i = self.find_closest_index(p);
        if i < self.trajectory.len() { // not the last point
            let p1 = Cartesian3D{x: self.trajectory[i-1].x, y: self.trajectory[i-1].y, z: self.trajectory[i-1].z};
            let p2 = Cartesian3D{x: self.trajectory[i+1].x, y: self.trajectory[i+1].y, z: self.trajectory[i+1].z};
            let mut gp = Cartesian3D::new();
            gp.set_to_intersection_with_plane(&p1, &p2, p); // set pt to intersection w/ plane
            gp
        } else { // last point in trajectory, edge case that causes panicking if not handled
            let p1 = Cartesian3D{x: self.trajectory[i-1].x, y: self.trajectory[i-1].y, z: self.trajectory[i-1].z};
            let p2 = Cartesian3D{x: self.trajectory[i].x, y: self.trajectory[i].y, z: self.trajectory[i+1].z};
            let mut gp = Cartesian3D::new();
            gp.set_to_intersection_with_plane(&p1, &p2, p); // set pt to intersection w/ plane
            gp
        }
        
    }

    pub fn dist2(&self, i:usize, p: &Cartesian3D) -> f64 {
        ((self.trajectory[i].x-p.x).powf(2.0)) + ((self.trajectory[i].y-p.y).powf(2.0)) +((self.trajectory[i].z-p.z).powf(2.0))
    }

    pub fn find_closest_index(&self, p: &Cartesian3D) -> usize {
        let mut l = 1;
        let mut r = self.trajectory.len()-2; // can cause panicking if len<2
        let mut i = r/2; // sets up binary search
        let mut dl = self.dist2(i, p) - self.dist2(i-1, p); // cause panicking if len<3
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
    pub fn filter_orbit_points(&mut self, region:&GeoRect) {
        let max_north = GeoPoint::from_lon_lat(region.west(),region.north()).as_ecef().z();
        let min_south =  GeoPoint::from_lon_lat(region.west(),region.south()).as_ecef().z();
        let mut inds_to_remove = vec![];
        for (index, value) in self.trajectory.iter().enumerate() {
            if value.z < min_south || value.z > max_north {
                inds_to_remove.push(index);
            }
        }
        inds_to_remove.sort();
        inds_to_remove.reverse();
        for i in inds_to_remove.into_iter() {
            self.trajectory.remove(i);
        }
    }
}

#[derive(Serialize,Deserialize,Debug,Clone, PartialEq)]
pub struct Trajectory {
    pub time: i64,
    pub x: f64,
    pub y: f64,
    pub z: f64
}

impl Trajectory {
    pub fn new(ecef: ECEF<f64>, time: &DateTime<Utc>) -> Self {
        Trajectory{
            time: time.timestamp_millis(), 
            x: ecef.x(),
            y: ecef.y(),
            z: ecef.z()
        }
    }
    pub fn as_ecef(&self) -> ECEF<f64> {
        ECEF::new(self.x, self.y, self.z)
    }
    pub fn as_wgs84(&self) -> GeoPoint3 {
        GeoPoint3::from(self.as_ecef())
    }
}

#[derive(Serialize,Deserialize,Debug,Clone)]
 pub struct OverpassList {
    pub overpasses: Vec<Overpass>
 }
 impl OverpassList {
    pub fn new() -> Self {
        OverpassList{ overpasses: Vec::new() }
    }

    pub fn from_overpasses( overpasses: Vec<Overpass> ) -> Self {
        OverpassList{ overpasses }
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
            let end = Utc.timestamp_millis_opt(op.last_date.clone()).unwrap();
            dates.push(end)
        }
        dates.sort();
        dates
    }

    pub fn get_start_dates(&self) -> Vec<DateTime<Utc>> {
        let mut dates = Vec::new();
        for op in self.overpasses.iter() {
            let start = Utc.timestamp_millis_opt(op.first_date.clone()).unwrap();
            dates.push(start);
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

pub fn get_overpass_for_date(date:&DateTime<Utc>, overpass_list: &OverpassList) -> Result<Overpass> {
    let date = date.clone().timestamp_millis();
    let mut tp:Option<Cartesian3D> = None;
    let mut overpass_for_pt = None;
    for overpass in overpass_list.overpasses.iter() {
        if (overpass.last_date >= date) & (overpass.first_date <= date) {
            overpass_for_pt = Some(overpass);
            break
        }
    }
    if overpass_for_pt.is_none() { // not mapped properly - need to get most recent orbit
        let time_dist: Vec<i64> = overpass_list.overpasses.iter().map(|x| (date - x.last_date).abs()). collect();
        let (min, max) = time_dist.argminmax();
        overpass_for_pt = Some(&overpass_list.overpasses[min]);
    }
    if let Some(overpass) = overpass_for_pt {
        Ok(overpass.clone())
    } else {
        Err(OdinOrbitalSatError::MiscError(String::from("No overpass for ground point")))
    }
}

pub fn get_trajectory_point(point: &Cartesian3D, date:&DateTime<Utc>, overpass_list: &OverpassList) -> Result<Option<Cartesian3D>> {
    let overpass = get_overpass_for_date(date, overpass_list)?;
    let tp = Some(overpass.find_closest_ground_track_point(point));
    Ok(tp)
}

/* #endregion overpass data structure */

/* #region TLE import functions */

pub async fn get_celestrak_response(sat_id: u32) -> Result<Response> {
    let client = Client::new();
    let sat_id_str = sat_id.clone().to_string();
    let query = vec![("CATNR", sat_id_str.as_str()),("FORMAT", "json")];
    let response = client.get("https://celestrak.com/NORAD/elements/gp.php")
            .query(&query).send().await?;
    Ok(response)
}

pub async fn get_tles_celestrak(sat_id: u32) -> Result<TLE>{
    let client = Client::new();
    let sat_id_str = sat_id.clone().to_string();
    let query = vec![("CATNR", sat_id_str.as_str()),("FORMAT", "TLE")];
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
                    Err(OdinOrbitalSatError::TleError(format!("Satkit TLE import failed {:?}", err)))
                }
            }
        } else if lines.len() == 3 {
            let tle_res =  TLE::load_3line(lines[0], lines[1], lines[2]);
            match tle_res {
                Ok(tle) => {
                    Ok(tle)
                }
                Err(err) => {
                    Err(OdinOrbitalSatError::TleError(format!("Satkit TLE import failed {:?}", err)))
                }
            }
        } else {  Err(OdinOrbitalSatError::TleError(format!("Inncorrect TLE lines {:?}", lines.len())))}
    } else {
        Err(OdinOrbitalSatError::FileDownloadError(format!("TLE download failed: {:?}", response.status())))
    } 
}

pub async fn get_spacetrack_request(sat_id: u32, username: &str, password:&str) -> Result<Response> {
    let client = Client::new();
    let mut form = HashMap::new();
    form.insert("identity", username);
    form.insert("password", password);

    let url = format!("https://www.space-track.org/basicspacedata/query/class/gp/NORAD_CAT_ID/{}/format/json", sat_id);
    form.insert("query", url.as_str());
    let response = client.post("https://www.space-track.org/ajaxauth/login").form(&form).send().await?;
    Ok(response)
}

pub async fn get_tles_spacetrack(sat_id: u32, username: &str, password:&str) -> Result<TLE>{
    let response = get_spacetrack_request(sat_id, username, password).await?;
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
                    Err(OdinOrbitalSatError::TleError(format!("Satkit TLE import failed {:?}", err)))
                }
            }
        } else if lines.len() == 3 {
            let tle_res =  TLE::load_3line(lines[0], lines[1], lines[2]);
            match tle_res {
                Ok(tle) => {
                    Ok(tle)
                }
                Err(err) => {
                    Err(OdinOrbitalSatError::TleError(format!("Satkit TLE import failed {:?}", err)))
                }
            }
        } else { Err(OdinOrbitalSatError::TleError(format!("Inncorrect TLE lines {:?}", lines.len())))}
    } else {
        Err(OdinOrbitalSatError::FileDownloadError(format!("TLE download failed: {:?}", response.status())))
    } 
}
/* #endregion TLE import functions */

/* #region overpass calculation functions  ***************************************************************************/

pub fn compute_full_orbits(mut tle: TLE, max_scan: f64, region: &GeoRect) -> Result<OverpassList> {
    let times = get_time_vector();
    compute_full_orbits_from_times(tle, max_scan, times, region)
}

pub fn compute_full_orbits_from_times(mut tle: TLE, max_scan: f64, times: Vec<DateTime<Utc>>, region: &GeoRect) -> Result<OverpassList> {
    let ats: Vec<Instant> = times.iter().map(|x| utc_to_instant(x)).collect();
    let (pred_teme, _, _) = satkit::sgp4::sgp4(&mut tle, &ats[..]);
    let overpass = format_prediction(pred_teme, times, ats, tle, max_scan, region)?;
    Ok(overpass)
}

pub fn get_init_times_vector(history: Duration) -> Vec<DateTime<Utc>> {
    let now = Utc::now();
    // start = now - history
    let start = now - TimeDelta::seconds(history.num_seconds());
    let total_steps = (history.num_seconds() + TimeDelta::hours(24).num_seconds())/5;
    let mut times:Vec<DateTime<Utc>> = vec![];
    let mut now_mut = start.round_subsecs(0).clone();
    times.push(now_mut);
    for i in 1..total_steps {
        now_mut = now_mut + TimeDelta::seconds(5);
        times.push(now_mut);
    }
    times
}

pub fn compute_initial_orbits(mut tle: TLE, max_scan: f64, history: Duration, region: &GeoRect) -> Result<OverpassList> {
    let now = Utc::now();
    let times = get_init_times_vector(history); 
    // println!("init times: {:?}, {:?}", times[0], times[times.len()-1]);
    let ats: Vec<Instant> = times.iter().map(|x| utc_to_instant(x)).collect();
    let spg4_now = Utc::now();
    let (pred_teme, _, _) = satkit::sgp4::sgp4(&mut tle, &ats[..]);
    // println!("compute initial orbit spg4 algo time: {:?}", Utc::now()-spg4_now);
    let format_now = Utc::now();
    let overpass = format_prediction(pred_teme, times, ats, tle, max_scan, region)?;
    // println!("compute initial orbit format time: {:?}", Utc::now()-format_now);
    // println!("compute initial orbit time: {:?}", Utc::now()-now);
    Ok(overpass)
}

pub fn compute_approximate_swath_width(altitude: Length, max_scan: f64) -> Length {
    let scan = max_scan*PI/180.0;
    let earth = Length::new::<meter>(6371000.0);
    let d = earth + altitude;
    let c0 = f64::sin(scan)/earth; 
    let c1 = earth.value.powf(2.0) - d.value.powf(2.0);
    // val c1 = squared(r) - squared(d)
    let c2 = d*f64::cos(scan);
    let a = c2.value - (c2.value.powf(2.0)+c1).sqrt();
    let alpha = (c0.value*a).asin();
    Length::new::<meter>(earth.value*alpha)
}

fn get_average_altitude(traj: &Vec<Trajectory>) -> f64{
    let p1 = Cartesian3D::from_ecef(ECEF::new(traj[0].x, traj[0].y, traj[0].z)).to_wgs84();
    let p2_ind = traj.len()-1;
    let p2 = Cartesian3D::from_ecef(ECEF::new(traj[p2_ind].x, traj[p2_ind].y, traj[p2_ind].z)).to_wgs84();
    (p1.altitude() + p2.altitude()) / 2.0
}

fn get_swath_for_orbit(traj: &Vec<Trajectory>, max_scan: f64) -> Length {
    let altitude = get_average_altitude(traj);
    compute_approximate_swath_width(Length::new::<meter>(altitude), max_scan)
}

pub fn get_time_vector() -> Vec<DateTime<Utc>> {
    let now = Utc::now();
    let now_round = now.round_subsecs(0);
    let num_steps = TimeDelta::hours(24).num_seconds()/5;
    let mut times:Vec<DateTime<Utc>> = vec![];
    let mut now_mut = now_round.clone();
    for i in 1..num_steps {
        now_mut = now_mut + TimeDelta::seconds(1);
        times.push(now_mut);
    }
    times
}

pub fn get_time_vector_from_start_end(start: DateTime<Utc>, end:DateTime<Utc>) -> Vec<DateTime<Utc>> {
    let delta = (end-start)/5;
    let mut times:Vec<DateTime<Utc>> = vec![];
    let mut now_mut = start.clone();
    times.push(now_mut);
    for i in 1..delta.num_seconds(){
        now_mut = now_mut + TimeDelta::seconds(5);
        times.push(now_mut);
    }
    times.push(end);
    times
}

pub fn utc_to_instant(time: &DateTime<Utc>) -> Instant{
    Instant::from_unixtime(time.timestamp() as f64)
}

pub fn convert_pred(pred: [f64;3] , time: &DateTime<Utc>) -> ECEF<f64> {
    let at = utc_to_instant(time);
    let itrf: Matrix<f64, Const<3>, Const<1>, nalgebra::ArrayStorage<f64, 3, 1>> = Rotation3::<f64>::from_matrix(qteme2itrf(&at).to_rotation_matrix().matrix()) *  Vector3::new(pred[0], pred[1], pred[2]);
    let itrf_coord = ITRFCoord::from_slice(&itrf.as_slice()).unwrap();
    ECEF::new(itrf_coord.itrf[0], itrf_coord.itrf[1], itrf_coord.itrf[2])
}

fn coverable_region(region: &GeoRect, max_scan: f64) -> bool {
    // if distance between edges are within max_scan, then reurn true
    let width = (region.west().degrees() - region.east().degrees()).abs();
    if width > max_scan {
        false
    } else {
        true
    }
}

pub fn filter_orbits(overpass_list: &OverpassList,  region: &GeoRect, max_scan: f64) -> OverpassList{
    let mut filtered_orbits: Vec<Overpass> = vec![];
    let coverable_region = coverable_region(region, max_scan);
    for mut overpass in overpass_list.overpasses.clone().into_iter(){
        // if (coverable_region) {
        //     println!("coverable");
        //     if covers_region(&overpass, region, max_scan) {
        //         filtered_orbits.push(overpass);
        //         println!("covers region");
        //     }
        // } else {
            if covers_region_partial(&overpass, region, max_scan) { // need to actually reduce orbit 
                overpass.set_coverage(&region);
                //overpass.filter_orbit_points(region);
                filtered_orbits.push(overpass);
            }
        //}
        
    }
    OverpassList { overpasses: filtered_orbits }
}

pub fn covers_region(overpass: &Overpass, region: &GeoRect, max_scan: f64) -> bool { // must cover entire region to be considered an overpass
    let mut covers = true;
    for vertex in region.points(){
        let point = Cartesian3D::from_latlon(vertex.clone());
        let mut orbit_point = overpass.find_closest_orbit_point(&point);
        let dist_to_earth = orbit_point.z;
        let max_scan_m = scan_angle_to_meters(max_scan, dist_to_earth);
        orbit_point.scale_to_earth_radius();
        let distance = orbit_point.to_wgs84().distance(&point.to_wgs84()); // uses great circle distance
        if (distance <= (max_scan_m/2.0)) {
            covers = true;
        } else {
            covers = false;
            break
        }
    }
    covers
}

pub fn scan_angle_to_meters(max_scan: f64, dist_to_earth: f64) -> f64{
    dist_to_earth * (f64::tan((max_scan/ 2.0)*PI/180.0) * 2.0)
}

pub fn covers_region_partial(overpass: &Overpass, region: &GeoRect, max_scan: f64) -> bool { // for cases when the region is too large to fully fit in a single overpass
    // must cover one vertex - issue for overpass enirely in one region 
    let mut covers = true; // update with interpolation
    for vertex in region.points(){
        let point = Cartesian3D::from_latlon(vertex.clone());
        let mut orbit_point = overpass.find_closest_orbit_point(&point);
        let dist_to_earth = orbit_point.z;
        let max_scan_m = scan_angle_to_meters(max_scan, dist_to_earth);
        orbit_point.scale_to_earth_radius();
        let distance = orbit_point.to_wgs84().distance(&point.to_wgs84()); // uses great circle distance
        if (distance <= (max_scan_m/2.0)) {
            covers = true;
            break
        } else {
            covers = false;
        }
    }
    if (covers == false) { // check if it is entirely contained in the region 
        // get mid point of orbit between the region lats
        let lat_mid = region.north().degrees().min(region.south().degrees()) + (region.north().degrees() - region.south().degrees()).abs()/2.0;
        // get mid point of orbit between the region lons
        let lon_max = region.west().degrees().max(region.east().degrees()); 
        let lon_min =  region.west().degrees().min(region.east().degrees()); 
        let lon_mid = region.west().degrees().min(region.east().degrees()) + (region.west().degrees() - region.east().degrees()).abs()/2.0;
        // check if lat lon is contained in the region bounds
        let pt = GeoPoint::from_lon_lat(Longitude::from_degrees(lon_mid), Latitude::from_degrees(lat_mid));
        let orbit_pt = overpass.find_closest_ground_track_point(&Cartesian3D::from_latlon(pt)).to_wgs84();
        let orbit_lon = orbit_pt.longitude_degrees();        
        if (orbit_lon <= lon_max) & (orbit_lon >= lon_min) {
            covers = true; // orbit goes between the region lon bounds, do not need to worry about lat 
        }
    }
    covers
}


pub fn convert_preds(preds:Matrix<f64, Const<3>, Dyn, VecStorage<f64, Const<3>, Dyn>> , ats: Vec<Instant>) -> Result<Vec<ECEF<f64>>> {
    let now = Utc::now();
    let rots: Vec<nalgebra::Rotation<f64, 3>> = ats.iter().map(|at| Rotation3::<f64>::from_matrix(qteme2itrf(at).to_rotation_matrix().matrix())).collect();
    // println!("time to get rotation mats: {:?}", Utc::now()-now);
    let itrf_now = Utc::now();
    let itrf_coords:Vec<ITRFCoord> = rots.into_iter().zip(preds.column_iter()).map(|(rot, pred)| ITRFCoord::from_slice(&(rot*pred).as_slice()).unwrap()).collect();
    // println!("time to get itfr: {:?}", Utc::now()-itrf_now);
    let ecef_now = Utc::now();
    let ecef_coords:Vec<ECEF<f64>> = itrf_coords.into_iter().map(|itrf_coord|ECEF::new(itrf_coord.itrf[0], itrf_coord.itrf[1], itrf_coord.itrf[2])).collect();
    // println!("time to get ecef from itfr: {:?}", Utc::now()-ecef_now);
    Ok(ecef_coords)
}

pub fn in_region(ecef: ECEF<f64>, region: &GeoRect, max_scan: f64) -> bool {
    let region_poly = region.to_polygon(); // creates geopolygon for checkin point inclusion
    let pt_wgs84 = GeoPoint3::from(ecef);
    let pt_lat_lon = Point::new( pt_wgs84.longitude().degrees(),pt_wgs84.latitude().degrees());
    let swath = compute_approximate_swath_width(pt_wgs84.altitude(), max_scan);
    let west_edge = Geodesic::destination(pt_lat_lon, 270.0, swath.value);
    let east_edge =  Geodesic::destination(pt_lat_lon, 90.0, swath.value);
    (region_poly.contains(&pt_lat_lon)) |(region_poly.contains(&west_edge)) |(region_poly.contains(&east_edge))
}
pub fn format_prediction(preds: Matrix<f64, Const<3>, Dyn, VecStorage<f64, Const<3>, Dyn>>, times: Vec<DateTime<Utc>>, ats:Vec<Instant>, tle: TLE, max_scan: f64, region: &GeoRect) -> Result<OverpassList> {
    //let times = get_time_vector();
    let mut trajectories: Vec<Trajectory> = vec![];
    let mut overpasses: Vec<Overpass> = vec![];
    let mut in_current_trajectory = false;
    let mut first_time = times[0].clone();
    let mut last_time = times[times.len()-1].clone();
    let preds_ecef = convert_preds(preds, ats)?;
    // println!("convert preds time: {:?}", Utc::now()-now);
    for (ecef, time) in preds_ecef.into_iter().zip(times.iter()) {
        // get ecef into lat,lon ground point, 
        let in_region = in_region(ecef, region, max_scan);
        //println!("inregion:{}", in_region);
        if in_region { // check if it is in region, if in region then save
            // take in full region, check if point or swath edge points are in the region 
            if (in_current_trajectory == false) { // first point in new trajectory
                in_current_trajectory = true;
                first_time = time.clone();
            }
            let traj = Trajectory::new(ecef, time);
            trajectories.push(traj);
            //last_time = time.clone();
        } else {
            if in_current_trajectory { // first point outside of trajectory - save completed trajectory
                // convert to overpass
                let overpass = Overpass::new(tle.sat_num, max_scan, trajectories);
                overpasses.push(overpass);
                // reset trajectories
                trajectories = vec![];

            }
            in_current_trajectory = false
        }
    }
    // save remaining trajectory to an overpass if propogation ends in the middle of an overpass
    if trajectories.len() > 0 {
        let overpass = Overpass::new(tle.sat_num, max_scan, trajectories);
        overpasses.push(overpass);
    }
    let overpass_list = OverpassList::from_overpasses(overpasses);
    Ok(overpass_list)
}      

pub fn get_overpasses_for_small_region(region:&GeoRect, overpass_list: &OverpassList, max_scan: f64) -> OverpassList {
    let filtered_overpasses = filter_orbits(overpass_list, region, max_scan);
    filtered_overpasses
}

/* #endregion overpass calculation functions */