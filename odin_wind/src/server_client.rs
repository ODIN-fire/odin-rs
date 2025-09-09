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

use std::{collections::{HashMap,HashSet, VecDeque}, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use http::HeaderMap;
use serde::{Serialize,Deserialize};
use serde_json;
use futures_util::{SinkExt,StreamExt,stream::{SplitSink,SplitStream}};
use chrono::Utc;
use tokio_tungstenite::{
    connect_async, WebSocketStream, MaybeTlsStream, 
    tungstenite::{self,
        protocol::Message, 
        http::{Request,header::{AUTHORIZATION,HeaderValue}}, 
        handshake::client::{Response,generate_key}, 
        client::IntoClientRequest
    }
};
use tokio::{net::TcpStream,io::{AsyncRead,AsyncReadExt,AsyncWrite,AsyncWriteExt}};
use reqwest::Client;

use odin_build::pkg_cache_dir;
use odin_common::{    
    collections::process_async, datetime::{hours, secs, short_utc_datetime_string, ZERO}, 
    fs::remove_old_files, geo::GeoRect, net::{get_file, NO_HEADERS, ZERO_ADDR}, 
    ron::{from_typed_compact_ron, TypedCompactRon}, ws
};
use odin_hrrr::HrrrFileAvailable;
use odin_actor::prelude::*;
use crate::{
    errors::{op_failed, OdinWindError, Result}, 
    AddWindClient, AddWindClientResponse, ExecSnapshotAction, Forecast, ForecastRegion, ForecastStore, RemoveWindClient, RemoveWindClientResponse, 
    SubscribeResponse, WindConfig, WindRegion, WnJobRegion, PKG_CACHE_DIR,
    huvw_wgs84_suffix, huvw_grid_suffix, huvw_vector_suffix, huvw_contour_suffix, 
    hrrr_wgs84_suffix, hrrr_10_grid_suffix, hrrr_10_vector_suffix, hrrr_10_contour_suffix,
    hrrr_80_grid_suffix, hrrr_80_vector_suffix, hrrr_80_contour_suffix
};

pub type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Deserialize,Serialize,Debug)]
pub struct WindServerClientConfig {
    pub ws_uri: String,
    pub file_uri: String, 
    pub(crate) access_token: String,
    pub max_age: Duration,
    pub max_forecasts: usize,
    // ...probably more to follow
}

/// an adapter actor that connects to a remote WindServer to obtain Forecasts. This is basically an adapter that delegates
/// weather/DEM data acquisition and WindNinja execution to a remote server
/// NOTE - this has to be action compatible with WindActor so that applications can chose which one to use
pub struct WindServerClient<S,U> where S: DataAction<SubscribeResponse>, U: DataRefAction<Forecast> {
    config: Arc<WindServerClientConfig>,

    forecast_store: ForecastStore,

    subscribe_action: S,
    update_action: U,

    client: Client,
    pending_requests: HashMap<String,HashSet<SocketAddr>>, // region_name -> pending browser requests
    connector: Option<WindServerClientConnector>,
    timer: Option<AbortHandle>, // housekeeping
}

struct WindServerClientConnector {
    ws_task: AbortHandle, // async task for websocket input
    ws_tx: MpscSender<String>, // sender part of websocket
}

impl <S,U> WindServerClient<S,U> where S: DataAction<SubscribeResponse> + 'static, U: DataRefAction<Forecast> + 'static
{
    pub fn new (config: WindServerClientConfig, subscribe_action: S, update_action: U)->Self {
        let config = Arc::new(config);
        let forecast_store = HashMap::new();
        let pending_requests = HashMap::new();
        let client = Client::new();
        WindServerClient { config, forecast_store, subscribe_action, update_action, client, pending_requests, connector: None, timer: None }
    }

    fn start (&mut self, hself: ActorHandle<WindServerClientMsg>)->Result<()> {
        let (ws_tx, ws_rx) = create_mpsc_sender_receiver::<String>(16);
        let ws_task = spawn( "ws-wind", Self::ws_loop( hself.clone(), self.config.clone(), ws_rx))?.abort_handle();
        self.connector = Some( WindServerClientConnector { ws_task, ws_tx } );

        Ok(())
    }

    async fn ws_loop (hself: ActorHandle<WindServerClientMsg>, config: Arc<WindServerClientConfig>, ws_rx: MpscReceiver<String>) {
        let proc_incoming = async move |msg: Message| {
            if let Ok(bytes) = msg.into_text() {
                hself.send_msg( ProcessServerWsMsg( bytes.as_str().to_string())).await
            } else {
                Ok(())
            }
        };

        ws::ws_loop( config.ws_uri.clone(), config.access_token.clone(), ws_rx, Some(secs(90)), proc_incoming).await;
    }

