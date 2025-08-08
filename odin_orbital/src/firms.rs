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
    angle::{Angle180, Latitude, Longitude}, cartesian3::{dist_squared, find_closest_index, Cartesian3}, cartographic::{earth_radius_at_geodetic_latitude, Cartographic}, cos, datetime::{self, de_duration_from_fractional_secs, de_from_epoch_millis, from_epoch_millis, minutes, ser_duration_as_fractional_secs, ser_epoch_millis}, geo::{GeoPoint, GeoRect}, macros::if_let, net::download_url, sin 
};
use odin_dem::DemSource;
use odin_macro::public_struct;
use crate::{errors::{op_failed, OdinOrbitalError, Result}, HotspotList, OrbitalSatelliteInfo};
use crate::{Hotspot, HotspotConfidence, HotspotImporter, overpass::Overpass, CompletedOverpass};

/// config of how/when to access hotspot data for supported satellites from FIRMS 
#[derive(Debug,Serialize,Deserialize)]
#[public_struct]
struct FirmsConfig {
    base_url: String,
    map_key: String,  // keep this private - it is rate limited
    bounds: GeoRect,
    dem: DemSource, // either Server or File - to retrieve heights of hotspot locations
    satellites: Vec<FirmsSatelliteData>,
}

impl FirmsConfig {
    fn get_source (&self, sat_id: u32)->Option<&String> {
        self.satellites.iter().find( |s|  s.sat_id == sat_id).map( |sd| &sd.data_source)
    }

    fn get_download_delay (&self, sat_id: u32)->Option<Duration> {
        self.satellites.iter().find( |s|  s.sat_id == sat_id).map( |sd| sd.download_delay)
    }
}

#[derive(Debug,Serialize,Deserialize)]
#[public_struct]
struct FirmsSatelliteData {
    sat_id: u32,
    sat_name: String,
    data_source: String, // the FIRMS data product name for this satellite 
    download_delay: Duration 
}

/// abstraction for FIRMS hotspot data records.
/// this is an internal type to support factoring out common functions for MODIS, VIIRS and OLI
pub trait RawFirmsHotspot: fmt::Debug + for<'de> serde::Deserialize<'de> {
    fn get_confidence (&self)->Option<HotspotConfidence>;
    fn get_sat_id (&self)->Option<u32>;
    fn get_utc_datetime (&self)->Option<DateTime<Utc>>;
    fn to_hotspot (&self, cop: &CompletedOverpass<HotspotList>)->Result<Hotspot>;
}

/// internal abstraction for FIRMS importers that is used to factor out common functions 
#[async_trait]
trait FirmsHotspotImporter: HotspotImporter {

    //--- our abstract field accessors
    fn get_config (&self)->&FirmsConfig;
    fn get_cache_dir (&self)->&PathBuf;
    fn get_source (&self)->&str;
    fn update_last_reported (&mut self, date: DateTime<Utc>);
    fn get_download_delay (&self)->Duration;


    /// according to https://firms.modaps.eosdis.nasa.gov/usfs/api/area/
    ///   [BASE_URL]/api/area/csv/[MAP_KEY]/[SOURCE]/[AREA_COORDINATES]/[DAY_RANGE]/[DAY]
    ///    e.g. /api/area/csv/534b391abcdf3cf5969cb7ec8ce07de5/VIIRS_NOAA21_NRT/-126,21,-66,50/1/2025-04-04
    /// Note that only full day ranges are allowed (1-10), which also means consecutive downloads over a day do overlap
    fn current_hotspots_request_url (&self, source: &str, n_days: usize)->String {
        let conf = self.get_config();
        let bbox = &conf.bounds;
        format!( "{}/usfs/api/area/csv/{}/{}/{:.0},{:.0},{:.0},{:.0}/{}", 
                conf.base_url, conf.map_key, source,  
                bbox.west().degrees(), bbox.south().degrees(), bbox.east().degrees(), bbox.north().degrees(), n_days)
    }

    fn file_path (&self, source: &str, date: DateTime<Utc>)->PathBuf {
        let now = datetime::utc_now();
        let fname = format!("{}_{:4}-{:02}-{:02}_{:02}{:02}.csv", source, date.year(), date.month(), date.day(), date.hour(), date.minute());
        self.get_cache_dir().join(Path::new(&fname))
    }
}


/* #region VIIRS hotspots **********************************************************************************/

/// this is the raw record format of the VIIRS FDDC data product as it is retrieved from the FIRMS server
/// field descriptions on https://www.earthdata.nasa.gov/data/instruments/viirs/viirs-i-band-375-m-active-fire-data
#[derive(Debug,Deserialize)]
#[public_struct]
pub struct RawViirsHotspot {
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

impl RawFirmsHotspot for RawViirsHotspot {
    fn get_confidence (&self)->Option<HotspotConfidence> {
        match self.confidence.as_str() {
            "n" => Some(HotspotConfidence::Medium),
            "h" => Some(HotspotConfidence::High),
            "l" => Some(HotspotConfidence::Low),
            _ => None
        }
    }

