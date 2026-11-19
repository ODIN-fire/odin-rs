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

//! the SpaService for NEXRAD products. Each product time step is a dynamic layer of our WMS endpoint (`nexrad/wms`),
//! which also provides value lookup (GetFeatureInfo). Clients get the stations and their available time steps through
//! websocket messages (`snapshot` and `update`)

use std::{any::type_name, collections::HashMap, net::SocketAddr, path::Path, sync::Arc};
use axum::{http::{HeaderMap, StatusCode, Uri}, response::{IntoResponse, Response}, routing::get, extract::{Path as AxumPath, Query}};
use async_trait::async_trait;

use odin_actor::prelude::*;
use odin_common::fs::EnvPathBuf;
use odin_server::prelude::*;
use odin_cesium::ImgLayerService;
use odin_gdal::{cmap::CmapEntry, wms_srv::{WmsLayer, WmsLayerConfig, WmsService, WmsStyleConfig}};

use crate::{
    load_asset, CACHE_DIR, NexradConfig, NexradEntry,
    actor::{ExecSnapshotAction, NexradActorMsg},
    product::reflectivity_cmap,
    store::{NexradStore, NexradUpdate},
    mosaic::MosaicUpdate,
};

/* #region WMS layers *************************************************************************************************/

/// the WMS of NEXRAD product time steps. This is shared between the server actions of the NexradActor (which add
/// and remove layers) and the NexradService (which serves WMS requests)
pub struct NexradWms {
    pub wms: Arc<WmsService>,
    cmap: Arc<Vec<CmapEntry>>, // all products are reflectivity codes
}

impl NexradWms {
    pub fn new (config: &NexradConfig)->Arc<Self> {
        let wms = Arc::new( WmsService::new( config.wms.clone(), CACHE_DIR.join("wms"), None));
        Arc::new( NexradWms { wms, cmap: Arc::new( reflectivity_cmap( config.min_dbz)) })
    }

    /// add the WMS layer of a product time step. Note this is blocking (opens the GeoTIFF)
    pub fn add_entry (&self, entry: &NexradEntry)->odin_gdal::errors::Result<()> {
        let lc = WmsLayerConfig {
            name: entry.layer_name(),
            title: format!("{} {} {}", entry.station, entry.product.title(), entry.scan_time.format("%Y-%m-%d %H:%M:%SZ")),
            descr: None,
            path: EnvPathBuf( entry.path.clone()),
            nodata: vec![],  // nodata values come from the color map (below threshold, range folded and < min_dbz)
            styles: vec![ WmsStyleConfig::Cmap( EnvPathBuf( entry.path.clone())) ],
        };
        let cmap = self.cmap.clone();
        let read_cmap = move |_: &Path| Ok( cmap.as_ref().clone());
        self.wms.add_layer( WmsLayer::new( &lc, Some(&read_cmap))?);
        Ok(())
    }

    pub fn remove_entry (&self, entry: &NexradEntry) {
        self.wms.remove_layer( &entry.layer_name());
    }

    /// update WMS layers (removed ones first). Errors are reported but don't stop processing
    pub fn apply_update (&self, update: &NexradUpdate) {
        for e in &update.removed { self.remove_entry( e) }
        for e in &update.added {
            if let Err(err) = self.add_entry( e) { eprintln!("failed to add NEXRAD WMS layer {}: {err}", e.layer_name()) }
        }
    }
}

/* #endregion WMS layers */

/* #region websocket messages *****************************************************************************************/

/// products and all stations with their positions and available time steps (newest first)
pub fn snapshot_msg (store: &NexradStore)->String {
    ws_msg_from_json( NexradService::mod_path(), "snapshot", &store.json_value().to_string())
}

/// added and removed time steps of a station
pub fn update_msg (update: &NexradUpdate)->String {
    ws_msg_from_json( NexradService::mod_path(), "update", &update.json_value().to_string())
}

/// a new mosaic of a product and the dates of the ones it replaced
pub fn mosaic_msg (update: &MosaicUpdate)->String {
    ws_msg_from_json( NexradService::mod_path(), "mosaic", &update.json_value().to_string())
}

/* #endregion websocket messages */

/* #region service ****************************************************************************************************/

pub struct NexradService {
    hactor: ActorHandle<NexradActorMsg>,
    wms: Arc<NexradWms>,
}

impl NexradService {
    /// `wms` has to be the same instance that is used by the server actions of the NexradActor (see
    /// `actor::server_init_action(..)` and `actor::server_update_action(..)`)
    pub fn new (hactor: ActorHandle<NexradActorMsg>, wms: Arc<NexradWms>)->Self {
        NexradService { hactor, wms }
    }
}

#[async_trait]
impl SpaService for NexradService {

    fn add_dependencies (&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder.add( build_service!( => ImgLayerService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()> {
        spa.add_assets( self_crate!(), load_asset);

        spa.add_module( asset_uri!( "odin_nexrad_config.js"));
        spa.add_module( asset_uri!( "odin_nexrad.js"));

        //--- WMS GetCapabilities/GetMap/GetFeatureInfo for all product time steps
        let wms = self.wms.wms.clone();
        spa.add_route( |router, spa_server_state| {
            router.route( &format!("/{}/nexrad/wms", spa_server_state.name.as_str()), get({
                move |Query(q): Query<HashMap<String,String>>, headers: HeaderMap, uri: Uri| {
                    let wms = wms.clone();
                    async move { wms.handle_wms( &q, &headers, &uri).await }
                }
            }))
        });

        //--- overview mosaic GeoJSON files
        spa.add_route( |router, spa_server_state| {
            router.route( &format!("/{}/nexrad/mosaic/{{file}}", spa_server_state.name.as_str()), get( mosaic_handler))
        });

        Ok(())
    }

    async fn data_available (&mut self, hself: &ActorHandle<SpaServerMsg>, has_connections: bool, sender_id: &str, data_type: &str) -> OdinServerResult<bool> {
        let mut is_our_data = false;

        if self.hactor.id() == sender_id && data_type == type_name::<NexradStore>() {
            if has_connections {
                let action = dyn_dataref_action!( let hself: ActorHandle<SpaServerMsg> = hself.clone() => |store: &NexradStore| {
                    let ws_msg = snapshot_msg( store);
                    Ok( hself.try_send_msg( BroadcastWsMsg{ws_msg})? )
                });
                self.hactor.send_msg( ExecSnapshotAction(action)).await?;
            }
            is_our_data = true;
        }
        Ok(is_our_data)
    }

    async fn init_connection (&mut self, hself: &ActorHandle<SpaServerMsg>, is_data_available: bool, conn: &mut WsConnection) -> OdinServerResult<()> {
        if is_data_available {
            let action = dyn_dataref_action!{
                let hself: ActorHandle<SpaServerMsg> = hself.clone(),
                let remote_addr: SocketAddr = conn.remote_addr =>
                |store: &NexradStore| {
                    let ws_msg = snapshot_msg( store);
                    Ok( hself.try_send_msg( SendWsMsg{remote_addr: *remote_addr, ws_msg})? )
                }
            };
            self.hactor.send_msg( ExecSnapshotAction(action)).await?;
        }
        Ok(())
    }
}

async fn mosaic_handler (AxumPath(file): AxumPath<String>)->Response {
    // only plain mosaic file names (no paths)
    if file.ends_with(".geojson") && !file.contains(['/', '\\']) && !file.starts_with('.') {
        compressable_file_response( CACHE_DIR.join("mosaic"), &file, "mosaic not found")
    } else {
        (StatusCode::BAD_REQUEST, "invalid mosaic file name").into_response()
    }
}

/* #endregion service */
