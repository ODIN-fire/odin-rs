/*
 * Copyright (c) 2024, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The ODIN - Open Data Integration Framework is licensed under the
 * Apache License, Version 2.0 (the "License"); you may not use this file
 * except in compliance with the License. You may obtain a copy of the
 * License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */
#![allow(unused)]
#![feature(trait_alias)]

pub mod errors;
pub mod warp;
pub mod contour;

use lazy_static::lazy_static;
use std::collections::HashMap;
use std::ptr::{null, null_mut};
use std::ffi::{CString,CStr};
use std::ops::{Sub,Index,Fn};
use std::sync::Mutex;
use std::path::Path;
use libc::{c_void,c_char,c_uint, c_int};

// we re-export these so that other crates don't have to use a direct gdal depedency to import.
// this is to ensure we run bindgen for new GDAL versions that don't yet have pre-computed bindings in gdal-sys
pub use gdal::{self, Driver, DriverManager, Metadata, MetadataEntry, Dataset, errors::GdalError, GeoTransform};
pub use gdal::raster::{GdalType,RasterBand,Buffer};
pub use gdal::spatial_ref::{CoordTransform, CoordTransformOptions, SpatialRef};

use gdal_sys::{self,CPLErrorReset, OGRErr, OSRExportToWkt, OSRNewSpatialReference, OSRSetFromUserInput, CPLErr};
use geo::{Coord, Rect};
use gdal::cpl::CslStringList;

use odin_common::{fs::*,geo::*,ranges::LinearRange};
use odin_common::macros::if_let;
use crate::errors::{Result,misc_error, last_gdal_error, OdinGdalError, gdal_error, map_gdal_error};

lazy_static! {
    // note that we can't automatically populate this by iterating over DriverManager since some
    // drivers use the same file extension
    static ref EXT_MAP: HashMap<&'static str, &'static str> = HashMap::from( [ // file extension -> driver short name
        //-- well known raster drivers
        ("tif", "GTiff"),
        ("png", "PNG"),
        ("nc", "netCDF"),
        ("grib2", "GRIB"),

        //--- vector drivers
        ("json", "GeoJSON"),
        ("geojson", "GeoJSON"),
        ("ndjson", "GeoJSONSeq"),
        ("csv", "CSV"),
        ("gpx", "GPX"),
        ("kml", "KML"),
        ("svg", "SVG"),
        ("pdf", "PDF"),
        ("shp", "ESRI Shapefile"),

        //... and many more to follow (see http://gdal.org/drivers
    ]);
}

/// use this to protect non-threadsafe GDAL operations
static GLOB_GDAL_MUTEX: Mutex<usize> = Mutex::new(0);

pub fn initialize_gdal() -> bool {
    EXT_MAP.len() > 0
}

pub fn get_driver_name_from_filename (filename: &str) -> Option<&'static str> {
    get_filename_extension(filename).and_then( |ext| EXT_MAP.get( ext.to_lowercase().as_str()).map(|v| *v))
}

pub fn get_driver_from_filename (filename: &str) -> Option<gdal::Driver> {
    get_filename_extension(filename)
        .and_then( |ext| EXT_MAP.get( ext.to_lowercase().as_str()))
        .and_then( |n| DriverManager::get_driver_by_name(n).ok())
}

pub fn pc_char_to_string (pc_char: *const c_char) -> String {
    let cstr = unsafe { CStr::from_ptr(pc_char) };
    String::from_utf8_lossy(cstr.to_bytes()).to_string()
}

pub fn ok_true <F> (cond: bool, err: F) -> Result<()> where F: FnOnce()->String {
    if cond { Ok(()) } else { Err( OdinGdalError::MiscError(err())) }
}

pub fn ok_not_zero <F> (res: c_int, err: F) -> Result<()> where F: FnOnce()->String {
    if res != 0 { Ok(()) } else {  Err( OdinGdalError::MiscError(err())) }
}

pub fn ok_non_null <R,F> (ptr: *const R, err: F) -> Result<*const R>  where F: FnOnce()->String {
    if ptr != null() { return Ok(ptr) } else {  Err(OdinGdalError::MiscError(err())) }
}

pub fn ok_mut_non_null <R,F> (ptr: *mut R, err: F) -> Result<*mut R>  where F: FnOnce()->String {
    if ptr != null_mut() { return Ok(ptr) }  else {  Err(OdinGdalError::MiscError(err())) }
}

pub fn ok_ce_none (res: CPLErr::Type) -> Result<()> {
    if res == CPLErr::CE_None { return Ok(()) } else { Err(last_gdal_error()) }
}

