//! Folderstorm: a lighter renderer while the session host's Economy has
//! given the video memory back (`folderstorm/plans/gpu-memory.md`,
//! "Economy, second pass"). The window is minimised and its 3D paused: the
//! 256 reflection probes (six faces of 128² in RGBA16F, about 275 MB) and
//! the shadow atlas (about 84 MB at quality 3) draw nothing until the
//! character is active again.
//!
//! Only what the renderer is handed changes ([`apply`], one hook at the end
//! of `Settings::render_settings`): the user's settings are neither changed
//! nor saved, Preferences shows them as they are, and every place that
//! applies the settings to the renderer applies the lowered ones while
//! Economy holds, so nothing puts the full ones back behind its back.
//! Coming back, the renderer makes the probes and the atlas again and the
//! probes are captured again (a few seconds).

use aurora_render::RenderSettings;
use std::sync::atomic::{AtomicBool, Ordering};

/// Reflection probe slots while lowered: the fewest the setting allows.
pub const PROBES: u32 = 8;
/// Shadow map resolution while lowered: the smallest the renderer makes.
pub const SHADOW_RESOLUTION: u32 = 512;

/// One viewer per process: the renderer of this process is lowered.
static LOWERED: AtomicBool = AtomicBool::new(false);

/// Lowers the renderer's settings (true) or gives the user's back; true
/// when that changed something (the caller then applies the settings).
pub fn set(lowered: bool) -> bool {
    LOWERED.swap(lowered, Ordering::Relaxed) != lowered
}

pub fn lowered_now() -> bool {
    LOWERED.load(Ordering::Relaxed)
}

/// The settings the renderer is handed: lowered while Economy holds.
pub fn apply(settings: RenderSettings) -> RenderSettings {
    if lowered_now() { lowered(settings) } else { settings }
}

/// The settings with the fewest probes and the smallest shadow atlas
/// (shadows that are off stay off, the projectors' shadows go).
pub fn lowered(s: RenderSettings) -> RenderSettings {
    RenderSettings {
        probe_slots: s.probe_slots.min(PROBES),
        shadow_resolution: if s.shadow_resolution > 0 {
            s.shadow_resolution.min(SHADOW_RESOLUTION)
        } else {
            0
        },
        projector_shadow_limit: 0,
        ..s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowered_keeps_everything_but_the_probes_and_the_shadows() {
        let high = RenderSettings {
            probe_slots: 256,
            shadow_resolution: 2048,
            projector_shadow_limit: 4,
            ssao: 2,
            ..RenderSettings::default()
        };
        let low = lowered(high);
        assert_eq!((low.probe_slots, low.shadow_resolution, low.projector_shadow_limit), (8, 512, 0));
        assert_eq!(
            RenderSettings {
                probe_slots: 256,
                shadow_resolution: 2048,
                projector_shadow_limit: 4,
                ..low
            },
            high
        );
        // shadows off stay off; already low stays as it is
        let off = lowered(RenderSettings {
            probe_slots: 8,
            shadow_resolution: 0,
            ..high
        });
        assert_eq!((off.probe_slots, off.shadow_resolution), (8, 0));
        // not lowered: as handed (the switch itself is not touched here,
        // the settings tests read it in parallel)
        if !lowered_now() {
            assert_eq!(apply(high), high);
        }
    }
}
