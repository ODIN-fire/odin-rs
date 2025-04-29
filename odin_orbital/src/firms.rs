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

/// a module that imports Hotspots from NASAs Fire Information for Resource Management System (FIRMS)
/// see https://firms.modaps.eosdis.nasa.gov/usfs/active_fire/ for available data and APIs
/// This will support VIIRS, MODIS and Landsat hotspot import

use std::{collections::VecDeque, fmt, fs::File, io::{self,Write}, path::{Path,PathBuf}, time::Duration, sync::Arc};
use serde::{Serialize,Deserialize};
use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Timelike, Utc};
use reqwest::{self, Client};
use uom::si::{f32::V, f64::{Length, Power, ThermodynamicTemperature}, length::meter, power::megawatt, thermodynamic_temperature::kelvin};
use csv;
use async_trait::async_trait;
use bit_set::BitSet;
use odin_common::{
    angle::{Angle180,Longitude,Latitude}, 
    cartesian3::{dist_squared, find_closest_index, Cartesian3}, 
    cartographic::Cartographic, 
    datetime::{self, de_duration_from_fractional_secs, de_from_epoch_millis, from_epoch_millis, ser_duration_as_fractional_secs, ser_epoch_millis}, 
    geo::{GeoPoint, GeoRect}, macros::if_let, 
    net::download_url 
};
use odin_macro::public_struct;
use crate::{errors::{op_failed, OdinOrbitalError, Result}, HotspotList, OrbitalSatelliteInfo};
use crate::{Hotspot, HotspotConfidence, HotspotImporter, overpass::Overpass, CompletedOverpass};

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
    download_delay: Duration 
}

/* #region VIIRS hotspots ******************************************************************************************/

/// this is the raw record format of the VIIRS FDDC data product as it is retrieved from the FIRMS server
/// field descriptions on https://www.earthdata.nasa.gov/data/instruments/viirs/viirs-i-band-375-m-active-fire-data
#[derive(Debug,Deserialize)]
#[public_struct]
struct RawViirsHotspot {
    latitude: f64,
    longitude: f64,
    bright_ti4: f64,
    scan: f64,
    track: f64,
    acq_date: String, // ?? Date
    acq_time: u32, // ?? hmm
    satellite: String,
    confidence: String,
    version: String,
    bright_ti5: f64,
    frp: f64,
    daynight: String
}

/// this is the internal format and what we send (serialized) to clients


pub struct ViirsHotspotImporter {
    config: FirmsConfig,
    sat_info: Arc<OrbitalSatelliteInfo>,
    download_delay: Duration,
    cache_dir: PathBuf,

    last_date: DateTime<Utc> // most recent hotspot date we have seen so far
}

impl ViirsHotspotImporter {
    pub fn new (config: FirmsConfig, sat_info: Arc<OrbitalSatelliteInfo>, cache_dir: impl AsRef<Path>)->Self {
        let cache_dir = cache_dir.as_ref().to_path_buf();
        let last_date = datetime::from_epoch_millis(0);
        let download_delay = if let Some(sat) = config.satellites.iter().find(|sat| sat.sat_id == sat_info.sat_id) {
            sat.download_delay
        } else {
            Duration::from_mins(10) // TODO - is this a sensible default value?
        };

        ViirsHotspotImporter { config, sat_info, download_delay, cache_dir, last_date }
    }

