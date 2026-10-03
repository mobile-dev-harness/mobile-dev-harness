//! Cross-configuration layout checks (functional design F13.3): the same screen at a larger font,
//! in dark mode and right to left, on one device, compared with how it looks by default.

use mdh_core::{Appearance, AppearanceKind, Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// Font scale 1.3, the largest step most users pick.
    FontScale,
    /// Dark theme.
    Dark,
    /// The app in Arabic: right-to-left layout for apps that support it.
    Rtl,
}

impl Variant {
    pub const ALL: [Variant; 3] = [Variant::FontScale, Variant::Dark, Variant::Rtl];

    pub fn parse(s: &str) -> Result<Variant> {
        match s.trim() {
            "font_scale" | "large_font" => Ok(Variant::FontScale),
            "dark" | "dark_mode" => Ok(Variant::Dark),
            "rtl" => Ok(Variant::Rtl),
            other => Err(Error::InvalidFlow {
                flow: "visual".into(),
                reason: format!("unknown configuration `{other}`; use font_scale, dark or rtl"),
            }),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Variant::FontScale => "font_scale",
            Variant::Dark => "dark",
            Variant::Rtl => "rtl",
        }
    }

    /// `at font scale 1.3`, as used in findings.
    pub fn describe(self) -> &'static str {
        match self {
            Variant::FontScale => "at font scale 1.3",
            Variant::Dark => "in dark mode",
            Variant::Rtl => "right to left (ar)",
        }
    }

    pub fn kind(self, package: &str) -> AppearanceKind {
        match self {
            Variant::FontScale => AppearanceKind::FontScale,
            Variant::Dark => AppearanceKind::NightMode,
            Variant::Rtl => AppearanceKind::AppLocales {
                package: package.to_owned(),
            },
        }
    }

    pub fn value(self, package: &str) -> Appearance {
        match self {
            Variant::FontScale => Appearance::FontScale(Some("1.3".into())),
            Variant::Dark => Appearance::NightMode("yes".into()),
            Variant::Rtl => Appearance::AppLocales {
                package: package.to_owned(),
                locales: "ar".into(),
            },
        }
    }
}
