//! Plain settings, put back as they are (Replace only): the world viewport
//! (`settings.folderstorm_viewport`, row 1.1) and the media choices
//! (`settings.media`, the Nearby media and the media button of the top
//! bar use them).

use super::super::section::{Env, Exported, How, Outcome, Section, Target};
use aurora_i18n::tr;
use serde_json::{Map, Value};

pub struct Viewport;

impl Section for Viewport {
    fn id(&self) -> &'static str {
        "viewport"
    }

    fn label(&self) -> String {
        tr!("fs-backup-section-viewport").to_owned()
    }

    fn merges(&self) -> bool {
        false
    }

    fn export(&self, env: &Env) -> Option<Exported> {
        Some(Exported::value(serde_json::to_value(&env.settings.folderstorm_viewport).ok()?))
    }

    fn import(&self, target: &mut Target, value: Value, _: &How) -> Result<Outcome, String> {
        let read = crate::tolerant::from_value(value, "backup viewport").ok_or_else(|| tr!("fs-backup-unreadable").to_owned())?;
        target.settings.folderstorm_viewport = read.value;
        Ok(Outcome::default())
    }
}

/// The media settings a backup carries: not the plugins' folder (a path of
/// this PC) nor the answer about the plugins' download (asked on each PC).
const MEDIA_KEYS: &[&str] = &[
    "enabled",
    "prim_media",
    "show_on_others",
    "max_instances",
    "allow_scripted",
    "first_click_interact",
    "autoplay_huds",
    "filter",
    "filter_trust_own_worn",
];

pub struct Media;

impl Section for Media {
    fn id(&self) -> &'static str {
        "media"
    }

    fn label(&self) -> String {
        tr!("fs-backup-section-media").to_owned()
    }

    fn merges(&self) -> bool {
        false
    }

    fn export(&self, env: &Env) -> Option<Exported> {
        let Value::Object(all) = serde_json::to_value(&env.settings.media).ok()? else {
            return None;
        };
        let kept: Map<String, Value> = all.into_iter().filter(|(key, _)| MEDIA_KEYS.contains(&key.as_str())).collect();
        Some(Exported::value(Value::Object(kept)))
    }

    fn import(&self, target: &mut Target, value: Value, _: &How) -> Result<Outcome, String> {
        let Value::Object(incoming) = value else {
            return Err(tr!("fs-backup-unreadable").to_owned());
        };
        let Ok(Value::Object(mut now)) = serde_json::to_value(&target.settings.media) else {
            return Err(tr!("fs-backup-unreadable").to_owned());
        };
        for (key, value) in incoming {
            if MEDIA_KEYS.contains(&key.as_str()) {
                now.insert(key, value);
            }
        }
        let read = crate::tolerant::from_value(Value::Object(now), "backup media").ok_or_else(|| tr!("fs-backup-unreadable").to_owned())?;
        target.settings.media = read.value;
        // the range of its slider in Preferences › Media
        target.settings.media.max_instances = target.settings.media.max_instances.clamp(1, 16);
        Ok(Outcome::default())
    }
}
