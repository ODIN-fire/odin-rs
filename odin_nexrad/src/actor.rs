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

//! the actor that imports NEXRAD Level II volumes of configured stations and keeps the store of available product
//! time steps. Each station is handled by its own task, which
//!
//! 1. retrieves the most recent volumes we don't have yet (`backfill`)
//! 2. checks for new volumes at the expected availability time (last scan + last interval + median publication delay
//!    + check_delay, see [`VolumeTiming`]), retrying every `retry_delay` until there is a new volume
//!
//! New volumes are downloaded, decoded and turned into product GeoTIFFs, which are sent to the actor. The actor
//! updates its store, executes its update_action and then removes the files of dropped time steps.
//!
//! If configured, the actor also checks every `mosaic.interval` if there were updates since the last overview mosaic,
//! in which case it computes new mosaics of all products (see [`crate::mosaic`]), adds them to the store (dropping the
//! oldest ones above `mosaic.max_mosaics`), executes its mosaic_action and then removes the files of dropped mosaics.
//! Mosaics are restored from the cache on startup

use std::{path::{Path,PathBuf}, sync::Arc, time::Duration};
use chrono::{DateTime, TimeDelta, Utc};
use tokio::task::{AbortHandle, JoinHandle};

use odin_actor::prelude::*;
use odin_actor::{error, warn, info, debug};
use odin_common::s3::S3Client;
use odin_server::spa::{SpaServerMsg, DataAvailable, BroadcastWsMsg};

use crate::{
    CACHE_DIR, NexradConfig, NexradEntry, NexradProduct, Station, VolumeInfo, VolumeTiming,
    configured_stations, create_s3_client, list_volumes, list_volumes_between, download_volume, decode_volume, volume_path,
    errors::{Result, op_failed},
    product::compute_products,
    mosaic::{MosaicBuilder, MosaicUpdate, NexradMosaic, mosaic_inputs, load_cached_mosaics, remove_mosaic_files},
    store::{NexradStore, NexradUpdate, load_cached_store, remove_files},
    service::{NexradWms, update_msg, mosaic_msg},
};

/// defaults for the next volume estimate if a station does not have enough volumes for timing statistics
const DEFAULT_INTERVAL: Duration = Duration::from_secs(360);
const DEFAULT_DELAY: Duration = Duration::from_secs(360);

/// number of recent volumes we use for timing statistics
const TIMING_WINDOW: usize = 10;

/// timer id for mosaic checks
const MOSAIC_TIMER: i64 = 1;

//--- external messages
#[derive(Debug)] pub struct ExecSnapshotAction( pub DynDataRefAction<NexradStore> );

//--- internal messages (from our station tasks)
#[derive(Debug)] pub struct ProductsComputed { pub(crate) station: String, pub(crate) entries: Vec<NexradEntry> }
#[derive(Debug)] pub struct MosaicsComputed( pub(crate) Vec<NexradMosaic> );

define_actor_msg_set! { pub NexradActorMsg = ExecSnapshotAction | ProductsComputed | MosaicsComputed }

pub struct NexradActor<I,U,M> where I: DataRefAction<NexradStore>, U: DataRefAction<NexradUpdate>, M: DataRefAction<MosaicUpdate> {
    config: Arc<NexradConfig>,
    cache_dir: PathBuf,
    store: Option<NexradStore>, // initialized from our cache when we start
    tasks: Vec<JoinHandle<()>>, // one import task per station

    mosaic_builder: Option<Arc<std::sync::Mutex<MosaicBuilder>>>, // if configured (keeps reduced station grids)
    mosaic_timer: Option<AbortHandle>,
    is_mosaic_outdated: bool, // there were updates since the last mosaic computation
    is_mosaic_running: bool,

    init_action: I,   // executed once the store is initialized from the cache (before any import)
    update_action: U, // executed for each imported volume (and the time steps it replaces)
    mosaic_action: M, // executed for each new mosaic and the ones it replaces (if configured)
}

impl<I,U,M> NexradActor<I,U,M> where I: DataRefAction<NexradStore>, U: DataRefAction<NexradUpdate>, M: DataRefAction<MosaicUpdate> {
    pub fn new (config: NexradConfig, init_action: I, update_action: U, mosaic_action: M)->Self {
        Self::with_cache_dir( config, CACHE_DIR.clone(), init_action, update_action, mosaic_action)
    }

    pub fn with_cache_dir (config: NexradConfig, cache_dir: PathBuf, init_action: I, update_action: U, mosaic_action: M)->Self {
        NexradActor {
            config: Arc::new(config), cache_dir, store: None, tasks: Vec::new(),
            mosaic_builder: None, mosaic_timer: None, is_mosaic_outdated: true, is_mosaic_running: false,
            init_action, update_action, mosaic_action
        }
    }

