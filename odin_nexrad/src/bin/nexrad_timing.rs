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

//! scan interval and publication delay statistics of NEXRAD Level II stations, obtained from the AWS S3 listings.
//! Optionally downloads and decodes the latest volume of each station to show its volume coverage pattern (VCP),
//! sweeps and reflectivity encoding, and writes a RON list of station timings

use std::{time::Duration, fmt::Write as _};
use chrono::{TimeDelta, Utc};
use clap::Parser;
use anyhow::{Result, anyhow};
use tokio::task::JoinSet;
use odin_nexrad::{
    NexradConfig, DEFAULT_BUCKET, DEFAULT_REGION, VolumeInfo, VolumeTiming, Station,
    load_config, get_station, read_stations_csv, list_stations, list_volumes_between, download_volume, decode_volume
};
use odin_common::s3::{S3Client, create_s3_client};
use nexrad::model::data::{GateStatus, Product, Scan};

/// max number of concurrent station queries
const MAX_CONCURRENT: usize = 16;

#[derive(Parser, Debug)]
#[command(version, about, long_about = "show scan interval and publication delay statistics of NEXRAD Level II stations")]
struct Args {
    /// time window (hours before now) for the statistics
    #[arg(long, default_value_t = 3)]
    hours: u32,

    /// all stations that have volumes today (instead of given or configured stations)
    #[arg(long)]
    all: bool,

    /// download and decode the latest volume of each station (VCP, sweeps, reflectivity encoding)
    #[arg(long)]
    decode: bool,

    /// write station timings as RON list to this file
    #[arg(long)]
    ron: Option<String>,

    /// NexradConfig file (bucket, region, default stations, station CSV)
    #[arg(long, default_value="norcal_nexrad.ron")]
    config: String,

    /// S3 bucket (default from config or unidata-nexrad-level2)
    #[arg(long)]
    bucket: Option<String>,

    /// station ids (default: stations of config)
    station_ids: Vec<String>,
}

struct StationResult {
    id: String,
    timing: Option<VolumeTiming>,
    summary: Option<String>, // decoded latest volume
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
    let now = Utc::now();

    let ids: Vec<String> = if args.all {
        list_stations( &client, &bucket, now.date_naive()).await?
    } else if !args.station_ids.is_empty() {
        args.station_ids.clone()
    } else {
        config.as_ref().map( |c| c.stations.clone()).ok_or( anyhow!("no station ids given and no {} config", args.config))?
    };

    //--- query stations concurrently
    let since = now - TimeDelta::hours( args.hours as i64);
    let mut results: Vec<StationResult> = Vec::with_capacity( ids.len());
    let mut tasks: JoinSet<StationResult> = JoinSet::new();
    for id in ids {
        if tasks.len() >= MAX_CONCURRENT { if let Some(Ok(r)) = tasks.join_next().await { results.push(r) } }
        let (client, bucket, decode) = (client.clone(), bucket.clone(), args.decode);
        tasks.spawn( async move { query_station( client, bucket, id, since, now, decode).await });
    }
    while let Some(r) = tasks.join_next().await { if let Ok(r) = r { results.push(r) } }
    results.sort_by( |a,b| a.id.cmp( &b.id));

    //--- report
    println!("scan intervals and publication delays of the last {} hours (min/median/max, [m:ss])", args.hours);
    println!("{:<5} {:>4}  {:<19}  {:>6}  {:<17}  {:<17}  {:<8}  {}", "id", "#", "last scan", "last", "interval", "pub delay", "next", "name");
    for r in &results {
        let name = get_station( &r.id, &csv_stations).map( |s| s.name).unwrap_or_else( || "?".into());
        match &r.timing {
            Some(t) => {
                let next = t.next_expected( Duration::from_secs(300), Duration::from_secs(360));
                println!("{:<5} {:>4}  {:<19}  {:>6}  {:<17}  {:<17}  {:<8}  {}", r.id, t.count, t.last_scan.format("%Y-%m-%d %H:%M:%S"),
                    fmt_dur( t.last_interval), fmt_range( t.interval_min, t.interval_median, t.interval_max),
                    fmt_range( t.delay_min, t.delay_median, t.delay_max), next.format("%H:%M:%S"), name);
            }
            None => println!("{:<5} {:>4}  no volumes                                                                    {}", r.id, 0, name)
        }
        if let Some(summary) = &r.summary { print!("{summary}") }
    }

    if let Some(path) = &args.ron {
        std::fs::write( path, timings_ron( &results, &csv_stations, args.hours))?;
        println!("station timings written to {path}");
    }
    Ok(())
}