    /// upstream (WindServer) responses/notifications (note this is using RON to communicate between WindServerClient and WindServer)
    async fn process_server_ws_msg (&mut self, msg: String)->Result<()> {
        // TODO - retrieve data files for forecast from external server and send local notifications

        if let Some(response) = from_typed_compact_ron::<AddWindClientResponse>(&msg) {
            return self.process_add_client_response(response).await;
        }
        
        if let Some(forecast) = from_typed_compact_ron::<Forecast>(&msg) {
            return self.process_forecast(forecast).await;
        }

        warn!("ignored server message: {}", msg);
        Ok(()) 
    }

    async fn process_add_client_response (&mut self, response: AddWindClientResponse)->Result<()> {
        if let Some(client_addrs) = self.pending_requests.remove( &response.wn_region.name) {
            let fcr = ForecastRegion {
                region: Arc::new( response.wn_region.name.clone()),
                bbox: response.wn_region.bbox.clone(),
                client_addrs: client_addrs.clone(),
                forecasts: VecDeque::with_capacity( self.config.max_forecasts)
            };
            self.forecast_store.insert( fcr.region.clone(), fcr);

            for remote_addr in &client_addrs {
                let mut acr = response.clone();
                acr.remote_addr = Some(remote_addr.clone());
                let rsp = SubscribeResponse::Add(acr);
                self.subscribe_action.execute(rsp).await.map_err(|e| OdinWindError::ActionFailure(e.to_string()))?;
            }
        }
        Ok(())
    }

    async fn process_forecast (&mut self, forecast: Forecast)->Result<()> {
        let client = &self.client;
        let base_uri = &self.config.file_uri;
        let cache_dir = &PKG_CACHE_DIR;

        if let Some(fcr) = self.forecast_store.get_mut( &forecast.region) {
            download_forecast_data( client, &forecast, base_uri, cache_dir).await?;

            if let Some(fc) = fcr.add_forecast(forecast) {  
                self.update_action.execute( fc).await.map_err(|e| OdinWindError::ActionFailure(e.to_string()))?;
            }  
        }
        Ok(())
    }

    /// this adds browser clients (downstream request)
    async fn add_client (&mut self, hself: ActorHandle<WindServerClientMsg>, mut request: AddWindClient)->Result<()> {
        // if this region is already in our list we can answer right away without contacting the server
        if let Some(fcr) = self.forecast_store.get_mut( &request.wn_region.name) { // do we already have this region?
            let mut rejection: Option<String> = None;

            if fcr.bbox != request.wn_region.bbox { // reject name in use for different coordinates
                rejection = Some("region in use".to_string());
            } else {
                fcr.client_addrs.insert( request.remote_addr);
            }

            let response = SubscribeResponse::Add( AddWindClientResponse { 
                wn_region: request.wn_region, 
                is_new: false,
                rejection,
                remote_addr: Some(request.remote_addr) 
            });
            return self.subscribe_action.execute(response).await.map_err(|e| OdinWindError::ActionFailure(e.to_string()))

        } else { // new region - send up to server
            self.add_pending_request( &request);
            request.remote_addr = ZERO_ADDR; // don't send our client addr to the server - the server gets our addr from the websocket msg
            self.send_ws_msg( request.to_typed_compact_ron()?).await
        }
    }

    fn add_pending_request (&mut self, request: &AddWindClient) {
        if let Some(clients) = self.pending_requests.get_mut( &request.wn_region.name) {
            clients.insert( request.remote_addr.clone());
        } else {
            self.pending_requests.insert( request.wn_region.name.clone(), HashSet::from( [request.remote_addr.clone()]));
        }
    }

    async fn send_ws_msg (&self, msg: String)->Result<()> {
        if let Some(conn) = &self.connector {
            Ok( conn.ws_tx.send(msg).await? )
        } else {
            Err( op_failed("send failed - not connected to wind server"))
        }
    }

    async fn remove_client( &mut self, hself: ActorHandle<WindServerClientMsg>, request: RemoveWindClient)->Result<()> {
        let mut drop_regions: Vec<Arc<String>> = Vec::new(); // will hold regions for which there are no clients left

        if let Some(region) = &request.region { // explicit unsubscribe for single region
            if let Some(fcr) = self.forecast_store.get_mut( region) {
                if fcr.client_addrs.remove(  &request.remote_addr) && fcr.client_addrs.is_empty() { 
                    drop_regions.push( fcr.region.clone())
                }
            }
        } else { // unsubscribe all regions for this client
            for (_,fcr) in self.forecast_store.iter_mut() {
                if fcr.client_addrs.remove(  &request.remote_addr) && fcr.client_addrs.is_empty() { 
                    drop_regions.push( fcr.region.clone())
                }
            }
        }

        for rgn in drop_regions.into_iter() {
            self.forecast_store.remove( &rgn);
            let response = SubscribeResponse::Remove( RemoveWindClientResponse{region: rgn.to_string()} );
            self.subscribe_action.execute(response).await.map_err(|e| OdinWindError::ActionFailure(e.to_string()))?;

            let remove_req = RemoveWindClient{ region: Some(rgn.to_string()), remote_addr: ZERO_ADDR };
            self.send_ws_msg( remove_req.to_typed_compact_ron()?).await?;
        }

        Ok(())
    }

