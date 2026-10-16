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

use std::{path::PathBuf, str::FromStr};
use anyhow::{anyhow,Result};
use chrono::{DateTime, Utc};
use uom::si::{f64::Length, length::meter};

use odin_common::{define_cli, datetime::{full_hour, parse_utc_datetime}, geo::GeoRect};
use odin_openmeteo::{self, OpenMeteoConfig, ids::ModelId};
use odin_landfire;
use odin_gdal::geods::GeoDs;
use odin_nfdrs::{CACHE_DIR, NfdrsConfig, load_config, grid::{NfdrsField, create_field_pngs, create_interpolated_field_pngs, create_nfdrs_grid_datasets}};

define_cli! { ARGS [about="compute hourly NFDRS grid datasets (EPSG:4326) for a given bounding box"] =
    config: String [help="filename of NFDRS config file", long, default_value="nfdrs.ron"],
    wx_config: String [help="filename of OpenMeteo config file", long, default_value="openmeteo.ron"],
    model: String [help="name of openmeteo wx model", long, default_value="ncep_hrrr_conus"],
    lf_src: String [help="name of landfire source", long, default_value="lf_2025_conus.ron"],
    date: Option<DateTime<Utc>> [help="base date (default: current full hour)", long, value_parser=parse_utc_datetime],
    hours: u32 [help="number of forecast hours (datasets have hours+1 bands)", long, default_value="6"],
    res: Option<f64> [help="grid resolution in meters (default: wx model resolution)", long],
    fields: Option<String> [help="comma separated list of fields (dfm_1h,dfm_10h,dfm_100h,lfm_herb,lfm_woody,bi,erc,sc,ic,gsi,kbdi), default: all", long],
    dir: Option<String> [help="output directory (default: <cache>/grid)", long],
    png: bool [help="also create (color mapped) hourly PNG images for each field", long],
    discrete: bool [help="use discrete color map classes for PNG images (default is interpolated colors)", long],

    west: f64 [help="west boundary [degrees]", allow_hyphen_values = true],
    south: f64 [help="south boundary [degrees]", allow_hyphen_values = true],
    east: f64 [help="east boundary [degrees]", allow_hyphen_values = true],
    north: f64 [help="north boundary [degrees]", allow_hyphen_values = true]
}

#[tokio::main]
async fn main ()->Result<()> {
    let config: NfdrsConfig = load_config( &ARGS.config)?;
    let wx_config: OpenMeteoConfig = odin_openmeteo::load_config( &ARGS.wx_config)?;
    let lf_src: GeoDs = odin_landfire::load_config( &ARGS.lf_src)?;
    let model = ModelId::from_str( &ARGS.model)?;

    let geo_rect = GeoRect::from_wsen_degrees( ARGS.west, ARGS.south, ARGS.east, ARGS.north);
    let resolution = ARGS.res.map( |r| Length::new::<meter>(r)).unwrap_or_else( || model.grid_resolution());
    let base_date = full_hour( &ARGS.date.unwrap_or_else( Utc::now));
    let dir = ARGS.dir.as_ref().map( |d| PathBuf::from(d)).unwrap_or_else( || CACHE_DIR.join("grid"));

    let fields: Vec<NfdrsField> = if let Some(s) = &ARGS.fields {
        s.split(',').map( |f| NfdrsField::from_str( f.trim()).map_err( |_| anyhow!("unknown field '{}'", f))).collect::<Result<Vec<_>>>()?
    } else {
        NfdrsField::all()
    };

    println!("computing NFDRS grids for {:?} at {:.0}m resolution from {} (+{}h)..", geo_rect, resolution.get::<meter>(), base_date, ARGS.hours);
    let pixel_res = Length::new::<meter>( config.pixel_res_m);
    let output = create_nfdrs_grid_datasets( &config, &wx_config, model, &lf_src, &geo_rect, resolution, pixel_res, &base_date, ARGS.hours, &fields, &dir).await?;

    for p in &output.paths {
        println!("created {:?}", p);
    }

    if ARGS.png {
        for (field, p) in fields.iter().zip( output.paths.iter()) {
            let pngs = if ARGS.discrete { create_field_pngs( p, *field, &dir)? } else { create_interpolated_field_pngs( p, *field, &dir)? };
            println!("created {} PNG images for {}", pngs.len(), field);
        }
    }

    println!("\nbbox statistics (min / avg / max):");
    for fs in &output.stats {
        print!("{:10}", fs.field);
        for (k, hs) in fs.hourly.iter().enumerate() {
            match hs {
                Some(hs) => print!("  +{}h: {:.1} / {:.1} / {:.1}", k, hs.min, hs.avg, hs.max),
                None => print!("  +{}h: -", k)
            }
        }
        println!();
    }

    Ok(())
}