    async fn start (&mut self, hself: ActorHandle<NexradActorMsg>)->Result<()> {
        let stations = configured_stations( &self.config);
        let mut store = load_cached_store( stations.clone(), &self.config.products, self.config.max_downloads, &self.cache_dir);
        info!("initialized NEXRAD store with {} cached product time steps", store.entries().count());

        if let Some(mc) = &self.config.mosaic {
            match MosaicBuilder::new( mc.clone(), stations.clone(), self.config.min_dbz, &self.cache_dir) {
                Ok(builder) => {
                    store.mosaics = load_cached_mosaics( builder.dir(), &self.config.products, mc.max_mosaics);
                    info!("restored {} cached NEXRAD mosaics", store.mosaics.len());
                    self.mosaic_builder = Some( Arc::new( std::sync::Mutex::new( builder)));
                }
                Err(e) => warn!("NEXRAD mosaic disabled: {e}")
            }
        }
        self.init_action.execute( &store).await;

        let client = create_s3_client( &self.config).await?;
        for station in stations {
            let known = store.scan_times( &station.id);
            let name = format!("nexrad-{}", station.id);
            let task = spawn( &name, run_station( hself.clone(), self.config.clone(), client.clone(), self.cache_dir.clone(), station, known))?;
            self.tasks.push( task);
        }

        self.store = Some(store);

        if let (Some(mc), Some(_)) = (&self.config.mosaic, &self.mosaic_builder) {
            self.mosaic_timer = Some( hself.start_repeat_timer( MOSAIC_TIMER, mc.interval, true)?);
        }
        Ok(())
    }

    /// compute new mosaics of all products in a blocking task if there were updates since the last ones (and we
    /// are not already computing). Results are sent back to us as MosaicsComputed messages
    fn check_mosaics (&mut self, hself: ActorHandle<NexradActorMsg>) {
        let (Some(builder), Some(store), Some(mc)) = (&self.mosaic_builder, &self.store, &self.config.mosaic) else { return };
        if !self.is_mosaic_outdated || self.is_mosaic_running { return }

        let inputs: Vec<(NexradProduct,Vec<NexradEntry>)> = self.config.products.iter().map( |p| {
            let latest = store.stations.iter().filter_map( |s| s.product( *p).and_then( |pe| pe.latest()));
            (*p, mosaic_inputs( latest, mc.max_age))
        }).filter( |(_,es)| !es.is_empty()).collect();
        if inputs.is_empty() { return }

        self.is_mosaic_outdated = false;
        self.is_mosaic_running = true;
        let builder = builder.clone();
        let res = spawn( "nexrad-mosaic", async move {
            let mosaics = tokio::task::spawn_blocking( move || {
                let mut builder = builder.lock().unwrap();
                inputs.iter().filter_map( |(p,es)| match builder.build( *p, es) {
                    Ok(m) => Some(m),
                    Err(e) => { warn!("failed to compute {} mosaic: {e}", p.name()); None }
                }).collect::<Vec<NexradMosaic>>()
            }).await.unwrap_or_default();
            let _ = hself.send_msg( MosaicsComputed( mosaics)).await;
        });
        if let Err(e) = res { warn!("failed to spawn mosaic task: {e}"); self.is_mosaic_running = false; }
    }

    async fn add_mosaics (&mut self, mosaics: Vec<NexradMosaic>) {
        self.is_mosaic_running = false;
        let max_mosaics = self.config.mosaic.as_ref().map( |mc| mc.max_mosaics).unwrap_or(1);
        if let Some(store) = &mut self.store {
            for m in mosaics {
                info!("new {} mosaic {} ({} stations, {} polygons, {} vertices, {:?})", m.product.name(), m.date.format("%H:%M:%SZ"),
                      m.stations.len(), m.n_polygons, m.n_vertices, m.compute_time);
                let update = store.add_mosaic( m, max_mosaics);
                self.mosaic_action.execute( &update).await;
                remove_mosaic_files( &update.removed); // not referenced anymore
            }
        }
    }

    /// update the store and execute the update_action, then remove the files of dropped time steps (which are not
    /// referenced anymore after the update_action was executed)
    async fn add_products (&mut self, station: String, entries: Vec<NexradEntry>) {
        if let Some(store) = &mut self.store {
            let update = store.insert( &station, entries);
            if !update.is_empty() {
                self.is_mosaic_outdated = true;
                info!("new {} products (dropped {})", station, update.removed.len());
                self.update_action.execute( &update).await;
                remove_files( &update.removed);

                if self.config.keep_volumes { // remove volumes that are not referenced by any product anymore
                    if let Some(se) = store.station( &station) {
                        for e in &update.removed {
                            if !se.has_scan( &e.scan_time) { remove_volume( &self.cache_dir, &station, &e.scan_time) }
                        }
                    }
                }
            }
        }
    }
}

