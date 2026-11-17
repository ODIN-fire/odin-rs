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

//! computation of geo-referenced products from decoded Level II volumes. Products are resampled from the polar sweep
//! data onto a geographic (EPSG:4326) grid that covers the max range of the used sweeps, encoded as 8 bit
//! reflectivity codes (see [`crate::dbz_to_code`]) and stored as tiled GeoTIFFs with a color table (the
//! reflectivity scale of the `nexrad-render` crate) and overviews

use std::{path::{Path,PathBuf}, thread};
use chrono::{DateTime, Utc};
use gdal::{Dataset, DriverManager, raster::{Buffer, ColorEntry, ColorTable, PaletteInterpretation, RasterCreationOptions}, spatial_ref::SpatialRef};
use nexrad::{
    model::{data::{GateStatus, Product, Scan, SweepField}, geo::{GeoExtent, GeoPoint, RadarCoordinateSystem}, meta::Site},
    render::default_scale
};
use odin_gdal::cmap::CmapEntry;

use crate::{
    NexradEntry, NexradProduct, Station, REF_BELOW_THRESHOLD, REF_RANGE_FOLDED, dbz_to_code, code_to_dbz,
    errors::{Result, op_failed}
};

/// max range [km] of NEXRAD reflectivity sweeps (used for station extents before we have data)
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

/// geographic extent [west,south,east,north] of a station for a max range (e.g. for display before we have products)
pub fn station_extent (station: &Station, max_range_km: f64)->Result<[f64;4]> {
    let e = station_coordinate_system( station)?.sweep_extent( max_range_km);
    Ok( [e.min.longitude, e.min.latitude, e.max.longitude, e.max.latitude] )
}

/// a geographic product grid with square cells of `resolution` degrees (row 0 is the north edge)
#[derive(Debug,Clone,PartialEq)]
pub struct ProductGrid {
    pub west: f64,
    pub north: f64,
    pub resolution: f64,
    pub width: usize,
    pub height: usize,
}

impl ProductGrid {
    /// grid that covers the given extent (rounded up to whole cells)
    pub fn covering (extent: &GeoExtent, resolution: f64)->Result<Self> {
        if !(resolution > 0.0) { return Err( op_failed!("invalid grid resolution {resolution}")) }
        let width = ((extent.max.longitude - extent.min.longitude) / resolution).ceil() as usize;
        let height = ((extent.max.latitude - extent.min.latitude) / resolution).ceil() as usize;
        if width == 0 || height == 0 { return Err( op_failed!("empty product grid")) }
        Ok( ProductGrid { west: extent.min.longitude, north: extent.max.latitude, resolution, width, height })
    }

    pub fn geotransform (&self)->[f64;6] {
        [ self.west, self.resolution, 0.0, self.north, 0.0, -self.resolution ]
    }

    /// center of a grid cell
    pub fn cell_center (&self, row: usize, col: usize)->GeoPoint {
        GeoPoint { latitude: self.north - (row as f64 + 0.5) * self.resolution, longitude: self.west + (col as f64 + 0.5) * self.resolution }
    }
}

/// resample sweep fields onto a grid. Each cell gets the max valid reflectivity of all fields at its position (for a
/// single field this is just the field value), or REF_RANGE_FOLDED / REF_BELOW_THRESHOLD if there is no valid value.
/// Rows are computed in parallel
pub fn resample_max (fields: &[SweepField], cs: &RadarCoordinateSystem, grid: &ProductGrid)->Vec<u8> {
    let mut codes = vec![REF_BELOW_THRESHOLD; grid.width * grid.height];
    let n_threads = thread::available_parallelism().map( |n| n.get()).unwrap_or(4).min( grid.height).max(1);
    let rows_per_thread = grid.height.div_ceil( n_threads);

    thread::scope( |scope| {
        for (chunk_idx, chunk) in codes.chunks_mut( rows_per_thread * grid.width).enumerate() {
            let row0 = chunk_idx * rows_per_thread;
            scope.spawn( move || {
                for (i, row_codes) in chunk.chunks_mut( grid.width).enumerate() {
                    let row = row0 + i;
                    for (col, code) in row_codes.iter_mut().enumerate() {
                        *code = sample_max( fields, cs, grid.cell_center( row, col));
                    }
                }
            });
        }
    });
    codes
}

fn sample_max (fields: &[SweepField], cs: &RadarCoordinateSystem, p: GeoPoint)->u8 {
    let mut max_dbz = f32::MIN;
    let mut is_valid = false;
    let mut is_range_folded = false;

    for field in fields {
        let Some(polar) = cs.geo_to_polar( p, field.elevation_angle_degrees() as f64) else { continue };
        if let Some((v, status)) = field.value_at_polar( polar.azimuth_angle_degrees, polar.range_km) {
            match status {
                GateStatus::Valid => if v > max_dbz { max_dbz = v; is_valid = true; }
                GateStatus::RangeFolded => is_range_folded = true,
                _ => {}
            }
        }
    }

    if is_valid { dbz_to_code( max_dbz) } else if is_range_folded { REF_RANGE_FOLDED } else { REF_BELOW_THRESHOLD }
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

/// compute a product of a scan and store it as GeoTIFF. Note this is blocking and compute intensive (GDAL file IO
/// and resampling)
pub fn compute_product (scan: &Scan, station: &Station, product: NexradProduct, scan_time: DateTime<Utc>,
                        resolution: f64, min_dbz: f32, cache_dir: &Path)->Result<NexradEntry> {
    let fields = product_fields( scan, product);
    if fields.is_empty() { return Err( op_failed!("no reflectivity sweeps for {} {}", station.id, product.name())) }

    let cs = coordinate_system( scan, station)?;
    let max_range = fields.iter().map( |f| f.max_range_km()).fold( 0.0, f64::max);
    let grid = ProductGrid::covering( &cs.sweep_extent( max_range), resolution)?;
    let codes = resample_max( &fields, &cs, &grid);

    let entry = NexradEntry::new( cache_dir, &station.id, product, scan_time);
    write_product_geotiff( &entry.path, &codes, &grid, &reflectivity_palette( min_dbz))?;
    Ok(entry)
}

/// compute all requested products of a scan
pub fn compute_products (scan: &Scan, station: &Station, products: &[NexradProduct], scan_time: DateTime<Utc>,
                         resolution: f64, min_dbz: f32, cache_dir: &Path)->Result<Vec<NexradEntry>> {
    products.iter().map( |p| compute_product( scan, station, *p, scan_time, resolution, min_dbz, cache_dir)).collect()
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
