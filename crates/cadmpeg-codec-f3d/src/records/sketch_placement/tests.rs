// SPDX-License-Identifier: Apache-2.0

use super::{DesignSketchPlacement, DesignSketchPlacementWire, SKETCH_PLACEMENT_CLONE_COUNT};

fn placement(member: bool) -> DesignSketchPlacement {
    let mut wire = serde_json::json!({
        "id":"f3d:native:sketch-placement#0", "entity_id":"0_35", "class_tag":"305", "record_index":5,
        "transform":[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],
        "paired_class_tag":"306"
    });
    if member {
        wire["member_run_head"] = true.into();
    }
    serde_json::from_value(wire).unwrap()
}

#[test]
fn sketch_placement_borrowed_wire_matches_owned_wire_bytes() {
    for record in [placement(false), placement(true)] {
        let owned = DesignSketchPlacementWire::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn sketch_placement_native_retained_limit_refuses_before_record_clone() {
    let record = placement(true);
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_sketch_placements",
        || SKETCH_PLACEMENT_CLONE_COUNT.with(|count| count.set(0)),
        || SKETCH_PLACEMENT_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn placement_document_preserves_transform_without_implied_source_layout() {
    for member in [false, true] {
        let mut wire = serde_json::to_value(placement(member)).unwrap();
        wire["transform"][0][3] = 12.0.into();
        let record: DesignSketchPlacement = serde_json::from_value(wire.clone()).unwrap();
        assert!(record.byte_offset().is_none());
        assert!(record.frame_length().is_none());
        assert!(record.paired_byte_offset().is_none());
        assert!(record.transform_offset().is_none());
        assert_eq!(record.member_run_head(), member);
        assert_eq!(serde_json::to_value(&record).unwrap(), wire);
        let mut fidelity = cadmpeg_ir::SourceFidelity::default();
        record.retain_source(&mut fidelity);
        assert!(fidelity.metadata.is_empty());
        for field in [
            "byte_offset",
            "frame_length",
            "transform_offset",
            "paired_byte_offset",
        ] {
            let mut invalid = wire.clone();
            invalid[field] = 0.into();
            assert!(serde_json::from_value::<DesignSketchPlacement>(invalid)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
    }
}

#[test]
fn sketch_visibility_collection_wire_requires_omission_or_a_positive_identity() {
    let mut visibility =
        super::DesignSketchVisibility::new(std::num::NonZeroU32::new(2).unwrap(), 30, false)
            .unwrap();
    visibility.ordinal_collection = std::num::NonZeroU64::new(300);
    let wire = serde_json::to_value(visibility).unwrap();
    let decoded = serde_json::from_value::<super::DesignSketchVisibility>(wire.clone()).unwrap();
    assert_eq!(
        (
            decoded.stream_ordinal,
            decoded.visible,
            decoded.ordinal_collection
        ),
        (
            visibility.stream_ordinal,
            visibility.visible,
            visibility.ordinal_collection
        )
    );
    let mut omitted = wire.clone();
    omitted
        .as_object_mut()
        .unwrap()
        .remove("ordinal_collection");
    assert!(
        serde_json::from_value::<super::DesignSketchVisibility>(omitted)
            .unwrap()
            .ordinal_collection
            .is_none()
    );
    for invalid in [serde_json::Value::Null, serde_json::json!(0)] {
        let mut invalid_wire = wire.clone();
        invalid_wire["ordinal_collection"] = invalid;
        assert!(serde_json::from_value::<super::DesignSketchVisibility>(invalid_wire).is_err());
    }
}
