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

//! computation of geo-referenced products from decoded Level II volumes (in WSR-88D format).
//! Products are resampled from the polar sweep data onto a geographic (EPSG:4326) grid that covers the
//! max range of the used sweeps, encoded as 8 bit reflectivity codes (see [`crate::dbz_to_code`]) and stored
//! as tiled GeoTIFFs with a color table (the reflectivity scale of the `nexrad-render` crate) and overviews

use std::{path::{Path,PathBuf}, thread, sync::atomic::{AtomicU8, Ordering}};
use chrono::{DateTime, Utc};
use gdal::{Dataset, DriverManager, raster::{Buffer, ColorEntry, ColorTable, PaletteInterpretation, RasterCreationOptions}, spatial_ref::SpatialRef};
use nexrad::{
    model::{data::{GateStatus, Product, Scan, SweepField}, geo::{GeoExtent, GeoPoint, PolarPoint, RadarCoordinateSystem}, meta::Site},
    render::default_scale
};
use odin_gdal::cmap::CmapEntry;

use crate::{
    NexradEntry, NexradProduct, Station, REF_BELOW_THRESHOLD, REF_RANGE_FOLDED, dbz_to_code, code_to_dbz,
    errors::{Result, op_failed}
};

/// max range \[km\] of NEXRAD reflectivity sweeps (used for station extents before we have data)
pub const MAX_RANGE_KM: f64 = 460.0;

/// overview levels of product GeoTIFFs
const OVERVIEWS: [i32;4] = [2, 4, 8, 16];

/// the reflectivity sweep fields used for a product: the first (lowest elevation, longest range) reflectivity sweep for
/// base reflectivity, all reflectivity sweeps for composite reflectivity
pub fn product_fields (scan: &Scan, product: NexradProduct)->Vec<SweepField> {
    match product {
        NexradProduct::BaseReflectivity => scan.first_field( Product::Reflectivity).map( |(_,f)| vec![f]).unwrap_or_default(),
        NexradProduct::CompositeReflectivity => scan.fields( Product::Reflectivity),
    }
}

/// the radar coordinate system of a scan. If the volume does not have site metadata we use the station position
/// (without feedhorn height)
pub fn coordinate_system (scan: &Scan, station: &Station)->Result<RadarCoordinateSystem> {
    if let Some(cs) = scan.coordinate_system() { return Ok(cs) }
    station_coordinate_system( station)
}

pub fn station_coordinate_system (station: &Station)->Result<RadarCoordinateSystem> {
    let id: [u8;4] = station.id.as_bytes().try_into().map_err( |_| op_failed!("invalid station id {}", station.id))?;
    let site = Site::builder( id, station.latitude, station.longitude).build().map_err( |e| op_failed!("invalid site {}: {e}", station.id))?;
    Ok( RadarCoordinateSystem::new( &site))
}

/// geographic extent \[west,south,east,north\] of a station for a max range (e.g. for display before we have products)
pub fn station_extent (station: &Station, max_range_km: f64)->Result<[f64;4]> {
    let e = station_coordinate_system( station)?.sweep_extent( max_range_km);
    Ok( [e.min.longitude, e.min.latitude, e.max.longitude, e.max.latitude] )
}

/// meters per degree latitude (on the spherical earth model of the nexrad crate, radius 6371km)
pub const METERS_PER_DEG_LAT: f64 = 6_371_000.0 * std::f64::consts::PI / 180.0;

/// a geographic (EPSG:4326) product grid (row 0 is the north edge). Cell sizes are in degrees and can differ for
/// latitude and longitude (see [`ProductGrid::for_station`])
#[derive(Debug,Clone,PartialEq)]
pub struct ProductGrid {
    pub west: f64,
    pub north: f64,
    pub res_lon: f64,
    pub res_lat: f64,
    pub width: usize,
    pub height: usize,
}

