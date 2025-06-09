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

use std::{path::{Path,PathBuf}, sync::Arc, collections::HashMap};
use chrono::{DateTime,Datelike,Timelike, Utc};
use odin_dem::DemSRS;
use reqwest::{self, Client};
use tokio::process::Command;

use odin_build::pkg_cache_dir;
use odin_common::{
    fs::{basename,replace_env_var_path,path_to_unchecked_string},
    datetime::{hours,short_utc_datetime_string}, 
    geo::GeoRect, net::download_url, collections::RingDeque,
    utm::{self,UtmRect,UtmZone,UTM}
};
use odin_hrrr::{AddDataSet, HrrrActorMsg, HrrrDataSetConfig, HrrrDataSetRequest, HrrrFileAvailable};
use odin_actor::prelude::*;
use crate::{errors::{OdinWindNinjaError,Result}, Forecast, ForecastRegion, ForecastStore, WnJob, WindNinjaConfig};

//macro_rules! info { ($fmt:literal $(, $arg:expr )* ) => { {print!("INFO: "); println!( $fmt $(, $arg)* )} } }
//macro_rules! error { ($fmt:literal $(, $arg:expr )* ) => { {eprint!("\x1b[32;1m \x1b[37m ERROR: "); eprint!( $fmt $(, $arg)* ); eprintln!("\x1b[0m")} } }


struct WnTask {
    abort_handle: AbortHandle,
    tx: MpscSender<WnJob>
}

/// the WindNinjaActor state
pub struct WindNinjaActor<I,S,U> where I: DataRefAction<ForecastStore>, S: DataAction<Result<AddClientResponse>>, U: DataRefAction<Forecast>
{
    config: Arc<WindNinjaConfig>,

    // from config, with env-var pathelements expanded
    windninja_cmd: String,
    huvw_csv_grid_cmd: String,
    huvw_csv_vector_cmd: String,

    cache_dir: Arc<PathBuf>,         // where to store computed forecasts
    hrrr: ActorHandle<HrrrActorMsg>, // where to get new HRRR reports from - this drives our data update

    forecast_store: ForecastStore,

    init_action: I,
    subscribe_action: S,
    update_action: U,

    wn_task: Option<WnTask>
}

impl <I,S,U> WindNinjaActor<I,S,U> where I: DataRefAction<ForecastStore>, S: DataAction<Result<AddClientResponse>>, U: DataRefAction<Forecast> {
    pub fn new (config: WindNinjaConfig, hrrr: ActorHandle<HrrrActorMsg>, init_action: I, subscribe_action: S, update_action: U)->Self {
        
         let windninja_cmd = path_to_unchecked_string( replace_env_var_path( &config.windninja_cmd).unwrap()); // Ok to panic - this is the ctor
         let huvw_csv_grid_cmd = path_to_unchecked_string( replace_env_var_path( &config.huvw_csv_grid_cmd).unwrap());
         let huvw_csv_vector_cmd = path_to_unchecked_string( replace_env_var_path( &config.huvw_csv_vector_cmd).unwrap());

        let config = Arc::new(config);
        let cache_dir = Arc::new(pkg_cache_dir!());
        let forecast_store = HashMap::new();
 
        WindNinjaActor { 
            config,
            windninja_cmd, huvw_csv_grid_cmd, huvw_csv_vector_cmd, 
            cache_dir, 
            hrrr, 
            forecast_store, 
            init_action, subscribe_action, update_action, 
            wn_task: None
        }
    }

