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

pub mod strings;
pub mod collections;
pub mod macros;
pub mod fs;
pub mod datetime;
pub mod angle;
pub mod geo;
pub mod sim_clock;
pub mod ranges;
pub mod schedule;
pub mod admin;
pub mod process;
pub mod net;

#[cfg(feature="s3")]
pub mod s3;

pub mod heap;

pub mod slack; // only requires reqwest so no feature gate (yet)

#[cfg(feature="slack_admin")]
odin_build::define_load_config!();

// syntactic sugar - this is just more readable
#[inline] pub fn sin(x:f64) -> f64 { x.sin() }
#[inline] pub fn sin2(x:f64) -> f64 { let sin_x = x.sin(); sin_x*sin_x }
#[inline] pub fn cos(x:f64) -> f64 { x.cos() }
#[inline] pub fn cos2(x:f64) -> f64 { let cos_x = x.cos(); cos_x*cos_x }
#[inline] pub fn sinh(x:f64) -> f64 { x.sinh() }
#[inline] pub fn cosh(x:f64) -> f64 { x.cosh() }
#[inline] pub fn tan(x:f64) -> f64 { x.tan() }
#[inline] pub fn asin(x:f64) -> f64 {x.asin() }
#[inline] pub fn atan(x:f64) -> f64 { x.atan() }
#[inline] pub fn atanh(x:f64) -> f64 { x.atanh() }
#[inline] pub fn sqrt(x:f64) -> f64 { x.sqrt() }
#[inline] pub fn pow2(x:f64) -> f64 { x*x }

// a global fn that can be used with serde(skip_serializing_if="odin_common::is_none")
#[inline] pub fn is_none<T> (opt: &Option<T>)->bool { opt.is_none() }
