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
#![allow(unused)]

use odin_common::{cartographic::Cartographic};
use odin_orbital::overpass::{abs_inclination, expand_dominant};


#[test]
fn test_expand_region () {
    // CONUS
    let input: Vec<(f64,f64)> = vec![
        (-129.8029, 50.4250 ), 
        (-122.5463, 32.3474 ),
        ( -97.6721, 24.1709 ),
        ( -79.8117, 24.1709 ),
        ( -62.8262, 47.7229 )
    ];
    let vs = Cartographic::from_lon_lat_degrees_slice(&input);
    println!("-- input:");
    for p in &vs { println!("  {:8.4}, {:8.4}", p.longitude_deg(), p.latitude_deg()); }

    let evs = expand_dominant(&vs, 1500000.0, abs_inclination(98.7219)); // VIIRS swath, NOAA-21 inclination
    println!("\n-- expanded:");
    for p in &evs { println!("  {:8.4}, {:8.4}", p.longitude_deg(), p.latitude_deg()); }
}