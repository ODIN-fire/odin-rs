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

//! terrain ray casting directly on (geographic) source DEMs, with a lazily populated and size bounded block cache.
//!
//! Instead of building (warped) grids around a ray origin we sample the source DEM (e.g. a VRT of 3DEP tiles) in its
//! native resolution and SRS, loading only the source blocks the ray passes through until it hits the terrain. Loaded
//! blocks are kept in a LRU cache so that repeated queries for the same area (e.g. different image points of the same
//! camera) do not have to read them again.
//!
//! The horizontal ray track is the geodesic from the ray origin with the given bearing. We compute geodesic positions
//! every [`NODE_DIST`] meters and linearly interpolate (lon,lat) in between - the deviation from the geodesic is
//! about `L²·tan(lat)/(8R)`, which is < 0.1m for 2km nodes at mid latitudes.
//!
//! A `DemBlockCache` is not `Sync` (GDAL datasets cannot be shared between threads). Use [`get_ray_intersections`] to
//! access a process wide (mutex protected) cache per DEM file.

use std::{collections::HashMap, path::{Path, PathBuf}, sync::{LazyLock, Mutex}};
use geo::{Destination, Geodesic, Point};
use odin_common::geo::GeoPoint;
use odin_gdal::{Dataset, Buffer, RasterBand};
use crate::{
    Result, errors::op_failed,
    raycast::{RayHit, RayQuery, NO_DATA_HEIGHT, march, ray_height_fn}
};

/// default max number of cached DEM blocks (for 128x128 blocks this is 64MB)
pub const DEFAULT_MAX_BLOCKS: usize = 1024;

/// distance \[m\] between geodesic nodes of the ray track
pub const NODE_DIST: f64 = 2000.0;

/// approximate meters per degree latitude (only used to compute sample steps)
const M_PER_DEG: f64 = 111_320.0;

struct Block {
    key: (usize,usize),    // block column/row
    data: Vec<f32>,
    width: usize,
    last_use: u64
}

/// a lazily loaded block cache for a north-up geographic DEM dataset
pub struct DemBlockCache {
    ds: Dataset,
    width: usize,          // raster size in pixels
    height: usize,
    x0: f64,               // west edge [deg]
    y0: f64,               // north edge [deg]
    dx: f64,               // pixel width [deg]
    dy: f64,               // pixel height [deg] (positive)
    bw: usize,             // block width/height in pixels
    bh: usize,
    no_data: Option<f64>,

    blocks: Vec<Block>,                  // slots - evicted blocks are replaced in place
    index: HashMap<(usize,usize),usize>, // block key -> slot
    last: Option<((usize,usize),usize)>, // last used (key,slot) - consecutive ray samples mostly hit the same block
    max_blocks: usize,
    clock: u64,
    n_loaded: usize,       // statistics: number of block reads
}

impl DemBlockCache {
    pub fn open (path: impl AsRef<Path>, max_blocks: usize)->Result<Self> {
        let path = path.as_ref();
        let ds = Dataset::open( path)?;
        let srs = ds.spatial_ref()?;
        if !srs.is_geographic() { return Err( op_failed( format!("DEM {path:?} does not use a geographic SRS"))) }

        let gt = ds.geo_transform()?;
        if gt[2] != 0.0 || gt[4] != 0.0 || gt[5] >= 0.0 { return Err( op_failed( format!("DEM {path:?} is not north-up"))) }

        let (width, height) = ds.raster_size();
        if width < 2 || height < 2 { return Err( op_failed( format!("DEM {path:?} is too small"))) }

        let band = ds.rasterband(1)?;
        let (bw, bh) = band.block_size();
        // datasets that are stored in rows (strips) have too small blocks - we read at least 256 rows/columns
        let (bw, bh) = (bw.clamp( 64, 512), bh.clamp( 64, 512));
        let no_data = band.no_data_value();

        Ok( DemBlockCache {
            ds, width, height, x0: gt[0], y0: gt[3], dx: gt[1], dy: -gt[5], bw, bh, no_data,
            blocks: Vec::new(), index: HashMap::new(), last: None, max_blocks: max_blocks.max(4), clock: 0, n_loaded: 0
        })
    }

    pub fn n_cached_blocks (&self)->usize { self.blocks.len() }
    pub fn n_loaded_blocks (&self)->usize { self.n_loaded }
    pub fn pixel_size (&self)->(f64,f64) { (self.dx, self.dy) }
    pub fn block_size (&self)->(usize,usize) { (self.bw, self.bh) }

