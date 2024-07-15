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

use std::{future,collections::{VecDeque,HashMap},sync::{Arc,atomic::AtomicU64,Mutex}};
use futures::{TryFutureExt, stream::{StreamExt,SplitStream,SplitSink}};
use tokio_tungstenite::{tungstenite::protocol::Message, MaybeTlsStream};
use reqwest::{Client};

use odin_actor::prelude::*;
use odin_common::{if_let,fs::{remove_old_files, ensure_dir}};

use crate::*;
use crate::actor::*;
use crate::errors::*;
use crate::ws::{WsStream,WsCmd,WsMsg, init_websocket, send_ws_text_msg, read_next_ws_msg};

/* #region LiveSentinelConnector *************************************************************************************/

/// a websocket based [`SentinelConnector`] implementation for live data.
/// 
/// Note that `SentinelConnector` instances are used for dependency injection into [`SentinelActor`] and hence
/// are created before we have a respective [`ActorHandle`]. This means the purpose of a `LiveSentinelConnector` is
/// twofold: 
///   - (a) to provide a [`SentinelConnector`] impl that is used by the actor, and 
///   - (b) to create the internal `LiveConnection` object that does the real work once the actor calls 
///    `SentinelConnector::start(actor_handle)` (during processing of its _Start_ message).
/// 
/// Note also that LiveSentinelConnector is a configured object. Since the [`SentinelConfig`] data is shared with
/// a number of background tasks managed by the `LiveConnection` we keep it in an `Arc`
/// 
pub struct LiveSentinelConnector { 
    config: Arc<SentinelConfig>,
    connection: Option<LiveConnection>
}

impl LiveSentinelConnector {

    /// called before actor instantiation
    pub fn new (config: SentinelConfig)->Self {
        LiveSentinelConnector { config: Arc::new(config), connection: None }
    }

    /// called from actor ctor (2nd half of our initialization)
    async fn initialize (&mut self, hself: ActorHandle<SentinelActorMsg>)->Result<()> {
        self.connection = Some(LiveConnection::new(self.config.clone(), hself).await?);
        Ok(())
    }

    fn sentinel_file_for_query (&self, query: &Query<GetSentinelFile,Result<SentinelFile>>)->SentinelFile {
        let record_id = query.question.record_id.clone();
        let pathname = odin_build::cache_dir().join(&query.question.filename);
        SentinelFile { record_id, pathname }   
    }
}

/// this is the interface used by the [`SentinelActor`] 
impl SentinelConnector for LiveSentinelConnector {
    async fn start (&mut self, hself: ActorHandle<SentinelActorMsg>)->Result<()> {
        self.initialize(hself).await
    }

    async fn send_cmd (&mut self, cmd: WsCmd)->Result<()> {
        if let Some(conn) = &self.connection {
            let json = serde_json::to_string(&cmd)?;
            Ok(conn.ws_cmd_tx.send( json).await.map_err(|e| send_error("command queue closed"))?)
        } else {
            Err( op_failed("connection not initialized"))
        }
    }

    async fn handle_file_query (&self, file_query: Query<GetSentinelFile,Result<SentinelFile>>)->Result<()> {
        let file = self.sentinel_file_for_query( &file_query);
        if file.pathname.is_file() { // already downloaded, respond right away
            file_query.respond( Ok(file) ).await.map_err(|e| e.into())
        } else { // in flight. respond once we get notified (means the query client should be prepared to wait)
            if let Some(connection) = &self.connection {
                connection.handle_file_query( &self.config, file_query, file).await
            } else {
                Err( op_failed("connection not initialized"))
            }
        }
    }

    fn terminate (&mut self) {
        if let Some(mut conn) = self.connection.as_mut() {
            conn.terminate();
            self.connection = None;
        }
    }

    fn max_history(&self)->usize {
        self.config.max_history_len
    }
}

/* #endregion LiveSentinelConnector */

/* #region LiveConnection ********************************************************************************************/

