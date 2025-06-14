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

use std::{collections::{HashMap,HashSet}, net::SocketAddr, path::{Path,PathBuf}, sync::Arc};
use chrono::{DateTime,Datelike,Timelike, Utc};
use odin_dem::DemSRS;
use reqwest::{self, Client};
use tokio::process::Command;
use serde::{Serialize,Deserialize};

use odin_build::pkg_cache_dir;
use odin_common::{
    fs::{basename,replace_env_var_path,path_to_unchecked_string,path_str_to_fname},
    datetime::{hours,short_utc_datetime_string}, 
    geo::GeoRect, net::download_url, collections::RingDeque,
    utm::{self,UtmRect,UtmZone,UTM}
};
use odin_hrrr::{AddDataSet, RemoveDataSet, HrrrActorMsg, HrrrDataSetConfig, HrrrDataSetRequest, HrrrFileAvailable};
use odin_actor::prelude::*;
use odin_server::prelude::*;
use crate::{
    errors::{OdinWindError,Result},
    wind_service::WindService,
    Forecast, ForecastRegion, ForecastStore, WnJob, WindConfig, WX_HRRR,
};

//macro_rules! info { ($fmt:literal $(, $arg:expr )* ) => { {print!("INFO: "); println!( $fmt $(, $arg)* )} } }
//macro_rules! error { ($fmt:literal $(, $arg:expr )* ) => { {eprint!("\x1b[32;1m \x1b[37m ERROR: "); eprint!( $fmt $(, $arg)* ); eprintln!("\x1b[0m")} } }

// TODO - put HRRR behind a Wx abstraction to support other weather sources

struct WnTask {
    abort_handle: AbortHandle,
    tx: MpscSender<WnJob>
}

/// the WindActor state
pub struct WindActor<S,U> where S: DataAction<SubscribeResponse>, U: DataRefAction<Forecast>
{
    config: Arc<WindConfig>,

    // from config, with env-var pathelements expanded
    windninja_cmd: String,
    huvw_csv_grid_cmd: String,
    huvw_csv_vector_cmd: String,

    cache_dir: Arc<PathBuf>,         // where to store computed forecasts
    hrrr: ActorHandle<HrrrActorMsg>, // where to get new HRRR reports from - this drives our data update

    forecast_store: ForecastStore,

    subscribe_action: S,
    update_action: U,

    wn_task: Option<WnTask>
}

impl <S,U> WindActor<S,U> where S: DataAction<SubscribeResponse>, U: DataRefAction<Forecast> {
    pub fn new (config: WindConfig, hrrr: ActorHandle<HrrrActorMsg>, subscribe_action: S, update_action: U)->Self {
        
         let windninja_cmd = path_to_unchecked_string( replace_env_var_path( &config.windninja_cmd).unwrap()); // Ok to panic - this is the ctor
         let huvw_csv_grid_cmd = path_to_unchecked_string( replace_env_var_path( &config.huvw_csv_grid_cmd).unwrap());
         let huvw_csv_vector_cmd = path_to_unchecked_string( replace_env_var_path( &config.huvw_csv_vector_cmd).unwrap());

        let config = Arc::new(config);
        let cache_dir = Arc::new(pkg_cache_dir!());
        let forecast_store = HashMap::new();
 
        WindActor { 
            config,
            windninja_cmd, huvw_csv_grid_cmd, huvw_csv_vector_cmd, 
            cache_dir, 
            hrrr, 
            forecast_store, 
            subscribe_action, update_action, 
            wn_task: None
        }
    }

    fn start (&mut self, hself: ActorHandle<WindActorMsg>)->Result<()> {
        let (tx, rx) = create_mpsc_sender_receiver::<WnJob>(64);
        let abort_handle = spawn("wn_task", wn_loop( hself, self.windninja_cmd.clone(), self.cache_dir.clone(), rx))?.abort_handle();

        self.wn_task = Some( WnTask{abort_handle, tx} );
        Ok(())
    }

