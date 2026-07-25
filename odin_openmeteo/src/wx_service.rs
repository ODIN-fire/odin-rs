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

use crate::{CACHE_DIR, OpenMeteoDataSet, actor::OpenMeteoActorMsg, convert, dataset::BasicEcmwfIfsData, get_fc_days, ids::{FieldId,ModelId}, url_query_hourly_bbox};

/// underlying OpenMeteo implementation struct for abstract WxService interface
pub struct OpenMeteoService {
    wx_name: Arc<String>,
    model_name: Arc<String>,
    dataset_name: Arc<String>,

    fields_query: String, // the invariant non-region part of the query (computed from fields and levels)
    hself: ActorHandle<OpenMeteoActorMsg>
}

impl OpenMeteoService {
    pub fn new (model: ModelId, dataset_name: Arc<String>, hself: ActorHandle<OpenMeteoActorMsg>)->Self {
        let wx_name = Arc::new( type_name::<Self>().to_string());
        let model_name = Arc::new( model.as_ref().to_string());
        let fields_query = BasicEcmwfIfsData::fields_query().to_string();

        OpenMeteoService{wx_name, model_name, dataset_name, fields_query, hself}
    }

    pub fn new_basic_ifs (hself: ActorHandle<OpenMeteoActorMsg>)->Self {
        let dataset_name = Arc::new(BasicEcmwfIfsData::dataset_name().to_string());

        Self::new( ModelId::ecmwf_ifs, dataset_name, hself)
    }
}

impl WxService for OpenMeteoService {
    fn wx_name(&self) -> Arc<String> {
        self.wx_name.clone()
    }

    fn model_name (&self)->Arc<String> {
        self.model_name.clone()
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
        let now = Utc::now();
        let fc_days = get_fc_days( &now, fc_duration);
        let query = url_query_hourly_bbox::<BasicEcmwfIfsData>(&bbox, fc_days, 0, self.model_name.as_str());
        let wx_name = self.wx_name.clone();
        let model_name = self.model_name.clone();
        let dataset_name = self.dataset_name.clone();
        WxDataSetRequest { region, bbox, wx_name, model_name, dataset_name, fc_duration, query }
    }

    fn to_wx_grids (&self, fa: &WxFileAvailable)->odin_wx::Result<Vec<Arc<PathBuf>>> {
        convert::basic_ecmwf_ifs_to_hrrr( &fa.request.as_ref(), fa.path.as_ref(), CACHE_DIR.as_path()).map_err(|e|
            odin_wx::errors::OdinWxError::OpFailedError(format!("cannot convert to HRRR compliant grids: {}", e)))
    }
}
