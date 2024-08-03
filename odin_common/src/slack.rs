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

 //! slack web api abstraction

use std::{fs,path::{Path,PathBuf},result::Result,error::Error,io::{Error as IOError,ErrorKind}};
use reqwest::{Client,blocking};
use serde::{Serialize,Deserialize};
use serde_json;
use crate::fs::filename_of_path;

type SlackError = Box<dyn Error>;
type SlackResult<T> = Result<T,SlackError>;

/// send chat text message as async
/// note - icon replaces avatar, but ony in the first of a sequence of messages from the same sender
pub async fn send_msg (token: &str, channel_id: &str, msg: &str, icon: Option<&str>) -> SlackResult<()> {
    let client = Client::new();

    let mut params: Vec<(&str,&str)> = Vec::new();
    params.push( ("channel", channel_id) );
    params.push( ("text", msg) );
    if let Some(icon_name) = icon { params.push( ("icon_emoji", icon_name) ); }

    let resp = client.post("https://slack.com/api/chat.postMessage")
        .bearer_auth( token)
        .query( &params)
        .send()
        .await?;

    Ok(())
}

/// send chat text message as blocking operation (can be called from sync and async, but blocks current thread)
/// note - icon replaces avatar, but ony in the first of a sequence of messages from the same sender
pub fn blocking_send_msg (token: &str, channel_id: &str, msg: &str, icon: Option<&str>) -> SlackResult<()> {
    let client = blocking::Client::new();

    let mut params: Vec<(&str,&str)> = Vec::new();
    params.push( ("channel", channel_id) );
    params.push( ("text", msg) );
    if let Some(icon_name) = icon { params.push( ("icon_emoji", icon_name) ); }

    let resp = client.post("https://slack.com/api/chat.postMessage")
        .bearer_auth( token)
        .query( &params)
        .send()?;

    Ok(())   
}

pub struct FileAttachment {
    pub path: PathBuf,
    pub caption: String
}


#[derive(Deserialize,Debug)]
pub struct FilesGetUploadUrlExternalResponse {
    pub ok: bool,
    pub upload_url: String,
    pub file_id: String
}

#[derive(Serialize)]
struct UploadFile {
    id: String, // slack id (not pathname)
    title: String
}

/// send a message with attached files to Slack channel
/// note that channel_id is not a channel name!
/// unfortunately this does not support icons - they would have to be uploaded as images
pub async fn send_msg_with_files (token: &str, channel_id: &str, msg: &str, files: &Vec<FileAttachment>) -> SlackResult<()> {
    let client = Client::new();
    let uploads = upload_files( &client, token, files).await?;
    
    let resp = client.get("https://slack.com/api/files.completeUploadExternal")
        .bearer_auth( token)
        .query( &[
            ("files", serde_json::to_string( &uploads)?.as_str()),
            ("channel_id", channel_id),
            ("initial_comment", msg)
        ])
        .send()
        .await?;

    Ok(())
}

/// upload a list of files to Slack
async fn upload_files (client: &Client, token: &str, files: &Vec<FileAttachment>)->SlackResult<Vec<UploadFile>> {
    let mut uploads: Vec<UploadFile> = Vec::with_capacity(files.len());

    for f in files {
        let path = &f.path;
        let filename = filename_of_path(path)?;
        let length = path.metadata()?.len() as usize;

        if !path.is_file() { return Err( Box::new(IOError::new(ErrorKind::NotFound, filename))) }
        let contents = fs::read(path)?;

        let resp = client.get( "https://slack.com/api/files.getUploadURLExternal")
            .bearer_auth( token)
            .query( &[
                ("filename", filename),
                ("length", length.to_string())
            ])
            .send()
            .await?;

        let data = resp.text().await?;
        let url_resp: FilesGetUploadUrlExternalResponse = serde_json::from_str( data.as_str())?;

        client.post( url_resp.upload_url.as_str())
            .body( contents)
            .send()
            .await?;

        uploads.push( UploadFile { id: url_resp.file_id, title: f.caption.clone() });
    }

    Ok(uploads)
}
