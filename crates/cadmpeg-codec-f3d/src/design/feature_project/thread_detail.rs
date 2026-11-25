// SPDX-License-Identifier: Apache-2.0

use crate::loss::F3dLossCode;
use crate::records::feature::thread::DesignThreadConstruction;
use cadmpeg_ir::report::loss::LossNote;

pub(super) fn loss(scope: &str, construction: &DesignThreadConstruction) -> LossNote {
    let side = construction
        .external()
        .map_or_else(String::new, |external| format!(" external {external} and"));
    let side_unresolved = if construction.external().is_none() {
        " Side control remains native."
    } else {
        ""
    };
    let hand = if construction
        .extended
        .as_ref()
        .and_then(|value| value.right_handed)
        .is_none()
    {
        " Hand remains native."
    } else {
        ""
    };
    F3dLossCode::ThreadDetailNative.note(format!(
        "Thread {scope} retains{side} flank angle {} degrees in its native construction.{side_unresolved}{hand} Its exact groove-profile table is unresolved.",
        construction.angle.degrees(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_thread_detail_has_a_named_loss() {
        let mut construction: DesignThreadConstruction =
            cadmpeg_core::serde_erase::json::from_value(serde_json::json!({
                "form": "standard_extended", "designation_offset": 100,
                "designation": "M10x1.5", "nominal_size_text": "10", "nominal_size": 10.0,
                "profile": "ISO Metric profile", "major_diameter": 1.0,
                "minor_diameter": 0.8, "pitch_diameter": 0.9, "pitch": 0.15,
                "modeled": true, "face_group_record_indices": [10],
                "extended": {"class": "6H", "external": false,
                    "full_length": true, "right_handed": false}
            }))
            .unwrap();
        let note = loss("scope", &construction);
        assert_eq!(note.code, F3dLossCode::ThreadDetailNative.kind());
        assert!(!note.message.contains("Hand remains native"));
        for detail in [
            "scope",
            "external false",
            "60 degrees",
            "groove-profile table",
        ] {
            assert!(note.message.contains(detail), "{detail}");
        }
        construction.extended = None;
        let note = loss("scope", &construction);
        assert_eq!(note.code, F3dLossCode::ThreadDetailNative.kind());
        assert!(note.message.contains("60 degrees"));
        assert!(!note.message.contains("external"));
        assert!(note.message.contains("Hand remains native"));
        assert!(note.message.contains("Side control remains native"));
    }
}
