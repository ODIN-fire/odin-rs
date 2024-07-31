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

use nalgebra::Vector3;
use chrono::{DateTime, Duration, TimeDelta, TimeZone, Utc};
use std::vec::Vec;
use nav_types::WGS84;
use serde::{Deserialize,Serialize};
use uom::si::molar_radioactivity::disintegrations_per_minute_per_mole;
use crate::jpss_geo::Cartesian3D;
use crate::errors::*;

#[derive(Serialize,Deserialize,Debug,Clone)]
#[serde(rename_all="camelCase")]
pub struct Overpass {
    sat_id: u32,
    first_date: i64,//DateTime<Utc>,
    last_date: i64,
    coverage: f32,
    trajectory: Vec<Trajectory>
}

#[derive(Serialize,Deserialize,Debug,Clone)]
pub struct Trajectory {
    time: i64,
    x: f64,
    y: f64,
    z: f64
}

#[derive(Serialize,Deserialize,Debug,Clone)]
 pub struct OverpassList {
    pub overpasses: Vec<OrbitalTrajectory>
 }
 impl OverpassList {
    pub fn new() -> Self {
        OverpassList{ overpasses: Vec::new() }
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
    pub sat_id: u32, 
}

impl OrbitalTrajectory{
    pub fn new(length: i32, start_t: DateTime<Utc>, d_t: TimeDelta, sat_id: u32) -> Self {
        OrbitalTrajectory {
            x: Vec::<f64>::with_capacity(length as usize),
            y: Vec::<f64>::with_capacity(length as usize),
            z: Vec::<f64>::with_capacity(length as usize),
            t_start: start_t.clone(),
            t_end: start_t + (d_t*length),
            length: length as usize,
            sat_id: sat_id
        }
    }

    pub fn from_overpass(op: &Overpass, margin: TimeDelta) -> Self {
        let x: Vec<f64> = op.trajectory.iter().map(|x| x.x).collect(); 
        let y: Vec<f64> = op.trajectory.iter().map(|x| x.y).collect(); 
        let z: Vec<f64> = op.trajectory.iter().map(|x| x.z).collect(); 
        OrbitalTrajectory {
            t_end:  Utc.timestamp_millis_opt(op.last_date).unwrap() + margin,
            t_start:  Utc.timestamp_millis_opt(op.first_date).unwrap() - margin,
            length: op.trajectory.len(),
            x: x,
            y: y,
            z: z,
            sat_id: op.sat_id
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