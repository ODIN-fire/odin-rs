/*
 * Copyright © 2026, United States Government, as represented by the Administrator of
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

use std::{collections::{HashMap, VecDeque}, path::{Path,PathBuf}, sync::Arc, time::Duration};
use chrono::{DateTime, Local, NaiveDateTime, Timelike, Utc};
use reqwest::{Client};

use odin_common::{datetime::{self, day_start, days, with_hms}, fs::{file_length, odin_data_filename, remove_old_files}, geo::GeoRect, net::{NO_HEADERS, download_url}};
use odin_actor::{errors::op_failed, prelude::*};
use odin_macro::public_struct;
use odin_wx::{WxDataSetRequest,AddDataSet,RemoveDataSet,WxFileAvailable};
use crate::{CACHE_DIR, OpenMeteoConfig, OpenMeteoMetadata, Result, data_url, errors::op_failed, get_timesteps_from_file, meta_url};


struct OpenMeteoStatus {
    meta: OpenMeteoMetadata,
    basedate: DateTime<Utc>,
    forecasts: Vec<DateTime<Utc>>,
    path: Arc<PathBuf>,
}
impl OpenMeteoStatus {
    fn new (meta: OpenMeteoMetadata, basedate: DateTime<Utc>, forecasts: Vec<DateTime<Utc>>, path: Arc<PathBuf>)->Self {
        OpenMeteoStatus { meta, basedate, forecasts, path }
    }
}

//--- our message alphabet

#[derive(Debug)]
pub struct UpdateDataSet (pub Arc<WxDataSetRequest>);

define_actor_msg_set! { pub OpenMeteoActorMsg = AddDataSet | UpdateDataSet | RemoveDataSet }

pub struct OpenMeteoActor<A> where A: DataAction<WxFileAvailable> + 'static {
    config: OpenMeteoConfig,
    file_avail_action: A,

    client: Client,
    datasets: HashMap<Arc<WxDataSetRequest>, OpenMeteoStatus>,
}

impl <A> OpenMeteoActor<A> where A: DataAction<WxFileAvailable> + 'static {
    pub fn new (config: OpenMeteoConfig, file_avail_action: A)->Self {
        OpenMeteoActor {
            config,
            file_avail_action,
            client: Client::new(),
            datasets: HashMap::new(),
        }
    }

    async fn add_dataset (&mut self, hself: ActorHandle<OpenMeteoActorMsg>, request: Arc<WxDataSetRequest>)->Result<()> {
        if let Some(status) = self.datasets.get( &request) {
            self.file_avail_action.execute( WxFileAvailable {
                request,
                basedate: status.basedate,
                forecasts: status.forecasts.clone(),
                path: status.path.clone()
            }).await;

        } else { // new request type
            let meta = self.download_meta( request.as_ref()).await?;
            let path = self.download_data( request.as_ref(), &meta).await?;

            self.schedule_update( hself, request.clone(), &meta);

            let forecasts = get_timesteps_from_file(path.as_ref())?;
            let basedate = forecasts[0];

            //let basedate = meta.base_date()?;
            //let forecasts = meta.forecasts( basedate, request.fc_duration); // TODO - we should extract it from the downloaded data (via regex)

            self.datasets.insert( request.clone(), OpenMeteoStatus::new( meta, basedate.clone(), forecasts.clone(), path.clone()));

            self.file_avail_action.execute( WxFileAvailable::new( request, basedate, forecasts, path)).await;
        }

        Ok(())
    }

    fn schedule_update (&self, hself: ActorHandle<OpenMeteoActorMsg>, req: Arc<WxDataSetRequest>, meta: &OpenMeteoMetadata)->Result<()> {
        let t_update = meta.next_update()? + self.config.initial_delay;
        let dur = (t_update - Utc::now()).to_std()?;
        if !dur.is_zero() {
            // most models update in hourly intervals so this is not worth its own timer
            info!("scheduling update for OpenMeteo dataset {:?} at {} ", req.region, t_update.with_timezone( &Local));
            if let Ok(mut scheduler) = hself.get_scheduler() {
                scheduler.schedule_once( dur, {
                    let hself = hself.clone();
                    move |_| {
                        let req = req.clone();
                        hself.try_send_msg( UpdateDataSet(req));
                    }
                })?;
            }
        }

        Ok(())
    }

    async fn update_dataset (&mut self, hself: ActorHandle<OpenMeteoActorMsg>, request: Arc<WxDataSetRequest>)->Result<()> {
        if let Some(status) = self.datasets.get(&request) { // do we still have this dataset?
            info!("updating OpenMet dataset {:?} at {}", request.region, Local::now());

            let meta = self.download_meta( request.as_ref()).await?;
            if meta != status.meta { // the underlying model got updated
                let path = self.download_data( request.as_ref(), &meta).await?;
                self.schedule_update( hself, request.clone(), &meta);

                let basedate = meta.base_date()?;
                let forecasts = meta.forecasts(basedate, request.fc_duration); // TODO - should be extracted from downloaded data

                self.datasets.insert( request.clone(), OpenMeteoStatus::new( meta, basedate, forecasts.clone(), path.clone()));
                self.file_avail_action.execute( WxFileAvailable::new( request, basedate, forecasts, path)).await;

                remove_old_files( &*CACHE_DIR, self.config.max_age);

            } else { // it hasn't been update yet - reschedule in a couple of minutes
                info!("retry OpenMet dataset {:?} in {} secs", request.region, self.config.retry_delay.as_secs());
                if let Ok(mut scheduler) = hself.get_scheduler() {
                    scheduler.schedule_once( self.config.retry_delay, {
                        let hself = hself.clone();
                        move |_| {
                            let req = request.clone();
                            hself.try_send_msg( UpdateDataSet(req));
                        }
                    })?;
                }
            }
        }

        Ok(())
    }

    fn remove_dataset (&mut self, dr: Arc<WxDataSetRequest>) {
        self.datasets.remove(&dr);
    }

    async fn download_data (&mut self, dr: &WxDataSetRequest, meta: &OpenMeteoMetadata)->Result<Arc<PathBuf>> {
        let url = data_url( &self.config, dr.query.as_str());
        info!("downloading open-meteo data: {}", url);

        let fname = odin_data_filename( dr.region.as_str(), Some(meta.base_date()?), &[dr.model_name.as_str()], Some("json"));
        let path = Arc::new( CACHE_DIR.join( &fname));

        let nbytes = if path.is_file() { // do we already have this file
            file_length( path.as_ref()).ok_or( op_failed!("no file length for {:?}", path))?
        } else {
            download_url( &self.client, &url, NO_HEADERS, path.as_ref()).await?
        };

        if nbytes > 0 {
            Ok( path )
        } else {
            Err( op_failed!("empty response") )
        }
    }

    async fn download_meta (&self, dr: &WxDataSetRequest)->Result<OpenMeteoMetadata> {
        let url = meta_url( &self.config, dr.model_name.as_str());

        let resp = self.client.get(&url).send().await?.text().await?; // response is bounded and small - no need to collect chunks
        let meta: OpenMeteoMetadata = serde_json::from_str( &resp)?;

        Ok( meta )
    }
}

impl_actor! { match msg for Actor<OpenMeteoActor<A>,OpenMeteoActorMsg> where A: DataAction<WxFileAvailable> + 'static + Sync as
    _Start_ => cont! {
        remove_old_files( &*CACHE_DIR, self.config.max_age);
    }
    AddDataSet => cont! {
        let hself = self.hself.clone();
        self.add_dataset( hself, msg.0).await;
    }
    UpdateDataSet => cont! {
        let hself = self.hself.clone();
        self.update_dataset( hself, msg.0).await;
    }
    RemoveDataSet => cont! {
        self.remove_dataset(msg.0);
    }
}
