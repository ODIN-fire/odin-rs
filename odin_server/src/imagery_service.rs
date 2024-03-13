/*
 * Copyright (c) 2024, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The ODIN - Open Data Integration Framework is licensed under the
 * Apache License, Version 2.0 (the "License"); you may not use this file
 * except in compliance with the License. You may obtain a copy of the
 * License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */
#![allow(unused)]

use std::{default::Default, collections::HashMap, fmt::Debug};
use serde::Deserialize;
use ron;
use crate::{Service};

/* #region data model **********************************************************************************/

/// the toplevel type for configured Imagery items
#[derive(Debug,Deserialize)]
pub struct Imagery {
    pathname: String,
    info: String,
    exclusive: Vec<String>,
    provider: CesiumImageryProvider,
    proxy: bool,
    show: bool,
    rendering: Option<ImageryRenderingParams>
}

// our supported Cesium ImageryProvider types
#[derive(Debug,Deserialize)]
pub enum CesiumImageryProvider {
    ArcGisMapServerImageryProvider { uri: String },
    OpenStreetMapImageryProvider { uri: String },
    TileMapServiceImageryProvider { uri: String, bounds: Option<GeoBounds> },
    WebMapTileServiceImageryProvider { uri: String, params: HashMap<String,ProviderParam> }
}

#[derive(Debug,Deserialize)]
pub struct GeoBounds {
    west: f32,
    south: f32,
    east: f32,
    north: f32,
}

#[derive(Debug,Deserialize)]
pub enum ProviderParam {
    String(String),
    Int(i64),
    Float(f32)
}


#[derive(Debug,Deserialize)]
#[serde(default)] 
pub struct ImageryRenderingParams {
    brightness: f32,
    contrast: f32,
    hue: f32,
    saturation: f32,
    gamma: f32,
    alpha: f32,
    alpha_color: Option<String>,
    alpha_color_threshold: Option<f32>,
}

impl Default for ImageryRenderingParams {
    fn default()->Self {
        ImageryRenderingParams {
            brightness: 0.6,
            contrast: 1.0,
            hue: 1.0,
            saturation: 1.0,
            gamma: 1.0,
            alpha: 1.0,
            alpha_color: None,
            alpha_color_threshold: None
        }
    }
}

/* #endregion data model */

pub struct ImageryService {
    imagery_items: Vec<Imagery>,
    default_rendering: ImageryRenderingParams
}

impl Service for ImageryService {
    fn router (&self)->Option<axum::Router> {
        todo!()
    }
}