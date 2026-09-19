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

/// support for gridding scattered data collections
/// (Note this is not very efficient for re-gridding with different resolution)

use std::{mem::size_of, path::Path, ptr, fmt::Debug};
use std::os::raw::{c_void, c_uint, c_double};

use ndarray::Array2;
use gdal::raster::RasterCreationOptions;
use gdal::{DriverManager, Driver, raster::{GdalType,Buffer}, Dataset, spatial_ref::SpatialRef};
use gdal_sys;

use odin_common::num_type::NumericType;
use crate::create_dataset;
use crate::errors::{Result, last_gdal_error, misc_error};

/* #region GDALGridAlgorithm structs *************************************************************************/

/// see https://gdal.org/en/stable/tutorials/gdal_grid_tut.html
pub enum GdalGridAlgorithmOptions {
    NearestNeighbor(GDALGridNearestNeighborOptions),
    Linear(GDALGridLinearOptions),
    InvDistToAPower(GDALInverseDistanceToAPowerOptions)
}

impl GdalGridAlgorithmOptions {
    pub fn nearest_neighbor (radius1: f64, radius2: f64, angle: f64, nodata_value: f64)->Self {
        Self::NearestNeighbor( GDALGridNearestNeighborOptions::new( radius1, radius2, angle, nodata_value))
    }

    pub fn nearest_neighbor_within (radius: f64, nodata_value: f64)->Self {
        Self::NearestNeighbor( GDALGridNearestNeighborOptions::new( radius, radius, 0.0, nodata_value))
    }

    pub fn linear (radius: f64, nodata_value: f64)->Self {
        Self::Linear( GDALGridLinearOptions::new( radius, nodata_value))
    }

    /// this uses an infinite search radius, i.e. it does not produce nodata values
    pub fn linear_inf ()-> Self {
        Self::Linear( GDALGridLinearOptions::new( -1.0, -9999.0))
    }

    pub fn inv_dist (power: f64, smooth: f64, radius: f64, nodata_value: f64)->  Self {
        Self::InvDistToAPower( GDALInverseDistanceToAPowerOptions::new(
            power,
            smooth,
            radius, radius, 0.0,
            0,0,
            nodata_value
        ))
    }

    pub fn inv_dist_default (nodata_value: f64)->  Self {
        Self::InvDistToAPower( GDALInverseDistanceToAPowerOptions::new(
            2.0,
            0.0,
            0.0, 0.0, 0.0,
            0,0,
            nodata_value
        ))
    }

    unsafe fn as_ptr (&self) -> *const c_void {
        match self {
            Self::NearestNeighbor(o) => o as *const GDALGridNearestNeighborOptions as *const c_void,
            Self::Linear(o) => o as *const GDALGridLinearOptions as *const c_void,
            Self::InvDistToAPower(o) => o as *const GDALInverseDistanceToAPowerOptions as *const c_void
        }
    }

    fn algorithm_type (&self) -> c_uint {
        match self {
            Self::NearestNeighbor(o) => gdal_sys::GDALGridAlgorithm::GGA_NearestNeighbor,
            Self::Linear(o) => gdal_sys::GDALGridAlgorithm::GGA_Linear,
            Self::InvDistToAPower(o) => gdal_sys::GDALGridAlgorithm::GGA_InverseDistanceToAPower,
        }
    }

    fn nodata_value_f64 (&self) -> f64 {
        match self {
            Self::NearestNeighbor(o) => o.nodata_value,
            Self::Linear(o) => o.nodata_value,
            Self::InvDistToAPower(o) => o.nodata_value
        }
    }

    fn nodata_value_i64 (&self) -> i64 {
        match self {
            Self::NearestNeighbor(o) => o.nodata_value as i64,
            Self::Linear(o) => o.nodata_value as i64,
            Self::InvDistToAPower(o) => o.nodata_value as i64
        }
    }

    fn nodata_value_u64 (&self) -> Option<u64> {
        let x = self.nodata_value_f64();
        if x < 0.0 { None } else { Some(x as u64) }
    }
}

// gdal_sys does not have the GDAL GGA_* GDALGridAlgorithm option structs
// we add a bunch here

#[repr(C)]
pub struct GDALGridNearestNeighborOptions {
    n_size_of_structure: usize,
    radius1: f64,
    radius2: f64,
    angle: f64,
    nodata_value: f64,
}

impl GDALGridNearestNeighborOptions {
    pub fn new (radius1: f64, radius2: f64, angle: f64, nodata_value: f64)->Self {
        GDALGridNearestNeighborOptions{ n_size_of_structure: size_of::<Self>(), radius1, radius2, angle, nodata_value }
    }
}

