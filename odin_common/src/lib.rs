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
#![allow(unused,uncommon_codepoints)]

use std::{f64::consts::{PI as STD_PI}, fmt, ops::Range, fmt::Debug};

use serde::{Serialize,Deserialize};
use num::{Num,ToPrimitive};
use ndarray::Array2;

pub mod strings;
pub mod collections;
pub mod macros;
pub mod fs;
pub mod datetime;
pub mod angle;
pub mod geo_constants;
pub mod geo;
pub mod cartesian3;
pub mod cartographic;
pub mod utm;
pub mod sim_clock;
pub mod ranges;
pub mod schedule;
pub mod admin;
pub mod process;
pub mod net;
pub mod ws;
pub mod uom;
pub mod json_writer;
pub mod u8extractor;
pub mod ron;
pub mod num_type;
pub mod var_val;

#[cfg(feature="s3")]
pub mod s3;

pub mod heap;

pub mod slack; // only requires reqwest so no feature gate (yet)

#[cfg(feature="slack_admin")]
odin_build::define_load_config!();

// syntactic sugar - this is just more readable in many cases
#[inline(always)] pub fn sin(x:f64) -> f64 { x.sin() }
#[inline(always)] pub fn sin2(x:f64) -> f64 { let sin_x = x.sin(); sin_x*sin_x }
#[inline(always)] pub fn cos(x:f64) -> f64 { x.cos() }
#[inline(always)] pub fn cos2(x:f64) -> f64 { let cos_x = x.cos(); cos_x*cos_x }
#[inline(always)] pub fn sinh(x:f64) -> f64 { x.sinh() }
#[inline(always)] pub fn cosh(x:f64) -> f64 { x.cosh() }
#[inline(always)] pub fn tan(x:f64) -> f64 { x.tan() }
#[inline(always)] pub fn asin(x:f64) -> f64 {x.asin() }
#[inline(always)] pub fn atan(x:f64) -> f64 { x.atan() }
#[inline(always)] pub fn atan2(y:f64,x:f64) -> f64 { y.atan2(x) }
#[inline(always)] pub fn atanh(x:f64) -> f64 { x.atanh() }
#[inline(always)] pub fn sqrt(x:f64) -> f64 { x.sqrt() }
#[inline(always)] pub fn pow2(x:f64) -> f64 { x*x }
#[inline(always)] pub fn abs(x:f64) -> f64 { x.abs() }
#[inline(always)] pub fn deg(x:f64)->f64 { x.to_degrees() }
#[inline(always)] pub fn rad(x:f64)->f64 { x.to_radians() }
#[inline(always)] pub fn signum(x:f64)->f64 { x.signum() }


// a global fn that can be used with serde(skip_serializing_if="odin_common::is_none")
#[inline] pub fn is_none<T> (opt: &Option<T>)->bool { opt.is_none() }


/// a generic bounding box without semantics for the coordinate type
#[repr(C)]
#[derive(Debug,Copy,Clone,Serialize,Deserialize,PartialEq)]
pub struct BoundingBox <T: Num> {
    pub west: T,
    pub south: T,
    pub east: T,
    pub north: T
}

impl <T: Num + Copy + ToPrimitive + From<f64> + Debug> BoundingBox<T> {
    pub fn new(west: T, south: T, east: T, north: T)->Self {
        BoundingBox{ west, south, east, north}
    }

    pub fn from_wsen<N> (wsen: &[N;4]) -> BoundingBox<T> where N: Num + Copy + Into<T> {
        BoundingBox::<T>{
            west: wsen[0].into(),
            south: wsen[1].into(),
            east: wsen[2].into(),
            north: wsen[3].into()
        }
    }

    pub fn to_minmax_array (&self) -> [T;4] {
        [self.west,self.south,self.east,self.north]
    }

    pub fn as_mimax_array_ref (&self) -> &[T;4] {
        unsafe { std::mem::transmute(self) }
    }

    // FIXME - should stay as (T,T) but how can we divide/round
    pub fn center (&self) -> (f64,f64) {
        ( (self.west + self.east).to_f64().unwrap() / 2.0, (self.south + self.north).to_f64().unwrap() / 2.0 )
    }

    #[inline] pub fn width (&self)->T { self.east - self.west }
    #[inline] pub fn height (&self)->T { self.north - self.south }
    #[inline] pub fn size (&self)->(T,T) { (self.width(), self.height()) }

    pub fn centered_grid (&self, res: T, exterior: bool)->Array2<(T,T)> {
        let center = self.center();
        let (nx,ny) = self.get_centered_grid_dimensions(res, exterior);
        let d = res.to_f64().unwrap();
        let left = center.0 - (nx/2) as f64 * d;
        let top = center.1 + (ny/2) as f64 * d;

        let mut v: Vec<(T,T)> = Vec::with_capacity( nx * ny);

        for j in 0..ny {
            let y = top - (j as f64) * d;
            for i in 0..nx {
                let x = left + (i as f64) * d;
                v.push( (x.into(), y.into()) );
            }
        }

        Array2::from_shape_vec( (nx,ny), v).unwrap()
    }

    /// this always returns odd numbers so that we have the same number of cells from the center in each direction
    pub fn get_centered_grid_dimensions (&self, res: T, exterior: bool)->(usize,usize) {
        let r = res.to_f64().unwrap();
        let xr2 = self.width().to_f64().unwrap() / (2.0 * r);
        let yr2 = self.height().to_f64().unwrap() / (2.0 * r);

        if exterior {
            (2 * (xr2.ceil() as usize) + 1, 2 * (yr2.ceil() as usize) + 1)
        } else {
            (2 * (xr2.floor() as usize) + 1, 2 * (yr2.floor() as usize) + 1)
        }
    }
}

