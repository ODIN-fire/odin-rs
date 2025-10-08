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
#![allow(unused)]

pub mod errors;
pub mod warp;
pub mod contour;

use gdal::{errors::CplErrType, raster::RasterCreationOptions, DatasetOptions, GdalOpenFlags};
use lazy_static::lazy_static;
use static_init::{constructor};
use std::{collections::HashMap, ffi::{CStr, CString}, fs::File, ops::{Fn, Index, Sub}, path::Path, ptr::{null, null_mut}, sync::Mutex, usize};
use libc::{c_void,c_char,c_uint, c_int};
use trait_set::trait_set;
use clap::ValueEnum;

// we re-export these so that other crates don't have to use a direct gdal depedency to import.
// this is to ensure we run bindgen for new GDAL versions that don't yet have pre-computed bindings in gdal-sys
pub use gdal::{self, Driver, DriverManager, Metadata, MetadataEntry, Dataset, errors::GdalError, GeoTransform, GeoTransformEx, cpl::CslStringList};
pub use gdal::raster::{GdalType,GdalDataType,RasterBand,Buffer};
pub use gdal::spatial_ref::{CoordTransform, CoordTransformOptions, SpatialRef};

use gdal_sys::{self,CPLErrorReset, OGRErr, OSRExportToWkt, OSRNewSpatialReference, OSRSetFromUserInput, GDALFillNodata, CPLErr};
use geo::{Coord, Rect};

use odin_common::{
    BoundingBox,
    geo::{GeoRect,GeoPoint}, 
    utm::{UtmZone,naive_utm_zone, geo_to_utm_zone, utm_zone}, 
    fs::{existing_non_empty_file_from_path,get_filename_extension},
    macros::if_let,
    ranges::LinearRange
};
use crate::errors::{Result,misc_error, last_gdal_error, OdinGdalError, gdal_error, map_gdal_error};


