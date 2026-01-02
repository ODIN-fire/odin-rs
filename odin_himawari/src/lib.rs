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

use std::{io::Read, ops::Index, path::{Path,PathBuf}, fs::{File}, time::Duration, collections::VecDeque};
use serde::{Serialize,Deserialize};
use serde_json;
use serde_repr::{Deserialize_repr};
use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, Timelike, Utc};
use uom::si::f64::{Area, Power};
use uom::si::{
    area::square_kilometer, power::megawatt,
};
use tokio::{fs::{File as AsyncFile},io::AsyncWriteExt};
use regex::Regex;
use suppaftp::tokio::{AsyncFtpStream};
use lazy_static::lazy_static;

use odin_common::{geo::GeoPoint3, collections::RingDeque, fs::get_modified_datetime};
use odin_dem::DemSource;
use odin_build::{define_load_asset, define_load_config, pkg_cache_dir};

pub mod errors;
use errors::Result;

use crate::errors::{OdinHimawariError,op_failed};

pub mod live_importer;
pub mod actor;
pub mod service;

define_load_config! {}
define_load_asset! {}

lazy_static! {
    pub static ref PKG_CACHE_DIR: PathBuf = pkg_cache_dir!();
    pub static ref HS_RE: Regex = Regex::new( r#".*/H09_(\d\d\d\d)(\d\d)(\d\d)_(\d\d)(\d\d)_.*\.csv"#).unwrap();
}

#[derive(Debug,Clone,Deserialize)]
pub struct HimawariConfig {
    pub sat_id: u32,
    user: String,
    pw: String,
    pub uri: String, // this includes the constant parts of the ftp path, e.g. 'ftp.ptree.jaxa.jp:/pub/himawari/L2/WLF/010/'
    pub dem: Option<DemSource>,

    pub init_hours: usize, // number of initial hours to retrieve
    pub update_interval: Duration, // how often to check for new data files
    pub cleanup_interval: Duration, // how often to purge old data files
    pub max_age: Duration, // max age of data files
}

/// raw data as we get it from the JAXA server
/// see https://www.eorc.jaxa.jp/ptree/documents/README_Himawari_L2WLF.txt
#[derive(Deserialize,Debug)]
pub struct RawHimawariHotspot {
    pub id: u32, // fire grid id
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub time: u32, // hhmm
    pub lat: f64, // deg
    pub lon: f64, // deg
    pub area: f64,  // km^2
    pub volcano: u32, // number of volcanos in 3x3 grid
    pub level: Level, // fire level
    pub reliability: Reliability,
    pub frp: f64, // Wm^-2
    pub qf: QualityFlag,
    pub hc: u32, // hot center id of fire cluster
}

/// the internal (high level) ODIN representation of a Himawari hotspot (might be extended)
#[derive(Serialize,Deserialize,Debug,Clone)]
pub struct HimawariHotspot {
    pub id: u32, // fire grid id
    #[serde(serialize_with = "odin_common::datetime::ser_epoch_millis")]
    pub date: DateTime<Utc>,
    pub position: GeoPoint3,
    #[serde(serialize_with = "odin_common::uom::ser_area_as_square_kilometers")]
    pub area: Area,
    pub volcano: u32, // number of volcanos in 3x3 grid
    pub level: Level, // fire level
    pub reliability: Reliability,
    #[serde(serialize_with = "odin_common::uom::ser_power_as_mw")]
    pub frp: Power,
    pub qf: QualityFlag,
    pub hc: u32, // hot center id of fire cluster
}

#[derive(Debug, Clone, PartialEq, Deserialize_repr, Serialize)]
#[repr(u8)]
pub enum Reliability { Low = 1, Normal = 3, High = 5 }

#[derive(Debug, Clone, PartialEq, Deserialize_repr, Serialize)]
#[repr(u8)]
pub enum QualityFlag { Normal = 0 , Saturated = 1, LowConfidence = 2 } // for frp reading

#[derive(Debug, Clone, PartialEq, Deserialize_repr, Serialize)]
#[repr(u8)]
pub enum Level { Cold = 1, Smoldering = 2, Flaming = 3 }

/// create a high level HimawariHotspot from a reference to a RawHimawariHotspot
impl TryFrom<&RawHimawariHotspot> for HimawariHotspot {
    type Error = OdinHimawariError;

    fn try_from (raw: &RawHimawariHotspot)->Result<HimawariHotspot> {
        let nd = NaiveDate::from_ymd_opt( raw.year, raw.month, raw.day).ok_or( op_failed!("invalid date"))?;
        let nt = NaiveTime::from_hms_opt( raw.time / 100, raw.time % 100, 0).ok_or( op_failed!("invalid date"))?;
        let date = NaiveDateTime::new( nd, nt).and_utc();

        Ok(
            HimawariHotspot {
                id: raw.id, // fire grid id
                date,
                position: GeoPoint3::from_lon_lat_degrees_alt_meters( raw.lon, raw.lat, 0.0),
                area: Area::new::<square_kilometer>(raw.area),
                volcano: raw.volcano,
                level: raw.level.clone(),
                reliability: raw.reliability.clone(),
                frp: Power::new::<megawatt>(raw.frp), // ? TODO
                qf: raw.qf.clone(),
                hc: raw.hc,
            }
        )
    }
}

#[derive(Debug, Clone, Serialize)] // to do: add to json, to json pretty
#[serde(rename_all(serialize = "camelCase"))]
pub struct HimawariHotspotSet {
    pub sat_id: u32,
    #[serde(serialize_with = "odin_common::datetime::ser_epoch_millis")]
    pub date: DateTime<Utc>,
    #[serde(serialize_with = "odin_common::datetime::ser_epoch_millis")]
    pub received: DateTime<Utc>,
    pub hotspots: Vec<HimawariHotspot>,

    //--- stats
    pub n_flaming: usize, // flaming level
    pub n_high: usize, // high reliability
    pub n_normal: usize, // normal quality
}

impl HimawariHotspotSet {
    pub fn new (sat_id: u32, date: DateTime<Utc>, received: DateTime<Utc>, hotspots: Vec<HimawariHotspot>)->Self {
        let mut n_flaming = 0;
        let mut n_high = 0;
        let mut n_normal = 0;

        for hs in &hotspots {
            if hs.level == Level::Flaming { n_flaming += 1 }
            if (hs.reliability == Reliability::High) { n_high += 1 }
            if (hs.qf == QualityFlag::Normal) { n_normal += 1 }
        }

        HimawariHotspotSet { sat_id, date, received, hotspots, n_flaming, n_high, n_normal }
    }

    pub fn from_file<P: AsRef<Path>> (sat_id: u32, path: &P)->Result<Self> {
        let date = date_of_hotspots( path)?;
        let received = get_modified_datetime( path).unwrap_or( Utc::now());
        let file = File::open(path)?;
        let hs = read_hotspots(file)?;

        Ok( Self::new( sat_id, date, received, hs) )
    }

    pub async fn fill_in_position_heights(&mut self, dem: &DemSource) -> Result<()> {
        let hotspots = &mut self.hotspots;
        let ps: Vec<(f64, f64)> = hotspots
            .iter()
            .map(|h| {
                let pos = &h.position;
                (pos.longitude_degrees(), pos.latitude_degrees())
            })
            .collect();

        let heights = dem.get_heights(Some(0.0), &ps).await?;

        for i in 0..ps.len() {
            let hotspot = &mut hotspots[i];
            let pos = &mut hotspot.position;
            pos.set_altitude_meters(heights[i]);
        }
        Ok(())
    }

    pub fn to_json_pretty(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(&self)?)
    }
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string(&self)?)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HimawariHotspotStore {
    hotspots: VecDeque<HimawariHotspotSet>,
    max_capacity: usize,
}

