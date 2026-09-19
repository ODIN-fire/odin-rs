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
use uom::si::{pressure::hectopascal, thermodynamic_temperature::degree_celsius, velocity::meter_per_second, angle::degree,ratio::percent};

use odin_common::{define_cli, geo::GeoRect, datetime::{days,utc_now}};
use odin_gdal::{get_gtiff_create_opts, grid::GdalGridAlgorithmOptions, is_tiff_ext, no_create_opts};
use odin_wx::{WxDataSetRequest};
use odin_openmeteo::{
    DataSet, VecDataSet, LocationData, HourlyLocationData, OpenMeteoTemporal,
    dataset::{BasicData},
    convert::create_hourly_geo_ds,
    parse_location_data
};

define_cli! { ARGS [about="create timestep tif files with hourly BasicData from JSON file"] =
    model: String [help="name of forecast model to use", long, default_value="ecmwf_ifs"],
    region: String [help="name of region for output file", short, long, default_value=""],
    bbox: Option<Vec<f64>> [help="WSEN bounding box for grid", short, long, allow_hyphen_values=true, num_args=4],
    alg: Option<String> [help="name of gridding algorithm to use (nearest,linear)", short, long, default_value="nearest"],
    tgt_dir: String [help="target directory where to store timestep grid files", long, default_value="../../cache/odin_openmeteo"],
    ext: String [help="file extension for GDAL data set to generate", short, long, default_value="tif"],
    input_file: String [help="filename of JSON input file"]
}

fn main ()->Result<()> {
    let data: Vec<HourlyLocationData<BasicData>> = parse_location_data( &ARGS.input_file)?;
    if data.is_empty() {
        return Err( anyhow!("empty data file"))
    }

    let ref_data = &data[0];
    let n_timesteps = ref_data.hourly.n_time_steps();

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

    let create_opts = if is_tiff_ext( &ARGS.ext) { get_gtiff_create_opts() } else { no_create_opts() };

    let mut res = create_hourly_geo_ds::<BasicData,_,f32,_> (
        data.as_slice(),
        0..n_timesteps,
        &alg,
        &ARGS.tgt_dir, &ARGS.region, &ARGS.model, BasicData::dataset_name(), &ARGS.ext,
        &create_opts,
        8,
        &bbox,
        0.1, 0.1,
        |d, timestep, v| {
            v[0].push( d.temperature_2m[timestep].get::<degree_celsius>().into());
            v[1].push( d.surface_pressure[timestep].get::<hectopascal>().into());
            v[2].push( d.relative_humidity_2m[timestep].get::<percent>().into());
            v[3].push( d.cloud_cover[timestep].get::<percent>().into());
            v[4].push( d.wind_speed_10m[timestep].get::<meter_per_second>().into());
            v[5].push( d.wind_direction_10m[timestep].get::<degree>().into());
            v[6].push( d.wind_speed_100m[timestep].get::<meter_per_second>().into());
            v[7].push( d.wind_direction_100m[timestep].get::<degree>().into());
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

fn get_bbox (data: &[HourlyLocationData<BasicData>])->GeoRect {
    let mut x_min: f64 = f64::MAX;
    let mut x_max: f64 = f64::MIN;
    let mut y_min: f64 = f64::MAX;
    let mut y_max: f64 = f64::MIN;

    for d in data {
        let x = d.meta_data.longitude as f64;
        let y = d.meta_data.latitude as f64;

        if x < x_min { x_min = x }
        if x > x_max { x_max = x }
        if y < y_min { y_min = y }
        if y > y_max { y_max = y }
    }

    GeoRect::from_wsen_degrees(x_min, y_min, x_max, y_max)
}