/// This is the internal workhorse struct that holds the data for a started LiveSentinelConnector, 
/// which includes the following tasks:
///   - websocket inbound (mostly notification of SensorRecord availability)
///   - websocket outbound (commands, including keepalive pings)
///   - websocket keepalive (scheduling ping messages for the IO task ) 
///   - file retrieval (for image files, which are downloaded automatically but independent of the websocket)
///   - file cleanup
///  
/// Note that our policy is to automatically initiate file downloads, i.e. subsequent file requests
/// from clients only have to be notified once a download has finished. This means for each incoming
/// request we either already have a file or we have a pending `FileRequest` entry that is used to
/// wake all requesters once the download is complete.
///
struct LiveConnection {
    hself: ActorHandle<SentinelActorMsg>, // the SentinelActor that uses this connection
    last_recv_epoch: Arc<AtomicU64>,

    ws_rx_task: AbortHandle, // async task for websocket input
    ws_tx_task: AbortHandle, // async task for websocket output
    ws_cmd_tx: MpscSender<String>, // channel to send websocket commands

    ping_task: Option<AbortHandle>, // optional periodic keepalive ping 

    file_request_task: AbortHandle, // async task for file requests
    file_request_tx: MpscSender<FileRequest>, // channel to send file requests to the task

    file_cleanup_task: AbortHandle, // periodic file cleanup task
}

impl LiveConnection {
    async fn new (config: Arc<SentinelConfig>, hself: ActorHandle<SentinelActorMsg>)->Result<Self> {
        let cache_dir = Arc::new(Self::sentinel_cache_dir());

        //--- get current sentinel data according to config (there is no point spawning tasks if we don't have a list of devices to watch)
        let http_client = Client::new();
        let mut sentinel_store = SentinelStore::new();
        sentinel_store.fetch_from_config( &http_client, &config).await?; // retrieve all records we need - this can take some time

        //--- now open a websocket and register for the devies we got
        let device_ids = sentinel_store.get_device_ids();
        debug!("monitored Sentinel devices: {:?}", device_ids);
        if !device_ids.is_empty() {
            let last_recv_epoch = Arc::new(AtomicU64::new(0));

            let file_fetcher = FileFetcher {
                config: config.clone(),
                cache_dir: cache_dir.clone(),
                client: http_client
            };

            let (file_request_task,file_request_tx) = file_fetcher.spawn( "sentinel-file_request", 64)?;

            let ws_stream = init_websocket( &config, device_ids).await?;
            let (ws_write, ws_read) = ws_stream.split();
            let ws_rx_task = spawn( "ws-sentinel-rx", 
                Self::ws_rx_loop( hself.clone(), config.clone(), cache_dir.clone(), file_request_tx.clone(), ws_read)
            )?.abort_handle();

            let (ws_cmd_tx, ws_cmd_rx) = create_mpsc_sender_receiver::<String>(16);
            let ws_tx_task = spawn( "ws-sentinel-tx", Self::ws_tx_loop(  ws_cmd_rx, ws_write))?.abort_handle();

            let file_cleanup_task = spawn( "sentinel-file-purge", 
                                           Self::file_cleanup_loop( config.clone(), cache_dir.clone()))?.abort_handle();

            let ping_task = match config.ping_interval {
                Some(interval) => {
                    let ws_cmd_tx = ws_cmd_tx.clone();
                    let ah = spawn( "ws-ping", Self::ws_ping_loop(interval, ws_cmd_tx))?.abort_handle();
                    Some(ah)
                }
                None => None
            };

            let live_conn = LiveConnection { 
                hself: hself.clone(), 
                last_recv_epoch, 
                ws_rx_task, ws_tx_task, ws_cmd_tx, 
                ping_task, 
                file_request_task, file_request_tx,
                file_cleanup_task,
            };
            live_conn.request_all_files( &config, &sentinel_store).await?;

            hself.send_msg( InitializeStore(sentinel_store)).await?;
            Ok(live_conn)

        } else {
            Err( OdinSentinelError::NoDevicesError)
        }
    }

    pub fn sentinel_cache_dir()->PathBuf {
        odin_build::cache_dir().join("sentinel")
    }

