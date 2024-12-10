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
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_share::prelude::*;
use odin_cesium::ImgLayerService;
use odin_action::{data_action,DataAction};
use odin_common::{angle::{LatAngle,LonAngle},geo::{LatLon,GeoPos}};

use std::{collections::HashMap, sync::Arc, any::type_name};

/// Cesium app using a ShareService
run_actor_system!( actor_system => {
    let pre_store = PreActorHandle::<SharedStoreActorMsg<SharedItem>>::new( &actor_system, "store", 8);

    let hserver = spawn_actor!( actor_system, "server", SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "cesium_share",
        SpaServiceList::new()
            .add( build_service!( => ImgLayerService::new()))
            .add( build_service!( let hstore = pre_store.to_actor_handle() => ShareService::new( hstore)) )
    ))?;

    let hstore = spawn_pre_actor!( actor_system, pre_store, SharedStoreActor::new(
        create_store(),
        shared_store_action!( let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => 
            |store as &dyn SharedStore<SharedItem>| {
                Ok( hserver.try_send_msg( DataAvailable{sender_id:"store",data_type: type_name::<SharedItem>()} )? )
            }
        ),
        data_action!( let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => 
            |update: SharedStoreChange<SharedItem>| {
                Ok(())
            }
        )
    ))?;

    Ok(())
});

fn create_store()->HashMap<String,SharedItem> {
    HashMap::from([
        ("view/bay_area".to_string(), SharedItem::Point3D( 
            SharedItemValue {
                comment: None,
                owner: None,
                data: Arc::new(GeoPos::new( LatAngle::from_degrees(38.15910), LonAngle::from_degrees(-122.67800), 800000.0))
            }
        )),
        ("incident/czu/ignition".to_string(), SharedItem::Point2D(
            SharedItemValue {
                comment: Some("origin of fire at blabla".to_string()),
                owner: None,
                data: Arc::new(LatLon::from_degrees( 37.137, -122.2854))
            }
        )),
        ("incident/czu/cause".to_string(), SharedItem::String(
            SharedItemValue {
                comment: Some("preliminary".to_string()),
                owner: None,
                data: Arc::new("dry lightning".to_string())
            }
        )),
    ])
}