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
#![allow(unused,uncommon_codepoints,non_snake_case)]


/// this module provides support for geometries on the WGS84 ellipsoid surface
/// Following odin-rs design principles we try to use existing crates, which in this domain
/// are mostly [geo](https://docs.rs/geo/latest/geo/index.html) and [nav_types](https://docs.rs/nav-types/latest/nav_types/index.html).
/// However, these crates do not directly support units-of measure (e.g. for lengths) via the [uom](https://docs.rs/uom/latest/uom/) crate,
/// and underlying value semantics such as euclidian/ellipsoid coordinate system and (normalized) angles as latitude or longitude.
/// We employ the Rust [new type](https://doc.rust-lang.org/rust-by-example/generics/new_types.html) pattern
/// to add those and still retain the capability to use algorithms of the 3rd party foundation crates with minimal cpoying overhead.

use std::fmt::{self,Debug,Display};
use serde::{Serialize,Deserialize};

use serde::ser::{Serialize as SerializeTrait, SerializeSeq, Serializer, SerializeStruct};
use serde::de::{self, Deserialize as DeserializeTrait, Deserializer, Visitor, SeqAccess, MapAccess};

use geo::{Closest, Contains, Coord, CoordsIter, Distance, Line, LineString, Point, Polygon, Rect};
use geo::algorithm::line_measures::metric_spaces::{Haversine,Geodesic};
use geo::algorithm::geodesic_area::GeodesicArea;
use geo::algorithm::haversine_closest_point::HaversineClosestPoint;
use geo_types::PointsIter;

use nav_types::{ECEF,WGS84};

use uom::si::area::square_meter;
use uom::si::f64::{Length,Area};
use uom::si::length::meter;

use chrono::{DateTime,TimeZone,Utc};

use crate::cartesian3::Cartesian3;
use crate::cartographic::Cartographic;
use crate::impl_deserialize_struct;
use crate::angle::{normalize_180, normalize_90, Angle360, Latitude, Longitude};
use crate::datetime::{Dated, EpochMillis};
use crate::json_writer::{JsonWritable,JsonWriter};

pub type GeoCoord = Coord<f64>;

/* #region GeoPoint ***********************************************************************************************/

/// a wrapper for geo::Point that uses geodetic degrees stored as f64
#[derive(Debug,Clone,Copy,PartialEq)]
pub struct GeoPoint(Point);

impl GeoPoint {
    #[inline] pub fn from_lon_lat(lon: Longitude, lat: Latitude) -> Self {
        GeoPoint( Point::new( lon.degrees(), lat.degrees()))
    }
    #[inline] pub fn from_lon_lat_degrees (lon: f64, lat: f64) -> Self {
        GeoPoint( Point::new( normalize_180(lon), normalize_90(lat)))
    }

    /// note this is not just a conversion but clamps the ECEF point to the WGS84 ellipsoid surface
    pub fn from_ecef (ecef: ECEF<f64>) -> Self {
        let wgs84: WGS84<f64> = ecef.into();
        GeoPoint( Point::new(
            normalize_180(wgs84.longitude_degrees()),
            normalize_90(wgs84.latitude_degrees())
        )) // TODO check if nav_types does normalize
    }

    #[inline] pub fn from_point(p:Point) -> Self { GeoPoint(p) } // TODO - should this be pub?

    #[inline] pub fn longitude(&self) -> Longitude { Longitude::from_degrees( self.0.x()) }
    #[inline] pub fn longitude_deg(&self) -> f64 { self.0.x() }

    #[inline] pub fn latitude(&self) -> Latitude { Latitude::from_degrees( self.0.y()) }
    #[inline] pub fn latitude_deg(&self) -> f64 { self.0.y() }

    #[inline] pub fn point<'a> (&'a self) -> &'a Point { &self.0 }
    #[inline] pub fn mut_point<'a> (&'a mut self) -> &'a mut Point { &mut self.0 }

    #[inline] pub fn coord (&self)->GeoCoord { self.0.0.clone() }

    /// non-consuming conversion to ECEF
    #[inline] pub fn as_ecef (&self)->ECEF<f64> { WGS84::from_degrees_and_meters( self.0.y(), self.0.x(), 0.0).into() }

    #[inline] pub fn to_cartographic (&self)->Cartographic {
        Cartographic::from_degrees( self.longitude_deg(), self.latitude_deg(), 0.0)
    }

    #[inline] pub fn to_cartesian3 (&self)->Cartesian3 {
        let cp = Cartographic::from_degrees( self.longitude_deg(), self.latitude_deg(), 0.0);
        Cartesian3::from( cp)
    }

    #[inline] pub fn bearing_from (&self, prev: &GeoPoint)->Angle360 {
        let cp1 = prev.to_cartographic();
        let cp2 = self.to_cartographic();
        Angle360::from_radians( cp2.bearing_from(&cp1))
    }
}

