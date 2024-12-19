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

use crate::{asset_uri, load_asset, self_crate, build_service, 
    errors::OdinServerResult, 
    spa::{SpaComponents, SpaService,SpaServiceList}, 
};

/// this is a resource-only SpaService that provides ODINs UI framework including the
/// window that is used to modify and store local themes. The initial theme can be
/// controlled by using a "?theme=dark|light|night" query param when requesting the document
pub struct UiService {
    // not yet - only resources so far
}

impl UiService {
    pub fn new ()->Self { UiService{} }
}

impl SpaService for UiService {
    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()>  {
        spa.add_assets( self_crate!(), load_asset);
        
        // TODO - add_route for themes (user-agent aware)

        //--- css
        // note that ui_load_theme.js needs to be included BEFORE ui.css (which uses theme vars) 
        // and it needs to be a plain javascript (not a module) so that it can document.currentScript
        // this also guarantees it is loaded/executed before any of our own css assets
        spa.add_script( asset_uri!("ui_load_theme.js"));
        spa.add_css( asset_uri!("ui.css"));

        //--- JS modules
        spa.add_module( asset_uri!("ui_data.js"));
        spa.add_module( asset_uri!("ui_util.js"));
        spa.add_module( asset_uri!("ui.js"));
        spa.add_module( asset_uri!("ui_windows.js"));
        spa.add_module( asset_uri!("ui_settings_config.js"));
        spa.add_module( asset_uri!("ui_settings.js"));

        Ok(())
    }
}