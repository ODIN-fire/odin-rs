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
use std::{path::{Path,PathBuf}, sync::Arc};
use gdal::{Metadata, raster::RasterCreationOptions};

use odin_gdal::{grid::GdalGridAlgorithmOptions,set_band_meta};
use odin_wx::WxDataSetRequest;
use odin_hrrr::meta;

use crate::{OpenMeteoDataSet, OpenMeteoLocationData, OpenMeteoTemporal, Result, create_datasets, errors::op_failed, dataset::{BasicEcmwfIfs,BasicEcmwfIfsData}};

/// re-grid an Open-Meteo dataset (represented as a `Vec<OpenMeteoLocationData<T>>`) into an equidistant
/// geotiff grid with bands that correspond to variables/meta info according to HRRR forecasts
pub fn basic_ecmwf_ifs_to_hrrr<P,U> (req: &WxDataSetRequest, path: P, tgt_dir: U)->Result<Vec<Arc<PathBuf>>>
    where P: AsRef<Path>, U: AsRef<Path>
{
    let data: Vec<OpenMeteoLocationData<BasicEcmwfIfsData>> = OpenMeteoLocationData::parse_path( path)?;

    fn push_timestep (d: &OpenMeteoLocationData<BasicEcmwfIfsData>, ts: usize, tgt: &mut Vec<Vec<f64>>) {
        if let Some(d) = d.hourly_data.as_ref().map(|hd| &hd.hourly) {
            let ws10  = d.wind_speed_10m[ts] as f64;
            let wd10  = (d.wind_direction_10m[ts] as f64).to_radians();
            let ws100 = d.wind_speed_80m[ts] as f64;  // note OpenMeteo only has 10/80/120/180m winds
            let wd100 = (d.wind_direction_80m[ts] as f64).to_radians();

            tgt[0].push( d.temperature_2m[ts].into());
            tgt[1].push( d.relative_humidity_2m[ts].into());
            tgt[2].push( d.surface_pressure[ts].into());
            tgt[3].push( d.cloud_cover[ts].into());
            tgt[4].push( -ws10 * wd10.sin());
            tgt[5].push( -ws10 * wd10.cos());
            tgt[6].push( -ws100 * wd100.sin());
            tgt[7].push( -ws100 * wd100.cos());
        }
    }

    if data.is_empty() {
        Ok( Vec::with_capacity(0) )
    } else {
        let alg = GdalGridAlgorithmOptions::nearest_neighbor_within( 0.2, -9999.0);
        let ref_data = data[0].hourly_data.as_ref().ok_or( op_failed!("no hourly data"))?;
        let n_time_steps = ref_data.hourly.n_time_steps();

        let co = RasterCreationOptions::from_iter(["COMPRESS=DEFLATE", "PREDICTOR=2"]);
        let mut res = create_datasets::<BasicEcmwfIfsData,_,f32,_>(
            req, data.as_slice(), OpenMeteoTemporal::Hourly, 0..n_time_steps, &alg, tgt_dir, "tif" ,&co, 8, 0.1, 0.1, push_timestep)?;

        let mut paths: Vec<Arc<PathBuf>> = Vec::new();
        for ds in res.iter_mut() {
            let fname = ds.description()?;
            let fc_epoch = ds.metadata_item("forecast_epoch", "").unwrap();
            let dt_grib_meta = &[
                ("GRIB_REF_TIME", fc_epoch.as_str(), ""),
                ("GRIB_VALID_TIME", fc_epoch.as_str(), ""),
                ("GRIB_FORECAST_SECONDS", "0", "")
            ];
            set_band_meta( ds, 1, meta::TMP_2_HTGL_GRIB_META);     set_band_meta( ds, 1, dt_grib_meta);
            set_band_meta( ds, 2, meta::RH_2_HTGL_GRIB_META);      set_band_meta( ds, 2, dt_grib_meta);
            set_band_meta( ds, 3, meta::PRES_0_SFC_GRIB_META);     set_band_meta( ds, 3, dt_grib_meta);
            set_band_meta( ds, 4, meta::TCDC_0_EATM_GRIB_META);    set_band_meta( ds, 4, dt_grib_meta);
            set_band_meta( ds, 5, meta::UGRD_10_HTGL_GRIB_META);   set_band_meta( ds, 5, dt_grib_meta);
            set_band_meta( ds, 6, meta::VGRD_10_HTGL_GRIB_META);   set_band_meta( ds, 6, dt_grib_meta);
            set_band_meta( ds, 7, meta::UGRD_80_HTGL_GRIB_META);   set_band_meta( ds, 7, dt_grib_meta);  // should be 100
            set_band_meta( ds, 8, meta::VGRD_80_HTGL_GRIB_META);   set_band_meta( ds, 8, dt_grib_meta);  // ditto

            paths.push( Arc::new( Path::new(&fname).to_path_buf()) );
        }

        Ok( paths )
    }
}