impl_actor! { match msg for Actor<NexradActor<I,U,M>, NexradActorMsg>
    where I: DataRefAction<NexradStore> + Send + Sync, U: DataRefAction<NexradUpdate> + Send + Sync, M: DataRefAction<MosaicUpdate> + Send + Sync
    as

    ExecSnapshotAction => cont! {
        if let Some(store) = &self.store {
            msg.0.execute( store).await;
        } else {
            warn!("ignoring snapshot request for uninitialized NEXRAD store");
        }
    }

    ProductsComputed => cont! {
        self.add_products( msg.station, msg.entries).await;
    }

    MosaicsComputed => cont! {
        self.add_mosaics( msg.0).await;
    }

    _Timer_ => cont! {
        if msg.id == MOSAIC_TIMER {
            let hself = self.hself.clone();
            self.check_mosaics( hself);
        }
    }

    _Start_ => cont! {
        let hself = self.hself.clone();
        if let Err(e) = self.start( hself).await { error!("failed to start NEXRAD import: {e}") }
    }

    _Terminate_ => stop! {
        for task in &self.tasks { task.abort() }
        if let Some(timer) = &self.mosaic_timer { timer.abort() }
    }
}

/* #region station tasks **********************************************************************************************/

/// the import task of a single station. `known` are the scan times of volumes we already have (from our cache)
async fn run_station (hself: ActorHandle<NexradActorMsg>, config: Arc<NexradConfig>, client: S3Client, cache_dir: PathBuf,
                      station: Station, mut known: Vec<DateTime<Utc>>) {
    let id = station.id.clone();
    let now = Utc::now();

    //--- initial listing and backfill (volumes are at least 3.5min apart, so 2*backfill*10min is more than enough)
    let window = TimeDelta::minutes( (20 * config.backfill.max(1) as i64).max(60));
    let mut recent: Vec<VolumeInfo> = match list_volumes_between( &client, &config.bucket, &id, now - window, now).await {
        Ok(vs) => vs,
        Err(e) => { warn!("failed to list {id} volumes: {e}"); Vec::new() }
    };

    let n = recent.len();
    for v in recent[n.saturating_sub( config.backfill)..].to_vec() {
        if !known.contains( &v.scan_time) {
            known.push( v.scan_time);
            if !import_volume( &hself, &config, &client, &cache_dir, &station, &v).await { return } // actor gone
        }
    }
    let mut last: Option<VolumeInfo> = recent.last().cloned();
    truncate_front( &mut recent, TIMING_WINDOW);

    //--- periodic updates
    loop {
        let next = VolumeTiming::from_volumes( &recent)
            .map( |t| t.next_expected( DEFAULT_INTERVAL, DEFAULT_DELAY) + config.check_delay)
            .unwrap_or_else( || Utc::now() + config.retry_delay);
        sleep_until( next).await;

        loop { // poll until there is a new volume
            match list_new_volumes( &client, &config.bucket, &id, last.as_ref()).await {
                Ok(vs) if !vs.is_empty() => {
                    for v in vs {
                        if !known.contains( &v.scan_time) {
                            known.push( v.scan_time);
                            if !import_volume( &hself, &config, &client, &cache_dir, &station, &v).await { return }
                        }
                        last = Some( v.clone());
                        recent.push( v);
                    }
                    truncate_front( &mut recent, TIMING_WINDOW);
                    truncate_front( &mut known, 4 * config.max_downloads.max( TIMING_WINDOW));
                    break
                }
                Ok(_) => debug!("no new {id} volume yet"),
                Err(e) => warn!("failed to list {id} volumes: {e}")
            }
            tokio::time::sleep( config.retry_delay).await;
        }
    }
}

/// volumes after `last` (sorted by scan time). This also lists dates after the date of `last` (UTC day boundaries)
async fn list_new_volumes (client: &S3Client, bucket: &str, id: &str, last: Option<&VolumeInfo>)->Result<Vec<VolumeInfo>> {
    let today = Utc::now().date_naive();
    match last {
        Some(last) => {
            let mut date = last.scan_time.date_naive();
            let mut volumes = list_volumes( client, bucket, id, date, Some( &last.key)).await?;
            while date < today {
                date = date.succ_opt().ok_or( op_failed!("invalid date"))?;
                volumes.extend( list_volumes( client, bucket, id, date, None).await?);
            }
            Ok(volumes)
        }
        None => list_volumes( client, bucket, id, today, None).await
    }
}

