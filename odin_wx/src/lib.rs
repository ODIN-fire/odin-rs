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

use std::{hash::{Hash,Hasher}, sync::Arc, time::Duration, path::{Path,PathBuf}};
use serde::{Serialize,Deserialize};
use chrono::{DateTime,Utc};

use odin_actor;
use odin_common::geo::GeoRect;

pub mod errors;
pub type Result<T> = errors::Result<T>;

/// abstraction for a weather forecasting service within ODIN
/// this has to be dyn compatible since it has to support heterogenous sets
/// the trait impl also has to satisfy the Send + Sync + 'static bounds since it is stored in an actor
/// note that a single service (such as 'openmet') can support mutiple wx models (e.g. 'ecmwf-ifs')
pub trait WxService: Send + Sync + 'static {
    fn wx_name(&self)->Arc<String>; // of the service (e.g. 'hrrr' or 'openmet')
    fn model_name(&self)->Arc<String>; // e.g. 'hrrr' or 'ec-ifs'
    fn dataset_name(&self)->Arc<String>; // e.g. 'basic'

    // these imply that implementors store ActorHandle<M> fields
    fn try_send_add_dataset (&self, req: Arc<WxDataSetRequest>)->odin_actor::Result<()>;
    fn try_send_remove_dataset (&self, req: Arc<WxDataSetRequest>)->odin_actor::Result<()>;

    fn create_request (&self, region: Arc<String>, bbox: GeoRect, fc_duration: Duration)->WxDataSetRequest;
    fn matches_request (&self, request: &WxDataSetRequest)->bool {
        self.wx_name() == request.wx_name && self.model_name() == request.model_name && self.dataset_name() == request.dataset_name
    }

    // if wx file contains timesteps break it up into per-timestep gridded datasets ala HRRR and add respective band meta info
    // note this can fail if data sets don't contain the required fields
    fn to_wx_grids (&self, fa: &WxFileAvailable)->Result<Vec<Arc<PathBuf>>>;
}

pub type WxServiceList = Vec<Box<dyn WxService>>;

/// the struct we use to define data sets we want to retrieve from a wx model
/// while this is normally created on demand this could also come from a config file
/// we use Arcs so that cloning is inexpensive and we don't end up with tons of duplicated strings
#[derive(Clone,Serialize,Deserialize,Debug)]
pub struct WxDataSetRequest {
    /// name of the region we retrieve data for
    pub region: Arc<String>,

    /// the bounding box of the region
    pub bbox: GeoRect,

    /// the type name of the WxService that was used to create this request
    pub wx_name: Arc<String>,

    /// the underlying model name (e.g. 'hrrr', 'ec-ifs', 'gfs', 'icon')
    pub model_name: Arc<String>,

    /// this is a name for the field set/use of the data we retrieve (e.g. 'basic')
    pub dataset_name: Arc<String>,

    // the duration for which we request forecasts
    pub fc_duration: Duration,

    /// the canonical, invariant, model specific query that is used to identify/hash this request
    /// this doubles as the unique identifier for a WxDataSetRequest since it defines the data we retrieve
    /// since this getc computed from service specific fields, the bbox and the fc_duration it has to be
    /// computed by the WxService
    pub query: String
}

/* #region hash trait imple for WxDataSetRequest ***********************************************/

impl WxDataSetRequest {
    pub fn new (region: Arc<String>, bbox: GeoRect, wx_name: Arc<String>, model_name: Arc<String>, dataset_name: Arc<String>, fc_duration: Duration, query: String)->Self {
        WxDataSetRequest { region, bbox, wx_name, model_name, dataset_name, fc_duration, query }
    }
}

impl Hash for WxDataSetRequest {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.query.hash(state);
    }
}

impl PartialEq for WxDataSetRequest {
    fn eq(&self, other: &Self) -> bool {
        self.query == other.query
    }
}

impl Eq for WxDataSetRequest {}

/* #end region  ********************************************************************************/

/* #region public messages *********************************************************************/

/// this is the message clients send to request start of updates
#[derive(Debug)]
pub struct AddDataSet (pub Arc<WxDataSetRequest>);

/// this is the message clients send to terminate requests
#[derive(Debug)]
pub struct RemoveDataSet (pub Arc<WxDataSetRequest>);


/// the message sent from service (actions) to clients when a new data sets becomes available
/// note that clients can use the provided request to check if it was theirs
#[derive(Debug,Clone)]
pub struct WxFileAvailable {
    // TODO - do we need the service/model here?

    /// the fields/area this forecast file covers (can be used to match/filter by client)
    pub request: Arc<WxDataSetRequest>,

    /// the model update time this forecast file is based on
    pub basedate: DateTime<Utc>,

    /// forecast times (steps) covered in this file
    pub forecasts: Vec<DateTime<Utc>>,

    /// the path where the wx data was stored
    pub path: Arc<PathBuf>,
}

impl WxFileAvailable {
    pub fn new (request: Arc<WxDataSetRequest>, basedate: DateTime<Utc>, forecasts: Vec<DateTime<Utc>>, path: Arc<PathBuf>)->Self {
        WxFileAvailable { request, basedate, forecasts, path }
    }
}

/* #end region  ********************************************************************************/
