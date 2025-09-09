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

use std::{collections::{HashMap,HashSet}, net::SocketAddr, path::{Path,PathBuf}, sync::Arc, fs::remove_file};
use chrono::{DateTime,Datelike,Timelike, Utc};
use odin_dem::DemSRS;
use reqwest::{self, Client};
use tokio::process::Command;
use serde::{Serialize,Deserialize};

use odin_build::pkg_cache_dir;
use odin_common::{
    collections::RingDeque, datetime::{hours,short_utc_datetime_string}, fs::{basename, gzip_path, odin_data_filename, path_str_to_fname, path_to_unchecked_string, remove_old_files, replace_env_var_path, set_accessed}, geo::GeoRect, pow2, sqrt, utm::{self,UtmRect,UtmZone,UTM}, BoundingBox 
};
use odin_hrrr::{AddDataSet, RemoveDataSet, HrrrActorMsg, HrrrDataSetConfig, HrrrDataSetRequest, HrrrFileAvailable};
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_gdal::{
    gdal::Dataset, GdalType, RasterInfo, 
    copy_full_rasterband, compute_rasterband_lines, get_raster_info, rasterband_index_for, 
    warp::{warp_to_raster_info, warp_to_rect, ResampleAlg}
};
use crate::{
    errors::{OdinWindError,Result}, 
    get_tmp_path, hrrr_10_contour_suffix, hrrr_10_grid_suffix, hrrr_10_vector_suffix, hrrr_80_contour_suffix, 
    hrrr_80_grid_suffix, hrrr_80_vector_suffix, hrrr_wgs84_suffix, huvw_contour_suffix, huvw_grid_suffix, 
    huvw_vector_suffix, huvw_wgs84_suffix, write_huvw_csv_cell_vectors, write_huvw_csv_grid, write_windspeed_contour, 
    wind_service::{self, WindService}, 
    AddWindClient, AddWindClientResponse, ExecSnapshotAction, Forecast, ForecastRegion, ForecastStore, 
    RemoveWindClient, RemoveWindClientResponse, SubscribeResponse, WindConfig, WindRegion, WnJob, WnJobRegion, WX_HRRR 
};

//macro_rules! info { ($fmt:literal $(, $arg:expr )* ) => { {print!("INFO: "); println!( $fmt $(, $arg)* )} } }
//macro_rules! error { ($fmt:literal $(, $arg:expr )* ) => { {eprint!("\x1b[32;1m \x1b[37m ERROR: "); eprint!( $fmt $(, $arg)* ); eprintln!("\x1b[0m")} } }

// TODO - put HRRR behind a Wx abstraction to support other weather sources

struct WnTask {
    join_handle: JoinHandle<()>,
    tx: MpscSender<WnJob>
}

/// the WindActor state. This reflects the data flow:
/// 
///  - client-request (from websocket/WindService) -> send HrrrDataSetRequest to HrrrActor
///  - HrrFileAvailable (from HrrrActor) -> schedule WnJob (to be processed sequentially by WnTask/WindNinja)
///  - Forecast (from WnTask) -> notify clients (through websocket)
pub struct WindActor<S,U> where S: DataAction<SubscribeResponse>, U: DataRefAction<Forecast>
{
    config: Arc<WindConfig>,

    // from config, with env-var pathelements expanded
    windninja_cmd: String,

    cache_dir: Arc<PathBuf>,         // where to store computed forecasts
    hrrr: ActorHandle<HrrrActorMsg>, // where to get new HRRR reports from - this drives our data update

    wn_job_regions: HashMap<Arc<String>,WnJobRegion>, // data we need to schedule WnJobs once we get HrrrDataAvailable notifications from an HrrrActor
    forecast_store: ForecastStore, // data we need to store Forecast data and to inform clients once we get Forecast notifications from the WnTask

    subscribe_action: S,
    update_action: U,

    wn_task: Option<WnTask>,
    timer: Option<AbortHandle>,
}

impl <S,U> WindActor<S,U> where S: DataAction<SubscribeResponse>, U: DataRefAction<Forecast> {
    pub fn new (config: WindConfig, hrrr: ActorHandle<HrrrActorMsg>, subscribe_action: S, update_action: U)->Self {
        
         let windninja_cmd = path_to_unchecked_string( replace_env_var_path( &config.windninja_cmd).unwrap()); // Ok to panic - this is the ctor
         // TODO - we should check here if comands are valid executables

        let config = Arc::new(config);
        let cache_dir = Arc::new(pkg_cache_dir!());
        let wn_job_regions = HashMap::new();
        let forecast_store = HashMap::new();
 
        WindActor { 
            config,
            windninja_cmd,
            cache_dir, 
            hrrr, 
            wn_job_regions,
            forecast_store, 
            subscribe_action, update_action, 
            wn_task: None,
            timer: None,
        }
    }

