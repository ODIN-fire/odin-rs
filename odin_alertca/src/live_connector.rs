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

use std::{collections::HashMap, sync::Arc};
use reqwest::Client;
use async_trait::async_trait;
use uom::si::{length::meter};
use uom::si::f64::Length;

use odin_actor::prelude::*;
use odin_common::{
    angle::{Angle360,Angle90}, 
    cartographic::Cartographic,
     datetime::{EpochMillis,secs}, 
     extract_all, 
     fs::filepath_contents, geo::GeoPoint, u8extractor::{read_val, MemMemFinder, U8Readable}
};
use odin_macro::{define_struct, public_struct};
use crate::{
    actor::{AlertCaActorMsg, CameraUpdates}, all_cameras_filepath, 
    errors::{op_failed, OdinAlertCaError, Result}, get_all_cameras, get_latest_camera_data, get_latest_image, image_filepath, 
    AlertCaConfig, AlertCaConnector, CalOesCamera, CameraUpdate, VarCameraData
};

/// an http based AlertCaConnector that does fixed schedule downloads of camera data
/// Note that `AlertCaConnector` instances are used for dependency injection into [crate::actor::AlertCaActor] and hence
/// are created before we have a respective [ActorHandle]
pub struct LiveAlertCaConnector { 
    config: Arc<AlertCaConfig>,
    cameras: Arc<HashMap<String,CalOesCamera>>,
    task: Option<AbortHandle>
}

impl LiveAlertCaConnector {
    /// called before actor instantiation
    pub fn new (config: Arc<AlertCaConfig>, cameras: Arc<HashMap<String,CalOesCamera>>)->Self {
        LiveAlertCaConnector { config, cameras, task: None }
    }
}

/// the live connector retrieves the latest all_cameras-v3.json file (2.5MB), checks which ones of the
/// configured cameras of interest have been updated, retrieved respective image files and reports
/// this as a `Vec<CameraUpdate>` to the actor
#[async_trait]
impl AlertCaConnector for LiveAlertCaConnector {

    async fn start (&mut self, hself: ActorHandle<AlertCaActorMsg>)->Result<()> {
        const MAX_RETRIES: usize = 3;

        if self.task.is_none() {
            let config = self.config.clone();
            let cameras = self.cameras.clone();

            let jh = spawn( "alertca-connector", async move {
                let client = Client::new();
                let finder = PropertyFinder::new();
                let mut last_updates = create_last_updates(&config);
                let mut retries = MAX_RETRIES;
                let mut sleep_dur = config.update_interval;

                loop {
                    match get_camera_updates( &client, &config, &cameras, &finder, &mut last_updates).await {
                        Ok(mut updates) => {
                            retries = MAX_RETRIES;
                            sleep_dur = config.update_interval;

                            for update in updates.iter_mut() {
                                let img_path = image_filepath( update.id.as_str(), update.data.last_update);
                                match get_latest_image( &client, &config, update.id.as_str(), &img_path).await {
                                    Ok(_) => {
                                        update.data.image = Some( img_path)
                                    }
                                    Err(e) => {
                                        update.data.image = None;
                                        eprintln!("error retrieving image file {img_path:?}: {e}");
                                    }
                                }
                            }

                            updates.retain( |update| update.data.image.is_some() ); // drop the updates for which we didn't get images
                            hself.send_msg( CameraUpdates(updates)).await; // let the actor know
                        }
                        Err(e) => {
                            if retries > 0 {
                                retries -= 1;
                                sleep_dur = secs(30);
                                println!("retry retrieving camera list in 30 sec..");
                            } else {
                                sleep_dur = config.update_interval;
                                eprintln!("error processing camera list: {e}")
                            }
                        }
                    }

                    sleep( sleep_dur).await;
                }
            })?;
            self.task = Some(jh.abort_handle());
        }
        Ok(())
    }

    fn terminate (&mut self) {
        if let Some(ah) = &self.task {
            ah.abort();
            self.task = None;
        }
    }
}


