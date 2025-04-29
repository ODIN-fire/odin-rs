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

use std::f64::{self, NAN};

/// cartographic coordinates.
/// note this overlaps with geo.rs (esp GeoPoint3) but Cartographic is mostly intended as an
/// internal format based on radians, to efficiently interface with unit-less 3rd party libraries.
/// It is mostly used in internal computations, not to store/retrieve uom values

use geo::{Bearing, Destination, Distance, Haversine, Point};
use crate::{
    abs, atan, atan2, cos, pow2, signum, sin, sqrt, tan, sin2, 
    PI, HALF_PI, 
    angle::{Latitude, Longitude}, 
    cartesian3::Cartesian3,
    geo::{GeoPoint, GeoRect, GeoPolygon}, 
    geo_constants::*, 
    BoundingBox
};


/// radian,meter based geodetic (or spherical) coordinates
/// fields have to have the same names as Cesium.Cartographic so that we can serialize/deserialize JSON without additional objects
#[derive(Debug,Clone,Copy,PartialEq)]
pub struct Cartographic {
    pub longitude: f64, // radians
    pub latitude: f64,  // radians
    pub height: f64     // meters above ellipsoid
}

impl Cartographic {
    pub fn zero ()->Self {
        Cartographic { longitude: 0.0, latitude: 0.0, height: 0.0 }
    }

    pub fn new (longitude:f64, latitude: f64, height: f64)->Self {
        Cartographic { longitude, latitude, height }
    }

    pub fn from_degrees (lon: f64, lat: f64, height: f64)->Self {
        Cartographic::new( lon.to_radians(), lat.to_radians(), height)
    }

    pub fn from_radians (lon: f64, lat: f64, height: f64)->Self {
        Cartographic::new( lon, lat, height)
    }

    pub fn longitude_deg (&self)-> f64 { self.longitude.to_degrees() }
    pub fn latitude_deg (&self)-> f64 { self.latitude.to_degrees() }

    /// get mean earth radius geocentric polar coords for this geodetic datum
    pub fn to_geocentric (&self)-> Self {
        let longitude = self.longitude;
        let latitude = geocentric_latitude(self.latitude);
        let height = self.height;

        Cartographic { longitude, latitude, height }
    }

    pub fn to_geodetic (&self)->Self {
        let longitude = self.longitude;
        let latitude = geodetic_latitude(self.latitude);
        let height = self.height;

        Cartographic { longitude, latitude, height }
    }

    // just a simplistic average of given coordinates
    pub fn mean (vertices: &[Cartographic])->Self {
        let n = vertices.len() as f64;
        let mut avg_lon: f64 = 0.0;
        let mut avg_lat: f64 = 0.0;
        let mut avg_height: f64 = 0.0;

        for v in vertices {
            avg_lon += v.longitude;
            avg_lat += v.latitude;
            avg_height += v.height;
        }

        Cartographic { longitude: avg_lon / n, latitude: avg_lat / n, height: avg_height / n }
    }

    /// haversine initial bearing in radians
    pub fn bearing_to (&self, other: &Cartographic)->f64 {
        let dlon = other.longitude - self.longitude;
        let cos_lat = cos(other.latitude);
        atan2( sin(dlon) * cos_lat, cos(self.latitude) * sin(other.latitude) - sin(self.latitude) * cos_lat * cos(dlon)) % HALF_PI
    }

    /// haversine final bearing in radians
    pub fn bearing_from (&self, other: &Cartographic)->f64 {
        (other.bearing_to( self) + PI) % HALF_PI
    }

    /// haversine great circle distance based on average radius (earth_radius + height)
    pub fn distance_to (&self, other: &Cartographic)->f64 {
        let dlat = other.latitude - self.latitude;
        let dlon = other.longitude  - self.longitude;

        let a = sin2(dlat / 2.0) + cos(self.latitude) * cos(other.latitude) * sin2( dlon / 2.0);
        let c = 2.0 * atan2( sqrt(a), sqrt(1.0 - a));

        let r_self = earth_radius_at_geodetic_latitude( self.latitude) + self.height;
        let r_other = earth_radius_at_geodetic_latitude( other.latitude) + other.height;

        ((r_self + r_other) / 2.0) * c 
    }

    pub fn from_lon_lat_degrees_slice (a: &[(f64,f64)]) -> Vec<Cartographic> {
        let mut vs: Vec<Cartographic> = Vec::with_capacity(a.len());
        for p in a {
            vs.push( Cartographic { longitude: p.0.to_radians(), latitude: p.1.to_radians(), height: 0.0 })
        }
        vs
    }