    fn start (&mut self, hself: ActorHandle<WindActorMsg>)->Result<()> {
        let (tx, rx) = create_mpsc_sender_receiver::<WnJob>(64);
        let join_handle = spawn("wn_task", wn_loop( hself, self.windninja_cmd.clone(), self.cache_dir.clone(), rx))?;

        self.wn_task = Some( WnTask{join_handle, tx} );
        Ok(())
    }

    async fn add_client (&mut self, hself: ActorHandle<WindActorMsg>, request: AddWindClient)->Result<()> {
        let rr = &request.wn_region;
        let mut rejection: Option<String> = None;
        let mut is_new = false;

        if let Some(fcr) = self.forecast_store.get_mut( &rr.name) { // we already monitor this region but client might be new
            if fcr.bbox != rr.bbox { // check if coordinates are the same
                rejection = Some("region in use".to_string())
            } else {
                fcr.add_client( request.remote_addr); // already monitored, just add new client
            }

        } else { // new region request -> get utm rect, send HRRR region request and add ForecastRegion to our store
            if let Some(mut utm_rect) = utm::geo_to_utm_rect( &rr.bbox) {
                utm_rect.round(); // we only need meters
                match self.get_dem_file( rr.name.as_str(), &utm_rect).await {
                    Ok(dem_path) => { // Ok, we have a DEM for the region, now start the HRRR forecast retrieval for it and register the region
                        info!("adding region for {rr:?}");
                        let region = Arc::new( rr.name.clone());
                        let hrrr_ds_request = self.add_hrrr_region( rr).await?;
                        let dem_path = Arc::new(dem_path);

                        let wri = WnJobRegion { region, dem_path, utm_rect, hrrr_ds_request };
                        let mut fcr = ForecastRegion::new( wri.region.clone(), rr.bbox.clone(), self.config.max_forecasts);
                        fcr.add_client( request.remote_addr);

                        self.wn_job_regions.insert( wri.region.clone(), wri);
                        self.forecast_store.insert( fcr.region.clone(), fcr);

                        is_new = true; // accepted as new region to monitor
                    },
                    Err(e) => {
                        rejection = Some("no elevation data".to_string())
                    }
                }

            } else {
                rejection = Some("invalid region".to_string())
            }
        };

        let response = SubscribeResponse::Add( AddWindClientResponse { 
            wn_region: request.wn_region, 
            is_new,
            rejection,
            remote_addr: Some(request.remote_addr) 
        });
        self.subscribe_action.execute(response).await.map_err(|e| OdinWindError::ActionFailure(e.to_string()))

    }

    async fn remove_client( &mut self, hself: ActorHandle<WindActorMsg>, request: RemoveWindClient)->Result<()> {

        if let Some(region) = &request.region { // this is for a single region
            if let Some(fr) = self.forecast_store.get_mut( region) {
                if fr.remove_client( &request.remote_addr) && (fr.client_addrs.is_empty()) { // did we remove the last client for this region
                    self.remove_region( region).await?
                }
            }
        } else { // all regions for this client are dropped
            let mut dropped: Vec<Arc<String>> = Vec::new();
            for (region,fr) in self.forecast_store.iter_mut() {
                if fr.remove_client( &request.remote_addr) && (fr.client_addrs.is_empty()) { dropped.push( fr.region.clone()); }
            }
            for region in dropped {
                self.remove_region( region.as_ref()).await?
            }
        }

        Ok(())
    }

    async fn remove_region (&mut self, region: &String)->Result<()> {
        if let Some(wri) = self.wn_job_regions.remove(region) {
            info!("removed HRRR data set requests for region {}", region);
            self.hrrr.send_msg( RemoveDataSet(wri.hrrr_ds_request)).await?;

            let response = SubscribeResponse::Remove( RemoveWindClientResponse{region: region.to_string()} );
            self.subscribe_action.execute(response).await.map_err(|e| OdinWindError::ActionFailure(e.to_string()))?;
        }

        self.forecast_store.remove( region);

        Ok(())
    }

