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

//! functions to draw (rasterize) 2D geometric primitives from [odin_common::geo] into GDAL datasets.
//!
//! Primitives are given in geographic (lon/lat degree) coordinates. They are converted into OGR geometries, transformed
//! into the spatial reference system of the target dataset (datasets without SRS are assumed to use lon/lat coordinates)
//! and then burned into all bands of the dataset with the provided pixel value, using GDAL's rasterizer
//! (`GDALRasterizeGeometries`). This means the geo transform of the dataset determines where primitives are drawn, and
//! parts outside of the dataset bounds are clipped.
//!
//! Lines and outlines (`draw_..` functions) set the pixels selected by Bresenham's algorithm, filled shapes
//! (`fill_..` functions) set all pixels whose center is inside of the shape.
//!
//! Note that straight lines in lon/lat coordinates become curves in projected SRS (e.g. UTM) - we densify geometries
//! before transforming them to account for that

use std::fmt::Write;
use gdal::{
    Dataset,
    vector::Geometry,
    spatial_ref::{AxisMappingStrategy, CoordTransform, SpatialRef},
    raster::{rasterize, RasterizeOptions}
};
use odin_common::geo::{GeoCircle, GeoLine, GeoLineString, GeoPoint, GeoPolygon, GeoRect, lat_degrees_per_meter, lon_degrees_per_meter};
use crate::errors::{Result, OdinGdalError, misc_error};

/// max segment length in degrees for geometries that are transformed into a projected SRS (~100m)
const MAX_SEGMENT_DEG: f64 = 0.001;

/// number of vertices we use to approximate circles
const N_CIRCLE_VERTICES: usize = 72;

// each primitive has two variants:
//  - `draw_../fill_..(ds, primitive, value)` sets the pixels in all bands of `ds` to `value`
//  - `draw_.._bands/fill_.._bands(ds, primitive, band_values)` sets the pixels of each band in `band_values` (list of
//    (1-based band number, pixel value) tuples) to the respective value, e.g. to draw with RGBA colors

//--- points and lines

/// set the pixel that contains `p`
pub fn draw_geo_point (ds: &mut Dataset, p: &GeoPoint, value: f64)->Result<()> {
    draw_geo_point_bands( ds, p, &all_bands( ds, value))
}

pub fn draw_geo_point_bands (ds: &mut Dataset, p: &GeoPoint, band_values: &[(usize,f64)])->Result<()> {
    draw_wkt( ds, &point_wkt(p), band_values)
}

pub fn draw_geo_line (ds: &mut Dataset, line: &GeoLine, value: f64)->Result<()> {
    draw_geo_line_bands( ds, line, &all_bands( ds, value))
}

pub fn draw_geo_line_bands (ds: &mut Dataset, line: &GeoLine, band_values: &[(usize,f64)])->Result<()> {
    draw_wkt( ds, &line_string_wkt( "LINESTRING", &[line.start(), line.end()]), band_values)
}

pub fn draw_geo_line_string (ds: &mut Dataset, line_string: &GeoLineString, value: f64)->Result<()> {
    draw_geo_line_string_bands( ds, line_string, &all_bands( ds, value))
}

pub fn draw_geo_line_string_bands (ds: &mut Dataset, line_string: &GeoLineString, band_values: &[(usize,f64)])->Result<()> {
    let ps = line_string.as_geo_points();
    if ps.len() < 2 { return Err( misc_error("line string needs at least 2 points".into())) }
    draw_wkt( ds, &line_string_wkt( "LINESTRING", &ps), band_values)
}

//--- rectangles

/// draw the outline of `rect`
pub fn draw_geo_rect (ds: &mut Dataset, rect: &GeoRect, value: f64)->Result<()> {
    draw_geo_rect_bands( ds, rect, &all_bands( ds, value))
}

pub fn draw_geo_rect_bands (ds: &mut Dataset, rect: &GeoRect, band_values: &[(usize,f64)])->Result<()> {
    draw_wkt( ds, &line_string_wkt( "LINESTRING", &rect_ring(rect)), band_values)
}

