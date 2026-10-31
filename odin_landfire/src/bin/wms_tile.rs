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

//! render a single landfire WMS GetMap image (without server) to check layer configuration and rendering times

use std::{collections::HashMap, time::Instant};
use anyhow::{Result, anyhow};
use odin_common::define_cli;
use odin_gdal::geods_srv::{WmsRequest, parse_wms_request};
use odin_landfire::{LandfireConfig, load_config, wms::load_layers};

define_cli! { ARGS [about="render landfire WMS GetMap image"] =
    config: String [help="LandfireConfig name", long, default_value="landfire.ron"],
    color: Option<String> [help="RRGGBB color for alpha encoded layers", long],
    style: Option<String> [help="WMS style name (csv, alpha)", long],
    ndcolor: Option<String> [help="RRGGBBAA color for nodata pixels", long],
    size: u32      [help="width and height of image", long, default_value="256"],
    layer: String  [help="WMS layer name"],
    bbox: String   [help="west,south,east,north in degrees"],
    output: String [help="PNG output file"]
}

fn main ()->Result<()> {
    let config: LandfireConfig = load_config( &ARGS.config)?;

    let t0 = Instant::now();
    let layers = load_layers( &config.wms);
    println!("loaded {} layers in {:?}", layers.len(), t0.elapsed());
    let layer = layers.get( &ARGS.layer).ok_or( anyhow!("unknown layer {}", ARGS.layer))?;

    let mut q = format!("service=WMS&version=1.1.1&request=GetMap&format=image/png&transparent=true&srs=EPSG:4326&layers={}&bbox={}&width={}&height={}",
                        ARGS.layer, ARGS.bbox, ARGS.size, ARGS.size);
    if let Some(color) = &ARGS.color { q.push_str( &format!("&color={color}")); }
    if let Some(style) = &ARGS.style { q.push_str( &format!("&styles={style}")); }
    if let Some(ndcolor) = &ARGS.ndcolor { q.push_str( &format!("&ndcolor={ndcolor}")); }
    let q: HashMap<String,String> = q.split('&').filter_map( |kv| kv.split_once('=')).map( |(k,v)| (k.to_string(), v.to_string())).collect();

    let Ok(WmsRequest::GetMap(gm)) = parse_wms_request( &q) else { return Err( anyhow!("invalid GetMap request")) };

    let t0 = Instant::now();
    let style = layer.style_for( &gm).ok_or( anyhow!("unknown style"))?;
    let png = layer.render_png( style, &gm)?;
    println!("rendered {} bytes in {:?}", png.len(), t0.elapsed());

    std::fs::write( &ARGS.output, &png)?;
    Ok(())
}
