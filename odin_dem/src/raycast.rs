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

//! terrain ray casting.
//!
//! The main use is to compute the geo location of image points (e.g. from image segmentation) of cameras with known
//! position and view direction: the ray from the camera position with a given (true north) bearing and pitch is
//! intersected with the terrain. There are two implementations that share the same ray model:
//!
//! - [`DemBlockCache`](crate::blockcache::DemBlockCache) samples the (geographic) source DEM directly, lazily loading
//!   only the source blocks along the ray. This is the default for local DEMs (see [`DemSource::ray_intersections`])
//!   since it uses the native DEM resolution and avoids building large grids that are mostly unused
//! - [`DemGrid`] is an in-memory UTM grid around a position that can be obtained from any `DemSource` (including
//!   remote `serve_dem` servers that do not support ray queries)
//!
//! Ray model:
//!
//! - the ray height at horizontal distance `d` is `h0 + d·tan(pitch) - d²·k/(2R)` with `R` being the earth radius at
//!   the origin latitude and `k` the curvature coefficient (`1 - refraction`, see [`DEFAULT_CURV_COEFF`]), which is the
//!   same model as used by the GDAL viewshed computation
//! - the ray is sampled in steps of half the DEM cell size, terrain heights are bilinearly interpolated. Once the ray
//!   is at or below the terrain the exact intersection is computed by bisection between the last two samples
//!
//! Note that heights are in the vertical datum of the DEM, which is normally orthometric (MSL), i.e. the ray origin
//! height has to use the same datum (e.g. the DEM height at the camera position plus the camera height above ground -
//! see [`RayQuery::above_ground`]).

use std::path::Path;
use serde::{Serialize, Deserialize};
use odin_common::{
    BoundingBox,
    cartographic::earth_radius_at_geodetic_latitude,
    geo::GeoPoint,
    utm::{UtmPoint, UtmZone, geo_to_utm, geo_to_utm_zone, utm_to_geo}
};
use odin_gdal::{Dataset, Buffer};
use crate::{DemImgType, DemSRS, DemSource, ResampleAlg, Result, dem_cache_dir, errors::op_failed, get_res_dem_filename};

/// GDAL default for the curvature coefficient (`1 - refraction`, with a standard refraction coefficient of 1/7).
/// 1.0 means no refraction, 0.0 no curvature correction. This is the same value used for viewshed computation
pub use odin_gdal::viewshed::DEFAULT_CURV_COEFF;

/// the height we use for nodata cells (normally open sea)
pub const NO_DATA_HEIGHT: f64 = 0.0;

/// number of bisection steps to refine terrain intersections (resolution is step / 2^n)
const N_BISECT: usize = 24;

fn default_curv_coeff ()->f64 { DEFAULT_CURV_COEFF }

/// a ray to intersect with the terrain
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct RayQuery {
    pub lon: f64,          // ray origin longitude [deg]
    pub lat: f64,          // ray origin latitude [deg]
    pub height: f64,       // ray origin height [m] - above ground if `above_ground` is set, otherwise in DEM vertical datum
    #[serde(default)]
    pub above_ground: bool,
    pub bearing: f64,      // [deg] clockwise from true north
    pub pitch: f64,        // [deg] negative is downwards
    pub max_dist: f64,     // max horizontal distance [m]
    #[serde(default="default_curv_coeff")]
    pub curv_coeff: f64    // earth curvature coefficient (1 - refraction)
}

impl RayQuery {
    /// a ray from a position that is `height_above_ground` \[m\] above the terrain, using the default curvature coefficient
    pub fn from_ground (lon: f64, lat: f64, height_above_ground: f64, bearing: f64, pitch: f64, max_dist: f64)->Self {
        RayQuery { lon, lat, height: height_above_ground, above_ground: true, bearing, pitch, max_dist, curv_coeff: DEFAULT_CURV_COEFF }
    }

    pub fn origin (&self)->GeoPoint { GeoPoint::from_lon_lat_degrees( self.lon, self.lat) }
}