impl fmt::Display for GeoPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{},{}]", self.0.x(),self.0.y())
    }
}

// we don't provide a From<Point<f64>> since that would allow to create a GeoPoint from arbitrary Points 

/// conversion to [x,y,z] in meters.
/// Note we can't impl From<ECEF> since GeoPoints are 2dimensional (altitude = 0)
/// Note also that nav_types::WGS84 uses lat,lon order
impl Into<ECEF<f64>> for GeoPoint {
    fn into (self)->ECEF<f64> { WGS84::from_degrees_and_meters( self.0.y(), self.0.x(), 0.0).into() }
}

impl SerializeTrait for GeoPoint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        let mut state = serializer.serialize_struct("GeoPoint", 2)?;
        state.serialize_field("lon", &self.longitude().degrees())?;
        state.serialize_field("lat", &self.latitude().degrees())?;
        state.end()
    }
}


// note that we support alternative input formats for our virtual fields: "lon", "longitude or "x" for longitude degrees
// and "lat", "latitude" or "y" for latitude degrees. This allows to directly deserialize from data that was
// serialized by `geo` types (which uses "x", "y"). This also means that we have to make sure the original source was
// using the same coordinate system.
impl_deserialize_struct!{ GeoPoint::from_lon_lat_degrees( lon | longitude | x, lat | latitude | y) }

/* #endregion GeoPoint */


/* #region GeoLine ***********************************************************************************************/

#[derive(Debug,Clone)]
pub struct GeoLine(Line);

impl GeoLine {
    pub fn from_geo_points (start: GeoPoint, end: GeoPoint) -> Self {
        GeoLine( Line::new( *start.point(), *end.point()))
    }
    pub fn line<'a> (&'a self) -> &'a Line { &self.0 }

    pub fn start (&self)->GeoPoint { GeoPoint::from_point(self.0.start_point()) }
    pub fn end (&self)->GeoPoint { GeoPoint::from_point(self.0.end_point()) }


    pub fn haversine_distance (&self) -> Length {
        let (start,end) = self.0.points();
        let dist = Haversine.distance( start, end);
        Length::new::<meter>(dist)
    }

    pub fn geodesic_distance (&self) -> Length {
        let (start,end) = self.0.points();
        let dist = Geodesic.distance( start, end);
        Length::new::<meter>(dist)
    }

    pub fn closest_point (&self, p: &GeoPoint) -> ClosestGeoPoint {
        match self.0.haversine_closest_point( &p.0) {
            Closest::Intersection(r) => ClosestGeoPoint::Intersection(GeoPoint::from_point(r)),
            Closest::SinglePoint(r) => ClosestGeoPoint::SinglePoint(GeoPoint::from_point(r)),
            Closest::Indeterminate => ClosestGeoPoint::Indeterminate
        }
    }
}

/// result of a closest point computation
#[derive(Debug)]
pub enum ClosestGeoPoint {
    Intersection(GeoPoint),  // closest point is on reference geometry
    SinglePoint(GeoPoint),   // single, unique solution
    Indeterminate,           // no or multiple solutions
}

