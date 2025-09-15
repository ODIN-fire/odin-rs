/*
 * Copyright © 2024, United States Government, as represented by the Administrator of 
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

use std::path::Path;
use gdal::{Dataset, spatial_ref::SpatialRef};
use odin_common::define_cli;
use odin_gdal::{get_driver_name_from_filename, to_csl_string_list, warp::{ResampleAlg, SimpleWarpBuilder}, GdalDataType};
use anyhow::{Result, anyhow};

define_cli! { ARGS [about="simple GDAL warper"] =
    te: Option<Vec<f64>> [help="target extent xmin, ymin, xmax, ymax", long, allow_hyphen_values=true, num_args=4],
    t_srs: Option<String> [help="target SRS", long],
    t_format: Option<String> [help="optional target format (default is GTiff)", long],
    t_type: Option<String> [help="GDAL target type (I16, F32 etc.)", long],
    t_res: Option<Vec<f64>> [help="optional target pixel resolution", long, allow_hyphen_values=true, num_args=2],
    s_nodata: Vec<f64> [help="source NO_DATA value", long, allow_hyphen_values=true],
    t_nodata: Vec<f64> [help="target NO_DATA_VALUE", long, allow_hyphen_values=true],
    s_band: Option<Vec<u32>> [help="source bands to process (1-based)", short, long],
    t_band: Option<Vec<u32>> [help="target bands to store (1-based)", short, long],
    resample_alg: Option<String> [help="resample algorithm to use (see GDAL doc)", short, long],
    co: Vec<String> [help="optional target create options", long, num_args=1],
    err_threshold: Option<f64> [help="optional max output error threshold (default 0.0)", long],

    src_filename: String [help="input filename"],
    tgt_filename: String [help="output filename"]
}


fn main () -> Result<()> {
    let src_path = Path::new(ARGS.src_filename.as_str());
    let src_ds = Dataset::open(src_path)?;
    let tgt_path = Path::new(ARGS.tgt_filename.as_str());

    let tgt_srs_opt: Option<SpatialRef> = if let Some(srs_def) = &ARGS.t_srs {
        Some(SpatialRef::from_definition(srs_def.as_str())?)
        //Some(SpatialRef::from_proj4(srs_def.as_str())?)
    } else { None };

    let co_list_opt = to_csl_string_list(&ARGS.co)?;

    let tgt_format: &str = if let Some(ref fmt) = ARGS.t_format {
        fmt.as_str()
    } else {
        if let Some(driver_name) = get_driver_name_from_filename(ARGS.tgt_filename.as_str()) {
            driver_name
        } else {
            "GTiff" // our last fallback
        }
    };

    let mut warper = SimpleWarpBuilder::new( &src_ds, tgt_path)?;
    if let Some(v) = &ARGS.te { warper.set_tgt_extent(v[0],v[1],v[2],v[3]); }
    if let Some(ref tgt_srs) = tgt_srs_opt { warper.set_tgt_srs(tgt_srs); }
    if let Some (ref co_list) = co_list_opt { warper.set_create_options(co_list); }
    if let Some(t_res) = &ARGS.t_res { warper.set_tgt_resolution( t_res[0], t_res[1]); }
    if let Some(max_error) = ARGS.err_threshold { warper.set_max_error(max_error); }
    if let Some(alg_name) = &ARGS.resample_alg { warper.set_resample_alg( get_resample_alg(alg_name.as_str())? ); }
    if let Some(s_bands) = &ARGS.s_band { warper.set_src_bands(s_bands.clone()); }
    if let Some(t_bands) = &ARGS.t_band { warper.set_tgt_bands(t_bands.clone()); }
    if let Some(data_type) = &ARGS.t_type {  warper.set_data_type( get_data_type(&data_type)?); }

    if !ARGS.s_nodata.is_empty() { warper.set_src_nodatas( ARGS.s_nodata.clone()); }
    if !ARGS.t_nodata.is_empty() { warper.set_tgt_nodatas( ARGS.t_nodata.clone()); }

    warper.set_tgt_format(tgt_format)?;

    warper.exec()?;

    // note that Dataset has a Drop impl so we don't need to close here - we would get a segfault from GDAL if we do

    Ok(())
}

fn get_resample_alg (name: &str)->Result<ResampleAlg> {
    match name {
        "near" => Ok(ResampleAlg::NearestNeighbour),
        "bilinear" => Ok(ResampleAlg::Bilinear),
        "cubic" => Ok(ResampleAlg::Cubic),
        "cubicspline" => Ok(ResampleAlg::CubicSpline),
        "lanczos" => Ok(ResampleAlg::Lanczos),
        "average" => Ok(ResampleAlg::Average),
        "rms" => Ok(ResampleAlg::RMS),
        "mode" => Ok(ResampleAlg::Mode),
        "min" => Ok(ResampleAlg::Min),
        "max" => Ok(ResampleAlg::Max),
        "med" => Ok(ResampleAlg::Med),
        "q1" => Ok(ResampleAlg::Q1),
        "q3" => Ok(ResampleAlg::Q3),
        "sum" => Ok(ResampleAlg::Sum),
        _ => Err( anyhow!("unknown resample algorithm"))
    }
}

fn get_data_type (name: &str)->Result<GdalDataType> {
    match name {
        "Byte" => Ok(GdalDataType::UInt8),
        "Int8" => Ok(GdalDataType::Int8),
        "UInt16" => Ok(GdalDataType::UInt16),
        "Int16" => Ok(GdalDataType::Int16),
        "UInt32" => Ok(GdalDataType::UInt32),
        "Int32" => Ok(GdalDataType::Int32),
        "UInt64" => Ok(GdalDataType::UInt64),
        "Int64" => Ok(GdalDataType::Int64),
        "Float32" => Ok(GdalDataType::Float32),
        "Float64" => Ok(GdalDataType::Float64),
        _ => Err( anyhow!("unknown GDAL data type"))
    }
}