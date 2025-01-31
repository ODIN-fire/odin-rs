/*
 * Copyright © 2025, United States Government, as represented by the Administrator of 
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

/// a simple test server that automatically serves the odin_server/ and odin_cesium/ assets in 
/// a synthesized document that includes a command line argument specified Javascript test module.
/// This module does not have to initialize Cesium and should have an exported "main.start()" function
/// that kicks off whatever is supposed to be tested.

use std::{path::{Path,PathBuf}, fs::{self,File}, env};
use axum::{http::StatusCode,routing::get,response::IntoResponse};

use odin_cesium::ImgLayerService;
use odin_common::define_cli;
use odin_actor::prelude::*;
use odin_server::{file_response, prelude::*};


define_cli! { ARGS [about="tool to test custom Javascript modules in a odin_cesium context"] =
    open: bool       [help="auto-open browser",short,long],
    pathname: String [help="path to Javascript test module"]
}

pub struct TestService {
    mod_path: String
}

impl TestService {
    pub fn new (mod_path: String) -> Self { TestService{mod_path} }
}

impl SpaService for TestService {

    fn add_dependencies (&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder.add( build_service!( => ImgLayerService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()> {
        spa.add_assets( self_crate!(), odin_cesium::load_asset);
        spa.add_module( asset_uri!("test.js"));

        spa.add_body_fragment(r#"
            <input id="button" type="button" value="start" onclick="main.start()" style="position:absolute;top:5px;left:250px;height:28px;font-size:medium;z-index:1;">
        "#);

        let path = Path::new( &self.mod_path).to_path_buf();
        spa.add_route( |router, spa_server_state| {
            router.route( &format!("/{}/asset/odin_cesium/test.js", spa_server_state.name.as_str()), get(
                async move || { file_response( &path, true).await.into_response() }
            ))
        });

        Ok(())
    }
}

run_actor_system!( actor_system => {    
    env::set_var( "ODIN_RELOAD_ASSETS", "1"); // make sure assets are always reloaded - this is for testing/debugging

    let hserver = spawn_actor!( actor_system, "spa_server", SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "test",
        SpaServiceList::new().add( build_service!( => TestService::new( ARGS.pathname.clone())))
    ))?;

    if ARGS.open { open( &hserver) }

    Ok(())
});