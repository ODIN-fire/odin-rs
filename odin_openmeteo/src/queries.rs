
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
    collections::Grid,
    datetime::{days,ymd,ymd_hms_enc},
    utm::UtmPoint,
    geo::GeoPoint,
    uom::{LengthF32,ToF64Quantity},
    net::{get_deserialized_cached_json_response}
};
use crate::{
    CACHE_DIR, DailyLocationData, LocationData, OpenMeteoConfig, VecDataSet,
    dataset::{Fw21Data, PrecipitationSum}, errors::{Result, op_failed}, get_archive_url, get_point_query,
    grid::{GridAlgorithm, UtmGrid}, ids::ModelId
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

// NOTE - using odin_common::containers::Grid is a workaround for the infamous
//       "Future not Send.. expected struct `Quantity<..., ..., f64>` found struct `Quantity<..., ..., f64>`"
// With ndarray v0.17.2 we can't directly use Array2<Length> in the loop and UtmGrid should not have any DeserializeOwned
// bounds on the type (only on the deserializing methods)
// TODO - remove workaround if fixed in rustc (https://github.com/rust-lang/rust/issues/162558)
// and/or ndarray (https://github.com/rust-ndarray/ndarray/issues/1617)

pub async fn get_annual_precip (
    wx_config: &OpenMeteoConfig, dir: &PathBuf,
    grid: &UtmGrid<Fw21Data>, geo_center: &GeoPoint,
    date: &DateTime<Utc>
) -> Result<Grid<Length>>
{
    // note the cache filename has to include the grid dimensions since it is only valid for this grid
    let center_enc = geo_center.encode(4);
    let (ny,nx) = grid.dim();
    let path = dir.join( format!("annual_precip__{}__{}x{}__{:.0}m__{}.json", center_enc, nx, ny, grid.cell_size.get::<meter>(), ymd(date)));

    let sum_f32 = get_cached_annual_grid_precipitations( wx_config, grid, date, &path).await?;
    let sum_f64 = sum_f32.map( |l| l.to_f64() ); // NOTE - this has to be the same unit as we use in the compute_nfdrs_recs argument

    Ok( Grid::from_array2(sum_f64) )
}

/* #endregion annual precipitation */

/* #region Fw21 grids *******************************************************************************************************************/

/// get a UtmGrid with (2*nx2+1) x (2*ny2+1) cells of size `res` around `geo_center` that is populated with
/// FW21 data from `start` to `end`. The Open-Meteo response is cached in `dir`
pub async fn get_fw21_grid (
    wx_config: &OpenMeteoConfig, wx_model: ModelId, dir: &PathBuf,
    geo_center: &GeoPoint, res: Length, nx2: usize, ny2: usize,
    start: &DateTime<Utc>, end: &DateTime<Utc>
) -> Result<UtmGrid<Fw21Data>>
{
    let mut grid: UtmGrid<Fw21Data> = UtmGrid::from_center( geo_center, res, nx2, ny2)?;

    // make sure we get all the OpenMeteo data in one query
    // note the cache filename has to include the grid dimensions since it is only valid for this grid
    let center_enc = geo_center.encode(4);
    let path = dir.join( format!("fw21__{}__{}x{}__{:.0}m__{}.json", center_enc, 2*nx2+1, 2*ny2+1, res.get::<meter>(), ymd_hms_enc( end)));
    grid.get_cached_hourly_data( wx_config, wx_model, start, end, GridAlgorithm::idw( 1.5, 2.0), &path).await?;

    Ok(grid)
}

/* #endregion Fw21 grids */
