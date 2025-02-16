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

use std::{env, error::Error, str::FromStr, fs::{self,File}, io::{Write,Read}};
use odin_common:: {define_cli, fs::ensure_writable_dir};
use std::path::{Path,PathBuf};
use tokio;
use reqwest;
use http::Uri;
use zip::read::ZipArchive;

define_cli! { ARGS [about="install_cesium - utility to install CesiumJS within given asset dir"] =
    version: String [help="cesium version to install",long,short,default_value="1.126"],
    target_dir: String [help="target directory",long,short, default_value="../../assets/odin_cesium"]
}

#[tokio::main]
async fn main()->Result<(),Box<dyn Error>> {
    odin_build::set_bin_context!();

    let tgt_dir = Path::new( &ARGS.target_dir).to_path_buf();
    println!("using target dir {}", ARGS.target_dir);
    ensure_writable_dir(&tgt_dir).expect("target dir does not exist or is not writable");

    let uri_str = format!("https://github.com/CesiumGS/cesium/releases/download/{0}/Cesium-{0}.zip", ARGS.version);
    let uri: Uri = Uri::from_str(uri_str.as_str())?;
    let uri_path = Path::new( uri.path());
    let fname = uri_path.file_name().unwrap().to_str().unwrap();
    println!("downloading CesiumJS archive from {uri}");

    let cur_dir = env::current_dir()?;
    env::set_current_dir( tgt_dir)?;
    
    // remove tmp if it exists
    let tmp_dir = Path::new("tmp");
    if tmp_dir.is_dir() {
        fs::remove_dir_all(tmp_dir)?;
    }
    
    fs::create_dir("tmp")?;
    env::set_current_dir("tmp")?;

    let client = reqwest::Client::new();
    println!("downloading...");

    let mut res = client.get( &uri_str).send().await?;
    println!("storing to file: {}, length: {}", fname, res.content_length().unwrap());

    let mut file = File::create_new(fname)?;
    while let Some(chunk) = res.chunk().await? {
        file.write(&chunk)?;
    }
    println!("file retrieved, unpacking...");

    let mut file = File::open(fname)?;
    let mut archive = ZipArchive::new(file)?;
    archive.extract( ".")?;

    env::set_current_dir("..")?;
    let tgt_dir = Path::new("cesiumjs");
    if tgt_dir.is_dir() {
        fs::remove_dir_all(tgt_dir)?;
    }

    println!("creating cesiumjs/ target directories");
    fs::create_dir("cesiumjs")?;
    fs::create_dir("cesiumjs/Widgets")?;
    fs::create_dir("cesiumjs/Workers")?;
    fs::create_dir("cesiumjs/Assets")?;
    fs::create_dir("cesiumjs/Assets/IAU2006_XYS")?;
    fs::create_dir("cesiumjs/Assets/Images")?;

    println!("moving files to target directories");
    fs::rename("tmp/LICENSE.md",                                          "cesiumjs/LICENSE.md")?;
    fs::rename("tmp/README.md",                                           "cesiumjs/README.md")?;
    fs::rename("tmp/Build/Cesium/Cesium.js",                              "cesiumjs/Cesium.min.js")?;
    fs::rename("tmp/Build/Cesium/Widgets/widgets.css",                    "cesiumjs/Widgets/widgets.css")?;
    fs::rename("tmp/Build/Cesium/Assets/approximateTerrainHeights.json",  "cesiumjs/Assets/approximateTerrainHeights.json")?;
    fs::rename("tmp/Build/Cesium/Assets/IAU2006_XYS/IAU2006_XYS_18.json", "cesiumjs/Assets/IAU2006_XYS/IAU2006_XYS_18.json")?;
    fs::rename("tmp/Build/Cesium/Assets/Images/ion-credit.png",           "cesiumjs/Assets/Images/ion-credit.png")?;

    println!("removing tmp directory");
    fs::remove_dir_all("tmp")?;

    env::set_current_dir( &cur_dir)?;
    println!("done.");

    Ok(())
}