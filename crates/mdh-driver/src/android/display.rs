//! `wm size` and `wm density` output: the panel and the overrides on top of it.

use mdh_core::PhysicalDisplay;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Display {
    pub physical: Option<PhysicalDisplay>,
    pub override_size: Option<(u32, u32)>,
    pub override_density: Option<u32>,
}

/// Parses `Physical size: 1344x2992`, `Override size: 1340x2144`, `Physical density: 480`,
/// `Override density: 268`.
pub fn parse(out: &str) -> Display {
    let mut size = None;
    let mut density = None;
    let mut d = Display::default();
    let wh = |v: &str| {
        let (w, h) = v.trim().split_once('x')?;
        Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
    };
    for line in out.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        match k.trim() {
            "Physical size" => size = wh(v),
            "Override size" => d.override_size = wh(v),
            "Physical density" => density = v.trim().parse().ok(),
            "Override density" => d.override_density = v.trim().parse().ok(),
            _ => {}
        }
    }
    if let (Some((width, height)), Some(density)) = (size, density) {
        d.physical = Some(PhysicalDisplay {
            width,
            height,
            density,
        });
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_and_overrides() {
        let d = parse(
            "Physical size: 1344x2992\nOverride size: 1340x2144\nPhysical density: 480\nOverride density: 268\n",
        );
        assert_eq!(
            d.physical,
            Some(PhysicalDisplay {
                width: 1344,
                height: 2992,
                density: 480
            })
        );
        assert_eq!(d.override_size, Some((1340, 2144)));
        assert_eq!(d.override_density, Some(268));
        let plain = parse("Physical size: 1080x2400\nPhysical density: 420");
        assert_eq!(plain.override_size, None);
        assert_eq!(plain.override_density, None);
    }
}
