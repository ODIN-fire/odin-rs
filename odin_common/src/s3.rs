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

//! support functions for AWS S3 objects

use std::{path::{Path,PathBuf},fmt::{Debug,Display}, fs::File, io::{Write,Error}, ops::Deref};
use thiserror::Error;
use aws_sdk_s3::{Client, types::Object, operation::list_objects::builders::ListObjectsFluentBuilder};
use aws_config::{Region,meta::region::RegionProviderChain};
use aws_smithy_types_convert::date_time::DateTimeExt;
use chrono::{DateTime,Utc};

use crate::datetime::Dated;

pub type S3Client = Client;

pub type Result<T> = std::result::Result<T, OdinS3Error>;

#[derive(Error,Debug)]
pub enum OdinS3Error {
    #[error("IO error {0}")]
    IOError( #[from] std::io::Error),

    #[error("AWS S3 get object error {0}")]
    AWSS3ObjectError( #[from] aws_smithy_runtime_api::client::result::SdkError<aws_sdk_s3::operation::get_object::GetObjectError, aws_smithy_runtime_api::http::Response>),

    #[error("AWS S3 list object error {0}")]
    AWSS3ListObjectError( #[from] aws_smithy_runtime_api::client::result::SdkError<aws_sdk_s3::operation::list_objects::ListObjectsError, aws_smithy_runtime_api::http::Response>),

    #[error("AWS byte stream download error {0}")]
    AWSByteStreamError( #[from] aws_smithy_types::byte_stream::error::Error),

    #[error("No object key error")]
    NoObjectKeyError(),

    #[error("No object date error")]
    NoObjectDateError(),    
}


/// newtype to allow extending the S3 Object interface
#[derive(Clone,Debug)]
pub struct S3Object(Object);

impl S3Object {
    pub fn is_dated (&self)->bool {
        self.last_modified.is_some()
    }

    pub fn is_newer (&self, dt: DateTime<Utc>) -> bool {
        if let Some(d) = self.last_modified {
            if let Ok(d) = d.to_chrono_utc() {
                d > dt
            } else { false }
        } else { false }
    }

    pub fn is_older_or_equal (&self, dt: DateTime<Utc>) -> bool {
        if let Some(d) = self.last_modified {
            if let Ok(d) = d.to_chrono_utc() {
                d <= dt
            } else { false }
        } else { false }
    }
}

impl Deref for S3Object {
    type Target = Object;
    fn deref(&self) -> &Self::Target { &self.0 }
}

impl Dated for S3Object {
    /// note this panics if object.last_modified() == None
    /// it should only be used after verifying the object has a proper date set
    fn date (&self)->DateTime<Utc> {
        self.last_modified().unwrap().to_chrono_utc().unwrap()
    }
}

/// create S3 Client for given region
pub async fn create_s3_client (region: String) -> Result<Client> {
    let region_provider = RegionProviderChain::first_try( Region::new( region));
    let aws_config = aws_config::from_env().no_credentials().region(region_provider).load().await; // add anonymous creditials
    Ok( Client::new(&aws_config) ) 
}

/// retrieve all objects (from optional marker) for given bucket/prefix. If there is no error this always returns a `Vec<S3Object>`
/// but it might be empty (if there were no matching objects)
pub async fn get_s3_objects (client: &Client, bucket: &str, prefix: &str, prev_key: Option<&str>) -> Result<Vec<S3Object>> {
    let mut builder = client.list_objects().bucket(bucket).prefix(prefix);
    if let Some(key) = prev_key { 
        builder = builder.marker(key);
    }
    let result = builder.send().await?;

    Ok( result.contents().to_vec().into_iter().map(|o| S3Object(o)).collect() )
}

/// retrieve last object (from optional marker) for given bucket/prefix. Note this can return Ok(None) in case the
/// query was without error but there is no matching object
pub async fn get_last_s3_object (client: &Client, dt: DateTime<Utc>, bucket: &str, prefix: &str, prev_key: Option<&String>) -> Result<Option<S3Object>> {
    let mut builder = client.list_objects().bucket(bucket).prefix(prefix);
    if let Some(key) = prev_key { 
        builder = builder.marker(key);
    }
    let result = builder.send().await?;

    Ok( result.contents.and_then(|mut v| v.pop()).map(|o| S3Object(o)) )
}

/// download a given `S3Object` and store it under its key as filename within the given path.
/// Return a `NoObjectKeyError` if the object has no key
pub async fn download_s3_object (client: &Client, bucket: &str, object: &S3Object, path: &PathBuf) -> Result<PathBuf>{
    if let Some(key) = &object.key {
        let file_name = key.split("/").collect::<Vec<&str>>().last().copied().unwrap();
        let file_path = path.join(file_name);
        let mut file = File::create(&file_path)?;

        let mut object = client
            .get_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await?; 

        while let Some(bytes) = object.body.try_next().await? {
            file.write_all(&bytes)?;
        }
        Ok(file_path)

    } else {
        Err(OdinS3Error::NoObjectKeyError())
    }
}