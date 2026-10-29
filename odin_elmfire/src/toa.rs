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

//! time-of-arrival (toa) post processing, i.e. generation of isochrone line contours and cumulative burn area polygons

use std::{fs::{File, remove_file}, io::BufReader, path::{Path, PathBuf}};
use gdal::Dataset;
use serde_json::{Value, json};

use odin_gdal::{
    contour::{ContourBuilder, to_fixed_level_geo_json},
    copy_mem_dataset, get_resolution, replace_rasterband_value,
    warp::{ResampleAlg, warp_to_res_mem}
};
use crate::errors::{OdinElmfireError, Result, op_failed};

/// nodata value of Elmfire toa rasters
const TOA_NODATA: f32 = -9999.0;

/// the time steps (in seconds) for which we want to compute time-of-arival contour isochrones
/// this starts 30min after first ignition and covers n_hours (from first ignition)
pub fn contour_levels (start_secs: i64, n_hours: u32)->Vec<f64> {
    let mut levels: Vec<f64> = Vec::with_capacity( n_hours as usize *2);
    let t_max = (start_secs + (n_hours as i64 * 3600) + 1) as f64;
    let mut t = (start_secs + 1800) as f64;

    while (t < t_max) {
        levels.push(t);
        t += 1800.0;
    }

    levels
}

/// create a MEM copy of the (already cropped) toa dataset that is suitable for contour generation: nodata cells are
/// replaced with a toa value that is beyond our last level, and small rasters get up-sampled to obtain smoother contours.
/// Note that nodata has to be replaced before we up-sample so that nodata positions are preserved
pub fn prepare_toa_contour_ds (ds: &Dataset, cell_threshold: usize, n_hours: u32)->Result<Dataset> {
    let (n_cols, n_rows) = ds.raster_size();
    let mut ds = copy_mem_dataset(ds)?;

    let max_toa = ((n_hours + 1) * 3600) as f32;
    replace_rasterband_value( &mut ds, 1, TOA_NODATA, max_toa)?;

    if n_cols * n_rows < cell_threshold {
        let (x_res, y_res) = get_resolution(&ds)?;
        ds = warp_to_res_mem( &ds, x_res / 2.0, y_res / 2.0, ResampleAlg::CubicSpline)?;
    }

    Ok(ds)
}

/// generate both the isochrone line contours (line_path) and the cumulative burn area polygons (poly_path) from
/// a (cropped) WGS84 toa dataset
pub fn process_toa_contours<P: AsRef<Path>> (ds: &Dataset, line_path: P, poly_path: P, cell_threshold: usize, start_secs: i64, n_hours: u32)->Result<()> {
    let ds = prepare_toa_contour_ds( ds, cell_threshold, n_hours)?;
    let levels = contour_levels( start_secs, n_hours);

    to_fixed_level_geo_json( &ds, 1, &levels, line_path)?;
    to_cumulative_poly_geo_json( &ds, &levels, poly_path)?;

    Ok(())
}

/// generate a GeoJSON FeatureCollection with one MultiPolygon feature per level that covers all cells with toa <= level.
/// Since polygon interior rings (holes) are preserved this represents the burned area up to the respective level.
///
/// The polygonizing GDAL contour generator creates polygons for the bands *between* levels so we run it for each level
/// separately with a [lower-bound, level] pair and keep the feature for the lower band (amax == level). We cannot
/// compute all bands at once and merge them since that would require a polygon union to eliminate shared edges
pub fn to_cumulative_poly_geo_json<P: AsRef<Path>> (ds: &Dataset, levels: &[f64], path: P)->Result<()> {
    let path = path.as_ref();
    let tmp_path = tmp_path_for( path);
    let mut features: Vec<Value> = Vec::with_capacity( levels.len());

    for level in levels {
        if tmp_path.is_file() { remove_file( &tmp_path)?; }

        let mut contourer = ContourBuilder::new( ds, &tmp_path)?;
        contourer
            .set_band( 1)
            .set_attr_min_name( "amin")?
            .set_attr_max_name( "amax")?
            .set_fixed_levels( &[0.0, *level]) // toa values are seconds since base date and hence >= 0
            .set_poly()
            .set_quiet();
        contourer.exec()?;
        drop(contourer); // make sure the target is closed before we read it

        let polys = read_band_polygons( &tmp_path, *level)?;
        if !polys.is_empty() {
            features.push( json!({
                "type": "Feature",
                "properties": { "level": level },
                "geometry": { "type": "MultiPolygon", "coordinates": polys }
            }));
        }
    }

    if tmp_path.is_file() { remove_file( &tmp_path)?; }

    let fc = json!({ "type": "FeatureCollection", "features": features });
    serde_json::to_writer( File::create( path)?, &fc)?;

    Ok(())
}

fn tmp_path_for (path: &Path)->PathBuf {
    let mut p = path.to_path_buf();
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("toa_poly").to_string();
    p.set_file_name( format!("{stem}.tmp.geojson"));
    p
}

/// get the polygon coordinates of all features in the GeoJSON file that have an 'amax' property equal to the given level
fn read_band_polygons (path: &Path, level: f64)->Result<Vec<Value>> {
    let fc: Value = serde_json::from_reader( BufReader::new( File::open( path)?))?;
    let features = fc["features"].as_array().ok_or( op_failed!("no features in {:?}", path))?;
    let mut polys: Vec<Value> = Vec::new();

    for f in features {
        let amax = f["properties"]["amax"].as_f64();
        if amax.is_some_and( |v| (v - level).abs() < 0.5) {
            let geom = &f["geometry"];
            match geom["type"].as_str() {
                Some("MultiPolygon") => if let Some(cs) = geom["coordinates"].as_array() { polys.extend( cs.iter().cloned()) }
                Some("Polygon") => polys.push( geom["coordinates"].clone()),
                _ => {}
            }
        }
    }

    Ok(polys)
}
