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

use std::{collections::HashMap, sync::Arc};
use odin_common::{geo::{GeoPos,LatLon},angle::{LatAngle,LonAngle}};
use odin_share::{errors::op_failed, prelude::*};
use odin_server::prelude::*;
use serde_json;

fn create_store()->HashMap<String,SharedItem> {
    HashMap::from([
        ("/views/bay_area".to_string(), SharedItem::Point3D( 
            SharedItemValue {
                comment: None,
                owner: None,
                data: Arc::new(GeoPos::new( LatAngle::from_degrees(38.15910), LonAngle::from_degrees(-122.67800), 800000.0))
            }
        )),
        ("/incidents/czu/ignition".to_string(), SharedItem::Point2D(
            SharedItemValue {
                comment: Some("origin of fire at blabla".to_string()),
                owner: None,
                data: Arc::new(LatLon::from_degrees( 37.137, -122.2854))
            }
        )),
        ("/incidents/czu/cause".to_string(), SharedItem::String(
            SharedItemValue {
                comment: Some("preliminary".to_string()),
                owner: None,
                data: Arc::new("dry lightning".to_string())
            }
        )),
    ])
}

// run with "cargo test test_store_ser -- --nocapture"
#[test]
fn test_store_serde()->Result<(),OdinShareError> {
   let map: HashMap<String,SharedItem> = create_store();

   let json = serde_json::to_string_pretty(&map)?;
   println!("### test serialization:\n{map:?}\n------->\n{json}\n");

   let map1: HashMap<String,SharedItem> = serde_json::from_str( &json)?;
   println!("### test deserialization:\n{map1:#?}");

   assert_eq!( map, map1, "testing serialization input and deserialization output equality");
   Ok(())
}


#[test]
fn test_init_msg()->Result<(),OdinShareError> {
    let map: HashMap<String,SharedItem> = create_store();
    let ws_msg = WsMsg::json( ShareService::mod_path(), "initShare", map).map_err(|e| op_failed(e))?;

    println!("### creating initShare WsMsg:\n{ws_msg}");
    Ok(())
}