    fn get_sat_id (&self)->Option<u32> {
        match self.satellite.as_str() {
            "N21" => Some(54234),  // NOAA-21
            "N20" => Some(43013),  // NOAA-20
            "N"   => Some(37849),  // Suomi-NPP
            _     => None
        }
    }

    fn get_utc_datetime (&self)->Option<DateTime<Utc>> {
        get_acq_utc_datetime( &self.acq_date, self.acq_time)
    }

    
    // this can't be a simple From<_> impl since we need overpass context info and the translation might fail
    fn to_hotspot (&self, cop: &CompletedOverpass<HotspotList>)->Result<Hotspot> {
        let date = self.get_utc_datetime().ok_or_else( || op_failed!("invalid hotspot date"))?;

        let lon = Longitude::from_degrees(self.longitude);
        let lat = Latitude::from_degrees(self.latitude);

        let geo = Cartographic::from_degrees(self.longitude, self.latitude, 0.0);
        let pos = Cartesian3::from(&geo);  // will be re-computed with height once we have all hotspots (deferred because it might use external DEM server)

        let scan_m = self.scan * 1000.0;
        let scan = Length::new::<meter>(scan_m);

        let track_m = self.track * 1000.0;
        let track = Length::new::<meter>(track_m);

        let gp = cop.overpass.closest_track_point( &pos);
        let geo_gp = Cartographic::from(gp);
        let alpha =  geo.bearing_to( &geo_gp);
        let area = compute_footprint( &pos, track_m, scan_m, -alpha);

        let rot = Angle180::from_radians(alpha);
        let dist = Length::new::<meter>( geo_gp.distance_to(&geo));

        let conf = self.get_confidence();
        let temp = Some(ThermodynamicTemperature::new::<kelvin>(self.bright_ti4));
        let frp = Some(Power::new::<megawatt>(self.frp));

        Ok( Hotspot { pos, lon, lat, area, scan, track, rot, dist, date, conf, temp, frp } )
    }

}

/// the importer for hotspot data derived from Visible Infrared Imaging Radiometer Suite (VIIRS) instruments
pub struct ViirsHotspotImporter {
    config: FirmsConfig,
    sat_info: Arc<OrbitalSatelliteInfo>,
    source: String,
    download_delay: Duration,
    cache_dir: PathBuf,
    last_date: DateTime<Utc> // most recent hotspot date we have seen so far
}

impl ViirsHotspotImporter {
    pub fn new (config: FirmsConfig, sat_info: Arc<OrbitalSatelliteInfo>, cache_dir: impl AsRef<Path>)->Self {
        let cache_dir = cache_dir.as_ref().to_path_buf();
        let last_date = datetime::from_epoch_millis(0);
        let download_delay = config.get_download_delay( sat_info.sat_id).unwrap(); // Ok to panic in toplevel ctor
        let source = config.get_source( sat_info.sat_id).unwrap().clone(); // Ok to panic since this is a toplevel ctor

        ViirsHotspotImporter { config, sat_info, source, download_delay, cache_dir, last_date }
    }
}

impl FirmsHotspotImporter for ViirsHotspotImporter {

    fn get_config(&self)->&FirmsConfig { &self.config }
    fn get_source (&self)->&str { self.source.as_str() }
    fn get_cache_dir (&self)->&PathBuf { &self.cache_dir }
    fn get_download_delay (&self)->Duration { self.download_delay }

    fn update_last_reported (&mut self, date:DateTime<Utc>) {
        if date > self.last_date {
            self.last_date = date;
        }
    }
}

#[async_trait]
impl HotspotImporter for ViirsHotspotImporter {

    async fn import_hotspots (&mut self, n_days: usize, cops: &mut VecDeque<CompletedOverpass<HotspotList>>) -> Result<BitSet> {
        import_firms_hotspots::<ViirsHotspotImporter,RawViirsHotspot>(&mut self, n_days, cops).await
    }

    fn last_reported (&self) -> DateTime<Utc>  { 
        self.last_date
    }

