// SPDX-License-Identifier: Apache-2.0
use std::collections::HashMap;

use super::{decode_stream, FREEDOM_ENTRY_PREFIX};

fn header(out: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(&class_tag);
    out.extend_from_slice(&record_index.to_le_bytes());
}

fn reference(out: &mut Vec<u8>, target: u32) {
    out.push(1);
    out.extend_from_slice(&target.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
}

fn freedom(out: &mut Vec<u8>, code: u32) {
    out.extend_from_slice(&FREEDOM_ENTRY_PREFIX);
    out.extend_from_slice(&code.to_le_bytes());
}

/// Joint record `index`: twenty bytes, its self-reference, the two limit
/// flags, ten bytes, then the maximum and the minimum.
fn joint(out: &mut Vec<u8>, index: u32, limits: Option<(f64, f64)>) {
    header(out, *b"331", index);
    out.extend_from_slice(&[0; 20]);
    reference(out, index);
    match limits {
        Some(_) => out.extend_from_slice(&[1, 1]),
        None => out.extend_from_slice(&[0, 0]),
    }
    out.extend_from_slice(&[0; 10]);
    let (maximum, minimum) = limits.unwrap_or((0.0, 0.0));
    out.extend_from_slice(&maximum.to_le_bytes());
    out.extend_from_slice(&minimum.to_le_bytes());
    out.extend_from_slice(&[0; 8]);
}

/// Scope record `index` referencing joint record `joint`.
fn scope(out: &mut Vec<u8>, index: u32, joint: u32) {
    header(out, *b"285", index);
    out.extend_from_slice(&[0; 10]);
    reference(out, joint);
    out.extend_from_slice(&[0; 8]);
}

/// Current-generation motion record `index` naming `joint` at offset 24.
fn motion(out: &mut Vec<u8>, class_tag: [u8; 3], index: u32, joint: u32, codes: &[u32]) {
    header(out, class_tag, index);
    out.extend_from_slice(&[0; 13]);
    reference(out, joint);
    out.extend_from_slice(&[0; 9]);
    for code in codes {
        freedom(out, *code);
        out.extend_from_slice(&[0; 4]);
    }
    out.extend_from_slice(&[0; 8]);
}

fn codes(bytes: &[u8], scopes: &HashMap<u32, bool>) -> Vec<(u32, u32, Vec<u32>)> {
    decode_stream(bytes, "Design/BulkStream.dat", scopes)
        .into_iter()
        .map(|motion| {
            (
                motion.joint_record_index,
                motion.motion_record_index,
                motion.freedoms.values().copied().collect(),
            )
        })
        .collect()
}

/// A revolute joint and a rigid joint of the current generation: the motion
/// record names its joint at offset 24 and the `Assemble` scope follows the
/// joint record. A partner record repeating the entry names the scope, not the
/// joint, and is not a motion record. The rigid joint has no entry.
#[test]
fn current_generation_motion_names_its_joint_and_limits() {
    let mut bytes = Vec::new();
    joint(&mut bytes, 10, Some((1.5, -1.5)));
    scope(&mut bytes, 11, 10);
    joint(&mut bytes, 12, None);
    scope(&mut bytes, 13, 12);
    let motion_at = bytes.len();
    motion(&mut bytes, *b"368", 20, 10, &[2]);
    motion(&mut bytes, *b"395", 21, 11, &[2]);
    let scopes = HashMap::from([(11, true), (13, true)]);
    assert_eq!(codes(&bytes, &scopes), [(10, 20, vec![2])]);
    let [motion] = decode_stream(&bytes, "Design/BulkStream.dat", &scopes)
        .try_into()
        .unwrap();
    let limits = motion.limits.as_ref().expect("limit lanes");
    assert_eq!((limits.first_flag, limits.second_flag), (1, 1));
    assert_eq!((limits.maximum, limits.minimum), (1.5, -1.5));

    // Source coordinates survive in fidelity, and never in document content.
    let mut fidelity = cadmpeg_ir::SourceFidelity::default();
    motion.retain_source(&mut fidelity);
    for (field, expected) in [
        ("joint_byte_offset", 0),
        ("motion_byte_offset", motion_at),
        ("limits.flags_offset", 42),
        ("limits.values_offset", 54),
        (
            "freedom_offsets.0",
            motion_at + 44 + FREEDOM_ENTRY_PREFIX.len(),
        ),
    ] {
        assert_eq!(
            fidelity.metadata[&format!("{}.{field}", motion.id)],
            expected.to_string()
        );
    }
    let wire = serde_json::to_value(&motion).unwrap();
    assert_eq!(wire["freedoms"], serde_json::json!([2]));
    for field in ["joint_byte_offset", "motion_byte_offset"] {
        assert!(wire.get(field).is_none());
    }
    for field in ["flags_offset", "values_offset"] {
        assert!(wire["limits"].get(field).is_none());
    }
    let restored: crate::records::feature::joint_motion::DesignJointMotion =
        serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(restored.joint_byte_offset, None);
    assert_eq!(restored.motion_byte_offset, None);
    assert!(restored.freedoms.located_rows().is_none());
    let limits = restored.limits.as_ref().unwrap();
    assert_eq!((limits.flags_offset, limits.values_offset), (None, None));
    assert_eq!((limits.first_flag, limits.second_flag), (1, 1));
    assert_eq!((limits.maximum, limits.minimum), (1.5, -1.5));
    let mut unknown = wire.clone();
    unknown["joint_byte_offset"] = serde_json::json!(0);
    assert!(
        serde_json::from_value::<crate::records::feature::joint_motion::DesignJointMotion>(unknown)
            .is_err()
    );
    let mut null = wire.clone();
    null["limits"] = serde_json::Value::Null;
    assert!(
        serde_json::from_value::<crate::records::feature::joint_motion::DesignJointMotion>(null)
            .is_err()
    );
    let mut absent = wire;
    absent.as_object_mut().unwrap().remove("limits");
    assert!(
        serde_json::from_value::<crate::records::feature::joint_motion::DesignJointMotion>(absent)
            .unwrap()
            .limits
            .is_none()
    );

    // The record after the joint is a decoded scope of another family.
    let scopes = HashMap::from([(11, false), (13, true)]);
    assert!(codes(&bytes, &scopes).is_empty());
}

/// Older generation: the motion record follows its joint record and
/// references it anywhere in its body.
#[test]
fn older_generation_motion_follows_its_joint_record() {
    let mut bytes = Vec::new();
    joint(&mut bytes, 30, None);
    header(&mut bytes, *b"357", 31);
    bytes.extend_from_slice(&[0; 30]);
    reference(&mut bytes, 30);
    freedom(&mut bytes, 1);
    bytes.extend_from_slice(&[0; 8]);
    assert_eq!(codes(&bytes, &HashMap::new()), [(30, 31, vec![1])]);
}

/// A freedom entry in a record that names no joint is not a joint motion.
#[test]
fn a_freedom_entry_outside_a_motion_record_is_ignored() {
    let mut bytes = Vec::new();
    header(&mut bytes, *b"300", 40);
    bytes.extend_from_slice(&[0; 20]);
    freedom(&mut bytes, 8);
    bytes.extend_from_slice(&[0; 8]);
    assert!(codes(&bytes, &HashMap::new()).is_empty());
}
