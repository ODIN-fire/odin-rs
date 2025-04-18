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
#![feature(trait_alias)]
#![feature(io_error_more)]

use std::f64::consts::{PI as STD_PI};

use serde::{Serialize,Deserialize};
use num::{Num,ToPrimitive};

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
pub mod uom;
pub mod json_writer;

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

impl <T: Num + Copy + ToPrimitive> BoundingBox<T> {
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

