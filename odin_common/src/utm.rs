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
#![allow(unused,uncommon_codepoints,non_snake_case)]
use std::{ops::Deref, hash::{Hash,Hasher}};

use chrono::{DateTime,Utc};
use serde::{Deserialize, Serialize};
use uom::si::f64::{Length};
use uom::si::length::{meter,foot};
use num::{Num, ToPrimitive, traits, zero};
use ndarray::prelude::*;

use crate::{
    angle, define_error, BoundingBox, sin, sinh, asin, atan, atan2, atanh, cos, cosh, sqrt,
    datetime,
    geo::{self, GeoPoint, GeoRect}
};

define_error!{ pub OdinUtmError =
    Unmappable(String) : "geo coordinates cannot be mapped to UTM: {0}"
}

// NOTE - those are the MGRS band identifiers, UTM only uses 'N' or 'S'
// no 'I' or 'O' bands
const LAT_BAND: [char;22] = ['A','B','C','D','E','F','G','H','J','K','L','M','N','P','Q','R','S','T','U','V','W','X'];

#[derive(Debug,Copy,Clone,Serialize,Deserialize,PartialEq)]
pub struct UtmZone {
    pub zone: u32,
    pub band: char, // NOTE - this is the MGRS id, NOT the UTM 'N' or 'S'
}

impl UtmZone {
    pub fn is_north(&self) -> bool { self.band >= 'N' }
    pub fn central_meridian(&self) -> f64 { -180.0 + (self.zone as f64)*6.0 - 3.0 }

    pub fn epsg (&self)->u32 {
        if self.is_north() {
            32600 + self.zone
        } else {
            32700 + self.zone
        }
    }

    pub fn utm_band_id (&self)->&'static str {
        if self.is_north() { "N" } else { "S" }
    }
}

impl std::fmt::Display for UtmZone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_north() {
            write!(f, "{}N", self.zone)
        } else {
            write!(f, "{}S", self.zone)
        }
    }
}

#[derive(Clone,Debug, Serialize,Deserialize)]
pub struct UtmRect {
    pub bbox: BoundingBox<f64>,
    pub utm_zone: UtmZone
}

impl UtmRect {
    pub fn from_wsen_meters (west: f64, south: f64, east: f64, north: f64, utm_zone: UtmZone) -> UtmRect {
        let bbox = BoundingBox { west, south, east, north };
        UtmRect { bbox, utm_zone }
    }

    pub fn from_utm_point (center: &UtmPoint, width: f64, height: f64) -> UtmRect {
        let x = center.easting;
        let y = center.northing;
        let w2 = width / 2.0;
        let h2 = height / 2.0;
        let utm_zone = center.utm_zone;

        let bbox = BoundingBox {
            west: x - w2,
            south: y - h2,
            east: x + w2,
            north: y + h2
        };
        UtmRect { bbox, utm_zone }
    }

    /// a constructor variant that represents the hull of a matrix of cells with size 'res' around a given center point
    /// nx2, ny2 are measured from (but not including) the center row, column, i.e. both dimensions are odd numbers (2*nx2 + 1) x (2*ny2 + 1)
    pub fn from_centered_matrix (center: &UtmPoint, res: f64, nx2: usize, ny2: usize)->UtmRect {
        let dw = (nx2 as f64 + 0.5) * res;
        let dh = (ny2 as f64 + 0.5) * res;
        let west = center.easting - dw;
        let east = center.easting + dw;
        let north = center.northing + dh;
        let south = center.northing - dh;
        let utm_zone = center.utm_zone;

        let bbox = BoundingBox { west, south, east, north };
        UtmRect { bbox, utm_zone }
    }

    pub fn epsg (&self)->u32 {
       self.utm_zone.epsg()
    }

    pub fn round (&mut self) {
        self.bbox.west  = self.bbox.west.round();
        self.bbox.south = self.bbox.south.round();
        self.bbox.east  = self.bbox.east.round();
        self.bbox.north = self.bbox.north.round();
    }

    pub fn rounded (&self)->Self {
        Self::from_wsen_meters(
            self.bbox.west.round(),
            self.bbox.south.round(),
            self.bbox.east.round(),
            self.bbox.north.round(),
            self.utm_zone
        )
    }

    pub fn contains_point (&self, utm: &UtmPoint)->Option<bool> {
        let bbox = &self.bbox;
        if self.utm_zone == utm.utm_zone {
            Some( utm.easting <= bbox.east && utm.easting >= bbox.west && utm.northing >= bbox.south && utm.northing <= bbox.north )
        } else {
            utm.map_to_zone( self.utm_zone).and_then( |p| self.contains_point( &p))
        }
    }

