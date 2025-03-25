/*
 * Copyright © 2025, United States Government, as represented by the Administrator of 
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

/// cartographic coordinates.
/// note this overlaps with geo.rs (esp GeoPoint3) but Cartographic is mostly intended as an
/// internal format based on radians, to efficiently interface with unit-less 3rd party libraries.
/// It is mostly used in internal computations, not to store/retrieve uom values

use crate::{cartesian3::Cartesian3, geo_constants::EQATORIAL_EARTH_RADIUS};

#[derive(Debug,Clone,Copy,PartialEq)]
pub struct Cartographic {
    pub longitude: f64, // radians
    pub latitude: f64,  // radians
    pub height: f64     // meters above ellipsoid
}

impl Cartographic {
    pub fn new (longitude:f64, latitude: f64, height: f64)->Self {
        Cartographic { longitude, latitude, height }
    }

    pub fn from_degrees (lon: f64, lat: f64, height: f64)->Self {
        Cartographic::new( lon.to_radians(), lat.to_radians(), height)
    }

    pub fn longitude_deg (&self)-> f64 { self.longitude.to_degrees() }
    pub fn latitude_deg (&self)-> f64 { self.latitude.to_degrees() }

}

impl From<&Cartesian3> for Cartographic {

    /// convert cartesian ECEF coordinates to Cartographic
    /// see
    ///    Olson, D. K. (1996).
    ///    Converting Earth-Centered, Earth-Fixed Coordinates to Geodetic Coordinates.
    ///    IEEE Transactions on Aerospace and Electronic Systems, 32(1), 473–476. https://doi.org/10.1109/7.481290
    ///
    /// this is ~1.4x faster than Osen and roundtrip errors are still below 1e-10 so we pick this as default
    fn from (p: &Cartesian3) -> Self {
        let a  = EQATORIAL_EARTH_RADIUS; // semi-major earth
        let e2 = 6.6943799901377997e-3;
        let a1 = 4.2697672707157535e+4;
        let a2 = 1.8230912546075455e+9;
        let a3 = 1.4291722289812413e+2;
        let a4 = 4.5577281365188637e+9;
        let a5 = 4.2840589930055659e+4;
        let a6 = 9.9330562000986220e-1;

        let x = p.x;
        let y = p.y;
        let z = p.z;

        let zp = z.abs();
        let w2 = x*x + y*y;
        let w = w2.sqrt();
        let z2 = z*z;
        let r2 = w2 + z2;
        let r = r2.sqrt();

        if r >= 100000.0 {
            let lon = y.atan2(x);
            let s2 = z2 / r2;
            let c2 = w2 / r2;
            let mut u = a2 / r;
            let mut v = a3 - a4 / r;

            let mut c = 0.0;
            let mut s = 0.0;
            let mut ss = 0.0;
            let mut lat = 0.0;

            if c2 > 0.3 {
                s = (zp/r)*(1.0 + c2*(a1 + u + s2*v)/r);
                lat = s.asin();
                ss = s*s;
                c = (1.0 - ss).sqrt();
            } else {
                c = (w/r)*(1.0 - s2*(a5 - u - c2*v)/r);
                lat = c.acos();
                ss = 1.0 - c*c;
                s = ss.sqrt();
            }
            let g = 1.0 - e2*ss;
            let rg = a / g.sqrt();
            let rf = a6 * rg;
            u = w - rg * c;
            v = zp - rf * s;
            let f = c * u + s * v;
            let m = c * v - s * u;
            let p = m / (rf / g + f);

            lat += p;
            let alt = f + m*p/2.0;
            if z < 0.0 { lat = -lat; }

            Cartographic::new( lon, lat, alt)

        } else {
            Cartographic::new( 0.0, 0.0, 0.0)
        }
    }
}

impl From<Cartesian3> for Cartographic {
    fn from (p: Cartesian3) -> Self {
        Cartographic::from(&p)
    }
}

impl std::fmt::Display for Cartographic {
    fn fmt (&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{{ longitude: {}, latitude: {}, height: {} }}",
            self.longitude.to_degrees(), self.latitude.to_degrees(), self.height)
    }
}
