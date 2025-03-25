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

use std::ops::{Add, AddAssign, Mul, MulAssign, Sub, SubAssign};
use nalgebra::{OMatrix,base::{Matrix,ArrayStorage,dimension::{Const,Dyn}}};
use serde::{Serialize,Deserialize};
use crate::geo_constants::{EARTH_RADIUS_RATIO_SQUARED, EQATORIAL_EARTH_RADIUS, E_EARTH_SQUARED, MEAN_EARTH_RADIUS, MER_SQUARED};
use crate::cartographic::Cartographic;

/// note that we do not use uom here to allow for abstract coordinate systems (although
/// it mostly is used for ITRF sysemts)

#[derive(Debug,Clone,Copy,Serialize,Deserialize)]
pub struct Cartesian3 {
    pub x: f64,
    pub y: f64,
    pub z: f64
}

impl Cartesian3 {
    pub fn new (x: f64, y: f64, z: f64)->Cartesian3 {
        Cartesian3{x,y,z}
    }

    pub fn from_column (m: &OMatrix<f64, Const<3>, Dyn>, idx: usize)->Cartesian3 {
        Cartesian3{
            x: m[(0,idx)],
            y: m[(1,idx)],
            z: m[(2,idx)]
        }
    }

    pub fn from_col (m: &Matrix<f64,Const<3>,Const<1>,ArrayStorage<f64,3,1>>)->Cartesian3 {
        Cartesian3{
            x: m[(0,0)],
            y: m[(1,0)],
            z: m[(2,0)]
        }
    }

    pub fn zero ()->Cartesian3 {
        Cartesian3{x: 0.0, y: 0.0, z: 0.0}
    }

    pub fn intersection_with_plane (p1: &Cartesian3, p2: &Cartesian3, p: &Cartesian3) -> Cartesian3 {
        let mut r = Self::cross( &p1, &p2);
        r.scale_to_unit_length();

        let dot = - r.dot(&p);
        r *= dot;
        r += *p;
        r
    }

    /// return if p1 and p2 are on same side of plane given by normal
    /// this returns false if either p1 or p2 is on the plane
    pub fn on_same_side (p1: &Cartesian3, p2: &Cartesian3, normal: &Cartesian3)->bool {
        let s1 = p1.dot(normal);
        let s2 = p2.dot(normal);

        if s1 == 0.0 || s2 == 0.0 { return false }
        (s1.signum() == s2.signum())
    }

    pub fn cross (&self, p: &Cartesian3)->Self {
        Cartesian3 {
            x: (self.y * p.z) - (self.z * p.y),
            y: (self.z * p.x) - (self.x * p.z),
            z: (self.x * p.y) - (self.y * p.x)
        }
    }

    pub fn normal (&self, p: &Cartesian3)->Self {
        let mut n = self.cross(p);
        n.scale_to_unit_length();
        n
    }

    pub fn normals (vs: &Vec<Cartesian3>)->Vec<Cartesian3> {
        let len = vs.len();
        let mut ns: Vec<Cartesian3> = Vec::with_capacity(len);

        for i in 1..len {
            let normal = vs[i-1].normal( &vs[i]);
            ns.push(normal);
        }
        ns.push( vs[len-1].normal( &vs[0]));

        ns
    }

    pub fn scale_to_unit_length(&mut self) {
        let length = self.length();
        self.x = self.x / length;
        self.y = self.y / length;
        self.z = self.z / length;
    }

    pub fn scaled_to_unit_length(&self)->Self {
        let length = self.length();
        self * length
    }

    pub fn dot(&self, p: &Cartesian3) -> f64 {
        (self.x * p.x) + (self.y * p.y) +(self.z * p.z)
    }

    pub fn length(&self) -> f64 {
        ((self.x * self.x) + (self.y * self.y) + (self.z * self.z)).sqrt()
    }

    pub fn length_squared(&self) -> f64 {
        let len = self.length();
        len*len
    }

    pub fn scale_to_earth_radius(&mut self) {
        self.mul_assign (MEAN_EARTH_RADIUS/self.length());
    }