    /// this assumes lat and lon are spherical coordinates
    /// use only as approximation (e.g. to calculate great-circle values) in case this is geodetic
    pub fn spherical_to_cartesian3 (&self, radius: f64)->Cartesian3 {
        let cos_lat = cos(self.latitude);
        let sin_lat = sin(self.latitude);
        let cos_lon = cos(self.longitude);
        let sin_lon = sin(self.longitude);

        let x = radius * cos_lon * cos_lat;
        let y = radius * sin_lon * cos_lat;
        let z = radius * sin_lat;

        Cartesian3::new( x, y, z)
    }

    /// get vertices of GeoPolygon.
    /// Note - this does NOT include a duplicated first/last point
    pub fn vertices_of (poly: &GeoPolygon)->Vec<Cartographic> {
        let len = poly.exterior_coords_count()-1;
        let mut vs: Vec<Cartographic> = Vec::with_capacity(len);

        let mut i = 0;
        for p in poly.points_iter() {
            if i < len { vs.push( Cartographic::from(p)) } 
        }
        vs
    }
}

pub fn geocentric_latitude (geodetic_latitude: f64) -> f64 {
    atan( tan(geodetic_latitude) * ONE_MINUS_E_EARTH_SQUARED)
}

pub fn geodetic_latitude (geocentric_latitude: f64) -> f64 {
    atan( tan(geocentric_latitude) / ONE_MINUS_E_EARTH_SQUARED )
}

pub fn earth_radius_at_geodetic_latitude (geodetic_latitude: f64) -> f64 {
    let cos_φ = cos(geodetic_latitude);
    let sin_φ = sin(geodetic_latitude);

    let n = ((EARTH_RADIUS_RATIO_SQUARED * cos_φ).powi(2)) + ((POLAR_EARTH_RADIUS_SQUARED * sin_φ).powi(2));
    let d = ((EQUATORIAL_EARTH_RADIUS * cos_φ).powi(2)) + ((POLAR_EARTH_RADIUS * sin_φ).powi(2));
    
    sqrt(n/d)
}

#[inline]
pub fn parallel_curvature_radius (latitude: f64) -> f64 {
    EQUATORIAL_EARTH_RADIUS_SQUARED / sqrt( pow2(EQUATORIAL_EARTH_RADIUS * cos(latitude)) + pow2(POLAR_EARTH_RADIUS * sin(latitude)) )
}

#[inline]
pub fn parallel_radius (latitude: f64) -> f64 {
    parallel_curvature_radius(latitude) * cos(latitude)
}

/// angular distance in radians for given latitude in radians and distance in meters
pub fn parallel_distance_rad (latitude: f64, dist_meter: f64)->f64 {
    let r = parallel_radius(latitude);
    dist_meter / r
}

pub fn mean_distance_rad (dist_meter: f64)->f64 {
    dist_meter / MEAN_EARTH_RADIUS
}

/// approximate the geodetic center of the given surface polygon (i.e. ignoring height of vertices)
/// note this is just an approximation as the moment computation is based on the assumption that vertices lie on
/// a spherical surface, not an ellipsoid
pub fn approximate_surface_centroid (vertices: &[Cartographic])->Cartographic {
    let avg_radius:f64 = vertices.iter().fold( 0.0, |acc,v| acc + earth_radius_at_geodetic_latitude(v.latitude) ) / vertices.len() as f64; 
    let vs: Vec<Cartesian3> = vertices.iter().map( |v| Cartesian3::from(v)).collect();

    let mut m = moment_of_spherical_poly(&vs); // this is assuming vertices are on spherical surface
    m.scale_to_length(avg_radius);

    m.into() // this will have height != 0 indicating the approximation error since the conversion is to ellipsoid (WGS84) coords
}

/// this assumes vertices are on a spherical surface hence it is here and not in Cartesian3
/// see https://github.com/chrisveness/geodesy/blob/8f4ef33d3a2e6b7127b5fa619a0b98042dbd0745/latlon-nvector-spherical.js
/// (based on Stoke's theorem)
fn moment_of_spherical_poly (vertices: &[Cartesian3])->Cartesian3 {
    let mut moment = Cartesian3::zero();
    let n = vertices.len();

    for i in 0..n {
        let p1 = &vertices[i];
        let p2 = &vertices[ (i+1)%n ];
        
        moment += p1.cross(p2).scaled_to_unit_length() * Cartesian3::angle_between(p1,p2) / 2.0;
    }

    moment
}

/// expand each vertex of the given geodetic polygon on great circle through centroid and the vertex by given dist
pub fn expand_on_centroid (vertices: &[Cartographic], dist: f64) -> Vec<Cartographic> {
    let ce = approximate_surface_centroid(vertices);
    let cp = Point::from( ce);
    let mut vs: Vec<Cartographic> = Vec::with_capacity(vertices.len());

    for v in vertices {
        let p = Point::new( v.longitude_deg(), v.latitude_deg());
        let mut b = (Haversine.bearing( p, cp) + 180.0) % 360.0; // opposite of init bearing from vertex to centroid
        let pp = Haversine.destination( p, b, dist);
        vs.push( pp.into())
    }

    vs
}