    async fn add_client (&mut self, hself: ActorHandle<WindActorMsg>, request: AddWindClient)->Result<()> {
        let rr = &request.wn_region;

        let maybe_rejection: Option<String> = if let Some(fcr) = self.forecast_store.get_mut( &rr.name) { // do we already have this region?
            if fcr.bbox != rr.bbox { // check if coordinates are the same
                Some("region in use".to_string())
                
            } else {
                fcr.add_client(request.remote_addr);
                Some("active".to_string()) // standard response if a client selects region
            }

        } else { // new region request
            if let Some(mut utm_rect) = utm::geo_to_utm_rect( &rr.bbox) {
                utm_rect.round(); // we only need meters
                match self.get_dem_file( rr.name.as_str(), &utm_rect).await {
                    Ok(dem_path) => { // Ok, we have a DEM for the region, now start the HRRR forecast retrieval for it
                        info!("adding region for {rr:?}");
                        let hrrr_ds_request = self.add_hrrr_region( rr).await?;

                        let mut fcr = ForecastRegion::new( 
                            Arc::new( rr.name.clone()), 
                            rr.bbox.clone(),
                            utm_rect.clone(), 
                            Arc::new(dem_path), 
                            hrrr_ds_request,
                            self.config.max_forecasts
                        );
                        fcr.add_client(request.remote_addr);
                        self.forecast_store.insert( fcr.region.clone(), fcr);

                        None // no rejection
                    },
                    Err(e) => Some("no elevation data".to_string())
                }

            } else {
                Some("invalid region".to_string())
            }
        };

        let response = SubscribeResponse::Add(AddClientResponse { 
            wn_region: request.wn_region, 
            rejection: maybe_rejection,
            remote_addr: request.remote_addr 
        });
        self.subscribe_action.execute(response).await.map_err(|e| OdinWindError::ActionFailure(e.to_string()))

    }

    async fn remove_client( &mut self, hself: ActorHandle<WindActorMsg>, request: RemoveWindClient)->Result<()> {

        if let Some(region) = &request.region {
            if let Some(fr) = self.forecast_store.get_mut( region) {
                if fr.remove_client( &request.remote_addr) && (fr.n_clients == 0) {
                    self.remove_region( region).await?
                }
            }
        } else { // we have to check all our regions
            let mut dropped: Vec<Arc<String>> = Vec::new();
            for (region,fr) in self.forecast_store.iter_mut() {
                if fr.remove_client( &request.remote_addr) && (fr.n_clients == 0) { dropped.push( fr.region.clone()); }
            }
            for region in dropped {
                self.remove_region( region.as_ref()).await?
            }
        }

        Ok(())
    }

    async fn remove_region (&mut self, region: &String)->Result<()> {
        if let Some(fr) = self.forecast_store.remove(region) {
            info!("removed region {}", region);
            self.hrrr.send_msg( RemoveDataSet(fr.hrrr_ds_request)).await?;

            let response = SubscribeResponse::Remove( RemoveClientResponse{region: region.to_string()} );
            self.subscribe_action.execute(response).await.map_err(|e| OdinWindError::ActionFailure(e.to_string()))
        } else { Ok(()) }
    }

    async fn get_dem_file (&self, region: &str, utm_rect: &UtmRect)->Result<PathBuf> {
        // TODO - should we compute instead of configure the resolution? we could compute from UTM bbox and the mesh resolution

        let fname = wn_dem_filename(region, &utm_rect);
        let path = self.cache_dir.join(fname);

        if path.is_file() { // we already have it in our cache
            return Ok(path)

        } else { // retrieve
            let bbox = &utm_rect.bbox;
            let epsg = utm_rect.epsg();
            let dem_res = self.config.dem_res;
            let srs = DemSRS::UTM{epsg};

            match self.config.dem.get_res_dem( bbox, srs, dem_res, dem_res, odin_dem::DemImgType::TIF, &path).await {
                Ok(()) => Ok(path),
                Err(e) => {
                    error!("DEM download failed with {e}");
                    Err( OdinWindError::DemError( format!("DEM download failed: {e}")) )
                }
            }
        }
    }

