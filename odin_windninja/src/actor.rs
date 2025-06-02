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

use std::{path::PathBuf, sync::Arc, collections::HashMap};
use reqwest::{self, Client};

use odin_build::pkg_cache_dir;
use odin_common::{geo::GeoRect, net::download_url, utm::{self,UtmRect,UtmZone,UTM}};
use odin_hrrr::{AddDataSet, HrrrActorMsg, HrrrDataSetConfig, HrrrDataSetRequest, HrrrFileAvailable};
use odin_actor::prelude::*;
use crate::{errors::{OdinWindNinjaError,Result}, Forecast, ForecastRegion, ForecastStore, WindNinjaConfig};


/// the WindNinjaActor state
pub struct WindNinjaActor<I,S,U> where I: DataRefAction<ForecastStore>, S: DataAction<Result<AddClientResponse>>, U: DataRefAction<Forecast>
{
    config: WindNinjaConfig,
    cache_dir: PathBuf,              // where to store computed forecasts
    hrrr: ActorHandle<HrrrActorMsg>, // where to get new HRRR reports from - this drives our data update

    forecast_store: ForecastStore,

    init_action: I,
    subscribe_action: S,
    update_action: U
}

impl <I,S,U> WindNinjaActor<I,S,U> where I: DataRefAction<ForecastStore>, S: DataAction<Result<AddClientResponse>>, U: DataRefAction<Forecast> {
    pub fn new (config: WindNinjaConfig, hrrr: ActorHandle<HrrrActorMsg>, init_action: I, subscribe_action: S, update_action: U)->Self {
        let cache_dir = pkg_cache_dir!();
        let forecast_store = HashMap::new();
        WindNinjaActor { config, cache_dir, hrrr, forecast_store, init_action, subscribe_action, update_action }
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
                        let hrrr_ds_request = self.add_hrrr_region( &request).await?;

                        let mut fcr = ForecastRegion::new( Arc::new( request.region.clone()), request.bbox.clone(), dem_path, hrrr_ds_request);
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
        // TODO - how do we get resolution or w/h ?

        let fname = odin_dem::get_res_dem_filename( "dem", utm_rect.epsg(), &utm_rect.bbox, self.config.dem_res_x, self.config.dem_res_y, "tif");
        let path = self.cache_dir.join(fname);

        if path.is_file() { // we already have it in our cache
            return Ok(path)

        } else { // retrieve then cache
            let uri = format!("{}", self.config.dem_url);
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
}


/// the response data for a successful subscription
/// (we can use this in the future to transmit session data or access keys)
#[derive(Debug)]
pub struct AddClientResponse {
    request: AddWindNinjaClient,
    n_clients: u32,
    // possibly more in the future
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

define_actor_msg_set!{ pub WindNinjaActorMsg = AddWindNinjaClient | ExecSnapshotAction | RemoveWindNinjaClient | HrrrFileAvailable }

/* #endregion WindNinja actor messages */

/* #region WindNinja actor impl ********************************************************************/

impl_actor! { match msg for Actor<WindNinjaActor<I,S,U>,WindNinjaActorMsg> 
    where I: DataRefAction<ForecastStore> + Sync, S: DataAction<Result<AddClientResponse>> + Sync, U: DataRefAction<Forecast> + Sync as

    // received from a client to start forecasts for the given area
    AddWindNinjaClient => cont! { 
        let hself = self.hself.clone();
        check_err( self.add_client( hself, msg).await, "failed to add windninja client")
    }

    // received from client to process snapshot of current data
    ExecSnapshotAction => cont! { msg.0.execute( &self.forecast_store).await; }

    // received from HrrrActor when new HRRR dataset for a monitored region is available. This kicks off WindNinja execution
    HrrrFileAvailable => cont! { }

    // received from client to stop forecasts for given area (if there are no other clients left)
    RemoveWindNinjaClient => cont! { }

}

/* #endregion WindNinja actor impl */