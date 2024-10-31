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

//! module to (eventually) implement a minimal [WMS](https://portal.ogc.org/files/?artifact_id=14416) server for
//! elevation data. The main end point is
//! 
//!    GET <host>:<port>/GetMap?<query>
//! 
//! with query parameters
//! 
//!       crs    : coordinate reference system ("epsg:<number>")
//!       bbox   : comma separated list of coordinate boundaries in crs dimensions 
//!                (xmin,ymin,xmax,ymax - corresponds to west,south,east,north in epsg:4326)
//!       format : response data image type ("tif", "png")
//!       width  : response data (image) width in pixels - we keep this optional and if not set use source data resolution
//!       height : response data (image) height in pixels - see 'width'

#[macro_use]
extern crate lazy_static;

use std::default::Default;
use std::error::Error;
use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::{MatchedPath,Query},
    http::Request,
    response::Html,
    Router,
    routing::get
};
use serde_derive::Deserialize;
use structopt::StructOpt;
use tokio::net::TcpListener;
use tower_http::{
    classify::{ServerErrorsAsFailures, SharedClassifier},
    trace::TraceLayer,
};
use tracing::{info_span, Level, Span};
use tracing_subscriber::{filter, layer::SubscriberExt, util::SubscriberInitExt};
use anyhow::Result;

use odin_build::set_bin_context;
use odin_common::geo::BoundingBox;
use odin_common::strings::deserialize_arr4;
use odin_server::{spawn_server_task,ServerConfig};
use odin_dem::load_config;


/// DEM configuration data
#[derive(Deserialize, Debug)]
pub struct DemConfig {
    pub vrt_path: String,
}

// the "default_xx" paths are a serde quirk - no default_value, only functions allowed
#[derive(Deserialize,Debug)]
struct GetMapQuery {
    #[serde(default = "default_service")]
    service: String,

    #[serde(default = "default_version")]
    version: String,

    layers: Option<String>,

    styles: Option<String>,

    #[serde(default = "default_crs")]
    crs: String,

    #[serde(deserialize_with="odin_common::strings::deserialize_arr4")]
    bbox:[f64;4],

    width: u32,
    height: u32,

    #[serde(default = "default_format")]
    format: String,

    #[serde(default = "default_transparent")]
    transparent: bool
}

fn default_service() -> String { "WMS".into() }
fn default_version() -> String { "1.3".into() }
fn default_crs() -> String { "EPSG:4326".into() }
fn default_format() -> String { "image/tif".into() }
fn default_transparent() -> bool { false }


#[tokio::main]
async fn main () -> Result<()> {
    odin_build::set_bin_context!();

    let config = load_config("dem_server.ron")?;
    let router = Router::new().route("/odin-dem/GetMap", get(get_map_handler));
    let server_task = spawn_server_task( &config, "GetMap", router);
    Ok( server_task.await? )
}

async fn get_map_handler (Query(q): Query<GetMapQuery>) { // just a '200 Ok' response for now
    println!("@@ query: {q:?}")
}