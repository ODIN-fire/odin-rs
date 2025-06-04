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

use std::{path::{Path,PathBuf}, sync::Arc, collections::HashMap, process::Command};
use chrono::{DateTime,Datelike,Timelike, Utc};
use reqwest::{self, Client};

use odin_build::pkg_cache_dir;
use odin_common::{
    datetime::{hours,short_utc_datetime_string}, 
    geo::GeoRect, net::download_url, collections::RingDeque,
    utm::{self,UtmRect,UtmZone,UTM}
};
use odin_hrrr::{AddDataSet, HrrrActorMsg, HrrrDataSetConfig, HrrrDataSetRequest, HrrrFileAvailable};
use odin_actor::prelude::*;
use crate::{errors::{OdinWindNinjaError,Result}, Forecast, ForecastRegion, ForecastStore, WindNinjaConfig};

//macro_rules! info { ($fmt:literal $(, $arg:expr )* ) => { {print!("INFO: "); println!( $fmt $(, $arg)* )} } }
//macro_rules! error { ($fmt:literal $(, $arg:expr )* ) => { {eprint!("\x1b[32;1m \x1b[37m ERROR: "); eprint!( $fmt $(, $arg)* ); eprintln!("\x1b[0m")} } }

/// the internal data structure that represents the input data for a single WindNinja run
#[derive(Debug)]
struct WnJob {
    region: Arc<String>,
    date: DateTime<Utc>,
    step: usize,
    hrrr_path: Arc<PathBuf>,
    dem_path: Arc<PathBuf>,
}

impl WnJob {
    fn into_forecast (self, path: Arc<PathBuf>)->Forecast {
        Forecast {
            region: self.region,
            date: self.date,
            step: self.step,
            path
        }
    }
}

struct WnTask {
    abort_handle: AbortHandle,
    tx: MpscSender<WnJob>
}

/// the WindNinjaActor state
pub struct WindNinjaActor<I,S,U> where I: DataRefAction<ForecastStore>, S: DataAction<Result<AddClientResponse>>, U: DataRefAction<Forecast>
{
    config: Arc<WindNinjaConfig>,
    cache_dir: Arc<PathBuf>,              // where to store computed forecasts
    hrrr: ActorHandle<HrrrActorMsg>, // where to get new HRRR reports from - this drives our data update

    forecast_store: ForecastStore,

    init_action: I,
    subscribe_action: S,
    update_action: U,

    wn_task: Option<WnTask>
}

impl <I,S,U> WindNinjaActor<I,S,U> where I: DataRefAction<ForecastStore>, S: DataAction<Result<AddClientResponse>>, U: DataRefAction<Forecast> {
    pub fn new (config: WindNinjaConfig, hrrr: ActorHandle<HrrrActorMsg>, init_action: I, subscribe_action: S, update_action: U)->Self {
        let config = Arc::new(config);
        let cache_dir = Arc::new(pkg_cache_dir!());
        let forecast_store = HashMap::new();
 
        WindNinjaActor { config, cache_dir, hrrr, forecast_store, init_action, subscribe_action, update_action, wn_task: None }
    }

    fn start (&mut self, hself: ActorHandle<WindNinjaActorMsg>)->Result<()> {
        let (tx, rx) = create_mpsc_sender_receiver::<WnJob>(64);
        let abort_handle = spawn("wn_task", wn_loop( hself, self.config.clone(), self.cache_dir.clone(), rx))?.abort_handle();

        self.wn_task = Some( WnTask{abort_handle, tx} );
        Ok(())
    }

    async fn add_client (&mut self, hself: ActorHandle<WindNinjaActorMsg>, request: AddWindNinjaClient)->Result<()> {
        let res = if let Some(fcr) = self.forecast_store.get_mut( &request.region) { // do we already have this region?
            if fcr.bbox != request.bbox { // check if coordinates are the same
                Err(OdinWindNinjaError::RegionInUseError(request))
                
            } else {
                fcr.n_clients += 1;
                Ok( AddClientResponse{ request, n_clients: fcr.n_clients})
            }

        } else { // new request
            if let Some(utm_rect) = utm::geo_to_utm_rect( &request.bbox) {
                match self.get_dem_file( request.region.as_str(), &utm_rect).await {
                    Ok(dem_path) => { // Ok, we have a DEM for the region, now start the HRRR forecast retrieval for it
                        info!("adding region for {request:?}");
                        let hrrr_ds_request = self.add_hrrr_region( &request).await?;

                        let mut fcr = ForecastRegion::new( 
                            Arc::new( request.region.clone()), 
                            request.bbox.clone(), 
                            Arc::new(dem_path), 
                            hrrr_ds_request,
                            self.config.max_forecasts
                        );
                        self.forecast_store.insert( fcr.region.clone(), fcr);

                        Ok( AddClientResponse{ request, n_clients: 1})
                    },
                    Err(e) => Err( OdinWindNinjaError::DemError(e.to_string()) )
                }

            } else {
                Err( OdinWindNinjaError::InvalidRegionError(request))
            }
        };

        self.subscribe_action.execute(res).await.map_err(|e| OdinWindNinjaError::ActionFailure(e.to_string()))

    }

