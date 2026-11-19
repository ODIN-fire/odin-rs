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

//! the store of available product time steps per station

use std::{collections::VecDeque, path::{Path,PathBuf}};
use chrono::{DateTime, Utc};

use crate::{
    NexradConfig, NexradEntry, NexradProduct, Station, entry_path, parse_date_tag,
    product::{MAX_RANGE_KM, station_extent},
    mosaic::{NexradMosaic, MosaicUpdate},
};

/// the time steps of a product (oldest first)
#[derive(Debug)]
pub struct ProductEntries {
    pub product: NexradProduct,
    pub entries: VecDeque<NexradEntry>,
}

impl ProductEntries {
    pub fn latest (&self)->Option<&NexradEntry> { self.entries.back() }

    pub fn json_value (&self)->serde_json::Value {
        let entries: Vec<serde_json::Value> = self.entries.iter().rev().map( |e| e.json_value()).collect(); // newest first
        serde_json::json!({ "name": self.product.name(), "entries": entries })
    }
}

/// the products of a station
#[derive(Debug)]
pub struct StationEntries {
    pub station: Station,
    pub products: Vec<ProductEntries>,
}

impl StationEntries {
    pub fn product (&self, product: NexradProduct)->Option<&ProductEntries> {
        self.products.iter().find( |p| p.product == product)
    }

    /// scan time of the latest product time step
    pub fn latest_scan (&self)->Option<DateTime<Utc>> {
        self.products.iter().filter_map( |p| p.latest()).map( |e| e.scan_time).max()
    }

    pub fn has_scan (&self, scan_time: &DateTime<Utc>)->bool {
        self.products.iter().any( |p| p.entries.iter().any( |e| e.scan_time == *scan_time))
    }

    pub fn json_value (&self)->serde_json::Value {
        let s = &self.station;
        let extent = station_extent( s, MAX_RANGE_KM).ok();
        let products: Vec<serde_json::Value> = self.products.iter().map( |p| p.json_value()).collect();
        serde_json::json!({ "id": s.id, "name": s.name, "lat": s.latitude, "lon": s.longitude, "extent": extent, "products": products })
    }
}

/// a change of the store - new time steps of a station and the ones that were dropped because of it
#[derive(Debug,Clone)]
pub struct NexradUpdate {
    pub station: String,
    pub added: Vec<NexradEntry>,
    pub removed: Vec<NexradEntry>,
}

impl NexradUpdate {
    pub fn new (station: &str)->Self { NexradUpdate { station: station.to_string(), added: Vec::new(), removed: Vec::new() } }

    pub fn is_empty (&self)->bool { self.added.is_empty() && self.removed.is_empty() }

    pub fn json_value (&self)->serde_json::Value {
        let added: Vec<serde_json::Value> = self.added.iter().map( |e| {
            serde_json::json!({ "product": e.product.name(), "date": e.scan_time.timestamp_millis(), "layer": e.layer_name() })
        }).collect();
        let removed: Vec<String> = self.removed.iter().map( |e| e.layer_name()).collect();
        serde_json::json!({ "station": self.station, "added": added, "removed": removed })
    }
}

/// the product time steps of all configured stations
#[derive(Debug)]
pub struct NexradStore {
    pub max_entries: usize,
    pub stations: Vec<StationEntries>,
    pub mosaics: Vec<NexradMosaic>, // the mosaics (overview time steps) of all products, oldest first (if configured)
}

impl NexradStore {
    /// an empty store for the given stations and products
    pub fn new (stations: Vec<Station>, products: &[NexradProduct], max_entries: usize)->Self {
        let stations = stations.into_iter().map( |station| StationEntries {
            station,
            products: products.iter().map( |p| ProductEntries { product: *p, entries: VecDeque::new() }).collect()
        }).collect();
        NexradStore { max_entries: max_entries.max(1), stations, mosaics: Vec::new() }
    }

    pub fn station (&self, id: &str)->Option<&StationEntries> {
        self.stations.iter().find( |s| s.station.id == id)
    }

    pub fn entries (&self)->impl Iterator<Item=&NexradEntry> {
        self.stations.iter().flat_map( |s| s.products.iter().flat_map( |p| p.entries.iter()))
    }

    /// scan times of all time steps of a station (any product)
    pub fn scan_times (&self, id: &str)->Vec<DateTime<Utc>> {
        let mut times: Vec<DateTime<Utc>> = self.station( id).map( |s| s.products.iter().flat_map( |p| p.entries.iter().map( |e| e.scan_time)).collect()).unwrap_or_default();
        times.sort();
        times.dedup();
        times
    }