pub fn get_bbox (vertices: &[Cartographic]) -> GeoRect {
    let mut west: f64 = f64::MAX;
    let mut east: f64 = f64::MIN;
    let mut north: f64 = f64::MIN;
    let mut south:  f64 = f64::MAX;

    for v in vertices {
        if v.longitude < west { west = v.longitude }
        if v.longitude > east { east = v.longitude }
        if v.latitude < south { south = v.latitude }
        if v.latitude > north { north = v.latitude }
    }

    let west = Longitude::from_degrees( west.to_degrees());
    let south = Latitude::from_degrees( south.to_degrees());
    let east = Longitude::from_degrees( east.to_degrees());
    let north = Latitude::from_degrees( north.to_degrees());

    GeoRect::from_wsen( west, south, east, north)
}

pub fn get_bbox_rad (vertices: &[Cartographic]) -> BoundingBox<f64> {
    let mut west: f64 = f64::MAX;
    let mut east: f64 = f64::MIN;
    let mut north: f64 = f64::MIN;
    let mut south:  f64 = f64::MAX;

    for v in vertices {
        if v.longitude < west { west = v.longitude }
        if v.longitude > east { east = v.longitude }
        if v.latitude < south { south = v.latitude }
        if v.latitude > north { north = v.latitude }
    }

    BoundingBox { west, south, east, north }
}

/// meridional degrees per distance in meters - approximation of Bowring
fn meridional_deg_per_meters_at (lat: f64, s: f64)->f64 {
    let lat = lat.to_radians();
    s / (111132.92 - 559.82 * cos(lat * 2.0) + 1.175 * cos(lat * 4.0) - 0.0023 * cos(lat * 6.0))
}

/// parallel degrees per distance in meters - approximation of Bowring
fn parallel_deg_per_meters_at (lat: f64, s: f64)->f64 {
    let lat = lat.to_radians();
    s / (111412.84 * cos(lat) - 93.5 * cos(lat * 3.0) + 0.118 * cos(lat * 5.0))
}

impl From<Cartographic> for GeoPoint {
    fn from (p: Cartographic) -> Self {
        GeoPoint::from_lon_lat_degrees( p.longitude_deg(), p.latitude_deg())
    }
}

impl From<&Cartographic> for GeoPoint {
    fn from (p: &Cartographic) -> Self {
        GeoPoint::from_lon_lat_degrees( p.longitude_deg(), p.latitude_deg())
    }
}

impl From<GeoPoint> for Cartographic {
    fn from (p: GeoPoint) -> Self {
        Cartographic::from_degrees( p.longitude_deg(), p.latitude_deg(), 0.0)
    }
}

impl From<&GeoPoint> for Cartographic {
    fn from (p: &GeoPoint) -> Self {
        Cartographic::from_degrees( p.longitude_deg(), p.latitude_deg(), 0.0)
    }
}

impl From<Point<f64>> for Cartographic {
    fn from (p: Point<f64>) -> Self {
        Cartographic::from_degrees( p.x(), p.y(), 0.0)
    }
}

impl From<&Point<f64>> for Cartographic {
    fn from (p: &Point<f64>) -> Self {
        Cartographic::from_degrees( p.x(), p.y(), 0.0)
    }
}

impl From<Cartographic> for Point<f64> {
    fn from (p: Cartographic) -> Point<f64> {
        Point::new( p.longitude_deg(), p.latitude_deg())
    }
}

impl From<&Cartographic> for Point<f64> {
    fn from (p: &Cartographic) -> Point<f64> {
        Point::new( p.longitude_deg(), p.latitude_deg())
    }
}

impl From<&Cartesian3> for Cartographic {

    /// convert cartesian ECEF coordinates to Cartographic (WGS84)
    /// see
    ///    Olson, D. K. (1996).
    ///    Converting Earth-Centered, Earth-Fixed Coordinates to Geodetic Coordinates.
    ///    IEEE Transactions on Aerospace and Electronic Systems, 32(1), 473–476. https://doi.org/10.1109/7.481290
    ///
    /// this is ~1.4x faster than Osen and roundtrip errors are still below 1e-10 so we pick this as default
    fn from (p: &Cartesian3) -> Self {
        let a  = EQUATORIAL_EARTH_RADIUS; // semi-major earth
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
        write!(f, "{{ longitude: {:.5}, latitude: {:.5}, height: {:.1} }}",
            self.longitude.to_degrees(), self.latitude.to_degrees(), self.height)
    }
}
