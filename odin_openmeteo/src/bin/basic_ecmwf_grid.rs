/*
 * Copyright © 2026, United States Government, as represented by the Administrator of
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

use std::{sync::Arc,any::type_name};
use gdal::{Metadata};
use anyhow::{anyhow,Result};
use odin_common::{define_cli, geo::GeoRect, datetime::{days,utc_now}};
use odin_gdal::{get_gtiff_create_opts, grid::GdalGridAlgorithmOptions, is_tiff_ext, no_create_opts};
use odin_wx::{WxDataSetRequest};
use odin_openmeteo::{
    OpenMeteoDataSet, OpenMeteoLocationData, OpenMeteoTemporal, create_datasets, dataset::{BasicEcmwfIfs, BasicEcmwfIfsData}, get_fc_days, url_query_hourly_bbox, wx_service:: OpenMeteoService
};

define_cli! { ARGS [about="grid Basic ECMWF-IFS JSON file"] =
    model: String [help="name of forecast model to use", long, default_value="ecmwf_ifs"],
    region: String [help="name of region for output file", short, long, default_value=""],
    bbox: Option<Vec<f64>> [help="WSEN bounding box for grid", short, long, allow_hyphen_values=true, num_args=4],
    days: u32 [help="number of forecast days", short, long, default_value="1"],
    alg: Option<String> [help="name of gridding algorithm to use (nearest,linear)", short, long, default_value="nearest"],
    tgt_dir: String [help="target directory where to store timestep grid files", long, default_value="../../cache/odin_openmeteo"],
    ext: String [help="file extension for GDAL data set to generate", short, long, default_value="tif"],
    input_file: String [help="filename of JSON input file"]
}

fn main ()->Result<()> {
    let data: Vec<BasicEcmwfIfs> = OpenMeteoLocationData::parse_path( &ARGS.input_file)?;
    if data.is_empty() {
        return Err( anyhow!("empty data file"))
    }

    let ref_data = &data[0];
    let n_timesteps = ref_data.hourly_data.as_ref().ok_or( anyhow!("no hourly data"))?.n_time_steps();

    let alg: GdalGridAlgorithmOptions = if let Some(alg) = &ARGS.alg {
        match alg.as_str() {
            "nearest" => GdalGridAlgorithmOptions::nearest_neighbor_within( 0.3, -9999.0),
            "linear" => GdalGridAlgorithmOptions::linear( 0.3, -9999.0),
            _ => return Err( anyhow!("unknown grid mapping algorithm"))
        }
    } else {
        GdalGridAlgorithmOptions::nearest_neighbor_within( 0.2, -9999.0)
    };

    let bbox = if let Some(wsen) = &ARGS.bbox {
        GeoRect::from_wsen_degrees(wsen[0], wsen[1], wsen[2], wsen[3])
    } else {
        get_bbox( &data)  // get bbox from data values
    };

    let region = Arc::new( ARGS.region.clone());
    let wx_name = Arc::new( type_name::<OpenMeteoService>().to_string());
    let model_name = Arc::new( ARGS.model.clone());
    let dataset_name = Arc::new( BasicEcmwfIfsData::dataset_name().to_string());

    let fc_duration = days( ARGS.days as u64);
    let fc_days = get_fc_days( &utc_now(), fc_duration);

    let query = url_query_hourly_bbox::<BasicEcmwfIfsData>( &bbox, fc_days, 0, model_name.as_str());
    let req = WxDataSetRequest::new( region, bbox, wx_name, model_name, dataset_name, fc_duration, query);
    let create_opts = if is_tiff_ext( &ARGS.ext) { get_gtiff_create_opts() } else { no_create_opts() };

    let mut res = create_datasets::<BasicEcmwfIfsData,_,f32,_> (
        &req, data.as_slice(),
        OpenMeteoTemporal::Hourly, 0..n_timesteps,
        &alg, &ARGS.tgt_dir, &ARGS.ext, &create_opts,
        8,
        0.1, 0.1,
        |ld, timestep, v| {
            if let Some(d) = ld.hourly_data.as_ref().map( |hd| &hd.hourly) {
                v[0].push( d.temperature_2m[timestep].into());
                v[1].push( d.surface_pressure[timestep].into());
                v[2].push( d.relative_humidity_2m[timestep].into());
                v[3].push( d.cloud_cover[timestep].into());
                v[4].push( d.wind_speed_10m[timestep].into());
                v[5].push( d.wind_direction_10m[timestep].into());
                v[6].push( d.wind_speed_80m[timestep].into());
                v[7].push( d.wind_direction_80m[timestep].into());
            }
        }
    )?;

    for ds in res.iter_mut() {
        let fname = ds.description()?;
        // NOTE these have to be the corresponding HRRR descriptions
        ds.rasterband(1)?.set_description("temperature_2m")?;
        ds.rasterband(2)?.set_description("surface_pressure")?;
        ds.rasterband(3)?.set_description("relative_humidity_2m")?;
        ds.rasterband(4)?.set_description("cloud_cover")?;
        ds.rasterband(5)?.set_description("wind_speed_10m")?;
        ds.rasterband(6)?.set_description("wind_direction_10m")?;
        ds.rasterband(7)?.set_description("wind_speed_100m")?;
        ds.rasterband(8)?.set_description("wind_direction_100m")?;

        println!( "created {:?}", fname);
    }

    Ok(())
}

fn get_bbox (data: &[OpenMeteoLocationData<BasicEcmwfIfsData>])->GeoRect {
    let mut x_min: f64 = f64::MAX;
    let mut x_max: f64 = f64::MIN;
    let mut y_min: f64 = f64::MAX;
    let mut y_max: f64 = f64::MIN;

    for d in data {
        let x = d.metadata.longitude as f64;
        let y = d.metadata.latitude as f64;

        if x < x_min { x_min = x }
        if x > x_max { x_max = x }
        if y < y_min { y_min = y }
        if y > y_max { y_max = y }
    }

    GeoRect::from_wsen_degrees(x_min, y_min, x_max, y_max)
}
