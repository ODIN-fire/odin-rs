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
use gdal::{Metadata};

use odin_gdal::{grid::GdalGridAlgorithmOptions,set_band_meta};
use odin_wx::WxDataSetRequest;
use odin_hrrr::meta;

use crate::{OpenMeteoLocationData,BasicEcmwfIfs,BasicEcmwfIfsData,Result};

/// re-grid an Open-Meteo dataset (represented as a `Vec<OpenMeteoLocationData<T>>`) into an equidistant
/// geotiff grid with bands that correspond to variables/meta info according to HRRR forecasts
pub fn basic_ecmwf_ifs_to_hrrr<P,U> (req: &WxDataSetRequest, path: P, tgt_dir: U)->Result<Vec<Arc<PathBuf>>>
where P: AsRef<Path>, U: AsRef<Path>
{
    let data: Vec<OpenMeteoLocationData<BasicEcmwfIfsData>> = OpenMeteoLocationData::parse_path( path)?;

    fn push_timestep (d: &OpenMeteoLocationData<BasicEcmwfIfsData>, ts: usize, tgt: &mut Vec<Vec<f64>>) {
        let ws10 = d.hourly.wind_speed_10m[ts] as f64;
        let wd10 = (d.hourly.wind_direction_10m[ts] as f64).to_radians();
        let ws100 = d.hourly.wind_speed_10m[ts] as f64;
        let wd100 = (d.hourly.wind_direction_10m[ts] as f64).to_radians();

        tgt[0].push( d.hourly.temperature_2m[ts].into());
        tgt[1].push( d.hourly.relative_humidity_2m[ts].into());
        tgt[2].push( d.hourly.surface_pressure[ts].into());
        tgt[3].push( d.hourly.cloud_cover[ts].into());
        tgt[4].push( -ws10 * wd10.sin());
        tgt[5].push( -ws10 * wd10.cos());
        tgt[6].push( -ws100 * wd100.sin());
        tgt[7].push( -ws100 * wd100.cos());
    }

    if data.is_empty() {
        Ok( Vec::with_capacity(0) )
    } else {
        let alg = GdalGridAlgorithmOptions::nearest_neighbor_within( 0.2, -9999.0);
        let n_timesteps = data[0].n_time_steps();
        let mut res = BasicEcmwfIfs::create_datasets::<_,f32,_>( req, data.as_slice(), 0..n_timesteps, &alg, tgt_dir, "tif", 8, push_timestep)?;

        let mut paths: Vec<Arc<PathBuf>> = Vec::new();
        for ds in res.iter_mut() {
            let fname = ds.description()?;
            let fc_epoch = ds.metadata_item("forecast_epoch", "").unwrap();
            let dt_grib_meta = &[
                ("GRIB_REF_TIME", fc_epoch.as_str(), ""),
                ("GRIB_VALID_TIME", fc_epoch.as_str(), ""),
                ("GRIB_FORECAST_SECONDS", "0", "")
            ];
            set_band_meta( ds, 1, meta::TMP_2_HTGL_GRIB_META);      set_band_meta( ds, 1, dt_grib_meta);
            set_band_meta( ds, 2, meta::RH_2_HTGL_GRIB_META);       set_band_meta( ds, 2, dt_grib_meta);
            set_band_meta( ds, 3, meta::PRES_0_SFC_GRIB_META);      set_band_meta( ds, 3, dt_grib_meta);
            set_band_meta( ds, 4, meta::TCDC_0_EATM_GRIB_META);     set_band_meta( ds, 4, dt_grib_meta);
            set_band_meta( ds, 5, meta::UGRD_10_HTGL_GRIB_META);    set_band_meta( ds, 5, dt_grib_meta);
            set_band_meta( ds, 6, meta::VGRD_10_HTGL_GRIB_META);    set_band_meta( ds, 6, dt_grib_meta);
            set_band_meta( ds, 7, meta::UGRD_80_HTGL_GRIB_META);   set_band_meta( ds, 7, dt_grib_meta);  // should be 100
            set_band_meta( ds, 8, meta::VGRD_80_HTGL_GRIB_META);   set_band_meta( ds, 8, dt_grib_meta);  // ditto

            paths.push( Arc::new( Path::new(&fname).to_path_buf()) );
        }

        Ok( paths )
    }
}
