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

use std::{path::Path, fs::File, io::Write, ops::Range};
use gdal::raster::RasterCreationOptions;
use gdal::{DriverManager, Driver, raster::{GdalType,Buffer}, Dataset, spatial_ref::SpatialRef, cpl::CslStringList};

use odin_common::fs::path_as_str;
use crate::errors::{Result, OdinGdalError, last_gdal_error, misc_error, op_failed};
use crate::{csl_string_list, get_adjusted_band_value_range, get_driver_from_filename, copy_srs};


pub fn to_dimmed_rgb<P: AsRef<Path>> (path: P, src_ds: &Dataset, band_no: usize, rgba_clr: u32, pct_range: Option<Range<f64>>)->Result<Dataset> {
    let src_band = src_ds.rasterband(band_no)?;
    let (width, height) = src_ds.raster_size();
    let nodata_val = src_band.no_data_value();

    let max_r = (rgba_clr >> 24) as u8;
    let max_g = (rgba_clr >> 16) as u8;
    let max_b = (rgba_clr >> 8) as u8;
    let max_a = rgba_clr as u8;

    let Range{start,end} = get_adjusted_band_value_range( &src_ds, band_no, pct_range)?;
    let range = end - start;

    // since PNG and JPG drivers only support create_copy we first have to create the target in memory
    let mem_driver = DriverManager::get_driver_by_name("MEM")?;
    let mut dst_ds = mem_driver.create_with_band_type::<u8, _>("", width as usize, height as usize, 4)?;
    copy_srs( src_ds, &mut dst_ds, Some(&path))?;

    for y in 0..height {
        let mut r_line = vec![0u8; width];
        let mut g_line = vec![0u8; width];
        let mut b_line = vec![0u8; width];
        let mut a_line = vec![0u8; width];

        let src_buf: Buffer<f64> = src_band.read_as(
            (0, y as isize),
            (width, 1),
            (width, 1),
            None,
        )?;
        let src_data = src_buf.data();

        for x in 0..width {
            let val = src_data[x];

            // Handle NoData pixels
            if let Some(nd) = nodata_val {
                if val == nd {
                    r_line[x] = 0;
                    g_line[x] = 0;
                    b_line[x] = 0;
                    a_line[x] = 0; // Fully transparent alpha
                    continue;
                }
            }

            // Map valid values to 0..=max_r,g,b
            if range != 0.0 {
                let scale_factor = (val - start) / range;
                r_line[x] = (scale_factor * max_r as f64).clamp(0.0, 255.0) as u8;
                g_line[x] = (scale_factor * max_g as f64).clamp(0.0, 255.0) as u8;
                b_line[x] = (scale_factor * max_b as f64).clamp(0.0, 255.0) as u8;
            } else {
                r_line[x] = max_r;
                g_line[x] = max_g;
                b_line[x] = max_b;
            }
            a_line[x] = 255; // Fully opaque alpha
        }

        let write_window = (0, y as isize);
        let write_size = (width, 1);
        let mut r_buf = Buffer::new( write_size, r_line);
        let mut g_buf = Buffer::new( write_size, g_line);
        let mut b_buf = Buffer::new( write_size, b_line);
        let mut a_buf = Buffer::new( write_size, a_line);

        dst_ds.rasterband(1)?.write(write_window, write_size, &mut r_buf)?;
        dst_ds.rasterband(2)?.write(write_window, write_size, &mut g_buf)?;
        dst_ds.rasterband(3)?.write(write_window, write_size, &mut b_buf)?;
        dst_ds.rasterband(4)?.write(write_window, write_size, &mut a_buf)?;
    }

    // now write the real target file
    let tgt_driver = get_driver_from_filename( path_as_str(&path)?).ok_or( op_failed!("no driver for {:?}", path.as_ref()))?;
    let c_opts: CslStringList = csl_string_list!("WORLDFILE=YES")?;

    let mut img_ds = dst_ds.create_copy( &tgt_driver, path, &c_opts)?;
    img_ds.flush_cache()?;

    Ok(img_ds)
}