impl SerializeTrait for GeoLine {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        let mut state = serializer.serialize_struct("GeoLine", 2)?;
        state.serialize_field("start", &self.start())?;
        state.serialize_field("end", &self.end())?;
        state.end()
    }
}

impl_deserialize_struct!{ GeoLine::from_geo_points( start, end) }

/* #endregion GeoLine */


/* #region GeoRect ***********************************************************************************************/

#[derive(Debug,Clone,PartialEq)]
pub struct GeoRect(Rect);

impl GeoRect {
    pub fn from_min_max (sw: GeoPoint, ne: GeoPoint) -> Self {
        GeoRect( Rect::new( sw.coord(), ne.coord()))
    }

    pub fn from_wsen (west: Longitude, south: Latitude, east: Longitude, north: Latitude) -> Self {
        GeoRect( Rect::new( Point::new( west.degrees(), south.degrees()), Point::new( east.degrees(), north.degrees()) ))
    }

    pub fn from_wsen_degrees (west: f64, south: f64, east: f64, north: f64) -> Self {
        GeoRect( Rect::new( Point::new( west, south), Point::new( east, north) ) )
    }

    pub fn area (&self) -> Area {
        let a = self.0.geodesic_area_unsigned();
        Area::new::<square_meter>(a)
    }
    
    pub fn points (&self) -> Vec<GeoPoint> {
        vec![GeoPoint::from_lon_lat(self.west(), self.north()),
            GeoPoint::from_lon_lat(self.west(), self.south()),
            GeoPoint::from_lon_lat(self.east(), self.north()),
            GeoPoint::from_lon_lat(self.east(), self.south())]
    }

    pub fn to_polygon(&self) -> Polygon {
        self.0.clone().to_polygon()
    }

    pub fn sw_point (&self)->GeoPoint {
        GeoPoint::from_lon_lat_degrees( self.west().degrees(), self.south().degrees())
    }

    pub fn ne_point (&self)->GeoPoint {
        GeoPoint::from_lon_lat_degrees( self.east().degrees(), self.north().degrees())
    }

    pub fn add_degrees (&self, dw: f64, ds: f64, de: f64, dn: f64)->GeoRect {
        GeoRect( Rect::new( 
            Point::new( self.west().degrees()+dw, self.south().degrees()+ds), 
            Point::new( self.east().degrees()+de, self.north().degrees()+dn)
        ))
    }

    #[inline] pub fn west(&self)->Longitude { Longitude::from_degrees( self.0.min().x )}
    #[inline] pub fn east(&self)->Longitude { Longitude::from_degrees( self.0.max().x )}
    #[inline] pub fn south(&self)->Latitude { Latitude::from_degrees( self.0.min().y )}
    #[inline] pub fn north(&self)->Latitude { Latitude::from_degrees( self.0.max().y )}
}

impl SerializeTrait for GeoRect {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        let mut state = serializer.serialize_struct("GeoRect", 4)?;
        state.serialize_field("west", &self.west())?;
        state.serialize_field("south", &self.south())?;
        state.serialize_field("east", &self.east())?;
        state.serialize_field("north", &self.north())?;
        state.end()
    }
}

impl_deserialize_struct!{ GeoRect::from_wsen(west, south, east, north) }

impl JsonWritable for GeoRect {
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object(|w| {
            w.write_field("west", self.west().degrees());
            w.write_field("south", self.south().degrees());
            w.write_field("east", self.east().degrees());
            w.write_field("north", self.north().degrees());      
        });
    }
}

/* #endregion GeoRect */

/* #region GeoCircle**** ***********************************************************************************************/

#[derive(Debug,Clone, Serialize, Deserialize)]
pub struct GeoCircle {
    lon: Longitude,
    lat: Latitude,
    radius: Length
}

impl GeoCircle {
    pub fn new (lon: Longitude, lat: Latitude, radius: Length)->Self {
        GeoCircle { lon, lat, radius }
    }

