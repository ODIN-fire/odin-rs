/*
 * Copyright © 2024, United States Government, as represented by the Administrator of
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

#[doc = include_str!("../doc/odin_hrrr.md")]

use std::{
    str::FromStr, path::{Path,PathBuf}, fmt::Write as FmtWrite, io::Write as IoWrite, fmt::Display, time::SystemTime,
    sync::Arc, hash::{Hash,DefaultHasher,Hasher}, any::type_name
};
use schedule::HrrrSchedules;
use serde::{Deserialize,Serialize};
use chrono::{DateTime,Datelike,Timelike,Utc,SecondsFormat};
use reqwest;
use regex::Regex;
use tempfile;
use tokio::{time::{Duration,Sleep}};
use async_trait::async_trait;

use odin_common::{
    angle::{Latitude, Longitude},
    datetime::{self, elapsed_minutes_since, full_hour, secs},
    fs::{ensure_writable_dir, odin_data_filename, path_str_to_fname, remove_old_files},
    geo::GeoRect,
    strings::{mk_string,to_sorted_string_vec}
};
use odin_actor::prelude::*;
use odin_actor::AbortHandle;
use odin_action::DataAction;
use odin_wx::{WxService, WxDataSetRequest, AddDataSet, RemoveDataSet, WxFileAvailable};
use odin_build::define_load_config;

mod actor;
pub use actor::*;

pub mod schedule;
pub mod meta;

mod errors;
pub use errors::*;

pub mod fields;
use fields::{FieldId,LevelId};

const ONE_HOUR: Duration = Duration::from_secs(3600);

define_load_config!{}

/// general HRRR server / download parameters configuration
#[derive(Clone,Serialize,Deserialize,Debug)]
pub struct HrrrConfig {
    // region name (e.g. "conus")
    pub region: String,

    /// server URL from where to retrieve files (e.g. <https://nomads.ncep.noaa.gov/cgi-bin/filter_hrrr_2d.pl>)
    pub url: String,

    /// server URL from where to retrieve directory listings (e.g. <https://nomads.ncep.noaa.gov/pub/data/nccf/com/hrrr/prod/hrrr.${yyyyMMdd}/conus>)
    /// to turn this into a real URL we have to expand the "${yyyyMMdd}" field
    pub dir_url_pattern: String,

    // fallbacks if we don't want to query schedules (the <https://nomads.ncep.noaa.gov/> dir listings do change and might not be reliable)
    // we assume roughly linear computation time
    pub reg_first: u32,
    pub reg_last: u32,
    pub reg_len: u32,

    pub ext_first: u32,
    pub ext_last: u32,
    pub ext_len: u32,

    /// delay between computed availability (schedule) of files and first download attempt
    pub delay: Duration,

    /// interval in which we check next forecast step availability
    pub check_interval: Duration,

    /// delay between download attempts
    pub retry_delay: Duration,

    /// max retry attempts
    pub max_retry: u8,

    /// how long to keep downloaded HRRR files
    pub max_age: Duration,
}

impl Default for HrrrConfig {
    fn default() -> Self {
        Self {
            region: "conus".to_string(),
            url: "https://nomads.ncep.noaa.gov/cgi-bin/filter_hrrr_2d.pl".to_string(),
            dir_url_pattern: "https://nomads.ncep.noaa.gov/pub/data/nccf/com/hrrr/prod/hrrr.${yyyyMMdd}/conus".to_string(),

            // those are just estimates (first dmin, last dmin, steps) - it might change
            reg_first: 48,
            reg_last: 84,
            reg_len: 19,

            ext_first: 48,
            ext_last: 108,
            ext_len: 49,

            delay: secs(60),
            check_interval: secs(30),
            retry_delay: secs( 30),
            max_retry: 4,
            max_age: datetime::hours(2),
        }
    }
}

pub struct HrrrService {
    wx_name: Arc<String>,
    model_name: Arc<String>,
    dataset_name: Arc<String>,

    fields_query: String, // the invariant non-region part of the query (computed from fields and levels)
    hself: ActorHandle<HrrrActorMsg>
}

impl HrrrService {
    pub fn new (fields: Vec<FieldId>, levels: Vec<LevelId>, dataset_name: Arc<String>, hself: ActorHandle<HrrrActorMsg>)->Self {

        let mut fields_query = String::with_capacity(256);

        for v in &fields {
            if !fields_query.is_empty() { fields_query.push('&') }
            fields_query.push_str("var_");
            fields_query.push_str(v.as_ref());
            fields_query.push_str("=on");
        }

        for v in &levels {
            fields_query.push('&');
            fields_query.push_str(v.as_ref());
            fields_query.push_str("=on");
        }

        let wx_name = Arc::new( type_name::<Self>().to_string());
        let model_name = Arc::new( "hrrr".to_string()); // we only support one

        HrrrService{fields_query, wx_name, model_name, dataset_name, hself}
    }

    pub fn new_basic (hself: ActorHandle<HrrrActorMsg>)->Self {
        use FieldId::*;
        use LevelId::*;
        let fields = vec![TMP,PRES,RH,TCDC,UGRD,VGRD];
        let levels = vec![lev_surface, lev_2_m_above_ground, lev_10_m_above_ground, lev_80_m_above_ground, lev_entire_atmosphere];

        let dataset_name = Arc::new( "basic".to_string());

        Self::new( fields, levels, dataset_name, hself)
    }
}

impl WxService for HrrrService {
    fn wx_name(&self) -> Arc<String> {
        self.wx_name.clone()
    }

    fn model_name (&self)->Arc<String> {
        self.model_name.clone()
    }

    fn dataset_name(&self)->Arc<String> {
        self.dataset_name.clone()
    }

    fn try_send_add_dataset(&self,req: Arc<WxDataSetRequest>) -> odin_actor::Result<()> {
        self.hself.try_send_msg( AddDataSet(req))
    }

    fn try_send_remove_dataset(&self,req: Arc<WxDataSetRequest>) -> odin_actor::Result<()> {
        self.hself.try_send_msg( RemoveDataSet(req))
    }

    fn create_request (&self, region: Arc<String>, bbox: GeoRect, fc_duration: Duration)->WxDataSetRequest {
        let query = format!("subregion=&toplat={}&leftlon={}&rightlon={}&bottomlat={}&{}",
                             bbox.north().degrees(), bbox.west().degrees(), bbox.east().degrees(), bbox.south().degrees(), self.fields_query);
        let wx_name = self.wx_name.clone();
        let model_name = self.model_name.clone();
        let dataset_name = self.dataset_name.clone();
        WxDataSetRequest { region, bbox, wx_name, model_name, dataset_name, fc_duration, query }
    }

    fn to_wx_grids (&self, fa: &WxFileAvailable)->odin_wx::Result<Vec<Arc<PathBuf>>> {
        // nothing to convert - this is HRRR
        Ok( vec![ fa.path.clone() ] )
    }
}

fn last_extended_forecast (dt: &DateTime<Utc>) -> DateTime<Utc> {
    let fh = full_hour::<Utc>(dt);
    let dh = fh.hour() % 6;

    if dh > 0 {
        fh - chrono::Duration::hours(dh as i64)
    } else {
        fh
    }
}

fn is_extended_forecast (dt: &DateTime<Utc>) -> bool {
    dt.hour() % 6 == 0
}

fn hours (h: u32) -> chrono::Duration {
    chrono::Duration::hours(h as i64)
}

fn minutes (m: u32) -> chrono::Duration {
    chrono::Duration::minutes(m as i64)
}

fn fmt_date(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Secs, true)
}

async fn sleep_secs (secs: u32) {
    if secs > 0 {
        tokio::time::sleep( tokio::time::Duration::from_secs( secs as u64)).await
    }
}

async fn wait_for (minutes: i64) {
    if minutes > 0 {
        info!("sleeping for {} min..", minutes);
        sleep_secs( minutes as u32 * 60).await;
    }
}

/// generate hrrr filename for given base hour and forecast step (hour from base hour) - this has to adhere to the ODIN data filename convention
fn get_odin_filename (cfg: &HrrrConfig, ds: &WxDataSetRequest, dt: &DateTime<Utc>, step: usize) -> String {
    let date = *dt + hours(step as u32);
    let fcs = step.to_string();
    let attrs: &[&str] = &[
        fcs.as_str(),
        ds.model_name.as_str()
    ];
    odin_data_filename( &ds.region, Some(date), attrs, Some("grib2"))
}

/// NOMADS file name (e.g. `hrrr.t15z.wrfsfcf08.grib2`)
fn get_nomad_filename (dt: &DateTime<Utc>, step: usize) -> String {
    format!("hrrr.t{:02}z.wrfsfcf{:02}.grib2", dt.hour(), step)
}

/// download a single file for given base date and forecast step
pub async fn download_file (cfg: &HrrrConfig, ds: &WxDataSetRequest, dt: &DateTime<Utc>, step: usize, cache_dir: &PathBuf) -> Result<PathBuf> {
    let filename = get_odin_filename( cfg, ds, dt, step);
    let nomad_filename = get_nomad_filename( dt, step);

    let url = format!("{}?dir=%2Fhrrr.{:04}{:02}{:02}%2F{}&file={}&{}",
        cfg.url,
        dt.year(), dt.month(), dt.day(),
        cfg.region,
        nomad_filename,
        ds.query
    );

    let mut pb = cache_dir.clone();
    pb.push(filename.as_str());
    let path = pb.as_path();
    let path_str = path.to_str().unwrap();

    if path.is_file() { // we already have it (from a previous run)
        info!("file {} already downloaded", filename);
        Ok(path.to_path_buf())

    } else { // we have to retrieve it from the NOAA server
        info!("downloading {}..", filename);

        let mut file = tempfile::NamedTempFile::new()?; // don't use path yet as that would expose partial downloads to the world
        let mut response = reqwest::get(&url).await?;
        while let Some(chunk) = response.chunk().await? {
            file.write_all(&chunk)?;
        }

        if response.status() == reqwest::StatusCode::OK {
            let file_len_kb = std::fs::metadata(file.path())?.len() / 1024;
            if file_len_kb > 0 {
                std::fs::rename(file.path(), path); // now make it visible to the world as a permanent file
                info!("{} kB saved to {}", file_len_kb, path_str);
                Ok(path.to_path_buf())
            } else {
                Err(op_failed("empty file"))
            }
        } else {
            Err(op_failed( format!("request failed with code {}", response.status().as_str())))
        }
        // note existing temp files will be automatically closed/deleted when dropped
    }
}

/// account for slightly varying file schedule and availability
pub async fn download_file_with_retry (cfg: &HrrrConfig, ds: &WxDataSetRequest, dt: &DateTime<Utc>, step: usize, cache_dir: &PathBuf) -> Result<PathBuf> {
    let mut retry = 0;
    loop {
        match download_file( cfg, ds, dt, step, cache_dir).await {
            Ok(path) => {
                return Ok(path)
            }
            Err(e) => {
                //println!("@@ step {} : {} failed with {e:?}, at min {}, retry {retry}", step, *dt + (ONE_HOUR * step as u32), Utc::now().minute() + 60);
                if retry < cfg.max_retry {
                    info!("step {} retry {}/{} in {} sec", step, retry, cfg.max_retry, cfg.retry_delay.as_secs());
                    tokio::time::sleep(cfg.retry_delay).await;
                    retry += 1;
                } else {
                    return Err(e)
                }
            }
        }
    }
}


/* #region download task ****************************************************************************************/

