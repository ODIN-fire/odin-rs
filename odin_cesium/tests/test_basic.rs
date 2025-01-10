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

use odin_server::{build_service, errors::OdinServerResult, spa::{SpaComponents, SpaServiceList}};
use odin_cesium::ImgLayerService;

#[test]
fn test_doc_assembly()->OdinServerResult<()> {
    let services = SpaServiceList::new()
        .add( build_service!( => ImgLayerService::new()));

    let spa_comps = SpaComponents::from( &services)?;

    let doc = spa_comps.to_html("basic_globe");
    println!("{doc}");

    Ok(())
}