    fn get_download_schedule (&self,overpass_end:DateTime<Utc>) -> DateTime<Utc>  {
        overpass_end + self.get_download_delay()
    }
}

/* #endregion VIIRS hotspots */


/* #region OLI (Landsat) hotspots ***********************************************************************/

/// this is the raw record format of the OLI data product as it is retrieved from the FIRMS server
/// field descriptions on https://www.earthdata.nasa.gov/data/tools/firms/faq ("attributes of Landsat fire data")
/// the pixel footprint is fixed 30x30m (this is a narrow push broom sensor)
#[derive(Debug,Deserialize)]
#[public_struct]
struct RawOliHotspot {
    latitude: f64,
    longitude: f64,
    path: i64,
    row: i64,
    scan: i64,  // NOTE: these are NOT pixel footprint dimensions but image indices
    track: i64,
    acq_date: String, // ?? Date
    acq_time: u32, // ?? hmm
    satellite: String,
    confidence: String,
    daynight: String
}

impl RawFirmsHotspot for RawOliHotspot {
    fn get_confidence (&self)->Option<HotspotConfidence> {
        match self.confidence.as_str() {
            "M" => Some(HotspotConfidence::Medium),
            "H" => Some(HotspotConfidence::High),
            "L" => Some(HotspotConfidence::Low),
            _ => None
        }
    }

    fn get_sat_id (&self)->Option<u32> {
        match self.satellite.as_str() {
            "L8" => Some(39084),
            "L9" => Some(49260),
            _     => None
        }
    }

    fn get_utc_datetime (&self)->Option<DateTime<Utc>> {
        get_acq_utc_datetime( &self.acq_date, self.acq_time)
    }

    fn to_hotspot (&self, cop: &CompletedOverpass<HotspotList>)->Result<Hotspot> {
        let date = self.get_utc_datetime().ok_or_else( || op_failed!("invalid hotspot date"))?;
        
        let lon = Longitude::from_degrees(self.longitude);
        let lat = Latitude::from_degrees(self.latitude);

        let geo = Cartographic::from_degrees(self.longitude, self.latitude, 0.0);
        let pos = Cartesian3::from(&geo);  // will be re-computed with height once we have all hotspots (deferred because it might use external DEM server)

        let scan_m: f64 = 30.0;
        let track_m: f64 = 30.0;

        let scan = Length::new::<meter>(scan_m);
        let track = Length::new::<meter>(track_m);

        let gp = cop.overpass.closest_track_point( &pos);
        let geo_gp = Cartographic::from(gp);
        let alpha =  geo.bearing_to( &geo_gp);
        let area = compute_footprint( &pos, track_m, scan_m, -alpha);

        let rot = Angle180::from_radians(alpha);
        let dist = Length::new::<meter>( geo_gp.distance_to(&geo));

        let conf = self.get_confidence();
        let temp = None;
        let frp = None;

        Ok( Hotspot { pos, lon, lat, area, scan, track, rot, dist, date, conf, temp, frp } )
    }
}

/// importer for hotspots obtained from Landsat Operational Land Imager (OLI) instrument
pub struct OliHotspotImporter {
    config: FirmsConfig,
    sat_info: Arc<OrbitalSatelliteInfo>,
    source: String,
    download_delay: Duration,
    cache_dir: PathBuf,
    last_date: DateTime<Utc> // most recent hotspot date we have seen so far
}

impl OliHotspotImporter {
    pub fn new (config: FirmsConfig, sat_info: Arc<OrbitalSatelliteInfo>, cache_dir: impl AsRef<Path>)->Self {
        let cache_dir = cache_dir.as_ref().to_path_buf();
        let last_date = datetime::from_epoch_millis(0);
        let download_delay = config.get_download_delay( sat_info.sat_id).unwrap(); // Ok to panic in toplevel ctor
        let source = config.get_source( sat_info.sat_id).unwrap().clone(); // Ok to panic since this is a toplevel ctor

        OliHotspotImporter { config, sat_info, source, download_delay, cache_dir, last_date }
    }
}

impl FirmsHotspotImporter for OliHotspotImporter {

    fn get_config(&self)->&FirmsConfig { &self.config }
    fn get_cache_dir (&self)->&PathBuf { &self.cache_dir }
    fn get_source (&self)->&str { self.source.as_str() }
    fn get_download_delay (&self)->Duration { self.download_delay }

    fn update_last_reported (&mut self, date:DateTime<Utc>) {
        if date > self.last_date {
            self.last_date = date;
        }
    }
}

#[async_trait]
impl HotspotImporter for OliHotspotImporter {

    async fn import_hotspots (&mut self, n_days: usize, cops: &mut VecDeque<CompletedOverpass<HotspotList>>) -> Result<BitSet> {
        import_firms_hotspots::<OliHotspotImporter,RawOliHotspot>(&mut self, n_days, cops).await
    }

    fn last_reported (&self) -> DateTime<Utc>  { 
        self.last_date
    }

