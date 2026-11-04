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

use std::{borrow::Cow, collections::{HashMap,VecDeque}, path::{Path,PathBuf}, io::{Read, Write}, sync::Arc, time::Duration};
use chrono::{DateTime,Utc};
use serde::{Serialize, Deserialize, Deserializer, de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor}};
use reqwest::{Client,Response};
use async_trait::async_trait;
use uom::si::{length::meter};
use uom::si::f64::Length;
use odin_build::{data_dir, pkg_cache_dir, pkg_data_dir, define_load_config, define_load_asset};
use odin_actor::prelude::*;
use odin_server::{spa::SpaService, ws_service::ws_msg_from_json};
use odin_common::{
    angle::{Angle360, Angle90}, cartesian3::Cartesian3, cartographic::Cartographic, collections::RingDeque,
    datetime::{utc_now, EpochMillis}, fs::{self, filename},
    geo::{GeoPoint, GeoPoint3, GeoPolygon}, json_writer::{JsonWritable, JsonWriter, NumFormat}, net::{download_url, NO_HEADERS}
};
use odin_macro::{define_struct, public_struct};

pub mod overlay;
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

/// our internal data model for (selected) AlertCalifornia cameras. Each `Camera` object represents the state of a camera
/// at a given time (`last_update`) as parsed from the AlertCalifornia `all_cameras` GeoJSON data. Note that cameras can
/// move, i.e. the position is part of the camera state
#[derive(Debug,Clone)]
#[public_struct]
struct Camera {
    id: Arc<String>,     // not human readable (shared between all states of the same camera)
    name: String,        // human readable name (used for display)

    position: GeoPoint3,
    pos: Cartesian3,
    max_fov: Angle90,    // horizontal field of view of the camera

    last_update: EpochMillis,

    fov_dist: Length,
    fov_angle: Angle90,

    azimut: Angle360,    // current camera direction (derived from fov_center for cameras that do not report azimut)
    tilt: Option<Angle90>,  // only reported by PTZ cameras
    zoom: Option<f64>,      // ditto

    image: Option<PathBuf>,
}

impl Camera {
    /// the time dependent (data) part of this camera state
    fn write_data_json_to (&self, w: &mut JsonWriter) {
        w.write_object(|w| {
            w.write_field("date", self.last_update.millis());
            w.write_f64_field("fovDist", self.fov_dist.get::<meter>(), NumFormat::Fp0);
            w.write_f64_field("fovAngle", self.fov_angle.degrees(), NumFormat::Fp2);
            w.write_f64_field("azimut", self.azimut.degrees(), NumFormat::Fp2);
            if let Some(tilt) = &self.tilt { w.write_f64_field("tilt", tilt.degrees(), NumFormat::Fp2); }
            if let Some(zoom) = self.zoom { w.write_f64_field("zoom", zoom, NumFormat::Fp1); }
            if let Some(path) = &self.image && let Some(fname) = filename( path) {
                w.write_field("image", fname); // we make sure it is valid utf8
            }
        })
    }

    /// write camera JSON with this as the latest state and `history` (oldest first, including self) as data entries
    fn write_json_with_history<'a> (&self, w: &mut JsonWriter, history: impl Iterator<Item=&'a Camera>) {
        w.write_object( |w| {
            w.write_field("id", self.id.as_str());
            w.write_field("name", self.name.as_str());
            w.write_json_field("position", &self.position);
            w.write_json_field("pos", &self.pos);
            w.write_f64_field("maxFov", self.max_fov.degrees(), NumFormat::Fp2);
            w.write_array_field("data", |w| {
                for c in history { c.write_data_json_to(w); }
            })
        })
    }
}

/// camera update messages contain the same camera objects as snapshot messages, but only the new state as data
impl JsonWritable for Camera {
    fn write_json_to (&self, w: &mut JsonWriter) {
        self.write_json_with_history( w, std::iter::once(self));
    }
}

pub fn get_json_update_msg (updates: &Vec<Camera>)->String {
    let mut w = JsonWriter::with_capacity(8192);
    w.write_object( |w| {
        w.write_field("date", EpochMillis::now().millis());
        w.write_array_field("changes", |w| {
            for camera in updates {
                camera.write_json_to(w);
            }
        })
    });
    ws_msg_from_json( AlertCaService::mod_path(), "update", w.as_str())
}

/// the store for the (configured) cameras we have received. For each camera we keep a history of its last `max_history` states
pub struct CameraStore {
    map: HashMap<String,VecDeque<Camera>>,
    max_history: usize,
    last_update: EpochMillis
}

impl CameraStore {
    pub fn new (max_history: usize)->Self {
        CameraStore{ map: HashMap::new(), max_history: max_history.max(1), last_update: EpochMillis::new(0) }
    }

    /// the latest state of camera `id`
    pub fn get (&self, id: &str)->Option<&Camera> { self.map.get( id).and_then( |h| h.back()) }