/// a terrain intersection
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct RayHit {
    pub lon: f64,           // [deg]
    pub lat: f64,           // [deg]
    pub height: f64,        // terrain height at the intersection [m] (in DEM vertical datum)
    pub distance: f64,      // horizontal distance from the ray origin [m]
}

impl RayHit {
    pub fn position (&self)->GeoPoint { GeoPoint::from_lon_lat_degrees( self.lon, self.lat) }
}

/// the ray height function for a given origin height, pitch and curvature
pub(crate) fn ray_height_fn (h0: f64, pitch: f64, curv_coeff: f64, origin_lat: f64)->impl Fn(f64)->f64 {
    let tan_p = pitch.to_radians().tan();
    let kc = curv_coeff / (2.0 * earth_radius_at_geodetic_latitude( origin_lat.to_radians()));
    move |d| h0 + d * tan_p - d * d * kc
}

/// the generic ray march: sample `clearance(d)` (ray height above terrain at horizontal distance d, None if outside of
/// DEM) in `step` increments until it is <= 0 or we reach `max_dist`, and then refine by bisection.
/// Returns the distance of the intersection
pub(crate) fn march (step: f64, max_dist: f64, mut clearance: impl FnMut(f64)->Result<Option<f64>>)->Result<Option<f64>> {
    let mut d_prev = 0.0;
    let mut d = step;

    while d_prev < max_dist {
        let d_cur = d.min( max_dist);
        let Some(c) = clearance( d_cur)? else { return Ok(None) }; // left the DEM

        if c <= 0.0 { // terrain hit between d_prev and d_cur - refine
            let (mut lo, mut hi) = (d_prev, d_cur);
            for _ in 0..N_BISECT {
                let mid = 0.5 * (lo + hi);
                match clearance( mid)? {
                    Some(c) if c > 0.0 => lo = mid,
                    _ => hi = mid
                }
            }
            return Ok( Some(hi))
        }

        d_prev = d_cur;
        d += step;
    }
    Ok(None)
}

/// bilinear interpolation of fractional cell center coordinates (px,py) for a grid of size (w,h) with a
/// fallible cell accessor. Coordinates are clamped to cell centers so that we also get values for outer half cells
pub(crate) fn bilinear (px: f64, py: f64, w: usize, h: usize, mut cell: impl FnMut(usize,usize)->Result<f64>)->Result<f64> {
    let px = px.clamp( 0.0, (w - 1) as f64);
    let py = py.clamp( 0.0, (h - 1) as f64);
    let ix = (px.floor() as usize).min( w - 2);
    let iy = (py.floor() as usize).min( h - 2);
    let tx = px - ix as f64;
    let ty = py - iy as f64;

    let h00 = cell( ix, iy)?;
    let h10 = cell( ix+1, iy)?;
    let h01 = cell( ix, iy+1)?;
    let h11 = cell( ix+1, iy+1)?;

    Ok( (h00 * (1.0 - tx) + h10 * tx) * (1.0 - ty) + (h01 * (1.0 - tx) + h11 * tx) * ty )
}

/* #region DemGrid **************************************************************************************************/

/// an in-memory DEM in a UTM projection, with (north-up) cells of constant size
pub struct DemGrid {
    data: Vec<f32>,       // row major, row 0 is the north-most row
    width: usize,
    height: usize,
    x0: f64,              // west edge of grid [m]
    y0: f64,              // north edge of grid [m]
    dx: f64,              // cell width [m]
    dy: f64,              // cell height [m] (positive)
    no_data: Option<f32>,
    utm_zone: UtmZone
}

impl DemGrid {
    /// create grid from row major data (row 0 = north-most row). `(x0,y0)` is the north west corner of the grid
    pub fn new (data: Vec<f32>, width: usize, height: usize, x0: f64, y0: f64, dx: f64, dy: f64, no_data: Option<f32>, utm_zone: UtmZone)->Result<Self> {
        if data.len() != width * height { return Err( op_failed( format!("grid data length {} does not match {width}x{height}", data.len()))) }
        if width < 2 || height < 2 { return Err( op_failed("grid has to have at least 2x2 cells")) }
        if !(dx > 0.0 && dy > 0.0) { return Err( op_failed("grid cell size has to be positive")) }
        Ok( DemGrid { data, width, height, x0, y0, dx, dy: dy.abs(), no_data, utm_zone } )
    }

