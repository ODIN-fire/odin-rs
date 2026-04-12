
#![allow(unused)]

use anyhow::Result;
use chrono::Utc;
use odin_actor::prelude::*;

use odin_fems::{load_config, FemsStore,FemsStation, actor::FemsActor};

/*
//--- example client actor
#[derive(Debug)] pub struct Init(String);
#[derive(Debug)] pub struct Update(String);

define_actor_msg_set! { FemsMonitorMsg = Init | Update }
struct FemsMonitor {}

impl_actor! { match msg for Actor<FemsMonitor,FemsMonitorMsg> as
    Init => cont! {
        println!("------------------------------ init {}", Utc::now().time());
        println!("{}", msg.0)
    }
    Update => cont! {
        println!("------------------------------ update {}", Utc::now().time());
        println!("{}", msg.0)
    }
}
*/

run_actor_system!( actor_system => {
    //let hmonitor = spawn_actor!( actor_system, "monitor", FemsMonitor{})?;

    let _hfems = spawn_actor!( actor_system, "fems", FemsActor::new(
        load_config("fems.ron")?,
        dataref_action!( => |store: &FemsStore| {
            println!("\n-------------- init at {}", Utc::now());
            println!("{}", store.get_json_snapshot_msg());

            Ok(())
        }),
        dataref_action!( => |station: &FemsStation| {
            println!("\n-------------- update at {}", Utc::now());
            println!("{}", station.get_json_update_msg());

            Ok(())
        })
    ))?;

    Ok(())
});
