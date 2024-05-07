/*
 * Copyright (c) 2024, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The ODIN - Open Data Integration Framework is licensed under the
 * Apache License, Version 2.0 (the "License"); you may not use this file
 * except in compliance with the License. You may obtain a copy of the
 * License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use thiserror::Error;

pub type Result<T> = std::result::Result<T, OdinGoesRError>;
#[derive(Error,Debug)]
pub enum OdinGoesRError {
    #[error("IO error {0}")]
    IOError( #[from] std::io::Error),

    #[error("config parse error {0}")]
    ConfigParseError( String ),

    #[error("AWS S3 get object error {0}")]
    AWSS3ObjectError( #[from] aws_smithy_runtime_api::client::result::SdkError<aws_sdk_s3::operation::get_object::GetObjectError, aws_smithy_runtime_api::http::Response>),

    #[error("AWS S3 list object error {0}")]
    AWSS3ListObjectError( #[from] aws_smithy_runtime_api::client::result::SdkError<aws_sdk_s3::operation::list_objects::ListObjectsError, aws_smithy_runtime_api::http::Response>),

    #[error("AWS byte stream download error {0}")]
    AWSByteStreamError( #[from] aws_smithy_types::byte_stream::error::Error),

    #[error("No object error")]
    NoObjectError( String ),

    #[error("No object key error")]
    NoObjectKeyError(),

    #[error("String to float conversion error {0}")]
    FloatConversionError( #[from] std::num::ParseFloatError),

    #[error("Misc error {0}")]
    MiscError( String ),

     // pass through for errors in gdal crate
     #[error("gdal error {0}")]
     GdalError( #[from] gdal::errors::GdalError),

     #[error("serde error {0}")]
     SerdeError( #[from] serde_json::Error),

     #[error("ODIN Actor error {0}")]
     OdinActorError( #[from] odin_actor::errors::OdinActorError),

}

pub fn no_object  (msg: impl ToString)->OdinGoesRError {
    OdinGoesRError::NoObjectError(msg.to_string())
}