    fn start (&mut self, hself: ActorHandle<WindNinjaActorMsg>)->Result<()> {
        let (tx, rx) = create_mpsc_sender_receiver::<WnJob>(64);
        let abort_handle = spawn("wn_task", wn_loop( hself, self.windninja_cmd.clone(), self.cache_dir.clone(), rx))?.abort_handle();

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
            if let Some(mut utm_rect) = utm::geo_to_utm_rect( &request.bbox) {
                utm_rect.round(); // we only need meters
                match self.get_dem_file( request.region.as_str(), &utm_rect).await {
                    Ok(dem_path) => { // Ok, we have a DEM for the region, now start the HRRR forecast retrieval for it
                        info!("adding region for {request:?}");
                        let hrrr_ds_request = self.add_hrrr_region( &request).await?;

                        let mut fcr = ForecastRegion::new( 
                            Arc::new( request.region.clone()), 
                            request.bbox.clone(),
                            utm_rect.clone(), 
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
                    Err( OdinWindNinjaError::DemError( format!("DEM download failed: {e}")) )
                }
            }
        }
    }

    async fn add_hrrr_region (&self, request: &AddWindNinjaClient)->Result<Arc<HrrrDataSetRequest>> {
        let mut bbox = request.bbox.add_degrees( -0.5, -0.5, 0.5, 0.5); // make sure we cover the bbox
        let mut hrrr_cfg = HrrrDataSetConfig::new( request.region.clone(), bbox, 
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
                let mesh_res = self.config.mesh_res;
                let wind_height = self.config.wind_height;
                let date = hfa.request.base + hours(step as u64);
                let hrrr_path = Arc::new(hfa.path);
                let dem_path = fcr.dem_path.clone();

                let bbox = &fcr.utm_rect.bbox;
                let wn_out_basename = Arc::new( format!("{}_{:.0}_{:.0}_{:.0}_{:.0}_{:02}-{:02}-{:04}_{:02}{:02}_{}m_huvw", 
                            region, bbox.west, bbox.south, bbox.east, bbox.north,
                            date.month(), date.day(), date.year(), date.hour(), date.minute(), mesh_res));

                if let Err(e) = send( &wn_task.tx, WnJob{region, date, step, mesh_res, wind_height, hrrr_path, dem_path, wn_out_basename}).await {
                    error!("failed to queue WnJob {} at {}+{} : {e}", fcr.region, hfa.request.base, hfa.request.step);
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
        Ok( self.update_action.execute( &forecast).await.map_err(|e| OdinWindNinjaError::ActionFailure(e.to_string()))? )
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

/// the response data for a successful subscription
/// (we can use this in the future to transmit session data or access keys)
#[derive(Debug)]
pub struct AddClientResponse {
    request: AddWindNinjaClient,
    n_clients: usize,
    // possibly more in the future
} 

async fn wn_loop (hself: ActorHandle<WindNinjaActorMsg>, windninja_cmd: String, cache_dir: Arc<PathBuf>, rx: MpscReceiver<WnJob>) {
    loop {
        match recv(&rx).await {
            Ok(wn_job) => {
                info!("processing WnJob {} at {}", wn_job.region, short_utc_datetime_string( &wn_job.date));
                match run_wn( &windninja_cmd, cache_dir.as_ref(), &wn_job).await {
                    Ok(()) => {
                        info!("WindNinja forecast step available: {:?}", wn_job);
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
        .arg("--forecast_filename").arg( wn_job.hrrr_path.as_os_str())  // wx model file name 
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
                Err(e) => Err( OdinWindNinjaError::ExecError(e.to_string()))
            }
        }
        Err(e) => Err( OdinWindNinjaError::ExecError(e.to_string())) 
    }
}

fn wn_forecast_time (date: &DateTime<Utc>)->String {
    format!("{:4}{:02}{:02}T{:02}0000", date.year(), date.month(), date.day(), date.hour()) // WindNinja assumes UTC (no zone)
}

fn wn_dem_filename (region: &str, utm_rect: &UtmRect)->PathBuf {
    let bbox = &utm_rect.bbox;
    Path::new( &format!("{}_{:.0}_{:.0}_{:.0}_{:.0}.tif", region, bbox.west,bbox.south,bbox.east,bbox.north)).to_path_buf()
}


//--- the public output files generated from the WindNinja huvw UTM grid file

/// this takes the WindNinja_cli generated huvw UTM grid and turns it into a WGS84 grid formatted as CSV. Since this conversion
/// creates no_data edge artifacts that would throw off particle animation and other visualization we have to not only warp to
/// epsg:4326 but also crop the grid so that it only contains defined data values. Note the CSV file contains a '#' prefixed meta
/// info line to define the lon/lat grid i.e. it might not be processed correctly by external programs.
/// TODO - ultimately we want to do this within process but since input and output are both just files we use a child process for now
async fn create_huvw_csv_grid (cmd: &String, forecast: &Forecast) -> Result<()> {
    let in_path = forecast.get_huvw_utm_grid_path();
    println!("@@@ processing {cmd} from {in_path:?}  {}", in_path.is_file());

    if !in_path.is_file() { return Err(OdinWindNinjaError::ExecError(format!("no such WindNinja output file {:?}", in_path))) }


    let out_path = forecast.get_huvw_grid_path();

    exec_huvw_csv_gen( cmd, &in_path, &out_path).await
}

/// this takes the WindNinja_cli generated huvw UTM grid and turns it into a list of ECEF vectors formatted as CSV.
async fn create_huvw_csv_vector (cmd: &String, forecast: &Forecast) -> Result<()> {
    let in_path = forecast.get_huvw_utm_grid_path();
    if !in_path.is_file() { return Err(OdinWindNinjaError::ExecError(format!("no such WindNinja output file {:?}", in_path))) }
    let out_path = forecast.get_huvw_vector_path();

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

async fn run_huvw_csv_contour (config: &WindNinjaConfig, cache_dir: &PathBuf, huvw_utm_grid: &PathBuf, wn_job: &WnJob) -> Result<PathBuf> {
    todo!()
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
        check_err(self.process_forecast( &msg).await, "failed to process forecast");
    }

    // received from client to stop forecasts for given area (if there are no other clients left)
    RemoveWindNinjaClient => cont! { 
        // todo
    }

    _Terminate_ => stop! { 
        self.terminate();
    }
}

/* #endregion WindNinja actor impl */