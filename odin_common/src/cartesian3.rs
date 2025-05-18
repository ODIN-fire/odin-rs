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

use std::{f64::{consts::PI, NAN}, ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign}};
use nalgebra::{OMatrix,base::{Matrix,ArrayStorage,dimension::{Const,Dyn}}};
use serde::{Serialize,Deserialize,ser::{Serializer,SerializeStruct},de::Deserializer};
use crate::geo_constants::{
    EARTH_RADIUS_RATIO_SQUARED, EQUATORIAL_EARTH_RADIUS, EQUATORIAL_EARTH_RADIUS_SQUARED, E_EARTH_SQUARED, MEAN_EARTH_RADIUS, MER_SQUARED, POLAR_EARTH_RADIUS_SQUARED
};
use crate::cartographic::Cartographic;
use crate::{pow2,sqrt,signum, atan, atan2,cos,sin};
use crate::json_writer::{JsonWritable,JsonWriter};

/// note that we do not use uom here to allow for abstract coordinate systems (although
/// it mostly is used for ITRF sysemts)

/// f64 based 3-dimensional cartesian
/// This is used for geometric computations where we know units
/// fields have to have the same names as Cesium.Cartesian3 so that we can serialize/deserialize JSON without additional objects
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

    pub fn undefined ()->Cartesian3 {
        Cartesian3{ x: NAN, y: NAN, z: NAN }
    }

    pub fn set_undefined (&mut self) {
        self.x = NAN; self.y = NAN; self.z = NAN;
    }

    #[inline]
    pub fn is_undefined (&self)->bool {
        self.x.is_nan() || self.y.is_nan() || self.z.is_nan() 
    }

    #[inline]
    pub fn is_defined (&self)->bool {
        !self.is_undefined() 
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

    /// Note - this assumes the first/last point is NOT duplicated 
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

    pub fn to_unit (&self)->Cartesian3 {
        let length = self.length();
        Cartesian3 { x: self.x / length, y: self.y / length, z: self.z / length }
    }

    pub fn scaled_to_unit_length(&self)->Self {
        let length = self.length();
        self / length
    }

    pub fn angle_between ( p1: &Cartesian3, p2: &Cartesian3) -> f64 {
        let u1 = p1.scaled_to_unit_length();
        let u2 = p2.scaled_to_unit_length();

        if u1.dot( &u2) < 0.0 {
            PI - 2.0 * ((u1 + u2).length()/2.0).asin()
        } else {
            2.0 * ((u1 - u2).length()/2.0).asin()
        }
    }

    pub fn dot(&self, p: &Cartesian3) -> f64 {
        (self.x * p.x) + (self.y * p.y) +(self.z * p.z)
    }

    pub fn length(&self) -> f64 {
        ((self.x * self.x) + (self.y * self.y) + (self.z * self.z)).sqrt()
    }

    /// return new rounded Cartesian3
    pub fn to_rounded_decimals (&self, n: u8)->Self {
        if n > 0 {
            let s = (10f64).powi(n as i32);
            Cartesian3 { 
                x: (self.x * s).round() / s, 
                y: (self.y * s).round() / s, 
                z: (self.z * s).round() / s
            }
        } else {
            Cartesian3 {
                x: self.x.round(),
                y: self.y.round(),
                z: self.z.round()    
            }
        }
    }

    /// round this Cartesian3
    pub fn round_to_decimals (&mut self, n: u8) {
        if n > 0 {
            let s = (10f64).powi(n as i32);
            self.x = (self.x * s).round() / s;
            self.y = (self.y * s).round() / s;
            self.z = (self.z * s).round() / s;
            
        } else {
            self.x = self.x.round();
            self.y = self.y.round();
            self.z = self.z.round();
        }
    }

    pub fn length_squared(&self) -> f64 {
        let len = self.length();
        len*len
    }

    pub fn scale_to_length(&mut self, len: f64) {
        self.mul_assign (len/self.length());
    } 

    pub fn scale_to_mean_earth_radius(&mut self) {
        self.scale_to_length(MEAN_EARTH_RADIUS);
    }

    pub fn scale_to_earth_radius (&mut self) {
        self.mul_assign( self.earth_radius()/self.length());
    }

    pub fn to_mean_earth_radius (&self)->Self {
        self * (MEAN_EARTH_RADIUS / self.length())
    }

    pub fn to_earth_radius (&self)->Self {
        self * (self.earth_radius()/self.length())
    }

    pub fn to_length (&self, len: f64)->Self {
        *self * (len/self.length())
    } 

    pub fn extended_by_length (&self, l: f64)->Self {
        let length = self.length();
        *self * ((length + l) / length) 
    }

    /// return great circle distance of p1 and p2 projected to earth radius
    /// this uses a spherical approximation
    pub fn gc_distance (p1: &Cartesian3, p2: &Cartesian3) -> f64 {
        let p1 = p1.to_mean_earth_radius();
        let p2 = p2.to_mean_earth_radius();

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
    /// p is on the same side of all bounding planes.
    /// Note this requires the polyhedron outside planes to define convex polygons
    pub fn is_inside_normals (&self, normals: &Vec<Cartesian3>)->bool {
        let len = normals.len();
        for i in 0..len {
            if self.dot(&normals[i]) < 0.0 {
                return false;
            }
        }
        true
    }

    pub fn earth_radius (&self)->f64 {
        let d2 = self.length().powi(2);
        let c0 = (self.z.powi(2)) / d2;
        let c1 = ((self.x.powi(2)) + (self.y.powi(2))) / d2;
        let c2 = c0 / POLAR_EARTH_RADIUS_SQUARED + c1 / EQUATORIAL_EARTH_RADIUS_SQUARED;
    
        sqrt(1.0/c2)
    }

    pub fn closest_point_on_plane (&self, p: &Cartesian3, q: &Cartesian3)->Self {
        let mut r = p.cross(q);        
        r.scale_to_unit_length();

        let dist = r.dot(self); // project self onto normal
        r *= dist;
        self - r
    }

    /// use only as approximation if geodetic coordinates are required
    pub fn cartesian_to_spherical (&self)->Cartographic {
        let longitude = atan2(self.y, self.x);
        let latitude = atan( self.z / sqrt( pow2(self.x) + pow2(self.y)));
        Cartographic { longitude, latitude, height: 0.0 }
    }

    /// compute the east and north facing unit vectors for the given point on a sphere
    pub fn en_units (&self)->(Cartesian3,Cartesian3,Cartesian3) {
        let length = self.length();
        let unit = Cartesian3 { x: self.x / length, y: self.y / length, z: self.z / length }; // own unit

        let cos_alpha = unit.dot( &Z_UNIT); // angle between self and z-axis
        let d = length / cos_alpha;
        let north_unit = (Cartesian3 { x: 0.0, y: 0.0, z: d } - self).scaled_to_unit_length();
        let east_unit = unit.cross( &north_unit);

        (unit, east_unit, north_unit)
    }

    // rotate this point around the given unit_normal
    pub fn rotate_around (&self, u_axis: &Cartesian3, radians: f64)->Cartesian3 {
        let a2 = radians/2.0;
        let cos_a2 = cos(a2);
        let sin_a2 = sin(a2);
        let b = 2.0 * cos_a2 * sin_a2;
        let c = 2.0 * sin_a2 * sin_a2;

        let uxp = u_axis.cross(self);
        let r = *self + (uxp * b) + (u_axis.cross(&uxp) * c);
        r
    }

    /// rotate all points of the given (mutable) Cartesian3 slice around axis unit vector u_axis with rotation angle radians
    /// this uses the quaternion based equation
    ///    r = p + 2*cos(a/2)sin(a/2)(u × p) + 2*sin²(a/2) u × (u × p)
    pub fn rotate_all (u_axis: &Cartesian3, radians: f64, points: &mut[Cartesian3]) {
        let a2 = radians/2.0;
        let cos_a2 = cos(a2);
        let sin_a2 = sin(a2);
        let b = 2.0 * cos_a2 * sin_a2;
        let c = 2.0 * sin_a2 * sin_a2;

        for p in points {
            let uxp = u_axis.cross(p);
            *p = *p + (uxp * b) + (u_axis.cross(&uxp) * c); 
        }
    }

    pub fn round_all (points: &mut[Cartesian3], n_digits: u8) {
        for p in points {
            p.round_to_decimals(n_digits);
        }
    }
}

pub const X_UNIT: Cartesian3 = Cartesian3 { x: 1.0, y: 0.0, z: 0.0 };
pub const Y_UNIT: Cartesian3 = Cartesian3 { x: 0.0, y: 1.0, z: 0.0 };
pub const Z_UNIT: Cartesian3 = Cartesian3 { x: 0.0, y: 0.0, z: 1.0 };

/// return closest and second closest index pair of vertices to given point
/// note the second index can either be the same (exact match), lower (intersection to the left) or 
/// higher (intersection to the right)  
pub fn find_closest_index (ps: &[Cartesian3], p: &Cartesian3) -> usize {
    let len = ps.len();

    // corner cases
    if len == 0 { panic!("no vertices") }
    if len == 1 { return 0 } // only choice
    if len == 2 { return if dist_squared( &ps[1], p) > dist_squared( &ps[0], p) { 0 } else { 1 } }

    let mut l = 1;
    let mut r = len-2;
    let mut i = r/2;

    let mut di = dist_squared( &ps[i], p);
    let mut dl = di - dist_squared( &ps[i-1], p);
    let mut dr = dist_squared( &ps[i+1], p) - di;

    while signum(dl) == signum(dr) {
        if dr < 0.0 {  // bisect right
            l = i;
          } else {  // bisect left
            r = i;
          }
          let i_last = i;
          i = (l + r)/2;
          if i == i_last { break; }
    
          di = dist_squared( &ps[i], p);
          dl = di - dist_squared( &ps[i-1], p);
          dr = dist_squared( &ps[i+1], p) - di;
    }

    i
}

#[inline]
pub fn dist_squared (p: &Cartesian3, q:&Cartesian3) -> f64 {
    let d = p - q;
    pow2(d.x) + pow2(d.y) + pow2(d.z)
}

pub fn scale_to_earth_radius (ps: &mut[Cartesian3]) {
    for p in ps.iter_mut() {
        p.scale_to_earth_radius();
    }
}

impl JsonWritable for Cartesian3 {
    /// note this is a lossy implementation as we round to integer (assuming the underlying unit is meter)
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object( |w| {
            w.write_field("x", self.x.round() as i64);
            w.write_field("y", self.y.round() as i64);
            w.write_field("z", self.z.round() as i64);
        });
    }

    fn estimated_length (&self)->usize { 64 }
}

