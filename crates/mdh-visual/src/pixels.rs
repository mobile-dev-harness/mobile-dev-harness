//! What the tree can't see: pixel baselines (functional design F13.1) and text contrast (F13.4).
//!
//! Frames are compared at half resolution, which averages away most anti-aliasing noise, in
//! blocks: a block changed when enough of its pixels did, and touching changed blocks form one
//! region, reported with the element it falls in.

use std::path::Path;

use image::{Rgb, RgbImage, imageops};
use mdh_core::ui::Rect;
use mdh_core::{Error, Result};
use mdh_observe::{Role, UiNode, UiTree, render_line};

use crate::rules::{Rule, Violation};

/// Device pixels per frame pixel.
pub const SCALE: u32 = 2;
/// Block edge in frame pixels (16 device pixels).
const BLOCK: u32 = 8;
/// A pixel differs when a channel moved by more than this.
const CHANNEL_TOLERANCE: u8 = 32;
/// A block changed when this share of its unmasked pixels differ (and at least a few do).
const BLOCK_SHARE: f64 = 0.08;
const BLOCK_MIN_PIXELS: u32 = 6;
/// WCAG 2 AA.
const CONTRAST_NORMAL: f64 = 4.5;
const CONTRAST_LARGE: f64 = 3.0;

pub fn decode(png: &[u8]) -> Result<RgbImage> {
    image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .map(|i| i.to_rgb8())
        .map_err(|e| Error::Parse {
            tool: "screencap".into(),
            detail: e.to_string(),
        })
}

/// The frame baselines are kept and compared at.
pub fn frame(full: &RgbImage) -> RgbImage {
    imageops::resize(
        full,
        full.width() / SCALE,
        full.height() / SCALE,
        imageops::FilterType::Triangle,
    )
}

pub fn load(path: &Path) -> Result<Option<RgbImage>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(decode(&bytes).ok()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn save(path: &Path, image: &RgbImage) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    image
        .save(path)
        .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))
}

/// A changed area, in device pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub rect: Rect,
    /// Share of the region's pixels that differ.
    pub changed: f64,
}

pub struct Comparison {
    pub regions: Vec<Region>,
    /// The frames have different sizes: nothing to compare.
    pub size_mismatch: bool,
}