    pub fn area (&self) -> Area {
        use std::f64::consts::PI;
        let radius = self.radius.get::<meter>();
        let a = PI * radius * radius;
        Area::new::<square_meter>(a)
    }
}


/* #endregion GeoCircle */


/* #region GeoLineString ***********************************************************************************************/

#[derive(Debug,Clone)]
pub struct GeoLineString(LineString);

impl GeoLineString {
    pub fn from_geo_points( ps: Vec<GeoPoint>) -> Self {
        let coords: Vec<GeoCoord> = ps.iter().map(|p| p.coord()).collect();
        GeoLineString( LineString::new(coords))
    }

    pub fn as_geo_points (&self)->Vec<GeoPoint> {
        self.0.points().map(|p| GeoPoint::from_point(p)).collect() // the inverse
    }

    pub fn coords_count(&self)->usize { self.0.coords_count() }
}

impl SerializeTrait for GeoLineString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        let mut state = serializer.serialize_struct("GeoLineString", 1)?;
        state.serialize_field("points", &self.as_geo_points())?;
        state.end()
    }
}
impl_deserialize_struct!{ GeoLineString::from_geo_points( points) }


/* #endregion GeoLineString */


/* #region GeoPolygon **********************************************************************************************/

/// note this is closed (i.e. first and last vertex have to be the same)
#[derive(Debug,Clone)]
pub struct GeoPolygon(Polygon);

impl GeoPolygon {
    pub fn from_geo_points (external: Vec<GeoPoint>, internals: Vec<Vec<GeoPoint>>) -> Self {
        let ext_coords: Vec<GeoCoord> = external.iter().map(|p| p.coord()).collect();
        let exterior = LineString::new( ext_coords);

        let mut interiors: Vec<LineString> = Vec::with_capacity(internals.len());
        for ps in internals.iter() {
            let coords: Vec<GeoCoord>  = ps.iter().map( |p| p.coord()).collect();
            interiors.push( LineString::new( coords));
        }

        GeoPolygon( Polygon::new( exterior, interiors) )
    }

    pub fn from_exterior_geo_points( external: Vec<GeoPoint>) -> Self {
        let ext_coords: Vec<GeoCoord> = external.iter().map(|p| p.coord()).collect();
        let exterior = LineString::new( ext_coords);

        GeoPolygon( Polygon::new( exterior, Vec::with_capacity(0)))
    }

    //... and more ctors to follow

    pub fn as_exterior_geo_points (&self)->Vec<GeoPoint> {
        self.0.exterior().points().map(|p| GeoPoint::from_point(p)).collect() // the inverse
    }

    pub fn as_interior_geo_points (&self)->Vec<Vec<GeoPoint>> {
        self.0.interiors().iter().map( |ls| ls.coords().map(|p| GeoPoint( Point(p.clone()))).collect()).collect()
    }

    pub fn exterior_coords_count(&self)->usize { self.0.exterior().coords_count() }

    pub fn has_interiors(&self)->bool { self.0.interiors().len() > 0 }

    pub fn contains (&self, p: &GeoPoint)->bool { self.0.contains( &p.0) }

    /// low level point iterator in case we have a large number of vertices to process - use with care
    pub fn points_iter (&self)->PointsIter<'_,f64> {
        self.0.exterior().points()
    }

    pub fn bounds (&self)->GeoRect {
        let mut west: f64 = f64::MAX;
        let mut south: f64 = f64::MAX;
        let mut east: f64 = f64::MIN;
        let mut north: f64 = f64::MIN;

        for ref p in self.0.exterior().points() {
            if p.0.x < west  { west  = p.0.x }
            if p.0.x > east  { east  = p.0.x }
            if p.0.y < south { south = p.0.y }
            if p.0.y > north { north = p.0.y }
        }

        GeoRect::from_wsen( 
            Longitude::from_radians(west), 
            Latitude::from_radians(south), 
            Longitude::from_radians(east), 
            Latitude::from_radians(north)
        )
    }
}