pub fn gdal_badarg(details: String) -> GdalError {
    GdalError::BadArgument(details)
}

/// run the provided closure with the global GDAL error handler disabled. Note this does not
/// change the return value but prevents GDAL from printing errors and warnings to the console
pub fn run_quiet<T,F> (f: F)->Result<T> where F: Fn()->Result<T> {
    let lock = GLOB_GDAL_MUTEX.lock().unwrap();
    unsafe { gdal_sys::CPLPushErrorHandler( Some(gdal_sys::CPLQuietErrorHandler)); }
    let result = f();
    unsafe { gdal_sys::CPLPopErrorHandler(); }
    result
}

// some NetCDF files (e.g. GoesR data sets) cause error messages printed to the console if
// the SRS does not conform to CF-1. If the dataset still works correctly (e.g. because
// we explicitly do coordinate transformation) use this function to open the Dataset without
// annoying console output
pub fn quiet_nc_dataset( nc_path: impl AsRef<Path>, var_name: &str) -> Result<Dataset> {
    let path = format!("NETCDF:{:?}:{:?}", nc_path.as_ref(), var_name);

    // work around the "Unhandled X/Y axis unit rad. SRS will ignore axis unit and be likely wrong" warning
    /* does not work
    let dso = DatasetOptions {
        open_flags: GdalOpenFlags::GDAL_OF_READONLY,
        allowed_drivers: None,
        open_options: Some( &["IGNORE_XY_AXIS_NAME_CHECKS=YES"] ),
        sibling_files: None
    };
    Ok( Dataset::open_ex(path, dso)? )
    */

    run_quiet( move || Ok( Dataset::open(&path)? ) )
}

pub fn nc_dataset( nc_path: impl AsRef<Path>, var_name: &str) -> Result<Dataset> {
    let path = format!("NETCDF:{:?}:{:?}", nc_path.as_ref(), var_name);
    Ok( Dataset::open(&path)? )
}

pub fn to_csl_string_list (strings: &Vec<String>) -> Result<Option<CslStringList>> {
    if ! strings.is_empty() { // don't allocate if there is nothing to convert
        let mut co_list =  CslStringList::new();
        for s in strings {
            co_list.add_string(s.as_str())?;
        }
        Ok(Some(co_list))
    } else {
        Ok(None)
    }
}

#[macro_export]
macro_rules! gdal_badarg {
    ($msg: literal) => {
        gdal_badarg(format!($msg))
    };

    ($fmt_str: literal , $($arg:expr),+) => {
        gdal_badarg(format!($fmt_str, $($arg),+))
    }
}

pub fn new_geotransform (x_upper_left: f64, x_resolution: f64, row_rotation: f64,
                         y_upper_left: f64, col_rotation: f64, y_resolution: f64) -> GeoTransform {
    [x_upper_left,x_resolution,row_rotation,y_upper_left,col_rotation,y_resolution]
}

pub fn geotransform_from_bbox (bbox: Rect<f64>, x_resolution: f64, y_resolution: f64) -> GeoTransform {
    new_geotransform(bbox.min().x, x_resolution,0.0,
                     bbox.max().y, 0.0, y_resolution)
}

//--- SpatialRef based coordinate transformations

pub fn bounds_center (x_min: f64, y_min: f64, x_max: f64, y_max: f64) -> (f64,f64) {
    let x_center = (x_min + x_max) / 2.0;
    let y_center = (y_min + y_max) / 2.0;
    (x_center, y_center)
}

pub fn transform_point_2d (transform: &CoordTransform, x: f64, y: f64) -> Result<(f64,f64)> {
    let mut ax: [f64;1] = [x];
    let mut ay: [f64;1] = [y];
    let mut az: [f64;0] = [];

    transform.transform_coords(&mut ax, &mut ay, &mut az)?;
    Ok((ax[0],ay[0]))
}

