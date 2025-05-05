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

/// example application to run/use OrbitalHotspotActors

use odin_actor::prelude::*;
use odin_common::define_cli;
use odin_server::prelude::*;
use odin_share::prelude::*;
use odin_orbital::{
    init_orbital_data, load_config,
    actor::spawn_orbital_hotspot_actors,
    hotspot_service::{HotspotSat, OrbitalHotspotService}
};

define_cli! { ARGS [about="show overpasses and hotspots for given satellites"] =
    region: String [help="filename of region", short, long, default_value="conus.ron"],
    sat_infos: Vec<String> [help="filenames of OrbitalSatelliteInfo configs"]
}

run_actor_system!( actor_system => {
    // make sure our orbit calculation uses up-to-date ephemeris
    init_orbital_data()?;

    // we need to pre-instantiate a server handle since it is used as input for the other actors
    let pre_server = PreActorHandle::new( &actor_system, "server", 64);

    // spawn a shared store actor so that we can share areas of interest with other users
    let hshare = spawn_server_share_actor(&mut actor_system, "share", pre_server.to_actor_handle(), default_shared_items(), false)?;

    // the macro region to calculate overpasses for
    let region = load_config( &ARGS.region)?;

    // spawn N OrbitalHotspotActors feeding into a single SpaServer actor
    let sats: Vec<&str> = ARGS.sat_infos.iter().map(|s| s.as_str()).collect();
    let orbital_sats = spawn_orbital_hotspot_actors( &mut actor_system, pre_server.to_actor_handle(), region, &sats)?;

    // and finally spawn the SpaServer actor with a OrbitalHotspotService
    let hserver = spawn_pre_actor!( actor_system, pre_server, SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "orbital_hotspots",
        SpaServiceList::new()
            .add( build_service!( => OrbitalHotspotService::new( orbital_sats) ))
            .add( build_service!( let hshare = hshare.clone() => ShareService::new( hshare)) )
    ))?;

    Ok(())
});
