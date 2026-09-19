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


use std::{sync::Arc, any::type_name, time::Duration, path::{Path,PathBuf}};
use chrono::{Utc, DateTime};

use odin_actor::{ActorHandle};
use odin_wx::{AddDataSet, RemoveDataSet, WxDataSetRequest, WxFileAvailable, WxService};
use odin_common::{geo::GeoRect, datetime::as_days};

use crate::{
    CACHE_DIR, HourlyLocationData, LocationData, OpenMeteoConfig, DataSet, actor::OpenMeteoActorMsg, convert, dataset::BasicData, ids::{FieldId,ModelId}
};

/// underlying OpenMeteo implementation struct for abstract WxService interface
pub struct OpenMeteoService {
    wx_name: Arc<String>,
    model: Arc<String>,
    dataset_name: Arc<String>,
    hself: ActorHandle<OpenMeteoActorMsg>
}

impl OpenMeteoService {
    pub fn new_basic_ifs (hself: ActorHandle<OpenMeteoActorMsg>)->Self {
        OpenMeteoService {
            wx_name: Arc::new( type_name::<Self>().to_string()),
            model: Arc::new(ModelId::ecmwf_ifs.as_ref().to_string()),
            dataset_name: Arc::new(BasicData::dataset_name().to_string()),
            hself
        }
    }
}

impl WxService for OpenMeteoService {
    fn wx_name(&self) -> Arc<String> {
        self.wx_name.clone()
    }

    fn model_name (&self)->Arc<String> {
        self.model.clone()
    }

    fn dataset_name(&self)->Arc<String> {
        self.dataset_name.clone()
    }

    fn try_send_add_dataset(&self,req: Arc<WxDataSetRequest>) -> odin_actor::Result<()> {
        self.hself.try_send_msg( AddDataSet(req))
    }

    fn try_send_remove_dataset(&self,req: Arc<WxDataSetRequest>) -> odin_actor::Result<()> {
        self.hself.try_send_msg( RemoveDataSet(req))
    }

    fn create_request (&self, region: Arc<String>, bbox: GeoRect, fc_duration: Duration)->WxDataSetRequest {
        let start_date = Utc::now();
        let end_date = start_date + fc_duration;

        let query = HourlyLocationData::<BasicData>::get_bbox_query( ModelId::ecmwf_ifs, &start_date, &end_date, &bbox);

        let wx_name = self.wx_name.clone();
        let model_name = self.model.clone();
        let dataset_name = self.dataset_name.clone();
        WxDataSetRequest { region, bbox, wx_name, model_name, dataset_name, fc_duration, query }
    }

    fn to_wx_grids (&self, fa: &WxFileAvailable)->odin_wx::Result<Vec<Arc<PathBuf>>> {
        let req = &fa.request.as_ref();

        convert::wn_path_to_hrrr::<_,_,BasicData>( &req.region.as_ref(), &req.model_name.as_ref(), &&req.bbox, fa.path.as_ref(), CACHE_DIR.as_path()).map_err(|e|
            odin_wx::errors::OdinWxError::OpFailedError(format!("cannot convert to HRRR compliant grids: {}", e)))
    }
}
