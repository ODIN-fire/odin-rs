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

use crate::{angle,geo::{self, GeoPoint}};
use crate::*;
use crate::datetime;
use num::{Num, ToPrimitive, traits, zero};



// no 'I' or 'O' bands
const LAT_BAND: [char;22] = ['A','B','C','D','E','F','G','H','J','K','L','M','N','P','Q','R','S','T','U','V','W','X'];

#[derive(Debug,Copy,Clone,Serialize,Deserialize,PartialEq)]
pub struct UtmZone {
    pub zone: u32,
    pub band: char,
}

impl UtmZone {
    fn is_north(&self) -> bool { self.band >= 'N' }
    fn central_meridian(&self) -> f64 { -180.0 + (self.zone as f64)*6.0 - 3.0 }
}

pub struct UtmRect (BoundingBox<f64>,UtmZone);

impl UtmRect {
    pub fn from_wsen_meters (wsen: &[f64;4], utm_zone: UtmZone) -> UtmRect {
        UtmRect(BoundingBox::<f64>::from_wsen(wsen),utm_zone)
    }
}

#[derive(Debug,Copy,Clone,Serialize,Deserialize,PartialEq)]
pub struct UTM {
    pub easting: f64,
    pub northing: f64,
    utm_zone: UtmZone,
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
    let band = LAT_BAND[ (lat_deg / 8.0).trunc() as usize ];

	UtmZone { zone, band }
}

// Krueger approximation - see https://en.wikipedia.org/wiki/Universal_Transverse_Mercator_coordinate_system
pub fn geo_to_utm_zone (geo: &GeoPoint, utm_zone: UtmZone) -> Option<UTM> {
    let lat_deg = geo.latitude().degrees();
    let lon_deg = geo.longitude().degrees();

    // let a = 6378.137
    // let f = 0.0033528106647474805 // 1.0/298.257223563
    // let n = 0.0016792203863837047 // f / (2.0 - f)
    // let n2 = 2.8197811060466384E-6 // n * n
    // let n3 = 4.7350339184131065E-9 // n2 * n
    // let n4 = 7.951165486017604E-12 // n2 * n2
    // let A = 6367.449145823416 // (a / (1.0 + n)) * (1 + n2/4.0 + n4/64.0)
    let α1 = 8.377318188192541E-4; // n/2.0 - (2.0/3.0)*n2 + (5.0/16.0)*n3
    let α2 = 7.608496958699166E-7; // (13.0/48.0)*n2 - (3.0/5.0)*n3
    let α3 = 1.2034877875966646E-9; // (61.0/240.0)*n3
    let C = 0.08181919084262149; // (2.0*sqrt(n)) / (1.0 + n)
    // let k0 = 0.9996
    let D = 6364.902166165087; // k0 * A
    let E0 = 500.0;

    if lat_deg < -80.0 || lat_deg > 84.0 { return None } // not valid outside

    let band = LAT_BAND[ (lat_deg + 80.0 / 6.0) as usize ];

    let φ = lat_deg.to_radians();
    let λ = lon_deg.to_radians();
    let λ0 = (((utm_zone.zone -1) * 6 - 180 + 3) as f64).to_radians();
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

    let easting = (E0 + D*(η + (α1 * cos(ξ2)*sinh(η2)) + (α2 * cos(ξ4)*sinh(η4)) + (α3 * cos(ξ6)*sinh(η6)))) * 1000.0;
    let northing = (N0 + D*(ξ + (α1 * sin(ξ2)*cosh(η2)) + (α2 * sin(ξ4)*cosh(η4)) + (α3 * sin(ξ6)*cosh(η6)))) * 1000.0;

    Some( UTM {easting, northing, utm_zone} )
}

pub fn geo_to_utm (geo_point: &GeoPoint) -> Option<UTM> {
    let utm_zone = naive_utm_zone( geo_point);
    geo_to_utm_zone( geo_point, utm_zone)
}

pub fn utm_to_geo (utm: &UTM) -> GeoPoint {
    let UTM { easting, northing, utm_zone} = utm;
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
    let λ0 = (utm_zone.zone * 6 - 183).to_f64().unwrap().to_radians();
    let λ = λ0 + atan( sin(ξʹ)/cosh(ηʹ));

    let lat_deg = φ.to_degrees();
    let lon_deg = λ.to_degrees();

    GeoPoint::from_lon_lat_degrees( lon_deg, lat_deg)
}