/// internal struct to queue download requests
#[derive(Debug)]
pub struct HrrrFileRequest {
    pub ds: Arc<WxDataSetRequest>,
    pub base: DateTime<Utc>, // base hour for forecast
    pub step: usize, // forecast hour
}

impl HrrrFileRequest {
    pub fn name(&self)->&String { &self.ds.region}
}

pub enum DownloadCmd {
    GetFile(HrrrFileRequest),
    Terminate
}


pub async fn process_download_requests<A> (rx: MpscReceiver<DownloadCmd>, cfg: Arc<HrrrConfig>, cache_dir: PathBuf, action: A)
    where A: DataAction<WxFileAvailable>
{
    remove_old_files( &cache_dir, cfg.max_age);
    let mut last_cleanup = SystemTime::now();

    loop {
        match recv(&rx).await {
            Ok(DownloadCmd::GetFile(request)) => {
                if let Ok(path) = download_file_with_retry(cfg.as_ref(), request.ds.as_ref(), &request.base, request.step, &cache_dir).await {
                    let basedate = request.base;
                    let forecasts = vec![ basedate + hours( request.step as u32) ];
                    let path = Arc::new(path);
                    let request = request.ds;

                    let data = WxFileAvailable { request, basedate, forecasts, path };
                    action.execute(data).await;
                } else {
                    warn!("step {}+{} permanently failed", request.base, request.step);
                }
            }
            Ok(DownloadCmd::Terminate) => { break }
            Err(_) => { break } // request queue closed, no use to go on
        }

        let now = SystemTime::now();
        if let Ok(elapsed) = now.duration_since(last_cleanup) {
            if elapsed > cfg.max_age {
                remove_old_files( &cache_dir, cfg.max_age);
                last_cleanup = now;
            }
        }
    }
}