    async fn add_hrrr_region (&self, wn_region: &WindRegion)->Result<Arc<HrrrDataSetRequest>> {
        let mut bbox = wn_region.bbox.add_degrees( -0.25, -0.25, 0.25, 0.25); // make sure we cover the bbox
        let region = wn_region.name.clone();
        let mut hrrr_cfg = HrrrDataSetConfig::new( region, bbox,  self.config.hrrr_fields.clone(), self.config.hrrr_levels.clone());
        let hrrr_ds_request = Arc::new( HrrrDataSetRequest::new( hrrr_cfg) );

        self.hrrr.send_msg( AddDataSet( hrrr_ds_request.clone())).await?;

        Ok(hrrr_ds_request)
    }

    async fn schedule_wn_job (&self, hfa: HrrrFileAvailable)->Result<()> {
        if let Some(wn_task) = &self.wn_task {
            if let Some(fcr) = self.forecast_store.get( hfa.request.name()) {
                if fcr.n_clients > 0 { // maybe it got unsubscribed in the meantime
                    let region = fcr.region.clone();
                    let step = hfa.request.step;   
                    let mesh_res = self.config.mesh_res;
                    let wind_height = self.config.wind_height;
                    let date = hfa.request.base + hours(step as u64);
                    let wx_path = Arc::new(hfa.path);
                    let dem_path = fcr.dem_path.clone();

                    let rn = path_str_to_fname( &fcr.region);
                    let bbox = &fcr.utm_rect.bbox;
                    let wn_out_basename = Arc::new( format!("{}_{:.0}_{:.0}_{:.0}_{:.0}_{:02}-{:02}-{:04}_{:02}{:02}_{}m_huvw", 
                                rn, bbox.west, bbox.south, bbox.east, bbox.north,
                                date.month(), date.day(), date.year(), date.hour(), date.minute(), mesh_res));
                    let wx_src = WX_HRRR.clone(); // FIXME - this shouldn't be hardcoded (there will be other sources)

                    if let Err(e) = send( &wn_task.tx, WnJob{region, date, step, mesh_res, wind_height, wx_src, wx_path, dem_path, wn_out_basename}).await {
                        error!("failed to queue WnJob {} at {}+{} : {e}", fcr.region, hfa.request.base, hfa.request.step);
                    }
                }
            }
        }
        Ok(())
    }
    
    async fn process_forecast (&mut self, forecast: &Forecast)->Result<()> {
        let huvw_grid = create_huvw_csv_grid( &self.huvw_csv_grid_cmd, &forecast).await?;
        let huvw_vector = create_huvw_csv_vector( &self.huvw_csv_vector_cmd, &forecast).await?;
        // TODO - add contour and HRRR grid computation here

        self.add_forecast( forecast.clone());

        info!("completed forecast {:?}", forecast);
        Ok( self.update_action.execute( &forecast).await.map_err(|e| OdinWindError::ActionFailure(e.to_string()))? )
    }

    fn add_forecast(&mut self, forecast: Forecast) {
        if let Some(fcr) = self.forecast_store.get_mut( &forecast.region) {
            let mut fcs = &mut fcr.forecasts;
            for i in 0..fcs.len() {
                let f = &fcs[i];
                if f.date == forecast.date  { 
                    if forecast.step < f.step {  // this replaces an older, now obsolete forecast for the same hour
                        fcs[i] = forecast;
                    } else {
                        warn!("ignoring dead-on-arrival forecast {:?}", forecast);
                    }
                    return
                } else if f.date > forecast.date { 
                    warn!("inserting previously missing forecast {:?}", forecast);
                    fcs.insert_into_ringbuffer(i, forecast);
                    return
                }
            }
            // if we get here we append (and possibly drop the first forecast)
            fcs.push_to_ringbuffer( forecast);
        }
    }

