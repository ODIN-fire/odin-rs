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

#![allow(unused)]

use std::{path::Path,fs::File,io::Write,time::Duration};
use serde::{Serialize,Deserialize};
use chrono::{DateTime, Datelike, Utc};
use reqwest;
use uom::si::{length::meter,f64::Length};
use odin_common::{angle::Angle360, geo::{GeoRect,GeoPoint}};
use odin_macro::public_struct;
use crate::errors::{op_failed, OdinOrbitalError, Result};

#[derive(Debug,Serialize,Deserialize)]
#[public_struct]
struct FirmsConfig {
    base_url: String,
    map_key: String,  // keep this private - it is rate limited
    bounds: GeoRect,
    satellites: Vec<FirmsSatelliteData>
}

#[derive(Debug,Serialize,Deserialize)]
#[public_struct]
struct FirmsSatelliteData {
    sat_id: u32,
    sat_name: String,
    data_source: String,
    download_schedule: Vec<Duration> 
}


/// this is the raw record format of the VIIRS FDDC data product as it is retrieved from the FIRMS server
/// field descriptions on https://www.earthdata.nasa.gov/data/instruments/viirs/viirs-i-band-375-m-active-fire-data
#[derive(Debug,Deserialize)]
#[public_struct]
struct RawViirsHotspot {
    latitude: f64,
    longitude: f64,
    bright_ti4: f32,
    scan: f32,
    track: f32,
    acq_date: String, // ?? Date
    acq_time: u32, // ?? hmm
    satellite: String,
    instrument: String,
    confidence: String,
    version: String,
    bright_ti5: f32,
    frp: f32,
    daynight: String
}

/// this is the internal format and what we send (serialized) to clients
#[derive(Debug,Serialize)]
#[public_struct]
struct ViirsHotspot {
    pos: GeoPoint,
    bright: f32,     // from bright_ti4
    frp: f32,
    scan: Length,    // cross-scan length of pixel in meters
    track: Length,   // along-track length of pixel in meters
    rot: Angle360,   // rotation angle of pixel rect
    date: DateTime<Utc>,
    conf: ViirsHotspotConfidence
}

#[derive(Debug,Serialize)]
pub enum ViirsHotspotConfidence {
    Low, Nominal, High
}

/// according to https://firms.modaps.eosdis.nasa.gov/usfs/api/area/
///   [BASE_URL]/api/area/csv/[MAP_KEY]/[SOURCE]/[AREA_COORDINATES]/[DAY_RANGE]/[DAY]
///    e.g. /api/area/csv/534b391acee33cf5969cb7ec8ce07de5/VIIRS_NOAA21_NRT/-126,21,-66,50/1/2025-04-04
/// Note that only full day ranges are allowed (1-10), which also means consecutive downloads over a day do overlap
fn request_url (config: &FirmsConfig, source: &str, n_days: u32, date: DateTime<Utc>)->String {
    let bbox = &config.bounds;
    format!( "{}//api/area/csv/{}/{}/{},{},{},{}/{}/{}-{}-{}", 
            config.base_url, config.map_key, source,  
            bbox.west().degrees(), bbox.south().degrees(), bbox.east().degrees(), bbox.north().degrees(), 
            n_days, date.year(), date.month(), date.day())
}

async fn retrieve_data<T> (url: &str, path: Option<T>)->Result<String> where T: AsRef<Path> {
    let response = reqwest::get(url).await?;
    let body = response.text().await?;

    if let Some(p) = path {
        let mut file = File::create(p)?;
        file.write_all(body.as_bytes());
    }

    Ok(body)
}