async fn query_station (client: S3Client, bucket: String, id: String, since: chrono::DateTime<Utc>, until: chrono::DateTime<Utc>, decode: bool)->StationResult {
    let volumes = match list_volumes_between( &client, &bucket, &id, since, until).await {
        Ok(vs) => vs,
        Err(e) => { eprintln!("failed to list {id}: {e}"); Vec::new() }
    };
    let timing = VolumeTiming::from_volumes( &volumes);

    let summary = if decode && let Some(latest) = volumes.last() {
        match download_volume( &client, &bucket, latest).await {
            Ok(data) => match tokio::task::spawn_blocking( move || decode_volume( data)).await {
                Ok(Ok(scan)) => Some( scan_summary( latest, &scan)),
                Ok(Err(e)) => Some( format!("      failed to decode {}: {e}\n", latest.name())),
                Err(e) => Some( format!("      decode task failed: {e}\n")),
            }
            Err(e) => Some( format!("      failed to download {}: {e}\n", latest.name()))
        }
    } else { None };

    StationResult { id, timing, summary }
}

/// VCP, sweeps and reflectivity encoding of a decoded volume
fn scan_summary (volume: &VolumeInfo, scan: &Scan)->String {
    let mut s = String::new();
    let _ = writeln!( s, "      {} ({:.1}MB): {}, {} sweeps", volume.name(), volume.size as f64 / 1e6, scan.coverage_pattern_number(), scan.sweeps().len());
    if let Some(site) = scan.site() {
        let _ = writeln!( s, "      site: {:.4},{:.4} elev {}m feedhorn {}m", site.latitude(), site.longitude(), site.site_elevation_meters(), site.feedhorn_height_meters());
    }

    let mut ref_values: Vec<f32> = Vec::new();
    for (i, sweep) in scan.sweeps().iter().enumerate() {
        let elev = sweep.elevation_angle_degrees().map( |e| format!("{e:.2}°")).unwrap_or( "?".into());
        match sweep.field( Product::Reflectivity) {
            Some(f) => {
                let _ = writeln!( s, "      sweep {i:2} elev {elev:>6}: REF {} radials x {} gates, gate {:.3}km, range {:.1}-{:.1}km",
                                  f.azimuth_count(), f.gate_count(), f.gate_interval_km(), f.first_gate_range_km(), f.max_range_km());
                ref_values.extend( f.values().iter().zip( f.statuses()).filter( |(_,st)| **st == GateStatus::Valid).map( |(v,_)| *v));
            }
            None => { let _ = writeln!( s, "      sweep {i:2} elev {elev:>6}: no REF"); }
        }
    }

    if !ref_values.is_empty() {
        let min = ref_values.iter().cloned().fold( f32::MAX, f32::min);
        let max = ref_values.iter().cloned().fold( f32::MIN, f32::max);
        let off_grid = ref_values.iter().filter( |v| ((**v * 2.0) - (**v * 2.0).round()).abs() > 1e-3).count();
        let _ = writeln!( s, "      REF: {} valid gates, {min:.1} .. {max:.1} dBZ, {off_grid} values not on 0.5dBZ grid", ref_values.len());
    }
    s
}

fn fmt_dur (d: Option<Duration>)->String {
    d.map( |d| format!("{}:{:02}", d.as_secs() / 60, d.as_secs() % 60)).unwrap_or( "-".into())
}

fn fmt_range (min: Option<Duration>, median: Option<Duration>, max: Option<Duration>)->String {
    format!("{}/{}/{}", fmt_dur(min), fmt_dur(median), fmt_dur(max))
}

/// RON list of station timings (seconds)
fn timings_ron (results: &[StationResult], csv_stations: &[Station], hours: u32)->String {
    let mut s = format!("// NEXRAD Level II station timings of the last {hours} hours, generated by nexrad_timing at {}\n// intervals and delays are [min, median, max] seconds\n[\n",
                        Utc::now().format("%Y-%m-%dT%H:%M:%SZ"));
    let secs = |d: Option<Duration>| d.map( |d| d.as_secs().to_string()).unwrap_or( "0".into());
    for r in results {
        let Some(t) = &r.timing else { continue };
        let st = get_station( &r.id, csv_stations);
        let (name, lat, lon) = st.map( |st| (st.name, st.latitude, st.longitude)).unwrap_or( ("?".into(), f64::NAN, f64::NAN));
        let _ = writeln!( s, "    ( id: \"{}\", name: \"{}\", lat: {lat:.4}, lon: {lon:.4}, volumes: {}, interval: [{}, {}, {}], delay: [{}, {}, {}] ),",
            r.id, name.replace('"', "'"), t.count, secs( t.interval_min), secs( t.interval_median), secs( t.interval_max),
            secs( t.delay_min), secs( t.delay_median), secs( t.delay_max));
    }
    s.push_str("]\n");
    s
}
