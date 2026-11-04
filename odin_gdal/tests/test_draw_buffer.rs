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

//! tests for drawing buffered (wide) lines into projected datasets

use gdal::{Dataset, DriverManager, spatial_ref::{AxisMappingStrategy, CoordTransform, SpatialRef}};
use uom::si::{f64::Length, length::meter};
use odin_common::{geo::{GeoEllipse, GeoPoint}, angle::{Angle360, Latitude, Longitude}};
use odin_gdal::{get_rasterband_as_vec, draw::{draw_geo_line_string_buffer, fill_geo_line_string_buffer, geo_ellipse_coverage, geo_line_string_coverage}};

const N: usize = 40;       // raster size
const RES: f64 = 30.0;     // cell size [m]
const X0: f64 = 590000.0;  // UTM zone 10N origin (upper left)
const Y0: f64 = 4110000.0;

fn create_ds ()->Dataset {
    let mut ds = DriverManager::get_driver_by_name("MEM").unwrap().create_with_band_type::<f32,_>( "", N, N, 1).unwrap();
    ds.set_geo_transform( &[X0, RES, 0.0, Y0, 0.0, -RES]).unwrap();
    ds.set_spatial_ref( &SpatialRef::from_epsg(32610).unwrap()).unwrap();
    ds
}

/// convert UTM points into geo points
fn geo_points (utm: &[(f64,f64)])->Vec<GeoPoint> {
    let mut src = SpatialRef::from_epsg(32610).unwrap();
    src.set_axis_mapping_strategy( AxisMappingStrategy::TraditionalGisOrder);
    let mut tgt = SpatialRef::from_epsg(4326).unwrap();
    tgt.set_axis_mapping_strategy( AxisMappingStrategy::TraditionalGisOrder);
    let ct = CoordTransform::new( &src, &tgt).unwrap();

    utm.iter().map( |(x,y)| {
        let (mut xs, mut ys) = ([*x], [*y]);
        ct.transform_coords( &mut xs, &mut ys, &mut []).unwrap();
        GeoPoint::from_lon_lat_degrees( xs[0], ys[0])
    }).collect()
}

/// UTM coordinates of the center of cell (col,row)
fn cell_center (col: usize, row: usize)->(f64,f64) {
    (X0 + (col as f64 + 0.5) * RES, Y0 - (row as f64 + 0.5) * RES)
}

fn values (ds: &Dataset)->Vec<f32> { get_rasterband_as_vec( ds, 1).unwrap() }

#[test]
fn test_gap_free_diagonal () {
    // a thin (1m) 45 degree line through cell centers - a Bresenham line would only set the diagonal cells
    let mut ds = create_ds();
    let ps = geo_points( &[cell_center(5,5), cell_center(30,30)]);
    draw_geo_line_string_buffer( &mut ds, &ps, 1.0, 1.0).unwrap();

    let vs = values( &ds);
    let set = |c: usize, r: usize| vs[r*N + c] > 0.0;

    for i in 6..29 {
        assert!( set(i,i));
        // each diagonal step has to be connected through at least one orthogonal neighbor
        assert!( set(i+1,i) || set(i,i+1), "diagonal gap at {i}");
    }
}

#[test]
fn test_fill_center_inside () {
    // a 30m wide horizontal line through cell centers of row 10 sets exactly that row
    let mut ds = create_ds();
    let ps = geo_points( &[cell_center(5,10), cell_center(30,10)]);
    fill_geo_line_string_buffer( &mut ds, &ps, 30.0, 99.0).unwrap();

    let vs = values( &ds);
    assert_eq!( vs[10*N + 15], 99.0);
    assert_eq!( vs[9*N + 15], 0.0);
    assert_eq!( vs[11*N + 15], 0.0);
}

#[test]
fn test_coverage () {
    let ds = create_ds();

    // a 15m wide horizontal line through the cell centers of row 10 covers half of each cell in that row
    let ps = geo_points( &[cell_center(5,10), cell_center(30,10)]);
    let cov = geo_line_string_coverage( &ds, &ps, 15.0).unwrap();
    let c = cov.iter().find( |(col,row,_)| *col == 15 && *row == 10).unwrap().2;
    assert!( (c - 0.5).abs() < 0.02, "coverage {c}");
    assert!( cov.iter().all( |(_,row,_)| *row == 10)); // the line does not reach other rows

    // a 60m wide line covers its own row fully and half of the adjacent rows
    let cov = geo_line_string_coverage( &ds, &ps, 60.0).unwrap();
    let at = |col: usize, row: usize| cov.iter().find( |(c,r,_)| *c == col && *r == row).map( |e| e.2).unwrap_or(0.0);
    assert!( (at(15,10) - 1.0).abs() < 0.02);
    assert!( (at(15,9) - 0.5).abs() < 0.02);
    assert!( (at(15,11) - 0.5).abs() < 0.02);
    assert_eq!( at(15,8), 0.0);
}

#[test]
fn test_ellipse_coverage () {
    let ds = create_ds();

    // 120m x 40m ellipse centered in cell (20,20), major axis pointing east
    let c = geo_points( &[cell_center(20,20)])[0];
    let e = GeoEllipse::new( Longitude::from_degrees( c.longitude_degrees()), Latitude::from_degrees( c.latitude_degrees()),
                             Length::new::<meter>(60.0), Length::new::<meter>(20.0), Angle360::from_degrees(90.0));
    let cov = geo_ellipse_coverage( &ds, &e).unwrap();

    // the covered area has to match the ellipse area (within polygon approximation and projection errors)
    let area: f64 = cov.iter().map( |(_,_,c)| c * RES * RES).sum();
    let expected = std::f64::consts::PI * 60.0 * 20.0;
    assert!( (area - expected).abs() / expected < 0.02, "area {area} vs {expected}");

    // it extends 2 cells east and west of the center cell but less than one cell north and south
    let at = |col: usize, row: usize| cov.iter().find( |(c,r,_)| *c == col && *r == row).map( |e| e.2).unwrap_or(0.0);
    assert!( at(20,20) > 0.9);
    assert!( at(22,20) > 0.0 && at(18,20) > 0.0);
    assert_eq!( at(20,18), 0.0);
    assert_eq!( at(20,22), 0.0);
}
