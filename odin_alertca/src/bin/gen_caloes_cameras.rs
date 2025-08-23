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

use std::{collections::HashMap, fs::File, io::{Read, Write}, path::{Path, PathBuf}};
use tokio::{self};
use ron;
use odin_common::{define_cli, extract_fields, u8extractor::{CsvStr, CsvFieldExtractor, CsvExtractor, AsyncCsvExtractor}, datetime::EpochMillis, fs::EnvPathBuf};
use odin_dem::DemSource;
use odin_alertca::CalOesCamera;
use anyhow::{anyhow,Result,Error};

/// read precise camera positions from CalOES data in CSV format (retrieved from https://gis-calema.opendata.arcgis.com/datasets/fire-camera-viewsheds/explore)
/// note that we are only interested in camera name and precise lon/lat 
/// while the CSV file has an 'Elevation_Ft' field it is currently empty and we have to retrieve it ourselves from odin_dem
/// note also that download from the AlertCA site is interactive, rarely updated and currently fails in Chrome 

/* CSV format is
    OBJECTID,ID,Camera ID,Camera Type,Location,Source,Source_URL,Display_Type,View_Direction,Webcam_URL,PopUp_URL,Consolidated_URL,NearBy_Place,County,State,Elevation_Ft,Longitude,Latitude,GlobalID,Thumbnail URL,View Degrees,Tilt,Zoom,Fi>
    1,,Axis-TassajaraPeak,Fire,Tassajara_Peak,Alert California,https://ops.alertcalifornia.org/,Picture,Pan-Tilt-Zoom,https://ops.alertcalifornia.org/cam-console/2438,,https://ops.alertcalifornia.org/,,SanLuisObispo,,0,-120.708572,35.393>
    ...
 */

define_cli! { ARGS [about="get&store precise AlertCalifornia camera locations from CalOES data and ODIN DEM"] =
    dem_path: String [help="file to DEM VRT file", long, short, default_value="../../data/3dep13-conus-i16/3dep13-conus-i16.vrt"],
    output_path: String [help="output path where to store serialized HashMap<String,CalOesCamera> resulte", long, short, default_value="../../data/odin_alertca/CalOesCameras.ron"],
    path: String [help="file path of CSV file downloaded from https://gis-calema.opendata.arcgis.com/datasets/fire-camera-viewsheds/explore"]
}

#[tokio::main]
async fn main ()->Result<()> {

    let dem_path = Path::new( &ARGS.dem_path).to_path_buf();
    if !dem_path.is_file() {
        return Err( anyhow!("invalid DEM path: {}", ARGS.dem_path));
    }
    let dem = DemSource::File(EnvPathBuf(dem_path));

    let path = Path::new( &ARGS.path);
    if path.is_file() {
        let file = File::open(&path)?;
        let mut reader = std::io::BufReader::with_capacity( 8192, file);
        let mut csv = CsvExtractor::new(reader);
        let mut cameras: Vec<CalOesCamera> = Vec::with_capacity(2048);

        while csv.next_line()? {
            extract_fields!{ csv ?
                let id: CsvStr = [2],
                let elev_ft: i64 = [15], 
                let lon: f64 = [16],
                let lat: f64 = [17],
                let fov: f64 = [23] => {
                    let id = id.to_string();
                    let height = 0.0; // filled in by DEM later
                    let camera = CalOesCamera { id, lon, lat, fov, height };

                    cameras.push( camera)
                }
            }
        }

        let locations: Vec<(f64,f64)> = cameras.iter().map( |c| (c.lon,c.lat)).collect();
        match dem.get_heights( None, &locations).await {
            Ok(heights) => {
                for i in 0..locations.len() { 
                    cameras[i].height = heights[i]; 
                    println!("{:?}", cameras[i]);
                }

                let kvs: Vec<(String,CalOesCamera)> = cameras.into_iter().map(|c| (c.id.clone(), c)).collect(); 
                let map: HashMap<String,CalOesCamera> = HashMap::from_iter(kvs.into_iter());
                
                match File::create( &ARGS.output_path) {
                    Ok(mut file) => {
                        let buf = ron::to_string(&map)?;
                        file.write_all(buf.as_bytes())?;
                        println!("serialized output written to {}", ARGS.output_path);

                        Ok(())
                    }
                    Err(e) => Err( anyhow!("failed to write serialized output to {}: {e}", ARGS.output_path))
                }
            }
            Err(e) => Err( anyhow!("failed to retrieve heights: {e}"))
        }

    } else {
        Err( anyhow!("file not found: {}", ARGS.path))
    }
}