    pub fn to_earth_radius (&self)->Self {
        *self * (MEAN_EARTH_RADIUS/self.length())
    }

    /// return great circle distance of p1 and p2 projected to earth radius
    /// this uses a spherical approximation
    pub fn gc_distance (p1: &Cartesian3, p2: &Cartesian3) -> f64 {
        let p1 = p1.to_earth_radius();
        let p2 = p2.to_earth_radius();

        let dx = p2.x - p1.x;
        let dy = p2.y - p1.y;
        let dz = p2.z - p1.z;

        let d2 = (dx*dx + dy*dy + dz*dz); // spherical cap distance squared

        let a = (1.0 - d2 / MER_SQUARED).acos();
        a * MEAN_EARTH_RADIUS
    }

    /// linear interpolation bewteen two points with factor r ∈ [0..1]
    pub fn linear_interpolation (p1: &Cartesian3, p2: &Cartesian3, r: f64) -> Self {
        *p1 + (p2-p1)*r
    }

    /// answer if point is within open polyhedron that is defined by (inwards pointing) normals, i.e. 
    /// p is on the same side of all bounding planes
    pub fn is_inside_normals (&self, normals: &Vec<Cartesian3>)->bool {
        for i in 0..normals.len() {
            if self.dot(&normals[i]) < 0.0 {
                return false;
            }
        }
        true
    }
}

impl std::fmt::Display for Cartesian3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[ {}, {}, {} ]", self.x, self.y, self.z)
    }
}

impl Add for Cartesian3 {
    type Output = Self;

     fn add (self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z
        }
    }
}

impl Add for &Cartesian3 {
    type Output = Cartesian3;

     fn add (self, rhs: &Cartesian3) -> Cartesian3 {
        Cartesian3 {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z
        }
    }
}

impl AddAssign for Cartesian3 {
     fn add_assign (&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
        self.z += rhs.z;
    }
}

impl Sub for Cartesian3 {
    type Output = Self;

     fn sub (self, rhs: Self) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z
        }
    }
}

impl Sub for &Cartesian3 {
    type Output = Cartesian3;

     fn sub (self, rhs: &Cartesian3) -> Cartesian3 {
        Cartesian3 {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z
        }
    }
}

impl SubAssign for Cartesian3 {
     fn sub_assign (&mut self, rhs: Self)  {
        self.x -= rhs.x;
        self.y -= rhs.y;
        self.z -= rhs.z;
    }
}

impl Mul<f64> for Cartesian3 {
    type Output = Self;

     fn mul (self, rhs: f64) -> Self {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
            z: self.z * rhs
        }
    }
}

impl Mul<f64> for &Cartesian3 {
    type Output = Cartesian3;

     fn mul (self, rhs: f64) -> Cartesian3 {
        Cartesian3 {
            x: self.x * rhs,
            y: self.y * rhs,
            z: self.z * rhs
        }
    }
}

impl MulAssign<f64> for Cartesian3 {
    fn mul_assign (&mut self, rhs: f64) {
        self.x *= rhs;
        self.y *= rhs;
        self.z *= rhs;
    }
}

/// convert WGS84 into ECEF coordinates
impl From<Cartographic> for Cartesian3 {
    fn from(p: Cartographic) -> Self {
        Cartesian3::from(&p)
    }
}

impl From<&Cartographic> for Cartesian3 {
    fn from(p: &Cartographic) -> Self {
        let φ = p.latitude;
        let λ = p.longitude;
        let h = p.height;

        let sin_φ = φ.sin();
        let cos_φ = φ.cos();

        let b = EQATORIAL_EARTH_RADIUS / ( 1.0 - E_EARTH_SQUARED* (sin_φ * sin_φ)).sqrt();
        let c = (b + h)*cos_φ;

        let x = c *  λ.cos();
        let y = c *  λ.sin();
        let z = (EARTH_RADIUS_RATIO_SQUARED * b + h) * sin_φ;

        Cartesian3::new( x, y, z)
    }
}