/// fill with the provided color but adjust alpha channel value according to data source value
/// this is more useful if the data set values represent intensities
/// note that the provided color is just rgb (3 channels) as the alpha channel is computed
pub fn to_alpha<P: AsRef<Path>> (path: P, src_ds: &Dataset, band_no: usize, rgb_clr: u32, pct_range: Option<Range<f64>>)->Result<Dataset>{
    let src_band = src_ds.rasterband(band_no)?;
    let (width, height) = src_ds.raster_size();
    let nodata_val = src_band.no_data_value();

    let r = (rgb_clr >> 16) as u8;
    let g = (rgb_clr >> 8) as u8;
    let b = rgb_clr as u8;

    let Range{start,end} = get_adjusted_band_value_range( &src_ds, band_no, pct_range)?;
    let range = end - start;

    // since PNG and JPG drivers only support create_copy we first have to create the target in memory
    let mem_driver = DriverManager::get_driver_by_name("MEM")?;
    let mut dst_ds = mem_driver.create_with_band_type::<u8, _>("", width as usize, height as usize, 4)?;
    copy_srs( src_ds, &mut dst_ds, Some(&path))?;

    for y in 0..height {
        let mut r_line = vec![0u8; width];
        let mut g_line = vec![0u8; width];
        let mut b_line = vec![0u8; width];
        let mut a_line = vec![0u8; width];

        let src_buf: Buffer<f64> = src_band.read_as((0, y as isize), (width, 1), (width, 1), None)?;
        let src_data = src_buf.data();

        for x in 0..width {
            let val = src_data[x];

            r_line[x] = r;
            g_line[x] = g;
            b_line[x] = b;

            // Handle NoData pixels
            if let Some(nd) = nodata_val {
                if val == nd {
                    a_line[x] = 0; // Fully transparent alpha
                    continue;
                }
            }

            // Map valid values to alpha channel
            if range != 0.0 {
                let scale_factor = (val - start) / range;
                a_line[x] = (255.0 * scale_factor).clamp(0.0, 255.0) as u8;
            } else { // we only have one intensity
                a_line[x] = 255;
            }
        }

        let write_window = (0, y as isize);
        let write_size = (width, 1);
        let mut r_buf = Buffer::new( write_size, r_line);
        let mut g_buf = Buffer::new( write_size, g_line);
        let mut b_buf = Buffer::new( write_size, b_line);
        let mut a_buf = Buffer::new( write_size, a_line);

        dst_ds.rasterband(1)?.write(write_window, write_size, &mut r_buf)?;
        dst_ds.rasterband(2)?.write(write_window, write_size, &mut g_buf)?;
        dst_ds.rasterband(3)?.write(write_window, write_size, &mut b_buf)?;
        dst_ds.rasterband(4)?.write(write_window, write_size, &mut a_buf)?;
    }

    // now write the real target file
    let tgt_driver = get_driver_from_filename( path_as_str(&path)?).ok_or( op_failed!("no driver for {:?}", path.as_ref()))?;
    let c_opts: CslStringList = csl_string_list!("WORLDFILE=YES")?;

    let mut img_ds = dst_ds.create_copy( &tgt_driver, path, &c_opts)?;
    img_ds.flush_cache()?;

    Ok(img_ds)
}

pub fn to_cmap<P: AsRef<Path>> (path: P, src_ds: &Dataset, band_no: usize, pct_range: Option<Range<f64>>, rgba_clrs: &[u32])->Result<Dataset> {
    let src_band = src_ds.rasterband(band_no)?;
    let (width, height) = src_ds.raster_size();
    let nodata_val = src_band.no_data_value();

    let Range{start,end} = get_adjusted_band_value_range( &src_ds, band_no, pct_range)?;
    let range = end - start;
    let dr = range / rgba_clrs.len() as f64;
    let max_clr_idx = rgba_clrs.len()-1;

    // since PNG and JPG drivers only support create_copy we first have to create the target in memory

    let mem_driver = DriverManager::get_driver_by_name("MEM")?;
    let mut dst_ds = mem_driver.create_with_band_type::<u8, _>("", width as usize, height as usize, 4)?;
    copy_srs( src_ds, &mut dst_ds, Some(&path))?;

    for y in 0..height {
        let mut r_line = vec![0u8; width];
        let mut g_line = vec![0u8; width];
        let mut b_line = vec![0u8; width];
        let mut a_line = vec![0u8; width];

        let src_buf: Buffer<f64> = src_band.read_as((0, y as isize), (width, 1), (width, 1), None)?;
        let src_data = src_buf.data();

        for x in 0..width {
            let val = src_data[x];

            if let Some(nd) = nodata_val { // make nodata transparent
                if val == nd {
                    a_line[x] = 0; // Fully transparent alpha
                    continue;
                }
            }

            let clr_idx = if dr != 0.0 { // get the cmap index
                ((val - start) / dr).floor().clamp( 0.0, max_clr_idx as f64) as usize
            } else { // we only have one intensity
                max_clr_idx
            };
            if clr_idx == rgba_clrs.len() {
                println!("@@@ ARGHH: val:{val}, start:{start}, end:{end}, dr: {dr}, max_clr_idx:{max_clr_idx}");
            }


            let clr = rgba_clrs[clr_idx];
            r_line[x] = (clr >> 24) as u8;
            g_line[x] = (clr >> 16) as u8;
            b_line[x] = (clr >> 8) as u8;
            a_line[x] = clr as u8;
        }

        let write_window = (0, y as isize);
        let write_size = (width, 1);
        let mut r_buf = Buffer::new( write_size, r_line);
        let mut g_buf = Buffer::new( write_size, g_line);
        let mut b_buf = Buffer::new( write_size, b_line);
        let mut a_buf = Buffer::new( write_size, a_line);

        dst_ds.rasterband(1)?.write(write_window, write_size, &mut r_buf)?;
        dst_ds.rasterband(2)?.write(write_window, write_size, &mut g_buf)?;
        dst_ds.rasterband(3)?.write(write_window, write_size, &mut b_buf)?;
        dst_ds.rasterband(4)?.write(write_window, write_size, &mut a_buf)?;
    }

    // now write the real target file
    let tgt_driver = get_driver_from_filename( path_as_str(&path)?).ok_or( op_failed!("no driver for {:?}", path.as_ref()))?;
    let c_opts: CslStringList = csl_string_list!("WORLDFILE=YES")?;

    let mut img_ds = dst_ds.create_copy( &tgt_driver, path, &c_opts)?;
    img_ds.flush_cache()?;

    Ok(img_ds)
}