/// Compares two frames, ignoring `masks` (device pixels).
pub fn compare(base: &RgbImage, now: &RgbImage, masks: &[Rect]) -> Comparison {
    if base.dimensions() != now.dimensions() {
        return Comparison {
            regions: Vec::new(),
            size_mismatch: true,
        };
    }
    let (w, h) = now.dimensions();
    let masked = |x: u32, y: u32| {
        let (dx, dy) = ((x * SCALE) as i32, (y * SCALE) as i32);
        masks
            .iter()
            .any(|m| dx >= m.left && dx < m.right && dy >= m.top && dy < m.bottom)
    };
    let differs = |a: &Rgb<u8>, b: &Rgb<u8>| {
        a.0.iter()
            .zip(b.0)
            .any(|(x, y)| x.abs_diff(y) > CHANNEL_TOLERANCE)
    };
    let (bw, bh) = (w.div_ceil(BLOCK), h.div_ceil(BLOCK));
    let mut changed = vec![false; (bw * bh) as usize];
    let mut diff_count = vec![0u32; (bw * bh) as usize];
    let mut counted = vec![0u32; (bw * bh) as usize];
    for y in 0..h {
        for x in 0..w {
            if masked(x, y) {
                continue;
            }
            let i = ((y / BLOCK) * bw + x / BLOCK) as usize;
            counted[i] += 1;
            if differs(base.get_pixel(x, y), now.get_pixel(x, y)) {
                diff_count[i] += 1;
            }
        }
    }
    for i in 0..changed.len() {
        changed[i] = diff_count[i] >= BLOCK_MIN_PIXELS
            && f64::from(diff_count[i]) >= BLOCK_SHARE * f64::from(counted[i]);
    }
    // Touching changed blocks (8-connected) form one region.
    let mut seen = vec![false; changed.len()];
    let mut regions = Vec::new();
    for start in 0..changed.len() {
        if !changed[start] || seen[start] {
            continue;
        }
        let mut stack = vec![start];
        seen[start] = true;
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        let (mut diffs, mut pixels) = (0u32, 0u32);
        while let Some(i) = stack.pop() {
            let (bx, by) = (i as u32 % bw, i as u32 / bw);
            x0 = x0.min(bx);
            y0 = y0.min(by);
            x1 = x1.max(bx);
            y1 = y1.max(by);
            diffs += diff_count[i];
            pixels += counted[i];
            for (dx, dy) in [
                (-1i32, -1i32),
                (0, -1),
                (1, -1),
                (-1, 0),
                (1, 0),
                (-1, 1),
                (0, 1),
                (1, 1),
            ] {
                let (nx, ny) = (bx as i32 + dx, by as i32 + dy);
                if nx < 0 || ny < 0 || nx >= bw as i32 || ny >= bh as i32 {
                    continue;
                }
                let j = (ny as u32 * bw + nx as u32) as usize;
                if changed[j] && !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        let px = |b: u32, limit: u32| ((b * BLOCK).min(limit) * SCALE) as i32;
        regions.push(Region {
            rect: Rect::new(px(x0, w), px(y0, h), px(x1 + 1, w), px(y1 + 1, h)),
            changed: f64::from(diffs) / f64::from(pixels.max(1)),
        });
    }
    regions.sort_by_key(|r| std::cmp::Reverse(r.rect.area()));
    Comparison {
        regions,
        size_mismatch: false,
    }
}

/// The current frame, dimmed, with changed regions outlined in red: evidence for people.
pub fn diff_image(now: &RgbImage, regions: &[Region]) -> RgbImage {
    let mut out = RgbImage::from_fn(now.width(), now.height(), |x, y| {
        let p = now.get_pixel(x, y).0;
        Rgb([p[0] / 2 + 64, p[1] / 2 + 64, p[2] / 2 + 64])
    });
    for r in regions {
        let (x0, y0) = ((r.rect.left as u32) / SCALE, (r.rect.top as u32) / SCALE);
        let (x1, y1) = (
            ((r.rect.right as u32) / SCALE)
                .min(now.width())
                .saturating_sub(1),
            ((r.rect.bottom as u32) / SCALE)
                .min(now.height())
                .saturating_sub(1),
        );
        for x in x0..=x1 {
            for t in 0..2 {
                out.put_pixel(x, (y0 + t).min(y1), Rgb([230, 20, 20]));
                out.put_pixel(x, y1.saturating_sub(t).max(y0), Rgb([230, 20, 20]));
            }
        }
        for y in y0..=y1 {
            for t in 0..2 {
                out.put_pixel((x0 + t).min(x1), y, Rgb([230, 20, 20]));
                out.put_pixel(x1.saturating_sub(t).max(x0), y, Rgb([230, 20, 20]));
            }
        }
    }
    out
}

/// The smallest element containing the region's center, to say where a change is.
pub fn element_at<'t>(tree: &'t UiTree, r: &Rect) -> Option<&'t UiNode> {
    let (cx, cy) = r.center();
    tree.iter()
        .filter(|n| {
            let b = n.bounds;
            cx >= b.left && cx < b.right && cy >= b.top && cy < b.bottom
        })
        .min_by_key(|n| n.bounds.area())
}

/// WCAG relative luminance.
fn luminance(p: &Rgb<u8>) -> f64 {
    let channel = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.039_28 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(p.0[0]) + 0.7152 * channel(p.0[1]) + 0.0722 * channel(p.0[2])
}

/// Contrast of the text in `rect` against its background: the background is the most common
/// color, the text the color farthest from it that still covers a visible share of the area.
pub fn contrast_in(image: &RgbImage, rect: &Rect) -> Option<(f64, Rgb<u8>, Rgb<u8>)> {
    let (w, h) = image.dimensions();
    let (x0, y0) = (rect.left.max(0) as u32, rect.top.max(0) as u32);
    let (x1, y1) = (
        (rect.right.max(0) as u32).min(w),
        (rect.bottom.max(0) as u32).min(h),
    );
    if x1 <= x0 + 4 || y1 <= y0 + 4 {
        return None;
    }
    // Colors bucketed to 4 bits per channel; at most ~20k samples.
    let area = u64::from(x1 - x0) * u64::from(y1 - y0);
    let stride = ((area / 20_000) as f64).sqrt().max(1.0) as u32;
    let mut buckets: std::collections::HashMap<[u8; 3], (u32, Rgb<u8>)> =
        std::collections::HashMap::new();
    let mut total = 0u32;
    for y in (y0..y1).step_by(stride as usize) {
        for x in (x0..x1).step_by(stride as usize) {
            let p = *image.get_pixel(x, y);
            let e = buckets
                .entry([p.0[0] >> 4, p.0[1] >> 4, p.0[2] >> 4])
                .or_insert((0, p));
            e.0 += 1;
            total += 1;
        }
    }
    let (_, (_, background)) = buckets.iter().max_by_key(|(_, (n, _))| *n)?;
    let lb = luminance(background);
    let ratio = |l: f64| (lb.max(l) + 0.05) / (lb.min(l) + 0.05);
    let floor = (f64::from(total) * 0.005).max(3.0);
    let (_, foreground) = buckets
        .values()
        .filter(|(n, _)| f64::from(*n) >= floor)
        .map(|(_, c)| (ratio(luminance(c)), *c))
        .max_by(|a, b| a.0.total_cmp(&b.0))?;
    let r = ratio(luminance(&foreground));
    // A plain surface has nothing drawn on it to judge.
    (r > 1.1).then_some((r, foreground, *background))
}