    async fn terminate (&mut self) {
        if let Some(connector) = &self.connector {
            connector.ws_task.abort(); // this should close the websocket which the server will detect
        }
    }

    fn cleanup (&mut self) {
        if remove_old_files( &pkg_cache_dir!(), hours(6)).is_err() {
            warn!("failed to cleanup cache");
        }
    }
}

async fn download_forecast_data( client: &Client, forecast: &Forecast, base_uri: &str, cache_dir: &PathBuf)->Result<()> {
    //--- WindNinja output files
    get_wind_file( client, base_uri, &forecast.wn_out_base_name, huvw_wgs84_suffix(), cache_dir).await?;

    get_wind_file( client, base_uri, &forecast.wn_out_base_name, huvw_grid_suffix(), cache_dir).await?;
    get_wind_file( client, base_uri, &forecast.wn_out_base_name, huvw_vector_suffix(), cache_dir).await?;
    get_wind_file( client, base_uri, &forecast.wn_out_base_name, huvw_contour_suffix(), cache_dir).await?;

    //--- HRRR output files
    get_wind_file( client, base_uri, &forecast.wn_out_base_name, hrrr_wgs84_suffix(), cache_dir).await?;

    get_wind_file( client, base_uri, &forecast.wn_out_base_name, hrrr_10_grid_suffix(), cache_dir).await?;
    get_wind_file( client, base_uri, &forecast.wn_out_base_name, hrrr_10_vector_suffix(), cache_dir).await?;
    get_wind_file( client, base_uri, &forecast.wn_out_base_name, hrrr_10_contour_suffix(), cache_dir).await?;

    get_wind_file( client, base_uri, &forecast.wn_out_base_name, hrrr_80_grid_suffix(), cache_dir).await?;
    get_wind_file( client, base_uri, &forecast.wn_out_base_name, hrrr_80_vector_suffix(), cache_dir).await?;
    get_wind_file( client, base_uri, &forecast.wn_out_base_name, hrrr_80_contour_suffix(), cache_dir).await?;

    Ok(())
}

async fn get_wind_file (client: &Client, base_uri: &str, base_name: &str, suffix: &str, cache_dir: &PathBuf)->Result<()> {
    let fname = format!("{}{}", base_name, suffix);

    let path = cache_dir.join( &fname);
    if !path.is_file() { // we might already have this from a previous run, or the server might run in the same ODIN_ROOT
        let url = format!("{}/{}", base_uri, fname);
        get_file( client, &url, NO_HEADERS, cache_dir).await?;
    }

    Ok(())
}

define_actor_msg_set!{ pub WindServerClientMsg = AddWindClient | ExecSnapshotAction | RemoveWindClient | Forecast | ProcessServerWsMsg }

#[derive(Debug)]
pub struct ProcessServerWsMsg(String);

impl_actor! { match msg for Actor<WindServerClient<S,U>,WindServerClientMsg> 
    where S: DataAction<SubscribeResponse> + Sync, U: DataRefAction<Forecast> + Sync as

    _Start_ => cont! {
        if let Ok(timer) = self.start_repeat_timer( 1, hours(1), false) {
            self.timer = Some(timer);
        } else { error!("failed to start cleanup timer") }

        let hself = self.hself.clone();
        check_err( self.start( hself), "failed to start");
    }

    _Timer_ => cont! {
        self.cleanup();
    }

    // received from a client to start forecasts for the given area
    AddWindClient => cont! { 
        let hself = self.hself.clone();
        check_err( self.add_client( hself, msg).await, "failed to add windninja client")
    }

    ProcessServerWsMsg => cont! {
        self.process_server_ws_msg(msg.0).await;
    }

    // received from client to process snapshot of current data
    ExecSnapshotAction => cont! { 
        msg.0.execute( &self.forecast_store).await; 
    }

    Forecast => cont! {
        check_err(self.process_forecast( msg).await, "failed to process forecast");
    }

    // received from client to stop forecasts for given area (if there are no other clients left)
    // this could either be an explicit unsubscribe from a user or a dropped connection to a browser (e.g. when closing the tab)
    RemoveWindClient => cont! { 
        let hself = self.hself.clone();
        check_err( self.remove_client( hself, msg).await, "failed to remove windninja client")
    }

    _Terminate_ => stop! { 
        self.terminate().await;
    }
}