pub fn latlon_to_utm_bounds (bbox: &BoundingBox<f64>, interior:  bool) -> (BoundingBox<f64>,UtmZone) {
    let ll_geo = LatLon {lat_deg: bbox.south, lon_deg: bbox.west };
    let lr_geo = LatLon {lat_deg: bbox.south, lon_deg: bbox.east };
    let ul_geo = LatLon {lat_deg: bbox.north, lon_deg: bbox.west };
    let ur_geo = LatLon {lat_deg: bbox.north, lon_deg: bbox.west };

    let center_geo = LatLon {lat_deg: (ll_geo.lat_deg + ul_geo.lat_deg) / 2.0,
                             lon_deg: (ll_geo.lon_deg + lr_geo.lon_deg) / 2.0 };
    let zone = naive_utm_zone( &center_geo);

    let ll_utm = latlon_to_utm_zone(&ll_geo, zone).unwrap();
    let ul_utm = latlon_to_utm_zone(&ul_geo, zone).unwrap();
    let lr_utm = latlon_to_utm_zone(&lr_geo, zone).unwrap();
    let ur_utm = latlon_to_utm_zone(&ur_geo, zone).unwrap();

    let (west, east) = if interior {
        ( ll_utm.easting.max( ul_utm.easting), lr_utm.easting.min( ur_utm.easting) )
    } else {
        ( ll_utm.easting.min( ul_utm.easting), lr_utm.easting.max( ur_utm.easting) )
    };
    (BoundingBox {west, south: ll_utm.northing, east, north: ul_utm.northing}, zone)
}

pub fn transform_latlon_to_utm_bounds (west_deg: f64, south_deg: f64, east_deg: f64, north_deg: f64,
                                       interior:  bool, utm_zone: Option<u32>, is_south: bool) -> Result<(f64,f64,f64,f64,u32)> {
    let s_srs = srs_epsg_4326(); // axis order is lat,lon, uom: degrees

    let (t_srs,zone) = if let Some(zone) = utm_zone {
        let zone_base = if is_south { 32700 } else { 32600 };
        (srs_epsg( zone_base + zone)?, zone)
    } else {
        let (lon_center,lat_center) = bounds_center(west_deg,south_deg,east_deg,north_deg);
        srs_utm_from_lon_lat(lon_center, lat_center, utm_zone)?
    };

    //let transform = CoordTransform::new(&s_srs, &t_srs)?;
    let mut ct_options = CoordTransformOptions::new()?;
    ct_options.desired_accuracy( 0.0);
    ct_options.set_ballpark_allowed(false);
    let transform = CoordTransform::new_with_options(&s_srs, &t_srs, &ct_options)?;

    let (x_ll,y_ll) = transform_point_2d( &transform, south_deg, west_deg)?;
    let (x_lr,y_lr) = transform_point_2d( &transform, south_deg, east_deg)?;
    let (x_ul,y_ul) = transform_point_2d( &transform, north_deg, west_deg)?;
    let (x_ur,y_ur) = transform_point_2d( &transform, north_deg, east_deg)?;

    if interior {
        Ok( (x_ll.max(x_ul),  y_ll.max(y_lr), x_lr.min(x_ur), y_ul.min(y_ur), zone) )
    } else {
        Ok( (x_ll.min(x_ul),  y_ll.min(y_lr), x_lr.max(x_ur), y_ul.max(y_ur), zone) )
    }
}

pub fn transform_utm_to_latlon_bounds (west_m: f64, south_m: f64, east_m: f64, north_m: f64, interior: bool, utm_zone: u32, is_south: bool) -> Result<(f64,f64,f64,f64)> {
    let zone_base = if is_south { 32700 } else { 32600 };
    let s_srs = srs_epsg( zone_base + utm_zone)?;
    let t_srs = srs_epsg_4326();

    let mut ct_options = CoordTransformOptions::new()?;
    ct_options.desired_accuracy( 0.0);
    ct_options.set_ballpark_allowed(false);
    let transform = CoordTransform::new_with_options(&s_srs, &t_srs, &ct_options)?;

    let (y_ll,x_ll) = transform_point_2d( &transform, west_m, south_m)?;
    let (y_lr,x_lr) = transform_point_2d( &transform, east_m, south_m)?;
    let (y_ul,x_ul) = transform_point_2d( &transform, west_m, north_m)?;
    let (y_ur,x_ur) = transform_point_2d( &transform, east_m, north_m)?;

    if interior {
        Ok( (x_ll.max(x_ul),  y_ll.max(y_lr), x_lr.min(x_ur), y_ul.min(y_ur)) )
    } else {
        Ok( (x_ll.min(x_ul),  y_ll.min(y_lr), x_lr.max(x_ur), y_ul.max(y_ur)) )
    }
}