    #[inline]
    fn value (&self, v: f32)->f64 {
        let v = v as f64;
        if Some(v) == self.no_data || v.is_nan() { NO_DATA_HEIGHT } else { v }
    }

    /// get the block for the given key, loading it if required
    fn block (&mut self, key: (usize,usize))->Result<&Block> {
        self.clock += 1;
        let slot = match self.last {
            Some((k,slot)) if k == key => slot,
            _ => {
                let slot = match self.index.get( &key) {
                    Some(slot) => *slot,
                    None => self.load_block( key)?
                };
                self.last = Some((key,slot));
                slot
            }
        };
        let block = &mut self.blocks[slot];
        block.last_use = self.clock;
        Ok(block)
    }

    /// value of pixel (ix,iy), loading its block if required
    fn pixel (&mut self, ix: usize, iy: usize)->Result<f64> {
        let (bw, bh) = (self.bw, self.bh);
        let block = self.block( (ix / bw, iy / bh))?;
        let v = block.data[ (iy % bh) * block.width + (ix % bw)];
        Ok( self.value(v))
    }

    /// the 4 pixel values (ix,iy),(ix+1,iy),(ix,iy+1),(ix+1,iy+1). Since this is the hot path of ray marching we
    /// avoid separate block lookups if all are within the same block (which is the normal case)
    fn quad (&mut self, ix: usize, iy: usize)->Result<[f64;4]> {
        let (bw, bh) = (self.bw, self.bh);
        let (lx, ly) = (ix % bw, iy % bh);

        if lx + 1 < bw && ly + 1 < bh {
            let block = self.block( (ix / bw, iy / bh))?;
            if lx + 1 < block.width && (ly + 1) * block.width + lx + 1 < block.data.len() {
                let w = block.width;
                let i = ly * w + lx;
                let (a, b, c, d) = (block.data[i], block.data[i+1], block.data[i+w], block.data[i+w+1]);
                return Ok( [self.value(a), self.value(b), self.value(c), self.value(d)])
            }
        }
        Ok( [self.pixel( ix, iy)?, self.pixel( ix+1, iy)?, self.pixel( ix, iy+1)?, self.pixel( ix+1, iy+1)?])
    }

    /// load block and return its slot
    fn load_block (&mut self, key: (usize,usize))->Result<usize> {
        let x = key.0 * self.bw;
        let y = key.1 * self.bh;
        let w = self.bw.min( self.width - x);
        let h = self.bh.min( self.height - y);

        let band = self.ds.rasterband(1)?;
        let buf: Buffer<f32> = band.read_as( (x as isize, y as isize), (w,h), (w,h), None)?;
        let (_, data) = buf.into_shape_and_vec();

        let block = Block { key, data, width: w, last_use: self.clock };
        self.n_loaded += 1;

        let slot = if self.blocks.len() < self.max_blocks {
            self.blocks.push( block);
            self.blocks.len() - 1
        } else { // replace least recently used
            let slot = self.blocks.iter().enumerate().min_by_key( |(_,b)| b.last_use).map( |(i,_)| i).unwrap();
            self.index.remove( &self.blocks[slot].key);
            if matches!( self.last, Some((_,s)) if s == slot) { self.last = None; }
            self.blocks[slot] = block;
            slot
        };
        self.index.insert( key, slot);
        Ok(slot)
    }

    /// bilinearly interpolated terrain height at a geographic position (None if outside of the DEM)
    pub fn height_at (&mut self, lon: f64, lat: f64)->Result<Option<f64>> {
        let fx = (lon - self.x0) / self.dx;
        let fy = (self.y0 - lat) / self.dy;
        if fx < 0.0 || fy < 0.0 || fx > self.width as f64 || fy > self.height as f64 { return Ok(None) }

        // see raycast::bilinear - clamp to cell centers so that we also get values for the outer half cells
        let px = (fx - 0.5).clamp( 0.0, (self.width - 1) as f64);
        let py = (fy - 0.5).clamp( 0.0, (self.height - 1) as f64);
        let ix = (px.floor() as usize).min( self.width - 2);
        let iy = (py.floor() as usize).min( self.height - 2);
        let tx = px - ix as f64;
        let ty = py - iy as f64;

        let [h00, h10, h01, h11] = self.quad( ix, iy)?;
        Ok( Some( (h00 * (1.0 - tx) + h10 * tx) * (1.0 - ty) + (h01 * (1.0 - tx) + h11 * tx) * ty ))
    }

