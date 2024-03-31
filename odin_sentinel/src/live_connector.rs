/*
 * Copyright (c) 2024, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The ODIN - Open Data Integration Framework is licensed under the
 * Apache License, Version 2.0 (the "License"); you may not use this file
 * except in compliance with the License. You may obtain a copy of the
 * License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */
#![allow(unused)]

use std::{future,collections::HashMap,sync::{Arc,atomic::AtomicU64,Mutex}};
use futures::{TryFutureExt, stream::{StreamExt,SplitStream,SplitSink}};
use tokio_tungstenite::{tungstenite::protocol::Message, MaybeTlsStream};
use reqwest::{Client};

use odin_actor::{error, minutes, prelude::*};
use odin_config::prelude::*;
use odin_common::{fs::remove_old_files};

use crate::*;
use crate::actor::*;
use crate::errors::*;
use crate::ws::{WsStream,WsCmd,WsMsg, init_websocket, send_ws_text_msg, read_next_ws_msg};

// holds the data for a started LiveSentinelConnector. Rather than putting each field value behind
// an option we bundle all of them into one struct. Since all the spawned tasks are endless loops we
// just keep their AbortHandles
struct LiveConn {
    hself: ActorHandle<SentinelActorMsg>, // set by actor once it is initialized
    last_recv_epoch: Arc<AtomicU64>,

    ws_rx_task: AbortHandle, // async task for websocket input
    ws_tx_task: AbortHandle, // async task for websocket output
    ws_cmd_tx: MpscSender<String>, // channel to send websocket commands

    ping_task: Option<AbortHandle>, // optional periodic keepalive ping 

    data_dir: PathBuf,
    file_request_task: AbortHandle, // async task for file requests
    file_request_tx: MpscSender<SentinelFileRequest>,
    pending_file_requests: Arc<Mutex<HashMap<String,Arc<Notify>>>>,

    file_cleanup_task: AbortHandle, // periodic file cleanup task
}

impl LiveConn {
    async fn new (config: Arc<SentinelConfig>, hself: ActorHandle<SentinelActorMsg>)->Result<Self> {
        //--- get current sentinel data according to config (there is no point spawning tasks if we don't have a list of devices to watch)
        let http_client = Client::new();
        let mut sentinel_store = SentinelStore::new();
        sentinel_store.fetch_from_config( &http_client, &config).await?; // this can take some time

        //--- now open a websocket and register for the devies we got
        let device_ids = sentinel_store.get_device_ids();
        if !device_ids.is_empty() {
            let last_recv_epoch = Arc::new(AtomicU64::new(0));

            let ws_stream = init_websocket( &config, device_ids).await?;
            let (ws_write, ws_read) = ws_stream.split();
            let ws_rx_task = spawn( "ws-sentinel-rx", Self::ws_rx_loop( hself.clone(), config.clone(), ws_read))?.abort_handle();

            let (ws_cmd_tx, ws_cmd_rx) = create_mpsc_sender_receiver::<String>(16);
            let ws_tx_task = spawn( "ws-sentinel-tx", Self::ws_tx_loop(  ws_cmd_rx, ws_write))?.abort_handle();

            let file_cleanup_task = spawn( "sentinel-file-purge", Self::file_cleanup_loop( config.clone()))?.abort_handle();

            let ping_task = match config.ping_interval {
                Some(interval) => {
                    let ws_cmd_tx = ws_cmd_tx.clone();
                    let ah = spawn( "ws-ping", Self::ws_ping_loop(interval, ws_cmd_tx))?.abort_handle();
                    Some(ah)
                }
                None => None
            };

            let pending_file_requests: Arc<Mutex<HashMap<String,Arc<Notify>>>> = Arc::new( Mutex::new( HashMap::new()));

            let (file_request_tx, file_request_rx) = create_mpsc_sender_receiver::<SentinelFileRequest>(256);
            let file_request_task = spawn( "sentinel-file_request", 
                                            Self::file_request_loop( config.clone(), file_request_rx, pending_file_requests.clone()))?.abort_handle();

            let data_dir: PathBuf = odin_config::app_metadata().data_dir.join("sentinel");

            hself.send_msg( InitializeStore(sentinel_store)).await?;
            
            Ok( LiveConn { 
                hself, last_recv_epoch, 
                ws_rx_task, ws_tx_task, ws_cmd_tx, 
                ping_task, 
                data_dir,
                file_request_task, file_request_tx, pending_file_requests,
                file_cleanup_task,
            })

        } else {
            Err( OdinSentinelError::NoDevicesError)
        }
    }