    async fn get_dem_file (&self, region: &str, utm_rect: &UtmRect)->Result<PathBuf> {
        // TODO - should we compute instead of configure the resolution? we could compute from UTM bbox and the mesh resolution

        let fname = odin_dem::get_res_dem_filename( "dem", utm_rect.epsg(), &utm_rect.bbox, self.config.dem_res, self.config.dem_res, "tif");
        let path = self.cache_dir.join(fname);

        if path.is_file() { // we already have it in our cache
            return Ok(path)

        } else { // retrieve then cache
            let bbox = &utm_rect.bbox;
            let uri = format!("{}/GetResDem?crs=EPSG:{}&bbox={:.0},{:.0},{:.0},{:.0}&res_x={}&res_y={}&format=image/tif", 
                self.config.dem_url, utm_rect.epsg(), 
                bbox.west, bbox.south, bbox.east, bbox.north,
                self.config.dem_res, self.config.dem_res
            );
            let client = Client::new();

            match download_url( &client, &uri, &None, &path).await {
                Ok(len) => Ok(path),
                Err(e) => Err( OdinWindNinjaError::DemError( format!("DEM download failed: {e}")) )
            }
        }
    }

    async fn add_hrrr_region (&self, request: &AddWindNinjaClient)->Result<Arc<HrrrDataSetRequest>> {
        let mut hrrr_cfg = HrrrDataSetConfig::new( request.region.clone(), request.bbox.clone(), 
                                               self.config.hrrr_fields.clone(), self.config.hrrr_levels.clone());
        let hrrr_ds_request = Arc::new( HrrrDataSetRequest::new( hrrr_cfg) );

        self.hrrr.send_msg( AddDataSet( hrrr_ds_request.clone())).await?;

        Ok(hrrr_ds_request)
    }

    async fn schedule_wn_job (&self, hfa: HrrrFileAvailable)->Result<()> {
        if let Some(wn_task) = &self.wn_task {
            if let Some(fcr) = self.forecast_store.get( hfa.request.name()) {
                let region = fcr.region.clone();
                let step = hfa.request.step;   
                let date = hfa.request.base + hours(step as u64);
                let hrrr_path = Arc::new(hfa.path);
                let dem_path = fcr.dem_path.clone();

                if let Err(e) = send( &wn_task.tx, WnJob{region,date,step,hrrr_path,dem_path}).await {
                    error!("failed to queue WnJob {} at {}+{} : {e}", fcr.region, hfa.request.base, hfa.request.step);
                }
            }
        }
        Ok(())
    }

    fn terminate (&mut self) {
        if let Some(wn_task) = &self.wn_task {
            wn_task.abort_handle.abort();
            self.wn_task = None;
        }
    }
}

/// the response data for a successful subscription
/// (we can use this in the future to transmit session data or access keys)
#[derive(Debug)]
pub struct AddClientResponse {
    request: AddWindNinjaClient,
    n_clients: usize,
    // possibly more in the future
} 

async fn wn_loop (hself: ActorHandle<WindNinjaActorMsg>, config: Arc<WindNinjaConfig>, cache_dir: Arc<PathBuf>, rx: MpscReceiver<WnJob>) {
    loop {
        match recv(&rx).await {
            Ok(wn_job) => {
                info!("processing WnJob {} at {}", wn_job.region, short_utc_datetime_string( &wn_job.date));
                match run_wn( config.as_ref(), cache_dir.as_ref(), &wn_job) {
                    Ok( wn_path ) => {
                        info!("WindNinja forecast step available: {:?}", wn_path);
                        hself.send_msg( wn_job.into_forecast( Arc::new(wn_path))).await;
                    }
                    Err(e) => { 
                        error!("failed to process region {} at {}: {e}", wn_job.region, wn_job.date) 
                    }
                }
            }
            Err(_) => { break } // request queue closed, no use to go on
        }
    }
}