pub fn apply_index_margin (bbox: &mut BoundingBox<usize>, margin: usize, max_x: usize, max_y: usize) {
    bbox.west = if bbox.west > margin { bbox.west - margin } else { 0 };
    bbox.east = if bbox.east + margin >= max_x { max_x } else { bbox.east + margin };
    bbox.north = if bbox.north > margin { bbox.north - margin } else { 0 };
    bbox.south = if bbox.south + margin >= max_y { max_y } else { bbox.south + margin };
}

/// a simple incremental min/max/avg accumulator
#[derive(Debug)]
pub struct MinMaxAvg {
    pub n: usize,
    pub min: f64,
    pub max: f64,
    pub avg: f64
}

impl MinMaxAvg {
    pub fn new()->Self { MinMaxAvg { n: 0, min: f64::MAX, max: f64::MIN, avg: f64::NAN } }

    /// add a new observation
    pub fn add (&mut self, x: f64) {
        self.n += 1;

        if self.n > 1 {
            self.avg = self.avg + (x - self.avg) / self.n as f64;
            if x < self.min { self.min = x }
            if x > self.max { self.max = x }
        } else {
            self.min = x;
            self.max = x;
            self.avg = x;
        }
    }
}

#[inline]
pub fn is_same_ref<T> (r1: &T, r2: &T) -> bool {
    (r1 as *const _) == (r2 as *const _)
}

pub const PI: f64 = STD_PI;
pub const HALF_PI: f64 = PI / 2.0;
pub const TWO_PI: f64 = PI * 2.0;
pub const PI_SQUARED: f64 = PI*PI;


/// a [0..100] range checked f64, mostly for explicit semantics
#[derive(Serialize,Deserialize,Debug,Clone,Copy,PartialEq)]
pub struct Percent(f64);

impl Percent {
    pub fn new (percent: f64)->Self { Percent(percent) }

    pub fn rounded_percent(&self)->u32 { self.0.round() as u32 }
    pub fn as_factor (&self)->f64 { self.0 / 100.0 }
    pub fn check_range (&self)->bool { self.0 >= 0.0 && self.0 <= 100.0 }
}

impl fmt::Display for Percent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}%", self.0)
    }
}

impl PartialOrd for Percent {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

impl <T> From<T> for Percent where T: Into<f64> + Copy {
    fn from (t: T)->Self { Percent( t.into()) }
}

/// get the basic typename (without crate/module path and type params) of a type
pub fn type_base_name<T> ()->&'static str {
    let mut te = std::any::type_name::<T>();

    if let Some(i) = te.find('<') { // path part does not contain generic type params
        te = &te[0..i];
    }
    if let Some(i) = te.rfind(':') { // with generic type params cut off find the last path sep
        te = &te[i+1..];
    }
    te
}

/// get the basic typename (without crate/module path and type params) of an object
#[inline]
pub fn type_base_name_of<T> (o: &T)->&'static str {
    type_base_name::<T>()
}


/// note this might still print trailing bits because some fp values cannot be represented exactly
#[inline]
pub fn round_to_decimals(val: f64, decimals: i32) -> f64 {
    let multiplier = 10.0_f64.powi(decimals);
    (val * multiplier).round() / multiplier
}

/// we can't do a From here as we own none of the types
#[inline]
pub fn as_min_max<T> (r: Range<T>)->(T,T) {
    (r.start, r.end)
}

/// return the common prefix of two slices of comparable+clonable elements as a Vec
pub fn common_prefix<T: PartialEq+Clone> (a: &[T], b: &[T])->Vec<T> {
    a.iter()
        .zip(b.iter())
        .take_while(|(x, y)| x == y)
        .map(|(x, _)| x.clone())
        .collect()
}

/// compute the sum of inverse distance weighted elements
/// both distance and accumulation functions are provided as parameters to make the target type T generic
/// note that the 'dist' function is abstract and it is Ok to use squared euclidean. The 'q' exponent can
/// be used to emphasize distance differences
pub fn  inverse_distance_weighted<T,F,G> (v0: &mut T, vs: &[T], q: i32, dist: F, acc: G)
    where F: Fn(&T,&T)->f64, G: Fn(&mut T,&T,f64)
{
    let len = vs.len();
    let mut ds: Vec<f64> = vs.iter().map( |v| dist( v0, v).powi(-q)).collect();
    let sum: f64 = ds.iter().sum();

    for i in 0..len {
        ds[i] = ds[i] / sum
    }

    for i in 0..len {
        acc( v0, &vs[i], ds[i])
    }
}

#[inline]
pub fn accumulate_weighted_f64 (acc: &f64, next: &f64, w: f64)->f64 {
    acc + next * w
}

#[inline]
pub fn accumulate_weighted_f32 (acc: &f32, next: &f32, w: f32)->f32 {
    acc + next * w
}

#[inline]
pub fn average_f64 (last_avg: f64, next: f64, n: usize)->f64 {
    last_avg + (next - last_avg)/(n as f64)
}

#[inline]
pub fn average_f32 (last_avg: f32, next: f32, n: usize)->f32 {
    last_avg + (next - last_avg)/(n as f32)
}

#[inline]
pub fn interpolate_f64 (a: f64, b: f64, r: f64)->f64 {
    a + (b - a) * r
}

#[inline]
pub fn interpolate_f32 (a: f32, b: f32, r: f32)->f32 {
    a + (b - a) * r
}
