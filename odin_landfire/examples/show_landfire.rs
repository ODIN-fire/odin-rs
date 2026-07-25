#![allow(unused)]

use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_cesium::{self,ImgLayerService};
use odin_landfire::{self,service::LandfireService};

run_actor_system!( actor_system => {
    let lf_config = odin_landfire::load_config("landfire.ron")?;

    spawn_actor!( actor_system, "spa_server", SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "show_landfire",
        SpaServiceList::new()
            .add( build_service!( => ImgLayerService::new())) // this automatically includes Cesium and UI services
            .add( build_service!( => LandfireService::new( lf_config)))
    ));

    Ok(())
});
