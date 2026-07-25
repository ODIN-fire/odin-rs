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
use csv;
use serde::{Serialize,Deserialize};
use clap;
use anyhow::{Result, anyhow};
use odin_common::{define_cli};
use odin_landfire::evt_cats::EvtCategories;


/// this tool is used to create and store a Landfire EVT category map (odin_landfire::evt_cats::EvtCategories)
/// we inititialize the respective data file from https://landfire.gov/sites/default/files/CSV/LF2025/LF2025_EVT.csv
/// (for convenience we have a copy in resources/)

define_cli! { ARGS [about="create EVT category set file"] =
    input: String [help="LF*_EVT.csv input filename"],
    output: String [help="EVT category output filename"]
}


/*
VALUE, EVT_NAME, LFRDB, EVT_FUEL, EVT_FUEL_N, EVT_LF, EVT_PHYS, EVT_GP, EVT_GP_N, SAF_SRM, EVT_ORDER, EVT_CLASS, EVT_SBCLS, R,G,B,RED,GREEN,BLUE

4401,
Alaska Arctic Coastal Sedge-Dwarf-Shrubland,
4401,
2701,
Sh Alaska Arctic Coastal Sedge-Dwarf-Shrubland,
Shrub,
Shrubland,
762,
Dwarf Shrubland,
LF 40: Sedge Meadow,
Shrub-dominated,
Dwarf
Shrubland,
Deciduous,
255,
205,
112,
1.000000,
0.803922,
0.439216
*/

#[derive(Deserialize, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
struct EvtCatRecord {
    value: i64,
    evt_name: String,
    evt_lf: String,
    evt_phys: String,
    evt_order: String
    //... more to follow (but we don't need all fields)
}

fn main()->Result<()> {
    let mut rdr = csv::Reader::from_reader(File::open( &ARGS.input)?);
    let mut cats = EvtCategories::new();

    for res in rdr.deserialize::<EvtCatRecord>() {
        if let Ok(rec) = res {
            if rec.value >= 0 {
                let idx = rec.value as usize;
                match rec.evt_lf.as_str() {
                    "Shrub" => cats.shrub_set.insert( idx),
                    "Tree" => cats.tree_set.insert( idx),
                    "Herb" => cats.herb_set.insert( idx),
                    "Sparse" => cats.sparse_set.insert( idx),
                    "Developed" => cats.developed_set.insert( idx),
                    "Water" => cats.water_set.insert( idx),
                    "Barren" => cats.barren_set.insert( idx),
                    "Snow-Ice" => cats.snow_ice_set.insert( idx),
                    "Agriculture" => cats.agriculture_set.insert( idx),
                    _ => {
                        cats.unknown_set.insert( idx)
                    }
                };

                match rec.evt_order.as_str() {
                    "Tree-dominated" => cats.tree_dominated.insert( idx),
                    "Shrub-dominated" => cats.shrub_dominated.insert(idx),
                    "Herbaceous / Nonvascular-dominated" => cats.herb_dominated.insert(idx),
                    "Herbaceous/Non-vascular" => cats.herb_dominated.insert(idx),
                    _ => {false}
                };
            }
        }
    }

    cats.print_stats();
    cats.save_to( &ARGS.output)?;

    Ok(())
}