    pub fn add_meters (&self, dw: f64, ds: f64, de: f64, dn: f64)->UtmRect {
        let bbox = BoundingBox {
            west: self.west + dw,
            south: self.south + ds,
            east: self.east + de,
            north: self.north + dn
        };
        UtmRect { bbox, utm_zone: self.utm_zone }
    }

    #[inline] pub fn width (&self)->f64 { self.bbox.east - self.bbox.west }
    #[inline] pub fn height (&self)->f64 { self.bbox.north - self.bbox.south }
    #[inline] pub fn dimensions (&self)->(f64,f64) { (self.width(), self.height()) }

    #[inline] pub fn x_min (&self)->f64 { self.bbox.west }
    #[inline] pub fn x_max (&self)->f64 { self.bbox.east }
    #[inline] pub fn y_min (&self)->f64 { self.bbox.south }
    #[inline] pub fn y_max (&self)->f64 { self.bbox.north }

    pub fn encode (&self)->String {
        format!("{:.0}_{:.0}_{:.0}_{:.0}_{}{}",
            self.bbox.west, self.bbox.south, self.bbox.east, self.bbox.north, self.utm_zone.zone, self.utm_zone.utm_band_id())
    }

    pub fn center (&self)->UtmPoint {
        UtmPoint {
            easting: (self.bbox.west + self.bbox.east) / 2.0,
            northing: (self.bbox.south + self.bbox.north) / 2.0,
            utm_zone: self.utm_zone.clone()
        }
    }

    pub fn nw_point (&self)->UtmPoint {
        UtmPoint {
            easting: self.bbox.west,
            northing: self.bbox.north,
            utm_zone: self.utm_zone.clone()
        }
    }

    pub fn ne_point (&self)->UtmPoint {
        UtmPoint {
            easting: self.bbox.east,
            northing: self.bbox.north,
            utm_zone: self.utm_zone.clone()
        }
    }

    pub fn sw_point (&self)->UtmPoint {
        UtmPoint {
            easting: self.bbox.west,
            northing: self.bbox.south,
            utm_zone: self.utm_zone.clone()
        }
    }

    pub fn se_point (&self)->UtmPoint {
        UtmPoint {
            easting: self.bbox.east,
            northing: self.bbox.south,
            utm_zone: self.utm_zone.clone()
        }
    }

    pub fn from_geo_rect (geo_rect: &GeoRect, rect_mapping: GeoRectMapping)->Result<UtmRect,OdinUtmError> {
        let geo_center = geo_rect.center();
        let geo_ul = GeoPoint::from_lon_lat( geo_rect.west(), geo_rect.north());
        let geo_ll = GeoPoint::from_lon_lat( geo_rect.west(), geo_rect.south());

        let utm_center: UtmPoint = geo_rect.center().try_into()?;
        let utm_zone = utm_center.utm_zone;
        let ul: UtmPoint = try_geo_to_utm_zone( &geo_ul, utm_zone)?;
        let ll: UtmPoint = try_geo_to_utm_zone( &geo_ll, utm_zone)?;

        let w = match rect_mapping {
            GeoRectMapping::Median => (ul.easting + ll.easting) / 2.0,
            GeoRectMapping::ExteriorGeo => ul.easting.max( ll.easting),
            GeoRectMapping::InteriorGeo => ul.easting.min( ll.easting)
        };

        let e = 2.0 * utm_center.easting - w;

        Ok( Self::from_wsen_meters( w, ll.northing, e, ul.northing, utm_zone) )
    }

    pub fn to_geo_rect (&self, rect_mapping: GeoRectMapping)->GeoRect {
        let sw: GeoPoint = self.sw_point().into();
        let nw: GeoPoint = self.nw_point().into();
        let center: GeoPoint = self.center().into();

        let w = match rect_mapping {
            GeoRectMapping::Median      => (sw.longitude_degrees() + nw.longitude_degrees()) / 2.0,
            GeoRectMapping::ExteriorGeo => sw.longitude_degrees().min( nw.longitude_degrees()),
            GeoRectMapping::InteriorGeo => sw.longitude_degrees().max( nw.longitude_degrees())
        };
        let clon = center.longitude_degrees();
        let dw = clon - w;

        GeoRect::from_wsen_degrees(
            clon - dw,
            sw.latitude_degrees(),
            clon + dw,
            nw.latitude_degrees()
        )
    }