impl std::fmt::Display for Cartesian3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[ {}, {}, {} ]", self.x, self.y, self.z)
    }
}

impl Add<Cartesian3> for Cartesian3 {
    type Output = Cartesian3;

     fn add (self, rhs: Cartesian3) -> Cartesian3 {
        Cartesian3 {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z
        }
    }
}

impl Add<&Cartesian3> for Cartesian3 {
    type Output = Cartesian3;

     fn add (self, rhs: &Cartesian3) -> Cartesian3 {
        Cartesian3 {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z
        }
    }
}

impl Add<&Cartesian3> for &Cartesian3 {
    type Output = Cartesian3;

     fn add (self, rhs: &Cartesian3) -> Cartesian3 {
        Cartesian3 {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z
        }
    }
}

impl Add<Cartesian3> for &Cartesian3 {
    type Output = Cartesian3;

     fn add (self, rhs: Cartesian3) -> Cartesian3 {
        Cartesian3 {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z
        }
    }
}

impl AddAssign<Cartesian3> for Cartesian3 {
     fn add_assign (&mut self, rhs: Cartesian3) {
        self.x += rhs.x;
        self.y += rhs.y;
        self.z += rhs.z;
    }
}

impl AddAssign<&Cartesian3> for Cartesian3 {
    fn add_assign (&mut self, rhs: &Cartesian3) {
       self.x += rhs.x;
       self.y += rhs.y;
       self.z += rhs.z;
   }
}

