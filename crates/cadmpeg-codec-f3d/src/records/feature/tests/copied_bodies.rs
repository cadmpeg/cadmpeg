// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::body_ops::{CopyPasteRecordLocation, DesignCopyPasteBodiesOperation};
use crate::records::identity::Sourced;

fn copied_wire() -> serde_json::Value {
    serde_json::json!({
        "body_group_record_index":501, "body_group_class_tag":"264",
        "body_operand_record_indices":[502,504],
        "relation_record_index":503, "relation_class_tag":"264",
        "source_body_entity_suffixes":[11,13],
        "copied_body_entity_suffixes":[12,14]
    })
}

/// The admitted bodies of `copied_wire` at the source offsets of a decoded
/// scope.
fn located_bodies(
    operand: [u64; 2],
    source: [u64; 2],
    copied: [u64; 2],
) -> Vec<crate::records::feature::body_ops::DesignCopiedBody> {
    let admitted: DesignCopyPasteBodiesOperation = serde_json::from_value(copied_wire()).unwrap();
    let mut bodies = admitted.bodies().to_vec();
    for (ordinal, body) in bodies.iter_mut().enumerate() {
        body.operand = Sourced::at(body.operand.value, operand[ordinal]);
        body.source = Sourced::at(body.source.value, source[ordinal]);
        body.copied = Sourced::at(body.copied.value, copied[ordinal]);
    }
    bodies
}

fn construct(
    bodies: Vec<crate::records::feature::body_ops::DesignCopiedBody>,
    body_group_byte_offset: u64,
    relation_byte_offset: u64,
) -> Result<DesignCopyPasteBodiesOperation, cadmpeg_core::CodecError> {
    DesignCopyPasteBodiesOperation::try_new(
        bodies,
        CopyPasteRecordLocation {
            record_index: 501,
            class_tag: "264".to_owned().try_into().unwrap(),
            byte_offset: body_group_byte_offset,
        },
        CopyPasteRecordLocation {
            record_index: 503,
            class_tag: "264".to_owned().try_into().unwrap(),
            byte_offset: relation_byte_offset,
        },
    )
}

#[test]
fn copied_bodies_reject_empty_aliased_and_misaligned_rows() {
    let admitted: DesignCopyPasteBodiesOperation = serde_json::from_value(copied_wire()).unwrap();
    let located = || located_bodies([126, 137], [225, 255], [240, 270]);
    let decoded = construct(located(), 100, 200).expect("decoded copied bodies");
    assert_eq!(serde_json::to_value(&decoded).unwrap(), copied_wire());
    assert!(construct(Vec::new(), 100, 200).is_err());
    for lane in 0..5 {
        let mut bodies = located();
        match lane {
            0 => bodies[1].source.value = bodies[0].source.value,
            1 => bodies[1].copied.value = bodies[0].source.value,
            2 => bodies[1].operand.offset = bodies[1].operand.offset.map(|offset| offset + 1),
            3 => bodies[1].source.offset = bodies[1].source.offset.map(|offset| offset + 1),
            _ => bodies[0].copied.offset = bodies[0].copied.offset.map(|offset| offset + 1),
        }
        assert!(construct(bodies, 100, 200).is_err());
    }
    let mut empty = copied_wire();
    for field in [
        "body_operand_record_indices",
        "source_body_entity_suffixes",
        "copied_body_entity_suffixes",
    ] {
        empty[field] = serde_json::json!([]);
    }
    assert!(serde_json::from_value::<DesignCopyPasteBodiesOperation>(empty).is_err());
    for (field, lane, value) in [
        ("source_body_entity_suffixes", 1, 11),
        ("copied_body_entity_suffixes", 1, 11),
    ] {
        let mut wire = copied_wire();
        wire[field][lane] = serde_json::json!(value);
        assert!(
            serde_json::from_value::<DesignCopyPasteBodiesOperation>(wire).is_err(),
            "{field}"
        );
    }
    for (operand, source, copied) in [
        ([127, 137], [225, 255], [240, 270]),
        ([126, 138], [225, 255], [240, 270]),
        ([126, 137], [226, 255], [240, 270]),
        ([126, 137], [225, 256], [240, 270]),
        ([126, 137], [225, 255], [241, 270]),
    ] {
        assert!(construct(located_bodies(operand, source, copied), 100, 200).is_err());
    }
    assert_eq!(serde_json::to_value(admitted).unwrap(), copied_wire());
}

#[test]
fn copied_bodies_reject_overflowed_operand_origin() {
    let bodies = located_bodies([u64::MAX, u64::MAX], [225, 255], [240, 270]);
    assert!(construct(bodies, u64::MAX, 200).is_err());
}

#[test]
fn copied_bodies_reject_overflowed_source_origin() {
    let bodies = located_bodies([126, 137], [u64::MAX, u64::MAX], [u64::MAX, u64::MAX]);
    assert!(construct(bodies, 100, u64::MAX).is_err());
}

#[test]
fn copied_bodies_reject_overflowed_copied_offset() {
    let bodies = located_bodies([126, 137], [u64::MAX, u64::MAX], [u64::MAX, u64::MAX]);
    assert!(construct(bodies, 100, u64::MAX - 25).is_err());
}

#[test]
fn copied_bodies_reject_overflowed_operand_stride() {
    let bodies = located_bodies([u64::MAX, u64::MAX], [225, 255], [240, 270]);
    assert!(construct(bodies, u64::MAX - 26, 200).is_err());
}

#[test]
fn copied_bodies_reject_overflowed_source_stride() {
    let bodies = located_bodies(
        [126, 137],
        [u64::MAX - 25, u64::MAX],
        [u64::MAX - 10, u64::MAX],
    );
    assert!(construct(bodies, 100, u64::MAX - 50).is_err());
}
