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

//! system crate to do periodic background cleanup of a file tree based on a `ScrubConfig`.
//! The rationale is to factor out the common periodic file system clean up so that we don't have to
//! implement it in all odin crates that make use of their filesystem CACHE. This is not only to avoid redundant code
//! but also to reduce the number of timers that would cause wakeup and scheduling of respective tokio actor tasks.
//!
//! The scrubber periodically traverses the configured `root_dir` recursively in the configured `scrub_interval`
//! and checks each file or dir in this tree against an ordered list of `Matcher` entries. We treat files and directories
//! uniformly (i.e. matching directories are recursively deleted).
//!
//! If the file path matches the `Matcher.glob` we check if any of the `Matcher.exclude` glob patterns prevents it
//! from being scrubbed. If there is no `exclude` glob specified or none does match the path then the `accessed`
//! time (if supported by the platform) and the `modified` time of the corresponding `File` is checked against a cut off
//! that is computed from the current `SystemTime` minus either (a) an optional `Matcher.max_age` or (b) if not
//! specified the configured `ScrubConfig.default_max_age`. If the respective file time is older than the cut off
//! time the file is deleted. If the config included a `log_file` option the deletion is recorded with deletion time,
//! file time, matcher number and pathname (in CSV format).
//!
//! The scrubber is confined to the configured `root_dir`, specified as an `EnvPathBuf` that allows the use of
//! environment variables (esp. including ODIN_ROOT). All given glob patterns are relative to this `root_dir`.
//!
//! The order of specified Matchers in the config file matters and has to be from global to specific. The match
//! process iterates over the list of Matchers in reverse and stops at the first match (hence the most global pattern
//! should be evaluated last)
//!
//! We do execute the `run_scrubber_loop()` function in a native thread and not as a (tokio) task since this is a
//! background process that has to traverse a potentially large file tree and we want minimal impact on tokio task
//! scheduling. To this end we want to be able to give this thread an optional priority. We also provide a `run_scrubber`
//! command line application to run the scrubber loop in a separate process.
//!
//! Implementation details:
//! - the file time of a file is the later of its `accessed` and `modified` times. For directories we only use the
//!   `modified` time since our own traversal updates the `accessed` time of directories
//! - directories are processed bottom-up. A matching directory is only deleted if the newest file time within its
//!   (remaining) sub-tree is older than the cut off of its matcher, and if none of its remaining entries was kept
//!   because of its own matcher (excluded or not old enough). Entries that do not match any matcher are covered by the
//!   matcher of their enclosing directory
//! - symbolic links are never followed (a matching link is deleted as a file)
//! - the `root_dir` itself and the `log_file` are never deleted
//! - entries that are neither matched nor below a matched dir are not `stat`ed (we don't need their times or sizes)
//!   unless `show_stats` is set, in which case we print the number and size of live (remaining) and deleted files
//!   at the end of each scrub cycle

#![allow(unused)]

use std::{fs::{self,File,FileType,Metadata,OpenOptions}, io::{self,Write}, path::{Path,PathBuf}, thread::JoinHandle, time::{Duration,Instant,SystemTime}};
use anyhow::{anyhow,Result};
use chrono::{DateTime,SecondsFormat,Utc};
use serde::{Serialize,Deserialize};
use globset::{Glob,GlobMatcher};
use thread_priority::{ThreadBuilder,ThreadPriority,ThreadPriorityValue};
use tracing::{debug,info,warn};

use odin_build::define_load_config;
use odin_macro::public_struct;
use odin_common::fs::EnvPathBuf;

define_load_config!{}

#[derive(Deserialize,Debug)]
#[public_struct]
struct Matcher {
    glob: Glob,
    #[serde(default)]
    exclude: Vec<Glob>,
    max_age: Option<Duration>
}

#[derive(Deserialize,Debug)]
#[public_struct]
struct ScrubConfig {
    priority: Option<u8>,         // OS thread priority

    #[serde(default)]
    log_file: Option<EnvPathBuf>,

    root_dir: EnvPathBuf,
    scrub_interval: Duration, // how often to check
    default_max_age: Duration, // to be used if the Matcher does not have an optional `max_age`

    #[serde(default)]
    show_stats: bool, // print live and deleted file statistics at the end of each scrub cycle (requires stat of all entries)

    matchers: Vec<Matcher>
}

/// summary of a single scrub pass
#[derive(Debug,Default,Clone)]
pub struct ScrubStats {
    pub n_files: usize,    // number of deleted files (including symlinks)
    pub n_dirs: usize,     // number of deleted directories
    pub n_bytes: u64,      // deleted bytes
    pub n_errors: usize,   // number of entries we could not process or delete
    pub elapsed: Duration,

