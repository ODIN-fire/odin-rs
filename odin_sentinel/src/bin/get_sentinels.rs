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

use lazy_static::lazy_static;
use std::{process::Output, path::PathBuf, str::FromStr, fmt::{Display,Formatter}, fs::File, io::Write};
use anyhow::Result;
use structopt::StructOpt;
use displaydoc::Display;
use tokio;
use reqwest;
use strum::EnumString;

use odin_sentinel::{get_http_client, load_config, SentinelConfig, SentinelStore};
use odin_build;

#[derive(Debug,EnumString)]
#[strum(serialize_all="snake_case")]
enum OutputFormat { Rust, Ron, Json }


#[derive(StructOpt)]
#[structopt(about = "Delphire Sentinel data retriever tool")]
struct CliOpts {
    /// run verbose
    #[structopt(short,long)]
    verbose: bool,

    /// produce formatted output
    #[structopt(short,long)]
    pretty: bool,

    /// output format (rust,ron,json)
    #[structopt(short,long,default_value="rust")]
    format: OutputFormat,

    /// optional path where to store output
    #[structopt(short,long)]
    output: Option<PathBuf>,

    //.. and more to follow
}

lazy_static! {
    static ref ARGS: CliOpts = CliOpts::from_args();
}

#[tokio::main]
async fn main()->Result<()> {
    odin_build::set_bin_context!();

    let sentinel_config: SentinelConfig = load_config( "sentinel.ron")?;
    let http_client = get_http_client()?;

    let mut sentinel_store = SentinelStore::new();
    sentinel_store.fetch_from_config( &http_client, &sentinel_config).await?;

    match ARGS.format {
        OutputFormat::Json => {
            produce_output( sentinel_store.to_json( ARGS.pretty)?);
        },
        OutputFormat::Ron => {
            produce_output( sentinel_store.to_ron( ARGS.pretty)?);
        },
        OutputFormat::Rust => {
            if ARGS.pretty {
                produce_output( format!( "{:#?}", sentinel_store.values()))?;
            } else {
                produce_output( format!( "{:?}", sentinel_store.values()))?;
            }
        }
    }

    Ok(())
}

fn produce_output (s: String)->Result<()> {
    if let Some(path) = &ARGS.output {
        let mut file = File::create(path)?;
        Ok(file.write_all( s.as_bytes())?)
    } else {
        println!("{}", s);
        Ok(())
    }
}