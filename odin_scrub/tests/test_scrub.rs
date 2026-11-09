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

use std::{fs::{self,File,FileTimes,OpenOptions}, path::{Path,PathBuf}, time::{Duration,SystemTime}};
use globset::Glob;
use odin_common::fs::EnvPathBuf;
use odin_scrub::{Matcher, ScrubConfig, scrub_once};

const HOUR: u64 = 3600;

/// a scratch dir that is removed when dropped
struct TestDir(PathBuf);

impl TestDir {
    fn new (name: &str)->Self {
        let path = std::env::temp_dir().join( format!("odin_scrub_test_{name}_{}", std::process::id()));
        if path.exists() { fs::remove_dir_all( &path).unwrap(); }
        fs::create_dir_all( &path).unwrap();
        TestDir(path)
    }

    /// create file with given age in hours (both accessed and modified time)
    fn file (&self, rel_path: &str, age_hours: u64)->PathBuf {
        let path = self.0.join( rel_path);
        if let Some(parent) = path.parent() { fs::create_dir_all( parent).unwrap(); }
        fs::write( &path, b"0123456789").unwrap();
        set_age( &path, age_hours);
        path
    }

    /// set dir age - has to be called after all entries of the dir have been created
    fn dir (&self, rel_path: &str, age_hours: u64)->PathBuf {
        let path = self.0.join( rel_path);
        fs::create_dir_all( &path).unwrap();
        set_age( &path, age_hours);
        path
    }

    fn exists (&self, rel_path: &str)->bool {
        self.0.join( rel_path).exists()
    }
}

impl Drop for TestDir {
    fn drop (&mut self) {
        let _ = fs::remove_dir_all( &self.0);
    }
}

fn set_age (path: &Path, age_hours: u64) {
    let t = SystemTime::now() - Duration::from_secs( age_hours * HOUR);
    let file = if path.is_dir() { File::open( path) } else { OpenOptions::new().write(true).open( path) }.unwrap();
    file.set_times( FileTimes::new().set_accessed(t).set_modified(t)).unwrap();
}

fn matcher (glob: &str, exclude: &[&str], max_age_hours: Option<u64>)->Matcher {
    Matcher {
        glob: Glob::new( glob).unwrap(),
        exclude: exclude.iter().map( |g| Glob::new(g).unwrap()).collect(),
        max_age: max_age_hours.map( |h| Duration::from_secs( h * HOUR))
    }
}

fn config (td: &TestDir, matchers: Vec<Matcher>)->ScrubConfig {
    ScrubConfig {
        priority: None,
        log_file: None,
        root_dir: EnvPathBuf( td.0.clone()),
        scrub_interval: Duration::from_secs( 600),
        default_max_age: Duration::from_secs( 8 * HOUR),
        show_stats: false,
        matchers
    }
}

#[test]
fn test_file_age () {
    let td = TestDir::new( "file_age");
    td.file( "a/old.txt", 10);
    td.file( "a/young.txt", 1);
    td.dir( "a", 1);

    let stats = scrub_once( &config( &td, vec![ matcher( "a/*", &[], None)]), false).unwrap();
    assert!( !td.exists( "a/old.txt"));
    assert!( td.exists( "a/young.txt"));
    assert!( td.exists( "a"));
    assert_eq!( stats.n_files, 1);
    assert_eq!( stats.n_bytes, 10);
}

#[test]
fn test_exclude () {
    let td = TestDir::new( "exclude");
    td.file( "a/old.txt", 10);
    td.file( "a/keep.dat", 10);
    td.file( "b/old.txt", 10);

    let conf = config( &td, vec![ matcher( "{a,b}/*", &["*.dat", "b/*"], None)]);
    scrub_once( &conf, false).unwrap();
    assert!( !td.exists( "a/old.txt"));
    assert!( td.exists( "a/keep.dat"));
    assert!( td.exists( "b/old.txt"));
}

#[test]
fn test_matcher_order () {
    let td = TestDir::new( "matcher_order");
    td.file( "a/x.txt", 10);
    td.file( "a/special/y.txt", 10);
    td.dir( "a/special", 10);
    td.dir( "a", 1);

    // the specific (last) matcher has a longer max_age and wins for a/special
    let conf = config( &td, vec![ matcher( "a/**", &[], None), matcher( "a/special/**", &[], Some(24))]);
    scrub_once( &conf, false).unwrap();
    assert!( !td.exists( "a/x.txt"));
    assert!( td.exists( "a/special/y.txt"));
    assert!( td.exists( "a/special"));
}

