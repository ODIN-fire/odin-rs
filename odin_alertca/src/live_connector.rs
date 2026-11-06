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

use odin_actor::prelude::*;
use odin_common::{angle::{Angle90, Angle360}, datetime::{EpochMillis,secs}};
use odin_image::ocr::{OcrEngine, create_default_ocr_engine};
use crate::{
    actor::{AlertCaActorMsg, CameraUpdates},
    errors::{op_failed, OdinAlertCaError, Result}, camera_ids, get_latest_camera_data, get_latest_image, image_filepath, parse_cameras,
    AlertCaConfig, AlertCaConnector, Camera, CameraAttrs, overlay::read_image_overlay
};

/// an http based AlertCaConnector that does fixed schedule downloads of camera data
/// Note that `AlertCaConnector` instances are used for dependency injection into [crate::actor::AlertCaActor] and hence
/// are created before we have a respective [ActorHandle]
pub struct LiveAlertCaConnector {
    config: Arc<AlertCaConfig>,
    task: Option<AbortHandle>
}

impl LiveAlertCaConnector {
    /// called before actor instantiation
    pub fn new (config: Arc<AlertCaConfig>)->Self {
        LiveAlertCaConnector { config, task: None }
    }
}

/// the live connector periodically retrieves the latest all_cameras GeoJSON file, parses the configured cameras of interest,
/// retrieves images for the ones that have been updated and reports them as a `Vec<Camera>` to the actor.
/// The first report contains all configured cameras we found (with or without image) - it is used to initialize the
/// actors CameraStore. Subsequent reports only contain updated cameras for which we could retrieve images
#[async_trait]
impl AlertCaConnector for LiveAlertCaConnector {

    async fn start (&mut self, hself: ActorHandle<AlertCaActorMsg>)->Result<()> {
        const MAX_RETRIES: usize = 3;

        if self.task.is_none() {
            let config = self.config.clone();

            let jh = spawn( "alertca-connector", async move {
                let client = Client::new();

                // OCR engine to read camera values from image overlays (cameras might move between data updates and
                // the AlertCalifornia data does not include PTZ values for all cameras)
                let ocr = match create_default_ocr_engine() {
                    Ok(engine) => Some( Arc::new(engine)),
                    Err(e) => { eprintln!("no image overlay OCR: {e}"); None }
                };
                let ids = camera_ids( &config);
                let mut last_updates: HashMap<Arc<String>,EpochMillis> = HashMap::new();
                let mut is_first = true;
                let mut retries = MAX_RETRIES;
                let mut sleep_dur = config.update_interval;

                loop {
                    match get_camera_updates( &client, &config, &ids, &mut last_updates).await {
                        Ok(mut updates) => {
                            retries = MAX_RETRIES;
                            sleep_dur = config.update_interval;

                            for camera in updates.iter_mut() {
                                let img_path = image_filepath( camera.id.as_str(), camera.last_update);
                                match get_latest_image( &client, &config, camera.id.as_str(), &img_path).await {
                                    Ok(_) => {
                                        if let Some(ocr) = &ocr {
                                            update_from_overlay( camera, ocr.clone(), &img_path).await;
                                        }
                                        camera.image = Some( img_path)
                                    }
                                    Err(e) => eprintln!("error retrieving image file {img_path:?}: {e}")
                                }
                            }

                            if !is_first {
                                updates.retain( |camera| camera.image.is_some() ); // drop the updates for which we didn't get images
                            }

                            if is_first || !updates.is_empty() {
                                if hself.send_msg( CameraUpdates(updates)).await.is_ok() { // let the actor know
                                    is_first = false;
                                }
                            }
                        }
                        Err(e) => {
                            if retries > 0 {
                                retries -= 1;
                                sleep_dur = secs(30);
                                eprintln!("error retrieving camera list: {e}, retry in 30 sec..");
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

/// download and parse the all_cameras GeoJSON and return the configured cameras that have been updated since the last call
async fn get_camera_updates (client: &Client, config: &AlertCaConfig, ids: &HashMap<String,CameraAttrs>,
                             last_updates: &mut HashMap<Arc<String>,EpochMillis>)->Result<Vec<Camera>>
{
    let contents = get_latest_camera_data( client, config).await?;
    let mut cameras = parse_cameras( &contents, ids)?;

    cameras.retain( |camera| {
        let last = last_updates.entry( camera.id.clone()).or_insert( EpochMillis::new(0));
        if camera.last_update > *last {
            *last = camera.last_update;
            true
        } else {
            false
        }
    });

    Ok(cameras)
}

/// read the camera values (azimuth, tilt, zoom) from the image overlay and use them for the camera state. OCR is CPU
/// intensive, i.e. we have to execute it in a blocking task
async fn update_from_overlay (camera: &mut Camera, ocr: Arc<OcrEngine>, img_path: &std::path::Path) {
    let path = img_path.to_path_buf();
    match tokio::task::spawn_blocking( move || read_image_overlay( &ocr, &path)).await {
        Ok(Ok(overlay)) => {
            camera.azimut = Angle360::from_degrees( overlay.azimut);
            camera.tilt = Some( Angle90::from_degrees( overlay.tilt));
            camera.zoom = Some( overlay.zoom);
        }
        Ok(Err(e)) => eprintln!("failed to read overlay of {img_path:?}: {e}"),
        Err(e) => eprintln!("overlay task failed: {e}")
    }
}
