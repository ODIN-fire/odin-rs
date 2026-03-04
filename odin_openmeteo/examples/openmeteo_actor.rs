use std::sync::Arc;
use odin_common::{define_cli, geo::GeoRect, datetime::{hours}, vec_boxed};
use odin_actor::prelude::*;
use odin_wx::{WxService,WxFileAvailable};
use odin_openmeteo::{load_config, OpenMeteoService, actor::OpenMeteoActor, OpenMeteoConfig};

define_cli! { ARGS [about="OpenMeteo download example using OpenMeteoActor"] =
    region: String [help="name of geo region to retrieve wx forecasts for"],
    bbox: Vec<f64> [help="WSEN bounding box for grid", allow_hyphen_values=true, num_args=4]
}

define_actor_msg_set! { OpenMeteoMonitorMsg = WxFileAvailable }
struct OpenMeteoMonitor {
    wxs: Vec<Box<dyn WxService>>,
    region: Arc<String>,
    bbox: GeoRect
}

impl OpenMeteoMonitor {
    fn new (wxs: Vec<Box<dyn WxService>>)->Self {
        let region = Arc::new(ARGS.region.clone());
        let bbox = GeoRect::from_wsen_degrees( ARGS.bbox[0], ARGS.bbox[1], ARGS.bbox[2], ARGS.bbox[3]);
        OpenMeteoMonitor{wxs,region,bbox}
    }
}

impl_actor! { match msg for Actor<OpenMeteoMonitor,OpenMeteoMonitorMsg> as
    _Start_ => cont! {
        for wx in &self.wxs {
            let req = wx.create_request( self.region.clone(), self.bbox.clone(), hours(6)); // will retrieve at least 1 day
            if wx.try_send_add_dataset( Arc::new(req)).is_err() {
                error!("failed to send WxDataSetRequest")
            }
        }
    }
    WxFileAvailable => cont! {
        println!("== monitor got WxFileAvailable: {:?} for {:?}", msg.path, msg.forecasts );

        // example of how to post process wx file
        if let Some(wx) = self.wxs.iter().find( |wx| wx.matches_request( msg.request.as_ref())) {
            match wx.to_wx_grids( &msg) {
                Ok(paths) => {
                    for p in paths.iter() {
                        println!("  created HRRR compliant timestep grid: {:?}", p);
                    }
                }
                Err(e) => error!("could not convert wx file to HRRR compliant grids: {}", e)
            }
        }
    }
}

run_actor_system!( actor_system => {
    let pre_hmon = PreActorHandle::new( &actor_system, "monitor", 8);

    let config: OpenMeteoConfig = load_config( "openmeteo.ron")?;
    let himporter = spawn_actor!( actor_system, "importer", OpenMeteoActor::new(
        config,
        data_action!( let hmon: ActorHandle<OpenMeteoMonitorMsg> = pre_hmon.to_actor_handle() => |data: WxFileAvailable| {
           hmon.send_msg( data.clone()).await?;
           Ok(())
        })
    ))?;

    let wx_services: Vec<Box<dyn WxService>> = vec_boxed![ OpenMeteoService::new_basic_ifs( himporter) ];
    let _hmon = spawn_pre_actor!( actor_system, pre_hmon, OpenMeteoMonitor::new( wx_services))?;

    Ok(())
});
