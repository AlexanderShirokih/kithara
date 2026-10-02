#![cfg(unix)]

use std::{env, fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

use anyhow::{Context, Result};

fn executable(path: &Path, source: &str) -> Result<()> {
    fs::write(path, source)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn run(args: &[&str], fail: &str) -> Result<(std::process::Output, Vec<String>)> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let bin = root.join("bin");
    let trace = root.join("trace");
    fs::create_dir_all(&bin)?;
    fs::create_dir_all(root.join(".config"))?;
    fs::write(root.join("Cargo.toml"), "[workspace]\n")?;
    let shipped = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("workspace root")?;
    for config in ["xtask.toml", "nextest.toml"] {
        fs::copy(
            shipped.join(".config").join(config),
            root.join(".config").join(config),
        )?;
    }
    let cargo = env::var("CARGO").context("Cargo supplies its path to tests")?;
    executable(
        &bin.join("cargo"),
        &format!(
            "#!/bin/sh\nif [ \"$1\" = metadata ]; then exec '{cargo}' \"$@\"; fi\nprintf 'cargo %s\\n' \"$*\" >> \"$UI_TRACE\"\n"
        ),
    )?;
    executable(
        &bin.join("just"),
        "#!/bin/sh\nprintf 'just %s\\n' \"$*\" >> \"$UI_TRACE\"\ncase \"$*\" in *\"$UI_FAIL\"*) if [ -n \"$UI_FAIL\" ]; then exit 7; fi;; esac\n",
    )?;
    let mut paths = vec![bin];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(args)
        .current_dir(root)
        .env("PATH", env::join_paths(paths)?)
        .env("CARGO", &cargo)
        .env("UI_TRACE", &trace)
        .env("UI_FAIL", fail)
        .output()?;
    let calls = fs::read_to_string(trace)?
        .lines()
        .map(str::to_owned)
        .collect();
    Ok((output, calls))
}

#[test]
fn ui_acceptance_runs_each_feature_set_once() -> Result<()> {
    let (output, calls) = run(&["ui", "--dir", "pictures"], "")?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        &calls[..3],
        [
            "just test run --lane=ui",
            "just test run --lane=ui-doc",
            "just test run --lane=ui-perf",
        ]
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.contains("--test gallery"))
            .count(),
        1
    );
    assert!(
        !calls.iter().any(|call| call.contains("--test ui_memory")),
        "the UI lane already owns the isolated memory test"
    );
    assert!(
        calls.iter().any(|call| call.contains("--compare")),
        "host parity remains part of acceptance"
    );
    assert_eq!(
        calls.last().map(String::as_str),
        Some("cargo test -p kithara-ui-gallery --test gallery --profile test-release")
    );
    Ok(())
}

#[test]
fn a_failed_ui_lane_stops_acceptance_before_parity() -> Result<()> {
    let (output, calls) = run(&["ui", "--dir", "pictures"], "--lane=ui-doc")?;
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(
        calls,
        ["just test run --lane=ui", "just test run --lane=ui-doc"]
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("ui-doc"));
    Ok(())
}

#[test]
fn a_scoped_ui_request_reaches_nextest_without_a_union_filter() -> Result<()> {
    let (output, calls) = run(&["test", "--lane=ui", "-E", "test(one_regression)"], "")?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let [call] = calls.as_slice() else {
        panic!("one nextest command must run: {calls:?}");
    };
    assert!(call.contains("--profile ui"));
    assert_eq!(
        call.split_whitespace().filter(|arg| *arg == "-E").count(),
        1
    );
    assert!(call.ends_with("-E test(one_regression)"));
    assert!(!call.contains("--ignore-default-filter"));
    Ok(())
}