// watch out - if source or target are geographic we might have to swap axis order
// (we don't want to change axis_mapping_strategy in the provided SpatialRefs though)
// TODO - round trips between epsg:4326 and UTM produce differing results also in lat/northing, find out why
pub fn transform_bounds_2d (s_srs: &SpatialRef, t_srs: &SpatialRef,
                            x_min: f64, y_min: f64,
                            x_max: f64, y_max: f64,
                            opt_densify_pts: Option<i32>) -> Result<(f64,f64,f64,f64)> {

    let s_is_geo = s_srs.is_geographic();
    let t_is_geo = t_srs.is_geographic();

    let mut bounds: [f64;4] = if s_is_geo && !t_is_geo { [y_min,x_min,y_max,x_max] } else { [x_min,y_min,x_max,y_max] };
    let densify_pts: i32 = if let Some(dp) = opt_densify_pts { dp } else { 21 }; // default recommended by GDAL OCTTransformBounds doc

    let mut ct_options = CoordTransformOptions::new()?;
    ct_options.desired_accuracy( 0.0);
    ct_options.set_ballpark_allowed(false);

    //CoordTransform::new(s_srs,t_srs)
    CoordTransform::new_with_options(s_srs,t_srs, &ct_options)
        .and_then( |transform| transform.transform_bounds(&mut bounds, densify_pts))
        .map_err(|e| {
            gdal_error(e)})
        .and_then( |a| {
            let ta = if t_is_geo && !s_is_geo { (a[1], a[0], a[3], a[2]) } else { (a[0], a[1], a[2], a[3]) };
            Ok(ta)
        })
}

/* #region well known SpatialRefs *********************************************************************************/

pub fn srs_lon_lat () -> SpatialRef { SpatialRef::from_epsg(4326).unwrap() }
pub fn srs_epsg_4326 () -> SpatialRef { SpatialRef::from_epsg(4326).unwrap() }

pub fn srs_utm_10_n() -> SpatialRef { SpatialRef::from_epsg(32610).unwrap() } // US Pacific coast (north west  CA)
pub fn srs_utm_11_n() -> SpatialRef { SpatialRef::from_epsg(32611).unwrap() } // south/east CA, east WA, east OR, west ID, west MT, west AZ, NV
pub fn srs_utm_12_n() -> SpatialRef { SpatialRef::from_epsg(32612).unwrap() } // UT, AZ, east ID, central MT, west WY, west CO, west NM
pub fn srs_utm_13_n() -> SpatialRef { SpatialRef::from_epsg(32613).unwrap() } // east MT, east WY, CO, NM, west ND, west SD

pub fn srs_epsg (utm_zone: u32) -> Result<SpatialRef> {
    Ok(SpatialRef::from_epsg(utm_zone)?)
}

pub fn srs_utm_n (zone: u32) -> Result<SpatialRef> {
    Ok(SpatialRef::from_epsg(32600 + zone)?)
}

pub fn srs_utm_s (zone: u32) -> Result<SpatialRef> {
    Ok(SpatialRef::from_epsg(32700 + zone)?)
}

pub fn srs_utm_from_lon_lat (lon_deg: f64, lat_deg: f64, opt_zone: Option<u32>) -> Result<(SpatialRef,u32)> {
    let utm_zone = if let Some(zone) = opt_zone {
        if zone <= 60 { zone } else {
            return Err(misc_error(format!("invalide UTM zone: {}", zone)));
        }
    } else {
		let lat_lon = LatLon { lat_deg, lon_deg };
        utm_zone( &lat_lon)
    };

    let epsg_base = if lat_deg < 0.0 { 32700 } else { 32600 };
    Ok(SpatialRef::from_epsg(epsg_base + utm_zone).map( |srs| (srs,utm_zone))?)
}

/* #endregion well known SpatialRefs */

/* #region generic Dataset/Rasterband access *********************************************************************************/

pub trait GdalValueType = std::fmt::Debug + std::fmt::Display + Copy + From<u8> + GdalType;

/// aggregate of indices and corresponding value of a 2D grid point
/// note this makes no assumption about axis order, it just uses whatever is in the dataset
#[derive(Debug)]
pub struct GridPoint<T> where T: GdalValueType {
    pub i0: usize,
    pub i1: usize,
    pub value: T
}

impl <T> GridPoint<T> where T: GdalValueType {
    #[inline]
    pub fn position(&self)->(isize,isize) { (self.i0 as isize, self.i1 as isize) }

    #[inline]
    pub fn transposed_position(&self)->(isize,isize) { (self.i1 as isize, self.i0 as isize) }
}


