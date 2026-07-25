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
    OpenMeteoDataSet, OpenMeteoLocationData, Result, convert, dataset::{BasicEcmwfIfs, BasicEcmwfIfsData}, get_fc_days, get_timesteps_from_file, ids::{FieldId, ModelId}, url_query_hourly_bbox, wx_service::OpenMeteoService
};

/// run with "cargo test --test test_basic_ecmwf_ifs test_parse -- --nocapture"
#[test]
fn test_parse ()->Result<()> {
    let data: Vec<OpenMeteoLocationData<BasicEcmwfIfsData>> = OpenMeteoLocationData::parse_path("resources/aus-basic-ecmwf_ifs.json")?;
    //println!("{:?}", size_of( &data));

    for e in &data {
        println!("{} , {}", e.metadata.latitude, e.metadata.longitude);
    }

    Ok(())
}

/// run with "cargo test --test test_basic_ecmwf_ifs test_gridding_aus -- --nocapture"
#[test]
fn test_gridding_aus ()->Result<()> {
    let region = Arc::new("rect/aus/act1".to_string());
    let bbox = GeoRect::from_wsen_degrees( 149.2, -35.7, 149.7, -35.3);
    let fc_duration = hours(6);
    let fc_days = get_fc_days( &Utc::now(), fc_duration);
    let wx_name = Arc::new( type_name::<OpenMeteoService>().to_string());
    let model_name = Arc::new( ModelId::ecmwf_ifs.as_ref().to_string());
    let dataset_name = Arc::new( BasicEcmwfIfsData::dataset_name().to_string());
    let query = url_query_hourly_bbox::<BasicEcmwfIfsData>( &bbox, fc_days, 0, model_name.as_str());

    let req = WxDataSetRequest::new( region, bbox, wx_name, model_name, dataset_name, fc_duration, query);

    let paths = convert::basic_ecmwf_ifs_to_hrrr( &req, "resources/aus-basic-ecmwf_ifs.json", "../../cache/odin_openmeteo")?;
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