impl HimawariHotspotStore {
    pub fn new(capacity: usize) -> Self {
        HimawariHotspotStore {
            hotspots: VecDeque::with_capacity(capacity),
            max_capacity: capacity,
        }
    }

    pub fn initialize_hotspots(&mut self, init_hotspots: Vec<HimawariHotspotSet>) -> () {
        for hs in init_hotspots {
            self.hotspots.push_back(hs);
        }
    }

    pub fn update_hotspots (&mut self, new_hs: HimawariHotspotSet)->bool {
        self.hotspots.sort_into_ringbuffer( new_hs, |new_hs,old_hs| { new_hs.date < old_hs.date }).is_some()
    }

    /// note this iterates old-to-new, i.e. the newest entry comes last
    pub fn iter_old_to_new<'a>(&'a self) -> impl Iterator<Item = &'a HimawariHotspotSet> {
        self.hotspots.iter()
    }

    pub fn to_json_pretty(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(&self.hotspots)?)
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string(&self.hotspots)?)
    }
}

pub async fn download_hotspots<P: AsRef<Path>> (config: &HimawariConfig, ref_date: DateTime<Utc>, n_hours: usize, dir: P, new_only: bool)->Result<Vec<PathBuf>>
{
    let mut available_files: Vec<PathBuf> = Vec::new();
    let mut ftp_stream = AsyncFtpStream::connect( &config.uri).await?;

    let mut date = ref_date;

    ftp_stream.login( &config.user, &config.pw).await?;

    let mut n = 0;
    loop {
        let year = date.year();
        let month = date.month();
        let day = date.day();
        let hour = date.hour();

        let path = remote_dir_name(date);
        if ftp_stream.cwd(&path).await.is_ok() {
            if let Ok(list) = ftp_stream.nlst(None).await {
                for f in &list {
                    let path = PKG_CACHE_DIR.join(f);
                    if path.is_file() {
                        if !new_only {
                            available_files.push( path.to_path_buf())
                        }

                    } else {
                        let mut reader = ftp_stream.retr_as_stream(f).await?;
                        let mut file = AsyncFile::create( &path).await?;

                        tokio::io::copy( &mut reader, &mut file).await?;
                        ftp_stream.finalize_retr_stream(reader).await?;

                        let f_len = file.metadata().await?.len();
                        println!("{f} ({f_len} bytes)");
                        available_files.push( path.to_path_buf());
                    }
                }
                n += 1;
                if n > n_hours {
                    break;
                }
            }
        } else {
            // dir does not (yet?) exist
        }

        date = date - Duration::from_hours(1); // retrieve prev hour
    }

    ftp_stream.quit().await?;

    Ok(available_files)
}