impl Sub<Cartesian3> for Cartesian3 {
    type Output = Cartesian3;

     fn sub (self, rhs: Cartesian3) -> Cartesian3 {
        Cartesian3 {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z
        }
    }
}

impl Sub<&Cartesian3> for Cartesian3 {
    type Output = Cartesian3;

     fn sub (self, rhs: &Cartesian3) -> Cartesian3 {
        Cartesian3 {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z
        }
    }
}

impl Sub<&Cartesian3> for &Cartesian3 {
    type Output = Cartesian3;

     fn sub (self, rhs: &Cartesian3) -> Cartesian3 {
        Cartesian3 {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z
        }
    }
}

impl Sub<Cartesian3> for &Cartesian3 {
    type Output = Cartesian3;

     fn sub (self, rhs: Cartesian3) -> Cartesian3 {
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

impl Div<f64> for Cartesian3 {
    type Output = Self;

     fn div (self, rhs: f64) -> Self {
        Self {
            x: self.x / rhs,
            y: self.y / rhs,
            z: self.z / rhs
        }
    }
}

impl Div<f64> for &Cartesian3 {
    type Output = Cartesian3;

     fn div (self, rhs: f64) -> Cartesian3 {
        Cartesian3 {
            x: self.x / rhs,
            y: self.y / rhs,
            z: self.z / rhs
        }
    }
}

impl DivAssign<f64> for Cartesian3 {
    fn div_assign (&mut self, rhs: f64) {
        self.x /= rhs;
        self.y /= rhs;
        self.z /= rhs;
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

        let b = EQUATORIAL_EARTH_RADIUS / ( 1.0 - E_EARTH_SQUARED* (sin_φ * sin_φ)).sqrt();
        let c = (b + h)*cos_φ;

        let x = c *  λ.cos();
        let y = c *  λ.sin();
        let z = (EARTH_RADIUS_RATIO_SQUARED * b + h) * sin_φ;

        Cartesian3::new( x, y, z)
    }
}

/* #region serde *******************************************************/

pub fn ser_rounded_cartesian3<S: Serializer> (p: &Cartesian3, s: S) -> Result<S::Ok, S::Error>  {
    let mut c3 = s.serialize_struct("Cartesian3", 3)?;
    c3.serialize_field("x", &(p.x.round() as i64))?;
    c3.serialize_field("y", &(p.y.round() as i64))?;
    c3.serialize_field("z", &(p.z.round() as i64))?;
    c3.end()
}

/* 

// while this would save bytes for transmitting serialized values it would increase memory in clients 
// since JSON.parse() would have to first create arrays which we then have to translate into Cesium.Cartesian3 objects

use serde::ser::{Serialize as SerializeTrait, SerializeSeq, Serializer, SerializeStruct};
use serde::de::{Deserialize as DeserializeTrait, Deserializer};

impl SerializeTrait for Cartesian3 {
    // a tuple might be more axiomatic but could not be used for JSON
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        let mut state = serializer.serialize_seq(Some(3))?;
        state.serialize_element( &self.x)?;
        state.serialize_element( &self.y)?;
        state.serialize_element( &self.z)?;
        state.end()
    }
}

impl<'de> DeserializeTrait<'de> for Cartesian3 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let a = <[f64; 3]>::deserialize(deserializer)?;
        Ok( Cartesian3::new( a[0], a[1], a[2]))
    }
}

*/