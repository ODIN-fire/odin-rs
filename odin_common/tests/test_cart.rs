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

use odin_common::{cartesian3::{self, Cartesian3}, cartographic::{self, approximate_surface_centroid, Cartographic}, rad};

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

#[test]
fn test_centroid () {
    // CONUS
    let input1: Vec<(f64,f64)> = vec![
        (-129.8029, 50.4250 ), 
        (-122.5463, 32.3474 ),
        ( -97.6721, 24.1709 ),
        ( -79.8117, 24.1709 ),
        ( -62.8262, 47.7229 )
    ];

    let vs: Vec<Cartographic> = input1.iter().map( |p| Cartographic::from_degrees(p.0, p.1, 0.0)).collect();
    let m = approximate_surface_centroid(&vs);
    println!("centroid-approx CONUS: {m}");
    let avg = Cartographic::mean(&vs);
    println!("avg CONUS: {avg}");


    // CZU bbox
    let input2: Vec<(f64,f64)> = vec![
        (-122.44855, 37.30877),
        (-122.44855, 36.9546),
        (-121.89434, 36.9546),
        (-121.89434, 37.30877)
    ];

    let vs: Vec<Cartographic> = input2.iter().map( |p| Cartographic::from_degrees(p.0, p.1, 0.0)).collect();
    let m = approximate_surface_centroid(&vs);
    println!("centroid-approx CZU: {m}");
    let avg = Cartographic::mean(&vs);
    println!("avg CZU: {avg}");
}

#[test]
fn test_spherical () {
    let c = Cartographic::from_degrees( -122.0, 40.0, 0.0);
    println!("spherical coords: {}  = {:?}", c, c);

    let p = c.spherical_to_cartesian3( 730000.0);
    println!("cartesian: {:?}", p);

    let d = p.cartesian_to_spherical();
    println!("spherical coords: {:?}", d);

}

#[test]
fn test_conversion () {
    let mut p = Cartesian3::new( -2458250.0, -5262107.0, 4259973.0);
    let c: Cartographic = p.into();

    println!("ecef:  {:?} : {}", p, p.length());
    println!("wgs84: {}", c);

    println!("\nscaled to mean Earth radius");
    let q = p.to_mean_earth_radius();
    let c: Cartographic = q.into();
    println!("ecef:  {:?} : {}", q, q.length());
    println!("wgs84: {}", c);

    println!("\nscaled to Earth radius");
    let q = p.to_earth_radius();
    let c: Cartographic = q.into();
    println!("ecef:  {:?} : {}", q, q.length());
    println!("wgs84: {}", c);
    
}

#[test]
fn test_rotation () {
    let p = Cartesian3::new( -2075733.0, -3632129.0, 4798378.0);
    let alpha = rad(73.0);
    let w2 = 400.0 / 2.0; // with
    let h2 = 370.0 / 2.0; // height

    println!("p: {p}: {}", p.length());

    let (n,n_east,n_north) = Cartesian3::en_units(&p);
    
    let p1 = p + n_east * w2 + n_north * h2;
    println!("p1: {p1}: {}", p1.length());

    let p1r = p1.rotate_around( &n, alpha);
    println!("p1r: {p1r}: {}", p1r.length());

    let q1 = p1r.rotate_around( &n, -alpha);
    println!("q1: {q1}: {}", q1.length());

    let q1 = p1r.rotate_around( &n, rad( 360.0 - 73.0));
    println!("q1: {q1}: {}", q1.length());

}