async fn get_camera_updates (client: &Client, config: &AlertCaConfig, cameras: &HashMap<String,CalOesCamera>, finder: &PropertyFinder, last_updates: &mut HashMap<String,LastUpdateEntry>)->Result<Vec<CameraUpdate>> {
    let path = all_cameras_filepath();
    let len = get_all_cameras(client, config, &path).await?;
    if len <= 0 { return  Err( op_failed!("no camera data")) }

    let contents = filepath_contents(&path)?;
    let mut data: &[u8] = contents.as_slice();
    let mut updates: Vec<CameraUpdate> = Vec::with_capacity(config.cameras.len());

    while let Some(i0) = finder.id.find_key(data) { 
        let i1 = i0 + finder.id.len();
        if let Some((id,len)) = read_val::<&str>( data, i1) {
            if let Some(update_entry) = last_updates.get_mut( id) { // this is a camera we care about
                let bs = &data[i1+len..];
                extract_all!{ bs ?
                    let last_frame_ts: i64 = finder.last_frame_ts,
                    let fov_lft: [f64;2] = finder.fov_lft,
                    let fov_rt: [f64;2] = finder.fov_rt,
                    let az_current: f64 = finder.az_current,
                    let tilt_current: f64 = finder.tilt_current,
                    let zoom_current: f64 = finder.zoom_current => {
                        let last_update = EpochMillis::from_secs( last_frame_ts);
                        if last_update > update_entry.last_update {
                            if let Some(camera) = cameras.get( id) {
                                let p_center = Cartographic::from_degrees( camera.lon, camera.lat, 0.0);
                                let p_lft = Cartographic::from_degrees( fov_lft[0], fov_lft[1], 0.0);
                                let p_rt = Cartographic::from_degrees( fov_rt[0], fov_rt[1], 0.0);
                                let (fov_dist,fov_angle) = compute_fov( p_center, p_lft, p_rt);

                                let azimut = Angle360::from_degrees(az_current);
                                let tilt = Angle90::from_degrees(tilt_current);
                                let zoom = zoom_current;
                                let image = None;

                                let id = update_entry.id.clone();
                                let data = VarCameraData{last_update,fov_dist,fov_angle,azimut,tilt,zoom,image};
                                updates.push( CameraUpdate{id,data} );

                                update_entry.last_update = last_update;
                            }
                        }
                    }
                }
            } 
            data = &data[i1+len..];
        } else {
            data = &data[i1..];
        }
    }

    Ok(updates)
}

fn compute_fov (p_center: Cartographic, p_lft: Cartographic, p_rt: Cartographic) -> (Length,Angle90) {
    let a_lft = p_center.bearing_to( &p_lft).to_degrees();
    let mut a_rt = p_center.bearing_to( &p_rt).to_degrees();
    if a_rt < a_lft { a_rt += 360.0; }

    let fov_dist = Length::new::<meter>(p_center.distance_to( &p_lft));
    let fov_angle = Angle90::from_degrees( (a_rt - a_lft));
    (fov_dist, fov_angle)
}

/// spec of what to retrieve from all-cameras-v3.json downloads. Since we only get the whole list from AlertCalifornia, we are only interested in a small
/// fraction of the 1000+ entries and only in a subset of fields per camera entry it would be overkill to parse the whole 2.5MB file
/// as GeoJSON on each download.
/// NOTE - this relies on all-cameras-v3.json format. This file is pretty-print line formatted (one property per line) with a fixed order of properties.
/// Should that change we would have to adapt / resort to full GeoJSON parsing
/// the alternative would be a nice little regex like:
///   "properties": *\{\s*"id":\s*"(.*)".*\s*.*\s*"last_frame_ts":\s*(\d+).*\s*"fov_lft":\s*\[\s*([-.\d]+).*\s*([-.\d]+)\s*],.*\s*"fov_rt":\s*\[\s*([-.\d]+).*\s*([-.\d]+)\s*],(?:.*\n)*\s*"az_current":\s*([-.\d]+),(?:.*\n)*\s*"tilt_current":\s*([-.\d]+),(?:.*\n)*\s*"zoom_current":\s*([-.\d]+),(?:.*\n)*\s*\}
define_struct! {
    pub PropertyFinder =
        pub id: MemMemFinder<'static>            = MemMemFinder::new(b"\"id\":"), // string
        pub last_frame_ts: MemMemFinder<'static> = MemMemFinder::new(b"\"last_frame_ts\":"), // epoch seconds
        pub fov_lft: MemMemFinder<'static>       = MemMemFinder::new(b"\"fov_lft\":"), // two number array on follow lines
        pub fov_rt: MemMemFinder<'static>        = MemMemFinder::new(b"\"fov_rt\":"), // two number array on follow lines
        pub fov_center: MemMemFinder<'static>    = MemMemFinder::new(b"\"fov_center\":"), // two number array on follow lines
        pub az_current: MemMemFinder<'static>    = MemMemFinder::new(b"\"az_current\":"), // f64
        pub tilt_current: MemMemFinder<'static>  = MemMemFinder::new(b"\"tilt_current\":"), // f64
        pub zoom_current: MemMemFinder<'static>  = MemMemFinder::new(b"\"zoom_current\":") // f64
}

/// internal helper struct to keep track of camera updates and provide shared ids
#[public_struct]
struct LastUpdateEntry {
    id: Arc<String>, // this is used as a shared id across CameraStore, CameraUpdate
    last_update: EpochMillis,
}

// we need fast lookup for &str keys (without allocation) but also a shared id for the matches 
pub fn create_last_updates (config: &AlertCaConfig)->HashMap<String,LastUpdateEntry> {
    HashMap::from_iter( config.cameras.iter().map( |id| {
        (id.to_string(), LastUpdateEntry{ id: id.clone(), last_update: EpochMillis::new(0) })
    }))
}
