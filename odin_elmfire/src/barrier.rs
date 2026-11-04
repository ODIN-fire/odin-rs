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

//! fuel breaks and air drops (see doc/odin_elmfire.md "Fuel Breaks and Air Drops")
//!
//! ELMFIRE only supports barriers through a (single band) "barriers.tif" raster of barrier widths [m]. A burning barrier
//! cell does not spread further unless 1.5 x flame length exceeds the barrier width, i.e. this is a fuel break model
//! that is in effect for the whole simulation. We map user barriers to ELMFIRE inputs as follows:
//!  - fuel breaks narrower than the landfire cell size (hand lines, dozer lines): barriers.tif with the real width
//!  - fuel breaks at least as wide as a landfire cell (wide fuel breaks): fbfm40.tif set to a non-burnable fuel model
//!  - air drops (barriers with a drop agent): time step aware increase of fuel moisture inputs (m1, m10, ..)

use std::{collections::HashMap, path::Path};
use serde::Deserialize;
use uom::si::length::meter;

use odin_gdal::{
    GdalDataType, copy_proj, create_dataset, get_gtiff_create_opts, get_gtiff_driver, open_dataset, open_dataset_for_update,
    get_rasterband_as_vec, set_rasterband_from_vec,
    draw::{draw_geo_line_string_buffer, fill_geo_line_string_buffer, geo_ellipse_coverage, geo_line_string_coverage},
    errors::OdinGdalError,
    warp::ResampleAlg,
};

use crate::{ElmfireConfig, ElmFire, Barrier, BarrierValue, errors::{OdinElmfireError, Result, op_failed}, input::resample_to_lf_grid};

/* #region configuration *********************************************************************************************/

/// a configured air drop agent (e.g. long-term retardant, water) that defines how drops modify fuel moisture inputs
#[derive(Deserialize, Debug, Clone)]
pub struct DropAgent {
    pub name: String,
    pub moisture: Vec<MoistureModification>,
}

/// modification of a single (hourly banded) fuel moisture input raster for cells that are covered by a drop
#[derive(Deserialize, Debug, Clone)]
pub struct MoistureModification {
    pub raster: String, // ELMFIRE input raster name ("m1", "m10", "m100", "lh" or "lw")
    pub increase: MoistureIncrease,
    pub decay: MoistureDecay,
}

/// initial moisture increase [percent] for fully covered cells (scaled by cell coverage)
#[derive(Deserialize, Debug, Clone, Copy)]
pub enum MoistureIncrease {
    Add(f64), // add given percentage points to the (forecast) moisture
    Set(f64), // raise moisture to the given value (no change if forecast moisture is already higher)
}

/// how the moisture increase degrades over time (down to the forecast moisture)
#[derive(Deserialize, Debug, Clone, Copy)]
pub enum MoistureDecay {
    Constant,         // no decay
    Linear(f64),      // linear decay to zero over given number of hours
    Exponential(f64), // exponential decay with given half life in hours
}

impl MoistureIncrease {
    pub fn delta (&self, base: f64)->f64 {
        match self {
            MoistureIncrease::Add(d) => *d,
            MoistureIncrease::Set(v) => (v - base).max(0.0),
        }
    }
}

impl MoistureDecay {
    /// remaining fraction of the initial increase after the given number of hours
    pub fn factor (&self, hours: f64)->f64 {
        match self {
            MoistureDecay::Constant => 1.0,
            MoistureDecay::Linear(h) => if *h > 0.0 { (1.0 - hours / h).max(0.0) } else { 0.0 },
            MoistureDecay::Exponential(hl) => if *hl > 0.0 { 0.5f64.powf( hours / hl) } else { 0.0 },
        }
    }
}

/// the fuel moisture input rasters that can be modified by drops
pub const MOISTURE_RASTERS: &[&str] = &["m1", "m10", "m100", "lh", "lw"];

/// default fuel model for wide fuel breaks: NB9 (bare ground)
pub fn default_fuel_break_fbfm40 ()->i16 { 99 }

