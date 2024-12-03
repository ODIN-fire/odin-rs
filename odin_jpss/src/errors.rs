/*
 * Copyright © 2024, United States Government, as represented by the Administrator of 
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

 use thiserror::Error;

 pub type Result<T> = std::result::Result<T, OdinJpssError>;
 
 #[derive(Error, Debug)]
 pub enum OdinJpssError {
    #[error("IO error {0}")]
    IOError( #[from] std::io::Error),

    #[error("serde error {0}")]
    SerdeError( #[from] serde_json::Error),

    #[error("CSV error {0}")]
    CsvError( #[from] csv::Error),

    #[error("Reqwest error {0}")]
    ReqwestError( #[from] reqwest::Error),

    #[error("SPG4 error {0}")]
    Spg4Error( #[from] sgp4::Error),

    #[error("SPG4 elements error {0}")]
    Spg4ElementsError( #[from] sgp4::ElementsError),
    
    #[error("SPG4 date time error {0}")]
    Spg4DatetimeError( #[from] sgp4::DatetimeToMinutesSinceEpochError),
    
    // #[error("Misc error {0}")]
    // StringError( #[from] std::string::String),

    #[error("Misc error {0}")]
    MiscError( String ),

    #[error("Date error {0}")]
    DateError( String ),

    #[error("time delta out of range error {0}")]
    DurationError( #[from] chrono::OutOfRangeError),

    #[error("Bounds error {0}")]
    BoundsError( String ),

    #[error("File download error {0}")]
    FileDownloadError( String ),

    #[error("TLE import failed: {0}")]
    TleError( String ),

    #[error("ODIN Actor error {0}")]
    OdinActorError( #[from] odin_actor::errors::OdinActorError),

 }
 
 pub fn date_error (msg: impl ToString)->OdinJpssError {
    OdinJpssError::DateError(msg.to_string())
 }

 pub fn bounds_error (msg: impl ToString)->OdinJpssError {
    OdinJpssError::BoundsError(msg.to_string())
}

 