impl ProductGrid {
    /// grid with `res_lon` x `res_lat` degree cells that covers the given extent (rounded up to whole cells)
    pub fn covering (extent: &GeoExtent, res_lon: f64, res_lat: f64)->Result<Self> {
        if !(res_lon > 0.0 && res_lat > 0.0) { return Err( op_failed!("invalid grid resolution {res_lon},{res_lat}")) }
        let width = ((extent.max.longitude - extent.min.longitude) / res_lon).ceil() as usize;
        let height = ((extent.max.latitude - extent.min.latitude) / res_lat).ceil() as usize;
        if width == 0 || height == 0 { return Err( op_failed!("empty product grid")) }
        Ok( ProductGrid { west: extent.min.longitude, north: extent.max.latitude, res_lon, res_lat, width, height })
    }

    /// grid that covers `extent` with cells of `resolution_m` meters at the latitude of the radar site, i.e. cells are
    /// square at the site latitude (at 460km north/south the longitude cell size is off by about ±6%)
    pub fn for_station (extent: &GeoExtent, site_latitude: f64, resolution_m: f64)->Result<Self> {
        let (res_lon, res_lat) = cell_size_deg( site_latitude, resolution_m);
        Self::covering( extent, res_lon, res_lat)
    }

    pub fn geotransform (&self)->[f64;6] {
        [ self.west, self.res_lon, 0.0, self.north, 0.0, -self.res_lat ]
    }

    /// center of a grid cell
    pub fn cell_center (&self, row: usize, col: usize)->GeoPoint {
        GeoPoint { latitude: self.north - (row as f64 + 0.5) * self.res_lat, longitude: self.west + (col as f64 + 0.5) * self.res_lon }
    }

    /// linear index of the grid cell that contains a position (None if outside of the grid)
    pub fn cell_index (&self, latitude: f64, longitude: f64)->Option<usize> {
        let col = (longitude - self.west) / self.res_lon;
        let row = (self.north - latitude) / self.res_lat;
        if col >= 0.0 && row >= 0.0 && (col as usize) < self.width && (row as usize) < self.height {
            Some( row as usize * self.width + col as usize)
        } else {
            None
        }
    }
}

/// (longitude, latitude) cell size in degrees for `resolution_m` meters at a given latitude
pub fn cell_size_deg (latitude: f64, resolution_m: f64)->(f64,f64) {
    let res_lat = resolution_m / METERS_PER_DEG_LAT;
    let res_lon = res_lat / latitude.to_radians().cos().max(1e-6);
    (res_lon, res_lat)
}

/// resample sweep fields onto a grid. Each cell gets the max valid reflectivity of all fields within it, or
/// REF_RANGE_FOLDED / REF_BELOW_THRESHOLD if there is no valid value. This is computed in two passes:
///
/// 1. sample all fields at each cell center, so that every cell within range gets a value even if it is smaller than
///    the gates (far range, where the azimuthal gate width exceeds the cell size)
/// 2. map each gate center into its grid cell (keeping the max), so that no gate is lost if gates are smaller than
///    cells (near range). Without this pass the max of composite reflectivity could miss small cores close to the radar
///
/// Both passes are computed in parallel and combined with an (atomic) max of the codes. Since our code order is
/// valid values > REF_RANGE_FOLDED > REF_BELOW_THRESHOLD this also implements the status priority.
/// Gate centers are at `first_gate_range + i * gate_interval` (see `RadarCoordinateSystem::polar_to_geo`), for
/// which we look up the nearest gate in pass 1 (`value_at_polar` floors the range)
pub fn resample_max (fields: &[SweepField], cs: &RadarCoordinateSystem, grid: &ProductGrid)->Vec<u8> {
    let codes: Vec<AtomicU8> = (0..grid.width * grid.height).map( |_| AtomicU8::new( REF_BELOW_THRESHOLD)).collect();
    let n_threads = thread::available_parallelism().map( |n| n.get()).unwrap_or(4).max(1);

    // pass 1: cell centers (row chunks)
    let rows_per_thread = grid.height.div_ceil( n_threads.min( grid.height));
    thread::scope( |scope| {
        for row0 in (0..grid.height).step_by( rows_per_thread) {
            let codes = &codes;
            scope.spawn( move || {
                for row in row0..(row0 + rows_per_thread).min( grid.height) {
                    for col in 0..grid.width {
                        let code = sample_max( fields, cs, grid.cell_center( row, col));
                        if code != REF_BELOW_THRESHOLD { codes[row * grid.width + col].fetch_max( code, Ordering::Relaxed); }
                    }
                }
            });
        }
    });

    // pass 2: gate centers (radial chunks of each field)
    thread::scope( |scope| {
        for field in fields {
            let n_az = field.azimuth_count();
            if n_az == 0 { continue }
            let az_per_thread = n_az.div_ceil( n_threads.min( n_az));
            for az0 in (0..n_az).step_by( az_per_thread) {
                let codes = &codes;
                scope.spawn( move || {
                    for az_idx in az0..(az0 + az_per_thread).min( n_az) {
                        splat_radial( field, az_idx, cs, grid, codes);
                    }
                });
            }
        }
    });

    codes.into_iter().map( |c| c.into_inner()).collect()
}