    /// parse the CSV data provided by the reader, convert the RawViirsHotpots from it into (uom-aware) ViirsHotspots,
    /// and sort them into the provided mutable list of CompletedOverpass items (which are aggregations of Overpass and related
    /// hotspot lists observed during this overpass)
    pub fn import_hotspots (reader: impl io::Read, cops: &mut VecDeque<CompletedOverpass<HotspotList>>) -> Result<(BitSet,DateTime<Utc>)> {
        let mut changed_overpasses = BitSet::with_capacity(cops.len());
        let mut last_idx: Option<usize> = None; // none yet
        let mut hotspots: Vec<Hotspot> = Vec::new();
        let mut csv_reader = csv::Reader::from_reader(reader);
        let mut last_date = datetime::from_epoch_millis(0);

        for res in csv_reader.deserialize() {
            let raw_hs: RawViirsHotspot = res?;
            //println!("@@ raw {raw_hs:?}");
            if_let! {
                Some(sat_id) = Self::get_sat_id( &raw_hs.satellite),
                Some(conf) = Self::get_confidence( &raw_hs.confidence),
                Ok(date) = NaiveDate::parse_from_str( &raw_hs.acq_date, "%Y-%m-%d"),
                Some(date) = date.and_hms_opt(raw_hs.acq_time/100, raw_hs.acq_time%100, 0) => {
                    let date = Utc.from_utc_datetime(&date);
                    if date > last_date { last_date = date; }
                
                    if let Some(idx) = find_covering_overpass( sat_id, date, cops, last_idx) {
                        if let Some(j) = last_idx {
                            if idx != j && !hotspots.is_empty() { // this is a new overpass
                                if cops[j].data.is_none() {
                                    cops[j].data = Some( HotspotList::new( &cops[j].overpass, hotspots) );
                                    changed_overpasses.insert(j);
                                }
                                hotspots = Vec::new();
                            }
                        }              
                        last_idx = Some(idx); // we have a last idx for which we have to store hotspots
                        

                        //println!("@@ {date} -> {}", cops[idx].overpass.end);

                        let lon = Longitude::from_degrees(raw_hs.longitude);
                        let lat = Latitude::from_degrees(raw_hs.latitude);

                        let geo = Cartographic::from_degrees(raw_hs.longitude, raw_hs.latitude, 0.0);
                        let pos = Cartesian3::from(&geo);

                        let scan_m = raw_hs.scan * 1000.0;
                        let scan = Length::new::<meter>(scan_m);

                        let track_m = raw_hs.track * 1000.0;
                        let track = Length::new::<meter>(track_m);

                        let gp = cops[idx].overpass.closest_track_point( &pos);
                        let geo_gp = Cartographic::from(gp);
                        let alpha =  geo.bearing_to( &geo_gp);
                        let area = compute_footprint( &pos, track_m, scan_m, -alpha);

                        let rot = Angle180::from_radians(alpha);
                        let dist = Length::new::<meter>( geo_gp.distance_to(&geo));

                        let temp = Some(ThermodynamicTemperature::new::<kelvin>(raw_hs.bright_ti4));
                        let frp = Some(Power::new::<megawatt>(raw_hs.frp));

                        let vhs = Hotspot { pos, lon, lat, area, scan, track, rot, dist, date, conf, temp, frp };
                        //println!("@@ vhs: {vhs:?}");
                        hotspots.push( vhs);
                    }
                }
            } 
        }
        if let Some(j) = last_idx {
            if !hotspots.is_empty() {
                if cops[j].data.is_none() {
                    cops[j].data = Some( HotspotList::new( &cops[j].overpass, hotspots) );
                    changed_overpasses.insert(j);
                } 
                // TODO should we replace if prev data contains URT hotspots?
            }
        }

        Ok((changed_overpasses,last_date))
    }        


    /// according to https://firms.modaps.eosdis.nasa.gov/usfs/api/area/
    ///   [BASE_URL]/api/area/csv/[MAP_KEY]/[SOURCE]/[AREA_COORDINATES]/[DAY_RANGE]/[DAY]
    ///    e.g. /api/area/csv/534b391abcdf3cf5969cb7ec8ce07de5/VIIRS_NOAA21_NRT/-126,21,-66,50/1/2025-04-04
    /// Note that only full day ranges are allowed (1-10), which also means consecutive downloads over a day do overlap
    fn current_hotspots_request_url (&self, source: &str, n_days: usize)->String {
        let bbox = &self.config.bounds;
        format!( "{}/usfs/api/area/csv/{}/{}/{:.0},{:.0},{:.0},{:.0}/{}", 
                self.config.base_url, self.config.map_key, source,  
                bbox.west().degrees(), bbox.south().degrees(), bbox.east().degrees(), bbox.north().degrees(), n_days)
    }

    fn file_path (&self, source: &str, date: DateTime<Utc>)->PathBuf {
        let now = Utc::now();
        let fname = format!("{}_{:4}-{:02}-{:02}_{:02}{:02}.csv", source, date.year(), date.month(), date.day(), date.hour(), date.minute());
        self.cache_dir.join(Path::new(&fname))
    }

    //--- CSV field parsers

    fn get_sat_id (name: &str)->Option<u32> {
        match name {
            "N21" => Some(54234),  // NOAA-21
            "N20" => Some(43013),  // NOAA-20
            "N"   => Some(37849),  // Suomi-NPP
            _     => None
        }
    }