    // this is guaranteed to return odd numbers so that we we have the same extend from the center column,row in each direction
    pub fn get_centered_grid_dimensions (&self, res: f64, exterior: bool)->(usize,usize) {
        self.bbox.get_centered_grid_dimensions(res, exterior)
    }

    /// nx2 and ny2 are the number grid steps in each x,y direction
    /// note this returns elements in row-major order, i.e. (0,0) is the NW corner
    pub fn get_centered_geo_grid_points (&self, res: f64, nx2: usize, ny2: usize)->Vec<GeoPoint> {
        let center = self.center();
        let utm_zone = self.utm_zone;

        let mut gps: Vec<GeoPoint> = Vec::with_capacity((nx2*ny2*4) as usize );

        for j in -(ny2 as isize)..(ny2 as isize + 1) {
            let northing = center.northing - (j as f64 * res);
            for i in -(nx2 as isize)..(nx2 as isize +1) {
                let easting = center.easting + (i as f64 * res);
                let p = UtmPoint{easting,northing,utm_zone};
                let gp: GeoPoint = p.into();
                gps.push( gp);
            }
        }

        gps
    }

    pub fn get_centered_geo_grid (&self, res: f64, exterior: bool)->Array2<GeoPoint> {
        let center = self.center();
        let utm_zone = self.utm_zone;
        let (nx,ny) = self.get_centered_grid_dimensions(res, exterior);
        let gps = self.get_centered_geo_grid_points( res, nx/2, ny/2);

        // safe to unwrap as we know gps has enough elements
        Array::from_shape_vec( (nx,ny), gps).unwrap()
    }

    /// Note that (0,0) is the NW corner cell, i.e. we count row indices south-wards and column indices east-wards
    pub fn get_grid_cell_indices (&self, nx: usize, ny: usize, p: &UtmPoint)->Option<(usize,usize)>{
        if p.utm_zone != self.utm_zone {
            p.map_to_zone( self.utm_zone).and_then(|p| self.get_grid_cell_indices( nx, ny, &p))
        } else {
            if p.easting < self.west { return None }
            if p.easting > self.east { return None } // outside horizontal range
            if p.northing < self.south { return None }
            if p.northing > self.north { return None } // outside vertical range

            let res_x = self.width() / (nx as f64);
            let res_y = self.height() / (ny as f64);

            let i = ((p.easting - self.west) / res_x).floor() as usize;
            let j = ((self.north - p.northing) / res_y).floor() as usize;

            Some( (i,j) )
        }
    }

    pub fn get_clipped_grid_cell_indices (&self, nx: usize, ny: usize, p: &UtmPoint)->Option<(usize,usize)> {
        if p.utm_zone != self.utm_zone {
            p.map_to_zone( self.utm_zone).and_then(|p| self.get_clipped_grid_cell_indices( nx, ny, &p))
        } else {
            let res_x = self.width() / (nx as f64);
            let res_y = self.height() / (ny as f64);

            let i = ((p.easting - self.west) / res_x).floor() as isize;
            let i = i.max(0).min(nx as isize -1) as usize;

            let j = ((self.north - p.northing) / res_y).floor() as isize;
            let j = j.max(0).min(ny as isize -1) as usize;

            Some( (i,j) )
        }
    }

    pub fn get_cell_points (&self, nx: usize, ny: usize)->Vec<UtmPoint> {
        let res_x = self.width() / (nx as f64);
        let res_y = self.height() / (ny as f64);
        let res_x2 = res_x / 2.0;
        let res_y2 = res_y / 2.0;
        let utm_zone = self.utm_zone;

        let mut cps: Vec<UtmPoint> = Vec::with_capacity(nx * ny);
        for j in 0..ny {
            for i in 0..nx {
                let northing = self.north - (j as f64)*res_y - res_y2;
                let easting = self.west + (i as f64)*res_x + res_x2;

                let p = UtmPoint{easting,northing,utm_zone};
                print!("  {:.0},{:.0}  ", p.easting, p.northing);
                cps.push( p);
            }
            println!();
        }

        cps
    }
}

/* this is ambiguous - we need a GeoRectMapping
impl From<UtmRect> for GeoRect {
    fn from(r: UtmRect) -> Self {
        let sw: GeoPoint = r.sw_point().into();
        let ne: GeoPoint = r.ne_point().into();

        GeoRect::from_wsen_degrees(
            sw.longitude_degrees(),
            sw.latitude_degrees(),
            ne.longitude_degrees(),
            ne.latitude_degrees()
        )
    }
}
*/

