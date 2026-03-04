/*
 * Copyright © 2024, United States Government, as represented by the Administrator of
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

use std::sync::Arc;
use odin_common::{define_cli, geo::GeoRect, datetime::{hours}, vec_boxed};
use odin_actor::prelude::*;
use odin_wx::{WxService,WxFileAvailable};
use odin_hrrr::{load_config, HrrrService, HrrrActor, HrrrConfig, schedule::{HrrrSchedules,get_hrrr_schedules}};

define_cli! { ARGS [about="NOAA HRRR download example using HrrrActor"] =
    hrrr_config: String [help="filename of HRRR config file", long,default_value="hrrr_conus-8.ron"],
    statistic_schedules: bool [help="compute schedules of available forecast files from server dir listing", long],

    region: String [help="name of geo region to retrieve wx forecasts for"],
    bbox: Vec<f64> [help="WSEN bounding box for grid", allow_hyphen_values=true, num_args=4]
}

define_actor_msg_set! { HrrrMonitorMsg = WxFileAvailable }
struct HrrrMonitor {
    wxs: Vec<Box<dyn WxService>>,
    region: Arc<String>,
    bbox: GeoRect
}

impl HrrrMonitor {
    fn new (wxs: Vec<Box<dyn WxService>>)->Self {
        let region = Arc::new(ARGS.region.clone());
        let bbox = GeoRect::from_wsen_degrees( ARGS.bbox[0], ARGS.bbox[1], ARGS.bbox[2], ARGS.bbox[3]);
        HrrrMonitor{wxs,region,bbox}
    }
}

impl_actor! { match msg for Actor<HrrrMonitor,HrrrMonitorMsg> as
    _Start_ => cont! {
        for wx in &self.wxs {
            let req = wx.create_request( self.region.clone(), self.bbox.clone(), hours(2));
            if wx.try_send_add_dataset( Arc::new(req)).is_err() {
                error!("failed to send WxDataSetRequest")
            }
        }
    }
    WxFileAvailable => cont! {
        println!("HrrrMonitor got WxFileAvailable: {:?} for {:?}", msg.path, msg.forecasts )
    }
}

run_actor_system!( actor_system => {
    let pre_hmon = PreActorHandle::new( &actor_system, "monitor", 8);

    let hrrr_config: HrrrConfig = load_config( "hrrr_conus-1.ron")?;
    let schedules: HrrrSchedules = get_hrrr_schedules( &hrrr_config, ARGS.statistic_schedules).await?;

    let himporter = spawn_actor!( actor_system, "hrrr_importer", HrrrActor::new(
        hrrr_config,
        schedules,
        data_action!( let hmon: ActorHandle<HrrrMonitorMsg> = pre_hmon.to_actor_handle() => |data: WxFileAvailable| {
           hmon.send_msg( data.clone()).await?;
           Ok(())
        })
    ))?;

    let wx_services: Vec<Box<dyn WxService>> = vec_boxed![ HrrrService::new_basic( himporter) ];
    let _hmon = spawn_pre_actor!( actor_system, pre_hmon, HrrrMonitor::new( wx_services))?;

    Ok(())
});
