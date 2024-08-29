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
use futures::{TryFutureExt, stream::{StreamExt,SplitStream,SplitSink}, SinkExt};
use tokio_tungstenite::{tungstenite::protocol::Message, MaybeTlsStream};
use tokio::{select,time::{sleep,Sleep}};
use reqwest::{Client};
use async_trait::async_trait;

use odin_actor::prelude::*;
use odin_common::{fs::{ensure_writable_dir, remove_old_files}, if_let, strings::str_from_last, collections::Snapshot, admin};

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
        let pathname = sentinel_cache_dir().join(&query.question.filename);
        SentinelFile { record_id, pathname }   
    }
}

/// this is the interface used by the [`SentinelActor`] 
#[async_trait]
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

    async fn handle_sentinel_file_query (&self, query: Query<GetSentinelFile,Result<SentinelFile>>)->Result<()> {
        let file = self.sentinel_file_for_query( &query);
        if file.pathname.is_file() { // already downloaded, respond right away
            query.respond( Ok(file) ).await.map_err(|e| e.into())
        } else { // in flight. respond once we get notified (means the query client should be prepared to wait)
            if let Some(connection) = &self.connection {
                connection.handle_file_query( &self.config, query, file).await
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

    ws_task: AbortHandle, // async task for websocket input
    ws_cmd_tx: MpscSender<String>,

    file_request_task: AbortHandle, // async task for file requests
    file_request_tx: MpscSender<FileRequest>, // channel to send file requests to the task

    file_cleanup_task: AbortHandle, // periodic file cleanup task
}

impl LiveConnection {
    async fn new (config: Arc<SentinelConfig>, hself: ActorHandle<SentinelActorMsg>)->Result<Self> {
        let cache_dir = Arc::new(sentinel_cache_dir());

        //--- get current sentinel data according to config (there is no point spawning tasks if we don't have a list of devices to watch)
        let http_client = Client::new();
        let mut sentinel_store = SentinelStore::new();
        sentinel_store.fetch_from_config( &http_client, &config).await?; // retrieve all records we need - this can take some time

        let mut latest_recs = sentinel_store.latest_records();

        //--- now open a websocket and register for the devices we've got (note that config might have a device_filter set)
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

            let (ws_cmd_tx, ws_cmd_rx) = create_mpsc_sender_receiver::<String>(16);

            let ws_task = spawn( "ws-sentinel", 
                Self::ws_loop( hself.clone(), config.clone(), cache_dir.clone(),
                               device_ids, latest_recs, 
                               file_request_tx.clone(), ws_cmd_rx)
            )?.abort_handle();

            let file_cleanup_task = spawn( "sentinel-file-purge", 
                                           Self::file_cleanup_loop( config.clone(), cache_dir.clone()))?.abort_handle();

            let live_conn = LiveConnection { 
                hself: hself.clone(), 
                last_recv_epoch, 
                ws_task, ws_cmd_tx,
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

    async fn ws_loop (hself: ActorHandle<SentinelActorMsg>, config: Arc<SentinelConfig>, cache_dir: Arc<PathBuf>, 
                      device_ids: Vec<String>, mut latest_recs: HashMap<String,String>,
                      file_request_tx: MpscSender<FileRequest>, ws_cmd_rx: MpscReceiver<String>) {
        let mut cycle = 0;
        let client = reqwest::Client::new();
        let ping_interval = if let Some(dur) = config.ping_interval { dur } else { Duration::MAX };

        loop {
            cycle += 1;
            if cycle > 1 {
                Self::get_and_send_missing_updates( &hself, &client, &config, &mut latest_recs, &cache_dir, &file_request_tx).await;
            }

            if let Ok(mut ws_stream) =  init_websocket( &config, &device_ids).await {
                admin::async_notify_info("websocket connected").await;

                loop {
                    select! { // NOTE - this requires all awaited futures to be cancellation safe !
                        //--- ws read (record availability notifications)
                        maybe_msg = ws_stream.next() => {
                            match maybe_msg {
                                Some(msg) => match msg {
                                    Ok(msg) => {
                                        if let Err(e) = Self::process_incoming_msg( &hself, &client, &config, msg, &mut latest_recs, &cache_dir, &file_request_tx).await {
                                            warn!("ignoring incoming websocket msg: {}", e)
                                        };
                                    }
                                    Err(e) => {
                                        let msg = format!("reconnecting after failed websocket read: {}", e);
                                        admin::async_notify_severe(&msg).await;
                                        warn!("{}", msg);
                                        break; // do we have to check the tungstenite::error::Error variant? I seems they all warrant restart
                                    }
                                }
                                None => {
                                    let msg = "reconnecting after websocket closed";
                                    admin::async_notify_severe(msg).await;
                                    warn!("{}", msg);
                                    break; // try to re-connect
                                }
                            }
                        }
    
                        //--- ws write (commands) 
                        maybe_cmd = recv(&ws_cmd_rx) => {
                            match maybe_cmd {
                                Ok(cmd) => {
                                    if let Err(e) = ws_stream.send( Message::Text(cmd)).await {
                                        let msg = format!("reconnecting after failed websocket write: {}", e);
                                        admin::async_notify_severe(&msg).await;
                                        warn!("{}", msg);
                                        break; // try to re-connect
                                    }
                                }
                                Err(e) => {
                                    // cmd queue closed - terminate (this is nominal termination, no error)
                                    return
                                }
                            }
                        }

                        //--- ping_interval timeout
                        // unfortunately a guard does not shortcut evaluating the async expression, it just enables/disables polling the future
                        _ = sleep( ping_interval), if config.ping_interval.is_some() => { 
                            let msg = WsCmd::new_ping("ping");
                            if let Ok(msg) = serde_json::to_string( &msg) {
                                if let Err(e) = ws_stream.send(Message::Text(msg)).await {
                                    let msg = format!("reconnecting after failed websocket write: {}", e);
                                    admin::async_notify_severe(&msg).await;
                                    warn!("{}", msg);
                                    break; // try to re-connect    
                                }
                            }
                        }
                    }
                }
            } else { // init_websocket failed
                if let Some(reconnect_delay) = config.reconnect_delay {
                    warn!("failed to initialize websocket, retry in {} sec", reconnect_delay.as_secs());
                    sleep(reconnect_delay).await;
                } else {
                    break;
                }
            }
        }
        error!("websocket processing terminated.")
    }

    async fn process_incoming_msg (hself: &ActorHandle<SentinelActorMsg>, client: &Client, config: &SentinelConfig,
                                   msg: Message,
                                   latest_recs: &mut HashMap<String,String>, 
                                   cache_dir: &PathBuf, file_request_tx: &MpscSender<FileRequest>)->Result<()> {
        if_let! {
            Message::Text(json) = { msg } else { Err(ws_protocol_error("ignored binary message")) }, // ignore binary messages
            Ok(msg) = { serde_json::from_str::<WsMsg>(&json) } else { warn!("malformed websocket message {json}"); Err(ws_protocol_error("malformed message")) },
            WsMsg::Record { device_id, sensor_no, rec_type } = { msg } else { Err(ws_protocol_error("unknown record type")) } => { // ignore other WsMsg variants
                use SensorCapability::*;
                match rec_type {
                    Accelerometer => Self::get_and_send_update::<AccelerometerData>( hself, client, config, &device_id, sensor_no, latest_recs).await,
                    Anemometer    => Self::get_and_send_update::<AnemometerData>( hself, client, config, &device_id, sensor_no, latest_recs).await,
                    Cloudcover    => Self::get_and_send_update::<CloudcoverData>( hself, client, config, &device_id, sensor_no, latest_recs).await,
                    Event         => Self::get_and_send_update::<EventData>( hself, client, config, &device_id, sensor_no, latest_recs).await,
                    Fire          => Self::get_and_send_update::<FireData>( hself, client, config, &device_id, sensor_no, latest_recs).await,
                    Gas           => Self::get_and_send_update::<GasData>( hself, client, config, &device_id, sensor_no, latest_recs).await,
                    Gps           => Self::get_and_send_update::<GpsData>( hself, client, config, &device_id, sensor_no, latest_recs).await,
                    Gyroscope     => Self::get_and_send_update::<GyroscopeData>( hself, client, config, &device_id, sensor_no, latest_recs).await,
                    Magnetometer  => Self::get_and_send_update::<MagnetometerData>( hself, client, config, &device_id, sensor_no, latest_recs).await,
                    Orientation   => Self::get_and_send_update::<OrientationData>( hself, client, config, &device_id, sensor_no, latest_recs).await,
                    Person        => Self::get_and_send_update::<PersonData>( hself, client, config, &device_id, sensor_no, latest_recs).await,
                    Power         => Self::get_and_send_update::<PowerData>( hself, client, config, &device_id, sensor_no, latest_recs).await,
                    Smoke         => Self::get_and_send_update::<SmokeData>( hself, client, config, &device_id, sensor_no, latest_recs).await,
                    Thermometer   => Self::get_and_send_update::<ThermometerData>( hself, client, config, &device_id, sensor_no, latest_recs).await,
                    Valve         => Self::get_and_send_update::<ValveData>( hself, client, config, &device_id, sensor_no, latest_recs).await,
                    Voc           => Self::get_and_send_update::<VocData>( hself, client, config, &device_id, sensor_no, latest_recs).await,

                    Image         => Self::get_and_send_image_update( hself, client, config, &device_id, sensor_no, latest_recs, cache_dir, file_request_tx).await,
                }
            }
        }
    }

    async fn get_and_send_update<T>(hself: &ActorHandle<SentinelActorMsg>, client: &Client, config: &SentinelConfig, 
                                    device_id: &str, sensor_no: u32, latest_recs: &mut HashMap<String,String>) -> Result<()> 
        where T: RecordDataBounds, SentinelUpdate: From<Arc<SensorRecord<T>>>
    {
        let rec = get_latest_record::<T>(client, &config.base_uri, &config.access_token, device_id, sensor_no).await?;
        let update = SentinelUpdate::from(Arc::new(rec));
        Self::update_latest_recs( latest_recs, &update);
        hself.send_msg( UpdateStore( update)).await?;

        Ok(())
    }

    fn update_latest_recs (latest_recs: &mut HashMap<String,String>, update: &SentinelUpdate) {
        let rec_key = rec_key( update.device_id(), update.sensor_no(), update.capability());
        latest_recs.insert(rec_key, update.record_id().clone());
    }

    async fn get_and_send_image_update (hself: &ActorHandle<SentinelActorMsg>, client: &Client, config: &SentinelConfig, 
                                        device_id: &str, sensor_no: u32, latest_recs: &mut HashMap<String,String>,
                                        cache_dir: &PathBuf, file_request_tx: &MpscSender<FileRequest> ) -> Result<()>  
    {
        let mut rec = get_latest_record::<ImageData>(client, &config.base_uri, &config.access_token, device_id, sensor_no).await?;
        rec.set_local_filename();

        Self::request_image_file( config, cache_dir, file_request_tx, &rec).await?;
        let update = SentinelUpdate::from(Arc::new(rec));
        Self::update_latest_recs( latest_recs, &update);
        hself.send_msg( UpdateStore( update)).await?;

        Ok(())
    }

    async fn get_and_send_missing_updates (hself: &ActorHandle<SentinelActorMsg>, client: &Client, config: &SentinelConfig, 
                                           latest_recs: &mut HashMap<String,String>,
                                           cache_dir: &PathBuf, file_request_tx: &MpscSender<FileRequest> )->Result<()> {
        let base_uri = config.base_uri.as_str();
        let access_token = config.access_token.as_str();

        for (uri_path, rec_id) in latest_recs.snapshot() {
            if let Some(rec_type) = str_from_last( &uri_path, '/') {
                use SensorCapability::*;

                let res = match SensorCapability::capability_of(rec_type) {
                    Some(Accelerometer)  => Self::get_and_send_missing::<AccelerometerData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                    Some(Anemometer)     => Self::get_and_send_missing::<AnemometerData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                    Some(Cloudcover)     => Self::get_and_send_missing::<CloudcoverData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                    Some(Event)          => Self::get_and_send_missing::<EventData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                    Some(Fire)           => Self::get_and_send_missing::<FireData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                    Some(Gas)            => Self::get_and_send_missing::<GasData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                    Some(Gps)            => Self::get_and_send_missing::<GpsData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                    Some(Gyroscope)      => Self::get_and_send_missing::<GyroscopeData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                    Some(Magnetometer)   => Self::get_and_send_missing::<MagnetometerData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                    Some(Orientation)    => Self::get_and_send_missing::<OrientationData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                    Some(Person)         => Self::get_and_send_missing::<PersonData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                    Some(Power)          => Self::get_and_send_missing::<PowerData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                    Some(Smoke)          => Self::get_and_send_missing::<SmokeData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                    Some(Thermometer)    => Self::get_and_send_missing::<ThermometerData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                    Some(Valve)          => Self::get_and_send_missing::<ValveData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                    Some(Voc)            => Self::get_and_send_missing::<VocData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,

                    Some(Image)          => Self::get_and_send_missing_images( hself, client, config, &uri_path, &rec_id, latest_recs, cache_dir, file_request_tx).await,

                    None => Err( op_failed("unknown capability")) 
                };
                if let Err(e) = res { warn!("failed to get missing updates for {uri_path}: {e}") }
            }
        }

        Ok(())
    }

    async fn get_and_send_missing<T> (hself: &ActorHandle<SentinelActorMsg>, 
                                      client: &Client, config: &SentinelConfig, uri_path: &str, last: &str, 
                                      latest_recs: &mut HashMap<String,String>) -> Result<()> 
        where T: RecordDataBounds, SentinelUpdate: From<Arc<SensorRecord<T>>>
    {
        let recs = get_records_since::<T>(client, &config.base_uri, &config.access_token, uri_path, last).await?;
        for rec in recs.into_iter() {
            let update = SentinelUpdate::from(Arc::new(rec));
            Self::update_latest_recs( latest_recs, &update);
            hself.send_msg( UpdateStore(update)).await?;
        }

        Ok(())
    }

    async fn get_and_send_missing_images (hself: &ActorHandle<SentinelActorMsg>, 
                                      client: &Client, config: &SentinelConfig, uri_path: &str, last: &str, 
                                      latest_recs: &mut HashMap<String,String>,
                                      cache_dir: &PathBuf, file_request_tx: &MpscSender<FileRequest> ) -> Result<()> 
    {
        let recs = get_records_since::<ImageData>(client, &config.base_uri, &config.access_token, uri_path, last).await?;
        for mut rec in recs.into_iter() {
            rec.set_local_filename();

            Self::request_image_file( config, cache_dir, file_request_tx, &rec).await?;
            let update = SentinelUpdate::from(Arc::new(rec));
            Self::update_latest_recs( latest_recs, &update);
            hself.send_msg( UpdateStore(update)).await?;
        }

        Ok(())
    }

    async fn request_all_files (&self, config: &SentinelConfig, sentinels: &SentinelStore) -> Result<()> {
        let sentinel_cache_dir = sentinel_cache_dir();
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
        // it would be easier if we would only have one meaningful filename
        let pathname = if let Some(local_name) = &rec.data.local_filename { cache_dir.join( local_name) } else { cache_dir.join( &rec.odin_filename())};
        let sentinel_file = SentinelFile { record_id, pathname };
        let req = FileRequest { uri, sentinel_file, query: None };

        Ok(file_request_tx.send( req).await.map_err(|e| send_error("file request queue closed"))?)
    }

    async fn handle_file_query (&self, config: &SentinelConfig, query: Query<GetSentinelFile,Result<SentinelFile>>, sentinel_file: SentinelFile)->Result<()> {
        // currently filenames are globally unique, but that could change (in which case we have to lookup the record_id of the query)
        let record_id = &query.question.record_id;
        let filename = &query.question.filename;

        let uri = if let Some(uri) = &query.question.uri { uri.clone() } else { self.get_file_uri( config, &record_id, &filename)? };
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
            // remove first so that frequent short runs that don't hit the interval do not accumulate
            remove_old_files( cache_dir.as_ref(), config.max_age);
            sleep(interval).await;
        }
        Ok(())
    }

    fn terminate(&mut self)->Result<()> {
        self.ws_task.abort();
        self.file_request_task.abort();
        self.file_cleanup_task.abort();

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