impl SerializeTrait for GeoPolygon {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        if self.has_interiors() {
            let mut state = serializer.serialize_struct("GeoPolygon", 2)?;
            state.serialize_field("exterior", &self.as_exterior_geo_points())?;
            state.serialize_field("interiors", &self.as_interior_geo_points())?;
            state.end()
        } else {
            let mut state = serializer.serialize_struct("GeoPolygon", 1)?;
            state.serialize_field("exterior", &self.as_exterior_geo_points())?;
            state.end()
        }
    }
}

impl_deserialize_struct!{ GeoPolygon::from_geo_points(exterior,interiors = Vec::new()) }

/* #endregion GeoPolygon */


//--- specialized types that don't map to GeoJSON

/* #region GeoPoint3 ***********************************************************************************************/

/// 3 dimensional point given by longitude, latitude and altitude above ellipsoid surface
/// note this is not supported bu the `geo` crate and hence we need a different representation. Since
/// this type is probably used in computations involving ECEF transformation we chose the `nav_types`
/// implementation as the underlying basis
#[derive(Debug,Clone,Copy,PartialEq)]
pub struct GeoPoint3 {
    point: Point,
    alt: f64
}

impl GeoPoint3 {
    pub fn from_lon_lat_alt(lon: Longitude, lat: Latitude, alt: Length) -> Self {
        GeoPoint3 {
            point: Point::new( lon.degrees(), lat.degrees()),
            alt: alt.get::<meter>()
        }
    }

    pub fn from_lon_lat_degrees_alt_meters (lon: f64, lat: f64, alt: f64) -> Self {
        GeoPoint3 {
            point: Point::new( lon, lat),
            alt
        }
    }

    #[inline] pub fn longitude(&self) -> Longitude { Longitude::from_degrees( self.point.x()) }
    #[inline] pub fn latitude(&self) -> Latitude { Latitude::from_degrees( self.point.y()) }
    #[inline] pub fn altitude(&self) -> Length { Length::new::<meter>(self.alt) }

    #[inline] pub fn longitude_degrees(&self) -> f64 { self.point.x() }
    #[inline] pub fn latitude_degrees(&self) -> f64 { self.point.y() }
    #[inline] pub fn altitude_meters(&self) -> f64 { self.alt }

    pub fn set_altitude_meters (&mut self, alt: f64) { self.alt = alt; }
    pub fn set_altitude (&mut self, alt: Length) { self.alt = alt.get::<meter>(); }

    #[inline] pub fn to_cartographic (&self)->Cartographic {
        Cartographic::from_degrees( self.longitude_degrees(), self.latitude_degrees(), self.altitude_meters())
    }

    #[inline] pub fn to_cartesian3 (&self)->Cartesian3 { Cartesian3::from( self.to_cartographic()) }
    
    pub fn bearing_from (&self, prev: &GeoPoint3)->Angle360 {
        let cp1 = prev.to_cartographic();
        let cp2 = self.to_cartographic();
        Angle360::from_radians( cp2.bearing_from(&cp1))
    }
}

impl fmt::Display for GeoPoint3 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:.5},{:.5},{:.0}m]", self.longitude_degrees(),self.latitude_degrees(), self.altitude_meters())
    }
}

// nav_types conversions

impl From<ECEF<f64>> for GeoPoint3 {
    fn from (ecef: ECEF<f64>) -> Self { 
        let wgs84: WGS84<f64> = ecef.into(); 
        GeoPoint3 {
            point: Point::new( wgs84.longitude_degrees(), wgs84.latitude_degrees()),
            alt: wgs84.altitude()
        }
    }
}

impl From<WGS84<f64>> for GeoPoint3 {
    fn from (wgs84: WGS84<f64>) -> Self {
        GeoPoint3 {
            point: Point::new( wgs84.longitude_degrees(), wgs84.latitude_degrees()),
            alt: wgs84.altitude()
        }
    }
}

