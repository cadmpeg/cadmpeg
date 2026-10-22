// SPDX-License-Identifier: Apache-2.0

use super::{
    DesignClassTag, DesignCompanionPayload, DesignParameter, DesignParameterCompanion,
    DesignParameterCompanionWire, DesignParameterDiscriminator, DesignParameterDraft,
    DesignParameterOwner, DesignParameterOwnerContent, DesignParameterOwnerWire,
    DesignParameterSerde, DesignParameterSource, PARAMETER_CLONE_COUNT,
    PARAMETER_COMPANION_CLONE_COUNT, PARAMETER_OWNER_CLONE_COUNT,
};
use crate::records::identity::{Located, RecordedValue};

fn parameter(owned: bool) -> DesignParameter {
    let source = if owned {
        DesignParameterSource::new("Feature Dimension".into(), Some(4), None).unwrap()
    } else {
        DesignParameterSource::User {
            family_discriminator: Some(Located {
                value: DesignParameterDiscriminator::Code0,
                offset: Some(122),
            }),
        }
    };
    DesignParameter::try_from(DesignParameterDraft {
        id: "f3d:native:parameter#0".into(),
        byte_offset: 100,
        class_tag: DesignClassTag::try_from("305".to_owned()).unwrap(),
        record_index: 1,
        source_ordinal: 0,
        source,
        expression: "-1 mm".into(),
        expression_offset: 140,
        source_kind_offset: 160,
        unit: (!owned).then(|| RecordedValue {
            value: "mm".into(),
            offset: 170,
        }),
        name: "Width".into(),
        name_offset: 180,
        evaluated_value: -1.0,
        evaluated_value_offset: 190,
    })
    .unwrap()
}

#[test]
fn parameter_borrowed_wire_matches_owned_wire_bytes() {
    for record in [parameter(false), parameter(true)] {
        let owned = DesignParameterSerde::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn parameter_native_retained_limit_refuses_before_record_clone() {
    let record = parameter(false);
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameters",
        || PARAMETER_CLONE_COUNT.with(|count| count.set(0)),
        || PARAMETER_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

fn owner(variant: Option<u8>) -> DesignParameterOwner {
    DesignParameterOwner::try_from(DesignParameterOwnerWire {
        id: "f3d:native:parameter-owner#0".into(),
        byte_offset: 100,
        frame_length: if variant.is_some() { 104 } else { 103 },
        class_tag: DesignClassTag::try_from("292".to_owned()).unwrap(),
        record_index: 10,
        scope_record_index: 3,
        local_ordinal: 0,
        evaluated_value: 2.0,
        evaluated_value_offset: 140,
        parameter_record_index: 11,
        owned_ordinal: 0,
        variant,
        companion_record_index: 12,
    })
    .unwrap()
}

fn companion(bound: bool) -> DesignParameterCompanion {
    let record = DesignParameterCompanion::unbound(
        "f3d:native:parameter-companion#0".into(),
        201,
        DesignClassTag::try_from("293".to_owned()).unwrap(),
        12,
        10,
        std::num::NonZeroU64::new(1).unwrap(),
        230,
    );
    if bound {
        record.bound(DesignCompanionPayload::new(
            240,
            50,
            vec!["recipe-a".into(), "recipe-b".into()],
        ))
    } else {
        record
    }
}

#[test]
fn parameter_owner_borrowed_wire_matches_owned_wire_bytes() {
    for record in [owner(None), owner(Some(0))] {
        let owned = DesignParameterOwnerContent::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn parameter_owner_native_retained_limit_refuses_before_record_clone() {
    let record = owner(Some(0));
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_owners",
        || PARAMETER_OWNER_CLONE_COUNT.with(|count| count.set(0)),
        || PARAMETER_OWNER_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn parameter_companion_borrowed_wire_matches_owned_wire_bytes() {
    for record in [companion(false), companion(true)] {
        let owned = DesignParameterCompanionWire::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn parameter_companion_native_retained_limit_refuses_before_record_clone() {
    let record = companion(true);
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_companions",
        || PARAMETER_COMPANION_CLONE_COUNT.with(|count| count.set(0)),
        || PARAMETER_COMPANION_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn authored_parameter_has_semantic_content_without_source_layout() {
    use super::{DesignParameterContent, DesignParameterDiscriminator};
    let source = DesignParameterSource::authored(
        "User Parameter".into(),
        None,
        Some(DesignParameterDiscriminator::Code0),
    )
    .unwrap();
    let parameter = DesignParameter::authored(DesignParameterContent {
        id: "f3d:Design:design-parameter#12".into(),
        class_tag: "365".to_owned().try_into().unwrap(),
        record_index: 12,
        source_ordinal: 0,
        source,
        expression: "7 mm".into(),
        unit: Some("mm".into()),
        name: "Length".into(),
        evaluated_value: 7.0,
    })
    .unwrap();
    assert!(parameter.byte_offset().is_none());
    assert!(parameter.evaluated_value_offset().is_none());
    assert!(parameter.unit().unwrap().offset.is_none());
    assert!(parameter.family_discriminator().unwrap().offset.is_none());
    let wire = serde_json::to_value(&parameter).unwrap();
    assert!(wire
        .as_object()
        .unwrap()
        .keys()
        .all(|key| !key.ends_with("_offset")));
    let restored: DesignParameter = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(restored).unwrap(), wire);
    let mut fidelity = cadmpeg_ir::SourceFidelity::default();
    parameter.retain_source(&mut fidelity, None);
    assert!(fidelity.metadata.is_empty());
    assert!(fidelity.retained_records().is_empty());
}

#[test]
fn semantic_owner_preserves_absent_scope_and_cached_value() {
    let mut value = serde_json::to_value(owner(None)).unwrap();
    value.as_object_mut().unwrap().remove("scope_record_index");
    value.as_object_mut().unwrap().remove("local_ordinal");
    let restored: DesignParameterOwner = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(restored.scope_record_index(), None);
    assert_eq!(restored.local_ordinal(), None);
    assert_eq!(restored.evaluated_value().get(), 2.0);
    assert_eq!(restored.byte_offset(), None);
    assert_eq!(restored.evaluated_value_offset(), None);
    assert_eq!(serde_json::to_value(&restored).unwrap(), value);
    value["local_ordinal"] = serde_json::json!(0);
    assert!(serde_json::from_value::<DesignParameterOwner>(value).is_err());
}

#[test]
fn semantic_companion_preserves_bound_empty_payload_without_source_layout() {
    let unbound = serde_json::to_value(companion(false)).unwrap();
    let bound = companion(false).bound(DesignCompanionPayload::new(240, 0, Vec::new()));
    let bound = serde_json::to_value(bound).unwrap();
    assert!(unbound.get("owned_recipe_ids").is_none());
    assert_eq!(bound["owned_recipe_ids"], serde_json::json!([]));
    for value in [unbound, bound] {
        assert!(value
            .as_object()
            .unwrap()
            .keys()
            .all(|key| !key.contains("offset") && !key.contains("length")));
        let restored: DesignParameterCompanion = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(restored.byte_offset(), None);
        assert_eq!(restored.timestamp_micros_offset(), None);
        assert!(restored.payload().is_none_or(
            |payload| payload.byte_offset().is_none() && payload.byte_length().is_none()
        ));
        assert_eq!(serde_json::to_value(restored).unwrap(), value);
    }
}