    pub fn history (&self, id: &str)->Option<&VecDeque<Camera>> { self.map.get( id) }

    pub fn len (&self)->usize { self.map.len() }

    pub fn last_update (&self)->EpochMillis { self.last_update }

    /// add new camera states. This also adds cameras we did not have before
    pub fn update_all (&mut self, cameras: Vec<Camera>) {
        for camera in cameras.into_iter() {
            if camera.last_update > self.last_update {
                self.last_update = camera.last_update;
            }

            let history = self.map.entry( camera.id.to_string()).or_insert_with( || VecDeque::with_capacity( self.max_history));
            while history.len() >= self.max_history { history.pop_front(); }
            history.push_back( camera);
        }
    }

    pub fn get_json_snapshot_msg (&self)->String {
        let mut w = JsonWriter::with_capacity(8192);
        w.write_object( |w| {
            w.write_field("date", self.last_update.millis());
            w.write_array_field("cameras", |w| {
                for history in self.map.values() {
                    if let Some(latest) = history.back() {
                        latest.write_json_with_history( w, history.iter());
                    }
                }
            })
        });
        ws_msg_from_json( AlertCaService::mod_path(), "snapshot", w.as_str())
    }
}

/// specification of which cameras to retrieve when
#[derive(Deserialize,Serialize,Debug)]
#[public_struct]
struct AlertCaConfig {
    cameras: Vec<Arc<String>>,  // we use ids for 3 different data structures - share the ids to avoid duplicate allocation
    base_url: String, // for all-camera / image retrieval
    update_interval: Duration, // data retrieval interval
    max_history: usize, // number of camera states we keep per camera
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

/// map from (configured) camera ids to shared id objects. This allows lookup without allocation while parsing
pub fn camera_ids (config: &AlertCaConfig)->HashMap<String,Arc<String>> {
    HashMap::from_iter( config.cameras.iter().map( |id| (id.to_string(), id.clone())))
}

/* #region all_cameras GeoJSON parsing *********************************************************************************/

// The AlertCalifornia all_cameras GeoJSON FeatureCollection contains >1000 features but we only need a small subset of
// them. We therefore stream-parse the features with serde_json, deserializing each one into a lightweight (borrowing)
// RawFeature that only contains the properties we need (others are skipped) and only turn features with configured ids
// into Camera objects. Property order and additional properties are irrelevant

#[derive(Deserialize)]
struct RawGeometry {
    coordinates: Vec<f64>, // [lon, lat, height]
}

#[derive(Deserialize)]
struct RawProperties<'a> {
    #[serde(borrow)] id: Cow<'a,str>,
    #[serde(borrow)] name: Option<Cow<'a,str>>,
    fov_lft: Option<[f64;2]>,
    fov_rt: Option<[f64;2]>,
    fov_center: Option<[f64;2]>,
    fov: Option<f64>,
    az_current: Option<f64>,
    tilt_current: Option<f64>,
    zoom_current: Option<f64>,
    last_frame_ts: Option<i64>, // epoch seconds
}

#[derive(Deserialize)]
struct RawFeature<'a> {
    geometry: Option<RawGeometry>,
    #[serde(borrow)] properties: RawProperties<'a>,
}

/// parse the all_cameras GeoJSON `data` and return the cameras with ids from `ids` (see [camera_ids]).
/// Features without position or last_frame_ts are ignored
pub fn parse_cameras (data: &[u8], ids: &HashMap<String,Arc<String>>)->Result<Vec<Camera>> {
    let mut cameras: Vec<Camera> = Vec::with_capacity( ids.len());
    let mut de = serde_json::Deserializer::from_slice( data);
    FeatureCollectionSeed{ ids, cameras: &mut cameras }.deserialize( &mut de)?;
    de.end()?;
    Ok(cameras)
}

struct FeatureCollectionSeed<'s> {
    ids: &'s HashMap<String,Arc<String>>,
    cameras: &'s mut Vec<Camera>,
}

impl<'de,'s> DeserializeSeed<'de> for FeatureCollectionSeed<'s> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>> (self, deserializer: D)->std::result::Result<(),D::Error> {
        deserializer.deserialize_map( self)
    }
}

impl<'de,'s> Visitor<'de> for FeatureCollectionSeed<'s> {
    type Value = ();

    fn expecting (&self, f: &mut std::fmt::Formatter)->std::fmt::Result { f.write_str("GeoJSON FeatureCollection") }

    fn visit_map<A: MapAccess<'de>> (self, mut map: A)->std::result::Result<(),A::Error> {
        while let Some(key) = map.next_key::<Cow<'de,str>>()? {
            if key == "features" {
                map.next_value_seed( FeaturesSeed{ ids: self.ids, cameras: &mut *self.cameras })?;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(())
    }
}

