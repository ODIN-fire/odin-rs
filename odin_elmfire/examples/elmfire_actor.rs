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

use std::{time::Duration, sync::Arc};

use anyhow::Result;
use chrono::{DateTime, Utc};
use uom::si::{f64::Length,length::{kilometer, meter}};
use odin_actor::prelude::*;

use odin_common::geo::GeoPoint;
use odin_elmfire::{ElmfireConfig, ElmFire, ElmfireRun, actor::ElmfireActor, fixed::{get_fixed_steps,fixed_src_from_steps}, load_config};

/// to see info/debug run with "ODIN_TRACE=1 RUST_LOG=info ..." environment set

run_actor_system!( actor_system => {
    info!("loading Elmfire config..");
    let config: ElmfireConfig = load_config("elmfire.ron")?;
    let bbox_size = Length::new::<meter>( config.bbox_size_m as f64);
    info!("loaded.");

    let hef = spawn_actor!( actor_system, "elmfire", ElmfireActor::new(
        config,
        dataref_action!( let hsys: Arc<ActorSystemHandle> = actor_system.clone_handle() => |run: &ElmfireRun| {
            println!("\n-------------- update at {}", Utc::now());
            let msg = run.get_json_update_msg();
            println!("{}", msg);

            if run.result.is_some() { // we are done
                info!("we are done, request termination.");
                hsys.request_termination( Duration::from_secs(1)).await;
            }

            Ok(())
        }),
        no_dataref_action() // we don't remove elmfire simulation runs
    ))?;

    let n_hours = 5;
    let fixed_steps = get_fixed_steps(
        n_hours+1,
        &[ 20.0 ], // ws_mph,
        &[ 0.0, 0.0, 0.0, 22.5, 45.0 ], //wd_deg,
        &[ 2.0 ], // m1
        &[ 3.0 ], // m10,
        &[ 4.0 ], // m100,
        &[ 30.0 ], // lh,
        &[ 60.0 ], // lw
        &[ 10.0 ], // tcc
        &[ 90.0 ], // t2
    )?;
    let fixed = fixed_src_from_steps( &fixed_steps);

    info!("creating ElmFire request");
    let fire = ElmFire::from_point_ignition(
        "test3".to_string(),
        DateTime::parse_from_rfc3339("2026-07-13T21:00:00Z")?.to_utc(),
        GeoPoint::from_lon_lat_degrees(-120.2810, 37.4400),
        bbox_size,
        n_hours as u32,
        false, // we don't use microgrid wind
        Some(fixed),
        None,
        None
    )?;

    info!("now sending ElmFire request to elmfire_actor");

    hef.try_send_msg( fire)?;

    Ok(())
});
