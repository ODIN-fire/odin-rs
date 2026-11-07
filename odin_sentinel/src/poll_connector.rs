/*
 * Copyright 2026 Peter Mehlitz
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://apache.org
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */
#![allow(unused)]

use std::{collections::HashMap, sync::Arc};
use odin_build::pkg_cache_dir;
use reqwest::Client;
use async_trait::async_trait;

use odin_actor::prelude::*;

use crate::*;
use crate::actor::*;
use crate::errors::*;
use crate::ws::WsCmd;

/* #region PollConnector *************************************************************************************/

/// a [`SentinelConnector`] implementation that does not use a websocket but only polls for new records in the
/// configured `SentinelConfig.poll_interval`. This is useful if websocket connections are not available or not
/// reliable. Note there is no channel to send commands to devices without a websocket, and that only sensors which
/// had at least one record during initialization are polled.
///
/// Like [`WsConnector`] this is created before the [`SentinelActor`] and its `PollConnection` (that does the
/// real work) is created once the actor calls `SentinelConnector::start(actor_handle)`
pub struct PollConnector {
    config: Arc<SentinelConfig>,
    poll_interval: Duration,
    connection: Option<PollConnection>
}

impl PollConnector {

    /// called before actor instantiation. Fails if the config does not have a poll_interval
    pub fn new (config: SentinelConfig)->Result<Self> {
        let poll_interval = config.poll_interval.ok_or( op_failed("no poll_interval configured"))?;
        Ok( PollConnector { config: Arc::new(config), poll_interval, connection: None } )
    }

    /// called from actor ctor (2nd half of our initialization)
    async fn initialize (&mut self, hself: ActorHandle<SentinelActorMsg>)->Result<()> {
        self.connection = Some(PollConnection::new( self.config.clone(), self.poll_interval, hself).await?);
        Ok(())
    }

}

/// this is the interface used by the [`SentinelActor`]
#[async_trait]
impl SentinelConnector for PollConnector {
    async fn start (&mut self, hself: ActorHandle<SentinelActorMsg>)->Result<()> {
        self.initialize(hself).await
    }

    async fn send_cmd (&mut self, cmd: WsCmd)->Result<()> {
        Err( op_failed("commands not supported without websocket (poll mode)"))
    }

    async fn handle_sentinel_file_query (&self, query: Query<GetSentinelFile,Result<SentinelFile>>)->Result<()> {
        let file = sentinel_file_for_query( &query);
        if file.pathname.is_file() { // already downloaded, respond right away
            query.respond( Ok(file) ).await.map_err(|e| e.into())
        } else { // in flight. respond once we get notified (means the query client should be prepared to wait)
            if let Some(connection) = &self.connection {
                handle_file_query( &self.config, &connection.file_request_tx, query, file).await
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

    fn inactive_duration(&self)->Duration {
        self.config.inactive_duration
    }

    fn inactive_interval(&self)->Duration {
        self.config.inactive_interval
    }
}

/* #endregion PollConnector */

/* #region PollConnection ********************************************************************************************/

/// the internal struct that holds the tasks of a started PollConnector:
///   - polling for records that are newer than the latest ones we know of
///   - file retrieval (for image files, which are downloaded automatically)
///   - file cleanup
struct PollConnection {
    poll_task: AbortHandle,

    file_request_task: AbortHandle,
    file_request_tx: MpscSender<FileRequest>,

    file_cleanup_task: AbortHandle,
}

impl PollConnection {
    async fn new (config: Arc<SentinelConfig>, poll_interval: Duration, hself: ActorHandle<SentinelActorMsg>)->Result<Self> {
        let cache_dir = Arc::new(pkg_cache_dir!());

        //--- get current sentinel data according to config (there is no point spawning tasks if we don't have devices to watch)
        let http_client = get_http_client()?;

        let mut sentinel_store = SentinelStore::new();
        sentinel_store.fetch_from_config( &http_client, &config).await?; // retrieve all records we need - this can take some time

        let latest_recs = sentinel_store.latest_records();
        let device_ids = sentinel_store.get_device_ids();
        debug!("polled Sentinel devices: {:?}", device_ids);

        if !device_ids.is_empty() {
            let file_fetcher = FileFetcher {
                config: config.clone(),
                cache_dir: cache_dir.clone(),
                client: http_client
            };
            let (file_request_task,file_request_tx) = file_fetcher.spawn( "sentinel-file_request", 64)?;

            let poll_task = spawn( "poll-sentinel",
                Self::poll_loop( hself.clone(), config.clone(), poll_interval, cache_dir.clone(), get_http_client()?,
                                 latest_recs, file_request_tx.clone())
            )?.abort_handle();

            let file_cleanup_task = spawn( "sentinel-file-purge",
                                           file_cleanup_loop( config.clone(), cache_dir.clone()))?.abort_handle();

            request_all_files( &config, &sentinel_store, &file_request_tx).await?;
            hself.send_msg( InitializeStore(sentinel_store)).await?;

            Ok( PollConnection { poll_task, file_request_task, file_request_tx, file_cleanup_task } )

        } else {
            Err( OdinSentinelError::NoDevicesError)
        }
    }

    /// the websocket-less part of WsConnector's ws_loop(): periodically retrieve and send all records that are newer
    /// than the latest ones we know of
    async fn poll_loop (hself: ActorHandle<SentinelActorMsg>, config: Arc<SentinelConfig>, poll_interval: Duration,
                        cache_dir: Arc<PathBuf>, client: Client,
                        mut latest_recs: HashMap<String,String>, file_request_tx: MpscSender<FileRequest>) {
        let mut cycle = 0;

        loop {
            odin_actor::sleep( poll_interval).await;

            cycle += 1;
            debug!("polling update records cycle {}", cycle);
            if let Err(e) = get_and_send_missing_updates( &hself, &client, &config, &mut latest_recs, &cache_dir, &file_request_tx).await {
                warn!("failed to poll Sentinel updates: {e}");
            }
        }
    }

    fn terminate (&mut self) {
        self.poll_task.abort();
        self.file_request_task.abort();
        self.file_cleanup_task.abort();
    }
}

/* #endregion PollConnection */
