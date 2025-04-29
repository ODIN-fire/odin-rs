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
pub const EQUATORIAL_EARTH_RADIUS: f64 = 6378137.0; 
pub const EQUATORIAL_EARTH_RADIUS_SQUARED: f64 = EQUATORIAL_EARTH_RADIUS * EQUATORIAL_EARTH_RADIUS; 

/// semi minor axis in meters
pub const POLAR_EARTH_RADIUS: f64 = 6356752.3142; 
pub const POLAR_EARTH_RADIUS_SQUARED: f64 = POLAR_EARTH_RADIUS * POLAR_EARTH_RADIUS;

pub const EARTH_RADIUS_RATIO: f64 = POLAR_EARTH_RADIUS / EQUATORIAL_EARTH_RADIUS;  // b / a

/// b²/a² - squared ratio of minor/major axis
pub const EARTH_RADIUS_RATIO_SQUARED: f64 = EARTH_RADIUS_RATIO*EARTH_RADIUS_RATIO;

pub const F_EARTH: f64 = (EQUATORIAL_EARTH_RADIUS - POLAR_EARTH_RADIUS) / EQUATORIAL_EARTH_RADIUS;  
pub const INVERSE_F_EARTH: f64 = 1.0 / F_EARTH;

/// eccentricity
pub const E_EARTH: f64 = 0.08181919092890692; // first eccentricity of earth  sqrt( 1.0 - b²/a²)
pub const E_EARTH_SQUARED: f64 = E_EARTH*E_EARTH;
pub const ONE_MINUS_E_EARTH_SQUARED: f64 = 1.0 - E_EARTH_SQUARED;  // 1-e²

pub const E_EARTH_PRIME: f64 = 0.082551710742; // (sqrt( a2 - b2)/b)
pub const E_EARTH_PRIME_SQUARED: f64 = E_EARTH_PRIME * E_EARTH_PRIME;
