/*
 * Copyright © 2026, United States Government, as represented by the Administrator of
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

//! the [`SentinelConnector`] abstraction and functions that are shared between connector implementations
//! (e.g. [`WsConnector`] and [`PollConnector`]): retrieval of records that are newer than the latest known ones and
//! (automatic) download of image files

use std::{collections::HashMap, fs::File, io::Write, sync::Arc};
use reqwest::Client;
use async_trait::async_trait;

use odin_actor::prelude::*;
use odin_common::{fs::remove_old_files, strings::str_from_last, collections::Snapshot};

use crate::*;
use crate::actor::*;
use crate::errors::*;
use crate::ws::WsCmd;

/* #region SentinelConnector trait ********************************************************************************************************/

/// this is the abstraction over the actual source of external information, which can be either a live connection to a Delphire server
/// or a replayer for archived data
#[async_trait]
pub trait SentinelConnector {

    async fn start (&mut self, hself: ActorHandle<SentinelActorMsg>) -> Result<()>;

    // note that these future do not wait until the request was resolved - only until the request has been queued (if it needs to)

    async fn send_cmd (&mut self, cmd: WsCmd) -> Result<()>;

    async fn handle_sentinel_file_query (&self, file_query: Query<GetSentinelFile,Result<SentinelFile>>) -> Result<()>;

    fn terminate (&mut self);

    fn max_history(&self)->usize;

    /// duration since last update after which we consider a device inactive
    fn inactive_duration(&self)->Duration;

    /// duration how often we check for inactive status
    fn inactive_interval(&self)->Duration;
 }

/* #endregion SentinelConnector trait */

/* #region shared connector functions ******************************************************************************/

pub(crate) fn sentinel_file_for_query (query: &Query<GetSentinelFile,Result<SentinelFile>>)->SentinelFile {
    let record_id = query.question.record_id.clone();
    let pathname = sentinel_cache_dir().join(&query.question.filename);
    SentinelFile { record_id, pathname }
}

pub(crate) fn update_latest_recs (latest_recs: &mut HashMap<String,String>, update: &SentinelUpdate) {
    let rec_key = rec_key( update.device_id(), update.sensor_no(), update.capability());
    latest_recs.insert(rec_key, update.record_id().clone());
}

pub(crate) async fn get_and_send_missing_updates (hself: &ActorHandle<SentinelActorMsg>, client: &Client, config: &SentinelConfig,
                                       latest_recs: &mut HashMap<String,String>,
                                       cache_dir: &PathBuf, file_request_tx: &MpscSender<FileRequest> )->Result<()> {
    let base_uri = config.base_uri.as_str();
    let access_token = config.access_token.as_str();

    for (uri_path, rec_id) in latest_recs.snapshot() {
        if let Some(rec_type) = str_from_last( &uri_path, '/') {
            use SensorCapability::*;

            let res = match SensorCapability::capability_of(rec_type) {
                Some(Accelerometer)  => get_and_send_missing::<AccelerometerData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                Some(Anemometer)     => get_and_send_missing::<AnemometerData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                Some(Cloudcover)     => get_and_send_missing::<CloudcoverData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                Some(Detection)      => get_and_send_missing::<DetectionData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                Some(Event)          => get_and_send_missing::<EventData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                Some(Fire)           => get_and_send_missing::<FireData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                Some(Gas)            => get_and_send_missing::<GasData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                Some(Gps)            => get_and_send_missing::<GpsData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                Some(Gyroscope)      => get_and_send_missing::<GyroscopeData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                Some(Magnetometer)   => get_and_send_missing::<MagnetometerData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                Some(Orientation)    => get_and_send_missing::<OrientationData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                Some(Person)         => get_and_send_missing::<PersonData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                Some(Power)          => get_and_send_missing::<PowerData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                Some(Smoke)          => get_and_send_missing::<SmokeData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                Some(Thermometer)    => get_and_send_missing::<ThermometerData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                Some(Valve)          => get_and_send_missing::<ValveData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,
                Some(Voc)            => get_and_send_missing::<VocData>( hself, client, config, &uri_path, &rec_id, latest_recs).await,

                Some(Image)          => get_and_send_missing_images( hself, client, config, &uri_path, &rec_id, latest_recs, cache_dir, file_request_tx).await,

                None => Err( op_failed("unknown capability"))
            };
            if let Err(e) = res { warn!("failed to get missing updates for {uri_path}: {e}") }
        }
    }

    Ok(())
}

pub(crate) async fn get_and_send_missing<T> (hself: &ActorHandle<SentinelActorMsg>,
                                  client: &Client, config: &SentinelConfig, uri_path: &str, last: &str,
                                  latest_recs: &mut HashMap<String,String>) -> Result<()>
    where T: RecordDataBounds, SentinelUpdate: From<Arc<SensorRecord<T>>>
{
    let recs = get_records_since::<T>(client, &config.base_uri, &config.access_token, uri_path, last).await?;
    for rec in recs.into_iter() {
        let update = SentinelUpdate::from(Arc::new(rec));
        update_latest_recs( latest_recs, &update);
        hself.send_msg( UpdateStore(update)).await?;
    }

    Ok(())
}