/// download, decode and process a volume, sending the computed products to the actor. Returns false if the actor
/// does not accept messages anymore. Processing errors are reported but don't stop the station task
async fn import_volume (hself: &ActorHandle<NexradActorMsg>, config: &Arc<NexradConfig>, client: &S3Client, cache_dir: &Path,
                        station: &Station, volume: &VolumeInfo)->bool {
    match process_volume( config, client, cache_dir, station, volume).await {
        Ok(entries) => hself.send_msg( ProductsComputed{ station: station.id.clone(), entries }).await.is_ok(),
        Err(e) => { warn!("failed to import {}: {e}", volume.name()); true }
    }
}

async fn process_volume (config: &Arc<NexradConfig>, client: &S3Client, cache_dir: &Path, station: &Station, volume: &VolumeInfo)->Result<Vec<NexradEntry>> {
    let data = download_volume( client, &config.bucket, volume).await?;
    if config.keep_volumes {
        let path = volume_path( cache_dir, volume);
        if let Some(dir) = path.parent() { std::fs::create_dir_all( dir)?; }
        std::fs::write( &path, &data)?;
    }

    let (config, cache_dir, station, scan_time) = (config.clone(), cache_dir.to_path_buf(), station.clone(), volume.scan_time);
    tokio::task::spawn_blocking( move || {
        let scan = decode_volume( data)?;
        compute_products( &scan, &station, &config.products, scan_time, config.resolution_m, config.min_dbz, &cache_dir)
    }).await.map_err( |e| op_failed!("processing task failed: {e}"))?
}

/// remove a kept volume (we don't know the version suffix of the volume name, i.e. we match the station/time prefix)
fn remove_volume (cache_dir: &Path, station: &str, scan_time: &DateTime<Utc>) {
    let dir = cache_dir.join( station).join( "volumes");
    let prefix = format!("{station}{}", scan_time.format("%Y%m%d_%H%M%S"));
    if let Ok(rd) = std::fs::read_dir( &dir) {
        for de in rd.flatten() {
            if de.file_name().to_str().map( |n| n.starts_with( &prefix)).unwrap_or(false) { let _ = std::fs::remove_file( de.path()); }
        }
    }
}

async fn sleep_until (t: DateTime<Utc>) {
    if let Ok(d) = (t - Utc::now()).to_std() { tokio::time::sleep( d).await; }
}

fn truncate_front<T> (v: &mut Vec<T>, max_len: usize) {
    if v.len() > max_len { v.drain( ..v.len() - max_len); }
}

/* #endregion station tasks */

/* #region server actions *********************************************************************************************/

/// init_action for SpaServer integration: registers the WMS layers of all cached time steps and notifies the server
pub fn server_init_action (hserver: ActorHandle<SpaServerMsg>, actor_id: &str, wms: Arc<NexradWms>)->impl DataRefAction<NexradStore> {
    dataref_action!(
        let sender_id: Arc<String> = Arc::new( actor_id.to_string()),
        let wms: Arc<NexradWms> = wms,
        let hserver: ActorHandle<SpaServerMsg> = hserver => |store: &NexradStore| {
            for e in store.entries() {
                if let Err(err) = wms.add_entry( e) { eprintln!("failed to add NEXRAD WMS layer {}: {err}", e.layer_name()) }
            }
            Ok( hserver.try_send_msg( DataAvailable::new::<NexradStore>(sender_id) )? )
        }
    )
}

/// update_action for SpaServer integration: updates WMS layers and broadcasts the update to all clients
pub fn server_update_action (hserver: ActorHandle<SpaServerMsg>, wms: Arc<NexradWms>)->impl DataRefAction<NexradUpdate> {
    dataref_action!(
        let wms: Arc<NexradWms> = wms,
        let hserver: ActorHandle<SpaServerMsg> = hserver => |update: &NexradUpdate| {
            wms.apply_update( update);
            let ws_msg = update_msg( update);
            Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
        }
    )
}

/// mosaic_action for SpaServer integration: broadcasts the new mosaic (and the dropped ones) to all clients, which
/// retrieve the GeoJSON through `nexrad/mosaic/<file>` if they display it
pub fn server_mosaic_action (hserver: ActorHandle<SpaServerMsg>)->impl DataRefAction<MosaicUpdate> {
    dataref_action!(
        let hserver: ActorHandle<SpaServerMsg> = hserver => |update: &MosaicUpdate| {
            let ws_msg = mosaic_msg( update);
            Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
        }
    )
}

/* #endregion server actions */