#[repr(C)]
pub struct GDALGridLinearOptions {
    n_size_of_structure: usize,
    radius: f64,
    nodata_value: f64,
}

impl GDALGridLinearOptions {
    pub fn new (radius: f64, nodata_value: f64)->Self {
        GDALGridLinearOptions{ n_size_of_structure: size_of::<Self>(), radius, nodata_value }
    }
}

#[repr(C)]
pub struct GDALInverseDistanceToAPowerOptions {
    n_size_of_structure: usize,
    power: f64,
    smoothing: f64,
    anisotropy_ratio: f64, // for future use
    anisotropy_angle: f64, // for future use
    radius1: f64,
    radius2: f64,
    angle: f64,
    max_points: u32,
    min_points: u32,
    nodata_value: f64,
}

impl GDALInverseDistanceToAPowerOptions {
    pub fn new (
        power: f64,
        smoothing: f64,
        radius1: f64,
        radius2: f64,
        angle: f64,
        max_points: u32,
        min_points: u32,
        nodata_value: f64
    )->Self {
        GDALInverseDistanceToAPowerOptions {
            n_size_of_structure: size_of::<Self>(),
            power, smoothing,
            anisotropy_ratio: 0.0,
            anisotropy_angle: 0.0,
            radius1, radius2, angle,
            max_points, min_points,
            nodata_value
        }
    }
}

/* #endregion GDALGridAlgorithm structs */

pub fn create_grid_ds_from_raster<T:GdalType + NumericType, P: AsRef<Path>> (
    driver: &Driver, path: P,
    epsg: u32,
    x_min: f64, y_max: f64, // upper left corner
    x_size: usize, y_size: usize, // dimensions in pixels
    x_res: f64, y_res: f64, // pixel resolutions
    raster_values: Vec<Vec<T>>,
    create_opts: &RasterCreationOptions
) ->Result<Dataset> {
    let n_bands = raster_values.len();
    let mut ds = driver.create_with_band_type_with_options::<T, P>( path, x_size, y_size, n_bands, create_opts)?;
    ds.set_geo_transform( &[ x_min, x_res, 0.0, y_max, 0.0, -y_res])?;

    let srs = SpatialRef::from_epsg( epsg)?;
    ds.set_spatial_ref(&srs)?;

    for (i,v) in raster_values.into_iter().enumerate() {
        let mut buf: Buffer<T> = Buffer::new( (x_size,y_size), v);
        let mut band = ds.rasterband(i+1)?;
        band.write( (0,0), (x_size, y_size), &mut buf)?;
    }

    ds.flush_cache();

    Ok(ds)
}

pub fn create_grid_ds_from_array2<T:GdalType + NumericType, P: AsRef<Path>> (
    driver: &Driver, path: P,
    epsg: u32,
    x_min: f64, y_max: f64, // upper left corner
    x_size: usize, y_size: usize, // dimensions in pixels
    x_res: f64, y_res: f64, // pixel resolutions
    raster_values: Vec<Array2<T>>,
    create_opts: &RasterCreationOptions
) ->Result<Dataset> {
    let rvs: Vec<Vec<T>> = raster_values.into_iter().map( |arr2| arr2.into_raw_vec_and_offset().0).collect();
    create_grid_ds_from_raster( driver, path, epsg, x_min, y_max, x_size, y_size, x_res, y_res, rvs, create_opts)
}

