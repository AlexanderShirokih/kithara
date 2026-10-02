use std::{fs, process::Command};

use anyhow::{Context, Result};
use kithara_devtools::{Ctx, verdict::ChildFailure};
use tracing::info;

use crate::parity::{self, ParityArgs};

/// Runs UI lanes and parity before checking the gallery without masonry.
/// The UI profile reserves every test thread for its memory measurement, so
/// that test needs no separate invocation after the suite.
pub(crate) fn run(args: &ParityArgs, ctx: &Ctx) -> Result<()> {
    let dir = ctx.root.join(&args.dir);
    let scenario = dir.join("scenario");
    if scenario.exists() {
        fs::remove_dir_all(&scenario)
            .with_context(|| format!("clearing {}", scenario.display()))?;
    }
    for lane in ["ui", "ui-doc", "ui-perf"] {
        let mut command = Command::new("just");
        command
            .current_dir(&ctx.root)
            .args(["test", "run"])
            .arg(format!("--lane={lane}"));
        if lane == "ui" {
            command.env("KITHARA_UI_SCENARIO_CAPTURE", &scenario);
        }
        finish(command, lane)?;
    }
    parity::run(args, ctx)?;

    let mut iced = Command::new("cargo");
    iced.current_dir(&ctx.root).args([
        "test",
        "-p",
        "kithara-ui-gallery",
        "--test",
        "gallery",
        "--profile",
        "test-release",
    ]);
    finish(iced, "iced-only gallery")
}

fn finish(mut command: Command, what: &str) -> Result<()> {
    info!(step = what, "UI acceptance");
    let status = command
        .status()
        .with_context(|| format!("running {what}"))?;
    if !status.success() {
        return Err(ChildFailure::inherited(what.to_owned(), status.code()));
    }
    Ok(())
}
