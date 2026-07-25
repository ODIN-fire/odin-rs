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

#![allow(unused)]

use std::{path::Path,fs::File,io::Write,collections::HashSet};
use gdal::Dataset;
use clap;
use anyhow::{Result, anyhow};
use odin_common::define_cli;
use odin_gdal::{RasterAttributeTable};
use csv::{Reader, ReaderBuilder, StringRecord, StringRecordsIntoIter};

/// this tool is used to create ODIN or gdal color maps from full landfire.gov data sets distributed as
/// zip files that already contain CSV color maps

define_cli! { ARGS [about="create color table from landfire.gov CSV table"] =
    format: String   [help="color table format (odin,gdal)", long, short, default_value="gdal"],
    alpha: bool      [help="add alpha channel", long, short],
    nodata: Vec<i32> [help="set extra no_data values",long],
    nd_color: Option<String> [help="override nodata color (in RRGGBBAA hex format)", long],
    src_path: String [help="dataset input filename"],
    tgt_path: String [help="colormap output filename"]
}

#[derive(Debug)]
struct FieldIndices {
    value_idx: usize,
    name_idx: usize,
    r_idx: usize,
    g_idx: usize,
    b_idx: usize
}

fn main()->Result<()> {
    let is_gdal_cmap = ARGS.format.eq_ignore_ascii_case("gdal");
    let file = File::open(&ARGS.src_path)?;
    let mut rdr = ReaderBuilder::new().has_headers(false).from_reader( file); // make sure we can read the header
    let mut records = rdr.records();

    let mut no_data_values: HashSet<i32> = HashSet::new();
    for value in ARGS.nodata.iter() { no_data_values.insert( *value); }

    let mut nodata_clr = if let Some(rgba) = ARGS.nd_color.as_ref().map( |v| u32::from_str_radix(v, 16).unwrap() ) {
        ((rgba >> 24) & 0xff, (rgba >> 16) & 0xff, (rgba >> 8) & 0xff, rgba & 0xff)
    } else {
        ( 0, 0, 0, 0 )   // might get overwritten by Fill-NoData record
    };

    if let Some(result) = records.next() {
        let hdrs = result?;
        let fidxs = read_field_indices( &ARGS.src_path, &hdrs)?;

        let mut cmap_file = File::create( &ARGS.tgt_path)?;
        let mut n = 0;

        for result in records {
            let record = result?;
            if let Some(value) = record.get( fidxs.value_idx)
            && let Some(name) = record.get( fidxs.name_idx)
            && let Some(r) = record.get( fidxs.r_idx)
            && let Some(g) = record.get( fidxs.g_idx)
            && let Some(b) = record.get( fidxs.b_idx) {
                let r:u32 = r.parse()?;
                let g:u32 = g.parse()?;
                let b:u32 = b.parse()?;

                if name == "Fill-NoData" { // don't write it yet but check if we have an explicit nd_color set
                    if ARGS.nd_color.is_none() {
                        nodata_clr = ( r, g, b, if ARGS.alpha { 0 } else { 255 } );
                    }

                    let v = i32::from_str_radix( value, 10)?;
                    no_data_values.insert(v);

                } else {
                    if is_gdal_cmap { // simple GDAL cmap

                        if ARGS.alpha {
                            writeln!( cmap_file, "{}  {} {} {} 255", value, r, g, b);
                        } else {
                            writeln!( cmap_file, "{}  {} {} {}", value, r, g, b);
                        }

                    } else { // ODIN JSON cmap (no alpha here)
                        let clr = format!("#{:02x}{:02x}{:02x}", r, g, b);
                        if n > 0 {
                            write!( cmap_file, ",\n{{ \"color\":\"{}\", \"code\":\"{}\", \"descr\":\"{}\" }}", clr, value, name);
                        } else {
                            write!( cmap_file, "[\n{{ \"color\":\"{}\", \"code\":\"{}\", \"descr\":\"{}\" }}", clr, value, name);
                        }
                    }
                    n += 1;
                }
            }
        }

        for value in no_data_values {
            let clr = format!("#{:02x}{:02x}{:02x}", nodata_clr.0, nodata_clr.1, nodata_clr.2);
            let name = "Fill-NoData";

            if is_gdal_cmap {
                    writeln!( cmap_file, "{}  {} {} {} {}", value, nodata_clr.0, nodata_clr.1, nodata_clr.2, nodata_clr.3);
            } else {
                if n > 0 {
                    write!( cmap_file, ",\n{{ \"color\":\"{}\", \"code\":\"{}\", \"descr\":\"{}\" }}", clr, value, name);
                } else {
                    write!( cmap_file, "[\n{{ \"color\":\"{}\", \"code\":\"{}\", \"descr\":\"{}\" }}", clr, value, name);
                }
            }
            n += 1;
        }

        if !is_gdal_cmap &&  n > 0 { write!( cmap_file, "\n]"); }
    }

    Ok(())
}

fn read_field_indices (fname: &str, hdrs: &StringRecord)->Result<FieldIndices> {
    let mut value_idx: Option<usize> = None;
    let mut name_idx: Option<usize> = None;
    let mut r_idx: Option<usize> = None;
    let mut g_idx: Option<usize> = None;
    let mut b_idx: Option<usize> = None;

    let lf_name = get_lf_name( fname).ok_or( anyhow!("unknown landfire data set"))?;

    for (h,i) in hdrs.iter().zip( 0..) {
        if h == "VALUE" { value_idx = Some(i) }
        else if h == "R" { r_idx = Some(i) }
        else if h == "G" { g_idx = Some(i) }
        else if h == "B" { b_idx = Some(i) }
        else if h == lf_name { name_idx = Some(i) }
        else {} // ignore
    }

    if let Some(value_idx) = value_idx
    && let Some(name_idx) = name_idx
    && let Some(r_idx) = r_idx
    && let Some(g_idx) = g_idx
    && let Some(b_idx) = b_idx {
        Ok( FieldIndices{value_idx, name_idx, r_idx, g_idx, b_idx} )
    } else {
        Err( anyhow!("missing field indices: value:{:?}, name:{:?}, r:{:?}, g:{:?}, b:{:?}", value_idx, name_idx,r_idx,g_idx,b_idx))
    }
}

fn get_lf_name (fname: &str)->Option<&'static str> {
    let fname = fname.to_lowercase();
    if fname.ends_with("_evt.csv") { Some("EVT_NAME") }
    else if fname.ends_with("_fvt.csv") { Some("EVT_FUEL_N") }
    else if fname.ends_with("_fbfm40.csv") { Some("FBFM40") }
    else if fname.ends_with("_cc.csv") { Some("CC_PERCENT") }
    else if fname.ends_with("_ch.csv") { Some("METERS") }
    else if fname.ends_with("_cbd.csv") { Some("KGM3") }
    else if fname.ends_with("_cbh.csv") { Some("METERS") }
    //... and a lot more to follow
    else { None }
}
