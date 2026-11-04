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

//! tests for the toa post processing (cumulative burn area polygons). The test raster is the (cropped) toa_wgs84.tif
//! output of a 4h Woods-1 run with a single point ignition 1080 sec after base date

use std::{fs::{File, remove_file}, io::BufReader, path::PathBuf};
use serde_json::Value;
use odin_gdal::{get_rasterband_as_vec, get_resolution, open_dataset};
use odin_elmfire::toa::{contour_levels, count_burned_cells, prepare_toa_contour_ds, to_cumulative_poly_geo_json};

const TOA_PATH: &str = "resources/woods-1_toa_wgs84.tif";
const T_START: i64 = 1080;
const N_HOURS: u32 = 4;

fn tmp_path (name: &str)->PathBuf {
    PathBuf::from( env!("CARGO_TARGET_TMPDIR")).join( name)
}

/// planar (shoelace) area of a ring in deg²
fn ring_area (ring: &Value)->f64 {
    let pts: Vec<(f64,f64)> = ring.as_array().unwrap().iter().map( |p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect();
    let mut a = 0.0;
    for i in 0..pts.len()-1 {
        a += pts[i].0 * pts[i+1].1 - pts[i+1].0 * pts[i].1;
    }
    (a / 2.0).abs()
}

/// area of a MultiPolygon in deg² (exterior rings minus holes) and number of holes
fn multi_polygon_area (coords: &Value)->(f64,usize) {
    let mut area = 0.0;
    let mut n_holes = 0;
    for poly in coords.as_array().unwrap() {
        for (i, ring) in poly.as_array().unwrap().iter().enumerate() {
            let pts = ring.as_array().unwrap();
            assert!( pts.len() >= 4, "degenerated ring");
            assert_eq!( pts.first(), pts.last(), "ring not closed");
            if i == 0 { area += ring_area(ring) } else { area -= ring_area(ring); n_holes += 1 }
        }
    }
    (area, n_holes)
}

/// create the polygons, check basic structure and return (level, area, n_holes) for each feature
fn create_polygons (cell_threshold: usize, name: &str)->Vec<(f64,f64,usize)> {
    let ds = open_dataset( TOA_PATH).unwrap();
    let ds = prepare_toa_contour_ds( &ds, cell_threshold, N_HOURS).unwrap();
    let levels = contour_levels( T_START, N_HOURS);

    let path = tmp_path( name);
    to_cumulative_poly_geo_json( &ds, &levels, &path).unwrap();

    let fc: Value = serde_json::from_reader( BufReader::new( File::open( &path).unwrap())).unwrap();
    remove_file( &path).unwrap();
    assert_eq!( fc["type"], "FeatureCollection");

    let features = fc["features"].as_array().unwrap();
    assert_eq!( features.len(), levels.len()); // all levels are within the fire duration

    features.iter().zip( levels.iter()).map( |(f, level)| {
        assert_eq!( f["properties"]["level"].as_f64().unwrap(), *level);
        assert_eq!( f["geometry"]["type"], "MultiPolygon");
        let (area, n_holes) = multi_polygon_area( &f["geometry"]["coordinates"]);
        (*level, area, n_holes)
    }).collect()
}

#[test]
fn test_cumulative_polygons () {
    let polys = create_polygons( 0, "toa_poly.geojson"); // no up-sampling

    // reference: number of raster cells with toa <= level
    let ds = open_dataset( TOA_PATH).unwrap();
    let (x_res, y_res) = get_resolution( &ds).unwrap();
    let cell_area = (x_res * y_res).abs();
    let vs: Vec<f32> = get_rasterband_as_vec( &ds, 1).unwrap();

    let mut last_area = 0.0;
    for (level, area, n_holes) in &polys {
        let n_cells = vs.iter().filter( |v| **v != -9999.0 && **v as f64 <= *level).count();
        let ratio = area / (n_cells as f64 * cell_area);
        println!("level {level}: {n_cells} cells, area ratio {ratio:.3}, {n_holes} holes");

        assert!( *area > last_area, "burn area not cumulative");
        if n_cells >= 100 { // contours interpolate between cell centers, which is too coarse for small areas
            assert!( (ratio - 1.0).abs() < 0.05, "polygon area does not match toa raster");
        }
        last_area = *area;
    }

    assert!( polys.iter().any( |(_,_,n_holes)| *n_holes > 0), "no holes in burn area polygons");
}

#[test]
fn test_upsampled_polygons () {
    let polys = create_polygons( usize::MAX, "toa_poly_upsampled.geojson"); // always up-sample

    let mut last_area = 0.0;
    for (_,area,_) in &polys {
        assert!( *area > last_area, "burn area not cumulative");
        last_area = *area;
    }
}

#[test]
fn test_small_fire_threshold () {
    // the threshold applies to the number of burned cells, not the raster size (spot fires can spread a small number
    // of burned cells over a large area)
    let ds = open_dataset( TOA_PATH).unwrap();
    assert_eq!( ds.raster_size(), (100, 84));
    assert_eq!( count_burned_cells( &ds).unwrap(), 2071);

    let ds_up = prepare_toa_contour_ds( &ds, 5000, N_HOURS).unwrap(); // 2071 < 5000 < 8400 raster cells
    assert_eq!( ds_up.raster_size(), (200, 168));

    let ds_orig = prepare_toa_contour_ds( &ds, 2071, N_HOURS).unwrap();
    assert_eq!( ds_orig.raster_size(), (100, 84));
}