fn sample_max (fields: &[SweepField], cs: &RadarCoordinateSystem, p: GeoPoint)->u8 {
    let mut max_dbz = f32::MIN;
    let mut is_valid = false;
    let mut is_range_folded = false;

    for field in fields {
        let Some(polar) = cs.geo_to_polar( p, field.elevation_angle_degrees() as f64) else { continue };
        // value_at_polar floors (range - first_gate_range) / gate_interval - shift by half a gate to get the nearest gate center
        if let Some((v, status)) = field.value_at_polar( polar.azimuth_angle_degrees, polar.range_km + field.gate_interval_km() / 2.0) {
            match status {
                GateStatus::Valid => if v > max_dbz { max_dbz = v; is_valid = true; }
                GateStatus::RangeFolded => is_range_folded = true,
                _ => {}
            }
        }
    }

    if is_valid { dbz_to_code( max_dbz) } else if is_range_folded { REF_RANGE_FOLDED } else { REF_BELOW_THRESHOLD }
}

/// map all gates of a radial into the grid cells that contain their centers
fn splat_radial (field: &SweepField, az_idx: usize, cs: &RadarCoordinateSystem, grid: &ProductGrid, codes: &[AtomicU8]) {
    let (Some(values), Some(statuses)) = (field.radial_values( az_idx), field.radial_statuses( az_idx)) else { return };
    let Some(&azimuth) = field.azimuth_angles_degrees().get( az_idx) else { return };
    let elevation = field.elevation_angle_degrees() as f64;
    let r0 = field.first_gate_range_km();
    let dr = field.gate_interval_km();

    for (gate_idx, (v, status)) in values.iter().zip( statuses.iter()).enumerate() {
        let code = match status {
            GateStatus::Valid => dbz_to_code( *v),
            GateStatus::RangeFolded => REF_RANGE_FOLDED,
            _ => continue
        };
        let polar = PolarPoint { azimuth_angle_degrees: azimuth as f64, range_km: r0 + gate_idx as f64 * dr, elevation_angle_degrees: elevation };
        let p = cs.polar_to_geo( polar);
        if let Some(idx) = grid.cell_index( p.latitude, p.longitude) {
            codes[idx].fetch_max( code, Ordering::Relaxed);
        }
    }
}

/// RGBA colors for all 256 reflectivity codes (the nexrad-render reflectivity scale). Codes below `min_dbz` and the
/// below-threshold/range-folded codes are transparent
pub fn reflectivity_palette (min_dbz: f32)->Vec<[u8;4]> {
    let scale = default_scale( Product::Reflectivity);
    (0..=255u8).map( |code| match code_to_dbz( code) {
        Some(dbz) if dbz >= min_dbz => { let mut c = scale.color( dbz).to_rgba8(); c[3] = 255; c }
        _ => [0,0,0,0]
    }).collect()
}

