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
use crate::{errors::{op_failed, OdinOrbitalError, Result}, find_covering_overpass, HotspotList, OrbitalSatelliteInfo};
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
    download_schedule: Vec<Duration> 
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
    cache_dir: PathBuf
}

impl ViirsHotspotImporter {
    pub fn new (config: FirmsConfig, sat_info: Arc<OrbitalSatelliteInfo>, cache_dir: impl AsRef<Path>)->Self {
        let cache_dir = cache_dir.as_ref().to_path_buf();
        ViirsHotspotImporter { config, sat_info, cache_dir }
    }

    /// parse the CSV data provided by the reader, convert the RawViirsHotpots from it into (uom-aware) ViirsHotspots,
    /// and sort them into the provided mutable list of CompletedOverpass items (which are aggregations of Overpass and related
    /// hotspot lists observed during this overpass)
    pub fn import_hotspots (reader: impl io::Read, cops: &mut VecDeque<CompletedOverpass<HotspotList>>) -> Result<BitSet> {
        let mut changed_overpasses = BitSet::with_capacity(cops.len());
        let mut last_idx: Option<usize> = None; // none yet
        let mut hotspots: Vec<Hotspot> = Vec::new();
        let mut csv_reader = csv::Reader::from_reader(reader);

        for res in csv_reader.deserialize() {
            let raw_hs: RawViirsHotspot = res?;
            //println!("@@ raw {raw_hs:?}");
            if_let! {
                Some(sat_id) = Self::get_sat_id( &raw_hs.satellite),
                Some(conf) = Self::get_confidence( &raw_hs.confidence),
                Ok(date) = NaiveDate::parse_from_str( &raw_hs.acq_date, "%Y-%m-%d"),
                Some(date) = date.and_hms_opt(raw_hs.acq_time/100, raw_hs.acq_time%100, 0).map(|d| Utc.from_utc_datetime(&d)),
                Some(idx) = find_covering_overpass( sat_id, date, cops, last_idx) => {
                    if let Some(j) = last_idx {
                        if idx != j && !hotspots.is_empty() {
                            if cops[j].data.is_none() {
                                cops[j].data = Some( HotspotList::new( &cops[idx].overpass, hotspots) );
                                changed_overpasses.insert(j);
                            }
                        }
                        hotspots = Vec::new();
                    }                
                    last_idx = Some(idx);

                    let lon = Longitude::from_degrees(raw_hs.longitude);
                    let lat = Latitude::from_degrees(raw_hs.latitude);

                    let geo = Cartographic::from_degrees(raw_hs.longitude, raw_hs.latitude, 0.0);
                    let pos = Cartesian3::from(&geo);

                    let scan = Length::new::<meter>(raw_hs.scan * 1000.0);
                    let track = Length::new::<meter>(raw_hs.track * 1000.0);

                    let gp = cops[idx].overpass.closest_ground_point( &pos);
                    let geo_gp = Cartographic::from(gp);
                    let rot = Angle180::from_radians( geo.bearing_to( &geo_gp));
                    let dist = Length::new::<meter>( geo_gp.distance_to(&geo));

                    let temp = Some(ThermodynamicTemperature::new::<kelvin>(raw_hs.bright_ti4));
                    let frp = Some(Power::new::<megawatt>(raw_hs.frp));

                    let vhs = Hotspot { pos, lon, lat, scan, track, rot, dist, date, conf, temp, frp };
                    //println!("@@ vhs: {vhs:?}");
                    hotspots.push( vhs);
                }
            } 
        }
        if let Some(idx) = last_idx {
            if !hotspots.is_empty() {
                if cops[idx].data.is_none() {
                    cops[idx].data = Some( HotspotList::new( &cops[idx].overpass, hotspots) );
                    changed_overpasses.insert(idx);
                } 
                // TODO should we replace if prev data contains URT hotspots?
            }
        }

        Ok(changed_overpasses)
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


#[async_trait]
impl HotspotImporter for ViirsHotspotImporter {

    async fn import_hotspots (&self, n_days: usize, cops: &mut VecDeque<CompletedOverpass<HotspotList>>) -> Result<BitSet> {
        let date = datetime::utc_now();
        let source = Self::get_source(self.sat_info.sat_id).ok_or( op_failed!("unknown VIIRS source"))?;
        let url = self.current_hotspots_request_url( source, n_days);
        let file_path = self.file_path(source, date);
        let client = Client::new(); // no need to keep it around as this is only called every couple of hours

        let size = download_url( &client, &url, &None, &file_path).await?;

        if size > 0 {
            let file = File::open( file_path)?;
            Self::import_hotspots( file, cops)

        } else { 
            Err( op_failed!("no FIRMS data"))
        }
    }

    fn get_download_schedule (&self, overpass_end: DateTime<Utc>) -> Vec<DateTime<Utc>> {
        if let Some(sat) = self.config.satellites.iter().find(|sat| sat.sat_id == self.sat_info.sat_id) {
            sat.download_schedule.iter().map( |dur| overpass_end + *dur).collect()
        } else {
            Vec::with_capacity(0)
        }
    }

}

/* #endregion VIIRS */
