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

#![allow(unused)]

use std::{path::Path,fs::File,io::Write};
use lazy_static::lazy_static;
use serde::{Serialize,Deserialize};
use ron;
use bit_set::BitSet;
use odin_macro::public_struct;
use odin_build::pkg_data_dir;
use crate::errors::{OdinLandfireError,op_failed, Result};

lazy_static! {
    pub static ref EVT_CATEGORIES: EvtCategories = { EvtCategories::load_from_data_file("evt_cats.ron").unwrap() };
}

#[derive(Serialize,Deserialize)]
#[public_struct]
struct EvtCategories {
    herb_set: BitSet,
    shrub_set: BitSet,
    tree_set: BitSet,
    sparse_set: BitSet,
    agriculture_set: BitSet,
    developed_set: BitSet,
    water_set: BitSet,
    barren_set: BitSet,
    snow_ice_set: BitSet,
    unknown_set: BitSet,

    shrub_dominated: BitSet,
    herb_dominated: BitSet,
    tree_dominated: BitSet
}

impl EvtCategories {
    pub fn new ()->Self {
        EvtCategories {
            herb_set: BitSet::new(),
            shrub_set: BitSet::new(),
            tree_set: BitSet::new(),
            sparse_set: BitSet::new(),
            agriculture_set: BitSet::new(),
            developed_set: BitSet::new(),
            water_set: BitSet::new(),
            barren_set: BitSet::new(),
            snow_ice_set: BitSet::new(),
            unknown_set: BitSet::new(),

            shrub_dominated: BitSet::new(),
            herb_dominated: BitSet::new(),
            tree_dominated: BitSet::new()
        }
    }

    pub fn save_to (&self, path: impl AsRef<Path>)->Result<()> {
        let content = ron::ser::to_string( self)?; // Ron to_writer requires a std::fmt::Write (not implemented by File)
        std::fs::write( &path, &content)?;

        Ok(())
    }

    pub fn load_from (path: impl AsRef<Path>)->Result<Self> {
        Ok( ron::de::from_reader( File::open(path)? ).map_err(|e| op_failed!("error loading KD tree: {}", e))? )
    }

    pub fn load_from_data_file (fname: &str)->Result<Self> {
        let path = pkg_data_dir!().join(fname);
        Self::load_from( &path)
    }

    pub fn is_herb (&self, v: usize)->bool { self.herb_set.contains(v) }
    pub fn is_shrub (&self, v: usize)->bool { self.shrub_set.contains(v) }
    pub fn is_tree (&self, v: usize)->bool { self.tree_set.contains(v) }
    pub fn is_sparse (&self, v: usize)->bool { self.sparse_set.contains(v) }
    pub fn is_agriculture (&self, v: usize)->bool { self.agriculture_set.contains(v) }
    pub fn is_developed (&self, v: usize)->bool { self.developed_set.contains(v) }
    pub fn is_water (&self, v: usize)->bool { self.water_set.contains(v) }
    pub fn is_barren (&self, v: usize)->bool { self.barren_set.contains(v) }
    pub fn is_snow_ice (&self, v: usize)->bool { self.snow_ice_set.contains(v) }
    pub fn is_unknown (&self, v: usize)->bool { self.unknown_set.contains(v) }

    pub fn is_tree_dominated (&self, v: usize)->bool { self.tree_dominated.contains(v) }
    pub fn is_shrub_dominated (&self, v: usize)->bool { self.shrub_dominated.contains(v) }
    pub fn is_herb_dominated (&self, v: usize)->bool { self.herb_dominated.contains(v) }

    pub fn is_fuel (&self, v: usize)->bool {
        ! (self.is_water(v) || self.is_snow_ice(v) || self.is_barren(v) || self.is_developed(v) )
    }

    pub fn print_stats (&self) {
        println!("herb:         {}", self.herb_set.count());
        println!("shrub:        {}", self.shrub_set.count());
        println!("tree:         {}", self.tree_set.count());
        println!("sparse:       {}", self.sparse_set.count());
        println!("agriculture:  {}", self.agriculture_set.count());
        println!("developed:    {}", self.developed_set.count());
        println!("water:        {}", self.water_set.count());
        println!("barren:       {}", self.barren_set.count());
        println!("snow_ice:     {}", self.snow_ice_set.count());
        println!("unknown:      {}", self.unknown_set.count());
    }
}
