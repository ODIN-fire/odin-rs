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

//! module to serve data from a GDAL vrt file
//! While we do provide our own api (GetValues) we also want to eventually support minimal
//! [WMS](https://portal.ogc.org/files/?artifact_id=14416) endpoints, i.e. mainly
//!```man_page
//!      GET <host>:<port>/GetMap?<query>
//!
//! with query parameters
//!
//!       crs    : coordinate reference system ("epsg:<number>")
//!       bbox   : comma separated list of coordinate boundaries in crs dimensions
//!                (xmin,ymin,xmax,ymax - corresponds to west,south,east,north in epsg:4326)
//!       format : response data image type ("tif", "png")
//!       width  : response data (image) width in pixels - we keep this optional and if not set use source data resolution
//!       height : response data (image) height in pixels - see 'width'
//!```

use anyhow::Result;

use odin_build;
use odin_common::define_cli;
use odin_gdal::geods_srv::run_server;

define_cli! { ARGS [about="serve_geods - serve GDAL dataset"] =
    path: String [help="path to GeoDsServerConfig"],
    // possibly more in the future
}

#[tokio::main]
async fn main () -> Result<()> {
    odin_build::set_bin_context!();
    let config_path = ARGS.path.clone();

    Ok( run_server( config_path).await? )
}