pub fn spawn_download_task<A> (cfg: Arc<HrrrConfig>, cache_dir: PathBuf, action: A)->Result<(JoinHandle<()>,MpscSender<DownloadCmd>)>
     where A: DataAction<WxFileAvailable> + 'static
{
    let (tx,rx) = create_mpsc_sender_receiver::<DownloadCmd>(128);
    Ok( (spawn("hrrr-download", process_download_requests( rx, cfg, cache_dir, action))?, tx) )
}


/// get the next base hour and step (forecast hour) for a given time. This is used to determine when to retrieve the next available data set
/// and based on the following HRRR schedule model:
/// ```diagram
///     Bi   : base hour i (cycle base)
///     s[j] : forecast step j (0..18 for regular, 0..48 for extended)
///     ◻︎    : forecast data set for t = Bi+s[j]
///
///          Bi              s[0]   Bi+1       s[N]        Bi+2
///          │0               50    │60         84         │
///          │                |     │  cycle i  |          │
///          │                ◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎          │
///          │    dm < s[0]   |s[0]<= dm <=s[N] |   dm > S[N]
///          │                      |                      │
///          ├──────────────────────|────>T                │
///                           dm: minutes(T) + 60
///```
pub fn get_next_base_step (schedules: &HrrrSchedules, dt: &DateTime<Utc>)->(DateTime<Utc>,usize) {
   let mut dm = dt.minute();
   let mut base = full_hour(dt);
   let mut sched = schedules.schedule_for(&base);
   let mut step = 0;

   if dm < sched[0] {// base if previous hour
       dm += 60;
       base -= ONE_HOUR;
       sched = schedules.schedule_for(&base);
   }

   if dm >= sched[sched.len() - 1] {
       base = base + ONE_HOUR;
   } else {
       while dm >= sched[step] {
           step += 1
       }
   }

  (base, step)
}

