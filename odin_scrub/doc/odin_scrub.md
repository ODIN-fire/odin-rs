# odin_scrub

`odin_scrub` is a system crate that periodically deletes aged files and directories within a configured file tree,
typically the ODIN cache directory (`$ODIN_ROOT/cache`). Many ODIN crates store retrieved or computed data in their
cache directories, and without a periodic clean up these directories grow without bounds. Instead of having each crate
implement its own clean up (with its own timer that wakes up respective actor tasks) we factor this out into a single,
configurable scrubber.

The scrubber runs in a native thread (not a tokio task) since it has to traverse a potentially large file tree, and
we do not want this to interfere with tokio task scheduling. The thread can be given an (OS specific) priority. The
scrubber can either be started from within an ODIN application or run as a separate process with the `run_scrubber`
tool.


## Configuration

The scrubber is configured with a `ScrubConfig` (by default `scrub.ron`):

```rust
ScrubConfig (
    priority: Some(0),                            // optional OS thread priority [0..99]
    log_file: Some("$ODIN_ROOT/scrub_log.csv"),   // optional CSV log of deleted entries

    root_dir: "$ODIN_ROOT/cache",                 // the scrubber never leaves this dir
    scrub_interval: Duration(secs: 600,nanos: 0), // check every 10min
    default_max_age: Duration(secs: 28800, nanos: 0), // delete after 8h unless the matcher specifies otherwise
    show_stats: true,                             // optional - print file statistics after each cycle (default false)

    matchers: [ // ordered from global to specific
        Matcher( glob: "odin_*/**/*", exclude: ["odin_elmfire/*"], max_age: None),
        Matcher( glob: "odin_hrrr/**", max_age: Some( Duration(secs: 3600, nanos: 0))),
    ]
)
```

Paths are `EnvPathBuf` values, i.e. they can contain environment variables. `$ODIN_ROOT` is automatically set to the
ODIN root directory if it is not defined in the environment.

Each `Matcher` has a `glob` pattern, an optional list of `exclude` glob patterns and an optional `max_age` (if not set
the `default_max_age` of the config is used). All patterns are relative to `root_dir`. Patterns follow the
[globset](https://docs.rs/globset) syntax, with the important detail that `*` also matches path separators, i.e.
`odin_elmfire/*` matches *all* entries below `odin_elmfire/`, not only its direct children. Use `{a,b}` for alternatives
and `[..]` for character classes.

The order of matchers is significant - they have to be specified from global to specific. The scrubber checks
matchers in reverse order and uses the first one whose `glob` matches the path of a file or directory (i.e. the most
specific one). The index of this matcher (0-based, in config order) is recorded in the log file.


## Scrub Algorithm

Each `scrub_interval` the scrubber traverses `root_dir` recursively. For each file or directory it determines the
matcher and then decides:

- if there is no matching matcher, the entry is kept (but see directories below)
- if any of the matcher's `exclude` patterns matches, the entry is kept
- if the entry's file time is older than the current time minus `max_age` it is deleted, otherwise it is kept

The *file time* of a file is the later of its `accessed` time (if supported and enabled on the platform/file system) and
its `modified` time. For directories only the `modified` time is used since our own traversal updates the `accessed`
time of directories.

Directories are processed bottom-up and a matching directory is deleted recursively (including all its remaining
entries) if

- the newest file time of all its remaining entries (and of the directory itself) is older than the cut off of its
  matcher, and
- none of its remaining entries was kept because of its own matcher (i.e. excluded or not old enough).

This means that a recently used file or an excluded file protects all its parent directories. Entries that do not match
any matcher are covered by the matcher of their enclosing directory - a pattern such as `runs/run_[0-9]*` can be used to
delete complete (aged) run directories regardless of their content.

Symbolic links are never followed - a matching link is deleted like a file. The `root_dir` itself and the `log_file`
are never deleted.

If a `log_file` is configured each deletion is recorded as a CSV line:

```
deletion_time,file_time,matcher,path
2026-11-08T17:05:12Z,2026-11-08T08:31:44Z,0,"/Users/me/odin/cache/odin_hrrr/hrrr-wrfsfcf01.grib2"
```

If `show_stats` is set the scrubber prints a summary line at the end of each scrub cycle that includes the cycle
number, its start time, number and total size of deleted files, number of deleted directories, number and total size
of live (remaining) files within `root_dir`, and the number of errors:

```
scrub cycle 12 at 2026-11-09T10:15:00Z: deleted 23 files (1.2 MiB) and 2 dirs, live 1834 files (2.3 GiB), 0 errors, scan took 6.103s
```

Since this requires to get the metadata of all entries (also the ones that are not covered by any matcher) it makes
scrub cycles slower and should mostly be used to monitor and tune scrubber configurations.

Note that the log file is not scrubbed itself - it should be placed outside of `root_dir` and rotated by other means if
required.


## Using the Scrubber within ODIN Applications

To make sure there is a scrubber running within an application process use `start_scrubber(..)`:

```rust
use odin_scrub::{start_scrubber, load_config};
...
start_scrubber( load_config("scrub.ron")?)?;
```

This spawns the scrubber thread and returns. Errors during a scrub pass (e.g. entries that cannot be deleted) are
reported as warnings and do not terminate the scrubber. There is also `scrub_once(&config, dry_run)` to execute a single
scrub pass synchronously.

Since all ODIN processes share the same cache directory there should only be one scrubber per `root_dir` - either
started by one of the applications or as a separate `run_scrubber` process.


## `run_scrubber`

The `run_scrubber` tool runs the scrubber loop in a separate process:

```shell
$> cargo run --release --bin run_scrubber [-- [--dry-run] [<config>]]
```

The optional `<config>` argument is either the path of a `ScrubConfig` file or the name of a config that is looked up
through the normal [odin_build](../odin_build/odin_build.md) config mechanism (default is `scrub.ron`).

With the `--dry-run` option `run_scrubber` executes a single scrub pass that only prints the entries that would be
deleted (with matcher index, age and size), followed by a summary (including live files if `show_stats` is set). Nothing is deleted or logged. This should be used to
check new or modified configs before they are deployed.

Logging output of the scrubber loop can be controlled with the standard `RUST_LOG` environment variable (default is
`info`, which reports each scrub pass that deleted entries).
