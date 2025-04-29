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

use odin_build;
use odin_actor::{errors::op_failed, prelude::*};
use odin_server::prelude::*;
use odin_share::prelude::*;
use odin_cesium::ImgLayerService;
use odin_action::{data_action,DataAction};
use odin_common::{angle::{Latitude,Longitude},geo::{GeoPoint,GeoPoint3}};

use std::{collections::HashMap, sync::Arc, any::type_name};

/// Cesium app using a ShareService
run_actor_system!( actor_system => {
    let pre_server = PreActorHandle::new( &actor_system, "server", 64);

    // we would normally initialize the store via default_shared_items() but those normally reside outside the repository
    let hstore = spawn_server_share_actor(&mut actor_system, "share", pre_server.to_actor_handle(), &"examples/shared_items.json", false)?;

    let hserver = spawn_pre_actor!( actor_system, pre_server, SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "cesium_share",
        SpaServiceList::new()
            .add( build_service!( => ImgLayerService::new()))
            .add( build_service!( let hstore = hstore.clone() => ShareService::new( hstore)) )
    ))?;

    /*
    // we could also excplicitly create the SharedStoreActor, which would require to set up the init and change actions.
    // If our only client is the server actor this would be boilerplate code duplicated between different applications.

    let hstore = spawn_actor!( actor_system, "share", SharedStoreActor::new(
        create_store(),
        shared_store_action!( let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => 
            |store as &dyn SharedStore<SharedItem>| announce_data_availability( &hserver, "store")
        ),
        data_action!( let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => 
            |change: SharedStoreChange<'_,SharedItem>| broadcast_store_change( &hserver, change).await
        )
    ))?;
    */

    Ok(())
});


// we could also programmatically create and initialize the store
fn create_store()->HashMap<String,SharedItemType> {
    HashMap::from([
        ("view/bay_area".to_string(), SharedItemType::GeoPoint3( 
            SharedItemValue {
                comment: None,
                owner: Some("🔒".to_string()),
                data: Arc::new( GeoPoint3::from_lon_lat_degrees_alt_meters(-122.67800, 38.15910, 800000.0))
            }
        )),
        ("incident/czu/ignition".to_string(), SharedItemType::GeoPoint(
            SharedItemValue {
                comment: Some("origin of fire at blabla".to_string()),
                owner: None,
                data: Arc::new(GeoPoint::from_lon_lat_degrees( -122.2854, 37.137 ))
            }
        )),
        ("incident/czu/cause".to_string(), SharedItemType::String(
            SharedItemValue {
                comment: Some("preliminary".to_string()),
                owner: None,
                data: Arc::new("dry lightning".to_string())
            }
        )),
    ])
}