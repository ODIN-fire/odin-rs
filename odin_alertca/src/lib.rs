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

use std::{collections::{HashMap,VecDeque}, path::{Path,PathBuf}, io::{Read, Write}, sync::Arc, time::Duration};
use chrono::{DateTime,Utc};
use serde::{Serialize,Deserialize};
use reqwest::{Client,Response};
use async_trait::async_trait;
use uom::si::{length::meter};
use uom::si::f64::Length;
use odin_build::{data_dir, pkg_cache_dir, pkg_data_dir, define_load_config, define_load_asset};
use odin_actor::prelude::*;
use odin_server::{spa::SpaService, ws_service::ws_msg_from_json};
use odin_common::{
    angle::{Angle360, Angle90}, cartesian3::Cartesian3, cartographic::Cartographic, collections::RingDeque, datetime::{utc_now, EpochMillis}, extract_all, extract_optional, fs::{self, filename}, geo::{GeoPoint, GeoPoint3, GeoPolygon}, json_writer::{JsonWritable, JsonWriter, NumFormat}, net::{download_url, NO_HEADERS} 
};
use odin_macro::{define_struct, public_struct};

pub mod errors;
use errors::op_failed;
pub type Result<T> = std::result::Result<T,errors::OdinAlertCaError>;

pub mod live_connector;

pub mod actor;
use actor::AlertCaActorMsg;

pub mod alertca_service;
use alertca_service::AlertCaService;

define_load_config!{}
define_load_asset!{}

/// our internal, updated data model for (selected) AlertCalifornia cameras
/// the fixed part is initialized from configuration and CalOES data
/// the VarCameraData is updated from connector messages 
#[public_struct]
struct Camera {
    id: Arc<String>,
    position: GeoPoint3,
    pos: Cartesian3,
    max_fov: Angle90,

    data: VecDeque<VarCameraData>
}

impl Camera {
    fn new (config: &AlertCaConfig, id: Arc<String>, cal_oes_cam: &CalOesCamera)->Self {
        let position = GeoPoint3::from_lon_lat_degrees_alt_meters( cal_oes_cam.lon, cal_oes_cam.lat, cal_oes_cam.height);
        let pos = position.to_cartesian3();
        let max_fov = Angle90::from_degrees(cal_oes_cam.fov);
        let data: VecDeque<VarCameraData> = VecDeque::with_capacity(config.max_history);

        Camera { id, position, pos, max_fov, data }
    }

    fn last_update (&self)->Option<DateTime<Utc>> {
        self.data.back().map( |d|  DateTime::from_timestamp_millis( d.last_update.millis()).unwrap())
    }

    fn last_update_timestamp (&self)->EpochMillis {
        self.data.back().map( |d|  d.last_update).or( Some(EpochMillis::new(0))).unwrap()
    }
}

impl JsonWritable for Camera {
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object( |w| {
            w.write_field("id", self.id.as_str());
            w.write_json_field("position", &self.position);
            w.write_json_field("pos", &self.pos);
            w.write_f64_field("maxFov", self.max_fov.degrees(), NumFormat::Fp1);
            w.write_array_field("data", |w| {
                for d in &self.data { d.write_json_to(w); }
            })
        })
    }
}

/// the variable camera data we store / publish
#[derive(Debug)]
#[public_struct]
struct VarCameraData {
    last_update: EpochMillis,

    fov_dist: Length,
    fov_angle: Angle90,

    azimut: Angle360,
    tilt: Angle90,
    zoom: f64,

    image: Option<PathBuf>,
}

impl VarCameraData {
    pub fn new()->Self {
        VarCameraData {
            last_update: EpochMillis::new(0),
            fov_dist: Length::new::<meter>(0.0),
            fov_angle: Angle90::from_degrees(0.0),
            azimut: Angle360::from_degrees(0.0),
            tilt: Angle90::from_degrees(0.0),
            zoom: 0.0,
            image: None
        }
    }
}


impl JsonWritable for VarCameraData {
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object(|w| {
            w.write_field("date", self.last_update.millis());
            w.write_f64_field("fovDist", self.fov_dist.get::<meter>(), NumFormat::Fp0);
            w.write_f64_field("fovAngle", self.fov_angle.degrees(), NumFormat::Fp0);
            w.write_f64_field("azimut", self.azimut.degrees(), NumFormat::Fp0);
            w.write_f64_field("tilt", self.tilt.degrees(), NumFormat::Fp0);
            w.write_f64_field("zoom", self.zoom, NumFormat::Fp0);
            if let Some(path) = &self.image && let Some(fname) = filename( path) {
                w.write_field("image", fname); // we make sure it is valid utf8
            }
        })
    }
}

#[derive(Debug)]
#[public_struct]
struct CameraUpdate {
    id: Arc<String>,
    data: VarCameraData,
}

impl JsonWritable for CameraUpdate {
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object(|w| {
            w.write_field("id", self.id.as_str());
            w.write_json_field("data", &self.data);
        })
    }
}

pub fn get_json_update_msg (updates: &Vec<CameraUpdate>)->String {
    let mut w = JsonWriter::with_capacity(8192);
    w.write_object( |w| {
        w.write_field("date", EpochMillis::now().millis());
        w.write_array_field("changes", |w| {
            for d in updates {
                d.write_json_to(w);
            }
        })
    });
    ws_msg_from_json( AlertCaService::mod_path(), "update", w.as_str())
}

/// abstraction for how we store updated Camera objects
/// since entries are static we accept the key duplication upon entry so that we can save temporary string objects on &str based lookup
pub struct CameraStore {
    map: HashMap<String,Camera>,
    last_update: EpochMillis
}

impl CameraStore {
    fn new(map: HashMap<String,Camera>)->Self { 
        CameraStore{ map, last_update: EpochMillis::new(0) } 
    }

