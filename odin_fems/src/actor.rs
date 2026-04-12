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

use std::collections::{HashMap, VecDeque};
use reqwest::Client;
use chrono::{DateTime,Utc,Datelike,Timelike};

use odin_actor::prelude::*;
use odin_actor::{error,debug,warn,info};
use odin_common::{collections::RingDeque,fs::remove_old_files, datetime::{hours, minutes, utc_now}};
use uom::si::time::minute;

use crate::{update_station_nfdrs_obs, update_station_weather_obs};
use crate::{
    CACHE_DIR, FemsConfig, FemsStation, FemsStore, get_stations, errors::{Result}
};

#[derive(Debug)] pub struct ExecSnapshotAction( pub DynDataRefAction<FemsStore> );

define_actor_msg_set! { pub FemsActorMsg = ExecSnapshotAction }

pub struct FemsActor <I,U>
    where I: DataRefAction<FemsStore>,  U: DataRefAction<FemsStation>
{
    config: FemsConfig,
    store: FemsStore,       // our internal store

    init_action: I,             // initialized interaction (triggered by self)
    update_action: U,           // update interactions (triggered by self)

    client: Client,
    n_updates: usize,
    timer: Option<AbortHandle>,
}

impl <I,U> FemsActor <I,U>
    where I: DataRefAction<FemsStore>,  U: DataRefAction<FemsStation>
{
    pub fn new (config: FemsConfig, init_action: I, update_action: U)->Self {
        let store: FemsStore = FemsStore(HashMap::new());
        let client = Client::new();
        FemsActor{ config, store, init_action, update_action, client, n_updates: 0, timer: None }
    }

    async fn initialize (&mut self, hself: ActorHandle<FemsActorMsg>)->Result<()> {
        // do initial update and slot exec here
        self.store = get_stations( &self.client, &self.config).await?;
        self.init_action.execute( &self.store).await;

        // periodically check (we use polling instead of a direct schedule since we might monitor an open number of stations with retry)
        if let Ok(timer) = hself.start_repeat_timer( 1, self.config.check_interval, false) {
            self.timer = Some(timer);
        } else { error!("failed to start Fems timer") }

        Ok(())
    }

    async fn update (&mut self)->Result<()> {
        remove_old_files( &*CACHE_DIR, self.config.max_file_age);

        let now = utc_now();

        for (id,station) in self.store.iter_mut() {
            let (last_date, start) = if let Some(last_date) = station.obs_date() {
                ( last_date, (last_date + station.tx_frequency).with_second(0).unwrap() )
            } else { // we don't have an observation yet - compute the last scheduled date
                let mut sched_date = now.with_minute(station.tx_time.minute()).unwrap().with_second(0).unwrap();
                if sched_date > now { sched_date = sched_date - station.tx_frequency; }
                (DateTime::<Utc>::MIN_UTC, sched_date)
            };

            if now >= start + self.config.tx_delay {
                // TODO - we could also check for position updates in the station metadata in case this is a mobile station
                // (we could get the position from weather_obs)

                let start = start - station.tx_frequency; // make sure we get at least one observation
                match update_station_weather_obs( &self.client, &self.config, station, start).await {
                    Ok(()) => {
                        let date = station.obs_date().unwrap_or( DateTime::<Utc>::MIN_UTC);
                        if date > last_date {
                            if let Err(e) = update_station_nfdrs_obs( &self.client, &self.config, station, start).await {
                                eprintln!("error updating NFDRS data for station {}: {}", station.id, e);
                            }

                            if let Err(e) = self.update_action.execute( station).await {
                                eprintln!("failed to execute update action for station {}: {:?}", station.id, e);
                            }
                        } else { // this might still update station but we don't report it as there is no new observation
                            //println!("   @@@ no new weather obs for station {}: {} ? {}", station.id, date, last_date );
                        }
                    }
                    Err(e) => {
                        eprintln!("error updating weather data for station {}: {}", station.id, e);
                    }
                }
            }
        }

        Ok(())
    }
}

impl_actor! { match msg for Actor<FemsActor<I,U>, FemsActorMsg>
    where I: DataRefAction<FemsStore> + Sync,  U: DataRefAction<FemsStation> + Sync
    as

    ExecSnapshotAction => cont! {
        msg.0.execute( &self.store).await;
    }

    _Start_ => cont! {
        let hself = self.hself.clone();
        self.initialize(hself).await;
    }

    _Timer_ => cont! {
        self.update().await;
    }
}