    // only computed if `show_stats` is set in the ScrubConfig
    pub n_live_files: usize, // number of remaining files (including symlinks)
    pub n_live_bytes: u64,   // remaining bytes
}

impl ScrubStats {
    /// one line summary of this scrub pass, including live files if they were computed
    pub fn summary (&self, show_live: bool)->String {
        let mut s = format!("deleted {} files ({}) and {} dirs", self.n_files, fmt_bytes(self.n_bytes), self.n_dirs);
        if show_live {
            s.push_str( &format!(", live {} files ({})", self.n_live_files, fmt_bytes(self.n_live_bytes)));
        }
        s.push_str( &format!(", {} errors, scan took {:.3}s", self.n_errors, self.elapsed.as_secs_f64()));
        s
    }
}

/// public interface to start the scrubber thread. This can be used explicitly from within ODIN applications
/// to ensure there is a scrubber running (within the process).
/// For running the scrubber in a separate process please see bin/run_scrubber.rs
pub fn start_scrubber(conf: ScrubConfig)->Result<()> {
    spawn_scrubber( conf)?;
    Ok(())
}

/// spawn the scrubber thread (with the configured priority) and return its JoinHandle. The thread only terminates
/// if the scrubber loop fails
pub fn spawn_scrubber (conf: ScrubConfig)->Result<JoinHandle<Result<()>>> {
    let mut builder = ThreadBuilder::default().name("odin-scrubber");
    if let Some(prio) = conf.priority {
        let value = ThreadPriorityValue::try_from( prio).map_err( |e| anyhow!("invalid scrubber priority {prio}: {e}"))?;
        builder = builder.priority( ThreadPriority::Crossplatform( value));
    }

    let jh = builder.spawn( move |prio_result| {
        if let Err(e) = prio_result { // not fatal - we just run with default priority
            warn!("could not set scrubber thread priority: {e:?}");
        }
        run_scrubber_loop( conf)
    })?;

    Ok(jh)
}

fn run_scrubber_loop(conf: ScrubConfig) ->Result<()> {
    let scrubber = Scrubber::new( &conf)?;
    println!("scrubber started for {:?} with interval {:?}", conf.root_dir, conf.scrub_interval);

    for cycle in 1.. {
        let t_start = SystemTime::now();
        let stats = scrubber.scrub( false);

        if conf.show_stats {
            println!("scrub cycle {cycle} at {}: {}", fmt_time(t_start), stats.summary(true));
        }

        if stats.n_files + stats.n_dirs > 0 || stats.n_errors > 0 {
            info!("scrubbed {} files and {} dirs ({} bytes, {} errors) in {:?}",
                  stats.n_files, stats.n_dirs, stats.n_bytes, stats.n_errors, stats.elapsed);
        } else {
            debug!("nothing to scrub ({:?})", stats.elapsed);
        }
        std::thread::sleep( conf.scrub_interval);
    }
    Ok(())
}

/// execute a single scrub pass. If `dry_run` is set nothing is deleted or logged - we only print the entries
/// that would be deleted to stdout. This is mostly for testing configs (see `run_scrubber --dry-run`)
pub fn scrub_once (conf: &ScrubConfig, dry_run: bool)->Result<ScrubStats> {
    let scrubber = Scrubber::new( conf)?;
    Ok( scrubber.scrub( dry_run))
}

//--- internal implementation

/// the result of processing an entry
enum Entry {
    Deleted,
    Kept { newest: SystemTime, size: u64, count: usize, by_matcher: bool } // count: number of remaining files in sub-tree
}

/// aggregate of the remaining (not deleted) entries of a directory
struct Remaining {
    newest: SystemTime, // newest file time
    size: u64,          // bytes
    count: usize,       // number of files
    by_matcher: bool    // did any entry get kept because of its matcher (excluded or not old enough)
}

struct CompiledMatcher {
    glob: GlobMatcher,
    exclude: Vec<GlobMatcher>,
    max_age: Duration
}

/// the compiled, immutable part of a ScrubConfig
struct Scrubber {
    root_dir: PathBuf,
    log_file: Option<PathBuf>,
    show_stats: bool,
    matchers: Vec<CompiledMatcher>
}

/// the mutable state of a single scrub pass
struct Pass<'a> {
    cutoff_base: SystemTime,
    dry_run: bool,
    log: Option<File>,
    stats: &'a mut ScrubStats
}

