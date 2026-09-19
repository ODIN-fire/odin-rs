
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


use std::path::{Path,PathBuf};
use uom::{ConstZero, si::{
    f64::Length, length::meter
}};
use chrono::{DateTime,Utc};
use ndarray::Array2;
use serde::Deserialize;

use odin_common::{
    datetime::{days,ymd},
    utm::UtmPoint,
    geo::GeoPoint,
    uom::LengthF32,
    net::{get_deserialized_cached_json_response}
};
use crate::{
    CACHE_DIR, DailyLocationData, LocationData, OpenMeteoConfig, VecDataSet, dataset::PrecipitationSum, errors::{Result, op_failed}, get_archive_url, get_point_query, grid::UtmGrid, ids::ModelId
};

/* #region annual precipitation for points and point collections ************************************************************************/

// note that Open-Meteo does not support bbox archive queries so we have to go through point lists

/// get sum of daily precitptation_sum values over last year for given point
/// since archive ops could be lengthy queries and we use daily values we cache the result to avoid rerunning queries
pub async fn get_annual_precipitation (config: &OpenMeteoConfig, pos: &GeoPoint, end: &DateTime<Utc>)->Result<LengthF32> {
    let start = *end - days(365);

    let fname = format!("daily-precip__{}__{}_{}.json", pos.encode(3), ymd( &start), ymd(end));
    let path = CACHE_DIR.join( fname);

    // short term archive queries should use ecmwf_ifs
    let url = get_archive_url( config, &DailyLocationData::<PrecipitationSum>::get_point_query(ModelId::ecmwf_ifs, &start, end, pos));
    let ld: DailyLocationData<PrecipitationSum> = get_deserialized_cached_json_response( &url, &path).await?;

    Ok( ld.daily.precipitation_sum.into_iter().fold( LengthF32::ZERO, |acc,ps| acc + ps) )
}

pub async fn get_cached_annual_precipitations<P> (config: &OpenMeteoConfig, pts: &[GeoPoint], end: &DateTime<Utc>, path: P)->Result<Vec<LengthF32>>
    where P: AsRef<Path>
{
    let start = *end - days(365);

    let url = get_archive_url( config, &DailyLocationData::<PrecipitationSum>::get_points_query(ModelId::ecmwf_ifs, &start, end, pts));
    let lds: Vec<DailyLocationData<PrecipitationSum>> = get_deserialized_cached_json_response( &url, path).await?;

    let sums: Vec<LengthF32> = lds.into_iter().map( |ld| {
        ld.daily.precipitation_sum.iter().fold( LengthF32::ZERO, |acc,ps| acc + *ps)
    }).collect();

    Ok(sums)
}

pub async fn get_cached_annual_grid_precipitations<T,P> (config: &OpenMeteoConfig, grid: &UtmGrid<T>, end: &DateTime<Utc>, path: P)->Result<Array2<LengthF32>>
    where P: AsRef<Path>, T: VecDataSet + Clone + for <'a> Deserialize<'a> + std::fmt::Debug // dataset type not used here but required for UtmGrid type bounds
{
    let pts = grid.get_geo_points();
    let pcps = get_cached_annual_precipitations(config, &pts, end, path).await?;

    let dim = grid.dim();
    let arr: Array2<LengthF32> = Array2::from_shape_vec( dim, pcps).map_err(|e| op_failed!("invalid vec length"))?; // can't happen

    Ok(arr)
}

/* #endregion annual precipitation */
