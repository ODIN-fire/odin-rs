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

//! test application for interactive image viewer. This is just a dummy service to dynamically create an ImageViewer 

use odin_actor::prelude::*;
use odin_server::prelude::*;
use open;

pub struct TestImageService {}

impl SpaService for TestImageService {

    fn add_dependencies (&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder.add( build_service!( => UiService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()> {
        spa.add_assets( self_crate!(), odin_server::load_asset);
        spa.add_module( asset_uri!("ui_windows.js"));
        spa.add_module( asset_uri!("test_image.js"));
        Ok(())
    }
}

run_actor_system!( actor_system => {
    
    spawn_actor!( actor_system, "spa_server", SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "image",
        SpaServiceList::new().add( build_service!( => TestImageService{}))
    ));

    open::that("http://localhost:9009/image");

    Ok(())
});