lazy_static! {
    // note that we can't automatically populate this by iterating over DriverManager since some
    // drivers use the same file extension
    static ref EXT_MAP: HashMap<&'static str, &'static str> = HashMap::from( [ // file extension -> driver short name
        //-- well known raster drivers
        ("tif", "GTiff"),
        ("tiff", "GTiff"),

        ("png", "PNG"),
        ("webp", "WEBP"),
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

#[constructor(0)]
extern "C" fn _initialize_gdal() {
    //println!("setting GDAL error handler");
    gdal::config::set_error_handler(no_error_output);
}

fn no_error_output (cpl_et: CplErrType, ec: i32, msg: &str) {}

/// Note that filename extension has to be lower case
pub fn get_driver_name_from_filename (filename: &str) -> Option<&'static str> {
    get_filename_extension(filename).and_then( |ext| EXT_MAP.get( ext)).map(|v| &**v)
}

/// Note that filename extension has to be lowercase
pub fn get_driver_name_for_extension (ext: &str) -> Option<&'static str> {
    EXT_MAP.get( ext).map(|v| &**v)
}

/// Note that filename extension has to be lowercase
pub fn get_driver_from_filename (filename: &str) -> Option<gdal::Driver> {
    get_filename_extension(filename)
        .and_then( |ext| EXT_MAP.get( ext))
        .and_then( |n| DriverManager::get_driver_by_name(&**n).ok())
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

pub fn open_update<P:AsRef<Path>> (path: P)->Result<Dataset> {
    let dso = DatasetOptions {
        open_flags: GdalOpenFlags::GDAL_OF_UPDATE,
        allowed_drivers: None,
        open_options: None,
        sibling_files: None
    };
    Ok( Dataset::open_ex(path, dso)? )
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

pub fn geo_bbox_to_utm (bbox: &BoundingBox<f64>, interior:  bool) -> (BoundingBox<f64>,UtmZone) {
    let ll_geo = GeoPoint::from_lon_lat_degrees( bbox.west, bbox.south);
    let lr_geo = GeoPoint::from_lon_lat_degrees( bbox.east, bbox.south);
    let ul_geo = GeoPoint::from_lon_lat_degrees( bbox.west, bbox.north);
    let ur_geo = GeoPoint::from_lon_lat_degrees( bbox.east, bbox.north);

    let center_geo = GeoPoint::from_lon_lat( 
        (ll_geo.longitude() + lr_geo.longitude()) / 2.0, 
        (ll_geo.latitude() + ul_geo.latitude()) / 2.0
    );
    
    let zone = naive_utm_zone( &center_geo);

    let ll_utm = geo_to_utm_zone(&ll_geo, zone).unwrap();
    let ul_utm = geo_to_utm_zone(&ul_geo, zone).unwrap();
    let lr_utm = geo_to_utm_zone(&lr_geo, zone).unwrap();
    let ur_utm = geo_to_utm_zone(&ur_geo, zone).unwrap();

    let (west, east) = if interior {
        ( ll_utm.easting.max( ul_utm.easting), lr_utm.easting.min( ur_utm.easting) )
    } else {
        ( ll_utm.easting.min( ul_utm.easting), lr_utm.easting.max( ur_utm.easting) )
    };
    (BoundingBox {west, south: ll_utm.northing, east, north: ul_utm.northing}, zone)
}

pub fn transform_geo_to_utm_bounds (west_deg: f64, south_deg: f64, east_deg: f64, north_deg: f64,
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

pub fn transform_utm_to_geo_bounds (west_m: f64, south_m: f64, east_m: f64, north_m: f64, interior: bool, utm_zone: u32, is_south: bool) -> Result<(f64,f64,f64,f64)> {
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

pub fn srs_utm_from_lon_lat (lon_deg: f64, lat_deg: f64, opt_zone: Option<u32>) -> Result<(SpatialRef,u32)> { // TODO - use Longitude,Latitude
    let utm_zone = if let Some(zone) = opt_zone {
        if zone <= 60 { zone } else {
            return Err(misc_error(format!("invalide UTM zone: {}", zone)));
        }
    } else {
		let geo_point = GeoPoint::from_lon_lat_degrees(lon_deg, lat_deg);
        utm_zone( &geo_point)
    };

    let epsg_base = if lat_deg < 0.0 { 32700 } else { 32600 };
    Ok(SpatialRef::from_epsg(epsg_base + utm_zone).map( |srs| (srs,utm_zone))?)
}

/* #endregion well known SpatialRefs */

/* #region generic Dataset/Rasterband access *********************************************************************************/

trait_set! {
  pub trait GdalValueType = std::fmt::Debug + std::fmt::Display + Copy + From<u8> + GdalType;
}

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
pub fn find_grid_points<T,P> (ds: &Dataset, band_index: usize, predicate: P)->Result<Vec<GridPoint<T>>> 
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
pub fn find_grid_points_in_slice<T,F> (ds: &Dataset, band_index: usize, accumulator: F)->Result<Vec<GridPoint<T>>> 
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

pub fn get_values_for_positions (ds: &Dataset, band_index: usize, sub_no_data: Option<f64>, pts: &[(f64,f64)]) -> Result<Vec<f64>> {
    let band = ds.rasterband(band_index)?;
    let pixel_to_geo = ds.geo_transform()?;
    let geo_to_pixel = pixel_to_geo.invert()?;
    let mut data = [0.0f64;1];
    let no_data = band.no_data_value();
    let mut result: Vec<f64> = Vec::with_capacity(pts.len());
    
    if no_data.is_some() && sub_no_data.is_some() {
        let no_data = no_data.unwrap();
        let sub_no_data = sub_no_data.unwrap();

        for mut p in pts {
            let xy = geo_to_pixel.apply( p.0, p.1);
            let xy = (xy.0.round() as isize, xy.1.round() as isize);
            let v = if band.read_into_slice( xy, (1,1), (1,1), &mut data, None).is_ok() {data[0]} else {sub_no_data};
            result.push( if v == no_data {sub_no_data} else {v})
        }
    } else {
        let no_data = sub_no_data.unwrap_or( band.no_data_value().unwrap_or(0.0));
        
        for mut p in pts {
            let xy = geo_to_pixel.apply( p.0, p.1);
            let xy = (xy.0.round() as isize, xy.1.round() as isize);
            let v = if band.read_into_slice( xy, (1,1), (1,1), &mut data, None).is_ok() {data[0]} else {no_data};
            result.push( v)
        }
    }

    Ok(result)
}

/// get Vec of values for given `Vec<GridPoint2D>` reference
pub fn get_grid_point_values<T,U> (ds: &Dataset, band_index: usize, sub_no_data: T, pts: &Vec<GridPoint<U>> )->Result<Vec<T>> 
    where T: GdalValueType + Into<f64>, U: GdalValueType
{
    let band = ds.rasterband(band_index)?;
    let mut result: Vec<T> = Vec::with_capacity(pts.len());
    let mut data = [T::from(0u8);1];

    if let Some(no_data) = band.no_data_value() {
        for p in pts {
            let v = if band.read_into_slice( p.position(), (1,1), (1,1), &mut data, None).is_ok() {data[0]} else {sub_no_data};
            result.push( if v.into() == no_data {sub_no_data} else {v} );
        }
    } else { // no no_data value set for band
        for p in pts {
            let v = if band.read_into_slice( p.position(), (1,1), (1,1), &mut data, None).is_ok() {data[0]} else {sub_no_data};
            result.push( v);
        }
    }

    Ok(result)
}


pub fn get_vec_f64<T> (ds: &Dataset, band_index: usize)->Result<Vec<f64>> 
    where T: GdalValueType + Into<f64>
{
    let band = ds.rasterband(band_index)?;
    let scale = if let Some(v) = band.scale() { v } else { 1.0 };
    let offset = if let Some(v) = band.offset() { v } else { 0.0 };
    let buf: Buffer<T> = band.read_as( (0,0), band.size(), band.size(), None)?;
    let data = buf.data();
    let len = data.len();
    let mut values: Vec<f64> = Vec::with_capacity(len);
    values.resize( len, 0.0);

    for i in 0..len { 
        values[i] = (data[i].into()) * scale + offset;
    }

    Ok(values)
}


pub fn get_linear_range<T> (ds: &Dataset, band_index: usize)->Result<LinearRange<f64>>
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


/* #region misc high level functions *************************************************************************************************/

/// create an image with given width,height from VRT
pub fn create_wh_image_from_vrt (bbox: &BoundingBox<f64>, tgt_epsg: u32, width: u32, height: u32, img_extension: &str, opts: &Option<CslStringList>, vrt_path: &Path, file_path: &Path) -> Result<()> {
    let src_ds =  Dataset::open(vrt_path)?;
    let tgt_srs = SpatialRef::from_epsg( tgt_epsg)?;
    let driver_name = get_driver_name_for_extension( img_extension).ok_or(misc_error(format!("unsupported image type {}", img_extension)))?;

    let mut warp = warp::SimpleWarpBuilder::new( &src_ds, file_path)?;

    warp.set_tgt_srs( &tgt_srs);
    warp.set_tgt_extent_from_bbox( bbox);
    warp.set_tgt_size( width as i32, height as i32);
    warp.set_tgt_format( driver_name)?;

    if let Some(ref opts) = *opts {
        warp.set_create_options( opts);
    }
        
    warp.exec()?;
    Ok(())
}

/// create an image with given resolution from VRT (width,height are computed)
pub fn create_res_image_from_vrt (bbox: &BoundingBox<f64>, tgt_epsg: u32, res_x: f64, res_y: f64, img_extension: &str, opts: &Option<CslStringList>, vrt_path: &Path, file_path: &Path) -> Result<()> {
    let src_ds =  Dataset::open(vrt_path)?;
    let tgt_srs = SpatialRef::from_epsg( tgt_epsg)?;
    let driver_name = get_driver_name_for_extension( img_extension).ok_or(misc_error(format!("unsupported image type {}", img_extension)))?;

    let mut warp = warp::SimpleWarpBuilder::new( &src_ds, file_path)?;

    warp.set_tgt_srs( &tgt_srs);
    warp.set_tgt_extent_from_bbox( bbox);
    warp.set_tgt_resolution( res_x, res_y);
    warp.set_tgt_format( driver_name)?;

    if let Some(ref opts) = *opts {
        warp.set_create_options( opts);
    }
        
    warp.exec()?;
    Ok(())
}

pub fn get_values_for_vrt_positions (vrt_path: impl AsRef<Path>, band_index: usize, sub_no_data: Option<f64>, pts: &[(f64,f64)]) -> Result<Vec<f64>> 
{
    let ds =  Dataset::open(vrt_path)?;
    get_values_for_positions( &ds, band_index, sub_no_data, pts)
}

pub fn compress_create_opts ()->RasterCreationOptions {
    let mut co = RasterCreationOptions::new();
    co.add_name_value("COMPRESS", "DEFLATE");
    co.add_name_value("PREDICTOR", "2");
    co
}

/// crop the provided dataset by cutting top/bottom lines that contain more nodata values than the given threshold and
/// skip leading/trailing columns with nodata from the rest.
/// This function is mainly useful when warping a dataset between different SRS that do not preserve bounds (e.g. from UTM to WGS84)
pub fn crop_no_data<P> (ds: &Dataset, path: P, create_opts: Option<RasterCreationOptions>) -> Result<Dataset> 
    where P: AsRef<Path>
{
    let n_bands = ds.raster_count();
    if n_bands < 1 { return Err( OdinGdalError::MiscError("no rasterbands to crop".into())) }
    if ! is_homogenous(ds) { return Err( OdinGdalError::MiscError("dataset not homogenous".into())) }

    let bbox = get_data_bounds(ds, 1)?;
    let width = bbox.east - bbox.west + 1;
    let height = bbox.south - bbox.north + 1;

    let driver = ds.driver();
    let band_type = ds.rasterband(1)?.band_type();
    let mut tgt_ds = create_dataset( &driver, path, width, height, n_bands, band_type, create_opts)?;

    for k in 1..=n_bands {
        let src_band = ds.rasterband(k)?;
        let mut tgt_band = tgt_ds.rasterband(k)?;
        copy_rasterband( &src_band, &mut tgt_band, bbox.west, bbox.north, bbox.east, bbox.south)?;
    }

    // copy the meta info
    if let Ok(srs) = ds.spatial_ref() { tgt_ds.set_spatial_ref( &srs)?; }
    if let Ok(geo_transform) = ds.geo_transform() { tgt_ds.set_geo_transform(&geo_transform)?;  }
    tgt_ds.set_projection( ds.projection().as_str())?;

    Ok(tgt_ds)
}

/// check if dimensions and raster type of all bands are the same
pub fn is_homogenous (ds: &Dataset)->bool {
    let (n_cols, n_rows) = ds.raster_size();
    let n_bands = ds.raster_count();
    if n_bands == 0 { return false } 

    let band = ds.rasterband(1).unwrap(); // we checked if there is at least one
    let band_type = band.band_type();
    let (w,h) = band.size();
    if w != n_cols || h != n_rows { return false }

    for i in 2..=n_bands {
        let band = ds.rasterband(i).unwrap();
        if band.band_type() != band_type { return false }
        let (w,h) = band.size();
        if w != n_cols || h != n_rows { return false }
    }

    true
}

pub fn create_dataset<P> (driver: &Driver, path: P, width: usize, height: usize, n_bands: usize, data_type: GdalDataType, co: Option<RasterCreationOptions>)->Result<Dataset> 
    where P: AsRef<Path>
{
    use GdalDataType::*;
    if let Some(co) = co {
        match data_type {
            UInt8   => Ok( driver.create_with_band_type_with_options::<u8,P>(path, width, height, n_bands, &co)? ),
            UInt16  => Ok( driver.create_with_band_type_with_options::<u16,P>(path, width, height, n_bands, &co)? ),
            UInt32  => Ok( driver.create_with_band_type_with_options::<u32,P>(path, width, height, n_bands, &co)? ),
            UInt64  => Ok( driver.create_with_band_type_with_options::<u64,P>(path, width, height, n_bands, &co)? ),
            Int8    => Ok( driver.create_with_band_type_with_options::<i8,P>(path, width, height, n_bands, &co)? ),
            Int16   => Ok( driver.create_with_band_type_with_options::<i16,P>(path, width, height, n_bands, &co)? ),
            Int32   => Ok( driver.create_with_band_type_with_options::<i32,P>(path, width, height, n_bands, &co)? ),
            Int64   => Ok( driver.create_with_band_type_with_options::<i64,P>(path, width, height, n_bands, &co)? ),
            Float32 => Ok( driver.create_with_band_type_with_options::<f32,P>(path, width, height, n_bands, &co)? ),
            Float64 => Ok( driver.create_with_band_type_with_options::<f64,P>(path, width, height, n_bands, &co)? ),
            _ => Err( OdinGdalError::MiscError("unsupported GDAL data type".into()))
        }

    } else {
        match data_type {
            UInt8   => Ok( driver.create_with_band_type::<u8,P>(path, width, height, n_bands)? ),
            UInt16  => Ok( driver.create_with_band_type::<u16,P>(path, width, height, n_bands)? ),
            UInt32  => Ok( driver.create_with_band_type::<u32,P>(path, width, height, n_bands)? ),
            UInt64  => Ok( driver.create_with_band_type::<u64,P>(path, width, height, n_bands)? ),
            Int8    => Ok( driver.create_with_band_type::<i8,P>(path, width, height, n_bands)? ),
            Int16   => Ok( driver.create_with_band_type::<i16,P>(path, width, height, n_bands)? ),
            Int32   => Ok( driver.create_with_band_type::<i32,P>(path, width, height, n_bands)? ),
            Int64   => Ok( driver.create_with_band_type::<i64,P>(path, width, height, n_bands)? ),
            Float32 => Ok( driver.create_with_band_type::<f32,P>(path, width, height, n_bands)? ),
            Float64 => Ok( driver.create_with_band_type::<f64,P>(path, width, height, n_bands)? ),
            _ => Err( OdinGdalError::MiscError("unsupported GDAL data type".into()))
        }
    }
}

pub fn copy_full_rasterband (src: &RasterBand, tgt: &mut RasterBand)->Result<()> {
    let (w,h) = tgt.size();
    copy_rasterband( src, tgt, 0, 0, w-1, h-1)
}

pub fn copy_rasterband( src: &RasterBand, tgt: &mut RasterBand, min_x: usize, min_y: usize, max_x: usize, max_y: usize)->Result<()> {
    use GdalDataType::*;

    let data_type = src.band_type();
    if data_type != tgt.band_type() { return Err( OdinGdalError::MiscError("different rasterband types".into()) ) }

    match data_type {
        UInt8   => copy_rasterband_type::<u8>( src, tgt, min_x, min_y, max_x, max_y, 0),
        UInt16  => copy_rasterband_type::<u16>( src, tgt, min_x, min_y, max_x, max_y, 0),
        UInt32  => copy_rasterband_type::<u32>( src, tgt, min_x, min_y, max_x, max_y, 0),
        UInt64  => copy_rasterband_type::<u64>( src, tgt, min_x, min_y, max_x, max_y, 0),
        Int8    => copy_rasterband_type::<i8>( src, tgt, min_x, min_y, max_x, max_y, 0),
        Int16   => copy_rasterband_type::<i16>( src, tgt, min_x, min_y, max_x, max_y, 0),
        Int32   => copy_rasterband_type::<i32>( src, tgt, min_x, min_y, max_x, max_y, 0),
        Int64   => copy_rasterband_type::<i64>( src, tgt, min_x, min_y, max_x, max_y, 0),
        Float32 => copy_rasterband_type::<f32>( src, tgt, min_x, min_y, max_x, max_y, 0.0),
        Float64 => copy_rasterband_type::<f64>( src, tgt, min_x, min_y, max_x, max_y, 0.0),
        _ => Err( OdinGdalError::MiscError("unsupported GDAL data type".into()))
    }
}

fn copy_rasterband_type <T: Copy + GdalType> (src: &RasterBand, tgt: &mut RasterBand, min_x: usize, min_y: usize, max_x: usize, max_y: usize, init: T)->Result<()> {
    let (src_width,src_height) = src.size();
    let (tgt_width,tgt_height) = tgt.size();

    let mut src_buf: Vec<T> = vec![init; src_width];
    let mut tgt_buf: Buffer<T> = Buffer::new((tgt_width,1), vec![init; tgt_width]);

    for j in 0..=max_y - min_y {
        src.read_into_slice( (0 as isize, (j + min_y) as isize), (src_width,1), (src_width,1), &mut src_buf, None)?;
        copy_scanline( &src_buf, tgt_buf.data_mut(), min_x, max_x)?;
        tgt.write( (0 as isize, j as isize), (tgt_width,1), &mut tgt_buf)?;
    }

    Ok(())
}

fn copy_scanline<T: Copy> (src: &[T], tgt: &mut[T], min_x: usize, max_x: usize)->Result<()> {
    if src.len() <= max_x { return Err( OdinGdalError::MiscError("invalid copy bounds".into()) ) }
    for i in 0..=max_x - min_x {
        tgt[i] = src[i + min_x];
    }
    Ok(())
}

pub fn copy_dataset_rasterbands (src_ds: &Dataset, src_band: usize, tgt_ds: &mut Dataset, tgt_band: usize)->Result<()> {
    let (sw, sh) = src_ds.raster_size();
    let (tw, th) = tgt_ds.raster_size();
    if (sw != tw) || (sh != th) { return Err( OdinGdalError::MiscError("different raster sizes".into())) } 

    let src = src_ds.rasterband(src_band)?;
    let mut tgt = tgt_ds.rasterband(tgt_band)?;

    copy_rasterband( &src, &mut tgt, 0, 0, sw-1, sh-1)
}

/// compute the values of a RasterBand line-by-line from provided input bands
/// note that input and output bands have to have the same size and compatible types
pub fn compute_rasterband_lines<T,F> (input_bands: &[&RasterBand], output_band: &mut RasterBand, init_val: T, mut f: F)->Result<()> 
    where T: Copy + GdalType, F: FnMut(&Vec<Vec<T>>,&mut [T])
{
    let (w,h) = output_band.size();

    let n_input = input_bands.len();
    let mut input_lines: Vec<Vec<T>> = input_bands.iter().map(|b| vec![init_val;w]).collect();

    let mut output_buf: Buffer<T> = Buffer::new((w,1), vec![init_val; w]);

    for j in 0..h {
        for i in 0..n_input {
            input_bands[i].read_into_slice( (0 as isize, j as isize), (w,1), (w,1), &mut input_lines[i], None)?;
        }
        f( input_lines.as_ref(), output_buf.data_mut());
        output_band.write( (0 as isize, j as isize), (w,1), &mut output_buf)?;
    }

    Ok(())
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum FillNoDataAlg {
    InverseDistance,
    NearestNeighbor
}

pub fn fill_nodata (ds: &mut Dataset, max_dist: usize, smoothing_passes: usize, fill_alg: FillNoDataAlg) -> Result<()> {
    unsafe {
        let mut options = CslStringList::new();
        match fill_alg {
            FillNoDataAlg::InverseDistance => options.add_string("INTERPOLATION=INV_DIST"),
            FillNoDataAlg::NearestNeighbor => options.add_string("INTERPOLATION=NEAREST")
        };

        let n_bands = ds.raster_count();

        for i in 1..=n_bands {
            let band = ds.rasterband(i)?;
            let h_band = band.c_rasterband();

            let c_res = gdal_sys::GDALFillNodata( h_band, null_mut(), max_dist as f64, 0, smoothing_passes as c_int, options.as_ptr(), None, null_mut());
            if c_res != gdal_sys::CPLErr::CE_None {
                return Err(last_gdal_error());
            }
        }

        Ok(())
    }
}

/// get min/max row/column indices of provided dataset so that the returned sub-region does not contain any nodata values.
/// NOTE - this assumes the data area edges are continuous and not concave (flaring out to opposite edges)
/// this is the case for WGS_84 <-> UTM conversions but not necessarily for for other SRS
/// the algorithm first determines the top and bottom row for which the number of leading/trailing NODATA pixels is not decreasing anymore
/// and then gets the minimum/maximum scanline indices for pixels with valid data in between those min/max lines
pub fn get_data_bounds (ds: &Dataset, ref_band: usize) -> Result<BoundingBox<usize>> {
    let (n_cols, n_rows) = ds.raster_size();
    let n_bands = ds.raster_count();
    if ref_band > n_bands { return Err(OdinGdalError::MiscError("reference band index out of range".into())) }

    let mut min_x = 0;
    let mut max_x = n_cols-1;
    let mut min_y = 0;
    let mut max_y = n_rows-1;

    let band = ds.rasterband(ref_band)?;
    if let Some(nodata_value) = band.no_data_value() { // nothing to do if we don't hava a nodata value
        let (b_cols,b_rows) = band.size();
        if b_cols != n_cols || b_rows != n_rows { return Err(OdinGdalError::MiscError("dataset has non-uniform dimensions".into())) }

        let mut scanline = vec![ nodata_value; n_cols]; // scanline buffer

        let mut last_total = n_cols;
        //--- get top row within [min_x..max_x]
        for j in 0..n_rows {
            band.read_into_slice( (0 as isize, j as isize), (n_cols,1), (n_cols,1), &mut scanline, None)?;
            let (n_leading, n_trailing, n_total) = count_nodata( &scanline, nodata_value);
            if n_leading + n_trailing < n_total { return Err(OdinGdalError::MiscError("upper data edge is not convex".into())) }

            if n_total >= last_total {
                min_y = if n_total > last_total {j-1} else {j};
                break;
            }
            last_total = n_total;
        }

        //--- get bottom row within [min_x..max_x]
        let mut last_total = n_cols;
        for j in (0..n_rows).rev() {
            band.read_into_slice( (0 as isize, j as isize), (n_cols,1), (n_cols,1), &mut scanline, None)?;
            let (n_leading, n_trailing, n_total) = count_nodata( &scanline, nodata_value);
            if n_leading + n_trailing < n_total { return Err(OdinGdalError::MiscError("lower data edge is not convex".into())) }

            let x1 = n_cols - n_trailing -1;
            if n_total >= last_total {
                max_y = if n_total > last_total {j+1} else {j};
                break;
            }
            last_total = n_total;
        }

        //--- get min/max x in between
        for j in min_y..=max_y {
            band.read_into_slice( (0 as isize, j as isize), (n_cols,1), (n_cols,1), &mut scanline, None)?;
            let (n_leading, n_trailing, n_total) = count_nodata( &scanline, nodata_value);
            if n_leading + n_trailing < n_total { return Err(OdinGdalError::MiscError("data area has holes".into())) }

            min_x = usize::max( min_x, n_leading);
            max_x = usize::min( max_x, n_cols - n_trailing -1);
        }
    }

    Ok( BoundingBox { west: min_x, south: max_y, east: max_x, north: min_y } )
}

fn count_nodata (scanline: &[f64], nodata_value: f64)->(usize,usize,usize) {
    let mut n_leading = 0;
    let mut n_trailing = 0;

    //--- count leading nodata
    for i in 0..scanline.len() {
        if scanline[i] == nodata_value { n_leading += 1 } else { break }
    }
    let mut n_total = n_leading; 

    if n_leading < scanline.len() {
        //--- count trailing nodata 
        for i in (0..scanline.len()).rev() {
            if scanline[i] == nodata_value { n_trailing += 1 } else { break }
        }

        //--- count nodata in-between leading and trailing
        n_total += n_trailing;
        for i in n_leading..scanline.len()-n_trailing {
            if scanline[i] == nodata_value { 
                n_total += 1 
            }
        }

    } else { // scanline does not contain any data
        n_trailing = n_leading;
    }

    (n_leading, n_trailing, n_total)
}


pub fn read_row <T: Copy + GdalType> (band: &RasterBand, row: isize, buf: &mut [T])->Result<()> {
    let cols = buf.len();
    Ok( band.read_into_slice( (0, row), (cols,1), (cols,1), buf, None)? )
}

/// check if provided Metadata reference has all the provided (domain,key,value) items
pub fn has_meta_info<M> (meta: &M, item_specs: &[(&str,&str,Option<&str>)])->bool where M: Metadata {
    for (domain,key,value) in item_specs {
        if !has_meta_info_item( meta, domain, key, value.clone()) { return false }
    }
    true
}

pub fn has_meta_info_item<M> (meta: &M, domain: &str, key: &str, expected_val: Option<&str>)->bool where M: Metadata {
    if let Some(val) = meta.metadata_item( key, domain) {
        if let Some(expected_val) = expected_val {
            if expected_val != val { return false }
        }
        true
    } else {
        false
    }
}

/// find the index of the rasterband that has all the specified (domain,key,val) meta infos
pub fn find_rasterband_index (ds: &Dataset, item_specs: &[(&str,&str,Option<&str>)]) -> Option<u32> {
    for i in 0..ds.raster_count() {
        let band_index = i + 1;
        if let Ok(band) = ds.rasterband( band_index) {
            if has_meta_info( &band, item_specs) {
                return Some(band_index as u32)
            }
        }
    }

    None
}

#[macro_export]
macro_rules! rasterband_index_for {
    ( $ds:ident, $( ( $dom:expr,$key:expr,$val:expr ) ),+ ) =>
    {
        {
            let mut res: Option<u32> = None;
            for i in 0..$ds.raster_count() {
                let band_index = i + 1;
                if let Ok(band) = $ds.rasterband( band_index) {
                    $(
                        if !odin_gdal::has_meta_info_item( &band, $dom, $key, $val) { continue }
                    )*
                    res = Some(band_index as u32);
                    break;
                }
            }
            res
        }
    }
}

#[derive(Debug)]
pub struct RasterInfo {
    pub cols: usize,
    pub left: f64,
    pub right: f64,
    pub dx: f64,

    pub rows: usize,
    pub top: f64,
    pub bottom: f64,
    pub dy: f64
}

pub fn get_raster_info (ds: &Dataset)->Result<RasterInfo> {
    let (cols,rows) = ds.raster_size();
    let a = ds.geo_transform()?;

    let left = a[0];
    let dx = a[1];
    let right = left + (dx * cols as f64); 

    let top = a[3];
    let dy = a[5];
    let bottom = top + (dy * rows as f64);

    Ok( RasterInfo { cols, left, right, dx, rows, top, bottom, dy } )
}

/// syntactic sugar for creating CslStringLists. Note this panics if an invalid string (that cannot be translated into
/// a C string) is provided.
#[macro_export]
macro_rules! csl_string_list {
    ( $( $v:expr ),* ) => {
        { 
            use odin_gdal::CslStringList;
            let mut co_list = CslStringList::new();
            $( co_list.add_string( $v).unwrap(); )*
            co_list
        }
    }
}

/* #endregion misc high level functions */