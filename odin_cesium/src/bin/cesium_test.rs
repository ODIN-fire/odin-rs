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
    open: bool              [help="auto-open browser",short,long],
    pathname: String        [help="path to Javascript test module"],
    config: Option<String>  [help="optional config module path"]
}

pub struct TestService {
    mod_path: PathBuf,
    config_path: Option<PathBuf>
}

impl TestService {
    pub fn new (mod_pathname: &String, config_pathname: &Option<String>) -> Self { 
        TestService {
            mod_path: Path::new(mod_pathname).to_path_buf(),
            config_path: config_pathname.as_ref().map( |pn| Path::new(&pn).to_path_buf())
        }
    }
}

impl SpaService for TestService {

    fn add_dependencies (&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder.add( build_service!( => ImgLayerService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()> {
        let mod_filename = self.mod_path.file_name()
            .and_then(|os| os.to_str())
            .ok_or(odin_server::errors::init_error("not a valid module filename"))?;
        
        let conf_filename = format!("{}_config.js", self.mod_path.file_stem()
            .and_then(|os| os.to_str())
            .ok_or(odin_server::errors::init_error("not a valid module filename"))?
        );

        spa.add_assets( self_crate!(), odin_cesium::load_asset);

        if let Some(conf) = &self.config_path {
            spa.add_module( format!("./asset/odin_cesium/{}", conf_filename));
        }

        spa.add_module( format!("./asset/odin_cesium/{}", mod_filename));

        spa.add_body_fragment(r#"
            <input id="button" type="button" value="start" onclick="main.test()" style="position:absolute;top:5px;left:250px;height:28px;font-size:medium;z-index:1;">
        "#);


        let mod_url = format!("/{}/asset/odin_cesium/{}", spa.name, mod_filename);
        let mod_path = self.mod_path.clone();
        spa.add_route( move |router, spa_server_state| {
            router.route( &mod_url, get(
                async move || { file_response( &mod_path, true).await.into_response() }
            ))
        });

        if let Some(conf_path) = &self.config_path {
            let conf_url = format!("/{}/asset/odin_cesium/{}", spa.name, conf_filename);
            let conf_path = conf_path.clone();
            spa.add_route( move |router, spa_server_state| {
                router.route( &conf_url, get(
                    async move || { file_response( &conf_path, true).await.into_response() }
                ))
            });
        }

        Ok(())
    }
}

run_actor_system!( actor_system => {    
    env::set_var( "ODIN_RELOAD_ASSETS", "1"); // make sure assets are always reloaded - this is for testing/debugging

    let hserver = spawn_actor!( actor_system, "spa_server", SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "test",
        SpaServiceList::new().add( build_service!( => TestService::new( &ARGS.pathname, &ARGS.config)))
    ))?;

    if ARGS.open { open( &hserver) }

    Ok(())
});