/// get all *most recent* forecasts for a `HrrrDataSetRequests` that are already available.
/// This can span up to 3 forecast cycles as a forecast hour might only be available from a previous cycle.
/// This function is used for new `HrrrDataSetRequests`.
/// Regular cycles have 18 forecast steps (hours). Extended cycles (at 00,06,12,18h) have 48 forecast steps, i.e.
/// each computed list contains some data sets of the last extended cycle
///
/// ```diagram
///   ◻︎ : obsolete available forecast step (updated by subsequent cycle)
///   ◼︎ : relevant available forecast to retrieve (most up-to-date forecast for base + step)
///   ○ : not-yet-available forecast step
///
///   ◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎   (3) last ext cycle
///    ◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎
///     ◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◼︎◼︎◼︎                                    (2) last cycle:    always completely available
///      ◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎○○○○                                   (1) current cycle: might only be partially available
/// ```
pub async fn queue_available_forecasts (tx: &MpscSender<DownloadCmd>, ds: Arc<WxDataSetRequest>, schedules: &HrrrSchedules) {
    let now = Utc::now();  // TODO - this should use sim time

    let mut dm = now.minute();
    let mut base = full_hour(&now);
    let mut sched = schedules.schedule_for(&base);
    let mut step = 0;

    if dm < sched[0] {  // base if previous hour
        dm += 60;
        base -= ONE_HOUR;
        sched = schedules.schedule_for(&base);
    }

    let max_steps = if is_extended_forecast(&base) {schedules.ext.len()} else {schedules.reg.len()};

    //--- (1) queue what is available from current cycle
    while (step < max_steps) && (dm >= sched[step]) {
        tx.send( DownloadCmd::GetFile( HrrrFileRequest{ds: ds.clone(),base,step}) ).await;
        step += 1;
    }

    //--- (2) queue not-yet-updated forecasts from previous cycle
    base -= ONE_HOUR;
    sched = schedules.schedule_for(&base);
    step += 1;
    while step < max_steps {
        tx.send( DownloadCmd::GetFile( HrrrFileRequest{ds: ds.clone(),base,step}) ).await;
        step += 1;
    }

    //--- (3) if prev cycle wasn't extended get not-yet-updated forecasts from last extended cycle
    if !is_extended_forecast(&base) {
        base -= ONE_HOUR;
        while !is_extended_forecast(&base) {
            base -= ONE_HOUR;
            step += 1;
        }
        step += 1;
        sched = schedules.schedule_for(&base);
        while step < sched.len() {
            tx.send( DownloadCmd::GetFile( HrrrFileRequest{ds: ds.clone(),base,step}) ).await;
            step += 1;
        }
    }
}


