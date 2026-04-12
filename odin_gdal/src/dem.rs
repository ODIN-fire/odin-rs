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

use std::{ffi::{CString}, ptr::{null, null_mut}, path::{Path,PathBuf}};

use gdal::{DriverManager, Driver, raster::{GdalType,Buffer}, Dataset, spatial_ref::SpatialRef};
use gdal_sys::{GDALDEMProcessing,GDALDEMProcessingOptions,GDALDEMProcessingOptionsNew,GDALDEMProcessingOptionsFree};

use crate::errors::{Result, last_gdal_error, misc_error, reset_last_gdal_error};

/// the algorithm to use for slope and aspect computation
pub enum GdalDemAlg {
    ZevenbergenThorne, // for smooth terrain
    Horn, // for rough terrain
}

impl GdalDemAlg {
    pub fn for_rough_terrain ()->Self { Self::Horn }
    pub fn for_smooth_terrain ()->Self { Self::ZevenbergenThorne }

    fn c_name (&self)->&'static str {
        match *self {
            GdalDemAlg::Horn => "Horn",
            GdalDemAlg::ZevenbergenThorne => "ZevenbergenThorne",
        }
    }
}

pub enum GdalDemOp {
    Aspect,
    Slope
}

impl GdalDemOp {
    fn c_name (&self)->&'static str {
        match *self {
            GdalDemOp::Aspect => "aspect",
            GdalDemOp::Slope => "slope",
        }
    }
}

pub fn create_aspect_ds<P> (elev_ds: &Dataset, out_path: P, alg: GdalDemAlg, trigonometric: bool, zero_for_flat: bool)->Result<Dataset>
    where P: AsRef<Path>
{
    create_dem_ds( elev_ds, out_path, GdalDemOp::Aspect, alg, trigonometric, zero_for_flat)
}

pub fn create_slope_ds<P> (elev_ds: &Dataset, out_path: P, alg: GdalDemAlg, trigonometric: bool, zero_for_flat: bool)->Result<Dataset>
    where P: AsRef<Path>
{
    create_dem_ds( elev_ds, out_path, GdalDemOp::Slope, alg, trigonometric, zero_for_flat)
}

/// create an aspect dataset from a given elevation dataset
pub fn create_dem_ds<P> (elev_ds: &Dataset, out_path: P, op: GdalDemOp, alg: GdalDemAlg, trigonometric: bool, zero_for_flat: bool)->Result<Dataset>
    where P: AsRef<Path>
{
    let mut argv: Vec<CString> = Vec::new();

    argv.push( CString::new("-alg")?);
    argv.push( CString::new(alg.c_name())?);
    argv.push( CString::new("-compute_edges")?);
    if trigonometric { argv.push( CString::new("-trigonometric")? ) }
    if zero_for_flat { argv.push( CString::new("-zero_for_flat")?) }

    if let Some(ext) = out_path.as_ref().extension() {
        if ext == "tif" {
            argv.push(CString::new("-co")?); argv.push( CString::new("COMPRESS=DEFLATE")?);
            argv.push(CString::new("-co")?); argv.push( CString::new("PREDICTOR=2")?);
        }
    }

    unsafe {
        let mut opt_args: Vec<*mut i8> = argv.iter().map( |s| s.as_ptr() as *mut i8).collect();
        let opts = GDALDEMProcessingOptionsNew( opt_args.as_mut_ptr(), null_mut());
        let path = CString::new( out_path.as_ref().to_str().ok_or( misc_error("invalid pathname".into()))?)?;
        let op = CString::new( op.c_name())?;

        reset_last_gdal_error();
        let mut err: i32 = 0;
        let h_dem = GDALDEMProcessing(
            path.as_ptr(),
            elev_ds.c_dataset(),
            op.as_ptr(),
            null(), // no color file for 'aspect'
            opts,
            &mut err
        );

        GDALDEMProcessingOptionsFree( opts);

        if err == 0 {
            let mut ds = Dataset::from_c_dataset( h_dem);
            ds.flush_cache()?;
            Ok( ds)
        } else {
            Err( last_gdal_error())
        }
    }
}