/// create a dataset that is initialized by computing pixel values from given point/value sets. All sets use the
/// same x_coord,y_coord positions but each set can have different values for these positions
/// the number of bands in the dataset if given by the number of value sets.
/// the number of xy_values for each band has to match the length of the x_coord,y_coord slices
pub fn create_grid_ds_from_scatter<T:GdalType + NumericType, P: AsRef<Path>> (
    driver: &Driver, path: P,
    epsg: u32,
    x_min: f64, x_max: f64,
    y_min: f64, y_max: f64, // the grid definition
    x_size: usize, y_size: usize,
    x_coords: &[f64],
    y_coords: &[f64], // the non-gridded data point coordinates
    xy_values: &[Vec<f64>], // slice of band value array
    alg: &GdalGridAlgorithmOptions,
    create_opts: &RasterCreationOptions
)->Result<Dataset> {
    unsafe {
        let pixel_width = (x_max - x_min) / x_size as f64;
        let pixel_height = (y_max - y_min) / y_size as f64;

        if x_coords.len() != y_coords.len() {
            return Err( misc_error("x and y coordinates do not have same length".into()))
        }
        for v in xy_values.iter() {
            if v.len() != x_coords.len() { return Err( misc_error("number of values inconsistent with number of x,y coordinates".into())) }
        }

        let mut ds = driver.create_with_band_type_with_options::<T, P>( path, x_size, y_size, xy_values.len(), create_opts)?;
        ds.set_geo_transform( &[ x_min, pixel_width, 0.0, y_max, 0.0, -pixel_height])?;

        let srs = SpatialRef::from_epsg( epsg)?;
        ds.set_spatial_ref(&srs)?;

        let mut band_no = 1;
        for i in 0..xy_values.len() {
            let mut grid_data:Vec<T> = Vec::with_capacity(x_size*y_size);

            let d = &xy_values[i];
            let result = gdal_sys::GDALGridCreate(
                alg.algorithm_type(),
                alg.as_ptr(),
                x_coords.len() as c_uint,
                x_coords.as_ptr() as *const c_double,
                y_coords.as_ptr() as *const c_double,
                d.as_ptr() as *const c_double,
                x_min, x_max, y_min, y_max,
                x_size as c_uint, y_size as c_uint,
                T::gdal_ordinal(),
                grid_data.as_mut_ptr() as *mut c_void,
                None,
                ptr::null_mut(),
            );

            if result != gdal_sys::CPLErr::CE_None {
                return Err(last_gdal_error());
            }
            grid_data.set_len( x_size*y_size);

            let mut band = ds.rasterband(band_no)?;
            let mut buf = Buffer::new((x_size,y_size), grid_data);
            band.write::<T>( (0, 0), (x_size, y_size), &mut buf)?;

            if T::is_float() {
                band.set_no_data_value( Some(alg.nodata_value_f64()));
            } else {
                if T::is_signed() {
                    band.set_no_data_value_i64( Some(alg.nodata_value_i64()));
                } else {
                    band.set_no_data_value_u64( alg.nodata_value_u64());
                }
            }

            band_no += 1;
        }

        ds.flush_cache();
        Ok(ds)
    }
}

/// create a ds with bands that are initialized with the given const values
pub fn create_const_grid_ds<T:GdalType + NumericType, P: AsRef<Path>> (
    driver: &Driver, path: P,
    epsg: u32,
    x_min: f64, x_max: f64,
    y_min: f64, y_max: f64, // the grid definition
    x_size: usize, y_size: usize,
    init_vals: &[f64], // slice of const init band values
    no_data: f64,
    create_opts: &RasterCreationOptions
)->Result<Dataset> {
    let pixel_width = (x_max - x_min) / x_size as f64;
    let pixel_height = (y_max - y_min) / y_size as f64;
    let n_bands = init_vals.len();

    let mut ds = driver.create_with_band_type_with_options::<T, P>( path, x_size, y_size, n_bands, create_opts)?;
    ds.set_geo_transform( &[ x_min, pixel_width, 0.0, y_max, 0.0, -pixel_height])?;

    let srs = SpatialRef::from_epsg( epsg)?;
    ds.set_spatial_ref(&srs)?;

    for i in 0..n_bands {
        let mut band = ds.rasterband( i+1)?;
        band.fill( init_vals[i], Some(no_data))?;
    }

    Ok(ds)
}


pub fn resample_ds<T,P> (driver: &Driver, path: P, src_ds: &Dataset, x_scale: f64, y_scale: f64, create_opts: Option<RasterCreationOptions>)->Result<Dataset>
    where T:GdalType + NumericType, P: AsRef<Path>
{
    let (sw,sh) = src_ds.raster_size();
    let dw = (sw as f64 * x_scale) as usize;
    let dh = (sh as f64 * y_scale) as usize;
    let n_bands = src_ds.raster_count();

    let data_type = src_ds.rasterband(1)?.band_type(); // src and dst need to be homogenous
    let mut ds = create_dataset(driver, path, dw, dh, n_bands, data_type, create_opts)?;

    let mut geo_transform = src_ds.geo_transform()?;
    geo_transform[1] /= x_scale;
    geo_transform[5] /= y_scale;
    ds.set_geo_transform(&geo_transform)?;

    ds.set_projection(src_ds.projection().as_str())?;

    for i in 0..n_bands {
        let src_band = src_ds.rasterband(i+1)?;
        let mut buf: Buffer<T> = src_band.read_as( (0,0), (sw,sh), (dw,dh), Some(gdal::raster::ResampleAlg::Bilinear))?;

        let mut dst_band = ds.rasterband(i+1)?;
        dst_band.write( (0,0), (dw,dh), &mut buf)?;
    }

    ds.flush_cache();

    Ok(ds)
}