    fn get_source (sat_id: u32)->Option<&'static str> {
        match sat_id {
            54234 => Some("VIIRS_NOAA21_NRT"),
            43013 => Some("VIIRS_NOAA20_NRT"),
            37849 => Some("VIIRS_SNPP_NRT"),
            _     => None
        }
    }

    fn get_confidence (s: &str)->Option<HotspotConfidence> {
        match s {
            "n" => Some(HotspotConfidence::Nominal),
            "h" => Some(HotspotConfidence::High),
            "l" => Some(HotspotConfidence::Low),
            _ => None
        }
    }
}

fn compute_footprint( p: &Cartesian3, track: f64, scan: f64, alpha: f64)->[Cartesian3;4] {
    let (u, u_east, u_north) = p.en_units();

    let dw = u_east * track / 2.0;
    let dh = u_north * scan / 2.0;

    let p1 = p - dw - dh; // WS
    let p2 = p + dw - dh; // ES
    let p3 = p + dw + dh; // EN
    let p4 = p - dw + dh; // WN

    let mut p1 = p1.rotate_around( &u, alpha);
    let mut p2 = p2.rotate_around( &u, alpha);
    let mut p3 = p3.rotate_around( &u, alpha);
    let mut p4 = p4.rotate_around( &u, alpha);
    
    p1.round_to_decimals(0);
    p2.round_to_decimals(0);
    p3.round_to_decimals(0);
    p4.round_to_decimals(0);
    
    [p1,p2,p3,p4]
}


// note this is based on the empirical assumption that VIIRS hotspot files are always monotonic in overpasses. This is not true with respect
// to acquisition dates within the same overpass. Since this might be FIRMS/instrument specific the function is here and not in overpass.rs
pub fn find_covering_overpass<T> (sat_id: u32, date: DateTime<Utc>, cops: &VecDeque<CompletedOverpass<T>>, last_idx: Option<usize>)->Option<usize> {
    if let Some(i) = last_idx {
        if cops[i].overpass.sat_id == sat_id && is_covering_overpass( &cops[i].overpass, date) {
            return last_idx;
        }
    }

    let mut i = if let Some(idx) = last_idx { idx+1 } else { 0 };
    let len = cops.len();
    while i < len {
        if cops[i].overpass.sat_id == sat_id && is_covering_overpass( &cops[i].overpass, date) { 
            return Some(i) 
        }
        i += 1;
    }

    None
}

// again this is here because it depends on the satellite & respective ground data processing facility what the hotspot date is
// and how it maps to the overpass start/end
pub fn is_covering_overpass (o: &Overpass, d: DateTime<Utc>)->bool {
    // give some leeway at the end since acquisition might have some latency - we assume download latency < orbit_dur / 2
    //let cutoff = o.end + o.mean_orbit_duration.div_f64(2.0);
    let cutoff = o.end + Duration::from_mins(10);
    //println!("@@ {} < {} < {}", o.start, d, cutoff);
    (d >= o.start) && (d <= cutoff)
}

#[async_trait]
impl HotspotImporter for ViirsHotspotImporter {

    async fn import_hotspots (&mut self, n_days: usize, cops: &mut VecDeque<CompletedOverpass<HotspotList>>) -> Result<BitSet> {
        let date = datetime::utc_now();
        let source = Self::get_source(self.sat_info.sat_id).ok_or( op_failed!("unknown VIIRS source"))?;
        let url = self.current_hotspots_request_url( source, n_days);
        let file_path = self.file_path(source, date);
        let client = Client::new(); // no need to keep it around as this is only called every couple of hours

        let size = download_url( &client, &url, &None, &file_path).await?;

        if size > 0 {
            let file = File::open( file_path)?;
            let (retrieved,last_date) = Self::import_hotspots( file, cops)?;
            if last_date > self.last_date {
                self.last_date = last_date;
            }
            Ok(retrieved)

        } else { 
            Err( op_failed!("no FIRMS data"))
        }
    }

    fn get_download_schedule (&self, overpass_end: DateTime<Utc>) -> DateTime<Utc> {
        // we get continuous FIRMS data - no need to wait for ground station overpasses
        overpass_end + self.download_delay
    }

    fn last_reported (&self)->DateTime<Utc> {
        self.last_date
    }
}

/* #endregion VIIRS */
