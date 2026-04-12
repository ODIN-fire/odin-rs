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

use std::{
    collections::{HashMap,VecDeque},
    io::{BufReader, Read, Write as IOWrite},
    ops::{Deref,DerefMut},
    path::{Path,PathBuf}, sync::Arc, time::Duration,  fs::File,
};
use chrono::{DateTime,Utc,Datelike,Timelike};
use lazy_static::lazy_static;
use odin_macro::public_struct;
use serde::{Serialize,Deserialize};
use serde_json::{self, value::{Value as JsonValue}};
use reqwest::Client;
use geojson::{GeoJson,Value,FeatureCollection,Feature,Geometry};
use geo::{Polygon, MultiPolygon, Centroid, Geometry as GeoGeometry};
use strum::AsRefStr;
use uom::si::{length::meter,length::kilometer,area::hectare,f64::{Length,Area}};

use odin_common::{
    collections::RingDeque, fs::{ensure_writable_dir, odin_data_filename},
    geo::GeoPoint3, json_writer::{JsonWritable,JsonWriter, NumFormat},
    net::{NO_HEADERS, download_url}
};
use odin_dem::DemSource;
use odin_build::{pkg_cache_dir, define_load_asset, define_load_config};
use odin_server::{spa::SpaService,ws_service::ws_msg_from_json};


pub mod errors;
pub type Result<T> = errors::Result<T>;
use errors::{OdinBushfireError,op_failed};

pub mod actor;

pub mod service;
pub use service::BushfireService;

lazy_static! {
    pub static ref CACHE_DIR: PathBuf = { pkg_cache_dir!() };
}

define_load_config!{}
define_load_asset!{}

#[derive(Deserialize,Debug)]
#[public_struct]
struct BushFireConfig {
    url: String,
    dem: Option<DemSource>,
    check_interval: Duration, // how often to download & check the database for new updates
    max_history: usize, // max number of most recent updates to keep for each bushfire
    max_age: Duration, // max age of fires
    max_file_age: Duration, // duration after which to delete cache files
}

#[derive(Serialize,Debug,Clone)]
#[public_struct]
struct Bushfire {
    id: String,
    name: String,
    date: DateTime<Utc>, // the data for this record was created
    fire_type: BushfireType,
    area: Area,
    perimeter: Length,
    state: State,
    agency: String,

    position: GeoPoint3,
    filename: String
}

impl JsonWritable for Bushfire {
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object( |w| {
            w.write_field( "id", &self.id);
            w.write_field( "name", &self.name);
            w.write_date_field( "date", self.date);
            w.write_field( "fireType", &self.fire_type.as_ref());
            w.write_field( "area", &self.area.get::<hectare>());
            w.write_field( "perimeter", &self.perimeter.get::<kilometer>());
            w.write_field( "state", &self.state.as_ref());
            w.write_field( "agency", &self.agency);
            w.write_f64_field( "lon", self.position.longitude_degrees(), NumFormat::Fp3);
            w.write_f64_field( "lat", self.position.latitude_degrees(), NumFormat::Fp3);
            w.write_f64_field( "height", self.position.altitude_meters(), NumFormat::Fp0);
            w.write_field( "filename", &self.filename);
        })
    }
}

#[derive(Serialize,Debug,AsRefStr,Clone)]
pub enum BushfireType { PrescribedBurn, Bushfire, VegetationFire, CurrentBurntArea, PowerPoleFire, Unknown }

impl BushfireType {
    fn from_str( s: &str)->Self {
        match s {
            "Bushfire" => BushfireType::Bushfire,
            "Prescribed Burn" => BushfireType::PrescribedBurn,
            "VEGETATION FIRE" => BushfireType::VegetationFire,
            "Current Burnt Area" => BushfireType::CurrentBurntArea,
            "POWER POLE FIRE" => BushfireType::PowerPoleFire,
            // TODO - are there more and can we rely on case?
            _ => BushfireType::Unknown
        }
    }
}

#[derive(Serialize,Debug,AsRefStr,Clone)]
#[allow(nonstandard_style)]
pub enum State { ACT, NSW, NT, QLD, SA, TAS, VIC, WA }

impl State {
    fn from_str( s: &str)->Option<Self> {
        match s {
            "ACT" => Some(State::ACT),
            "NSW" => Some(State::NSW),
            "NT" => Some(State::NT),
            "QLD" => Some(State::QLD),
            "SA" => Some(State::SA),
            "TAS" => Some(State::TAS),
            "VIC" => Some(State::VIC),
            "WA" => Some(State::WA),
            _ => None
        }
    }
}

pub fn get_json_update_msg (bushfires: &Vec<Bushfire>)->String {
    let mut w = JsonWriter::with_capacity(4096);
    w.write_object( |w| {
        w.write_array_field("bushfires", |w| {
            for bushfire in bushfires {
                bushfire.write_json_to(w);
            }
        })
    });
    ws_msg_from_json( BushfireService::mod_path(), "update", w.as_str())
}

pub struct BushfireStore (HashMap<String,VecDeque<Bushfire>>);

impl BushfireStore {
    /// this happens infrequently from a dyn action so we don't cache the writer (but for that save the clone)
    pub fn get_json_snapshot_msg (&self)->String {
        let mut w = JsonWriter::with_capacity(8192);
        w.write_object( |w| {
            w.write_array_field("bushfires", |w| {
                for hist in self.0.values() {
                    for bushfire in hist {
                        bushfire.write_json_to(w);
                    }
                }
            })
        });
        ws_msg_from_json( BushfireService::mod_path(), "snapshot", w.as_str())
    }
}

