/*
 * Copyright 2026 Peter Mehlitz
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://apache.org
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use odin_common::{cartographic::earth_radius_at_geodetic_latitude, geo::GeoPoint, utm::geo_to_utm};
use odin_dem::{DemGrid, DEFAULT_CURV_COEFF};

const RES: f64 = 30.0;

/// a synthetic grid of (2n x 2n) cells with given resolution around `origin`, with heights computed from cell center
/// offsets (dx,dy) to the origin (east, north)
fn grid_around (origin: &GeoPoint, n: usize, f: impl Fn(f64,f64)->f32)->DemGrid {
    let o = geo_to_utm( origin).unwrap();
    let x0 = o.easting - n as f64 * RES;
    let y0 = o.northing + n as f64 * RES;
    let w = 2*n;
    let mut data = Vec::with_capacity( w*w);
    for iy in 0..w {
        for ix in 0..w {
            let x = x0 + (ix as f64 + 0.5) * RES;
            let y = y0 - (iy as f64 + 0.5) * RES;
            data.push( f( x - o.easting, y - o.northing));
        }
    }
    DemGrid::new( data, w, w, x0, y0, RES, RES, Some(-9999.0), o.utm_zone).unwrap()
}

fn origin ()->GeoPoint { GeoPoint::from_lon_lat_degrees( -122.0, 37.5) }

#[test]
fn test_bilinear_height () {
    let o = origin();
    let g = grid_around( &o, 10, |dx,dy| (dx + 2.0*dy) as f32); // a plane is reproduced exactly by bilinear interpolation
    let u = geo_to_utm( &o).unwrap();
    for (dx,dy) in [(0.0,0.0), (17.0,-41.5), (-100.0,123.4), (250.0,250.0)] {
        let h = g.height_at( u.easting + dx, u.northing + dy).unwrap();
        assert!( (h - (dx + 2.0*dy)).abs() < 1e-3, "height at ({dx},{dy}) is {h}");
    }
    assert!( g.height_at( u.easting + 10_000.0, u.northing).is_none());
}

#[test]
fn test_flat_no_curvature () {
    let o = origin();
    let g = grid_around( &o, 400, |_,_| 0.0);
    let hit = g.ray_intersection( &o, 100.0, 90.0, -1.0, 20_000.0, 0.0).unwrap().unwrap();
    let expected = 100.0 / 1.0f64.to_radians().tan(); // 5729.4m
    println!("flat hit at {:.2}m (expected {expected:.2}m): {:.5},{:.5}", hit.distance, hit.lon, hit.lat);
    assert!( (hit.distance - expected).abs() < 0.01);
    assert!( hit.height.abs() < 1e-6);
}

#[test]
fn test_flat_with_curvature () {
    let o = origin();
    let g = grid_around( &o, 400, |_,_| 0.0);
    let k = DEFAULT_CURV_COEFF / (2.0 * earth_radius_at_geodetic_latitude( 37.5f64.to_radians()));
    let t = 1.0f64.to_radians().tan();
    let expected = (-t + (t*t + 4.0 * k * 100.0).sqrt()) / (2.0 * k); // root of k·d² + t·d - h0 = 0

    let hit = g.ray_intersection( &o, 100.0, 45.0, -1.0, 20_000.0, DEFAULT_CURV_COEFF).unwrap().unwrap();
    println!("flat hit with curvature at {:.2}m (expected {expected:.2}m)", hit.distance);
    assert!( (hit.distance - expected).abs() < 0.01);
}

#[test]
fn test_horizon () {
    let o = origin();
    let g = grid_around( &o, 1400, |_,_| 0.0); // 84km grid
    let k = DEFAULT_CURV_COEFF / (2.0 * earth_radius_at_geodetic_latitude( 37.5f64.to_radians()));
    let expected = (100.0 / k).sqrt(); // ~38.6km for a horizontal ray from 100m

    let hit = g.ray_intersection( &o, 100.0, 180.0, 0.0, 41_000.0, DEFAULT_CURV_COEFF).unwrap().unwrap();
    println!("horizon at {:.1}m (expected {expected:.1}m)", hit.distance);
    assert!( (hit.distance - expected).abs() < 0.1);

    // without curvature a horizontal ray never hits flat terrain
    assert!( g.ray_intersection( &o, 100.0, 180.0, 0.0, 41_000.0, 0.0).unwrap().is_none());
}

#[test]
fn test_narrow_ridge () {
    // a single cell ridge of 200m at 3km distance (east) - coarse sampling as in the browser (200m steps) would miss it
    let o = origin();
    let g = grid_around( &o, 400, |dx,_| if (dx - 2985.0).abs() < 1.0 { 200.0 } else { 0.0 }); // cell centers are at odd multiples of RES/2
    let hit = g.ray_intersection( &o, 100.0, 90.0, -0.5, 20_000.0, DEFAULT_CURV_COEFF).unwrap().unwrap();
    println!("ridge hit at {:.1}m, height {:.1}m", hit.distance, hit.height);
    assert!( hit.distance > 2985.0 - RES && hit.distance < 2985.0); // on the (bilinear) slope of the ridge cell
    assert!( (hit.height - (100.0 + hit.distance * (-0.5f64).to_radians().tan())).abs() < 1.0); // ray height ~ terrain
}

#[test]
fn test_grid_convergence () {
    // far west of the UTM zone 10 central meridian (-123) true north has a grid bearing of ~ -Δλ·sin(φ) = 0.9deg
    let o = GeoPoint::from_lon_lat_degrees( -124.4, 40.5);
    let g = grid_around( &o, 400, |_,_| 0.0);
    let gamma = g.grid_convergence( &o).unwrap();
    println!("grid convergence: {gamma:.3}deg");
    assert!( (gamma - 1.4 * 40.5f64.to_radians().sin()).abs() < 0.05);

    // a true north ray has to end up on the same meridian
    let hit = g.ray_intersection( &o, 100.0, 0.0, -1.0, 20_000.0, 0.0).unwrap().unwrap();
    let dlon = hit.lon - o.longitude_degrees();
    println!("north ray hit at {:.5},{:.5}, dlon = {dlon:e}", hit.lon, hit.lat);
    assert!( dlon.abs() < 1e-5); // < 1m
    assert!( hit.lat > o.latitude_degrees());
}

#[test]
fn test_misses_and_errors () {
    let o = origin();
    let g = grid_around( &o, 100, |dx,dy| if dx.abs() < 100.0 && dy.abs() < 100.0 { 50.0 } else { 0.0 });

    assert!( g.ray_intersection( &o, 20.0, 0.0, -1.0, 1000.0, 0.0).is_err()); // origin below terrain
    assert!( g.ray_intersection( &GeoPoint::from_lon_lat_degrees( -121.0, 37.5), 100.0, 0.0, -1.0, 1000.0, 0.0).is_err()); // origin outside grid
    assert!( g.ray_intersection( &o, 100.0, 0.0, 1.0, 10_000.0, 0.0).unwrap().is_none()); // upwards, leaves grid
    assert!( g.ray_intersection( &o, 100.0, 0.0, -1.0, 1000.0, 0.0).unwrap().is_none()); // max_dist too short
}