    /// the websocket receiver loop
    async fn ws_rx_loop (hself: ActorHandle<SentinelActorMsg>, config: Arc<SentinelConfig>, 
                           cache_dir: Arc<PathBuf>, file_request_tx: MpscSender<FileRequest>,
                           mut ws_read: SplitStream<WsStream>) -> Result<()> 
    {
        let client = reqwest::Client::new();
        loop {
            let res: Result<()> = if_let! {
                Some(m) = { ws_read.next().await } else { Err(OdinSentinelError::WsClosedError{}) }, // no use going on, terminate task
                Ok(Message::Text(json)) = { m } else { Ok(()) }, // ignore binary messages
                Ok(msg) = { serde_json::from_str::<WsMsg>(&json) } else { warn!("malformed websocket message {json}"); Ok(()) },
                WsMsg::Record { device_id, sensor_no, rec_type } = { msg } else { Ok(()) } => { // ignore other WsMsg variants
                    use SensorCapability::*;
                    match rec_type {
                        Accelerometer => Self::get_and_send_update::<AccelerometerData>( &hself, &client, &config, &device_id, sensor_no).await,
                        Anemometer    => Self::get_and_send_update::<AnemometerData>( &hself, &client, &config, &device_id, sensor_no).await,
                        Cloudcover    => Self::get_and_send_update::<CloudcoverData>( &hself, &client, &config, &device_id, sensor_no).await,
                        Event         => Self::get_and_send_update::<EventData>( &hself, &client, &config, &device_id, sensor_no).await,
                        Fire          => Self::get_and_send_update::<FireData>( &hself, &client, &config, &device_id, sensor_no).await,
                        Gas           => Self::get_and_send_update::<GasData>( &hself, &client, &config, &device_id, sensor_no).await,
                        Gps           => Self::get_and_send_update::<GpsData>( &hself, &client, &config, &device_id, sensor_no).await,
                        Gyroscope     => Self::get_and_send_update::<GyroscopeData>( &hself, &client, &config, &device_id, sensor_no).await,
                        Magnetometer  => Self::get_and_send_update::<MagnetometerData>( &hself, &client, &config, &device_id, sensor_no).await,
                        Orientation   => Self::get_and_send_update::<OrientationData>( &hself, &client, &config, &device_id, sensor_no).await,
                        Person        => Self::get_and_send_update::<PersonData>( &hself, &client, &config, &device_id, sensor_no).await,
                        Power         => Self::get_and_send_update::<PowerData>( &hself, &client, &config, &device_id, sensor_no).await,
                        Smoke         => Self::get_and_send_update::<SmokeData>( &hself, &client, &config, &device_id, sensor_no).await,
                        Thermometer   => Self::get_and_send_update::<ThermometerData>( &hself, &client, &config, &device_id, sensor_no).await,
                        Valve         => Self::get_and_send_update::<ValveData>( &hself, &client, &config, &device_id, sensor_no).await,
                        Voc           => Self::get_and_send_update::<VocData>( &hself, &client, &config, &device_id, sensor_no).await,

                        Image         => Self::get_and_send_image_update( &hself, &client, &config, &cache_dir, &file_request_tx,
                                                                            &device_id, sensor_no).await,
                    };
                    Ok(())
                }
            };
            if res.is_err() { return res }
        }
        Ok(())
    }

    async fn get_and_send_update<T>(hself: &ActorHandle<SentinelActorMsg>, client: &Client, config: &SentinelConfig, 
                                    device_id: &str, sensor_no: u32) -> Result<()> 
        where T: RecordDataBounds, SentinelUpdate: From<Arc<SensorRecord<T>>>
    {
        let update = Self::get_update::<T>( client, config, device_id, sensor_no).await?;
        Ok(hself.send_msg( UpdateStore( update)).await?)
    }

    async fn get_and_send_image_update (hself: &ActorHandle<SentinelActorMsg>, client: &Client, config: &SentinelConfig, 
                                        cache_dir: &PathBuf, file_request_tx: &MpscSender<FileRequest>,
                                        device_id: &str, sensor_no: u32) -> Result<()>  
    {
        let update = Self::get_update::<ImageData>( client, config, device_id, sensor_no).await?;
        match_algebraic_type! { update: SentinelUpdate as
            ref Arc<SensorRecord<ImageData>> => { Self::request_image_file( config, cache_dir, file_request_tx, update).await? }
            _ => {}
        }
        Ok(hself.send_msg( UpdateStore( update)).await?)
    }

