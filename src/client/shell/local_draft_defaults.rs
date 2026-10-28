use super::*;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT_DRAFT_WRITE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct DraftPane {
    profile_id: String,
    boot_id: String,
    pane_id: String,
    enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct DraftDefaults {
    version: u32,
    panes: Vec<DraftPane>,
}

impl Default for DraftDefaults {
    fn default() -> Self {
        Self {
            version: 1,
            panes: Vec::new(),
        }
    }
}

impl DraftDefaults {
    pub(super) fn load(path: Option<&std::path::Path>) -> Self {
        let load = || -> Option<Self> {
            let file = std::fs::File::open(path?).ok()?;
            let mut bytes = Vec::new();
            file.take(65_537).read_to_end(&mut bytes).ok()?;
            if bytes.len() > 65_536 {
                return None;
            }
            let value: Self = serde_json::from_slice(&bytes).ok()?;
            (value.version == 1
                && value.panes.len() <= 128
                && value.panes.iter().all(|p| {
                    crate::client::endpoint::ProfileId::parse(&p.profile_id).is_ok()
                        && !p.boot_id.is_empty()
                        && p.boot_id.len() <= 256
                        && !p.pane_id.is_empty()
                        && p.pane_id.len() <= 256
                }))
            .then_some(value)
        };
        load().unwrap_or_default()
    }

    fn store(&self, path: &std::path::Path) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        if bytes.len() > 65_536 || self.panes.len() > 128 {
            return Err("Draft preferences exceed the size limit".into());
        }
        let parent = path.parent().ok_or("Invalid draft preference path")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let serial = NEXT_DRAFT_WRITE.fetch_add(1, Ordering::Relaxed);
        let temp = path.with_extension(format!("drafts-{}-{serial}.tmp", std::process::id()));
        std::fs::write(&temp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&temp, path).map_err(|e| {
            let _ = std::fs::remove_file(&temp);
            e.to_string()
        })
    }
}

impl ClientShellState {
    fn enrolled_draft_index(&self) -> Option<usize> {
        let ClientEndpointId::Ssh(profile) = &self.active_endpoint_id else {
            return None;
        };
        let snapshot = self.snapshot.as_ref()?;
        let pane = self.focused_pane_id()?;
        self.draft_defaults.panes.iter().position(|p| {
            p.profile_id == profile.to_string()
                && p.boot_id == snapshot.boot_id
                && p.pane_id == pane
        })
    }

    pub(super) fn draft_mode_label(&self) -> Option<&'static str> {
        self.enrolled_draft_index().map(|i| {
            if self.draft_defaults.panes[i].enabled {
                "draft"
            } else {
                "direct"
            }
        })
    }

    pub(super) fn default_draft_enabled(&self) -> bool {
        self.enrolled_draft_index()
            .is_some_and(|i| self.draft_defaults.panes[i].enabled)
    }

    pub(super) fn set_default_draft(&mut self, enabled: bool) {
        let Some(index) = self.enrolled_draft_index() else {
            self.push_endpoint_notice(ClientEndpointNoticeKind::Unsupported, "draft-enrollment", "Pane not enrolled", "Local draft defaults require an explicit current endpoint, boot and pane enrollment.");
            return;
        };
        let mut next = self.draft_defaults.clone();
        next.panes[index].enabled = enabled;
        if let Some(path) = self.draft_defaults_path.as_ref() {
            if let Err(error) = next.store(path) {
                self.push_endpoint_notice(
                    ClientEndpointNoticeKind::Unsupported,
                    "draft-save",
                    "Draft preference not saved",
                    &error,
                );
                return;
            }
        }
        self.draft_defaults = next;
    }

    pub(super) fn capture_default_draft_text(
        &mut self,
        text: &str,
        outcome: &mut ClientShellInput,
    ) -> bool {
        if !self.default_draft_enabled()
            || self.overlay.is_some()
            || self.mode != ClientShellMode::Terminal
            || self.popup_pending
            || self.popup_input_target().is_some()
        {
            return false;
        }
        self.open_local_compose();
        // Enrollment must never fall through to remote input when the local draft limit is reached.
        if matches!(self.overlay, Some(ClientShellOverlay::LocalCompose(_))) {
            self.insert_overlay_text(text);
        }
        outcome.repaint = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_round_trip_and_reject_unknown_version_or_oversize() {
        let path =
            std::env::temp_dir().join(format!("herdr-default-drafts-{}.json", std::process::id()));
        let mut value = DraftDefaults {
            version: 1,
            panes: vec![DraftPane {
                profile_id: "0123456789abcdef0123456789abcdef".into(),
                boot_id: "boot".into(),
                pane_id: "pane".into(),
                enabled: true,
            }],
        };
        value.store(&path).unwrap();
        assert!(DraftDefaults::load(Some(&path)).panes[0].enabled);
        value.panes[0].enabled = false;
        value.store(&path).unwrap();
        assert!(!DraftDefaults::load(Some(&path)).panes[0].enabled);
        let mut oversized = value.clone();
        oversized.panes = vec![
            DraftPane {
                profile_id: "0".repeat(32),
                boot_id: "b".repeat(256),
                pane_id: "p".repeat(256),
                enabled: true
            };
            128
        ];
        assert!(oversized.store(&path).is_err());
        assert!(!DraftDefaults::load(Some(&path)).panes[0].enabled);
        value.version = 2;
        value.store(&path).unwrap();
        assert!(DraftDefaults::load(Some(&path)).panes.is_empty());
        std::fs::write(&path, vec![b' '; 65_537]).unwrap();
        assert!(DraftDefaults::load(Some(&path)).panes.is_empty());
        std::fs::remove_file(path).unwrap();
    }
}