/// default drop agents if none are configured. Note these values are initial estimates that need calibration:
///  - long-term retardant (LTR) keeps its effect after drying (until it is washed out or burned)
///  - water only has an effect until it evaporates
/// A fine dead fuel moisture of 60% exceeds the moisture of extinction of all standard (FBFM40) fuel models
pub fn default_drop_agents ()->Vec<DropAgent> {
    vec![
        DropAgent { name: "LTR".into(), moisture: vec![
            MoistureModification { raster: "m1".into(), increase: MoistureIncrease::Set(60.0), decay: MoistureDecay::Linear(24.0) },
            MoistureModification { raster: "m10".into(), increase: MoistureIncrease::Add(15.0), decay: MoistureDecay::Linear(24.0) },
        ]},
        DropAgent { name: "water".into(), moisture: vec![
            MoistureModification { raster: "m1".into(), increase: MoistureIncrease::Set(40.0), decay: MoistureDecay::Exponential(0.5) },
            MoistureModification { raster: "m10".into(), increase: MoistureIncrease::Add(5.0), decay: MoistureDecay::Exponential(0.5) },
        ]},
    ]
}

/* #endregion configuration */

/* #region fuel breaks ***********************************************************************************************/

/// is this a fuel break that is narrower than a landfire cell (i.e. modeled by barriers.tif)
pub fn is_narrow_fuel_break (b: &Barrier, config: &ElmfireConfig)->bool {
    !b.is_air_drop() && b.width.get::<meter>() < config.lf_res_m as f64
}

/// do we need barriers.tif (USE_BARRIERS) for this simulation
pub fn uses_barrier_raster (ef: &ElmFire, config: &ElmfireConfig)->bool {
    ef.barriers.iter().any( |b| is_narrow_fuel_break( b, config))
}

/// create the ELMFIRE inputs for fuel breaks. Note this has to be called after the landfire inputs have been created
/// since we modify the fbfm40.tif and use it as the reference grid for barriers.tif
pub fn create_fuel_break_input (ef: &ElmFire, config: &ElmfireConfig)->Result<()> {
    let dir = ef.dir.join("inputs");
    let lf_res = config.lf_res_m as f64;

    //--- wide fuel breaks: set fbfm40 to non-burnable for all cells whose center is inside of the fuel break. We use a
    // min width of sqrt(2) cells to avoid diagonal-only connections between cells (the level set front could pass through)
    let wide: Vec<&Barrier> = ef.barriers.iter().filter( |b| !b.is_air_drop() && !is_narrow_fuel_break( b, config)).collect();
    if !wide.is_empty() {
        let mut ds = open_dataset_for_update( dir.join("fbfm40.tif"))?;
        for b in wide {
            let width = b.width.get::<meter>().max( lf_res * std::f64::consts::SQRT_2);
            fill_geo_line_string_buffer( &mut ds, &b.geo_points(), width, config.fuel_break_fbfm40 as f64)?;
        }
    }

    //--- narrow fuel breaks: barriers.tif with the barrier width [m] (ELMFIRE breach threshold) for all cells that are
    // touched by the fuel break. This is gap free, i.e. the front can't pass through diagonal-only cell connections
    let mut narrow: Vec<&Barrier> = ef.barriers.iter().filter( |b| is_narrow_fuel_break( b, config)).collect();
    if !narrow.is_empty() {
        let fbfm_ds = open_dataset( dir.join("fbfm40.tif"))?;
        let (w,h) = fbfm_ds.raster_size();

        let mut ds = create_dataset( &get_gtiff_driver(), dir.join("barriers.tif"), w, h, 1, GdalDataType::Float32, Some(get_gtiff_create_opts()))?;
        copy_proj( &fbfm_ds, &mut ds)?; // same bbox, SRS and pixel resolution
        ds.rasterband(1).and_then( |mut band| band.fill(0.0, None)).map_err( OdinGdalError::from)?; // cells without barriers

        // crossing fuel breaks should use the max width. Since the rasterizer replaces cell values we draw them in
        // ascending width order, i.e. the widest one is drawn last
        narrow.sort_by( |a,b| a.width.value.total_cmp( &b.width.value));
        for b in narrow {
            let width = b.width.get::<meter>();
            draw_geo_line_string_buffer( &mut ds, &b.geo_points(), width, width)?;
        }
    } // ds is written when it goes out of scope

    Ok(())
}