impl Scrubber {
    fn new (conf: &ScrubConfig)->Result<Self> {
        let matchers = conf.matchers.iter().map( |m| CompiledMatcher {
            glob: m.glob.compile_matcher(),
            exclude: m.exclude.iter().map( |g| g.compile_matcher()).collect(),
            max_age: m.max_age.unwrap_or( conf.default_max_age)
        }).collect();

        Ok( Scrubber {
            root_dir: conf.root_dir.0.clone(),
            log_file: conf.log_file.as_ref().map( |p| p.0.clone()),
            show_stats: conf.show_stats,
            matchers
        })
    }

    fn scrub (&self, dry_run: bool)->ScrubStats {
        let t0 = Instant::now();
        let mut stats = ScrubStats::default();

        if self.root_dir.is_dir() {
            let log = if dry_run { None } else { self.open_log() };
            let mut pass = Pass { cutoff_base: SystemTime::now(), dry_run, log, stats: &mut stats };

            // root_dir itself is never deleted
            if let Some(rem) = self.process_children( &mut pass, &self.root_dir, false) {
                if self.show_stats {
                    stats.n_live_files = rem.count;
                    stats.n_live_bytes = rem.size;
                }
            }
        } else {
            debug!("scrubber root dir {:?} does not exist (yet)", self.root_dir);
        }

        stats.elapsed = t0.elapsed();
        stats
    }

    fn open_log (&self)->Option<File> {
        if let Some(path) = &self.log_file {
            let is_new = !path.exists();
            match OpenOptions::new().create(true).append(true).open( path) {
                Ok(mut file) => {
                    if is_new { let _ = writeln!( file, "deletion_time,file_time,matcher,path"); }
                    Some(file)
                }
                Err(e) => { warn!("failed to open scrubber log file {path:?}: {e}"); None }
            }
        } else {
            None
        }
    }

    /// process all entries of a directory and return the aggregate of the remaining ones (None if dir can't be read).
    /// `covered` means there is a matching ancestor dir, i.e. the times and sizes of unmatched entries matter
    fn process_children (&self, pass: &mut Pass, dir: &Path, covered: bool)->Option<Remaining> {
        let mut rem = Remaining { newest: SystemTime::UNIX_EPOCH, size: 0, count: 0, by_matcher: false };

        match fs::read_dir( dir) {
            Ok(entries) => {
                for e in entries {
                    let res = e.and_then( |e| Ok( (e.path(), e.file_type()?))); // file_type does not follow symlinks
                    match res {
                        Ok((path,ftype)) => {
                            if let Entry::Kept{ newest, size, count, by_matcher } = self.process( pass, &path, ftype, covered) {
                                if newest > rem.newest { rem.newest = newest; }
                                rem.size += size;
                                rem.count += count;
                                rem.by_matcher |= by_matcher;
                            }
                        }
                        Err(e) => { // don't delete what we can't see
                            warn!("failed to read entry in {dir:?}: {e}");
                            pass.stats.n_errors += 1;
                            rem.by_matcher = true;
                        }
                    }
                }
                Some(rem)
            }
            Err(e) => {
                warn!("failed to read dir {dir:?}: {e}");
                pass.stats.n_errors += 1;
                None
            }
        }
    }

    /// process a file system entry (recursively for directories) and return if it was deleted or kept
    fn process (&self, pass: &mut Pass, path: &Path, ftype: FileType, covered: bool)->Entry {
        if self.log_file.as_deref() == Some(path) { // never scrub our own log
            return Entry::Kept { newest: SystemTime::now(), size: 0, count: 1, by_matcher: true }
        }

        let rel_path = path.strip_prefix( &self.root_dir).unwrap_or( path);
        let matcher = self.find_matcher( rel_path);

        if matcher.is_none() && !covered && !self.show_stats { // nobody uses time or size of this entry - avoid the metadata syscall
            return if ftype.is_dir() {
                let by_matcher = self.process_children( pass, path, false).map( |rem| rem.by_matcher).unwrap_or(true);
                Entry::Kept { newest: SystemTime::UNIX_EPOCH, size: 0, count: 0, by_matcher }
            } else {
                Entry::Kept { newest: SystemTime::UNIX_EPOCH, size: 0, count: 0, by_matcher: false }
            }
        }

        let meta = match fs::symlink_metadata( path) { // don't follow symlinks
            Ok(meta) => meta,
            Err(e) => { // could have been deleted by its owner in the meantime
                if e.kind() != io::ErrorKind::NotFound { warn!("failed to get metadata of {path:?}: {e}"); pass.stats.n_errors += 1; }
                return Entry::Deleted
            }
        };

        if ftype.is_dir() {
            self.process_dir( pass, path, rel_path, &meta, matcher)
        } else {
            self.process_file( pass, path, rel_path, &meta, matcher)
        }
    }

