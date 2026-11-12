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

//! compute viewshed polygons for AlertCalifornia cameras and store them as `<camera-id>.geojson.gz` files that are
//! served by `AlertCaService` (default output dir is `$ODIN_ROOT/data/odin_alertca`). Cameras are either all cameras
//! of an `AlertCaConfig` (with positions from the current AlertCalifornia all_cameras data), or a single camera
//! given by id and position. The DEM is obtained through `odin_dem` (local VRT file or `serve_dem` server)

use std::{path::{Path, PathBuf}, time::Instant};
use anyhow::{Result, anyhow};
use reqwest::Client;
use odin_build::{data_dir, pkg_data_dir};
use odin_common::{BoundingBox, define_cli, fs::{EnvPathBuf, ensure_writable_dir, filepath_contents, gzip_path}, geo::GeoPoint, utm::geo_to_utm};
use odin_dem::{DemImgType, DemSRS, DemSource, ResampleAlg, dem_cache_dir, get_res_dem_filename};
use odin_gdal::{open_dataset, viewshed::{DEFAULT_CURV_COEFF, ViewshedParams, write_viewshed_geojson}};
use odin_alertca::{AlertCaConfig, all_cameras_filepath, camera_ids, get_latest_camera_data, load_config, parse_cameras};

define_cli! { ARGS [about="compute_vsh - compute viewshed GeoJSON polygons for AlertCalifornia cameras"] =
    config: String [help="AlertCaConfig with the cameras to process (if no --id is given)", long, default_value="sf_bay_area.ron"],
    id: Option<String> [help="id of single camera to process", long, requires_all=["lon","lat"]],
    lon: Option<f64> [help="longitude of single camera [deg]", long, allow_hyphen_values=true],
    lat: Option<f64> [help="latitude of single camera [deg]", long, allow_hyphen_values=true],
    height: Option<f64> [help="camera height above ground [m] (default: configured height_above_ground, or default_height_above_ground of config for --id)", long],
    target_height: f64 [help="height of visible targets above ground [m]", long, default_value="0"],
    max_dist: f64 [help="max viewshed distance [m]", long, default_value="40000"],
    res: f64 [help="DEM grid resolution [m]", long, default_value="30"],
    resample: String [help="DEM resampling algorithm (GDAL name: near, bilinear, cubic, average..)", long, default_value="bilinear"],
    min_patch: usize [help="min number of grid cells for visible and invisible patches (smaller ones are merged)", long, default_value="50"],
    simplify: f64 [help="polygon simplification tolerance [m]", long, default_value="30"],
    dem: Option<String> [help="odin_dem server URL or DEM VRT file (default: $ODIN_ROOT/data/odin_dem/3dep13-conus-i16/3dep13-conus-i16.vrt)", long],
    output_dir: Option<String> [help="output directory (default: $ODIN_ROOT/data/odin_alertca)", long, short]
}

#[tokio::main]
async fn main ()->Result<()> {
    let output_dir = ARGS.output_dir.as_ref().map( |d| PathBuf::from(d)).unwrap_or_else( || pkg_data_dir!());
    ensure_writable_dir( &output_dir)?;

    let dem_src = get_dem_source();
    let alg = ResampleAlg::from_name( &ARGS.resample).ok_or( anyhow!("unknown resampling algorithm '{}'", ARGS.resample))?;
    let mut params = ViewshedParams {
        observer_height: 0.0, // set per camera
        target_height: ARGS.target_height,
        max_distance: ARGS.max_dist,
        curv_coeff: DEFAULT_CURV_COEFF,
        min_patch_cells: ARGS.min_patch,
        simplify: ARGS.simplify,
        precision: 5,
    };

    let config: AlertCaConfig = load_config( &ARGS.config)?;
    let cameras: Vec<(String,GeoPoint,f64)> = if let Some(id) = &ARGS.id {
        // clap ensures lon,lat. Use the configured height if the camera is in the config
        let height = config.cameras.iter().find( |cc| cc.id.as_str() == id.as_str())
            .map( |cc| config.camera_attrs( cc).height_above_ground).unwrap_or( config.default_height_above_ground);
        vec![ (id.clone(), GeoPoint::from_lon_lat_degrees( ARGS.lon.unwrap(), ARGS.lat.unwrap()), height) ]
    } else {
        get_config_cameras( &config).await?
    };

    let mut n_failed = 0;
    for (id, pos, height) in &cameras {
        let t0 = Instant::now();
        params.observer_height = ARGS.height.unwrap_or( *height); // explicit --height overrides configured heights
        match compute_viewshed( &dem_src, alg, id, pos, &params, &output_dir).await {
            Ok((path, n_polys)) => println!("{id}: {n_polys} polygons -> {path:?} ({:.1}s)", t0.elapsed().as_secs_f64()),
            Err(e) => { eprintln!("{id}: failed to compute viewshed: {e}"); n_failed += 1; }
        }
    }

    if n_failed > 0 { Err( anyhow!("{n_failed} of {} viewsheds failed", cameras.len())) } else { Ok(()) }
}

