
use std::{mem::size_of, path::Path, ptr, fmt::Debug};
use std::os::raw::{c_void, c_uint, c_double};

use gdal::{DriverManager, Driver, raster::{GdalType,Buffer}, Dataset, spatial_ref::SpatialRef};
use gdal_sys;

use odin_common::num_type::NumericType;
use crate::errors::{Result, last_gdal_error, misc_error};

/* #region GDALGridAlgorithm structs *************************************************************************/

/// see https://gdal.org/en/stable/tutorials/gdal_grid_tut.html
pub enum GdalGridAlgorithmOptions {
    NearestNeighbor(GDALGridNearestNeighborOptions),
    Linear(GDALGridLinearOptions)
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

    unsafe fn as_ptr (&self) -> *const c_void {
        match self {
            Self::NearestNeighbor(o) => o as *const GDALGridNearestNeighborOptions as *const c_void,
            Self::Linear(o) => o as *const GDALGridLinearOptions as *const c_void,
        }
    }

    fn algorithm_type (&self) -> c_uint {
        match self {
            Self::NearestNeighbor(o) => gdal_sys::GDALGridAlgorithm::GGA_NearestNeighbor,
            Self::Linear(o) => gdal_sys::GDALGridAlgorithm::GGA_Linear,
        }
    }

    fn nodata_value_f64 (&self) -> f64 {
        match self {
            Self::NearestNeighbor(o) => o.nodata_value,
            Self::Linear(o) => o.nodata_value
        }
    }

    fn nodata_value_i64 (&self) -> i64 {
        match self {
            Self::NearestNeighbor(o) => o.nodata_value as i64,
            Self::Linear(o) => o.nodata_value as i64
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

/* #endregion GDALGridAlgorithm structs */

pub fn create_grid_ds<T:GdalType + NumericType, P: AsRef<Path>> (
    driver: &Driver, path: P,
    epsg: u32,
    x_min: f64, x_max: f64, y_min: f64, y_max: f64, // the grid definition
    x_size: usize, y_size: usize,
    x_coords: &[f64], y_coords: &[f64], // the non-gridded data point coordinates
    data: &[Vec<f64>],
    alg: &GdalGridAlgorithmOptions
)->Result<Dataset> {
    unsafe {
        let pixel_width = (x_max - x_min) / x_size as f64;
        let pixel_height = (y_max - y_min) / y_size as f64;

        if x_coords.len() != y_coords.len() {
            return Err( misc_error("x and y coordinates do not have same length".into()))
        }

        let mut ds = driver.create_with_band_type::<T, P>( path, x_size, y_size, data.len())?;
        ds.set_geo_transform( &[ x_min, pixel_width, 0.0, y_max, 0.0, -pixel_height])?;

        let srs = SpatialRef::from_epsg( epsg)?;
        ds.set_spatial_ref(&srs)?;

        let mut band_no = 1;
        for i in 0..data.len() {
            let mut grid_data:Vec<T> = Vec::with_capacity(x_size*y_size);

            let d = &data[i];
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
