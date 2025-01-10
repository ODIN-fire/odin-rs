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
#![allow(unused)]

use std::{io::Write, path::Path};
use odin_common:: {
    define_cli, 
    fs::{self, ensure_writable_dir, existing_non_empty_file_from_path, file_contents_as_string}, 
    net::{get_differing_size_file, get_content_length, get_headermap}
};
use reqwest::{Client,header::{HeaderMap,HeaderName,HeaderValue}};
use anyhow::Result;
use tokio;
use num_format::{Locale, ToFormattedString};

define_cli! { ARGS [about="get_files - download URLs"] =
    size_only: bool [ help="only retrieve total content length, do not download", long,short],
    output_dir: String [help="directory where to store downloaded file(s)",long,short, default_value="."],
    headers: Vec<String> [help="headers to set for download requests (<type>:<value>)", long,short], 
    from_file: bool [help="input is filename with list of URLs (one per line)", long, short],
    input: String [help="URL or file name with list of URLs (if --from_file is set)"]
}

#[tokio::main]
async fn main()->Result<()> {
    let mut n_files = 0;
    let mut n_bytes: u64 = 0;
    let client = Client::new();
    let headers = get_headermap( &ARGS.headers)?;
    let opt_headers = if headers.is_empty() {None} else {Some(headers)};
    
    if ARGS.from_file {
        process_input_file( &client, &ARGS.input, &opt_headers, &mut n_files, &mut n_bytes).await?;
        println!("{n_files} files with {} bytes", n_bytes.to_formatted_string(&Locale::en));

    } else {
        process_url( &client, &ARGS.input,  &opt_headers, &mut n_files, &mut n_bytes).await?;
    }

    Ok(())
}


async fn process_input_file (client: &Client, fpath: &str, opt_headers: &Option<HeaderMap>, n_files: &mut usize, n_bytes: &mut u64) -> Result<()> {
    let mut input_file = existing_non_empty_file_from_path(&ARGS.input)?;
    let input = file_contents_as_string( &mut input_file)?;

    for url in input.lines() {
        process_url( client, url, opt_headers, n_files, n_bytes).await?;
    }

    Ok(())
}

async fn process_url (client: &Client, url: &str, opt_headers: &Option<HeaderMap>, n_files: &mut usize, n_bytes: &mut u64) -> Result<()> {
    print!("{url}.. ");
    std::io::stdout().flush();

    let res = if ARGS.size_only {
        get_content_length( &client, url, opt_headers).await
    } else {
        get_differing_size_file( &client, url, opt_headers, &ARGS.output_dir).await
    };

    match res {
        Ok(len) => { 
            println!(" {}", len.to_formatted_string(&Locale::en));
            *n_files += 1;
            *n_bytes += len;
        }
        Err(e) => println!(" ERROR: {e}")
    }

    Ok(())
}