    async fn get_dem_file (&self, region: &str, utm_rect: &UtmRect)->Result<PathBuf> {
        // TODO - should we compute instead of configure the resolution? we could compute from UTM bbox and the mesh resolution

        let fname = wn_dem_filename(region, &utm_rect);
        let path = self.cache_dir.join(fname);

        if path.is_file() { // we already have it in our cache.
            set_accessed( &path);  // update the time stamp so that we don't prematurely delete it (it rarely changes)
            return Ok(path)

        } else { // retrieve
            let bbox = &utm_rect.bbox;
            let epsg = utm_rect.epsg();
            let dem_res = self.config.dem_res;
            let srs = DemSRS::UTM{epsg};

            match self.config.dem.get_res_dem( bbox, srs, dem_res, dem_res, odin_dem::DemImgType::TIF, &path).await {
                Ok(()) => Ok(path),
                Err(e) => {
                    error!("DEM download of {:?} failed with {e}", &path);
                    Err( OdinWindError::DemError( format!("DEM download failed: {e}")) )
                }
            }
        }
    }

    async fn add_hrrr_region (&self, wn_region: &WindRegion)->Result<Arc<HrrrDataSetRequest>> {
        let mut bbox = wn_region.bbox.add_degrees( -0.3, -0.3, 0.3, 0.3); // make sure we cover the bbox after warping to EPSG 4326
        let region = wn_region.name.clone();
        let set_name = "hrrr-wind".to_string();
        let mut hrrr_cfg = HrrrDataSetConfig::new( region, bbox, set_name, self.config.hrrr_fields.clone(), self.config.hrrr_levels.clone());
        let hrrr_ds_request = Arc::new( HrrrDataSetRequest::new( hrrr_cfg) );

        info!("requesting HRRR data sets for region {}", wn_region.name);
        self.hrrr.send_msg( AddDataSet( hrrr_ds_request.clone())).await?;

        Ok(hrrr_ds_request)
    }