    /// read band 1 of a (north-up) UTM dataset into memory
    pub fn from_dataset (ds: &Dataset)->Result<Self> {
        let srs = ds.spatial_ref()?;
        let epsg = srs.auth_code().map_err( |_| op_failed("DEM dataset has no EPSG code"))? as u32;
        let utm_zone = match epsg {
            32601..=32660 => UtmZone { zone: epsg - 32600, band: 'N' },
            32701..=32760 => UtmZone { zone: epsg - 32700, band: 'M' },
            _ => return Err( op_failed( format!("DEM dataset is not in a UTM projection (EPSG {epsg})")))
        };

        let gt = ds.geo_transform()?;
        if gt[2] != 0.0 || gt[4] != 0.0 || gt[5] >= 0.0 { return Err( op_failed("DEM dataset is not north-up")) }

        let band = ds.rasterband(1)?;
        let (width, height) = ds.raster_size();
        let no_data = band.no_data_value().map( |v| v as f32);
        let buf: Buffer<f32> = band.read_as( (0,0), (width,height), (width,height), None)?;
        let (_, data) = buf.into_shape_and_vec();

        DemGrid::new( data, width, height, gt[0], gt[3], gt[1], -gt[5], no_data, utm_zone)
    }

    pub fn from_file (path: impl AsRef<Path>)->Result<Self> {
        let ds = Dataset::open( path.as_ref())?;
        DemGrid::from_dataset( &ds)
    }

    /// get a DEM grid with resolution `res` \[m\] that covers `radius` \[m\] around `center` from the given `DemSource`.
    /// The UTM zone is the one of `center`. The underlying DEM file is cached by odin_dem (in its cache dir)
    pub async fn around (src: &DemSource, center: &GeoPoint, radius: f64, res: f64, alg: ResampleAlg)->Result<Self> {
        let utm = geo_to_utm( center).ok_or( op_failed( format!("position {center:?} cannot be mapped to UTM")))?;
        let epsg = utm.utm_zone.epsg();
        let srs = DemSRS::from_epsg( epsg).ok_or( op_failed( format!("unsupported UTM zone {epsg}")))?;

        // round to grid resolution so that close positions can share the cached file
        let x = (utm.easting / res).round() * res;
        let y = (utm.northing / res).round() * res;
        let d = ((radius / res).ceil() + 2.0) * res;
        let bbox = BoundingBox::new( x - d, y - d, x + d, y + d);

        let path = dem_cache_dir().join( get_res_dem_filename( &format!("grid-{}", src.cache_tag()), epsg, &bbox, res, res, alg, "tif"));
        src.get_res_dem( &bbox, srs, res, res, alg, DemImgType::TIF, &path).await?;

        DemGrid::from_file( &path)
    }

    pub fn utm_zone (&self)->UtmZone { self.utm_zone }
    pub fn dimensions (&self)->(usize,usize) { (self.width, self.height) }
    pub fn cell_size (&self)->(f64,f64) { (self.dx, self.dy) }

    /// the grid extent (west,south,east,north) in UTM meters
    pub fn bbox (&self)->BoundingBox<f64> {
        BoundingBox::new( self.x0, self.y0 - self.height as f64 * self.dy, self.x0 + self.width as f64 * self.dx, self.y0)
    }

    #[inline]
    fn cell (&self, ix: usize, iy: usize)->f64 {
        let v = self.data[iy * self.width + ix];
        if Some(v) == self.no_data || v.is_nan() { NO_DATA_HEIGHT } else { v as f64 }
    }

    /// bilinearly interpolated terrain height at UTM grid coordinates (cell values are taken at cell centers).
    /// Returns None if the position is outside of the grid
    pub fn height_at (&self, x: f64, y: f64)->Option<f64> {
        let fx = (x - self.x0) / self.dx;
        let fy = (self.y0 - y) / self.dy;
        if fx < 0.0 || fy < 0.0 || fx > self.width as f64 || fy > self.height as f64 { return None }
        bilinear( fx - 0.5, fy - 0.5, self.width, self.height, |ix,iy| Ok( self.cell(ix,iy))).ok()
    }