/// The contrast rule over the labeled elements on screen; `image` is full resolution.
pub fn contrast(tree: &UiTree, image: &RgbImage, density: u32) -> Vec<Violation> {
    let dp = |px: i32| f64::from(px) * 160.0 / f64::from(density.max(1));
    let screen_area = tree.screen.area().max(1);
    let hex = |c: Rgb<u8>| format!("#{:02x}{:02x}{:02x}", c.0[0], c.0[1], c.0[2]);
    let mut out = Vec::new();
    for n in tree.iter() {
        let text_like = matches!(
            n.role,
            Role::Text
                | Role::Button
                | Role::Item
                | Role::Checkbox
                | Role::Switch
                | Role::Radio
                | Role::Tab
        );
        // Inactive controls are exempt (WCAG 1.4.3); whole-screen containers have no single text.
        if !text_like
            || n.label.is_none()
            || n.state.disabled
            || n.bounds.area() * 2 > screen_area
            || !n.children.is_empty()
        {
            continue;
        }
        let Some((ratio, fg, bg)) = contrast_in(image, &n.bounds) else {
            continue;
        };
        let large = n.role == Role::Text && dp(n.bounds.height()) >= 32.0;
        let needed = if large {
            CONTRAST_LARGE
        } else {
            CONTRAST_NORMAL
        };
        if ratio < needed {
            out.push(Violation {
                rule: Rule::Contrast,
                detail: format!(
                    "{}: contrast {ratio:.1}:1 ({} on {}), needs {needed}:1",
                    render_line(n),
                    hex(fg),
                    hex(bg)
                ),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, c: [u8; 3]) -> RgbImage {
        RgbImage::from_pixel(w, h, Rgb(c))
    }

    fn paint(img: &mut RgbImage, r: Rect, c: [u8; 3]) {
        for y in r.top as u32..r.bottom as u32 {
            for x in r.left as u32..r.right as u32 {
                img.put_pixel(x, y, Rgb(c));
            }
        }
    }

    #[test]
    fn changed_blocks_form_regions_and_masks_hide_them() {
        let base = solid(200, 400, [255, 255, 255]);
        let mut now = base.clone();
        paint(&mut now, Rect::new(20, 40, 60, 60), [0, 0, 0]); // frame px
        paint(&mut now, Rect::new(150, 300, 152, 301), [0, 0, 0]); // 2 px of noise
        let c = compare(&base, &now, &[]);
        assert_eq!(c.regions.len(), 1, "{:?}", c.regions);
        let r = &c.regions[0];
        assert_eq!(r.rect, Rect::new(32, 80, 128, 128));
        assert!(r.changed > 0.4, "{r:?}");
        let masked = compare(&base, &now, &[Rect::new(0, 0, 400, 200)]);
        assert!(masked.regions.is_empty());
        assert!(compare(&base, &solid(100, 100, [0, 0, 0]), &[]).size_mismatch);
    }

    #[test]
    fn contrast_of_text_on_a_background() {
        let mut img = solid(300, 100, [255, 255, 255]);
        // "Text": 10% of the area in near-black.
        for x in (0..300).step_by(10) {
            paint(&mut img, Rect::new(x, 30, x + 1, 70), [20, 20, 20]);
        }
        let (ratio, _, _) = contrast_in(&img, &Rect::new(0, 0, 300, 100)).unwrap();
        assert!(ratio > 15.0, "{ratio}");
        let mut grey = solid(300, 100, [224, 224, 224]);
        for x in (0..300).step_by(10) {
            paint(&mut grey, Rect::new(x, 30, x + 1, 70), [158, 158, 158]);
        }
        let (ratio, _, _) = contrast_in(&grey, &Rect::new(0, 0, 300, 100)).unwrap();
        assert!(ratio < 2.5, "{ratio}");
        assert!(contrast_in(&solid(300, 100, [10, 200, 10]), &Rect::new(0, 0, 300, 100)).is_none());
    }
}
