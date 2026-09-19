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

#![allow(unused)]

use crate::{asset_uri, proxy_uri, load_asset, self_crate, build_service,
    errors::OdinServerResult,
    ui_service::UiService,
    spa::{SpaComponents, SpaService,SpaServiceList}
};

/// this is a resource-only SpaService that provides support for using the external [code-input](https://v2.code-input-js.org/)
/// library in combination with ODIN's ui and server infrastructure
pub struct CodeService {
    // not yet - only resources so far
}

impl CodeService {
    pub fn new ()->Self { CodeService{} }
}

impl SpaService for CodeService {
    fn add_dependencies (&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder
            .add( build_service!( => UiService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()>  {
        spa.add_assets( self_crate!(), load_asset);

        // TODO we should probably add indent.min.js here

        #[cfg(feature="code_input_proxy")]
        {
            spa.add_proxy( "code_input", format!("https://cdn.jsdelivr.net/gh/WebCoder49/code-input@2.8/"));
            spa.add_script( proxy_uri!( "code_input", "code-input.min.js"));
            spa.add_script( proxy_uri!( "code_input", "plugins/indent.min.js"));
            spa.add_css( proxy_uri!( "code_input", "code-input.min.css"));
        }
        #[cfg(feature="code_input_asset")]
        {
            // download from https://github.com/WebCoder49/code-input and put into assets/code_input
            // (library is under MIT license)
            spa.add_script( asset_uri!("code_input/code-input.min.js"));
            spa.add_script( asset_uri!("code_input/plugins/code-input.min.js"));
            spa.add_css( asset_uri!("code_input/code-input.min.css"));
        }

        spa.add_module( asset_uri!("ui_code_input.js"));
        spa.add_css( asset_uri!("ui_code_input.css")); // our own style settings that are based on ui.css and theme vars

        Ok(())
     }
 }
