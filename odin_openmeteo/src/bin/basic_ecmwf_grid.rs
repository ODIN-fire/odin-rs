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
use std::{sync::Arc,any::type_name};
use gdal::Metadata;
use anyhow::{anyhow,Result};
use odin_common::{define_cli, geo::GeoRect, datetime::days};
use odin_gdal::{grid::GdalGridAlgorithmOptions};
use odin_wx::{WxDataSetRequest};
use odin_openmeteo::{
    BasicEcmwfIfs, BasicEcmwfIfsData, OpenMeteoData, OpenMeteoLocationData, OpenMeteoService,
    data_url_query, fields::{FieldId}, push_timestep
};

define_cli! { ARGS [about="grid Basic ECMWF-IFS JSON file"] =
    model: String [help="name of forecast model to use", long, default_value="ecmwf_ifs"],
    region: String [help="name of region for output file", short, long, default_value=""],
    bbox: Option<Vec<f64>> [help="WSEN bounding box for grid", short, long, allow_hyphen_values=true, num_args=4],
    days: u64 [help="number of forecast days", short, long, default_value="1"],
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
    let n_timesteps = data[0].n_time_steps();

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
    let fc_duration = days( ARGS.days);
    let fields = BasicEcmwfIfsData::hourly_fields();
    let fields_query = FieldId::as_list_string( &fields);
    let query = data_url_query( &bbox, fc_duration, model_name.as_str(), fields_query.as_str());
    let req = WxDataSetRequest::new( region, bbox, wx_name, model_name, dataset_name, fc_duration, query);
    let mut res = BasicEcmwfIfs::create_datasets::<_,f32,_> (&req, data.as_slice(), 0..n_timesteps, &alg, &ARGS.tgt_dir, &ARGS.ext, BasicEcmwfIfsData::n_hourly_fields(), push_timestep)?;

    for ds in res.iter_mut() {
        let fname = ds.description()?;
        for band_no in 1..=ds.raster_count() {
            let mut band = ds.rasterband( band_no)?;
            band.set_description( fields[band_no-1].as_ref())?;
        }

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
        let x = d.longitude as f64;
        let y = d.latitude as f64;

        if x < x_min { x_min = x }
        if x > x_max { x_max = x }
        if y < y_min { y_min = y }
        if y > y_max { y_max = y }
    }

    GeoRect::from_wsen_degrees(x_min, y_min, x_max, y_max)
}
