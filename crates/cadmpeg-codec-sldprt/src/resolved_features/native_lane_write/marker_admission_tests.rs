// SPDX-License-Identifier: Apache-2.0
//! Native scalar operands must survive as actual reconstructed markers.

use super::*;

fn source() -> SldprtNative {
    let history = crate::records::FeatureHistory {
        id: "sldprt:history#operand-control".into(),
        part_name: None,
        properties: Default::default(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![crate::records::Feature {
            id: "sldprt:history:feature#operand-control".into(),
            parent: "sldprt:history#operand-control".into(),
            xml_tag: "Sketch".into(),
            tree_parent: None,
            source_id: crate::records::FeatureSource::from_value(1),
            ordinal: 0,
            name: "ControlSketch".into(),
            kind: "Sketch".into(),
            input_class: Some("moProfileFeature_c".into()),
            suppressed: false,
            parameters: Default::default(),
            dimension_properties: Default::default(),
            properties: Default::default(),
            text: None,
            content: Vec::new(),
        }],
    };
    let mut payload = Vec::new();
    crate::resolved_features::write_generate::append_generated_object_name(
        &mut payload,
        "ControlSketch",
        1,
    )
    .unwrap();
    for (index, id, coordinates) in [(0u32, 1u32, [0.001, 0.002]), (2, 2, [0.003, 0.004])] {
        payload.extend_from_slice(&index.to_le_bytes());
        crate::resolved_features::write_generate::append_coordinate_marker(
            &mut payload,
            SketchInputKind::Point,
            coordinates,
            id,
        );
    }
    crate::resolved_features::write_generate::append_generated_scalar(
        &mut payload,
        "sgPntPntDist",
        "Distance",
        0.025,
        17,
        &[
            (crate::records::FeatureInputOperandKind::D6, 0),
            (crate::records::FeatureInputOperandKind::D6, 2),
        ],
    )
    .unwrap();
    crate::resolved_features::write_generate::append_generated_object_name(
        &mut payload,
        "IndependentName",
        77,
    )
    .unwrap();
    let lane = crate::resolved_features::write_prepare::generated_native_lane(
        std::slice::from_ref(&history),
        "test:native:lane#operand-control",
        Some("0".into()),
        payload,
    )
    .unwrap();
    assert_eq!(lane.sketch_entities.len(), 2);
    assert_eq!(lane.scalars.len(), 1);
    assert!(
        lane.scalars[0]
            .operands
            .iter()
            .all(|operand| operand.entity_ref.is_some())
    );
    SldprtNative {
        feature_histories: vec![history],
        feature_input_lanes: vec![lane],
        ..Default::default()
    }
}

fn regenerate_control(source: &SldprtNative) -> (SldprtNative, Vec<LossNote>, NativeDispositions) {
    let mut ir = CadIr::empty();
    ir.set_scope(cadmpeg_ir::ContentScope::container()).unwrap();
    let mut target = source.clone();
    target.feature_input_lanes.clear();
    let mut losses = Vec::new();
    let mut dispositions = NativeDispositions::for_namespace(
        "sldprt",
        crate::native::SLDPRT_ARENA_NAMES,
        NativeDisposition::Dropped,
    );
    regenerate(
        &ir,
        &ir,
        source,
        &mut target,
        &mut losses,
        &mut dispositions,
    )
    .unwrap();
    (target, losses, dispositions)
}

#[test]
fn linked_native_scalar_requires_reconstructible_markers_but_keeps_independent_names() {
    let source = source();
    let (target, losses, dispositions) = regenerate_control(&source);
    assert!(losses.is_empty(), "{losses:#?}");
    let [lane] = target.feature_input_lanes.as_slice() else {
        panic!("one valid lane");
    };
    assert_eq!(lane.sketch_entities.len(), 2);
    assert_eq!(lane.scalars[0].value.get(), 0.025);
    assert_eq!(lane.scalars[0].entity_indices(), [0, 2]);
    assert!(
        lane.scalars[0]
            .operands
            .iter()
            .all(|operand| operand.entity_ref.is_some())
    );
    assert_eq!(
        dispositions.record_disposition(
            "sldprt",
            "feature_input_scalars",
            &source.feature_input_lanes[0].scalars[0].id
        ),
        NativeDisposition::Regenerated
    );
    for unresolved in [false, true] {
        let mut rejected = source.clone();
        if unresolved {
            rejected.feature_input_lanes[0].scalars[0].operands[1].entity_ref = None;
        } else {
            rejected.feature_input_lanes[0].sketch_entities[1]
                .reclassify(SketchInputKind::from_native_code(9));
        }
        let (target, losses, dispositions) = regenerate_control(&rejected);
        let [lane] = target.feature_input_lanes.as_slice() else {
            panic!("independent typed facts survive");
        };
        assert!(lane.scalars.is_empty());
        assert!(lane.references.is_empty());
        assert!(lane.relation_bindings.is_empty());
        assert!(lane.relation_instances.is_empty());
        assert_eq!(
            lane.classes
                .iter()
                .map(|class| class.name.as_str())
                .collect::<Vec<_>>(),
            ["sgPntPntDist"]
        );
        assert_eq!(
            lane.names
                .iter()
                .map(|name| (
                    name.value.as_str(),
                    name.object_id.and_then(crate::records::ObjectId::value)
                ))
                .collect::<Vec<_>>(),
            [("ControlSketch", Some(1)), ("IndependentName", Some(77))]
        );
        let scalar = &rejected.feature_input_lanes[0].scalars[0];
        assert_eq!(
            dispositions.record_disposition("sldprt", "feature_input_scalars", &scalar.id),
            NativeDisposition::Dropped
        );
        assert!(losses.iter().any(|loss| {
            loss.message.contains(&scalar.id)
                && loss
                    .message
                    .contains("typed scalar operand no longer resolves")
        }));
        for operand in &scalar.operands {
            assert_eq!(
                dispositions.record_disposition(
                    "sldprt",
                    "feature_input_references",
                    &operand.reference_ref
                ),
                NativeDisposition::Dropped
            );
            assert!(
                losses
                    .iter()
                    .any(|loss| loss.message.contains(&operand.reference_ref))
            );
        }
    }
}

#[test]
fn genuinely_untyped_marker_only_lane_is_omitted() {
    let mut source = source();
    let lane = &mut source.feature_input_lanes[0];
    lane.classes.clear();
    lane.names.clear();
    lane.scalars.clear();
    lane.references.clear();
    lane.relation_bindings.clear();
    lane.relation_instances.clear();
    for marker in &mut lane.sketch_entities {
        marker.reclassify(SketchInputKind::from_native_code(9));
    }
    let lane_id = lane.id.clone();
    let marker_ids = lane
        .sketch_entities
        .iter()
        .map(|marker| marker.id().to_owned())
        .collect::<Vec<_>>();
    let (target, losses, dispositions) = regenerate_control(&source);
    assert!(target.feature_input_lanes.is_empty());
    assert_eq!(
        dispositions.record_disposition("sldprt", "feature_input_lanes", &lane_id),
        NativeDisposition::Dropped
    );
    for marker_id in &marker_ids {
        assert_eq!(
            dispositions.record_disposition("sldprt", "sketch_input_entities", marker_id),
            NativeDisposition::Dropped
        );
        assert!(losses.iter().any(|loss| loss.message.contains(marker_id)));
    }
}