    fn terminate (&mut self) {
        if let Some(wn_task) = &self.wn_task {
            wn_task.abort_handle.abort();
            self.wn_task = None;
        }
    }
}

#[derive(Debug)]
pub enum SubscribeResponse {
    Add(AddClientResponse),
    Remove(RemoveClientResponse)
}

/// the response to a AddWindClient message. This is fed into the subscribe_action
#[derive(Debug,Serialize)]
pub struct AddClientResponse {
    pub wn_region: WindRegion,
    pub rejection: Option<String>, // if None then region was accepted

    #[serde(skip_serializing)] // SocketAddr is internal
    pub remote_addr: Option<SocketAddr>
}

#[derive(Debug,Serialize)]
pub struct RemoveClientResponse {
    pub region: String,
}

async fn wn_loop (hself: ActorHandle<WindActorMsg>, windninja_cmd: String, cache_dir: Arc<PathBuf>, rx: MpscReceiver<WnJob>) {
    loop {
        match recv(&rx).await {
            Ok(wn_job) => {
                info!("processing WnJob {} at {}", wn_job.region, short_utc_datetime_string( &wn_job.date));
                match run_wn( &windninja_cmd, cache_dir.as_ref(), &wn_job).await {
                    Ok(()) => {
                        info!("Wind forecast step available: {:?}", wn_job);
                        hself.send_msg( Forecast::from(wn_job)).await;
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

async fn run_wn (windninja_cmd: &String, cache_dir: &PathBuf, wn_job: &WnJob) -> Result<()> {
    let date = &wn_job.date;

    let mut cmd = Command::new( windninja_cmd);
    cmd
        .arg("--mesh_resolution").arg(wn_job.mesh_res.to_string())
        .arg("--units_mesh_resolution").arg("m")
        .arg("--output_wind_height").arg(wn_job.wind_height.to_string())
        .arg("--units_output_wind_height").arg("m")
        .arg("--elevation_file").arg( wn_job.dem_path.as_os_str())
        .arg("--vegetation").arg("trees")  // FIXME - this should be computed from landfire.gov (grass,brush,trees)
        .arg("--initialization_method").arg("wxModelInitialization")
        .arg("--time_zone").arg("UTC")
        .arg("--forecast_filename").arg( wn_job.wx_path.as_os_str())  // wx model file name 
        .arg("--forecast_time").arg( &wn_forecast_time(date)) // datetime string (UTC)
        .arg("--start_year").arg( date.year().to_string())
        .arg("--start_month").arg( date.month().to_string())
        .arg("--start_day").arg( date.day().to_string())
        .arg("--start_hour").arg( date.hour().to_string())
        .arg("--stop_year").arg( date.year().to_string())
        .arg("--stop_month").arg( date.month().to_string())
        .arg("--stop_day").arg( date.day().to_string())
        .arg("--stop_hour").arg( date.hour().to_string())
        .arg( "--write_goog_output").arg( "false")
        .arg( "--write_shapefile_output").arg( "false")
        .arg( "--write_pdf_output").arg( "false")
        .arg( "--write_farsite_atm").arg( "false")
        .arg( "--write_wx_model_goog_output").arg( "false")
        .arg( "--write_wx_model_shapefile_output").arg( "false")
        .arg( "--write_wx_model_ascii_output").arg( "false")
        .arg( "--write_wx_station_kml").arg( "false")
        .arg( "--write_huvw_output").arg( "true")
        //.arg( "--write_huvw_0_output").arg( "true") // this only makes sense if we need the same grid points (e.g. for diffs)
        .arg("--diurnal_winds").arg( "true")
        .arg( "--output_path").arg( cache_dir.as_os_str());

    execute_cmd( &mut cmd).await
}

async fn execute_cmd( cmd: &mut Command) -> Result<()> {
    debug!("executing {cmd:?}");

    match cmd.spawn() {
        Ok(mut child) => {
            match child.wait().await {
                Ok(status) => {
                    info!("{:?} completed with status {}", cmd.as_std().get_program(), status);
                    Ok(())
                }
                Err(e) => Err( OdinWindError::ExecError(e.to_string()))
            }
        }
        Err(e) => Err( OdinWindError::ExecError(e.to_string())) 
    }
}

fn wn_forecast_time (date: &DateTime<Utc>)->String {
    format!("{:4}{:02}{:02}T{:02}0000", date.year(), date.month(), date.day(), date.hour()) // Wind assumes UTC (no zone)
}

fn wn_dem_filename (region: &str, utm_rect: &UtmRect)->PathBuf {
    let rn = path_str_to_fname( region);
    let bbox = &utm_rect.bbox;
    Path::new( &format!("{}_{:.0}_{:.0}_{:.0}_{:.0}.tif", rn, bbox.west,bbox.south,bbox.east,bbox.north)).to_path_buf()
}

//--- the public output files generated from the Wind huvw UTM grid file

/// this takes the Wind_cli generated huvw UTM grid and turns it into a WGS84 grid formatted as CSV. Since this conversion
/// creates no_data edge artifacts that would throw off particle animation and other visualization we have to not only warp to
/// epsg:4326 but also crop the grid so that it only contains defined data values. Note the CSV file contains a '#' prefixed meta
/// info line to define the lon/lat grid i.e. it might not be processed correctly by external programs.
/// TODO - ultimately we want to do this within process but since input and output are both just files we use a child process for now
async fn create_huvw_csv_grid (cmd: &String, forecast: &Forecast) -> Result<()> {
    let in_path = forecast.get_huvw_utm_grid_path();
    if !in_path.is_file() { return Err(OdinWindError::ExecError(format!("no such Wind output file {:?}", in_path))) }
    let out_path = forecast.get_huvw_grid_path();

    exec_huvw_csv_gen( cmd, &in_path, &out_path).await
}

/// this takes the Wind_cli generated huvw UTM grid and turns it into a list of ECEF vectors formatted as CSV.
async fn create_huvw_csv_vector (cmd: &String, forecast: &Forecast) -> Result<()> {
    let in_path = forecast.get_huvw_utm_grid_path();
    if !in_path.is_file() { return Err(OdinWindError::ExecError(format!("no such Wind output file {:?}", in_path))) }
    let out_path = forecast.get_huvw_vector_path();

    exec_huvw_csv_gen( cmd, &in_path, &out_path).await
}

async fn create_huvw_json_contour (cmd: &String, forecast: &Forecast) -> Result<()> {
    let in_path = forecast.get_huvw_utm_grid_path();
    if !in_path.is_file() { return Err(OdinWindError::ExecError(format!("no such Wind output file {:?}", in_path))) }
    let out_path = forecast.get_huvw_contour_path();

    exec_huvw_csv_gen( cmd, &in_path, &out_path).await
}

async fn exec_huvw_csv_gen (cmd_path: &String, in_path: &PathBuf, out_path: &PathBuf) -> Result<()> {
    let mut cmd = Command::new(cmd_path);
    cmd
        .arg( "-z") // compress output
        .arg( in_path.as_os_str()) // the input file
        .arg( out_path.as_os_str());

    execute_cmd( &mut cmd).await?;
    Ok(())
}

async fn run_huvw_csv_contour (config: &WindConfig, cache_dir: &PathBuf, huvw_utm_grid: &PathBuf, wn_job: &WnJob) -> Result<PathBuf> {
    todo!()
}

/* #region Wind actor messages ****************************************************************/

#[derive(Debug,Serialize,Deserialize)] 
pub struct WindRegion {
    pub name: String,
    pub bbox: GeoRect,
}

#[derive(Debug)]
pub struct AddWindClient {
    pub wn_region: WindRegion,
    pub remote_addr: Option<SocketAddr>
}

impl AddWindClient {
    pub fn new<T: ToString> (name: T, bbox: GeoRect, remote_addr: Option<SocketAddr>)-> Self { 
        let name = name.to_string();
        AddWindClient { wn_region: WindRegion{name, bbox}, remote_addr } 
    }
}

#[derive(Debug)] 
pub struct RemoveWindClient {
    pub region: Option<String>, // of region
    pub remote_addr: Option<SocketAddr>
}

/// external message to request action execution with the current HotspotStore
#[derive(Debug)] 
pub struct ExecSnapshotAction(pub DynDataRefAction<ForecastStore>);

define_actor_msg_set!{ pub WindActorMsg = AddWindClient | ExecSnapshotAction | RemoveWindClient | HrrrFileAvailable | Forecast }

/* #endregion Wind actor messages */

/* #region Wind actor impl ********************************************************************/

impl_actor! { match msg for Actor<WindActor<S,U>,WindActorMsg> 
    where S: DataAction<SubscribeResponse> + Sync, U: DataRefAction<Forecast> + Sync as

    _Start_ => cont! {
        let hself = self.hself.clone();
        self.start( hself);
    }

    // received from a client to start forecasts for the given area
    AddWindClient => cont! { 
        let hself = self.hself.clone();
        check_err( self.add_client( hself, msg).await, "failed to add windninja client")
    }

    // received from client to process snapshot of current data
    ExecSnapshotAction => cont! { 
        msg.0.execute( &self.forecast_store).await; 
    }

    // received from HrrrActor when new HRRR dataset for a monitored region is available. This kicks off Wind execution
    HrrrFileAvailable => cont! {
        check_err( self.schedule_wn_job( msg).await, "failed to queue forecast");
    }

    Forecast => cont! {
        check_err(self.process_forecast( &msg).await, "failed to process forecast");
    }

    // received from client to stop forecasts for given area (if there are no other clients left)
    RemoveWindClient => cont! { 
        let hself = self.hself.clone();
        check_err( self.remove_client( hself, msg).await, "failed to remove windninja client")
    }

    _Terminate_ => stop! { 
        self.terminate();
    }
}

/* #endregion Wind actor impl */

/// standard subscribe action that sends/broadcasts add/rejectForecastRegion websocket messages through a SpaServer
pub fn server_subscribe_action (hserver: ActorHandle<SpaServerMsg>) -> impl DataAction<SubscribeResponse> {
    data_action!( let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |response: SubscribeResponse| {
        match response {
            SubscribeResponse::Add(response) => {
                if let Some(cause) = &response.rejection {
                    // tell only the requester why it was rejected
                    if let Some(remote_addr) = response.remote_addr {
                        let json =  serde_json::to_string( &response)?;
                        let ws_msg = ws_msg_from_json( WindService::mod_path(), "rejectForecastRegion", &json);
                        hserver.send_msg( SendWsMsg{ remote_addr, ws_msg}).await;
                    }
                } else {
                    // tell everybody there is a new forecast region
                    let json = serde_json::to_string( &response.wn_region)?;
                    let ws_msg = ws_msg_from_json( WindService::mod_path(), "startForecastRegion", &json);
                    hserver.send_msg( BroadcastWsMsg{ws_msg}).await;
                }
            }
            SubscribeResponse::Remove(response) => {
                let json = serde_json::to_string( &response)?;
                let ws_msg = ws_msg_from_json( WindService::mod_path(), "stopForecastRegion", &json);
                hserver.send_msg( BroadcastWsMsg{ws_msg}).await;
            }
        }
        Ok(())
    })
}

/// standard update action that broadcasts `forecast` websocket messages through a SpaServer
pub fn server_update_action (hserver: ActorHandle<SpaServerMsg>) -> impl DataRefAction<Forecast> {
    dataref_action!( let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |forecast: &Forecast| {  // update action
        let json = forecast.to_json();
        let ws_msg = ws_msg_from_json( WindService::mod_path(), "forecast", &json);
        hserver.send_msg( BroadcastWsMsg{ws_msg}).await;
        Ok(())
    })
}
