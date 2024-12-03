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

use odin_common::sqrt;
use odin_common::geo::LatLon;
use std::ops::{Add, AddAssign, Mul, MulAssign};
use nav_types::{ECEF, WGS84};
use geo::{GeodesicBearing, GeodesicDestination, Point};

pub struct Cartesian3D {
    pub x: f64,
    pub y: f64,
    pub z: f64
}

impl Cartesian3D {
    pub fn new() -> Self {
        Cartesian3D { 
            x: 0.0,
            y: 0.0,
            z: 0.0
        }
    }
    pub fn from_ecef(ecef:ECEF<f64>) -> Self {
        Cartesian3D {
            x: ecef.x(),
            y: ecef.y(),
            z: ecef.z()
        }
    }
    pub fn from_latlon(latlon: LatLon) -> Self {
        let wgs84 = WGS84::from_degrees_and_meters(latlon.lat_deg, latlon.lon_deg, 0.0);
        let ecef = ECEF::from(wgs84);
        Cartesian3D {
            x: ecef.x(),
            y: ecef.y(),
            z: ecef.z()
        }
    }
    fn mul_f64(&mut self, rhs: f64) {
        self.x *= rhs;
        self.y *= rhs;
        self.z *= rhs;
    }
    pub fn set_to_intersection_with_plane(&mut self, p1: &Cartesian3D, p2: &Cartesian3D, p: &Cartesian3D) {
        self.set_to_cross(&p1, &p2);
        self.scale_to_unit_length();
        let d = - self.dot(&p);
        self.mul_f64(d);
        self.add_assign(&p);
    }
    pub fn set_to_cross(&mut self, p1: &Cartesian3D, p2: &Cartesian3D) {
        self.x = (p1.y * p2.z) - (p1.z * p2.y);
        self.y = (p1.z * p2.x) - (p1.x * p2.z);
        self.z = (p1.x * p2.y) - (p1.y * p2.x);
    }
    pub fn scale_to_unit_length(&mut self) {
        let length = self.length();
        self.x = self.x/length; 
        self.y = self.y/length; 
        self.z = self.z/length; 
    }
    pub fn dot(&self, p: &Cartesian3D) -> f64 {
        (self.x*p.x) + (self.y*p.y) +(self.z*p.z)
    }
    pub fn length(&self) -> f64 {
        sqrt(self.x.powf(2.0) + self.y.powf(2.0) + self.z.powf(2.0))
    }
    pub fn length2(&self) -> f64 {
        self.length().powf(2.0)
    }
    pub fn earth_radius(&self) -> f64 {
        let a2: f64 =  4.0680631590769e13;
        let b2: f64 = 4.04082999828157e13;
        let d2: f64 = self.length2();
        let c0: f64 = self.z.powf(2.0) / d2;
        let c1: f64 = (self.x.powf(2.0) + self.y.powf(2.0))/d2;
        let c2: f64 = (c0/b2) + (c1/a2);
        sqrt(1.0/c2)
    }
    pub fn scale_to_earth_radius(&mut self) {
        self.mul_f64(self.earth_radius()/self.length())
    }
    fn add_assign(&mut self, rhs: &Cartesian3D) {
        self.x += rhs.x;
        self.y += rhs.y;
        self.z += rhs.z;
    }

    pub fn to_ecef(&self) -> ECEF<f64> {
        ECEF::new(self.x, self.y, self.z)
    }

    pub fn to_wgs84(&self) -> WGS84<f64> {
        WGS84::from(self.to_ecef())
    }

}

pub fn lat_lon_from_point(p: Point<f64>) -> LatLon {
    LatLon{lat_deg: p.y(), lon_deg:p.x()}
}