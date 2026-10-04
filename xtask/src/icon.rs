//! Renders the app icon: two arrows on a Route-blue rounded square.

use anyhow::Result;
use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::imageops::{FilterType, resize};
use image::{ExtendedColorType, Rgba, RgbaImage};
use std::path::Path;

pub const SIZE: u32 = 1024;
const MARGIN: f32 = 100.0;
const RADIUS: f32 = 0.22 * (SIZE as f32 - 2.0 * MARGIN);
const TOP: [f32; 3] = [0x4A as f32, 0x89 as f32, 0xC9 as f32];
const BOTTOM: [f32; 3] = [0x2F as f32, 0x6F as f32, 0xB2 as f32];
/// Samples per axis for edge anti-aliasing.
const SS: u32 = 4;

fn in_rounded_square(x: f32, y: f32) -> bool {
    let (lo, hi) = (MARGIN, SIZE as f32 - MARGIN);
    let cx = x.clamp(lo + RADIUS, hi - RADIUS);
    let cy = y.clamp(lo + RADIUS, hi - RADIUS);
    x >= lo && x <= hi && y >= lo && y <= hi && (x - cx).hypot(y - cy) <= RADIUS
}

/// An arrow centered on `cy` spanning `x0..x1`, pointing toward `x1`.
fn in_arrow(x: f32, y: f32, cy: f32, x0: f32, x1: f32) -> bool {
    const SHAFT: f32 = 30.0;
    const HEAD_LEN: f32 = 130.0;
    const HEAD_HALF: f32 = 105.0;
    let dir = (x1 - x0).signum();
    let t = (x - x0) * dir; // distance along the arrow
    let len = (x1 - x0).abs();
    let dy = (y - cy).abs();
    let shaft = t >= 0.0 && t <= len - HEAD_LEN + 1.0 && dy <= SHAFT;
    let into_head = t - (len - HEAD_LEN);
    let head = into_head >= 0.0 && t <= len && dy <= HEAD_HALF * (1.0 - into_head / HEAD_LEN);
    shaft || head
}

fn in_arrows(x: f32, y: f32) -> bool {
    in_arrow(x, y, 400.0, 270.0, 754.0) || in_arrow(x, y, 624.0, 754.0, 270.0)
}

pub fn render() -> RgbaImage {
    let mut img = RgbaImage::new(SIZE, SIZE);
    let n = (SS * SS) as f32;
    for py in 0..SIZE {
        for px in 0..SIZE {
            let (mut shape, mut arrow) = (0u32, 0u32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = px as f32 + (sx as f32 + 0.5) / SS as f32;
                    let y = py as f32 + (sy as f32 + 0.5) / SS as f32;
                    if in_rounded_square(x, y) {
                        shape += 1;
                        if in_arrows(x, y) {
                            arrow += 1;
                        }
                    }
                }
            }
            if shape == 0 {
                continue;
            }
            let k = (py as f32 - MARGIN) / (SIZE as f32 - 2.0 * MARGIN);
            let mut c = [0f32; 3];
            for i in 0..3 {
                let base = TOP[i] + (BOTTOM[i] - TOP[i]) * k.clamp(0.0, 1.0);
                // Arrows are white over the gradient, weighted by coverage.
                let w = arrow as f32 / shape as f32;
                c[i] = base * (1.0 - w) + 255.0 * w;
            }
            let alpha = shape as f32 / n * 255.0;
            img.put_pixel(
                px,
                py,
                Rgba([c[0] as u8, c[1] as u8, c[2] as u8, alpha.round() as u8]),
            );
        }
    }
    img
}

pub fn resized(icon: &RgbaImage, px: u32) -> RgbaImage {
    resize(icon, px, px, FilterType::Lanczos3)
}

/// Writes assets/icon.png and assets/icon.ico (the Windows executable icon).
pub fn render_files(root: &Path) -> Result<()> {
    let icon = render();
    let assets = root.join("assets");
    std::fs::create_dir_all(&assets)?;
    icon.save(assets.join("icon.png"))?;
    let frames = [16u32, 32, 48, 64, 128, 256]
        .map(|px| {
            let img = resized(&icon, px);
            IcoFrame::as_png(img.as_raw(), px, px, ExtendedColorType::Rgba8)
        })
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let file = std::fs::File::create(assets.join("icon.ico"))?;
    IcoEncoder::new(file).encode_images(&frames)?;
    println!("wrote {}", assets.join("icon.png").display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_has_transparent_margin_and_corners() {
        assert!(!in_rounded_square(10.0, 10.0));
        assert!(
            !in_rounded_square(MARGIN + 2.0, MARGIN + 2.0),
            "corner is cut"
        );
        assert!(in_rounded_square(512.0, 512.0));
    }

    #[test]
    fn arrows_point_opposite_ways() {
        // Tip of the upper arrow is at the right, of the lower at the left.
        assert!(in_arrows(740.0, 400.0));
        assert!(!in_arrows(285.0, 400.0 - 100.0));
        assert!(in_arrows(284.0, 624.0));
        assert!(!in_arrows(740.0, 624.0 - 90.0));
        assert!(!in_arrows(512.0, 512.0));
    }
}