#[test]
fn test_dir_deletion () {
    let td = TestDir::new( "dir_deletion");
    td.file( "runs/r1/out/data.bin", 10);
    td.file( "runs/r1/log.txt", 10);
    td.dir( "runs/r1/out", 10);
    td.dir( "runs/r1", 10);
    td.file( "runs/r2/data.bin", 10);
    td.file( "runs/r2/current.bin", 1); // keeps r2 alive
    td.dir( "runs/r2", 10);
    td.dir( "runs", 1);

    // only run dirs match - their content is covered by the dir matcher
    let conf = config( &td, vec![ matcher( "runs/r[0-9]", &[], None)]);
    let stats = scrub_once( &conf, false).unwrap();
    assert!( !td.exists( "runs/r1"));
    assert!( td.exists( "runs/r2/data.bin")); // not matched itself
    assert!( td.exists( "runs/r2/current.bin"));
    assert!( td.exists( "runs"));
    assert_eq!( stats.n_dirs, 1);
    assert_eq!( stats.n_files, 2);
}

#[test]
fn test_kept_entries_protect_dir () {
    let td = TestDir::new( "protect_dir");
    td.file( "d/old.txt", 10);
    td.file( "d/keep.dat", 10);
    td.dir( "d", 10);

    // d itself is old and matches, but it contains an excluded file
    let conf = config( &td, vec![ matcher( "**", &["**/*.dat"], None)]);
    scrub_once( &conf, false).unwrap();
    assert!( !td.exists( "d/old.txt"));
    assert!( td.exists( "d/keep.dat"));
    assert!( td.exists( "d"));
}

#[test]
fn test_dry_run () {
    let td = TestDir::new( "dry_run");
    td.file( "d/old.txt", 10);
    td.dir( "d", 10);

    let stats = scrub_once( &config( &td, vec![ matcher( "**", &[], None)]), true).unwrap();
    assert!( td.exists( "d/old.txt"));
    assert!( td.exists( "d"));
    assert_eq!( stats.n_files, 1); // what would have been deleted
    assert_eq!( stats.n_dirs, 1);
}

#[test]
fn test_log_file () {
    let td = TestDir::new( "log_file");
    td.file( "cache/old.txt", 10);
    td.dir( "cache", 1);
    let log_path = td.0.join( "scrub_log.csv");

    let mut conf = config( &td, vec![ matcher( "**/*.txt", &[], None)]);
    conf.root_dir = EnvPathBuf( td.0.join( "cache"));
    conf.log_file = Some( EnvPathBuf( log_path.clone()));
    scrub_once( &conf, false).unwrap();

    let log = fs::read_to_string( &log_path).unwrap();
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!( lines.len(), 2);
    assert_eq!( lines[0], "deletion_time,file_time,matcher,path");
    assert!( lines[1].contains( ",0,\"") && lines[1].ends_with( "old.txt\""));
}

#[test]
fn test_unmatched_and_root () {
    let td = TestDir::new( "unmatched");
    td.file( "other/old.txt", 10);
    td.file( "old.txt", 10);

    let conf = config( &td, vec![ matcher( "cache/**", &[], None)]);
    scrub_once( &conf, false).unwrap();
    assert!( td.exists( "other/old.txt"));
    assert!( td.exists( "old.txt"));
    assert!( td.0.exists());
}

#[test]
fn test_parse_config () {
    let src = r#"ScrubConfig (
        priority: Some(0),
        log_file: Some("$HOME/scrub_log.csv"),
        root_dir: "$HOME/cache",
        scrub_interval: Duration(secs: 600, nanos: 0),
        default_max_age: Duration(secs: 28800, nanos: 0),
        matchers: [
            Matcher( glob: "odin_*/**/*", exclude: ["odin_elmfire/*"], max_age: None),
            Matcher( glob: "odin_hrrr/**", max_age: Some(Duration(secs: 3600, nanos: 0))),
        ]
    )"#;
    let conf: ScrubConfig = ron::from_str( src).unwrap();
    assert_eq!( conf.matchers.len(), 2);
    assert!( conf.matchers[1].exclude.is_empty());
    assert!( !conf.root_dir.to_string_lossy().contains( "$HOME"));
    assert!( !conf.show_stats); // optional, defaults to false
}

#[test]
fn test_show_stats () {
    let td = TestDir::new( "show_stats");
    td.file( "a/old.txt", 10);
    td.file( "a/young.txt", 1);
    td.dir( "a", 1);
    td.file( "other/x.txt", 10); // unmatched and uncovered - only counted with show_stats
    td.file( "other/sub/y.txt", 10);

    let mut conf = config( &td, vec![ matcher( "a/*", &[], None)]);
    let stats = scrub_once( &conf, false).unwrap();
    assert_eq!( stats.n_files, 1);
    assert_eq!( stats.n_live_files, 0); // not computed

    td.file( "a/old.txt", 10);
    conf.show_stats = true;
    let stats = scrub_once( &conf, false).unwrap();
    assert_eq!( stats.n_files, 1);
    assert_eq!( stats.n_bytes, 10);
    assert_eq!( stats.n_live_files, 3);
    assert_eq!( stats.n_live_bytes, 30);
    assert!( td.exists( "other/sub/y.txt"));
    println!("{}", stats.summary(true));
}
