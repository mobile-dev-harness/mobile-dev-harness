//! The verification plan (ADR-0011): the fewest cells that cover the risks, cheapest first —
//! configurations of the current device, then other connected devices, then AVDs to start.

use std::collections::BTreeMap;

use mdh_core::{Avd, Device};
use serde::Serialize;

use crate::kb::Shape;
use crate::risk::{NEWEST, Need, Risk};

/// What there is to run on.
#[derive(Debug, Clone)]
pub struct Inventory {
    /// The session's device: the reference.
    pub current: Device,
    /// Other online devices.
    pub online: Vec<Device>,
    /// AVDs that aren't running.
    pub avds: Vec<Avd>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    /// A connected device (the current one included), by serial.
    Device { id: String, describe: String },
    /// An AVD to start, with consent.
    Start { avd: String, api: Option<u32> },
}

impl Target {
    pub fn describe(&self) -> String {
        match self {
            Target::Device { describe, .. } => describe.clone(),
            Target::Start {
                avd,
                api: Some(api),
            } => format!("{avd} (API {api}, to start)"),
            Target::Start { avd, api: None } => format!("{avd} (to start)"),
        }
    }

    fn key(&self) -> String {
        match self {
            Target::Device { id, .. } => id.clone(),
            Target::Start { avd, .. } => format!("avd:{avd}"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Cell {
    pub target: Target,
    pub shape: Shape,
    /// The current device as it is: what the other cells are compared with.
    pub reference: bool,
    /// Risks this cell verifies.
    pub risks: Vec<String>,
    /// Some risk on this cell needs the state check across rotation.
    pub state_check: bool,
}

impl Cell {
    /// As results name it: the device, and the configuration it was put in.
    pub fn name(&self) -> String {
        let device = match &self.target {
            Target::Device { describe, .. } => describe.clone(),
            Target::Start {
                avd,
                api: Some(api),
            } => format!("{avd} (API {api})"),
            Target::Start { avd, api: None } => avd.clone(),
        };
        if self.shape == Shape::Default {
            device
        } else {
            format!("{device} as {} (simulated)", self.shape.describe())
        }
    }

    /// As plans show it: what it costs too.
    pub fn describe(&self) -> String {
        if self.shape == Shape::Default {
            self.target.describe()
        } else {
            format!(
                "{} as {} (simulated)",
                self.target.describe(),
                self.shape.describe()
            )
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub cells: Vec<Cell>,
    /// Risk id → why it can't be verified here.
    pub unverifiable: Vec<(String, String)>,
    /// AVDs the plan starts.
    pub starts: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct PlanOptions {
    /// New emulators a run may start.
    pub max_starts: usize,
}

impl Default for PlanOptions {
    fn default() -> Self {
        PlanOptions { max_starts: 2 }
    }
}

pub fn plan(risks: &[Risk], inv: &Inventory, options: PlanOptions) -> Plan {
    let current = Target::Device {
        id: inv.current.id.clone(),
        describe: inv.current.describe(),
    };
    let mut cells: BTreeMap<(String, Shape), Cell> = BTreeMap::new();
    cells.insert(
        (current.key(), Shape::Default),
        Cell {
            target: current.clone(),
            shape: Shape::Default,
            reference: true,
            risks: Vec::new(),
            state_check: false,
        },
    );
    let mut unverifiable = Vec::new();
    let mut starts: Vec<String> = Vec::new();
    for risk in risks {
        if let Some(why) = &risk.unverifiable {
            unverifiable.push((risk.id.clone(), why.clone()));
            continue;
        }
        let mut chosen = Vec::new();
        let mut missing = None;
        for need in &risk.needs {
            match target_for(need, inv, &starts, options) {
                Ok(t) => chosen.push((t, need.shape)),
                Err(why) => {
                    missing = Some(why);
                    break;
                }
            }
        }
        if let Some(why) = missing {
            unverifiable.push((risk.id.clone(), why));
            continue;
        }
        for (target, shape) in chosen {
            if let Target::Start { avd, .. } = &target
                && !starts.contains(avd)
            {
                starts.push(avd.clone());
            }
            let cell = cells.entry((target.key(), shape)).or_insert_with(|| Cell {
                target: target.clone(),
                shape,
                reference: false,
                risks: Vec::new(),
                state_check: false,
            });
            if !cell.risks.contains(&risk.id) {
                cell.risks.push(risk.id.clone());
            }
            cell.state_check |= risk.state_check;
        }
    }
    let mut cells: Vec<Cell> = cells.into_values().collect();
    // The reference first, then the current device's configurations, then other devices.
    cells.sort_by_key(|c| (!c.reference, c.target != current, c.target.key(), c.shape));
    Plan {
        cells,
        unverifiable,
        starts,
    }
}

/// The cheapest device meeting `need`, or why there is none.
fn target_for(
    need: &Need,
    inv: &Inventory,
    starts: &[String],
    options: PlanOptions,
) -> Result<Target, String> {
    let (lo, hi) = need.api;
    let fits = |d: &Device| {
        d.api.is_none_or(|a| a >= lo && a <= hi)
            && (need.vendors.is_empty()
                || d.manufacturer
                    .as_ref()
                    .is_some_and(|m| need.vendors.contains(m)))
            && (need.shape == Shape::Default || d.is_emulator)
    };
    let device = |d: &Device| Target::Device {
        id: d.id.clone(),
        describe: d.describe(),
    };
    if fits(&inv.current) {
        return Ok(device(&inv.current));
    }
    // Closest to the boundary: the newest below one, the oldest above one.
    let rank = |api: Option<u32>| {
        let a = api.unwrap_or(0);
        if hi == NEWEST { a } else { u32::MAX - a }
    };
    if let Some(d) = inv
        .online
        .iter()
        .filter(|d| fits(d))
        .min_by_key(|d| rank(d.api))
    {
        return Ok(device(d));
    }
    if !need.vendors.is_empty() {
        return Err(format!(
            "needs a device from {} (none connected)",
            need.vendors.join(", ")
        ));
    }
    let avd = inv
        .avds
        .iter()
        .filter(|a| a.running.is_none() && a.api.is_some_and(|x| x >= lo && x <= hi))
        .min_by_key(|a| rank(a.api));
    match avd {
        Some(a) if starts.contains(&a.name) || starts.len() < options.max_starts => {
            Ok(Target::Start {
                avd: a.name.clone(),
                api: a.api,
            })
        }
        Some(a) if options.max_starts == 0 => Err(format!(
            "needs the emulator {} ({}), left out: this run starts no emulators",
            a.name,
            need.describe()
        )),
        Some(a) => Err(format!(
            "needs {} ({}), over the limit of {} new emulators per run",
            a.name,
            need.describe(),
            options.max_starts
        )),
        None => Err(format!(
            "needs a device with {}: no connected device or AVD has it; create one in Android Studio's \
             Device Manager, or with `sdkmanager \"system-images;android-{api};google_apis;<abi>\"` and \
             `avdmanager create avd`",
            need.describe(),
            api = if hi == NEWEST { lo } else { hi }
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::{Dimension, Likelihood};
    use mdh_core::{DeviceState, Platform};

    fn device(id: &str, api: u32, emulator: bool, manufacturer: &str) -> Device {
        Device {
            id: id.into(),
            platform: Platform::Android,
            state: DeviceState::Online,
            model: None,
            is_emulator: emulator,
            avd: emulator.then(|| format!("avd-{api}")),
            api: Some(api),
            manufacturer: Some(manufacturer.into()),
        }
    }

    fn risk(id: &str, needs: Vec<Need>) -> Risk {
        Risk {
            id: id.into(),
            dimension: Dimension::Os,
            likelihood: Likelihood::High,
            title: id.into(),
            evidence: Vec::new(),
            screens: Vec::new(),
            needs,
            state_check: false,
            all_flows: false,
            unverifiable: None,
            verify: String::new(),
            source: None,
        }
    }

    fn api(lo: u32, hi: u32) -> Need {
        Need {
            api: (lo, hi),
            ..Need::any()
        }
    }

    fn shape(shape: Shape) -> Need {
        Need {
            shape,
            ..Need::any()
        }
    }

    #[test]
    fn cheapest_cells_first_and_what_cant_be_covered() {
        let inv = Inventory {
            current: device("emulator-5554", 36, true, "google"),
            online: vec![device("R5CT", 34, false, "samsung")],
            avds: vec![
                Avd {
                    name: "api30".into(),
                    api: Some(30),
                    running: None,
                },
                Avd {
                    name: "api32".into(),
                    api: Some(32),
                    running: None,
                },
                Avd {
                    name: "api28".into(),
                    api: Some(28),
                    running: None,
                },
            ],
        };
        let risks = vec![
            risk("api-gate:33", vec![api(26, 32), api(33, NEWEST)]),
            risk(
                "screen-size",
                vec![shape(Shape::Compact), shape(Shape::Tablet)],
            ),
            risk("min-sdk:28", vec![api(28, 28)]),
            risk("legacy", vec![api(26, 26)]),
            risk(
                "vendor:background",
                vec![Need {
                    vendors: vec!["xiaomi".into(), "samsung".into()],
                    ..Need::any()
                }],
            ),
            risk(
                "vendor:popups",
                vec![Need {
                    vendors: vec!["xiaomi".into()],
                    ..Need::any()
                }],
            ),
        ];
        let p = plan(&risks, &inv, PlanOptions { max_starts: 2 });
        let cells: Vec<String> = p
            .cells
            .iter()
            .map(|c| format!("{}: {}", c.describe(), c.risks.join(", ")))
            .collect();
        assert_eq!(
            cells,
            [
                "emulator-5554 (avd-36, API 36): api-gate:33",
                "emulator-5554 (avd-36, API 36) as compact 360×640 dp (simulated): screen-size",
                "emulator-5554 (avd-36, API 36) as tablet 1280×800 dp (simulated): screen-size",
                "R5CT (API 34): vendor:background",
                "api28 (API 28, to start): min-sdk:28",
                "api32 (API 32, to start): api-gate:33",
            ]
        );
        assert_eq!(p.starts, ["api32", "api28"]);
        let missing: Vec<&str> = p.unverifiable.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(missing, ["legacy", "vendor:popups"]);
        assert!(
            p.unverifiable[0].1.contains("API 26"),
            "{:?}",
            p.unverifiable
        );
        assert!(p.unverifiable[1].1.contains("xiaomi"));
    }
}
