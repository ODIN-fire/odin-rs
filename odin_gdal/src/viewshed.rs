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

//! viewshed computation (visible area from an observer position) based on GDAL's `GDALViewshedGenerate()`, and
//! conversion of the resulting visibility raster into simplified GeoJSON polygons

use std::{ffi::{CStr, CString}, fs::File, io::{BufWriter, Write}, path::Path, ptr::{null, null_mut}};
use gdal::{
    Dataset, DriverManager, cpl::CslStringList,
    spatial_ref::{AxisMappingStrategy, CoordTransform, SpatialRef},
    vector::{Geometry, LayerAccess, LayerOptions},
};
use gdal_sys::{CPLErr, GDALViewshedMode, GDALViewshedOutputType, OGRFieldType, OGRwkbGeometryType};
use libc::c_char;

use crate::errors::{Result, last_gdal_error, misc_error, reset_last_gdal_error};

/// GDAL default for the curvature coefficient (`1 - refraction`, with a standard refraction coefficient of 1/7)
pub const DEFAULT_CURV_COEFF: f64 = 0.85714;

/// raster values of the visibility dataset returned by [`compute_viewshed`]
pub const VISIBLE: f64 = 1.0;
pub const INVISIBLE: f64 = 0.0;

/// parameters for viewshed computation and polygon generation. Lengths are in units of the (projected) DEM SRS
#[derive(Debug,Clone)]
pub struct ViewshedParams {
    pub observer_height: f64,    // observer height above ground
    pub target_height: f64,      // height above ground of target points (e.g. 0 for visible ground)
    pub max_distance: f64,       // max distance from observer (0 for unlimited)
    pub curv_coeff: f64,         // earth curvature coefficient (1.0: no refraction, 0.0: no curvature correction)
    pub min_patch_cells: usize,  // visible or invisible patches with less cells get merged into their surroundings (0: keep all)
    pub simplify: f64,           // polygon simplification tolerance (0: no simplification)
    pub precision: usize,        // number of decimals for WGS84 GeoJSON coordinates
}

impl Default for ViewshedParams {
    fn default()->Self {
        ViewshedParams {
            observer_height: 10.0,
            target_height: 0.0,
            max_distance: 40_000.0,
            curv_coeff: DEFAULT_CURV_COEFF,
            min_patch_cells: 50,
            simplify: 30.0,
            precision: 5,
        }
    }
}

/// compute the visibility raster (single Byte band in-memory dataset with [`VISIBLE`] and [`INVISIBLE`] values, the
/// latter also used for cells beyond max_distance) for an observer at (`x`,`y`) in coordinates of the `dem` SRS.
/// Note that the DEM has to use a projected SRS (e.g. UTM) with the same horizontal and vertical units. If
/// `params.max_distance` is positive the returned dataset only covers the respective window around the observer
pub fn compute_viewshed (dem: &Dataset, band: usize, x: f64, y: f64, params: &ViewshedParams)->Result<Dataset> {
    let driver = CString::new("MEM")?;
    let tgt_name = CString::new("")?;

    unsafe {
        reset_last_gdal_error();
        let h_band = dem.rasterband( band)?.c_rasterband();
        let h_ds = gdal_sys::GDALViewshedGenerate(
            h_band, driver.as_ptr(), tgt_name.as_ptr(), null_mut(),
            x, y, params.observer_height, params.target_height,
            VISIBLE, INVISIBLE, INVISIBLE, // visible, invisible, out-of-range
            -1.0,                          // no nodata value
            params.curv_coeff,
            GDALViewshedMode::GVM_Edge,
            params.max_distance,
            None, null_mut(),
            GDALViewshedOutputType::GVOT_NORMAL,
            null_mut()
        );
        if h_ds.is_null() { return Err( last_gdal_error()) }

        Ok( Dataset::from_c_dataset( h_ds) )
    }
}

/// remove raster polygons (connected areas of same value) with less than `threshold` cells by merging them into their
/// largest neighbor polygon. This modifies the `band` of `ds` in place
pub fn sieve (ds: &mut Dataset, band: usize, threshold: usize, eight_connected: bool)->Result<()> {
    unsafe {
        reset_last_gdal_error();
        let h_band = ds.rasterband( band)?.c_rasterband();
        let connectedness = if eight_connected { 8 } else { 4 };
        let res = gdal_sys::GDALSieveFilter( h_band, null_mut(), h_band, threshold as i32, connectedness, null_mut(), None, null_mut());
        if res != CPLErr::CE_None { return Err( last_gdal_error()) }
    }
    Ok(())
}

