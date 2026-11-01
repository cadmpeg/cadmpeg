// SPDX-License-Identifier: Apache-2.0

use crate::feature::tests::{reference, reference_list, test_label};
use crate::record_identity::Located;
use crate::test_support::mirror::new_body_form;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};

#[test]
fn mirror_new_body_uses_the_source_collection_and_generated_result() {
    let (fixture, source) = new_body_form();
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let (feature, result) = fixture
        .index()
        .project(&ctx, &source, &fixture.label, |_| None, |_| {})
        .expect("new body")
        .expect("projection");
    let FeatureDefinition::Operation(FeatureOperation::MirrorShape {
        source: selected, ..
    }) = feature.evaluation.definition()
    else {
        panic!("mirror shape")
    };
    assert_eq!(*selected, BodySelection::Native(fixture.properties[2].id()));
    assert_eq!(
        result.bodies()[0].as_str(),
        fixture.properties.last().expect("output").id()
    );
    assert!(fixture
        .index()
        .matches_projection(&source, &fixture.label, &feature, &result, |_| None));
}

#[test]
fn generated_body_checks_each_variable_word_and_its_type() {
    for ordinal in [201, 202, 203] {
        for field in ["name_value", "nominal_value", "model_value", "type_id"] {
            let (mut fixture, source) = new_body_form();
            let index = fixture
                .properties
                .iter()
                .position(|p| p.identity.record_ordinal == ordinal)
                .expect("variable");
            let mut wire = serde_json::to_value(&fixture.properties[index]).expect("variable");
            if field == "type_id" {
                wire[field] =
                    serde_json::json!(crate::pmdc::type_id_string(crate::feature::BOOLEAN_TYPE));
            } else {
                let value = wire["kind"][field].as_u64().expect("word");
                wire["kind"][field] = serde_json::json!(value + 1);
            }
            fixture.properties[index] = serde_json::from_value(wire).expect("changed variable");
            assert!(
                fixture
                    .index()
                    .resolve(&source, &fixture.label, |_| None)
                    .is_none(),
                "{ordinal} {field}"
            );
        }
    }
}

#[test]
fn generated_body_checks_node_flags_outline_value_and_property_count() {
    for change in 0..5 {
        let (mut fixture, source) = new_body_form();
        let node = &fixture.generated_features[0];
        let mut payload = (**node).clone();
        match change {
            0 => payload.header.flags = 0x86200,
            1 => payload.outline_value = 0,
            2 => payload.value = 1,
            3 => payload.properties = reference_list(&[202, 203, 204]),
            _ => payload.properties = reference_list(&[202, 203, 204, 205, 206]),
        }
        fixture.generated_features[0] = Located::new(
            payload,
            node.identity.type_id.clone(),
            node.identity.segment_token.clone(),
            node.identity.record_ordinal,
        );
        assert!(fixture
            .index()
            .resolve(&source, &fixture.label, |_| None)
            .is_none());
    }
}

#[test]
fn generated_body_checks_owner_class_source_and_generated_reference() {
    for change in 0..5 {
        let (mut fixture, source) = new_body_form();
        match change {
            0 => fixture.generated_labels[0] = test_label(200, 1, fixture.label.class_id(), &[]),
            1 => fixture.label = test_label(20, 2, fixture.label.class_id(), &[202]),
            2 => fixture
                .generated_features
                .push(fixture.generated_features[0].clone()),
            3 => fixture
                .generated_labels
                .push(fixture.generated_labels[0].clone()),
            _ => {
                let map = fixture
                    .properties
                    .iter_mut()
                    .find(|p| p.identity.record_ordinal == 113)
                    .expect("map");
                let mut wire = serde_json::to_value(&*map).expect("map");
                wire["kind"]["definition"]["source"] =
                    serde_json::to_value(reference(107)).expect("reference");
                *map = serde_json::from_value(wire).expect("changed map");
            }
        }
        assert!(fixture
            .index()
            .resolve(&source, &fixture.label, |_| None)
            .is_none());
    }
}

#[test]
fn body_copy_refusals_emit_the_pinned_instance_map_loss() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    for (name, fixture, source) in crate::test_support::mirror::body_copy_refusal_cases() {
        let mut refusal = None;
        assert!(
            fixture
                .index()
                .project(
                    &ctx,
                    &source,
                    &fixture.label,
                    |_| None,
                    |value| refusal = Some(value)
                )
                .is_none(),
            "{name}"
        );
        let losses = super::super::native_losses(&[source], &[], &[refusal]);
        assert_eq!(losses.len(), 1, "{name}");
        assert_eq!(
            losses[0].code.local_code(),
            "feature.mirror-instance-map-native",
            "{name}"
        );
    }
}

#[test]
fn body_copy_keeps_each_measured_outline_form() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    for outline in [3, 9, 15] {
        let (mut fixture, source) = new_body_form();
        let node = &fixture.generated_features[0];
        let mut payload = (**node).clone();
        payload.outline_value = outline;
        fixture.generated_features[0] = Located::new(
            payload,
            node.identity.type_id.clone(),
            node.identity.segment_token.clone(),
            node.identity.record_ordinal,
        );
        assert!(fixture
            .index()
            .project(&ctx, &source, &fixture.label, |_| None, |_| {})
            .is_some());
    }
}

#[test]
fn replacement_boolean_bytes_select_the_historical_body_and_offset_plane() {
    let (mut fixture, source) = crate::test_support::mirror::replacement_form();
    let mut bytes = crate::test_support::test_fixtures::content_at(102, 19, &[]);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(1);
    let parsed = crate::test_support::test_fixtures::parse(&bytes, |ctx, source| {
        crate::feature::parse_boolean(ctx, source, 19)
    })
    .expect("true Boolean bytes");
    fixture.properties[1] = Located::new(
        parsed,
        crate::record_identity::RecordTypeId::from_bytes(crate::feature::BOOLEAN_TYPE),
        fixture.properties[1].identity.segment_token.clone(),
        102,
    );
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let (feature, result) = fixture
        .index()
        .project(&ctx, &source, &fixture.label, |_| None, |_| {})
        .expect("replacement")
        .expect("projection");
    let FeatureDefinition::Operation(FeatureOperation::MirrorShape {
        source: selected,
        plane_origin,
        plane_normal,
        plane_reference,
    }) = feature.evaluation.definition()
    else {
        panic!("mirror shape")
    };
    assert_eq!(*selected, BodySelection::Native(fixture.properties[2].id()));
    assert_eq!(
        plane_origin.get(),
        cadmpeg_ir::math::Point3::new(20.0, 0.0, 0.0)
    );
    assert_eq!(
        *plane_normal,
        cadmpeg_ir::units::UnitVector3::normalized(cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0))
            .expect("normal")
    );
    assert_eq!(
        *plane_reference,
        Some(cadmpeg_ir::features::FaceSelection::Native(
            fixture.planes[0].id()
        ))
    );
    assert_eq!(result.bodies()[0].as_str(), fixture.properties[3].id());
}