    fn process_file (&self, pass: &mut Pass, path: &Path, rel_path: &Path, meta: &Metadata, matcher: Option<(usize,&CompiledMatcher)>)->Entry {
        let file_time = file_time( meta, false);
        let size = meta.len();

        if let Some((idx,m)) = matcher {
            if !is_excluded( m, rel_path) && is_older( file_time, pass.cutoff_base, m.max_age) {
                if self.delete( pass, path, false, file_time, idx, size) {
                    pass.stats.n_files += 1;
                    pass.stats.n_bytes += size;
                    return Entry::Deleted
                }
            }
            Entry::Kept { newest: file_time, size, count: 1, by_matcher: true }

        } else {
            Entry::Kept { newest: file_time, size, count: 1, by_matcher: false }
        }
    }

    fn process_dir (&self, pass: &mut Pass, path: &Path, rel_path: &Path, meta: &Metadata, matcher: Option<(usize,&CompiledMatcher)>)->Entry {
        // get the dir time before we process (and possibly delete) children, which would update it
        let dir_time = file_time( meta, true);

        let Some(rem) = self.process_children( pass, path, true) else { // don't delete what we can't see
            return Entry::Kept { newest: SystemTime::now(), size: 0, count: 0, by_matcher: true }
        };
        let newest = rem.newest.max( dir_time);

        if let Some((idx,m)) = matcher {
            if !rem.by_matcher && !is_excluded( m, rel_path) && is_older( newest, pass.cutoff_base, m.max_age) {
                if self.delete( pass, path, true, newest, idx, rem.size) {
                    pass.stats.n_dirs += 1;
                    pass.stats.n_files += rem.count; // remaining (unmatched) files get deleted with the dir
                    pass.stats.n_bytes += rem.size;
                    return Entry::Deleted
                }
            }
            Entry::Kept { newest, size: rem.size, count: rem.count, by_matcher: true }

        } else {
            Entry::Kept { newest, size: rem.size, count: rem.count, by_matcher: rem.by_matcher }
        }
    }

    /// matchers are specified from global to specific, the most specific match wins
    fn find_matcher (&self, rel_path: &Path)->Option<(usize,&CompiledMatcher)> {
        self.matchers.iter().enumerate().rev().find( |(_,m)| m.glob.is_match( rel_path))
    }

    /// delete file or dir, returns true if the entry was deleted (or would be in a dry run)
    fn delete (&self, pass: &mut Pass, path: &Path, is_dir: bool, file_time: SystemTime, matcher_idx: usize, size: u64)->bool {
        let now = SystemTime::now();

        if pass.dry_run {
            let age = now.duration_since( file_time).unwrap_or_default().as_secs();
            println!("would delete {}{} (matcher {matcher_idx}, age {}h{:02}m, {size} bytes)",
                     path.display(), if is_dir {"/"} else {""}, age / 3600, (age % 3600) / 60);
            return true
        }

        let res = if is_dir { fs::remove_dir_all( path) } else { fs::remove_file( path) };
        match res {
            Ok(_) => {
                if let Some(log) = pass.log.as_mut() {
                    let _ = writeln!( log, "\"{}\",\"{}\",{},\"{}\"", fmt_time(now), fmt_time(file_time), matcher_idx, path.display());
                }
                true
            }
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound { return true } // somebody else was faster
                warn!("failed to delete {path:?}: {e}");
                pass.stats.n_errors += 1;
                false
            }
        }
    }
}

/// the time of last use. We don't use access times for directories since our own traversal updates them
fn file_time (meta: &Metadata, is_dir: bool)->SystemTime {
    let modified = meta.modified().unwrap_or( SystemTime::now()); // if we can't tell it's not old
    if is_dir {
        modified
    } else {
        match meta.accessed() {
            Ok(accessed) if accessed > modified => accessed,
            _ => modified
        }
    }
}

fn is_excluded (m: &CompiledMatcher, rel_path: &Path)->bool {
    m.exclude.iter().any( |g| g.is_match( rel_path))
}

fn is_older (file_time: SystemTime, now: SystemTime, max_age: Duration)->bool {
    match now.checked_sub( max_age) {
        Some(cutoff) => file_time < cutoff,
        None => false
    }
}

/// human readable byte size (binary prefixes)
pub fn fmt_bytes (n: u64)->String {
    const UNITS: [&str;5] = ["KiB", "MiB", "GiB", "TiB", "PiB"];
    if n < 1024 { return format!("{n} B") }

    let mut v = n as f64 / 1024.0;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len()-1 { v /= 1024.0; i += 1; }
    format!("{v:.1} {}", UNITS[i])
}

fn fmt_time (t: SystemTime)->String {
    DateTime::<Utc>::from(t).to_rfc3339_opts( SecondsFormat::Secs, true)
}
