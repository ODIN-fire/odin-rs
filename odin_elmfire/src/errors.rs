#![allow(unused)]

use thiserror::Error;
use odin_build;
use odin_openmeteo;

pub type Result<T> = std::result::Result<T, OdinElmfireError>;

#[derive(Error,Debug)]
pub enum OdinElmfireError {
    #[error("build error {0}")]
    BuildError( #[from] odin_build::OdinBuildError),

    #[error("IO error {0}")]
    IOError( #[from] std::io::Error),

    #[error("Utf8Error {0}")]
    Utf8Error( #[from] std::str::Utf8Error),

    #[error("ParseIntError {0}")]
    ParseIntError( #[from] std::num::ParseIntError),

    #[error("ParseFloatError {0}")]
    ParseFloatError( #[from] std::num::ParseFloatError),

    #[error("net error {0}")]
    NetError( #[from] odin_common::net::OdinNetError),

    #[error("CSV error {0}")]
    CsvError( #[from] csv::Error),

    #[error("UTM error {0}")]
    UtmError( #[from] odin_common::utm::OdinUtmError),

    #[error("DEM error {0}")]
    DemError( #[from] odin_dem::errors::OdinDemError),

    #[error("action failure {0}")]
    ActionFailure(String), // ActionFailure cannot impl std::error::Error

    #[error("FEMS error {0}")]
    FemsError( #[from] odin_fems::errors::OdinFemsError),

    #[error("Landfire error {0}")]
    LandfireError( #[from] odin_landfire::errors::OdinLandfireError),

    #[error("NFDRS error {0}")]
    NfdrsError( #[from] odin_nfdrs::errors::OdinNfdrsError),

    #[error("OpenMeteo error {0}")]
    OpenMeteoError( #[from] odin_openmeteo::errors::OdinOpenMeteoError),

    #[error("GDAL error {0}")]
    GdalError( #[from] odin_gdal::errors::OdinGdalError),

    #[error("actor error {0}")]
    ActorError( #[from] odin_actor::errors::OdinActorError),

    #[error("reqwest error {0}")]
    ReqwestError( #[from] reqwest::Error),

    #[error("serde error {0}")]
    SerdeError( #[from] serde_json::Error),

    #[error("RON error {0}")]
    RonError( #[from] ron::error::Error),

    #[error("RON read error {0}")]
    RonReadError( #[from] ron::error::SpannedError),

    #[error("config chrono error {0}")]
    ChronoError( #[from] chrono::OutOfRangeError),

    #[error("chrono parse error {0}")]
    ChronoParseError( #[from] chrono::ParseError),

    #[error("file not found")]
    FileNotFoundError(),

    #[error("namelist parse error")]
    NmlParseError(String),

    #[error("operation failed {0}")]
    OpFailedError(String),
}

macro_rules! op_failed {
    ($fmt:literal $(, $arg:expr )* ) => {
        OdinElmfireError::OpFailedError( format!( $fmt $(, $arg)* ))
    };
}
pub (crate) use op_failed;

macro_rules! nml_parse_error {
    ($fmt:literal $(, $arg:expr )* ) => {
        OdinElmfireError::NmlParseError( format!( $fmt $(, $arg)* ))
    };
}
pub (crate) use nml_parse_error;
