/*
 * Copyright 2026 Peter Mehlitz
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://apache.org
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! list available NEXRAD Level II volumes (or stations) in the AWS S3 bucket

use chrono::{NaiveDate, Utc, TimeDelta};
use clap::Parser;
use anyhow::{Result, anyhow};
use odin_nexrad::{
    NexradConfig, DEFAULT_BUCKET, DEFAULT_REGION, VolumeInfo, Station,
    load_config, get_station, read_stations_csv, list_stations, list_volumes, list_volumes_between
};
use odin_common::s3::create_s3_client;

#[derive(Parser, Debug)]
#[command(version, about, long_about = "list NEXRAD Level II volumes of stations (or available stations) in the AWS S3 bucket")]
struct Args {
    /// list the stations that have volumes for the date instead of volumes
    #[arg(long)]
    stations: bool,

    /// UTC date (YYYY-MM-DD), default is today
    #[arg(short, long)]
    date: Option<NaiveDate>,

    /// list volumes of the last N hours (across date boundaries) instead of a date
    #[arg(long)]
    hours: Option<u32>,

    /// NexradConfig file (bucket, region, default stations, station CSV)
    #[arg(long, default_value="norcal_nexrad.ron")]
    config: String,

    /// S3 bucket (default from config or unidata-nexrad-level2)
    #[arg(long)]
    bucket: Option<String>,

    /// station ids (default: stations of config)
    station_ids: Vec<String>,
}

#[tokio::main]
async fn main ()->Result<()> {
    let args = Args::parse();
    let config: Option<NexradConfig> = load_config( &args.config).ok();
    let bucket = args.bucket.clone().or( config.as_ref().map( |c| c.bucket.clone())).unwrap_or( DEFAULT_BUCKET.to_string());
    let region = config.as_ref().map( |c| c.region.clone()).unwrap_or( DEFAULT_REGION.to_string());
    let csv_stations: Vec<Station> = config.as_ref().and_then( |c| c.stations_csv.as_ref())
        .and_then( |p| read_stations_csv( &p.0).ok()).unwrap_or_default();

    let client = create_s3_client( &region).await?;
    let date = args.date.unwrap_or_else( || Utc::now().date_naive());

    if args.stations {
        let ids = list_stations( &client, &bucket, date).await?;
        println!("{} stations with volumes on {date}:", ids.len());
        for id in &ids {
            match get_station( id, &csv_stations) {
                Some(s) => println!("  {id}  {:9.4} {:10.4}  {}", s.latitude, s.longitude, s.name),
                None => println!("  {id}  (unknown position)")
            }
        }
        return Ok(())
    }

    let ids = if !args.station_ids.is_empty() { args.station_ids.clone() } else {
        config.as_ref().map( |c| c.stations.clone()).ok_or( anyhow!("no station ids given and no {} config", args.config))?
    };

    for id in &ids {
        let volumes = match args.hours {
            Some(h) => { let now = Utc::now(); list_volumes_between( &client, &bucket, id, now - TimeDelta::hours( h as i64), now).await? }
            None => list_volumes( &client, &bucket, id, date, None).await?
        };

        match get_station( id, &csv_stations) {
            Some(s) => println!("\n{id} ({}, {:.4},{:.4}): {} volumes", s.name, s.latitude, s.longitude, volumes.len()),
            None => println!("\n{id} (unknown position): {} volumes", volumes.len())
        }
        if !volumes.is_empty() {
            println!("  {:<20} {:<20} {:>7} {:>9}  {}", "scan start", "published", "delay", "size[MB]", "key");
            for v in &volumes { print_volume( v) }
        }
    }
    Ok(())
}

fn print_volume (v: &VolumeInfo) {
    let published = v.last_modified.map( |d| d.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or( "-".into());
    let delay = v.publication_delay().map( |d| format!("{}:{:02}", d.as_secs() / 60, d.as_secs() % 60)).unwrap_or( "-".into());
    println!("  {:<20} {:<20} {:>7} {:>9.2}  {}", v.scan_time.format("%Y-%m-%d %H:%M:%S"), published, delay, v.size as f64 / 1e6, v.key);
}