/* #endregion fuel breaks */

/* #region air drops *************************************************************************************************/

/// increase fuel moisture inputs for cells that are covered by air drops, starting with the drop time step. This has to
/// be called after the moisture inputs have been created and resampled (see input::resample_computed_inputs) since drop
/// footprints require the landfire grid resolution. The increase for each cell is scaled by its drop coverage, and
/// overlapping drops use the max increase
pub fn apply_air_drops (ef: &ElmFire, config: &ElmfireConfig)->Result<()> {
    let drops: Vec<&Barrier> = ef.barriers.iter().filter( |b| b.is_air_drop()).collect();
    if drops.is_empty() { return Ok(()) }

    // group the moisture modifications of all drops by raster
    let mut by_raster: HashMap<&str, Vec<(&Barrier, &MoistureModification)>> = HashMap::new();
    for drop in &drops {
        let agent_name = drop.agent.as_deref().unwrap_or_default(); // is_air_drop guarantees it is set
        let agent = config.drop_agents.iter().find( |a| a.name == agent_name)
            .ok_or( op_failed!("unknown drop agent '{}' for barrier {}", agent_name, drop.name))?;

        for m in &agent.moisture {
            if !MOISTURE_RASTERS.contains( &m.raster.as_str()) {
                return Err( op_failed!("invalid moisture raster '{}' for drop agent {}", m.raster, agent.name))
            }
            by_raster.entry( m.raster.as_str()).or_default().push( (*drop, m));
        }
    }

    let dir = ef.dir.join("inputs");
    for (raster, mods) in by_raster {
        let path = dir.join( format!("{raster}.tif"));
        if !path.is_file() { return Err( op_failed!("no {raster} input for air drops")) }

        // drop footprints require the landfire grid (this is a no-op if the input is already resampled)
        resample_to_lf_grid( ef, config, &path, ResampleAlg::Cubic)?;
        apply_moisture_modifications( &path, &mods, config)?;
    }

    Ok(())
}

/// the per cell coverage of a drop footprint - bucket drops are ellipses, other drops are lines with a width
fn drop_coverage (ds: &odin_gdal::Dataset, drop: &Barrier)->Result<Vec<(usize,usize,f64)>> {
    match &drop.value {
        BarrierValue::GeoEllipse(ellipse) => Ok( geo_ellipse_coverage( ds, ellipse)?),
        _ => Ok( geo_line_string_coverage( ds, &drop.geo_points(), drop.width.get::<meter>())?)
    }
}

fn apply_moisture_modifications (path: &Path, mods: &[(&Barrier, &MoistureModification)], config: &ElmfireConfig)->Result<()> {
    let mut ds = open_dataset_for_update( path)?;
    let n_bands = ds.raster_count();
    let (n_cols, _) = ds.raster_size();

    // the per cell coverage of each drop (computed once)
    let mut coverages: Vec<Vec<(usize,usize,f64)>> = Vec::with_capacity( mods.len());
    for (drop, _) in mods {
        coverages.push( drop_coverage( &ds, drop)?);
    }

    for band in 1..=n_bands {
        let step = (band - 1) as u32; // band 1 is the full hour before the first ignition
        if !mods.iter().any( |(drop,_)| drop.step <= step) { continue }

        let mut vs: Vec<f32> = get_rasterband_as_vec( &ds, band)?;
        let mut incs: HashMap<usize,f64> = HashMap::new(); // max increase per cell index

        for ((drop, m), coverage) in mods.iter().zip( coverages.iter()) {
            if drop.step > step { continue }
            let decay = m.decay.factor( (step - drop.step) as f64);
            if decay <= 0.0 { continue }

            for (col, row, c) in coverage {
                let idx = row * n_cols + col;
                let base = vs[idx] as f64;
                if base < 0.0 { continue } // nodata
                let inc = m.increase.delta( base) * c * decay;
                let e = incs.entry( idx).or_insert( 0.0);
                if inc > *e { *e = inc }
            }
        }

        for (idx, inc) in incs {
            vs[idx] = (vs[idx] as f64 + inc) as f32;
        }
        set_rasterband_from_vec( &mut ds, band, vs)?;
    }

    Ok(())
}

/* #endregion air drops */