/// fill `rect`
pub fn fill_geo_rect (ds: &mut Dataset, rect: &GeoRect, value: f64)->Result<()> {
    fill_geo_rect_bands( ds, rect, &all_bands( ds, value))
}

pub fn fill_geo_rect_bands (ds: &mut Dataset, rect: &GeoRect, band_values: &[(usize,f64)])->Result<()> {
    draw_wkt( ds, &polygon_wkt( &rect_ring(rect), &[]), band_values)
}

//--- polygons

/// draw the exterior and interior rings of `polygon`
pub fn draw_geo_polygon (ds: &mut Dataset, polygon: &GeoPolygon, value: f64)->Result<()> {
    draw_geo_polygon_bands( ds, polygon, &all_bands( ds, value))
}

pub fn draw_geo_polygon_bands (ds: &mut Dataset, polygon: &GeoPolygon, band_values: &[(usize,f64)])->Result<()> {
    let mut wkt = String::from("MULTILINESTRING (");
    push_ring( &mut wkt, &closed( polygon.as_exterior_geo_points()));
    for ring in polygon.as_interior_geo_points() {
        wkt.push(',');
        push_ring( &mut wkt, &closed(ring));
    }
    wkt.push(')');
    draw_wkt( ds, &wkt, band_values)
}

/// fill `polygon` (pixels inside of interior rings are not set)
pub fn fill_geo_polygon (ds: &mut Dataset, polygon: &GeoPolygon, value: f64)->Result<()> {
    fill_geo_polygon_bands( ds, polygon, &all_bands( ds, value))
}

pub fn fill_geo_polygon_bands (ds: &mut Dataset, polygon: &GeoPolygon, band_values: &[(usize,f64)])->Result<()> {
    let interiors: Vec<Vec<GeoPoint>> = polygon.as_interior_geo_points().into_iter().map( |ring| closed(ring)).collect();
    draw_wkt( ds, &polygon_wkt( &closed( polygon.as_exterior_geo_points()), &interiors), band_values)
}

//--- circles

/// draw the outline of `circle`. Note that circles are approximated by polygons (using the local degrees per meter at
/// the circle center), which is adequate for radii that are small compared to the earth radius
pub fn draw_geo_circle (ds: &mut Dataset, circle: &GeoCircle, value: f64)->Result<()> {
    draw_geo_circle_bands( ds, circle, &all_bands( ds, value))
}

pub fn draw_geo_circle_bands (ds: &mut Dataset, circle: &GeoCircle, band_values: &[(usize,f64)])->Result<()> {
    draw_wkt( ds, &line_string_wkt( "LINESTRING", &circle_ring(circle)), band_values)
}

/// fill `circle` (see [draw_geo_circle] for approximation)
pub fn fill_geo_circle (ds: &mut Dataset, circle: &GeoCircle, value: f64)->Result<()> {
    fill_geo_circle_bands( ds, circle, &all_bands( ds, value))
}

pub fn fill_geo_circle_bands (ds: &mut Dataset, circle: &GeoCircle, band_values: &[(usize,f64)])->Result<()> {
    draw_wkt( ds, &polygon_wkt( &circle_ring(circle), &[]), band_values)
}

//--- generic geometry drawing

/// draw an arbitrary OGR `geometry` that uses geographic coordinates (lon,lat in degrees - EPSG:4326 with traditional
/// GIS axis order) into all bands of `ds`
pub fn draw_geometry (ds: &mut Dataset, geometry: &Geometry, value: f64)->Result<()> {
    draw_geometry_bands( ds, geometry, &all_bands( ds, value))
}

/// draw an arbitrary OGR `geometry` that uses geographic coordinates into the bands of `band_values` (list of
/// (1-based band number, pixel value) tuples). The geometry is only transformed once, but GDAL only supports one burn
/// value per geometry for all bands of a rasterize call, i.e. we rasterize once for each band
pub fn draw_geometry_bands (ds: &mut Dataset, geometry: &Geometry, band_values: &[(usize,f64)])->Result<()> {
    if band_values.is_empty() { return Ok(()) }

    let geom = to_dataset_srs( ds, geometry)?;
    let geoms = [geom];
    for (band, value) in band_values {
        rasterize( ds, &[*band], &geoms, &[*value], Some(RasterizeOptions::default()))?; // checks band numbers
    }
    Ok(())
}