    /// insert entries (in scan time order), dropping the oldest entries above max_entries per product. Entries we
    /// already have or that are for unknown stations/products are ignored. Note the returned update can include new
    /// entries in `removed` if they are older than all max_entries entries
    pub fn insert (&mut self, station: &str, entries: Vec<NexradEntry>)->NexradUpdate {
        let max_entries = self.max_entries;
        let mut update = NexradUpdate::new( station);

        if let Some(se) = self.stations.iter_mut().find( |s| s.station.id == station) {
            for entry in entries {
                if entry.station != station { continue }
                if let Some(pe) = se.products.iter_mut().find( |p| p.product == entry.product) {
                    if pe.entries.iter().any( |e| e.scan_time == entry.scan_time) { continue }

                    let idx = pe.entries.iter().position( |e| e.scan_time > entry.scan_time).unwrap_or( pe.entries.len());
                    pe.entries.insert( idx, entry.clone());
                    update.added.push( entry);
                    while pe.entries.len() > max_entries {
                        if let Some(e) = pe.entries.pop_front() { update.removed.push(e) }
                    }
                }
            }
        }
        update.added.retain( |e| !update.removed.contains( e));
        update
    }

    /// add a new mosaic (replacing one of the same product and date), dropping the oldest ones of the same product above
    /// max_mosaics. Mosaics are added in date order
    pub fn add_mosaic (&mut self, mosaic: NexradMosaic, max_mosaics: usize)->MosaicUpdate {
        self.mosaics.retain( |m| !(m.product == mosaic.product && m.date == mosaic.date));
        self.mosaics.push( mosaic.clone());

        let n = self.mosaics.iter().filter( |m| m.product == mosaic.product).count();
        let mut n_drop = n.saturating_sub( max_mosaics.max(1));
        let mut removed = Vec::new();
        self.mosaics.retain( |m| {
            if n_drop > 0 && m.product == mosaic.product { n_drop -= 1; removed.push( m.clone()); false } else { true }
        });
        MosaicUpdate { added: mosaic, removed }
    }

    /// the mosaics of a product (oldest first)
    pub fn product_mosaics (&self, product: NexradProduct)->impl Iterator<Item=&NexradMosaic> {
        self.mosaics.iter().filter( move |m| m.product == product)
    }

    pub fn json_value (&self)->serde_json::Value {
        let products: Vec<serde_json::Value> = self.stations.first().map( |s| s.products.iter().map( |p| {
            serde_json::json!({ "name": p.product.name(), "title": p.product.title() })
        }).collect()).unwrap_or_default();
        let stations: Vec<serde_json::Value> = self.stations.iter().map( |s| s.json_value()).collect();
        let mosaics: Vec<serde_json::Value> = self.mosaics.iter().map( |m| m.json_value()).collect();
        serde_json::json!({ "products": products, "stations": stations, "mosaics": mosaics })
    }
}

/// rebuild the store from product files in our cache (e.g. after restart). Files that are not needed anymore (older
/// than the max_downloads newest ones of a product, unknown or incomplete files) are removed
pub fn load_cached_store (stations: Vec<Station>, products: &[NexradProduct], max_entries: usize, cache_dir: &Path)->NexradStore {
    let mut store = NexradStore::new( stations, products, max_entries);
    let ids: Vec<String> = store.stations.iter().map( |s| s.station.id.clone()).collect();

    for id in &ids {
        for product in products {
            let dir = cache_dir.join( id).join( product.name());
            let Ok(rd) = std::fs::read_dir( &dir) else { continue };
            let prefix = format!("{id}_{}_", product.name());

            for de in rd.flatten() {
                let path = de.path();
                let scan_time = path.file_name().and_then( |f| f.to_str())
                    .and_then( |f| f.strip_prefix( &prefix)).and_then( |f| f.strip_suffix(".tif"))
                    .and_then( parse_date_tag);

                match scan_time {
                    Some(scan_time) if path.is_file() => {
                        let update = store.insert( id, vec![ NexradEntry{ station: id.clone(), product: *product, scan_time, path }]);
                        remove_files( &update.removed);
                    }
                    _ => { let _ = std::fs::remove_file( &path); } // tmp file or foreign file
                }
            }
        }
    }
    store
}

/// remove the product files of entries
pub fn remove_files (entries: &[NexradEntry]) {
    for e in entries {
        if let Err(err) = std::fs::remove_file( &e.path) {
            if err.kind() != std::io::ErrorKind::NotFound { eprintln!("failed to remove {:?}: {err}", e.path) }
        }
    }
}
