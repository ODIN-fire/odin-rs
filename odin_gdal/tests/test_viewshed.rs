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

//! tests for viewshed computation and polygon generation

use gdal::{Dataset, DriverManager, spatial_ref::{AxisMappingStrategy, CoordTransform, SpatialRef}};
use odin_gdal::{get_rasterband_as_vec, set_rasterband_from_vec, viewshed::{ViewshedParams, VISIBLE, compute_viewshed, write_viewshed_geojson}};

const N: usize = 100;      // raster size
const RES: f64 = 30.0;     // cell size [m]
const X0: f64 = 590000.0;  // UTM zone 10N origin (upper left)
const Y0: f64 = 4110000.0;
const WALL_COL: usize = 60;

/// flat terrain at 100m with a 300m high north-south wall at WALL_COL
fn create_dem ()->Dataset {
    let mut ds = DriverManager::get_driver_by_name("MEM").unwrap().create_with_band_type::<f32,_>( "", N, N, 1).unwrap();
    ds.set_geo_transform( &[X0, RES, 0.0, Y0, 0.0, -RES]).unwrap();
    ds.set_spatial_ref( &SpatialRef::from_epsg(32610).unwrap()).unwrap();

    let vs: Vec<f32> = (0..N*N).map( |i| if i % N == WALL_COL { 400.0 } else { 100.0 }).collect();
    set_rasterband_from_vec( &mut ds, 1, vs).unwrap();
    ds
}

/// UTM coordinates of the center of cell (col,row)
fn cell_center (col: usize, row: usize)->(f64,f64) {
    (X0 + (col as f64 + 0.5) * RES, Y0 - (row as f64 + 0.5) * RES)
}

#[test]
fn test_viewshed_wall () {
    let dem = create_dem();
    let (x, y) = cell_center( 30, 50);
    let params = ViewshedParams { observer_height: 10.0, max_distance: 0.0, ..Default::default() };

    let vs_ds = compute_viewshed( &dem, 1, x, y, &params).unwrap();
    assert_eq!( vs_ds.raster_size(), (N, N));
    let vs: Vec<u8> = get_rasterband_as_vec( &vs_ds, 1).unwrap();
    let visible = |c: usize, r: usize| vs[r*N + c] as f64 == VISIBLE;

    assert!( visible( 31, 50));             // next to observer
    assert!( visible( 10, 20));             // flat terrain on observer side
    assert!( visible( WALL_COL, 50));       // the wall itself
    assert!( !visible( WALL_COL + 5, 50));  // behind the wall
    assert!( !visible( 95, 10));
}

#[test]
fn test_viewshed_max_distance () {
    let dem = create_dem();
    let (x, y) = cell_center( 30, 50);
    let params = ViewshedParams { observer_height: 10.0, max_distance: 600.0, ..Default::default() }; // 20 cells

    // the output only covers the max_distance window around the observer
    let vs_ds = compute_viewshed( &dem, 1, x, y, &params).unwrap();
    let (w, h) = vs_ds.raster_size();
    assert!( w < N && h < N);

    let gt = vs_ds.geo_transform().unwrap();
    let vs: Vec<u8> = get_rasterband_as_vec( &vs_ds, 1).unwrap();
    let visible = |c: usize, r: usize| { // in DEM cell coordinates
        let (x, y) = cell_center( c, r);
        let (fi, fj) = ((x - gt[0]) / gt[1], (y - gt[3]) / gt[5]);
        fi >= 0.0 && fj >= 0.0 && (fi as usize) < w && (fj as usize) < h && vs[(fj as usize)*w + fi as usize] as f64 == VISIBLE
    };

    assert!( visible( 30, 35));  // 15 cells (450m) north of observer
    assert!( !visible( 30, 27)); // 23 cells (690m) - beyond max distance
}

#[test]
fn test_viewshed_geojson () {
    let dem = create_dem();
    let (x, y) = cell_center( 30, 50);
    let params = ViewshedParams { observer_height: 10.0, max_distance: 0.0, simplify: 0.0, ..Default::default() }; // exact cell boundaries

    let path = std::env::temp_dir().join("odin_gdal_test_viewshed.geojson");
    let n = write_viewshed_geojson( &dem, x, y, &params, r#"{"id":"test"}"#, &path).unwrap();
    let json: serde_json::Value = serde_json::from_reader( std::fs::File::open( &path).unwrap()).unwrap();
    let _ = std::fs::remove_file( &path);

    assert_eq!( n, 1); // everything west of the wall (including the wall)
    let feature = &json["features"][0];
    assert_eq!( feature["properties"]["id"], "test");
    assert_eq!( feature["geometry"]["type"], "MultiPolygon");

    // WGS84 lon/lat order, the polygon has to be west of the wall
    let ring = feature["geometry"]["coordinates"][0][0].as_array().unwrap();
    let lons: Vec<f64> = ring.iter().map( |p| p[0].as_f64().unwrap()).collect();
    let lats: Vec<f64> = ring.iter().map( |p| p[1].as_f64().unwrap()).collect();
    assert!( lons.iter().all( |lon| *lon > -122.0 && *lon < -121.9));
    assert!( lats.iter().all( |lat| *lat > 37.1 && *lat < 37.2));

    // UTM columns are not aligned with meridians, hence we check the polygon vertices in UTM coordinates
    let (mut xs, mut ys) = (lons, lats);
    utm_transform().transform_coords( &mut xs, &mut ys, &mut []).unwrap();
    let x_wall_east = X0 + (WALL_COL + 1) as f64 * RES;
    let x_max = xs.iter().cloned().fold( f64::MIN, f64::max);
    assert!( x_max < x_wall_east + 1.0, "polygon extends beyond wall: {x_max} > {x_wall_east}");
    assert!( x_max > x_wall_east - RES - 1.0, "wall not included: {x_max}"); // the wall itself is visible
}

/// WGS84 -> UTM 10N transformation (both in lon/lat, easting/northing order)
fn utm_transform ()->CoordTransform {
    let mut src = SpatialRef::from_epsg(4326).unwrap();
    src.set_axis_mapping_strategy( AxisMappingStrategy::TraditionalGisOrder);
    let mut tgt = SpatialRef::from_epsg(32610).unwrap();
    tgt.set_axis_mapping_strategy( AxisMappingStrategy::TraditionalGisOrder);
    CoordTransform::new( &src, &tgt).unwrap()
}