impl SerializeTrait for GeoPoint3 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        let mut state = serializer.serialize_struct("GeoPoint3", 3)?;
        state.serialize_field("lon", &self.longitude_degrees())?;
        state.serialize_field("lat", &self.latitude_degrees())?;
        state.serialize_field("alt", &self.altitude_meters())?;
        state.end()
    }
}

impl_deserialize_struct!{ GeoPoint3::from_lon_lat_degrees_alt_meters(
    lon | longitude | x,
    lat | latitude | y,
    alt | altitude | z
)}

/* #endregion GeoPoint3 */

/* #region DatedGeoPoint3 ***********************************************************************************************/

#[derive(Debug,Clone,Copy,PartialEq)]
pub struct GeoPoint4 {
    pub location: GeoPoint3,
    pub date: EpochMillis  // msec is enough precision and saves us 4 bytes. More importantly it makes GeoPoint4 arrays dense
}

impl GeoPoint4 {
    pub fn from_geo3_epoch (loc: GeoPoint3, date: EpochMillis)-> Self {
        GeoPoint4{ location: loc, date }
    }

    pub fn from_lon_lat_alt_epoch (lon: Longitude, lat: Latitude, alt: Length, date: EpochMillis)->Self {
        GeoPoint4{ location: GeoPoint3::from_lon_lat_alt(lon, lat, alt), date }
    }

    pub fn from_lon_lat_degrees_alt_meters_epoch_millis (lon_deg: f64, lat_deg: f64, alt_m: f64, epoch_millis: i64) -> Self {
        GeoPoint4{ location: GeoPoint3::from_lon_lat_degrees_alt_meters( lon_deg, lat_deg, alt_m), date: EpochMillis::new(epoch_millis) }
    } 

    #[inline] pub fn longitude(&self) -> Longitude { Longitude::from_degrees( self.location.longitude_degrees()) }
    #[inline] pub fn latitude(&self) -> Latitude { Latitude::from_degrees( self.location.latitude_degrees()) }
    #[inline] pub fn altitude(&self) -> Length { self.location.altitude() }
    #[inline] pub fn epoch_millis(&self) ->EpochMillis { self.date }
}

impl Dated for GeoPoint4 {
    fn date (&self)->DateTime<Utc> { self.date.into() }
}


impl SerializeTrait for GeoPoint4 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        let mut state = serializer.serialize_struct("GeoPoint4", 4)?;
        state.serialize_field( "lon", &self.location.longitude_degrees())?;
        state.serialize_field( "lat", &self.location.latitude_degrees())?;
        state.serialize_field( "alt", &self.location.altitude())?;
        state.serialize_field( "date", &self.date)?;
        state.end()
    }
}

impl_deserialize_struct!{ GeoPoint4::from_lon_lat_degrees_alt_meters_epoch_millis(
    lon | longitude | x,
    lat | latitude | y,
    alt | altitude | z,
    date | time | t
)}

/* #endregion GeoPoint4 */

/* #region GeoLineString4 ************************************************************************************************/

/// this can serve as a simple trajectory but is not the most space efficient way to store or serialize
#[derive(Debug,Clone)]
pub struct GeoLineString4(Vec<GeoPoint4>);

impl GeoLineString4 {
    pub fn from_geo_points4( ps: Vec<GeoPoint4>) -> Self {
        GeoLineString4( ps)
    }

    pub fn as_geo_points4 (&self)->Vec<GeoPoint4> {
        self.0.clone()
    }

    pub fn coords_count(&self)->usize { self.0.len() }
}

impl SerializeTrait for GeoLineString4 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        let mut state = serializer.serialize_struct("GeoLineString4", 1)?;
        state.serialize_field("points4", &self.0)?;
        state.end()
    }
}
impl_deserialize_struct!{ GeoLineString4::from_geo_points4( points4) }

/* #endregion GeoLineString4 */