    /// the websocket receiver loop
    async fn ws_rx_loop (hself: ActorHandle<SentinelActorMsg>, config: Arc<SentinelConfig>, mut ws_read: SplitStream<WsStream>)->Result<()> {
        let http_client = reqwest::Client::new();
        loop {
            match ws_read.next().await {
                Some(m) => {
                    match m {
                        Ok(Message::Text(json)) => {
                            match serde_json::from_str::<WsMsg>(&json) {
                                Ok(msg) => {
                                    match msg {
                                        WsMsg::Record { device_id, sensor_no, rec_type } => {
                                            Self::get_and_send_record( &hself, &http_client, &config, device_id.as_str(), sensor_no, rec_type).await?;
                                        }
                                        WsMsg::Pong { request_time, response_time, message_id } => {}
                                        WsMsg::Error { message } => {}
                                        _ => {} // ignore other messages
                                    }
                                }
                                Err(e) => {
                                    hself.try_send_msg( ConnectorError(OdinSentinelError::JsonError(e)));
                                }
                            }
                        }
                        Ok(Message::Binary(bs)) => {} // TODO unexpected binary message
                        Ok(Message::Ping(_)) => {} // system level ping
                        Ok(Message::Pong(_)) => {} // system level pong
                        Ok(Message::Close(_)) => {} // TODO 
                        Ok(Message::Frame(_)) => {}
                        Err(e) => {
                            hself.try_send_msg( ConnectorError(OdinSentinelError::WsError(e)));
                            // if this causes the websocket to be closed we still catch it in next() so we don't have to break here
                        } 
                    }
                }
                None => { // stream closed by server
                    hself.try_send_msg( ConnectorError(OdinSentinelError::WsClosedError{}));
                    return Err(OdinSentinelError::WsClosedError{})
                }
            }
        }
        Ok(())
    }    

    async fn get_and_send_record (hself: &ActorHandle<SentinelActorMsg>, client: &Client, config: &SentinelConfig,
                                  device_id: &str, sensor_no: u32, capability: SensorCapability) -> Result<()> 
    {   
        let base_uri = config.base_uri.as_str();
        let access_token = config.access_token.as_str();

        use SensorCapability::*;
        Ok(match capability {
            Accelerometer => Self::get_and_send_update::<AccelerometerData>( hself, client, base_uri, access_token, device_id, sensor_no).await,
            Anemometer    => Self::get_and_send_update::<AnemometerData>( hself, client, base_uri, access_token, device_id, sensor_no).await,
            Cloudcover    => Self::get_and_send_update::<CloudcoverData>( hself, client, base_uri, access_token, device_id, sensor_no).await,
            Fire          => Self::get_and_send_update::<FireData>( hself, client, base_uri, access_token, device_id, sensor_no).await,
            Gas           => Self::get_and_send_update::<GasData>( hself, client, base_uri, access_token, device_id, sensor_no).await,
            Gps           => Self::get_and_send_update::<GpsData>( hself, client, base_uri, access_token, device_id, sensor_no).await,
            Gyroscope     => Self::get_and_send_update::<GyroscopeData>( hself, client, base_uri, access_token, device_id, sensor_no).await,
            Image         => Self::get_and_send_update::<ImageData>( hself, client, base_uri, access_token, device_id, sensor_no).await,
            Magnetometer  => Self::get_and_send_update::<MagnetometerData>( hself, client, base_uri, access_token, device_id, sensor_no).await,
            Orientation   => Self::get_and_send_update::<OrientationData>( hself, client, base_uri, access_token, device_id, sensor_no).await,
            Person        => Self::get_and_send_update::<PersonData>( hself, client, base_uri, access_token, device_id, sensor_no).await,
            Power         => Self::get_and_send_update::<PowerData>( hself, client, base_uri, access_token, device_id, sensor_no).await,
            Smoke         => Self::get_and_send_update::<SmokeData>( hself, client, base_uri, access_token, device_id, sensor_no).await,
            Thermometer   => Self::get_and_send_update::<ThermometerData>( hself, client, base_uri, access_token, device_id, sensor_no).await,
            Valve         => Self::get_and_send_update::<ValveData>( hself, client, base_uri, access_token, device_id, sensor_no).await,
            Voc           => Self::get_and_send_update::<VocData>( hself, client, base_uri, access_token, device_id, sensor_no).await,
        }?)
    }