    /// terrain height at a geographic position (None if outside of the grid)
    pub fn height_at_geo (&self, pos: &GeoPoint)->Option<f64> {
        let utm = geo_to_utm_zone( pos, self.utm_zone)?;
        self.height_at( utm.easting, utm.northing)
    }

    /// the grid bearing \[deg\] of true north at the given position, i.e. the grid bearing of a true bearing `b` is
    /// `b + grid_convergence(pos)`. Note this is the negative of the conventional grid convergence angle (which is the
    /// true bearing of grid north)
    pub fn grid_convergence (&self, pos: &GeoPoint)->Option<f64> {
        let lat = pos.latitude_degrees();
        let dlat = if lat < 89.0 { 0.01 } else { -0.01 }; // ~1.1km
        let p0 = geo_to_utm_zone( pos, self.utm_zone)?;
        let p1 = geo_to_utm_zone( &GeoPoint::from_lon_lat_degrees( pos.longitude_degrees(), lat + dlat), self.utm_zone)?;
        let (de, dn) = if dlat > 0.0 { (p1.easting - p0.easting, p1.northing - p0.northing) } else { (p0.easting - p1.easting, p0.northing - p1.northing) };
        Some( de.atan2( dn).to_degrees())
    }

    /// find the first intersection of a ray with the terrain. The ray starts at `origin` with height `h0` \[m\] (in the
    /// vertical datum of the DEM) and has the given `bearing` (deg clockwise from true north) and `pitch` (deg, negative
    /// is downwards). Returns `Ok(None)` if the ray does not hit the terrain within `max_dist` \[m\] (horizontal) or leaves
    /// the grid before, and an error if the origin is outside of the grid or below the terrain.
    /// The bearing is converted into a UTM grid bearing by correcting for the grid convergence at the ray origin.
    /// UTM scale distortions (< 0.1%) are ignored - distances are grid distances
    pub fn ray_intersection (&self, origin: &GeoPoint, h0: f64, bearing: f64, pitch: f64, max_dist: f64, curv_coeff: f64)->Result<Option<RayHit>> {
        let o = geo_to_utm_zone( origin, self.utm_zone).ok_or( op_failed( format!("ray origin {origin:?} cannot be mapped to UTM zone {}", self.utm_zone)))?;
        let t0 = self.height_at( o.easting, o.northing).ok_or( op_failed( "ray origin outside of DEM grid"))?;
        if h0 < t0 { return Err( op_failed( format!("ray origin height {h0:.1}m is below terrain ({t0:.1}m)"))) }

        let gamma = self.grid_convergence( origin).unwrap_or(0.0);
        let gb = (bearing + gamma).to_radians(); // grid bearing
        let (ux, uy) = (gb.sin(), gb.cos());
        let ray_height = ray_height_fn( h0, pitch, curv_coeff, origin.latitude_degrees());

        let step = 0.5 * self.dx.min( self.dy);
        let dist = march( step, max_dist, |d| {
            Ok( self.height_at( o.easting + d * ux, o.northing + d * uy).map( |t| ray_height(d) - t))
        })?;

        Ok( dist.map( |d| {
            let (x, y) = (o.easting + d * ux, o.northing + d * uy);
            let pos = utm_to_geo( &UtmPoint { easting: x, northing: y, utm_zone: self.utm_zone });
            RayHit { lon: pos.longitude_degrees(), lat: pos.latitude_degrees(), height: self.height_at( x, y).unwrap_or( NO_DATA_HEIGHT), distance: d }
        }))
    }

    /// execute a `RayQuery` on this grid
    pub fn query (&self, q: &RayQuery)->Result<Option<RayHit>> {
        let origin = q.origin();
        let h0 = if q.above_ground {
            self.height_at_geo( &origin).ok_or( op_failed( "ray origin outside of DEM grid"))? + q.height
        } else { q.height };
        self.ray_intersection( &origin, h0, q.bearing, q.pitch, q.max_dist, q.curv_coeff)
    }
}

/* #endregion DemGrid */