/// non-actor function to spawn download task and periodically send it file requests for a fixed set of HrrrDataSetRequests
pub async fn run_downloads<A> (conf: HrrrConfig, dsrs: Vec<Arc<WxDataSetRequest>>, schedules: HrrrSchedules,
                               is_periodic: bool, file_avail_action: A) -> Result<()>
    where A: DataAction<WxFileAvailable> + 'static
{
    let check_interval = conf.check_interval;
    let (download_task,tx) = spawn_download_task( Arc::new(conf), hrrr_cache_dir(), file_avail_action)?;

    //--- initial download
    for dsr in &dsrs {
        queue_available_forecasts( &tx, dsr.clone(), &schedules).await;
    }

    //--- periodic download
    if is_periodic {
        let now = Utc::now();
        let (mut base, mut step) = get_next_base_step( &schedules, &now);

        loop {
            sleep( check_interval).await;

            let now = Utc::now();
            let mut sched = schedules.schedule_for(&base);

            while (now - base).num_minutes() as u32 >= sched[step] {
                for ds in &dsrs {
                    let cmd = DownloadCmd::GetFile( HrrrFileRequest {ds: ds.clone(), base, step} );
                    tx.send( cmd).await;
                }
                step += 1;

                if step >= sched.len() { // next cycle
                    base = base + datetime::hours(1);
                    step = 0;
                    sched =  schedules.schedule_for(&base);
                }
            }
        }

    } else {
        tx.send( DownloadCmd::Terminate).await;
        download_task.await.map_err(|e| op_failed(e))?;
    }

    Ok(())
}

/* #end region download task */

pub fn hrrr_cache_dir()->PathBuf {
    let path = odin_build::cache_dir().join("odin_hrrr");
    // Ok to panic - this is called during sys init
    ensure_writable_dir(&path).expect( &format!("invalid HRRR cache dir: {path:?}"));
    path
}

async fn wait_for_schedule (base: &DateTime<Utc>, scheduled: u32) {
    let elapsed = elapsed_minutes_since(base);
    let sched_min = scheduled as i64;

    if elapsed > 0 && elapsed < sched_min {
        wait_for(sched_min - elapsed).await;
    }
}