    async fn get_update<T> (client: &Client, config: &SentinelConfig, device_id: &str, sensor_no: u32) -> Result<SentinelUpdate> 
        where T: RecordDataBounds, SentinelUpdate: From<Arc<SensorRecord<T>>>
    {
        let base_uri = config.base_uri.as_str();
        let access_token = config.access_token.as_str();
        get_latest_update::<T>( client, base_uri, access_token, device_id, sensor_no).await
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

    async fn request_all_files (&self, config: &SentinelConfig, sentinels: &SentinelStore) -> Result<()> {
        let sentinel_cache_dir = Self::sentinel_cache_dir();
        for sentinel in sentinels.values_iter() {
            for rec in &sentinel.image {
                Self::request_image_file( config, &sentinel_cache_dir, &self.file_request_tx, rec).await?;
            }
        }
        Ok(())
    }

    async fn request_image_file (config: &SentinelConfig, cache_dir: &PathBuf, file_request_tx: &MpscSender<FileRequest>, 
                                 rec: &SensorRecord<ImageData>) -> Result<()> 
    {
        let record_id = rec.id.clone();
        let uri = get_image_uri( &config.base_uri, &record_id);
        let pathname = cache_dir.join( &rec.data.filename);
        let sentinel_file = SentinelFile { record_id, pathname };
        let req = FileRequest { uri, sentinel_file, query: None };

        Ok(file_request_tx.send( req).await.map_err(|e| send_error("file request queue closed"))?)
    }

    async fn handle_file_query (&self, config: &SentinelConfig, query: Query<GetSentinelFile,Result<SentinelFile>>, sentinel_file: SentinelFile)->Result<()> {
        // currently filenames are globally unique, but that could change (in which case we have to lookup the record_id of the query)
        let record_id = &query.question.record_id;
        let filename = &query.question.filename;
        let uri = self.get_file_uri( config, &record_id, &filename)?;
        let request = FileRequest { uri, sentinel_file , query: Some(query)};

        self.file_request_tx.send(request).await.map_err(|e| OdinSentinelError::FileRequestError(e.to_string()))
    }

    fn get_file_uri (&self, config: &SentinelConfig, record_id: &str, filename: &str)->Result<String> {
        // for now we just have image files so there is only one uri scheme
        Ok( get_image_uri( &config.base_uri, record_id) )
    }

    async fn file_cleanup_loop (config: Arc<SentinelConfig>, cache_dir: Arc<PathBuf>)->Result<()> {
        let interval = minutes(60); // should we configure this?

        loop {
            sleep(interval).await;
            remove_old_files( cache_dir.as_ref(), config.max_age);
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

/* #endregion LiveConnection */

/* #region FileFetcher ***********************************************************************************************/

/// struct to request a SentinelFile from an external server
#[derive(Debug)]
struct FileRequest {
    uri: String,  // this is where we get the file from
    sentinel_file: SentinelFile, // this is where we store it
    query: Option<SentinelFileQuery> // in case this request came from an external entity
}


/// struct that holds all the info to resolve a Query<GetSentinelFile,Result<SentinelFile>>
struct FileFetcher {
    config: Arc<SentinelConfig>,
    cache_dir: Arc<PathBuf>,
    client: Client,
}

impl RequestProcessor<FileRequest,Result<SentinelFile>> for FileFetcher {

    async fn get_response_future (&self, req: Option<FileRequest>) -> Option<(FileRequest,Result<SentinelFile>)> {
        fn result (request: FileRequest)->Option<(FileRequest,Result<SentinelFile>)> {
            let response = Ok(request.sentinel_file.clone());
            Some( (request, response) )
        }

        if let Some(request) = req {
            if request.sentinel_file.pathname.is_file() { // we already have it
                result( request)
            } else {
                info!("downloading Sentinel file {:?}", request.sentinel_file.pathname);
                match get_file_request( &self.client, &self.config.access_token, &request.uri, &request.sentinel_file.pathname).await {
                    Ok(()) => result( request),
                    Err(e) => Some( (request, Err( OdinSentinelError::FileRequestError( e.to_string()))) )
                }
            }
        } else { None }
    }

    async fn process_response (&self, request: &FileRequest, response: SentinelFileResult)->odin_actor::Result<()> {
        if let Some(query) = &request.query {
            query.respond(response).await
        } else { 
            Ok(()) // nothing to do here - this request was internal and we only had to download the file
        }
    }

    fn is_same_request (&self, a: &FileRequest, b: &FileRequest) -> bool {
        a.sentinel_file.pathname == b.sentinel_file.pathname
    }
}

/* #endregion FileFetcher */