struct FeaturesSeed<'s> {
    ids: &'s HashMap<String,Arc<String>>,
    cameras: &'s mut Vec<Camera>,
}

impl<'de,'s> DeserializeSeed<'de> for FeaturesSeed<'s> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>> (self, deserializer: D)->std::result::Result<(),D::Error> {
        deserializer.deserialize_seq( self)
    }
}

impl<'de,'s> Visitor<'de> for FeaturesSeed<'s> {
    type Value = ();

    fn expecting (&self, f: &mut std::fmt::Formatter)->std::fmt::Result { f.write_str("array of GeoJSON Features") }

    fn visit_seq<A: SeqAccess<'de>> (self, mut seq: A)->std::result::Result<(),A::Error> {
        while let Some(feature) = seq.next_element::<RawFeature<'de>>()? {
            if let Some(id) = self.ids.get( feature.properties.id.as_ref()) {
                if let Some(camera) = camera_from_feature( id.clone(), feature) {
                    self.cameras.push( camera);
                }
            }
        }
        Ok(())
    }
}

fn camera_from_feature (id: Arc<String>, feature: RawFeature)->Option<Camera> {
    let coords = feature.geometry?.coordinates;
    if coords.len() < 2 { return None }
    let p = feature.properties;

    let (lon, lat) = (coords[0], coords[1]);
    let alt = if coords.len() > 2 { coords[2] } else { 0.0 };
    let position = GeoPoint3::from_lon_lat_degrees_alt_meters( lon, lat, alt);
    let pos = position.to_cartesian3();

    let last_update = EpochMillis::from_secs( p.last_frame_ts?);
    let name = p.name.map( |n| n.to_string()).unwrap_or_else( || id.to_string());
    let max_fov = Angle90::from_degrees( p.fov.unwrap_or(0.0));

    let p_center = Cartographic::from_degrees( lon, lat, 0.0);
    let (fov_dist, mut fov_angle) = match (p.fov_lft, p.fov_rt) {
        (Some(lft), Some(rt)) => compute_fov( &p_center, &Cartographic::from_degrees( lft[0], lft[1], 0.0), &Cartographic::from_degrees( rt[0], rt[1], 0.0)),
        _ => (Length::new::<meter>(0.0), Angle90::from_degrees(0.0))
    };
    // the reported (zoom dependent) horizontal fov is more precise than the one computed from the fov boundary points
    if let Some(fov) = p.fov && fov > 0.0 { fov_angle = Angle90::from_degrees( fov); }

    let azimut = if let Some(az) = p.az_current {
        Angle360::from_degrees( az)
    } else if let Some(c) = p.fov_center { // fixed (non-PTZ) camera
        Angle360::from_degrees( p_center.bearing_to( &Cartographic::from_degrees( c[0], c[1], 0.0)).to_degrees())
    } else {
        Angle360::from_degrees( 0.0)
    };
    let tilt = p.tilt_current.map( |t| Angle90::from_degrees(t));
    let zoom = p.zoom_current;

    Some( Camera{ id, name, position, pos, max_fov, last_update, fov_dist, fov_angle, azimut, tilt, zoom, image: None } )
}

/// compute distance and angle of the field of view from the camera position and the left and right fov boundary points
fn compute_fov (p_center: &Cartographic, p_lft: &Cartographic, p_rt: &Cartographic) -> (Length,Angle90) {
    let a_lft = p_center.bearing_to( p_lft).to_degrees();
    let mut a_rt = p_center.bearing_to( p_rt).to_degrees();
    if a_rt < a_lft { a_rt += 360.0; }

    let fov_dist = Length::new::<meter>(p_center.distance_to( p_lft));
    let fov_angle = Angle90::from_degrees( a_rt - a_lft);
    (fov_dist, fov_angle)
}

/* #endregion all_cameras GeoJSON parsing */

/* #region file download functions *************************************************************************************/

pub async fn get_all_cameras (client: &Client, config: &AlertCaConfig, download_path: impl AsRef<Path>)->Result<u64> {
    let url = format!("{}/all_cameras-v3.json", config.base_url);
    Ok( download_url( client, &url, NO_HEADERS, download_path).await? )
}

pub async fn get_latest_camera_data (client: &Client, config: &AlertCaConfig)->Result<Vec<u8>> {
    let path = all_cameras_filepath();
    let len = get_all_cameras(client, config, &path).await?;
    if len <= 0 { return  Err( op_failed!("no camera data")) }

    Ok( fs::filepath_contents(&path)? )
}

pub async fn get_latest_image (client: &Client, config: &AlertCaConfig, camera_id: &str, download_path: impl AsRef<Path>)->Result<u64> {
    let url = format!("{}/{}/latest-frame.jpg", config.base_url, camera_id);
    Ok( download_url( client, &url, NO_HEADERS, download_path).await? )
}

/* #endregion file download functions */
