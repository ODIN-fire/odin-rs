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
use std::ffi::CStr;
use std::ptr::null;
use thiserror::Error;
use crate::pc_char_to_string;
use gdal_sys::{CPLErr::CE_None, CPLErr};

pub type Result<T> = std::result::Result<T, OdinGdalError>;

pub type GdalError = gdal::errors::GdalError;

#[derive(Error,Debug)]
pub enum OdinGdalError {
    #[error("invalid file name {0}")]
    InvalidFileName(String),

    #[error("GDAL function {0} failed")]
    GdalFunctionFailed(&'static str),

    #[error("no spatial reference system")]
    NoSpatialReferenceSystem,

    #[error("failed to convert to C string {0}")]
    CStringConversion( #[from] std::ffi::NulError),

    #[error("IO error {0}")]
    IOError( #[from] std::io::Error),

    #[error("String to float conversion error {0}")]
    FloatConversionError( #[from] std::num::ParseFloatError),

    // pass through for errors in gdal crate
    #[error("gdal error {0}")]
    Error( #[from] gdal::errors::GdalError),

    #[error("gdal error {0}")]
    MiscError(String),

    #[error("last gdal error {0}")]
    LastGdalError(String)
}

pub fn reset_last_gdal_error() {
    unsafe {
        gdal_sys::CPLErrorReset();
    }
}

pub fn last_gdal_err_description() -> Option<String> {
    unsafe {
        let err = gdal_sys::CPLGetLastErrorType();
        if err != gdal_sys::CPLErr::CE_None {
            let err_no = gdal_sys::CPLGetLastErrorNo();
            let p_msg = gdal_sys::CPLGetLastErrorMsg();

            gdal_sys::CPLErrorReset();

            if p_msg != null() {
                let msg_cstr = CStr::from_ptr(p_msg);
                Some(format!("{}:{}: {}", err, err_no, msg_cstr.to_string_lossy()))
            } else {
                Some(format!("{}:{}", err, err_no))
            }
        } else {
            None
        }
    }
}

pub fn gdal_error(e: GdalError) -> OdinGdalError {
    OdinGdalError::Error(e)
}

pub fn map_gdal_error <T> (res: std::result::Result<T,GdalError>) -> Result<T> {
    res.map_err(|e| OdinGdalError::Error(e))
}

pub fn last_gdal_error() -> OdinGdalError {
    OdinGdalError::LastGdalError(last_gdal_err_description().unwrap_or_else(|| "none".to_string()))
}

pub fn last_cpl_err(cpl_err_class: CPLErr::Type) -> GdalError {
    let last_err_no = unsafe { gdal_sys::CPLGetLastErrorNo() };
    let last_err_msg = pc_char_to_string(unsafe { gdal_sys::CPLGetLastErrorMsg() });
    unsafe { gdal_sys::CPLErrorReset() };
    GdalError::CplError {
        class: cpl_err_class,
        number: last_err_no,
        msg: last_err_msg,
    }
}

pub fn misc_error(s: String) -> OdinGdalError {
    OdinGdalError::MiscError(s)
}