    fn get_download_schedule (&self,overpass_end:DateTime<Utc>) -> DateTime<Utc>  {
        overpass_end + self.get_download_delay()
    }
}

/* #endregion OLI */

/* #region common funcs **********************************************************************************************/

async fn import_firms_hotspots<I,R> (importer: &mut I, n_days: usize, cops: &mut VecDeque<CompletedOverpass<HotspotList>>) -> Result<BitSet> 
    where I: FirmsHotspotImporter, R: RawFirmsHotspot
{
    let date = datetime::utc_now();
    let source = importer.get_source();
    let url = importer.current_hotspots_request_url( source, n_days);
    let file_path = importer.file_path(source, date);
    let config = importer.get_config();
    let client = Client::new(); // no need to keep it around as this is only called every couple of hours

    let size = download_url( &client, &url, &None, &file_path).await?;

    if size > 0 {
        let file = File::open( file_path)?;
        let (retrieved,last_date) = read_hotspots::<R>( file, cops)?;

        // FIRMS hotspots do not have terrain height - fill those in from our configured DEM
        add_hotspot_heights( &retrieved, &config.dem, cops).await?;

        importer.update_last_reported( last_date);
        Ok(retrieved)

    } else { 
        Err( op_failed!("no FIRMS data"))
    }
}

async fn add_hotspot_heights (retrieved: &BitSet, dem_src: &DemSource, cops: &mut VecDeque<CompletedOverpass<HotspotList>>) -> Result<()> {
    for i in retrieved {
        if let Some(hsl) = cops[i].data.as_mut() {
            let mut hotspots = &mut hsl.hotspots;
            let locations: Vec<(f64,f64)> = hotspots.iter().map( |h| (h.lon.degrees(), h.lat.degrees())).collect();
            if let Ok(heights) = dem_src.get_heights(Some(0.0), locations.as_ref()).await {
                for j in 0..heights.len() {
                    let mut hs = &mut hotspots[j];
                    let r = earth_radius_at_geodetic_latitude( hs.lat.radians());
                    let a = r + heights[j];

                    hs.pos.scale_to_length(a);
                    hs.pos.round_to_decimals(0); // we don't need more precision than 1m
                }
            }
        }
    }
    Ok(())
}

/// parse the CSV data provided by the reader, convert the RawViirsHotpots from it into (uom-aware) ViirsHotspots,
/// and sort them into the provided mutable list of CompletedOverpass items (which are aggregations of Overpass and related
/// hotspot lists observed during this overpass)
pub fn read_hotspots<R> (reader: impl io::Read, cops: &mut VecDeque<CompletedOverpass<HotspotList>>) -> Result<(BitSet,DateTime<Utc>)>
    where R: RawFirmsHotspot
{
    let mut changed_overpasses = BitSet::with_capacity(cops.len());
    let mut last_idx: Option<usize> = None; // none yet
    let mut hotspots: Vec<Hotspot> = Vec::new();
    let mut csv_reader = csv::Reader::from_reader(reader);
    let mut last_date = datetime::from_epoch_millis(0);

    for res in csv_reader.deserialize() {
        let raw_hs: R = res?;
        if_let! {
            Some(sat_id) = raw_hs.get_sat_id(),
            Some(conf) = raw_hs.get_confidence(),
            Some(date) = raw_hs.get_utc_datetime() => {
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
                    
                    if let Ok(hs) = raw_hs.to_hotspot( &cops[idx]) {
                        hotspots.push( hs);
                    }
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



/// scan and track are cross-scan and along-track dimentions of hotspot in meters
/// alpha is the outgoing bearing from the hotspot to its closest ground track point
fn compute_footprint( p: &Cartesian3, track: f64, scan: f64, alpha: f64)->[Cartesian3;4] {
    let (u, u_east, u_north) = p.en_units();

    let dw = u_east * track / 2.0;
    let dh = u_north * scan / 2.0;

    let mut vertices: [Cartesian3;4] = [
        p - dw - dh, // WS
        p + dw - dh, // ES
        p + dw + dh, // EN
        p - dw + dh, // WN
    ];

    Cartesian3::rotate_all(&u, alpha, &mut vertices);
    Cartesian3::round_all( &mut vertices, 0);
    vertices
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
    let d_start = o.start - minutes(30);
    let d_end = o.end + minutes(30);
    //println!("@@ {} < {} < {}", o.start, d, cutoff);
    (d >= d_start) && (d <= d_end)
}

/// get datetime for date/time specs of a raw FIRMS record.
/// date is specified as a YYYY-MM-DD string
/// time is a [H]HMM number 
fn get_acq_utc_datetime (acq_date: &str, acq_time: u32)->Option<DateTime<Utc>> {
    NaiveDate::parse_from_str( acq_date, "%Y-%m-%d").ok()
        .and_then( |nd| nd.and_hms_opt(acq_time/100, acq_time%100, 0) )
        .map( |ndt| Utc.from_utc_datetime(&ndt))
}

/* #endregion common funcs */