/// get the (MultiPolygon) geometry of all non-zero cells of `band` in `ds`, in the SRS of `ds`. Polygons are simplified
/// with the given `tolerance` (in SRS units) if it is positive. Polygons that are empty after simplification are dropped
pub fn polygonize_nonzero (ds: &Dataset, band: usize, tolerance: f64)->Result<Geometry> {
    let srs = ds.spatial_ref()?;
    let mut vds = DriverManager::get_driver_by_name("MEM")?.create_vector_only("")?;
    let mut layer = vds.create_layer( LayerOptions{ name: "polygons", srs: Some(&srs), ty: OGRwkbGeometryType::wkbPolygon, options: None })?;
    layer.create_defn_fields( &[("DN", OGRFieldType::OFTInteger)])?;

    unsafe {
        reset_last_gdal_error();
        let h_band = ds.rasterband( band)?.c_rasterband();
        // using the source band as mask band only polygonizes non-zero cells
        let res = gdal_sys::GDALPolygonize( h_band, h_band, layer.c_layer(), 0, null_mut(), None, null_mut());
        if res != CPLErr::CE_None { return Err( last_gdal_error()) }
    }

    let mut mp = Geometry::empty( OGRwkbGeometryType::wkbMultiPolygon)?;
    for feature in layer.features() {
        if let Some(geom) = feature.geometry() {
            let poly = if tolerance > 0.0 { geom.simplify_preserve_topology( tolerance)? } else { geom.clone() };
            if !poly.is_empty() && poly.area() > 0.0 {
                mp.add_geometry( poly)?;
            }
        }
    }

    // simplification preserves the topology of each polygon but polygons that touch at corners can overlap afterwards,
    // which makes the MultiPolygon invalid. The STRUCTURE method merges overlapping parts (LINEWORK would create holes)
    if !mp.is_valid() {
        let mut opts = CslStringList::new();
        opts.set_name_value( "METHOD", "STRUCTURE")?;
        mp = mp.make_valid( &opts)?;
    }
    Ok(mp)
}

/// write a (projected) geometry as a single feature GeoJSON FeatureCollection in WGS84 (lon,lat order) with the
/// given coordinate `precision` (decimals). `properties` has to be a JSON object string (e.g. "{}")
pub fn write_wgs84_geojson<P: AsRef<Path>> (geom: &Geometry, properties: &str, precision: usize, path: P)->Result<()> {
    let mut tgt_srs = SpatialRef::from_epsg( 4326)?;
    tgt_srs.set_axis_mapping_strategy( AxisMappingStrategy::TraditionalGisOrder);
    let mut src_srs = geom.spatial_ref().ok_or( misc_error( "geometry has no spatial reference".into()))?;
    src_srs.set_axis_mapping_strategy( AxisMappingStrategy::TraditionalGisOrder);
    let ct = CoordTransform::new( &src_srs, &tgt_srs)?;
    let wgs84_geom = geom.transform( &ct)?;

    let opt = CString::new( format!("COORDINATE_PRECISION={precision}"))?;
    let mut opts: [*mut c_char; 2] = [ opt.as_ptr() as *mut c_char, null_mut() ];
    let json = unsafe {
        let c_json = gdal_sys::OGR_G_ExportToJsonEx( wgs84_geom.c_geometry(), opts.as_mut_ptr());
        if c_json.is_null() { return Err( last_gdal_error()) }
        let s = CStr::from_ptr( c_json).to_str().map( |s| s.to_string());
        gdal_sys::VSIFree( c_json as *mut libc::c_void);
        s?
    };

    let mut w = BufWriter::new( File::create( path)?);
    write!( w, r#"{{"type":"FeatureCollection","features":[{{"type":"Feature","properties":{},"geometry":{}}}]}}"#, properties, json)?;
    w.flush()?;
    Ok(())
}

/// compute the viewshed polygons for an observer at (`x`,`y`) (in `dem` SRS coordinates) and store them as WGS84
/// GeoJSON file at `path`. Returns the number of polygons
pub fn write_viewshed_geojson<P: AsRef<Path>> (dem: &Dataset, x: f64, y: f64, params: &ViewshedParams, properties: &str, path: P)->Result<usize> {
    let mut vs_ds = compute_viewshed( dem, 1, x, y, params)?;
    if params.min_patch_cells > 1 {
        sieve( &mut vs_ds, 1, params.min_patch_cells, true)?;
    }

    let mut geom = polygonize_nonzero( &vs_ds, 1, params.simplify)?;
    if geom.spatial_ref().is_none() { geom.set_spatial_ref( vs_ds.spatial_ref()?); }
    write_wgs84_geojson( &geom, properties, params.precision, path)?;

    Ok( geom.geometry_count() )
}