    /// find the first intersection of a ray with the terrain. The ray starts at `origin` with height `h0` \[m\] (in the
    /// vertical datum of the DEM) and has the given `bearing` (deg clockwise from true north) and `pitch` (deg, negative
    /// is downwards). Returns `Ok(None)` if the ray does not hit the terrain within `max_dist` \[m\] (horizontal) or leaves
    /// the DEM before, and an error if the origin is outside of the DEM or below the terrain
    pub fn ray_intersection (&mut self, origin: &GeoPoint, h0: f64, bearing: f64, pitch: f64, max_dist: f64, curv_coeff: f64)->Result<Option<RayHit>> {
        let (lon0, lat0) = (origin.longitude_degrees(), origin.latitude_degrees());
        let t0 = self.height_at( lon0, lat0)?.ok_or( op_failed( "ray origin outside of DEM"))?;
        if h0 < t0 { return Err( op_failed( format!("ray origin height {h0:.1}m is below terrain ({t0:.1}m)"))) }

        let ray_height = ray_height_fn( h0, pitch, curv_coeff, lat0);
        let step = 0.5 * (self.dy * M_PER_DEG).min( self.dx * M_PER_DEG * lat0.to_radians().cos());
        let mut track = GeodesicTrack::new( lon0, lat0, bearing);

        let dist = march( step, max_dist, |d| {
            let (lon, lat) = track.position( d);
            Ok( self.height_at( lon, lat)?.map( |t| ray_height(d) - t))
        })?;

        match dist {
            Some(d) => {
                let (lon, lat) = track.position( d);
                let height = self.height_at( lon, lat)?.unwrap_or( NO_DATA_HEIGHT);
                Ok( Some( RayHit { lon, lat, height, distance: d }))
            }
            None => Ok(None)
        }
    }

    /// execute a `RayQuery` on this DEM
    pub fn query (&mut self, q: &RayQuery)->Result<Option<RayHit>> {
        let h0 = if q.above_ground {
            self.height_at( q.lon, q.lat)?.ok_or( op_failed( "ray origin outside of DEM"))? + q.height
        } else { q.height };
        self.ray_intersection( &q.origin(), h0, q.bearing, q.pitch, q.max_dist, q.curv_coeff)
    }
}

/// geodesic ray track positions, computed at node distances and linearly interpolated in between
struct GeodesicTrack {
    origin: Point<f64>,
    bearing: f64,
    nodes: Vec<(f64,f64)> // (lon,lat) at i * NODE_DIST
}

impl GeodesicTrack {
    fn new (lon: f64, lat: f64, bearing: f64)->Self {
        GeodesicTrack { origin: Point::new( lon, lat), bearing, nodes: vec![ (lon,lat) ] }
    }

    fn node (&mut self, i: usize)->(f64,f64) {
        while self.nodes.len() <= i {
            let p = Geodesic.destination( self.origin, self.bearing, self.nodes.len() as f64 * NODE_DIST);
            self.nodes.push( (p.x(), p.y()));
        }
        self.nodes[i]
    }

    fn position (&mut self, d: f64)->(f64,f64) {
        let i = (d / NODE_DIST).floor() as usize;
        let t = d / NODE_DIST - i as f64;
        let (lon0, lat0) = self.node( i);
        let (lon1, lat1) = self.node( i+1);
        (lon0 + t * (lon1 - lon0), lat0 + t * (lat1 - lat0))
    }
}

/* #region process wide block caches *********************************************************************************/

static BLOCK_CACHES: LazyLock<Mutex<HashMap<PathBuf,DemBlockCache>>> = LazyLock::new( || Mutex::new( HashMap::new()));

/// execute ray queries on the process wide block cache for the given (local) DEM file, which is created on demand.
/// Note this is a blocking function (it might read DEM blocks)
pub fn get_ray_intersections (dem_path: impl AsRef<Path>, queries: &[RayQuery])->Result<Vec<Option<RayHit>>> {
    let path = dem_path.as_ref();
    let mut caches = BLOCK_CACHES.lock().map_err( |_| op_failed("DEM block cache poisoned"))?;

    if !caches.contains_key( path) {
        caches.insert( path.to_path_buf(), DemBlockCache::open( path, DEFAULT_MAX_BLOCKS)?);
    }
    let cache = caches.get_mut( path).unwrap();

    queries.iter().map( |q| cache.query( q)).collect()
}

/* #endregion process wide block caches */