    async fn get_and_send_update<T>(hself: &ActorHandle<SentinelActorMsg>, client: &Client, 
                                    base_uri: &str, access_token: &str, device_id: &str, sensor_no: u32) -> Result<()> 
        where T: RecordDataBounds, SentinelUpdate: From<Arc<SensorRecord<T>>>
    {
        let update = get_latest_update::<T>( client, base_uri, access_token, device_id, sensor_no).await?;
        Ok(hself.send_msg( UpdateStore( update)).await?)
    }

    async fn ws_tx_loop (ws_cmd_rx: MpscReceiver<String>, mut ws_write: SplitSink<WsStream,Message>) -> Result<()> {
        loop {
            match recv(&ws_cmd_rx).await {
                Ok(msg) => {
                    if let Err(e) = send_ws_text_msg( &mut ws_write, msg).await {
                        error!("failed to send websocket message: {e:?}")
                    }
                }
                Err(e) => return Err(connector_error("command queue closed"))
            }
        }
        Ok(())
    }

    async fn ws_ping_loop (interval: Duration, ws_cmd_tx: MpscSender<String>) -> Result<()> {
        loop {
            sleep(interval).await;
            let ping = WsCmd::new_ping("ping");
            if let Ok(json) = serde_json::to_string( &ping) {
                ws_cmd_tx.send(json).await;
            }
        }
        Ok(())
    }

    async fn file_request_loop (config: Arc<SentinelConfig>, file_request_rx: MpscReceiver<SentinelFileRequest>,
                                pending_file_requests: Arc<Mutex<HashMap<String,Arc<Notify>>>>) -> Result<()> {
        let http_client = reqwest::Client::new();
        loop {
            match recv(&file_request_rx).await {
                Ok(req) => {
                    let result = get_file_request( &http_client, &config.access_token, &req).await;
                    match result {
                        Ok(()) => {
                            req.notify.notify_waiters();
                            if let Ok(mut pr) = pending_file_requests.lock() {
                                pr.remove( &req.uri);
                            }
                        }
                        Err(e) => { 
                            error!("failed to retrieve file {}: {:?}", req.uri, e)
                        }
                    }
                }
                Err(e) => return Err(connector_error("file request queue closed"))
            }
        }
        Ok(())
    }

    async fn request_image_file (&self, config: &SentinelConfig, rec: &SensorRecord<ImageData>) -> Result<()> {
        let uri = get_image_uri( &config.base_uri, &rec.id);
        let data_dir = odin_config::app_metadata().data_dir.join("sentinel");

        let req = SentinelFileRequest::for_image_record( &rec, &data_dir, uri);

        if let Ok(mut pending_requests) = self.pending_file_requests.lock() {
            pending_requests.insert( req.sentinel_file.pathname.to_string_lossy().to_string(), req.notify.clone());
        }

        Ok(self.file_request_tx.send( req).await.map_err(|e| send_error("file request queue closed"))?)
    }