/// WMS color map for reflectivity codes. Codes below `min_dbz` and the below-threshold/range-folded codes are nodata
/// (transparent). Entry names are the values (e.g. "35.5 dBZ")
pub fn reflectivity_cmap (min_dbz: f32)->Vec<CmapEntry> {
    reflectivity_palette( min_dbz).iter().enumerate().map( |(i, rgba)| {
        let code = i as u8;
        let name = match code {
            REF_BELOW_THRESHOLD => "below threshold".to_string(),
            REF_RANGE_FOLDED => "range folded".to_string(),
            _ => format!("{} dBZ", code_to_dbz( code).unwrap())
        };
        CmapEntry { value: i as i32, name, rgb: [rgba[0], rgba[1], rgba[2]], is_nodata: rgba[3] == 0 }
    }).collect()
}

/// compute a product of a scan with `resolution_m` meter grid cells and store it as GeoTIFF. Note this is blocking and compute intensive (GDAL file IO
/// and resampling)
pub fn compute_product (scan: &Scan, station: &Station, product: NexradProduct, scan_time: DateTime<Utc>,
                        resolution_m: f64, min_dbz: f32, cache_dir: &Path)->Result<NexradEntry> {
    let fields = product_fields( scan, product);
    if fields.is_empty() { return Err( op_failed!("no reflectivity sweeps for {} {}", station.id, product.name())) }

    let cs = coordinate_system( scan, station)?;
    let max_range = fields.iter().map( |f| f.max_range_km()).fold( 0.0, f64::max);
    let grid = ProductGrid::for_station( &cs.sweep_extent( max_range), cs.latitude(), resolution_m)?;
    let codes = resample_max( &fields, &cs, &grid);

    let entry = NexradEntry::new( cache_dir, &station.id, product, scan_time);
    write_product_geotiff( &entry.path, &codes, &grid, &reflectivity_palette( min_dbz))?;
    Ok(entry)
}

/// compute all requested products of a scan
pub fn compute_products (scan: &Scan, station: &Station, products: &[NexradProduct], scan_time: DateTime<Utc>,
                         resolution_m: f64, min_dbz: f32, cache_dir: &Path)->Result<Vec<NexradEntry>> {
    products.iter().map( |p| compute_product( scan, station, *p, scan_time, resolution_m, min_dbz, cache_dir)).collect()
}

/// write 8 bit codes as tiled GeoTIFF with palette, EPSG:4326 SRS and overviews (atomically, through a tmp file)
pub fn write_product_geotiff (path: &Path, codes: &[u8], grid: &ProductGrid, palette: &[[u8;4]])->Result<()> {
    if codes.len() != grid.width * grid.height { return Err( op_failed!("grid size mismatch")) }
    if let Some(dir) = path.parent() { std::fs::create_dir_all( dir)?; }
    let tmp_path = path.with_extension("tmp");

    let res = (|| -> Result<()> {
        let driver = DriverManager::get_driver_by_name("GTiff")?;
        let opts = RasterCreationOptions::from_iter( ["TILED=YES", "BLOCKXSIZE=256", "BLOCKYSIZE=256", "COMPRESS=DEFLATE"]);
        let mut ds = driver.create_with_band_type_with_options::<u8,_>( &tmp_path, grid.width, grid.height, 1, &opts)?;
        ds.set_geo_transform( &grid.geotransform())?;
        ds.set_spatial_ref( &SpatialRef::from_epsg(4326)?)?;
        {
            let mut band = ds.rasterband(1)?;
            let mut ct = ColorTable::new( PaletteInterpretation::Rgba);
            for (i, c) in palette.iter().enumerate() {
                ct.set_color_entry( i as u16, &ColorEntry::rgba( c[0] as i16, c[1] as i16, c[2] as i16, c[3] as i16));
            }
            band.set_color_table( &ct);
            band.write( (0,0), (grid.width, grid.height), &mut Buffer::new( (grid.width, grid.height), codes.to_vec()))?;
        }
        ds.build_overviews( "NEAREST", &OVERVIEWS, &[])?; // codes can't be interpolated
        ds.close()?;
        Ok(())
    })();

    match res {
        Ok(()) => { std::fs::rename( &tmp_path, path)?; Ok(()) }
        Err(e) => { let _ = std::fs::remove_file( &tmp_path); Err(e) }
    }
}
