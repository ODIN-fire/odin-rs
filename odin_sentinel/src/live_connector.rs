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

use std::{collections::HashMap,sync::{Arc,atomic::AtomicU64}};
use odin_actor::tokio_kanal::{ActorHandle,Query,AbortHandle,JoinHandle, MpscSender,MpscReceiver,spawn};
use futures::stream::{StreamExt,SplitStream,SplitSink};
use tokio_tungstenite::tungstenite::protocol::Message;
use odin_common::{fs::remove_old_files};
use reqwest::{Client};
use crate::*;
use crate::actor::*;
use crate::ws::{WsStream,WsCmd,WsMsg, init_websocket, send_ws_text_msg, read_next_ws_msg};
use odin_actor::prelude::*;

const PING_TIMER: i64 = 1;
const FILE_TIMER: i64 = 2;


define_struct! { pub LiveSentinelConnector = 
    config: Arc<SentinelConfig>,
    last_recv_epoch: Arc<AtomicU64> = Arc::new(AtomicU64::new(0)),

    ping_timer: Option<AbortHandle> = None,
    file_timer: Option<AbortHandle> = None,

    websocket_task: Option<JoinHandle<Result<()>>> = None,
    ws_write: Option<SplitSink<WsStream,Message>> = None,

    file_request_task: Option<JoinHandle<Result<()>>> = None,
    file_request_queue: Option<MpscSender<SentinelFileRequest>> = None,
    pending_file_requests: HashMap<PathBuf,Arc<Notify>> = HashMap::new()
}

impl LiveSentinelConnector {

    async fn spawn_websocket_task (&mut self, device_ids: Vec<String>, hself: &ActorHandle<SentinelConnectorMsg>)->Result<()> {
        let hself = hself.clone();
        let config = self.config.clone();

        let ws_stream = init_websocket(self.config.clone(), device_ids).await?;
        let (ws_write, ws_read) = ws_stream.split();
        self.ws_write = Some(ws_write);

        self.websocket_task = Some( spawn( Self::run_websocket( hself.clone(), config, ws_read)) );
        if let Some(interval) = self.config.ping_interval {
            self.ping_timer = Some( hself.start_repeat_timer( PING_TIMER, interval) )
        }
        Ok(())
    }

    // this is running in a spawned task so we can't capture &self
    async fn run_websocket (hself: ActorHandle<SentinelConnectorMsg>, config: Arc<SentinelConfig>, mut ws_read: SplitStream<WsStream>)->Result<()> {
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
                                            get_and_send_record( &hself, &http_client, config.as_ref(), device_id.as_str(), sensor_no, rec_type).await?;
                                        }
                                        WsMsg::Pong { request_time, response_time, message_id } => {}
                                        WsMsg::Error { message } => {}
                                        _ => {} // ignore other messages
                                    }
                                }
                                Err(e) => {
                                    hself.send_msg( OdinSentinelError::JsonError(e)).await;
                                }
                            }
                        }
                        Ok(Message::Binary(bs)) => {} // TODO unexpected binary message
                        Ok(Message::Ping(_)) => {} // system level ping
                        Ok(Message::Pong(_)) => {} // system level pong
                        Ok(Message::Close(_)) => {} // TODO 
                        Ok(Message::Frame(_)) => {}
                        Err(e) => {
                            hself.send_msg( OdinSentinelError::WsError(e)).await;
                            // if this causes the websocket to be closed we still catch it in next() so we don't have to break here
                        } 
                    }
                }
                None => { // stream closed by server
                    hself.send_msg( OdinSentinelError::WsClosedError{}).await;
                    return Err(OdinSentinelError::WsClosedError{})
                }
            }
        }
    
        Ok(())
    }    

    pub async fn get_and_send_record (hself: &ActorHandle<SentinelConnectorMsg>, client: &Client, config: &SentinelConfig,
                                      device_id: &str, sensor_no: u32, capability: SensorCapability) -> Result<()> 
    {
        let base_uri = config.base_uri.as_str();
        let access_token = config.access_token.as_str();
        let data_dir = &config.data_dir;

        use SensorCapability::*;
        match capability {
            Accelerometer => Ok(hself.send_msg( get_latest_update::<AccelerometerData>(client, base_uri, access_token, device_id, sensor_no).await?).await?),
            Anemometer    => Ok(hself.send_msg( get_latest_update::<AnemometerData>(client, base_uri, access_token, device_id, sensor_no).await?).await?),
            Cloudcover    => Ok(hself.send_msg( get_latest_update::<CloudcoverData>(client, base_uri, access_token, device_id, sensor_no).await?).await?),
            Fire          => Ok(hself.send_msg( get_latest_update::<FireData>(client, base_uri, access_token, device_id, sensor_no).await?).await?),
            Gas           => Ok(hself.send_msg( get_latest_update::<GasData>(client, base_uri, access_token, device_id, sensor_no).await?).await?),
            Gps           => Ok(hself.send_msg( get_latest_update::<GpsData>(client, base_uri, access_token, device_id, sensor_no).await?).await?),
            Gyroscope     => Ok(hself.send_msg( get_latest_update::<GyroscopeData>(client, base_uri, access_token, device_id, sensor_no).await?).await?),
            Image         => Ok(hself.send_msg( get_latest_update::<Image>(client, base_uri, access_token, device_id, sensor_no).await?).await?),
            Magnetometer  => Ok(hself.send_msg( get_latest_update::<MagnetometerData>(client, base_uri, access_token, device_id, sensor_no).await?).await?),
            Orientation   => Ok(hself.send_msg( get_latest_update::<OrientationData>(client, base_uri, access_token, device_id, sensor_no).await?).await?),
            Person        => Ok(hself.send_msg( get_latest_update::<PersonData>(client, base_uri, access_token, device_id, sensor_no).await?).await?),
            Power         => Ok(hself.send_msg( get_latest_update::<PowerData>(client, base_uri, access_token, device_id, sensor_no).await?).await?),
            Smoke         => Ok(hself.send_msg( get_latest_update::<SmokeData>(client, base_uri, access_token, device_id, sensor_no).await?).await?),
            Thermometer   => Ok(hself.send_msg( get_latest_update::<ThermometerData>(client, base_uri, access_token, device_id, sensor_no).await?).await?),
            Valve         => Ok(hself.send_msg( get_latest_update::<ValveData>(client, base_uri, access_token, device_id, sensor_no).await?).await?),
            Voc           => Ok(hself.send_msg( get_latest_update::<VocData>(client, base_uri, access_token, device_id, sensor_no).await?).await?),
        }
    }
 
    fn stop_websocket_task (&mut self) {
        self.ws_write = None;

        if let Some(abort_handle) = &self.ping_timer {
            abort_handle.abort()
        }

        if let Some(join_handle) = &self.websocket_task {
            if !join_handle.is_finished() {
                join_handle.abort();
            }
            self.websocket_task = None;
        }
    }

    fn spawn_file_request_task (&mut self, hself: ActorHandle<SentinelConnectorMsg>)->Result<()> {
        let hself = hself.clone();
        let config = self.config.clone();
        let client = reqwest::Client::new();

        let (tx,rx) = create_mpsc_sender_receiver::<SentinelFileRequest>(128);

        self.file_request_task = Some( spawn( async move {
            while let Ok(mut req) = rx.recv().await {
                let result = get_file_request( &client, &config.access_token, &req).await;
                match result {
                    Ok(()) => {
                        req.notify.notify_waiters();
                        let pr = self.pending_file_requests.lock();
                        pr.remove( &req.uri)
                    }
                    Err(e) => { 
                        hself.send_msg( OdinSentinelError::FileRequestError(format!("{} (:?}", req.uri, e))) 
                    }
                }
            }
            Ok(())
        }));

        self.file_request_queue = Some(tx);
        
        Ok(())
    }

    fn cleanup_files (&self) {
        let config = &self.config;
        remove_old_files( &config.data_dir, config.max_age);
    }

    fn stop_file_request_task (&mut self) {
        if let Some(join_handle) = &self.file_request_task {
            if !join_handle.is_finished() {
                join_handle.abort();
            }
            self.file_request_task = None;
        }

        self.file_request_queue = None;
    }
}