    async fn handle_file_query (&self, data_dir: &PathBuf, file_query: Query<GetSentinelFile,Option<PathBuf>>)->Result<()> {
        let filename = &file_query.question.filename;
        let pn = data_dir.join( &filename);

        if let Ok(pending_requests) = self.pending_file_requests.lock() {
            if let Some(req) = pending_requests.get( filename) {
                let notify = req.clone();
                drop(pending_requests); // release lock before we await

                let q = spawn( "file-query", async move {
                    notify.notified().await;
                    file_query.respond( Some(pn)).await;
                });
                match q {
                    Ok(_) => Ok(()),
                    Err(_) => Err( op_failed("could not spawn file query"))
                }
            } else {
                Err( op_failed("receiver closed"))
            }
        } else {
            Err( op_failed("could not obtain file request lock"))
        }
    }

    async fn file_cleanup_loop (config: Arc<SentinelConfig>)->Result<()> {
        let interval = minutes(60); // should we configure this?
        let data_dir = odin_config::app_metadata().data_dir.join("sentinel");

        loop {
            sleep(interval).await;
            remove_old_files( &data_dir, config.max_age);
        }
        Ok(())
    }

    fn terminate(&mut self)->Result<()> {
        if let Some(ping_task) = &self.ping_task {
            ping_task.abort();
            self.ping_task = None;
        }
        self.file_request_task.abort();
        self.file_cleanup_task.abort();

        self.ws_tx_task.abort();
        self.ws_rx_task.abort();

        Ok(())
    }
}

/// struct that represents a websocket based SentinelConnector for live data
/// Note this gets instantiated before the respective actor (it's an actor argument) but
/// we can't fully initialize yet before we have the ActorHandle. Hence all our child tasks
/// are spawned once we get a start(hself) from the actor
pub struct LiveSentinelConnector { 
    config: Arc<SentinelConfig>,
    data_dir: PathBuf,
    conn: Option<LiveConn>
}

impl LiveSentinelConnector {
    pub fn new (config: SentinelConfig)->Self {
        let data_dir = odin_config::app_metadata().data_dir.join("sentinel");
        LiveSentinelConnector { config: Arc::new(config), data_dir, conn: None }
    }

    async fn initialize (&mut self, hself: ActorHandle<SentinelActorMsg>)->Result<()> {
        self.conn = Some(LiveConn::new(self.config.clone(), hself).await?);
        Ok(())
    }
}

impl SentinelConnector for LiveSentinelConnector {
    async fn start (&mut self, hself: ActorHandle<SentinelActorMsg>)->Result<()> {
        self.initialize(hself).await
    }

    async fn send_cmd (&mut self, cmd: WsCmd)->Result<()> {
        if let Some(conn) = &self.conn {
            let json = serde_json::to_string(&cmd)?;
            Ok(conn.ws_cmd_tx.send( json).await.map_err(|e| send_error("command queue closed"))?)
        } else {
            Err( op_failed("connection not initialized"))
        }
    }

    async fn request_image_file (&self, rec: &SensorRecord<ImageData>)->Result<()> {
        if let Some(conn) = &self.conn {
            conn.request_image_file( &self.config, rec).await
        } else {
            Err( op_failed("connection not initialized"))
        }
    }

    async fn handle_file_query (&self, file_query: Query<GetSentinelFile,Option<PathBuf>>)->Result<()> {
        let pn = self.data_dir.join(&file_query.question.filename);
        if pn.is_file() { // already downloaded, respond right away
            file_query.respond(Some(pn)).await.map_err(|_| op_failed("receiver closed"))
        } else { // in flight. respond once we get notified (means the query client should be prepared to wait)
            if let Some(conn) = &self.conn {
                conn.handle_file_query( &self.data_dir, file_query).await
            } else {
                Err( op_failed("connection not initialized"))
            }
        }
    }

    fn terminate (&mut self) {
        if let Some(mut conn) = self.conn.as_mut() {
            conn.terminate();
            self.conn = None;
        }
    }

    fn max_history(&self)->usize {
        self.config.max_history_len
    }

}