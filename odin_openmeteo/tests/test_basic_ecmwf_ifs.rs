#![allow(unused)]

use std::{fs,sync::Arc,any::type_name};
use odin_common::{geo::GeoRect, datetime::{days, hours}};
use odin_gdal::{grid::GdalGridAlgorithmOptions,set_band_meta};
use serde_json;
use gdal::{Dataset, Metadata};
use chrono::{DateTime,Utc};

use odin_hrrr::meta;
use odin_wx::WxDataSetRequest;
use odin_openmeteo::{
    DataSet, HourlyLocationData, LocationData, Result, convert, dataset::{BasicData},
    get_timesteps_from_file, parse_location_data,
    ids::{FieldId, ModelId}
};

/// run with "cargo test --test test_basic_ecmwf_ifs test_parse -- --nocapture"
#[test]
fn test_parse ()->Result<()> {
    let data: Vec<HourlyLocationData<BasicData>> = parse_location_data("resources/aus-basic-ecmwf_ifs.json")?;
    //println!("{:?}", size_of( &data));

    for e in &data {
        println!("{} , {}", e.meta_data.latitude, e.meta_data.longitude);
    }

    Ok(())
}

/// run with "cargo test --test test_basic_ecmwf_ifs test_gridding_aus -- --nocapture"
#[test]
fn test_gridding_aus ()->Result<()> {
    let region = "rect/aus/act1";
    let model_name = "ecmwf_ifs";
    let bbox = GeoRect::from_wsen_degrees( 149.2, -35.7, 149.7, -35.3);
    let start_date = Utc::now();
    let end_date = start_date + hours(6);

    let paths = convert::wn_path_to_hrrr::<_,_,BasicData>( &region, &model_name, &bbox, "resources/aus-basic-ecmwf_ifs.json", "../../cache/odin_openmeteo")?;
    for path in &paths {
        println!("wrote {:?}", path);
        fs::remove_file( path.as_ref())?;
    }

    Ok(())
}

/// run with "cargo test --test test_basic_ecmwf_ifs test_timesteps -- --nocapture"
#[test]
fn test_timesteps ()->Result<()> {
    let ts = get_timesteps_from_file("resources/aus-basic-ecmwf_ifs.json")?;
;
    for t in &ts {
        println!("{}", t);
    }

    assert!( ts.len() > 0);

    Ok(())
}