fn draw_wkt (ds: &mut Dataset, wkt: &str, band_values: &[(usize,f64)])->Result<()> {
    let geometry = Geometry::from_wkt( wkt)?;
    draw_geometry_bands( ds, &geometry, band_values)
}

/// the band values for setting all bands of `ds` to `value`
fn all_bands (ds: &Dataset, value: f64)->Vec<(usize,f64)> {
    (1..=ds.raster_count()).map( |band| (band,value)).collect()
}

/// transform a lon/lat `geometry` into the SRS of `ds` (densifying it if the target SRS is projected)
fn to_dataset_srs (ds: &Dataset, geometry: &Geometry)->Result<Geometry> {
    let mut geom = geometry.clone();

    if let Ok(mut tgt_srs) = ds.spatial_ref() {
        let mut src_srs = SpatialRef::from_epsg(4326)?;
        src_srs.set_axis_mapping_strategy( AxisMappingStrategy::TraditionalGisOrder); // lon,lat
        tgt_srs.set_axis_mapping_strategy( AxisMappingStrategy::TraditionalGisOrder); // the geo transform uses x,y order

        if !tgt_srs.is_geographic() {
            unsafe { gdal_sys::OGR_G_Segmentize( geom.c_geometry(), MAX_SEGMENT_DEG); }
        }

        let ct = CoordTransform::new( &src_srs, &tgt_srs)?;
        geom.transform_inplace( &ct)?;
    }

    Ok(geom)
}

//--- WKT helpers

fn point_wkt (p: &GeoPoint)->String {
    let (lon,lat) = p.lon_lat_degrees();
    format!("POINT ({} {})", lon, lat)
}

/// polygon WKT from (closed) exterior and interior rings
fn polygon_wkt (exterior: &[GeoPoint], interiors: &[Vec<GeoPoint>])->String {
    let mut wkt = String::from("POLYGON (");
    push_ring( &mut wkt, exterior);
    for ring in interiors {
        wkt.push(',');
        push_ring( &mut wkt, ring);
    }
    wkt.push(')');
    wkt
}

fn rect_ring (rect: &GeoRect)->Vec<GeoPoint> {
    let (w, s, e, n) = (rect.west(), rect.south(), rect.east(), rect.north());
    vec![ GeoPoint::from_lon_lat(w,s), GeoPoint::from_lon_lat(e,s), GeoPoint::from_lon_lat(e,n), GeoPoint::from_lon_lat(w,n), GeoPoint::from_lon_lat(w,s) ]
}

fn circle_ring (circle: &GeoCircle)->Vec<GeoPoint> {
    let lon0 = circle.lon().degrees();
    let lat0 = circle.lat().degrees();
    let r = circle.radius().value; // uom stores lengths in the SI base unit, i.e. meters
    let dlon = r * lon_degrees_per_meter( circle.lat());
    let dlat = r * lat_degrees_per_meter( circle.lat());

    let mut ps: Vec<GeoPoint> = Vec::with_capacity( N_CIRCLE_VERTICES + 1);
    for i in 0..N_CIRCLE_VERTICES {
        let a = (i as f64) * std::f64::consts::TAU / (N_CIRCLE_VERTICES as f64);
        ps.push( GeoPoint::from_lon_lat_degrees( lon0 + dlon * a.sin(), lat0 + dlat * a.cos()));
    }
    ps.push( ps[0]);
    ps
}

/// make sure ring is closed (first and last point are the same)
fn closed (mut ps: Vec<GeoPoint>)->Vec<GeoPoint> {
    if let (Some(first), Some(last)) = (ps.first(), ps.last()) {
        if first != last { ps.push( *first); }
    }
    ps
}

fn line_string_wkt (geom_type: &str, ps: &[GeoPoint])->String {
    let mut wkt = String::with_capacity( 16 + ps.len() * 24);
    wkt.push_str( geom_type);
    wkt.push(' ');
    push_ring( &mut wkt, ps);
    wkt
}