// file name convention:  Hnn_YYYYMMDD_hhmm_L2WLFVER_FLDK.xxxxx_yyyyy.csv
//
//    nn: 2-digit number of the Himawari satellite (8, >9)
//    YYYY: 4-digit year of observation start time (timeline)
//    MM: 2-digit month of timeline
//    DD: 2-digit day of timeline
//    hh: 2-digit hour of timeline
//    mm: 2-digit minutes of timeline
//    VER: version
//    xxxxx: pixel number
//    yyyyy: line number
//
//  pixel/line number appears to be 06001 ??
//
// e.g. H09_20251209_1900_L2WLF010_FLDK.06001_06001.csv
pub fn hotspot_filename (date: DateTime<Utc>)->String {
    let minute = (date.minute() / 10) * 10;
    format!("H09_{:04}{:02}{:02}_{:02}{:02}_L2WLFVER_FLDK..06001_06001.csv",
        date.year(), date.month(), date.day(), date.hour(), minute)
}

pub fn date_of_hotspots<P: AsRef<Path>> (p: &P)->Result<DateTime<Utc>> {
    if let Some(ps) = p.as_ref().to_str()
    && let Some(cap) = HS_RE.captures(ps)
    && 6 == cap.len()
    && let Ok(year) = cap[1].parse::<i32>()
    && let Ok(month) = cap[2].parse::<u32>()
    && let Ok(day) = cap[3].parse::<u32>()
    && let Ok(hour) = cap[4].parse::<u32>()
    && let Ok(minute) = cap[5].parse::<u32>() {
        let nd = NaiveDate::from_ymd_opt( year, month, day).ok_or( op_failed!("invalid date"))?;
        let nt = NaiveTime::from_hms_opt( hour, minute, 0).ok_or( op_failed!("invalid date"))?;
        let date = NaiveDateTime::new( nd, nt).and_utc();
        Ok(date)
    } else {
        Err(op_failed!("not a valid Himawari hotspot filename: {:?}", p.as_ref()))
    }
}

// directory convention: /pub/himawari/L2/WLF/<VER>/<YYYYMM>/<DD>/<hh>
// e.g. /pub/himawari/L2/WLF/010/202512/09/19
pub fn remote_dir_name (date: DateTime<Utc>)->String {
    format!("/pub/himawari/L2/WLF/010/{:04}{:02}/{:02}/{:02}",
        date.year(), date.month(), date.day(), date.hour())
}

pub fn read_hotspots (reader: impl std::io::Read)->Result<Vec<HimawariHotspot>> {
    let mut csv_reader = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .has_headers(false) // don't lose 1st record - header is comment
        .from_reader(reader);

    let mut hotspots: Vec<HimawariHotspot> = Vec::new();

    for res in csv_reader.deserialize::<RawHimawariHotspot>() {
        if let Ok(ref raw_hs) = res {
            if let Ok(hs) = raw_hs.try_into() {
                hotspots.push(hs)
            }
       }
    }

    Ok( hotspots )
}

pub async fn fill_in_position_heights (hs: &mut [HimawariHotspot], dem: &DemSource)->Result<()> {
    let pts: Vec<(f64, f64)> = hs
        .iter()
        .map(|h| {
            let pos = &h.position;
            (pos.longitude_degrees(), pos.latitude_degrees())
        })
        .collect();

    let heights = dem.get_heights(Some(0.0), &pts).await?;

    for i in 0..pts.len() {
        let hotspot = &mut hs[i];
        let pos = &mut hotspot.position;
        pos.set_altitude_meters(heights[i]);
    }

    Ok(())
}