fn get_dem_source ()->DemSource {
    match &ARGS.dem {
        Some(s) if s.starts_with("http://") || s.starts_with("https://") => DemSource::Server( s.clone()),
        Some(s) => DemSource::File( EnvPathBuf( PathBuf::from(s))),
        None => DemSource::File( EnvPathBuf( data_dir().join("odin_dem/3dep13-conus-i16/3dep13-conus-i16.vrt")))
    }
}

/// get (id,position,height_above_ground) of the configured cameras from the current AlertCalifornia all_cameras data.
/// If we cannot retrieve it we fall back to the cached all_cameras data (if any)
async fn get_config_cameras (config: &AlertCaConfig)->Result<Vec<(String,GeoPoint,f64)>> {
    let data = match get_latest_camera_data( &Client::new(), config).await {
        Ok(data) => data,
        Err(e) => {
            let path = all_cameras_filepath();
            eprintln!("failed to retrieve camera data ({e}), using cached {path:?}");
            filepath_contents( &path)?
        }
    };

    let cameras = parse_cameras( &data, &camera_ids( config))?;
    for cc in &config.cameras { // report the ones we don't have data for
        if !cameras.iter().any( |c| c.id == cc.id) { eprintln!("{}: no camera data", cc.id); }
    }

    Ok( cameras.into_iter().map( |c| {
        (c.id.to_string(), GeoPoint::from_lon_lat_degrees( c.position.longitude_degrees(), c.position.latitude_degrees()), c.height_above_ground)
    }).collect() )
}

/// compute and store the viewshed GeoJSON (gzipped) for a single camera, returning the path and number of polygons
async fn compute_viewshed (dem_src: &DemSource, alg: ResampleAlg, id: &str, pos: &GeoPoint, params: &ViewshedParams, output_dir: &Path)->Result<(PathBuf,usize)> {
    let utm = geo_to_utm( pos).ok_or( anyhow!("invalid camera position {pos:?}"))?;
    let (x, y) = (utm.easting.round(), utm.northing.round());
    let epsg = utm.utm_zone.epsg();
    let srs = DemSRS::from_epsg( epsg).ok_or( anyhow!("unsupported UTM zone {epsg}"))?;

    // the DEM covers the max distance (plus margin) around the camera. DEM files get cached by odin_dem
    let d = (params.max_distance + 2.0 * ARGS.res).ceil();
    let bbox = BoundingBox::new( x - d, y - d, x + d, y + d);
    let dem_path = dem_cache_dir().join( get_res_dem_filename( "vsh", epsg, &bbox, ARGS.res, ARGS.res, alg, "tif"));
    dem_src.get_res_dem( &bbox, srs, ARGS.res, ARGS.res, alg, DemImgType::TIF, &dem_path).await?;
    let dem = open_dataset( &dem_path)?;

    let props = format!( r#"{{"id":"{}","lon":{:.5},"lat":{:.5},"height":{},"targetHeight":{},"maxDist":{}}}"#,
                         id, pos.longitude_degrees(), pos.latitude_degrees(), params.observer_height, params.target_height, params.max_distance);
    let path = output_dir.join( format!("{id}.geojson"));
    let n_polys = write_viewshed_geojson( &dem, x, y, params, &props, &path)?;

    let gz_path = gzip_path( &path)?; // served as <id>.geojson (see AlertCaService)
    std::fs::remove_file( &path)?;

    Ok( (gz_path, n_polys) )
}