pub(crate) async fn get_and_send_missing_images (hself: &ActorHandle<SentinelActorMsg>,
                                  client: &Client, config: &SentinelConfig, uri_path: &str, last: &str,
                                  latest_recs: &mut HashMap<String,String>,
                                  cache_dir: &PathBuf, file_request_tx: &MpscSender<FileRequest> ) -> Result<()>
{
    let recs = get_records_since::<ImageData>(client, &config.base_uri, &config.access_token, uri_path, last).await?;
    for mut rec in recs.into_iter() {
        rec.set_local_filename();

        request_image_file( config, cache_dir, file_request_tx, &rec).await?;
        let update = SentinelUpdate::from(Arc::new(rec));
        update_latest_recs( latest_recs, &update);
        hself.send_msg( UpdateStore(update)).await?;
    }

    Ok(())
}

pub(crate) async fn request_all_files (config: &SentinelConfig, sentinels: &SentinelStore, file_request_tx: &MpscSender<FileRequest>) -> Result<()> {
    let sentinel_cache_dir = sentinel_cache_dir();
    for sentinel in sentinels.values_iter() {
        for rec in &sentinel.image {
            request_image_file( config, &sentinel_cache_dir, file_request_tx, rec).await?;
        }
    }
    Ok(())
}

pub(crate) async fn request_image_file (config: &SentinelConfig, cache_dir: &PathBuf, file_request_tx: &MpscSender<FileRequest>,
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

pub(crate) async fn handle_file_query (config: &SentinelConfig, file_request_tx: &MpscSender<FileRequest>,
                                       query: Query<GetSentinelFile,Result<SentinelFile>>, sentinel_file: SentinelFile)->Result<()> {
    // currently filenames are globally unique, but that could change (in which case we have to lookup the record_id of the query)
    let record_id = &query.question.record_id;
    let filename = &query.question.filename;

    let uri = if let Some(uri) = &query.question.uri { uri.clone() } else { get_file_uri( config, &record_id, &filename)? };
    let request = FileRequest { uri, sentinel_file , query: Some(query)};

    file_request_tx.send(request).await.map_err(|e| OdinSentinelError::FileRequestError(e.to_string()))
}

pub(crate) fn get_file_uri (config: &SentinelConfig, record_id: &str, filename: &str)->Result<String> {
    // for now we just have image files so there is only one uri scheme
    Ok( get_image_uri( &config.base_uri, record_id) )
}

pub(crate) async fn file_cleanup_loop (config: Arc<SentinelConfig>, cache_dir: Arc<PathBuf>)->Result<()> {
    let interval = minutes(60); // should we configure this?

    loop {
        // remove first so that frequent short runs that don't hit the interval do not accumulate
        remove_old_files( cache_dir.as_ref(), config.max_age);
        sleep(interval).await;
    }
    Ok(())
}

/* #endregion shared connector functions */

/* #region connector http getters **********************************************************************************/

pub async fn get_records_since <T> (client: &Client, base_uri: &str, access_token: &str, uri_path: &str, last: &str) -> Result<Vec<SensorRecord<T>>>
    where T: RecordDataBounds
{
    let uri = format!("{base_uri}{uri_path}?sort=timeRecorded,DESC&last={last}"); // FIXME - the uri_path has a heading '/'
    let response = client.get(uri).bearer_auth(access_token).send().await?;
    let record_list: RecordList<T> = from_json(response).await?;
    Ok(record_list.data)
}

pub async fn get_latest_record <T> (client: &Client, base_uri: &str, access_token: &str,
                                    device_id: &str, sensor_no:u32) -> Result<SensorRecord<T>>
    where T: RecordDataBounds, SentinelUpdate: From<Arc<SensorRecord<T>>>
{
    let mut recs = get_time_sorted_records::<T>( client, base_uri, access_token, device_id, sensor_no, 1).await?;
    recs.pop().ok_or( no_data(format!("for device: {}, sensor: {}, capability: {:?}", device_id, sensor_no, T::capability())))
}

pub(crate) async fn get_file_request (client: &Client, access_token: &str, uri: &str, pathname: &PathBuf)->Result<()> {
    let mut response = client.get(uri).bearer_auth(access_token).send().await?;

    let mut file = File::create(pathname)?;
    while let Some(chunk) = response.chunk().await? {
        file.write(&chunk)?;
    }

    Ok(())
}

pub fn get_image_uri (base_uri: &str, record_id: &str)->String {
    format!("{base_uri}/images/{record_id}")
}

/* #endregion connector http getters */

/* #region FileFetcher ***********************************************************************************************/

/// struct to request a SentinelFile from an external server
#[derive(Debug)]
pub(crate) struct FileRequest {
    uri: String,  // this is where we get the file from
    sentinel_file: SentinelFile, // this is where we store it
    query: Option<SentinelFileQuery> // in case this request came from an external entity
}


/// struct that holds all the info to resolve a Query<GetSentinelFile,Result<SentinelFile>>
pub(crate) struct FileFetcher {
    pub(crate) config: Arc<SentinelConfig>,
    pub(crate) cache_dir: Arc<PathBuf>,
    pub(crate) client: Client,
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