/// append "(lon lat, lon lat, ..)"
fn push_ring (wkt: &mut String, ps: &[GeoPoint]) {
    wkt.push('(');
    for (i,p) in ps.iter().enumerate() {
        if i > 0 { wkt.push(','); }
        let (lon,lat) = p.lon_lat_degrees();
        write!( wkt, "{} {}", lon, lat).unwrap();
    }
    wkt.push(')');
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdal::raster::GdalDataType;
    use odin_common::angle::{Latitude, Longitude};
    use crate::{create_dataset, get_mem_driver, get_rasterband_as_vec};

    const NX: usize = 200;
    const NY: usize = 200;

    /// 200x200 dataset covering lon [-122,-121], lat [37,38] (0.005 deg pixels)
    fn geo_ds ()->Dataset {
        let mut ds = create_dataset( &get_mem_driver(), "", NX, NY, 1, GdalDataType::UInt8, None).unwrap();
        ds.set_geo_transform( &[-122.0, 0.005, 0.0, 38.0, 0.0, -0.005]).unwrap();
        ds.set_spatial_ref( &SpatialRef::from_epsg(4326).unwrap()).unwrap();
        ds
    }

    /// 200x200 UTM zone 10N dataset with 100m pixels (20x20km)
    fn utm_ds ()->Dataset {
        let mut ds = create_dataset( &get_mem_driver(), "", NX, NY, 1, GdalDataType::UInt8, None).unwrap();
        ds.set_geo_transform( &[585000.0, 100.0, 0.0, 4125000.0, 0.0, -100.0]).unwrap();
        ds.set_spatial_ref( &SpatialRef::from_epsg(32610).unwrap()).unwrap();
        ds
    }

    fn count (ds: &Dataset, value: u8)->usize {
        let vs: Vec<u8> = get_rasterband_as_vec( ds, 1).unwrap();
        vs.iter().filter( |v| **v == value).count()
    }

    #[test]
    fn test_geo_primitives () {
        let mut ds = geo_ds();
        draw_geo_point( &mut ds, &GeoPoint::from_lon_lat_degrees( -121.5, 37.5), 1.0).unwrap();
        assert_eq!( count(&ds,1), 1);

        let mut ds = geo_ds(); // horizontal line across 100 pixels
        draw_geo_line( &mut ds, &GeoLine::from_geo_points( GeoPoint::from_lon_lat_degrees( -121.9, 37.5), GeoPoint::from_lon_lat_degrees( -121.4, 37.5)), 1.0).unwrap();
        let n = count(&ds,1);
        println!("line pixels: {n}");
        assert!( n >= 100 && n <= 101);

        let mut ds = geo_ds(); // 0.5 x 0.5 deg rect = 100x100 pixels
        let rect = GeoRect::from_wsen_degrees( -121.75, 37.25, -121.25, 37.75);
        fill_geo_rect( &mut ds, &rect, 1.0).unwrap();
        let n = count(&ds,1);
        println!("filled rect pixels: {n}");
        assert_eq!( n, 100*100);

        let mut ds = geo_ds(); // outline of same rect
        draw_geo_rect( &mut ds, &rect, 1.0).unwrap();
        let n = count(&ds,1);
        println!("rect outline pixels: {n}");
        assert!( n >= 396 && n <= 404);

        let mut ds = geo_ds(); // polygon with hole
        let ext = vec![ GeoPoint::from_lon_lat_degrees(-121.9,37.1), GeoPoint::from_lon_lat_degrees(-121.1,37.1), GeoPoint::from_lon_lat_degrees(-121.1,37.9), GeoPoint::from_lon_lat_degrees(-121.9,37.9) ];
        let hole = vec![ GeoPoint::from_lon_lat_degrees(-121.6,37.4), GeoPoint::from_lon_lat_degrees(-121.4,37.4), GeoPoint::from_lon_lat_degrees(-121.4,37.6), GeoPoint::from_lon_lat_degrees(-121.6,37.6) ];
        let polygon = GeoPolygon::from_geo_points( ext, vec![hole]);
        fill_geo_polygon( &mut ds, &polygon, 1.0).unwrap();
        let n = count(&ds,1);
        println!("filled polygon pixels: {n}");
        assert_eq!( n, 160*160 - 40*40);

        draw_geo_polygon( &mut ds, &polygon, 2.0).unwrap(); // outlines over the fill
        let n2 = count(&ds,2);
        println!("polygon outline pixels: {n2}");
        assert!( n2 > 4*160 && n2 < 4*160 + 4*40 + 16);

        let mut ds = geo_ds();
        let circle = GeoCircle::new( Longitude::from_degrees(-121.5), Latitude::from_degrees(37.5), uom::si::f64::Length::new::<uom::si::length::kilometer>(20.0));
        draw_geo_circle( &mut ds, &circle, 1.0).unwrap();
        let n = count(&ds,1);
        println!("circle outline pixels: {n}");
        assert!( n > 0);
    }

    #[test]
    fn test_utm_primitives () {
        // 5x5km rect (around 37.2N, -121.9E) should roughly cover 50x50 pixels of 100m
        let mut ds = utm_ds();
        let lon_d = 5000.0 * lon_degrees_per_meter( Latitude::from_degrees(37.2));
        let lat_d = 5000.0 * lat_degrees_per_meter( Latitude::from_degrees(37.2));
        let rect = GeoRect::from_wsen_degrees( -121.9, 37.2, -121.9 + lon_d, 37.2 + lat_d);
        fill_geo_rect( &mut ds, &rect, 1.0).unwrap();
        let n = count(&ds,1);
        println!("filled UTM rect pixels: {n} (expected ~2500)");
        assert!( (n as f64 - 2500.0).abs() < 100.0);

        // circle with 3km radius ~ 2827 pixels
        let mut ds = utm_ds();
        let circle = GeoCircle::new( Longitude::from_degrees(-121.85), Latitude::from_degrees(37.2), uom::si::f64::Length::new::<uom::si::length::meter>(3000.0));
        fill_geo_circle( &mut ds, &circle, 1.0).unwrap();
        let n = count(&ds,1);
        println!("filled UTM circle pixels: {n} (expected ~2827)");
        assert!( (n as f64 - 2827.0).abs() < 100.0);
    }

    #[test]
    fn test_band_values () {
        let mut ds = create_dataset( &get_mem_driver(), "", NX, NY, 4, GdalDataType::UInt8, None).unwrap();
        ds.set_geo_transform( &[-122.0, 0.005, 0.0, 38.0, 0.0, -0.005]).unwrap();
        ds.set_spatial_ref( &SpatialRef::from_epsg(4326).unwrap()).unwrap();

        // fill 100x100 pixel rect with RGBA (255,128,0,255)
        let rect = GeoRect::from_wsen_degrees( -121.75, 37.25, -121.25, 37.75);
        let rgba = [(1,255.0), (2,128.0), (3,0.0), (4,255.0)];
        fill_geo_rect_bands( &mut ds, &rect, &rgba).unwrap();

        for (band, value) in rgba {
            let vs: Vec<u8> = get_rasterband_as_vec( &ds, band).unwrap();
            let n = vs.iter().filter( |v| **v == value as u8).count();
            println!("band {band} pixels with value {value}: {n}");
            if value > 0.0 { assert_eq!( n, 100*100); }
        }

        // only set band 2 (others unchanged)
        draw_geo_point_bands( &mut ds, &GeoPoint::from_lon_lat_degrees( -121.9, 37.9), &[(2, 42.0)]).unwrap();
        let vs: Vec<u8> = get_rasterband_as_vec( &ds, 2).unwrap();
        assert_eq!( vs.iter().filter( |v| **v == 42).count(), 1);
        let vs: Vec<u8> = get_rasterband_as_vec( &ds, 1).unwrap();
        assert_eq!( vs.iter().filter( |v| **v == 255).count(), 100*100);

        // invalid band numbers are reported as errors
        assert!( draw_geo_point_bands( &mut ds, &GeoPoint::from_lon_lat_degrees( -121.9, 37.9), &[(5, 1.0)]).is_err());
    }
}