    pub fn get(&self, id: &str)->Option<&Camera> { self.map.get( id) }
    pub fn get_mut (&mut self, id: &str)->Option<&mut Camera> { self.map.get_mut( id) }

    pub fn update_all (&mut self, camera_updates: Vec<CameraUpdate>) {
        for update in camera_updates.into_iter() {
            if let Some(camera) = self.map.get_mut(update.id.as_str()) {
                if update.data.last_update > self.last_update {
                    self.last_update = update.data.last_update;
                }

                camera.data.push_back( update.data);
            }
        }
    }

    pub fn get_json_snapshot_msg (&self)->String {
        let mut w = JsonWriter::with_capacity(8192);
        w.write_object( |w| {
            w.write_field("date", self.last_update.millis());
            w.write_array_field("cameras", |w| {
                for (id,camera) in self.map.iter() {
                    camera.write_json_to(w);
                }
            })
        });
        ws_msg_from_json( AlertCaService::mod_path(), "snapshot", w.as_str())
    }
}

/// camera information we get from CalOES data and DEM
/// only used during CameraStore construction for invariant Camera part
/// deserialized from data/config file
#[derive(Debug,Serialize,Deserialize)]
#[public_struct]
struct CalOesCamera {
    id: String,
    lon: f64, // in degrees
    lat: f64, // in degrees
    fov: f64, // in degrees
    height: f64 // in meters
}

impl CalOesCamera {
    pub fn new(id:&str, lon: f64, lat: f64, fov: f64, height: f64)-> Self {
        CalOesCamera { id: id.to_string(), lon, lat, fov, height }
    }
}

/// specification of which cameras to retrieve when
#[derive(Deserialize,Serialize,Debug)]
#[public_struct]
struct AlertCaConfig {
    cameras: Vec<Arc<String>>,  // we use ids for 3 different data structures - share the ids to avoid duplicate allocation
    base_url: String, // for all-camera / image retrieval
    update_interval: Duration, // data retrieval interval
    max_history: usize,
    max_age: Duration, // duration after which to drop camera data
}

#[async_trait]
pub trait AlertCaConnector {
    async fn start (&mut self, hself: ActorHandle<AlertCaActorMsg>)->Result<()>;
    fn terminate (&mut self);
}


/// filename for images: `<camera-id>_YYYY-MM-DD[THHMM[SS].jpg`
pub fn image_filepath (id: &str, last_update: EpochMillis)->PathBuf {
    let fname = format!("{}__{}.jpg", id, fs::epoch_millis_to_fname(last_update, fs::TimeResolution::Seconds));
    pkg_cache_dir!().join( fname)
}

pub fn all_cameras_filepath ()->PathBuf {
    pkg_cache_dir!().join( "all_cameras.json")
}

pub fn create_cameras (config: &AlertCaConfig, cal_oes_cameras: &HashMap<String,CalOesCamera>)->Result<CameraStore> {
    let mut map: HashMap<String,Camera> = HashMap::with_capacity( config.cameras.len());
    for camera_id in &config.cameras {
        if let Some(cal_oes_cam) = cal_oes_cameras.get( camera_id.as_str()) {
            let camera = Camera::new( config, camera_id.clone(), cal_oes_cam);
            map.insert( camera.id.to_string(), camera);
        } else {
            return Err( op_failed!("could not find camera position for {}", camera_id));
        }
    }

    Ok( CameraStore::new(map) )
}

pub fn get_cal_oes_cameras (path: impl AsRef<Path>)->Result<HashMap<String,CalOesCamera>> {
    let ron = fs::filepath_contents_as_string(&path)?;
    let mut map: HashMap<String,CalOesCamera> = ron::from_str(&ron)?;

    //--- add missing cameras (TODO - this should be added to the source files, not hardcoded)
    let mut missing_cameras = vec![
        CalOesCamera::new( "Axis-Almaden1", -121.847187, 37.194245, 62.8, 180.0),
        CalOesCamera::new( "Axis-Almaden2", -121.847187, 37.194245, 62.8, 180.0),
        CalOesCamera::new( "Axis-BlackMtSCC", -122.147301, 37.3186, 62.8, 855.0),
        CalOesCamera::new( "Axis-BlackMtSCC2", -122.147301, 37.3186, 62.8, 855.0),
    ];
    for c in missing_cameras.into_iter() { map.insert( c.id.clone(), c); }

    Ok( map )
}

pub fn get_default_cal_oes_cameras()->Result<HashMap<String,CalOesCamera>> {
    let cal_oes_path = pkg_data_dir!().join("CalOesCameras.ron");
    get_cal_oes_cameras(&cal_oes_path)
}




/* #region file download functions *************************************************************************************/

pub async fn get_all_cameras (client: &Client, config: &AlertCaConfig, download_path: impl AsRef<Path>)->Result<u64> {
    let url = format!("{}/all_cameras-v3.json", config.base_url);
    Ok( download_url( client, &url, &NO_HEADERS, download_path).await? )
}

pub async fn get_latest_camera_data (client: &Client, config: &AlertCaConfig)->Result<Vec<u8>> {
    let path = all_cameras_filepath();
    let len = get_all_cameras(client, config, &path).await?;
    if len <= 0 { return  Err( op_failed!("no camera data")) }

    Ok( fs::filepath_contents(&path)? )
}

pub async fn get_latest_image (client: &Client, config: &AlertCaConfig, camera_id: &str, download_path: impl AsRef<Path>)->Result<u64> {
    let url = format!("{}/{}/latest-frame.jpg", config.base_url, camera_id);
    Ok( download_url( client, &url, &NO_HEADERS, download_path).await? )
}

/* #endregion file download functions */