    async fn schedule_wn_job (&mut self, hfa: HrrrFileAvailable)->Result<()> {
        info!("received HrrrFileAvailable notification for {:?}", hfa.path);
        if let Some(wn_task) = &self.wn_task {
            let region_name = hfa.request.name();

            // only schedule if there still are clients. WindNinja exec is expensive
            // there can be a long time between a HRRR request and respective HrrrFileAvailable notifications
            if let Some(fcr) = self.forecast_store.get( region_name) {
                if !fcr.client_addrs.is_empty() {
                    if let Some(wri) = self.wn_job_regions.get( region_name) {
                        let region = wri.region.clone();
                        let step = hfa.request.step;   
                        let mesh_res = self.config.mesh_res;
                        let wind_height = self.config.wind_height;
                        let date = hfa.request.base + hours(step as u64);
                        let dem_path = wri.dem_path.clone();
                        let wx_path = Arc::new(hfa.path);
                        let wx_src = WX_HRRR.clone(); // FIXME - this shouldn't be hardcoded (there will be other sources)
                        let wn_out_basename = Arc::new( Self::get_wn_out_basename( &wri.region, date, &wri.utm_rect.bbox, mesh_res) );

                        let wn_job = WnJob{region, date, step, mesh_res, wind_height, wx_src, wx_path, dem_path, wn_out_basename};

                        if !wn_job.output_files_exist() {
                            info!("scheduling WnJob for region {} date {}", wn_job.region, wn_job.date);
                            if let Err(e) = send( &wn_task.tx, wn_job).await {
                                error!("failed to queue WnJob {} at {}+{} : {e}", wri.region, hfa.request.base, hfa.request.step);
                            }
                        } else { // no need to run WindNinja we already have the forecast from a previous run - add and notify clients
                            info!("serving WnJob for region {} date {} from cache", wn_job.region, wn_job.date);
                            let forecast = Forecast::from(wn_job);
                            self.finish_forecast( forecast).await?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
    
    fn get_wn_out_basename (region: &str, date: DateTime<Utc>, bbox: &BoundingBox<f64>, mesh_res: f64) -> String {
        //let sbb = format!("{:.0}_{:.0}_{:.0}_{:.0}", bbox.west, bbox.south, bbox.east, bbox.north);
        let mr = format!("{:.0}m", mesh_res);
        let attrs = &[ mr.as_str(), "huvw" ];
        odin_data_filename( region, Some(date), attrs, None)
    }

    async fn process_forecast (&mut self, forecast: Forecast)->Result<()> {
        info!("creating derived products for forecast {} date {} step {}", forecast.region, forecast.date, forecast.step);

        let huvw_wgs84_path = forecast.get_wn_path( huvw_wgs84_suffix());
        let huvw_ds = self.get_cropped_wgs84_ds( &forecast, &huvw_wgs84_path, false)?; // this is the basis for derived data
        let huvw_bands: &[usize] = &[1, 2, 3, 4]; // GDAL band numbers are 1-based 
        let s_band = 5; // the windspeed band

        Self::create_grid_csv( &forecast.get_wn_path( huvw_grid_suffix()), &huvw_ds, huvw_bands)?;
        Self::create_vector_csv( &forecast.get_wn_path( huvw_vector_suffix()), &huvw_ds, huvw_bands, forecast.mesh_res)?;
        Self::create_contour_json( &forecast.get_wn_path( huvw_contour_suffix()), &huvw_ds, s_band)?;

        // compute the HRRR based data products (directly from HRRR forecasts)
        let wx_ds = Dataset::open( forecast.wx_path.as_ref())?;
        let hrrr_wgs84_path = forecast.get_wn_path( hrrr_wgs84_suffix());
        let mut hrrr_ds = self.get_hrrr_wgs84_ds( &wx_ds, &huvw_ds, &hrrr_wgs84_path)?; // this creates a {u10,v10, u80,v80, s10, s80, h} dataset

        let hrrr_10_bands: &[usize] = &[7, 1, 2];
        Self::create_grid_csv( &forecast.get_wn_path( hrrr_10_grid_suffix()), &hrrr_ds, hrrr_10_bands);
        Self::create_vector_csv( &forecast.get_wn_path( hrrr_10_vector_suffix()), &hrrr_ds, hrrr_10_bands, forecast.mesh_res)?;
        Self::create_contour_json( &forecast.get_wn_path( hrrr_10_contour_suffix()), &hrrr_ds, 5)?;


        let hrrr_80_bands: &[usize] = &[7, 3, 4];
        Self::create_grid_csv( &forecast.get_wn_path( hrrr_80_grid_suffix()), &hrrr_ds, hrrr_80_bands);
        Self::create_vector_csv( &forecast.get_wn_path( hrrr_80_vector_suffix()), &hrrr_ds, hrrr_80_bands, forecast.mesh_res)?;
        Self::create_contour_json( &forecast.get_wn_path( hrrr_80_contour_suffix()), &hrrr_ds, 6)?;

        remove_file( &huvw_wgs84_path)?;
        remove_file( &hrrr_wgs84_path)?;

        self.finish_forecast(forecast).await
    }

    fn create_grid_csv (path: &PathBuf, ds: &Dataset, bands: &[usize])->Result<()> {
        write_huvw_csv_grid( ds, path, bands)?;
        gzip_path( path)?; // this stores as "*.gz" so we can delete the uncompressed version
        remove_file( path)?;
        Ok(())
    }

    fn create_vector_csv (path: &PathBuf, ds: &Dataset, bands: &[usize], mesh_res: f64)->Result<()> {
        write_huvw_csv_cell_vectors( ds, path, mesh_res, bands)?;
        gzip_path( path)?; // this stores as "*.gz" so we can delete the uncompressed version
        remove_file( path)?;
        Ok(())
    }

    fn create_contour_json (path: &PathBuf, ds: &Dataset, band: usize)->Result<()> {
        write_windspeed_contour( ds, path, band)?;
        gzip_path( path)?;
        remove_file( path)?;
        Ok(())
    }

    async fn finish_forecast (&mut self, forecast: Forecast)->Result<()> {
        if let Some(fcr) = self.forecast_store.get_mut( &forecast.region) {
            if let Some(fc) = fcr.add_forecast(forecast) {
                info!("execute update action for {:?}", fc);
                self.update_action.execute( fc).await.map_err(|e| OdinWindError::ActionFailure(e.to_string()))?;
            }
        }
        Ok(()) 
    }

    /// translate UTM forecast grid back into WGS84 (epsg 4326) and crop to account for noData values caused by the translation
    /// (UTM is cartesian, WGS84 is ellipsoid)
    fn get_cropped_wgs84_ds <P> (&self, forecast: &Forecast, path: P, keep_utm: bool) -> Result<Dataset> 
        where P: AsRef<Path> 
    {
        let huvw_wn = forecast.get_wn_output_path(); // the WindNinja huvw output filename
        let huvw_tmp = get_tmp_path( forecast.wn_out_base_name.as_str()); // the (temp) non-cropped output in WGS84 (contains nodata) 

        let tmp_ds = odin_gdal::warp::warp_to_wgs84( &huvw_wn, &huvw_tmp, vec![-9999.0])?;
        let huvw_ds = odin_gdal::crop_no_data( &tmp_ds, 0.2, path, Some(odin_gdal::compress_create_opts()))?;

        std::fs::remove_file( &huvw_tmp)?;
        if !keep_utm { std::fs::remove_file( &huvw_wn)? }

        Ok(huvw_ds)
    }

    /// warp the HRRR dataset into the same WGS84 grid as the WindNinja huvw output and compute the windspeed bands for 10m and 08m
    /// since we need them for respective contours
    /// this will produce a [u10, v10, u80, v80, s10, s80, h] dataset
    fn get_hrrr_wgs84_ds<P> ( &self, hrrr_ds: &Dataset, huvw_ds: &Dataset, tgt_path: P) -> Result<Dataset> 
        where P: AsRef<Path> 
    {
        // since the HRRR data set is configured and might contain extra fields we have to query/check the band numbers
        if let Some(id_u10) = rasterband_index_for!( hrrr_ds, ("", "GRIB_ELEMENT", Some("UGRD")), ("", "GRIB_SHORT_NAME", Some("10-HTGL")))
        && let Some(id_v10) = rasterband_index_for!( hrrr_ds, ("", "GRIB_ELEMENT", Some("VGRD")), ("", "GRIB_SHORT_NAME", Some("10-HTGL")))
        && let Some(id_u80) = rasterband_index_for!( hrrr_ds, ("", "GRIB_ELEMENT", Some("UGRD")), ("", "GRIB_SHORT_NAME", Some("80-HTGL")))
        && let Some(id_v80) = rasterband_index_for!( hrrr_ds, ("", "GRIB_ELEMENT", Some("VGRD")), ("", "GRIB_SHORT_NAME", Some("80-HTGL"))) {
            let tgt_ri = get_raster_info( huvw_ds)?;
            let src_bands = vec![ id_u10, id_v10, id_u80, id_v80 ];

            // we add tgt bands for height (which we copy from the wn_output) and for the wind speeds at 10m, 80m (which we compute)
            let mut tgt_ds = warp_to_raster_info( &hrrr_ds, tgt_path, 4326, &tgt_ri, ResampleAlg::CubicSpline, 
                                     Some(src_bands), Some(3), Some(odin_gdal::GdalDataType::Float32))?;

            // compute the wind speed bands
            Self::set_hrrr_wind_spd_band::<f32>( &mut tgt_ds, 1, 2, 5)?; // 10m
            Self::set_hrrr_wind_spd_band::<f32>( &mut tgt_ds, 3, 4, 6)?; // 80m

            // copy the height band from huvw_ds
            let h_src_band = huvw_ds.rasterband(1)?;
            let mut h_tgt_band = tgt_ds.rasterband(7)?;
            copy_full_rasterband( &h_src_band, &mut h_tgt_band)?;

            Ok(tgt_ds)
        } else {
            Err( OdinWindError::OpFailedError("invalid HRRR dataset".into()))
        }
    }

    /// we need  wind speed bands for computing contours
    fn set_hrrr_wind_spd_band<T> ( ds: &mut Dataset, u_band_nr: usize, v_band_nr: usize, s_band_nr: usize) -> Result<()> where T: Copy + GdalType {
        let u_band = ds.rasterband(u_band_nr)?;
        let v_band = ds.rasterband(v_band_nr)?;
        let mut s_band = ds.rasterband(s_band_nr)?;

        compute_rasterband_lines( &[&u_band, &v_band], &mut s_band, 0.0f32, |inputs,output| {
            for i in 0..output.len() {
                output[i] = (inputs[0][i].powi(2) + inputs[1][i].powi(2)).sqrt();
            }
        });
        Ok(())
    }

    async fn terminate (&mut self) {
        if let Some(wn_task) = &self.wn_task {
            println!("terminating wn_task...");
            wn_task.tx.close();
            wn_task.join_handle.abort();
            println!("wn_task terminated.");
        }
    }

    fn cleanup (&mut self) {
        if remove_old_files( &pkg_cache_dir!(), hours(6)).is_err() {
            warn!("failed to cleanup cache");
        }
    }
}


async fn wn_loop (hself: ActorHandle<WindActorMsg>, windninja_cmd: String, cache_dir: Arc<PathBuf>, rx: MpscReceiver<WnJob>) {
    loop {
        match recv(&rx).await {
            Ok(wn_job) => {
                info!("processing WnJob {} at {}", wn_job.region, short_utc_datetime_string( &wn_job.date));

                if wn_job.dem_path.is_file() && wn_job.wx_path.is_file() { // make sure our input files still exist
                    match run_wn( &windninja_cmd, cache_dir.as_ref(), &wn_job).await {
                        Ok(()) => {
                            info!("Wind forecast step available: {:?}", wn_job);
                            hself.send_msg( Forecast::from(wn_job)).await;
                        }
                        Err(e) => { 
                            error!("failed to process region {} at {}: {e}", wn_job.region, wn_job.date) 
                        }
                    }
                } else {
                    error!("failed to process region {} at {}: because of missing input files", wn_job.region, wn_job.date) 
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
    cmd.kill_on_drop(true);

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

/// note this is for WindNinja, which implicitly assumes UTC (no zone)
fn wn_forecast_time (date: &DateTime<Utc>)->String {
    format!("{:4}{:02}{:02}T{:02}0000", date.year(), date.month(), date.day(), date.hour()) 
}

/// this has to adhere to our data filename conventions
fn wn_dem_filename (region: &str, utm_rect: &UtmRect)->PathBuf {
    let mut rn = path_str_to_fname( region);
    //Path::new( &format!("{}__{:.0}_{:.0}_{:.0}_{:.0}.tif", rn, bbox.west,bbox.south,bbox.east,bbox.north)).to_path_buf()
    rn.push_str(".tif");
    Path::new( &rn).to_path_buf()
}

//--- the public output files generated from the Wind huvw UTM grid file


/* #region Wind actor messages ****************************************************************/

define_actor_msg_set!{ pub WindActorMsg = AddWindClient | ExecSnapshotAction | RemoveWindClient | HrrrFileAvailable | Forecast }

/* #endregion Wind actor messages */

/* #region Wind actor impl ********************************************************************/

impl_actor! { match msg for Actor<WindActor<S,U>,WindActorMsg> 
    where S: DataAction<SubscribeResponse> + Sync, U: DataRefAction<Forecast> + Sync as

    _Start_ => cont! {
        if let Ok(timer) = self.start_repeat_timer( 1, hours(1), false) {
            self.timer = Some(timer);
        } else { error!("failed to start cleanup timer") }

        let hself = self.hself.clone();
        self.start( hself);
    }

    _Timer_ => cont! {
        self.cleanup();
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
        check_err(self.process_forecast( msg).await, "failed to process forecast");
    }

    // received from client to stop forecasts for given area (if there are no other clients left)
    RemoveWindClient => cont! { 
        let hself = self.hself.clone();
        check_err( self.remove_client( hself, msg).await, "failed to remove windninja client")
    }

    _Terminate_ => stop! { 
        self.terminate().await;
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
                        let ws_msg = ws_msg_from_json( wind_service::MOD_PATH, "rejectForecastRegion", &json);
                        hserver.send_msg( SendWsMsg{ remote_addr, ws_msg}).await;
                    }

                } else {
                    let json = serde_json::to_string( &response.wn_region)?;
                    let ws_msg = ws_msg_from_json( wind_service::MOD_PATH, "startForecastRegion", &json);
                    if response.is_new {
                        hserver.send_msg( BroadcastWsMsg{ws_msg}).await; // tell everybody there is a new region
                    } else {
                        if let Some(remote_addr) = response.remote_addr {
                            hserver.send_msg( SendWsMsg{ remote_addr, ws_msg}).await; // let only the requester know it is subscribed
                        }
                    }
                }
            }
            SubscribeResponse::Remove(response) => {
                let json = serde_json::to_string( &response)?;
                let ws_msg = ws_msg_from_json( wind_service::MOD_PATH, "stopForecastRegion", &json);
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
        let ws_msg = ws_msg_from_json( wind_service::MOD_PATH, "forecast", &json);
        hserver.send_msg( BroadcastWsMsg{ws_msg}).await;
        Ok(())
    })
}