/// get vec of GridPoint2D elements that match the provided predicate
/// note that the RasterBand::read_* functions swap axis order
pub fn find_grid_points<T,P> (ds: &Dataset, band_index: isize, predicate: P)->Result<Vec<GridPoint<T>>> 
    where T: GdalValueType, P: Fn(T)->bool 
{
    let band = ds.rasterband(band_index)?;
    let x_size = band.x_size();
    let y_size = band.y_size();
    let mut scan_line: Vec<T> = Vec::with_capacity( x_size);
    scan_line.resize( x_size, 0.into());

    // unfortunately we don't know the value upfront but with large grids a 2-pass solution would be probably more expensive
    let mut result: Vec<GridPoint<T>> = Vec::new();

    for i1 in 0..y_size {
        band.read_into_slice( (0, i1 as isize), (x_size,1), (x_size,1), &mut scan_line, None)?;
        for i0 in 0..x_size {
            let value = scan_line[i0];
            if predicate(value) {
                result.push( GridPoint{i0,i1,value})
            }
        }
    }
    Ok(result)
}

// a version that uses a caller provided closure to iterate over a whole data row, thus allowing optimizations
// such as inlining or simd to speed up
pub fn find_grid_points_in_slice<T,F> (ds: &Dataset, band_index: isize, accumulator: F)->Result<Vec<GridPoint<T>>> 
    where T: GdalValueType, F: Fn(usize,&[T],&mut Vec<GridPoint<T>>)
{
    let band = ds.rasterband(band_index)?;
    let x_size = band.x_size();
    let y_size = band.y_size();
    let mut scan_line: Vec<T> = Vec::with_capacity( x_size);
    scan_line.resize( x_size, 0.into());
    let mut result: Vec<GridPoint<T>> = Vec::new();

    for i1 in 0..y_size {
        band.read_into_slice( (0, i1 as isize), (x_size,1), (x_size,1), &mut scan_line, None)?;
        accumulator( i1, &scan_line, &mut result);
    }
    Ok(result)
}


/// get Vec of values for given Vec<GridPoint2D> reference
pub fn get_grid_point_values<T,U> (ds: &Dataset, band_index: isize, sub_no_data: Option<T>, pts: &Vec<GridPoint<U>> )->Result<Vec<T>> 
    where T: GdalValueType + Into<f64>, U: GdalValueType
{
    let band = ds.rasterband(band_index)?;
    let mut result: Vec<T> = Vec::with_capacity(pts.len());
    let mut data = [T::from(0u8);1];
    let no_data = band.no_data_value();

    if no_data.is_some() && sub_no_data.is_some() {
        let no_data = no_data.unwrap();
        let sub_no_data = sub_no_data.unwrap();

        for p in pts {
            band.read_into_slice( p.position(), (1,1), (1,1), &mut data, None)?;
            if data[0].into() == no_data { data[0] = sub_no_data } 
            result.push( data[0]);
        }
    } else {
        for p in pts {
            band.read_into_slice( p.position(), (1,1), (1,1), &mut data, None)?;
            result.push( data[0]);
        }
    }

    Ok(result)
}


pub fn get_vec_f64<T> (ds: &Dataset, band_index: isize)->Result<Vec<f64>> 
    where T: GdalValueType + Into<f64>
{
    let band = ds.rasterband(band_index)?;
    let scale = if let Some(v) = band.scale() { v } else { 1.0 };
    let offset = if let Some(v) = band.offset() { v } else { 0.0 };
    let buf: Buffer<T> = band.read_as( (0,0), band.size(), band.size(), None)?;
    let data = buf.data;
    let len = data.len();
    let mut values: Vec<f64> = Vec::with_capacity(len);
    values.resize( len, 0.0);

    for i in 0..len { 
        values[i] = (data[i].into()) * scale + offset;
    }

    Ok(values)
}


pub fn get_linear_range<T> (ds: &Dataset, band_index: isize)->Result<LinearRange<f64>>
    where T: GdalValueType + Into<f64> + Sub<Output=T>
{
    let band = ds.rasterband(band_index)?;
    let n = band.x_size();
    let scale = if let Some(v) = band.scale() { v } else { 1.0 };
    let offset = if let Some(v) = band.offset() { v } else { 0.0 };
    let mut data = [T::from(0u8);1]; 

    // this is slightly more expensive because of two reads but base this on the whole range to minimize truncation errors

    band.read_into_slice( (0isize,0isize), (1,1), (1,1), &mut data, None)?;
    let first = data[0].into() * scale + offset;

    band.read_into_slice( ((n-1) as isize, 0isize), (1,1), (1,1), &mut data, None)?;
    let last = data[0].into() * scale + offset;

    let inc = (last - first) / (n as f64); 

    Ok( LinearRange::new( first, inc, n) )
}

/* #endregion generic Dataset/Rasterband access */