#[derive(Clone, Copy)]
pub enum GeoRectMapping {
    Median,  // average width
    ExteriorGeo,  // greater width (geo is hull, i.e. UtmRect will be fully inside GeoRect)
    InteriorGeo    // smaller width (utm is hull, i.e. GeoRect will be fully inside UtmRect)
}

impl Deref for UtmRect {
    type Target = BoundingBox<f64>;
    fn deref (&self)->&Self::Target { &self.bbox }
}

// we can't impl From<GeoRect> since UTM is only defined between [-80,84] deg latitude, i.e. the conversion can fail

#[derive(Debug,Copy,Clone,Serialize,Deserialize,PartialEq)]
pub struct UtmPoint {
    pub easting: f64,
    pub northing: f64,
    pub utm_zone: UtmZone,
}

impl UtmPoint {
    #[inline] pub fn x(&self)->f64 { self.easting }
    #[inline] pub fn y(&self)->f64 { self.northing }

    pub fn map_to_zone (&self, new_zone: UtmZone)->Option<UtmPoint> {
        if self.utm_zone == new_zone {
            Some( self.clone() )
        } else {
            let geo = utm_to_geo( &self);
            geo_to_utm_zone( &geo, new_zone)
        }
    }

    pub fn round (&mut self) {
        self.easting  = self.easting.round();
        self.northing = self.northing.round();
    }

    pub fn rounded (self)->Self {
        UtmPoint {
            easting: self.easting.round(),
            northing: self.northing.round(),
            utm_zone: self.utm_zone
        }
    }

    /// this can be used to rasterize locations to the nearest multiple of the given resolution in meters
    pub fn to_gridded (&self, res_meters: usize)->Self {
        let res = res_meters as f64;
        let easting = (self.easting / res).round() * res;
        let northing = (self.northing / res).round() * res;
        let utm_zone = self.utm_zone;

        UtmPoint { easting, northing, utm_zone }
    }

    pub fn grid_points_from_center (&self, width: Length, height: Length, raster: Length, exterior: bool)->Vec<UtmPoint> {
        let r = raster.get::<meter>();
        let r2 = 2.0 * r;
        let w = width.get::<meter>();
        let h = height.get::<meter>();

        let n = if exterior { (w / r2).ceil() } else { (w / r2).floor() } as isize;
        let m = if exterior { (h / r2).ceil() } else { (h / r2).floor() } as isize;

        let n2 = (2 * n) + 1;
        let m2 = (2 * m) + 1;

        let mut vs: Vec<UtmPoint> = Vec::with_capacity( (n2 * m2) as usize );
        let utm_zone = self.utm_zone;

        for j in 0..m2 {
            let northing = self.northing + ((j-m) as f64) * r;
            for i in 0..n2 {
                let easting = self.easting + ((i-n) as f64) * r;
                vs.push( UtmPoint { easting, northing, utm_zone });
            }
        }

        vs
    }

    pub fn from_geo_points_in_zone( gps: &[GeoPoint], utm_zone: UtmZone)->std::result::Result<Vec<UtmPoint>,OdinUtmError> {
        gps.iter().map( |p| try_geo_to_utm_zone( &p, utm_zone)).collect()
    }

    pub fn add_delta (&self, delta_easting: f64, delta_northing: f64)->Self {
        UtmPoint{
            easting: self.easting + delta_easting,
            northing: self.northing + delta_northing,
            utm_zone: self.utm_zone
        }
    }
}

pub fn utm_zone (geo: &GeoPoint) -> u32 {
    let lat_deg = geo.latitude().degrees();
    let lon_deg = geo.longitude().degrees();

    // handle special cases (Svalbard/Norway)
    if lat_deg > 55.0 && lat_deg < 64.0 && lon_deg > 2.0 && lon_deg < 6.0 {
        return 32
    }

    if lat_deg > 71.0 {
        if lon_deg >= 6.0 && lon_deg < 9.0 {
            return 31
        }
        if (lon_deg >= 9.0 && lon_deg < 12.0) || (lon_deg >= 18.0 && lon_deg < 21.0) {
            return 33
        }
        if (lon_deg >= 21.0 && lon_deg < 24.0) || (lon_deg >= 30.0 && lon_deg < 33.0) {
            return 35
        }
    }

    (((lon_deg + 180.0) / 6.0).trunc() as u32 % 60) + 1
}