impl Deref for BushfireStore {
    type Target = HashMap<String,VecDeque<Bushfire>>;
    fn deref (&self)->&Self::Target { &self.0 }
}
impl DerefMut for BushfireStore {
    fn deref_mut (&mut self)->&mut Self::Target { &mut self.0 }
}


pub fn snapshot_path (date: DateTime<Utc>, dir: &PathBuf)->PathBuf {
    let fname = odin_data_filename( "bushfires", Some(date), &[], Some("geojson"));
    dir.join( fname)
}

pub async fn download_file (client: &Client, url: &str, date: DateTime<Utc>)->Result<PathBuf> {
    let path = snapshot_path( date, &CACHE_DIR);
    let len = download_url( client, url, NO_HEADERS, &path).await?;

    if len > 0 {
        Ok(path)
    } else {
        Err( op_failed!("server returned empty file"))
    }
}

pub fn get_features<P> (path: P)->Result<Vec<Feature>> where P: AsRef<Path> {
    let reader = BufReader::new(File::open(path)?);

    match GeoJson::from_reader(reader)? {
        GeoJson::FeatureCollection(fc) => {
            Ok( fc.features)
        }
        _ => Err( op_failed!("expected GeoJSON FeatureCollection"))
    }
}

pub fn cleanup_feature_properties (features: &mut Vec<Feature>) {
    for feature in features {
        if let Some(props) = &mut feature.properties {
            props.remove("OBJECTID");
            props.remove("GlobalID");
            props.remove("Shape__Area");
            props.remove("Shape__Length");
        }
    }
}

pub fn get_centroid (geometry: &Geometry) -> Result<GeoPoint3> {
    let geo_val = &geometry.value;
    match *geo_val {
        Value::Polygon(_) => {
            let poly: Polygon<f64> = geo_val.try_into()?;
            Ok( GeoPoint3::from_point_alt_meters( poly.centroid().ok_or( op_failed!("invalid polygon"))?, 0.0) )
        }
        Value::MultiPolygon(_) => {
            let multi_poly: MultiPolygon<f64> = geo_val.try_into()?;
            Ok( GeoPoint3::from_point_alt_meters( multi_poly.centroid().ok_or( op_failed!("invalid multi-polygon"))?, 0.0) )
        }
        _ => {
            Err( op_failed!("unsupported geometry") )
        }
    }
}

pub fn get_bushfires<P> (features: &Vec<Feature>, dir: Option<P>, max_age: Option<Duration>)->Result<Vec<Bushfire>> where P: AsRef<Path> {
    let mut bushfires = Vec::with_capacity(features.len());
    let stale_date = if let Some(dur) = max_age { Utc::now() - dur } else { DateTime::from_timestamp(0,0).unwrap() };

    for feature in features {
        if let Some(props) = &feature.properties {
            if let Some( JsonValue::String(id) ) = props.get("fire_id")
            && let Some( JsonValue::Number(area_ha) ) = props.get( "area_ha")
            && let Some( area_ha ) = area_ha.as_f64()
            && let Some( JsonValue::Number(perim_km) ) = props.get( "perim_km")
            && let Some( perim_km ) = perim_km.as_f64()
            && let Some( JsonValue::String(state) ) = props.get("state")
            && let Some( state ) = State::from_str(&state)
            && let Some( JsonValue::String(agency) ) = props.get("agency")
            && let Some( JsonValue::Number(capt) ) = props.get("capt_date")
            && let Some (capt_i64) = capt.as_i64()
            && let Some( date ) = DateTime::from_timestamp_millis( capt_i64)
            && let Some( geo ) = &feature.geometry
            && let Ok( position ) = get_centroid( geo)
            {
                if date > stale_date {
                    let filename = format!("bushfire_{}", id);
                    let filename = odin_data_filename( &filename, Some(date), &[], Some("geojson"));

                    if let Some(dir) = &dir {
                        let path = dir.as_ref().to_path_buf().join( &filename);
                        let mut file = File::create( &path)?;
                        serde_json::to_writer( &file, feature)?;
                    }

                    let name = if let Some( JsonValue::String(name) ) = props.get("fire_name") {
                        name.clone()
                    } else {
                        "unassigned".to_string()
                    };

                    let mut fire_type = BushfireType::Unknown;
                    if let Some( JsonValue::String(ft) ) = props.get("fire_type") {
                        fire_type = BushfireType::from_str(ft);
                    }

                    let id = id.clone();
                    let area = Area::new::<hectare>(area_ha);
                    let perimeter = Length::new::<kilometer>(perim_km);
                    let agency = agency.clone();

                    let bf = Bushfire { id, name, date, fire_type, area, perimeter, state, agency, position, filename };
                    bushfires.push( bf)
                }
            }
        } else {
            eprintln!("ignoring bushfire feature object without properties");
        }
    }

    Ok( bushfires )
}

pub async fn fill_in_position_heights (fires: &mut Vec<Bushfire>, dem: &DemSource) -> Result<()> {
    let ps: Vec<(f64, f64)> = fires
        .iter()
        .map(|f| (f.position.longitude_degrees(), f.position.latitude_degrees()))
        .collect();

    let heights = dem.get_heights(Some(0.0), &ps).await?;

    for i in 0..ps.len() {
        let fire = &mut fires[i];
        let pos = &mut fire.position;
        pos.set_altitude_meters(heights[i]);
    }
    Ok(())
}
