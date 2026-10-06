//! `cargo xtask screenshot`: renders the main window from the tests' seeded
//! fake state and saves it as the README's lossless webp.

use crate::{run, target_dir};
use anyhow::{Context, Result};
use image::codecs::webp::WebPEncoder;
use std::path::Path;
use std::process::Command;

const OUT: &str = "assets/images/project-transfer.webp";

pub fn render(root: &Path) -> Result<()> {
    let dir = target_dir(root)?.join("readme-shot");
    std::fs::create_dir_all(&dir)?;
    let png = dir.join("project-transfer.png");
    // A stale PNG would hide a test that ran but drew nothing.
    let _ = std::fs::remove_file(&png);

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    run(Command::new(cargo)
        .current_dir(root)
        .env("README_SHOT_PNG", &png)
        .args([
            "test",
            "--package",
            "project-transfer",
            "--test",
            "readme_shot",
            "--",
            "--ignored",
            "--exact",
            "readme_screenshot",
        ]))?;

    let img = image::open(&png)
        .with_context(|| format!("the test did not leave a PNG at {}", png.display()))?
        .to_rgba8();
    let out = root.join(OUT);
    let tmp = out.with_extension("webp.tmp");
    let file = std::fs::File::create(&tmp)
        .with_context(|| format!("could not create {}", tmp.display()))?;
    WebPEncoder::new_lossless(std::io::BufWriter::new(file))
        .encode(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .context("could not encode the webp")?;
    std::fs::rename(&tmp, &out)?;
    println!("Wrote {OUT} ({}x{})", img.width(), img.height());
    Ok(())
}