fn run_wn (config: &WindNinjaConfig, cache_dir: &PathBuf, wn_job: &WnJob) -> Result<PathBuf> {
    let output_path = cache_dir.join( wn_filename(&wn_job));
    let date = &wn_job.date;

    let mut cmd = Command::new( &config.windninja_path)
        .arg("--mesh_resolution")
        .arg(config.mesh_res.to_string())
        .arg("--units_mesh_resolution")
        .arg("m")
        .arg("--output_wind_height")
        .arg(config.wind_height.to_string())
        .arg("--units_output_wind_height")
        .arg("m")
        .arg("--elevation_file")
        .arg( wn_job.dem_path.as_os_str())
        .arg("--initialization_method")
        .arg("wxModelInitialization")
        .arg("--forecast_filename")
        .arg( wn_job.hrrr_path.as_os_str())  // wx model file name 
        .arg("--forecast_time")
        .arg( &wn_forecast_time(date)) // datetime string (UTC)
        .arg("--start_year")
        .arg( date.year().to_string())
        .arg("--start_month")
        .arg( date.month().to_string())
        .arg("--start_day")
        .arg( date.day().to_string())
        .arg("--start_hour")
        .arg( date.hour().to_string())
        .arg("--stop_year")
        .arg( date.year().to_string())
        .arg("--stop_month")
        .arg( date.month().to_string())
        .arg("--stop_day")
        .arg( date.day().to_string())
        .arg("--stop_hour")
        .arg( date.hour().to_string())
        .arg( "--write_goog_output")
        .arg( "false")
        .arg( "--write_shapefile_output")
        .arg( "false")
        .arg( "--write_pdf_output")
        .arg( "false")
        .arg( "--write_farsite_atm")
        .arg( "false")
        .arg( "--write_wx_model_goog_output")
        .arg( "false")
        .arg( "--write_wx_model_shapefile_output")
        .arg( "false")
        .arg( "--write_wx_model_ascii_output")
        .arg( "false")
        .arg( "--write_wx_station_kml")
        .arg( "false")
        .arg( "--write_huvw_output")
        .arg( "true")
        .arg("--diurnal_winds")
        .arg( "true")
        .arg( "--output_path")
        .arg( output_path.as_os_str());

    debug!("executing {cmd:?}");

    match cmd.spawn() {
        Ok(_) => Ok(output_path),
        Err(e) => Err( OdinWindNinjaError::ExecError(e.to_string())) 
    }
}

fn wn_forecast_time (date: &DateTime<Utc>)->String {
    format!("{:4}{:02}{:02}T{:02}0000", date.year(), date.month(), date.day(), date.hour()) // WindNinja assumes UTC (no zone)
}

fn wn_filename (wn_job: &WnJob)->PathBuf {
    let date = wn_job.date;
    Path::new( &format!("wind_{}_{}-{}-{}_{}.tif", wn_job.region, date.year(), date.month(), date.day(), date.hour())).to_path_buf()
}

/* #region WindNinja actor messages ****************************************************************/

#[derive(Debug)] 
pub struct AddWindNinjaClient {
    pub region: String,
    pub bbox: GeoRect
}
impl AddWindNinjaClient {
    pub fn new<T: ToString> (region: T, bbox: GeoRect)-> Self { AddWindNinjaClient { region: region.to_string(), bbox } }
}

#[derive(Debug)] 
pub struct RemoveWindNinjaClient (String);

/// external message to request action execution with the current HotspotStore
#[derive(Debug)] 
pub struct ExecSnapshotAction(pub DynDataRefAction<ForecastStore>);

define_actor_msg_set!{ pub WindNinjaActorMsg = AddWindNinjaClient | ExecSnapshotAction | RemoveWindNinjaClient | HrrrFileAvailable | Forecast }

/* #endregion WindNinja actor messages */

/* #region WindNinja actor impl ********************************************************************/

impl_actor! { match msg for Actor<WindNinjaActor<I,S,U>,WindNinjaActorMsg> 
    where I: DataRefAction<ForecastStore> + Sync, S: DataAction<Result<AddClientResponse>> + Sync, U: DataRefAction<Forecast> + Sync as

    _Start_ => cont! {
        let hself = self.hself.clone();
        self.start( hself);
    }

    // received from a client to start forecasts for the given area
    AddWindNinjaClient => cont! { 
        let hself = self.hself.clone();
        check_err( self.add_client( hself, msg).await, "failed to add windninja client")
    }

    // received from client to process snapshot of current data
    ExecSnapshotAction => cont! { 
        msg.0.execute( &self.forecast_store).await; 
    }

    // received from HrrrActor when new HRRR dataset for a monitored region is available. This kicks off WindNinja execution
    HrrrFileAvailable => cont! {
        check_err( self.schedule_wn_job( msg).await, "failed to queue forecast");
    }

    Forecast => cont! {
        println!("forecast ready: {msg:?}");
    }

    // received from client to stop forecasts for given area (if there are no other clients left)
    RemoveWindNinjaClient => cont! { }

    _Terminate_ => stop! { 
        self.terminate();
    }
}

/* #endregion WindNinja actor impl */