pub fn naive_utm_zone (geo: &GeoPoint) -> UtmZone {
    let lon_deg = geo.longitude().degrees();
    let zone = (((lon_deg + 180.0) / 6.0).trunc() as u32 % 60) + 1;

    let lat_deg = geo.latitude().degrees();
    let band = latitude_band( lon_deg, lat_deg);

	UtmZone { zone, band }
}

pub fn latitude_band (lon_deg: f64, lat_deg: f64)->char {
    // the polar regions are special cases in MGRS
    if lat_deg < -80.0 {
       return if lon_deg < 0.0 { 'A' } else { 'B' }
    }
    if lat_deg >= 72.0 {
        if lat_deg >= 84.0 {
            return if lon_deg < 0.0 { 'Y' } else { 'Z' }
        } else {
            return 'X'
        }
    }

    let i = ((lat_deg + 80.0) / 8.0) as usize;
    let band = [ 'C', 'D', 'E', 'F', 'G', 'H', 'J', 'K', 'L', 'M', 'N', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W']; // no 'I', 'O'

    band[i]
}

pub fn try_geo_to_utm_zone (geo: &GeoPoint, utm_zone: UtmZone) -> Result<UtmPoint,OdinUtmError> {
    geo_to_utm_zone( geo, utm_zone).ok_or( OdinUtmError::Unmappable(geo.to_string()))
}

// Krueger approximation - see https://en.wikipedia.org/wiki/Universal_Transverse_Mercator_coordinate_system
pub fn geo_to_utm_zone (geo: &GeoPoint, utm_zone: UtmZone) -> Option<UtmPoint> {
    let lat_deg = geo.latitude().degrees();
    let lon_deg = geo.longitude().degrees();

    // let a = 6378.137
    // let f = 0.0033528106647474805 // 1.0/298.257223563
    // let n = 0.0016792203863837047 // f / (2.0 - f)
    // let n2 = 2.8197811060466384E-6 // n * n
    // let n3 = 4.7350339184131065E-9 // n2 * n
    // let n4 = 7.951165486017604E-12 // n2 * n2
    // let A = 6367.449145823416 // (a / (1.0 + n)) * (1 + n2/4.0 + n4/64.0)
    let a1 = 8.377318188192541E-4; // n/2.0 - (2.0/3.0)*n2 + (5.0/16.0)*n3
    let a2 = 7.608496958699166E-7; // (13.0/48.0)*n2 - (3.0/5.0)*n3
    let a3 = 1.2034877875966646E-9; // (61.0/240.0)*n3
    let C = 0.08181919084262149; // (2.0*sqrt(n)) / (1.0 + n)
    // let k0 = 0.9996
    let D = 6364.902166165087; // k0 * A
    let E0 = 500.0;

    if lat_deg < -80.0 || lat_deg > 84.0 { return None } // not valid outside

    let φ = lat_deg.to_radians();
    let λ = lon_deg.to_radians();
    let λ0 = (((utm_zone.zone as i64 -1) * 6 - 180 + 3) as f64).to_radians();
    let dλ = λ - λ0;
    let N0 = if φ < 0.0 { 10000.0 } else { 0.0 };

    let sin_φ = sin(φ);
    let t = sinh( atanh(sin_φ) - C * atanh( C*sin_φ));

    let ξ = atan( t/cos(dλ));
    let ξ2 = ξ * 2.0;
    let ξ4 = ξ * 4.0;
    let ξ6 = ξ * 6.0;

    let η = atanh( sin(dλ) / sqrt(1.0 + t*t));
    let η2 = η * 2.0;
    let η4 = η * 4.0;
    let η6 = η * 6.0;

    let easting = (E0 + D*(η + (a1 * cos(ξ2)*sinh(η2)) + (a2 * cos(ξ4)*sinh(η4)) + (a3 * cos(ξ6)*sinh(η6)))) * 1000.0;
    let northing = (N0 + D*(ξ + (a1 * sin(ξ2)*cosh(η2)) + (a2 * sin(ξ4)*cosh(η4)) + (a3 * sin(ξ6)*cosh(η6)))) * 1000.0;

    Some( UtmPoint {easting, northing, utm_zone} )
}

impl TryFrom<GeoPoint> for UtmPoint {
    type Error = OdinUtmError;

    fn try_from(gp: GeoPoint) -> Result<Self,Self::Error> {
        geo_to_utm(&gp).ok_or( OdinUtmError::Unmappable(gp.to_string()))
    }
}

impl TryFrom<&GeoPoint> for UtmPoint {
    type Error = OdinUtmError;

    fn try_from(gp: &GeoPoint) -> Result<Self,Self::Error> {
        geo_to_utm(gp).ok_or( OdinUtmError::Unmappable(gp.to_string()))
    }
}

impl From<UtmPoint> for GeoPoint {
    fn from (utm: UtmPoint) -> Self {
        utm_to_geo( &utm)
    }
}

impl From<&UtmPoint> for GeoPoint {
    fn from (utm: &UtmPoint) -> Self {
        utm_to_geo( utm)
    }
}

pub fn geo_to_utm (geo_point: &GeoPoint) -> Option<UtmPoint> {
    let utm_zone = naive_utm_zone( geo_point);
    geo_to_utm_zone( geo_point, utm_zone)
}

pub fn try_geo_to_utm (geo_point: &GeoPoint) -> Result<UtmPoint,OdinUtmError> {
    geo_to_utm(geo_point).ok_or( OdinUtmError::Unmappable(geo_point.to_string()))
}

pub fn utm_to_geo (utm: &UtmPoint) -> GeoPoint {
    let UtmPoint { easting, northing, utm_zone} = utm;
    let N = northing / 1000.0;
    let E = easting / 1000.0;

    //let A = 6367.449145823416;
    //let k0 = 0.9996;
    let k0_A = 6364.902166165086634;
    let n = 0.0016792203863837047; // f / (2.0 - f)
    let β1 = 0.000837732164082144;
    let β2 = 0.00000005906110863719917;
    let β3 = 0.00000000016769911794379754;
    let δ1 = 0.003356551448628875;
    let δ2 = 0.000006571913193172695;
    let δ3 = 0.0000000176774599620756;

    let E0 = 500.0;
    let N0 = if utm_zone.is_north() { 0.0 } else { 10000.0 };

    let ξ = (N - N0)/k0_A;
    let ξ2 = ξ * 2.0;
    let ξ4 = ξ * 4.0;
    let ξ6 = ξ * 6.0;

    let η = (E - E0)/k0_A;
    let η2 = η * 2.0;
    let η4 = η * 4.0;
    let η6 = η * 6.0;

    let β1_2 = β1 * 2.0;
    let β2_4 = β2 * 4.0;
    let β3_6 = β3 * 6.0;

    let ξʹ = ξ - ((β1*sin(ξ2)*cosh(η2)) + (β2*sin(ξ4)*cosh(η4)) + (β3*sin(ξ6)*cosh(η6)));
    let ηʹ = η - ((β1*cos(ξ2)*sinh(η2)) + (β2*cos(ξ4)*sinh(η4)) + (β3*cos(ξ6)*sinh(η6)));

    let χ = asin( sin(ξʹ) / cosh(ηʹ));

    let φ = χ + (δ1*sin(2.0*χ)) + (δ2*sin(4.0*χ)) + (δ3*sin(6.0*χ));
    let λ0 = (utm_zone.zone as i64 * 6 - 183).to_f64().unwrap().to_radians();
    let λ = λ0 + atan2(sinh(ηʹ), cos(ξʹ));

    let lat_deg = φ.to_degrees();
    let lon_deg = λ.to_degrees();

    GeoPoint::from_lon_lat_degrees( lon_deg, lat_deg)
}


pub fn try_get_xy_vecs (ps: &[UtmPoint], utm_zone: UtmZone)->std::result::Result<(Vec<f64>,Vec<f64>),OdinUtmError> {
    get_xy_vecs(ps, utm_zone).ok_or( OdinUtmError::Unmappable("incompatible UTM zones".into()))
}

pub fn get_xy_vecs (ps: &[UtmPoint], utm_zone: UtmZone)->Option<(Vec<f64>,Vec<f64>)> {
    if !ps.is_empty() {
        let mut xs = Vec::with_capacity(ps.len());
        let mut ys = Vec::with_capacity(ps.len());

        for p in ps.iter() {
            if p.utm_zone != utm_zone {
                if let Some(p) = p.map_to_zone(utm_zone) {
                    xs.push( p.easting);
                    ys.push( p.northing);
                } else {
                    return None
                }
            } else {
                xs.push( p.easting);
                ys.push( p.northing);
            }
        }
        Some( (xs,ys) )

    } else {
        Some( (Vec::with_capacity(0),Vec::with_capacity(0)) )
    }
}