impl SentinelConnector for LiveSentinelConnector {

    async fn start (&mut self, hself: ActorHandle<SentinelDbMsg>)->Result<()> {
        let http_client = Client::new();
        let mut sentinel_store = SentinelStore::new();

        sentinel_store.fetch_from_config( &http_client, &config).await?; // this can take some time

        let device_ids = sentinel_store.get_device_ids();
        if !device_ids.is_empty() {
            match self.spawn_websocket_task( device_ids, &hself) {
                Ok(()) => {
                    self.spawn_file_request_task( hself.clone())?;
                    self.file_timer = Some( hself.start_repeat_timer( FILE_TIMER, Duration::from_secs( 60*60))); // run cleanup once per hour

                    Ok(hself.send_msg( InitializeDb(sentinel_store)).await?) // inform the actor we are up&running
                }
                Err(e) => Ok(hself.send_msg( e).await?)
            }

        } else {
            Ok(hself.send_msg( OdinSentinelError::NoDevicesError)?)
        }
    }

    async fn send_cmd (&mut self, cmd: WsCmd)->Result<()> {
        if let Some(mut tx) = self.ws_write.as_mut() { 
            let json = serde_json::to_string(&cmd)?;
            Ok(send_ws_text_msg( &mut tx, json).await?)

        } else {
            Err( op_failed("no websocket"))
        }
    }

    async fn request_image_file (&mut self, rec: &SensorRecord<ImageData>)->Result<()> {
        if let Some(tx) = self.file_request_queue {
            let uri = get_image_uri( &config.base_uri, &rec.id);
            let req = SentinelFileRequest::for_image_record( &rec, &config.data_dir, uri);

            self.pending_file_requests.insert( req.sentinel_file.pathname.clone(), req.notify.clone());
            Ok(tx.write( req).await?)
        } else {
            Err( op_failed("no file request queue"))
        }
    }

    async fn handle_file_query (&self, file_query: Query<SentinelFile,Option<PathBuf>>) {
        let pn = file_query.question.pathname.clone();
        if pn.is_file() { // already downloaded, respond right away
            file_query.respond(Some(pn)).await

        } else { // in flight. respond once we get notified (means the query client should be prepared to wait)
            if let Some(req) = self.pending_file_requests.get( &file_query.question.uri) {
                let notify = req.notify.clone();
                spawn( async move {
                    notify.notified().await;
                    file_query.respond( Some(pn)).await;
                })
            } else {
                file_query.respond(None).await;
            }
        }
    }

    fn terminate (&mut self) {
        self.stop_file_task();
        self.stop_websocket_task();
    }
}