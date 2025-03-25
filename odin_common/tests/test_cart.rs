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

use odin_common::{cartesian3::{self, Cartesian3}, cartographic::{self, Cartographic}};

/// unit tests for cartesian3 and cartographic
/// run with "cargo test test_inside -- --nocapture"

#[test]
fn test_inside () {
    let vs: Vec<Cartographic> = vec![
        (-129.8029, 50.4250 ), 
        (-122.5463, 32.3474 ),
        ( -97.6721, 24.1709 ),
        ( -79.8117, 24.1709 ),
        ( -62.8262, 47.7229 )
    ].iter().map( |p| Cartographic::from_degrees(p.0, p.1, 0.0)).collect();

    let vs_ecef: Vec<Cartesian3> = vs.iter().map( |v| Cartesian3::from(v)).collect();
    let normals = Cartesian3::normals( &vs_ecef);

    println!("-- test inside");
    let ps: Vec<Cartesian3> = vec![
        (-102.40, 40.0 ), 
        (-96.0, 53.4 ),
        ( -68.26, 46.38 ),
        ( -88.21, 26.13 ),
        ( -110.32, 30.36 )
    ].iter().map( |p| Cartesian3::from( Cartographic::from_degrees(p.0, p.1, 830000.0))).collect();

    for p in ps.iter() {
        print!("  {}", Cartographic::from(p));
        assert!( p.is_inside_normals(&normals));
        println!("✅");
    }

    println!("-- test outside");
    let ps: Vec<Cartesian3> = vec![
        (-127.0, 40.5), 
        (-98.32, 55.6),
        (-67.96, 39.86),
        (-88.98, 21.5),
        (-111.5, 26.56)

    ].iter().map( |p| Cartesian3::from( Cartographic::from_degrees(p.0, p.1, 830000.0))).collect();

    for p in ps.iter() {
        print!("  {}", Cartographic::from(p));
        assert!( !p.is_inside_normals(&normals));
        println!("✅");
    }

}