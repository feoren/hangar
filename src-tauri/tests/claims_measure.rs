//! Measurement, not a check: how long the claims rescan's git calls take through `Git::run`
//! (fsmonitor off, optional locks off). Ignored by default; run it by hand with the folders to
//! measure, separated by `;`:
//!
//! ```text
//! HANGAR_MEASURE_REPOS="C:\path\a;C:\path\b" cargo test --release --test claims_measure -- --ignored --nocapture
//! ```
//!
//! The record of a run lives in docs/wiki/raw/ (see the claims measurement source page).

use std::path::Path;
use std::time::{Duration, Instant};

use hangar_lib::git::{filter_overrides, Git, MAX_LIST_BYTES};

const ROUNDS: usize = 7;

fn time_call(git: &Git, root: &Path, args: &[String]) -> (Duration, usize) {
    let start = Instant::now();
    let output = git
        .run(
            root,
            args,
            Instant::now() + Duration::from_secs(60),
            MAX_LIST_BYTES,
        )
        .expect("git call");
    assert!(output.usable(), "git {args:?} failed");
    (start.elapsed(), output.stdout.len())
}

fn report(label: &str, mut times: Vec<Duration>, bytes: usize) {
    times.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    println!(
        "  {label:<34} min {:>7.1} ms  median {:>7.1} ms  max {:>7.1} ms  ({bytes} bytes out)",
        ms(times[0]),
        ms(times[times.len() / 2]),
        ms(times[times.len() - 1])
    );
}

#[test]
#[ignore = "measurement; run by hand with HANGAR_MEASURE_REPOS"]
fn measure_rescan_calls() {
    let Ok(repos) = std::env::var("HANGAR_MEASURE_REPOS") else {
        eprintln!("HANGAR_MEASURE_REPOS not set");
        return;
    };
    let git = Git::locate().expect("git on PATH");
    let version = git
        .run(
            Path::new("."),
            ["version"],
            Instant::now() + Duration::from_secs(10),
            4096,
        )
        .expect("git version");
    println!("{}", String::from_utf8_lossy(&version.stdout).trim());
    for repo in repos.split(';').filter(|r| !r.is_empty()) {
        let root = Path::new(repo);
        let files = git
            .run(
                root,
                ["ls-files", "-z"],
                Instant::now() + Duration::from_secs(60),
                64 * 1024 * 1024,
            )
            .expect("ls-files");
        let count = files.stdout.iter().filter(|b| **b == 0).count();
        println!("{repo} ({count} tracked files)");
        let deadline = Instant::now() + Duration::from_secs(60);
        let neutral = filter_overrides(&git, root, deadline).expect("filter overrides");
        let with = |rest: &[&str]| -> Vec<String> {
            neutral
                .iter()
                .cloned()
                .chain(rest.iter().map(|s| (*s).to_owned()))
                .collect()
        };
        let calls: [(&str, Vec<String>); 4] = [
            (
                "config filters (filter_overrides)",
                vec![
                    "config".into(),
                    "-z".into(),
                    "--get-regexp".into(),
                    r"^filter\..*\.(clean|smudge|process)$".into(),
                ],
            ),
            (
                "rev-parse identity",
                vec![
                    "rev-parse".into(),
                    "--show-toplevel".into(),
                    "--git-dir".into(),
                    "--git-common-dir".into(),
                ],
            ),
            (
                "status --porcelain=v2 -z --branch",
                with(&[
                    "status",
                    "--porcelain=v2",
                    "-z",
                    "--branch",
                    "--no-renames",
                    "--untracked-files=normal",
                    "--ignore-submodules=all",
                ]),
            ),
            (
                "diff -U0 HEAD",
                with(&[
                    "diff",
                    "-U0",
                    "--no-color",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--no-renames",
                    "--ignore-submodules=all",
                    "HEAD",
                    "--",
                ]),
            ),
        ];
        for (label, args) in &calls {
            let mut times = Vec::new();
            let mut bytes = 0;
            for _ in 0..ROUNDS {
                let (time, out) = time_call(&git, root, args);
                times.push(time);
                bytes = out;
            }
            report(label, times, bytes);
        }
        // One whole rescan as the claims worker runs it: filters, status, diff in a row.
        let mut times = Vec::new();
        for _ in 0..ROUNDS {
            let start = Instant::now();
            let deadline = Instant::now() + Duration::from_secs(60);
            let _ = filter_overrides(&git, root, deadline).expect("filters");
            time_call(&git, root, &calls[2].1);
            time_call(&git, root, &calls[3].1);
            times.push(start.elapsed());
        }
        report("whole rescan (3 calls)", times, 0);
    }
}
