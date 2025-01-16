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
use odin_common::{geo::{GeoPoint,GeoPoint3,GeoRect},angle::{Longitude,Latitude}};
use odin_share::{errors::op_failed, prelude::*};
use odin_server::prelude::*;
use serde_json;

fn create_store()->HashMap<String,SharedItemType> {
    HashMap::from([
        ("/views/bay_area".to_string(), SharedItemType::GeoPoint3( 
            SharedItemValue {
                comment: None,
                owner: Some("🔒".to_string()),
                data: Arc::new( GeoPoint3::from_lon_lat_degrees_alt_meters( -122.67800, 38.15910, 800000.0))
            }
        )),
        ("/incidents/czu/ignition".to_string(), SharedItemType::GeoPoint(
            SharedItemValue {
                comment: Some("origin of fire at blabla".to_string()),
                owner: None,
                data: Arc::new( GeoPoint::from_lon_lat( Longitude::from_degrees(-122.2854), Latitude::from_degrees(37.137)))
            }
        )),
        ("/incidents/czu/bbox".to_string(), SharedItemType::GeoRect(
            SharedItemValue {
                comment: None,
                owner: None,
                data: Arc::new( GeoRect::from_wsen(
                    Longitude::from_degrees(-122.6800),
                    Latitude::from_degrees(36.9947),
                    Longitude::from_degrees(-121.8617),
                    Latitude::from_degrees(37.4843),
                ))
            }
        )),
        ("/incidents/czu/cause".to_string(), SharedItemType::String(
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
   let map: HashMap<String,SharedItemType> = create_store();

   let json = serde_json::to_string_pretty(&map)?;
   println!("### test serialization:\n{map:?}\n------->\n{json}\n");

   let map1: HashMap<String,SharedItemType> = serde_json::from_str( &json)?;
   println!("### test deserialization:\n{map1:?}");

   let json1 = serde_json::to_string_pretty(&map1)?;
   println!("### test serialization roundtrip:\n{map:?}\n------->\n{json1}\n");

   assert!( map.len() == map1.len());
   Ok(())
}

#[test]
fn test_str_init()->Result<(),OdinShareError> {
    let input = r#"
{
    "view/bay_area": {
        "type": "GeoPoint3",
        "owner": "🔒",
        "data": {
            "lat": 38.15910,
            "lon": -122.67800,
            "alt": 800000.0
        }
    },

    "incident/czu/ignition": {
        "type": "GeoPoint",
        "data": {
            "lon": -122.2854, 
            "lat": 37.137
        }
    }, 

    "incident/czu/cause": {
        "type": "String",
        "comment": "preliminary",
        "data": "dry lightning"
    }
}
    "#;

    let map: HashMap<String,SharedItemType> = serde_json::from_str( input)?;
    println!("### test JSON map init:\n{map:?}");

    assert!( map.len() == 3);
    Ok(())
}

#[test]
fn test_file_init()->Result<(),OdinShareError> {
    let store: PersistentHashMapStore<SharedItemType> = PersistentHashMapStore::new( &"tests/shared_items.json", false)?;
    println!("### test JSON store init:\n{store:?}");

    let json = store.to_json()?;
    println!("### serialized contents:\n{}", json);

    Ok(())
}

#[test]
fn test_init_msg()->Result<(),OdinShareError> {
    let map: HashMap<String,SharedItemType> = create_store();
    let ws_msg = WsMsg::json( ShareService::mod_path(), "initShare", map).map_err(|e| op_failed(e))?;

    println!("### creating initShare WsMsg:\n{ws_msg}");
    Ok(())
}

