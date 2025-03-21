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

/// common geodetic constants that should be consistent through ODIN applications
/// Note that const floats are still not stabilized as of Rust 1.85 (see https://github.com/rust-lang/rust/issues/57241)

/// mean earth radius in meters
pub const MEAN_EARTH_RADIUS: f64 = 6371000.0; 
pub const MER_SQUARED: f64 = (MEAN_EARTH_RADIUS * MEAN_EARTH_RADIUS);

/// semi major axis in meters
pub const EQATORIAL_EARTH_RADIUS: f64 = 6378137.0; 

/// semi minor axis in meters
pub const POLAR_EARTH_RADIUS: f64 = 6356752.3142; 

pub const EARTH_RADIUS_RATIO: f64 = POLAR_EARTH_RADIUS / EQATORIAL_EARTH_RADIUS;  // b / a

/// b²/a² - squared ratio of minor/major axis
pub const EARTH_RADIUS_RATIO_SQUARED: f64 = EARTH_RADIUS_RATIO*EARTH_RADIUS_RATIO;

pub const F_EARTH: f64 = (EQATORIAL_EARTH_RADIUS - POLAR_EARTH_RADIUS) / EQATORIAL_EARTH_RADIUS;  
pub const INVERSE_F_EARTH: f64 = 1.0 / F_EARTH;

/// first eccentricity of earth
pub const E_EARTH: f64 = 0.08181919092890692; // (1.0 - B2A2).sqrt(); // f64::sqrt() not const
pub const E_EARTH_SQUARED: f64 = E_EARTH*E_EARTH;
