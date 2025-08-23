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

use odin_common::{geo::{GeoPoint, GeoPoint3}, ranges::LinearRange, *};
use odin_gdal::{Dataset, GdalValueType, GridPoint, Metadata, MetadataEntry}; // gdal re-exports
use serde::Serialize;

use crate::{OdinGoesrError,Result};


// note this is not a GeoRect since it is not aligned with parallels and meridians
#[derive(Debug,Clone, Copy, Serialize)]
pub struct GoesrBoundingBox {
    pub ne: GeoPoint,
    pub nw: GeoPoint,
    pub sw: GeoPoint,
    pub se: GeoPoint
}

pub fn get_bounds<T> (proj: &GoesrProjection, x_range: &LinearRange<f64>, y_range: &LinearRange<f64>, p: &GridPoint<T>)->GoesrBoundingBox 
    where T: GdalValueType
{
    let x_inc = x_range.inc() / 2.0;
    let y_inc = y_range.inc() / 2.0;

    let x = x_range.at( p.i0);
    let y = y_range.at( p.i1);

    let xw = x - x_inc;
    let xe = x + x_inc;
    let yn = y - y_inc;  // y_inc negative
    let ys = y + y_inc;

    let nw = proj.geo_from_instrument_angles( xw, yn);
    let ne = proj.geo_from_instrument_angles( xe, yn);
    let sw = proj.geo_from_instrument_angles( xw, ys);
    let se = proj.geo_from_instrument_angles( xe, ys);

    GoesrBoundingBox{ne,nw,sw,se}
}

/// structure that supports instrument scan/elevation angle to/from geodetic position conversion
/// this is partly based on metadata included in the data set (NetCDF) files, of which we need at least
/// the satellite specific `longitude_of_projection_origin` and `perspective_point_height`.
/// See https://www.goes-r.gov/products/docs/PUG-L2+-vol5.pdf pg. 23
#[derive(Debug)]
pub struct GoesrProjection {
    h: f64,
    r2: f64,
    lon0: f64,
    c: f64
}

impl GoesrProjection {
    pub fn from_dataset (ds: &Dataset)->Result<Self> {
        // the values we get from the dataset metadata
        let mut f_inv: f64 = 298.257222096;
        let mut lon0: f64 = f64::NAN;
        let mut pph: f64 = f64::NAN;
        let mut r_eq: f64 = 6378137.0;
        let mut r_pol: f64 = 6356752.31414;

        for MetadataEntry { domain:_, key, value } in ds.metadata() {
            if key.ends_with("#inverse_flattening") { f_inv = value.parse()? }
            else if key.ends_with("#longitude_of_projection_origin") { lon0 = value.parse::<f64>()?.to_radians() } // stored as degrees
            else if key.ends_with("#perspective_point_height") { pph = value.parse()? }
            else if key.ends_with("#semi_major_axis") { r_eq = value.parse()? }
            else if key.ends_with("#semi_minor_axis") { r_pol = value.parse()? }
        }
        if (lon0.is_nan() || pph.is_nan()) { return Err( OdinGoesrError::DatasetError("missing projection metadata".into())) }

        let r2:f64 = pow2( r_eq / r_pol);
        let h: f64 = pph + r_eq;
        let c: f64 = pow2(h) - pow2(r_eq);

        Ok( GoesrProjection { h, r2, lon0, c } )
    }

    pub fn geo_from_instrument_angles (&self, ew_scan: f64, ns_elevation: f64)->GeoPoint {
        let x = ew_scan;
        let y = ns_elevation;

        let GoesrProjection{ h, r2, lon0, c } = self;

        let sin_x = x.sin();
        let cos_x = x.cos();
        let sin_y = y.sin();
        let cos_y = y.cos();

        let sin2_x = sin_x * sin_x;
        let cos2_x = 1.0 - sin2_x;
        let sin2_y = sin_y * sin_y;
        let cos2_y = 1.0 - sin2_y;

        let a = sin2_x + cos2_x * (cos2_y + r2*sin2_y);
        let b = -2.0 * h * cos_x * cos_y;
        let r_s = (-b - sqrt(b*b - 4.0*a * c)) / (2.0 * a);
        let s_x = r_s * cos_x * cos_y;
        let s_y = -r_s * sin_x;
        let s_z = r_s * cos_x * sin_y;
    
        let lat_deg = (atan(r2 * s_z/sqrt( pow2(h - s_x) + pow2(s_y)))).to_degrees();
        let lon_deg = (lon0 - atan(s_y / (h - s_x))).to_degrees();

        GeoPoint::from_lon_lat_degrees(lon_deg, lat_deg)
    }

    pub fn geo3_from_instrument_angles (&self, ew_scan: f64, ns_elevation: f64, alt: f64)->GeoPoint3 {
        let p = self.geo_from_instrument_angles(ew_scan, ns_elevation);
        GeoPoint3::from_lon_lat_degrees_alt_meters( p.longitude_degrees(), p.latitude_degrees(), alt)
    }
}
