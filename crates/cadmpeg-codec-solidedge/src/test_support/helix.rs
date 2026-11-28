// SPDX-License-Identifier: Apache-2.0
//! Synthetic helical protrusion documents (`docs/formats/solidedge.md` §6.5).

use super::{
    feature_with_groups, part_body_with_profile, profile_group, slot_object, Member2d,
    ProfileFrame, Slot, EXTENT_CLASSES, EXTENT_SET,
};

/// The first slot object of a helix document.
const SLOT_OBJECT: u32 = 600;

/// The slots of a helical protrusion, in metres.
#[derive(Clone)]
pub(crate) struct HelixSpec {
    pub(crate) axis_point: [f64; 3],
    pub(crate) axis_direction: [f64; 3],
    /// The start-end value: 29 or 30.
    pub(crate) from: f64,
    /// The extent kind.
    pub(crate) kind: f64,
    pub(crate) height: f64,
    pub(crate) pitch: f64,
    pub(crate) turns: f64,
    /// The hand value: 1 or 2.
    pub(crate) hand: f64,
}

impl Default for HelixSpec {
    fn default() -> Self {
        Self {
            axis_point: [0.0, 0.0, 0.0],
            axis_direction: [0.0, 1.0, 0.0],
            from: 29.0,
            kind: 13.0,
            height: 0.02,
            pitch: 0.005,
            turns: 4.0,
            hand: 2.0,
        }
    }
}

/// A document with one `JMHelixProtrusionFunction` feature, object 10, whose
/// profile is the group of `members` on `frame` (128 bytes after the name)
/// and whose slots hold `spec`. The unsettled slots hold `0` and, at 520,
/// `1`.
pub(crate) fn helix_document(
    frame: &ProfileFrame,
    members: &[Member2d],
    spec: &HelixSpec,
) -> Vec<u8> {
    let mut body = part_body_with_profile("JMHelixProtrusionFunction", 128, EXTENT_SET + 1);
    let end = body.len() - 128 - 4;
    body.resize(end + 520 + 4, 0);
    let mut extra = profile_group(EXTENT_SET, frame, 1, 200, members);
    let table = [
        (
            68,
            Slot::Axis {
                point: spec.axis_point,
                direction: spec.axis_direction,
            },
        ),
        (96, Slot::Double(spec.from)),
        (156, Slot::Double(spec.kind)),
        (184, Slot::Double(spec.height)),
        (212, Slot::Double(spec.pitch)),
        (240, Slot::Double(spec.turns)),
        (268, Slot::Double(spec.hand)),
        (352, Slot::Double(0.0)),
        (408, Slot::Double(0.0)),
        (520, Slot::Double(1.0)),
    ];
    for (index, (after, slot)) in table.into_iter().enumerate() {
        let id = SLOT_OBJECT + u32::try_from(index).unwrap();
        if let Some(object) = slot_object(id, slot) {
            let at = end + after;
            body[at..at + 4].copy_from_slice(&id.to_le_bytes());
            extra.push(object);
        }
    }
    feature_with_groups(body, extra, &EXTENT_CLASSES)
}
