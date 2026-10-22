// SPDX-License-Identifier: Apache-2.0
//! Native scalar operands must survive as actual reconstructed markers.

use super::super::*;

fn source() -> SldprtNative {
    crate::test_support::native::complete_marker_source()
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
    assert_eq!(lane.sketch_entities.len(), 3);
    assert_eq!(lane.scalars[0].value.get(), 0.025);
    assert_eq!(lane.scalars[0].entity_indices(), [0, 2]);
    assert!(lane.scalars[0]
        .operands
        .iter()
        .all(|operand| operand.entity_ref.is_some()));
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
            rejected.feature_input_lanes[0].sketch_entities[2]
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
            [
                ("ControlSketch", Some(1)),
                ("Distance", None),
                ("IndependentName", Some(77))
            ]
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
            assert!(losses
                .iter()
                .any(|loss| loss.message.contains(&operand.reference_ref)));
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

#[test]
fn edited_native_scalar_with_actual_marker_operands_writes_current_value() {
    let mut source = source();
    let scalar_id = source.feature_input_lanes[0].scalars[0].id.clone();
    // The original authored lane bytes carry 0.025; current typed content is
    // the edited 0.050. Regeneration must take the current value with all three
    // actual marker ordinals, never return the old source cache.
    source.feature_input_lanes[0].scalars[0].value =
        cadmpeg_ir::scalar::FiniteReal::new(0.050).expect("finite test scalar");
    let (target, losses, dispositions) = regenerate_control(&source);
    assert!(losses.is_empty(), "{losses:#?}");
    let [lane] = target.feature_input_lanes.as_slice() else {
        panic!("one complete native lane");
    };
    assert_eq!(lane.sketch_entities.len(), 3);
    assert_eq!(lane.scalars.len(), 1);
    assert_eq!(
        lane.scalars[0].value.get(),
        0.050,
        "the old source scalar must not return"
    );
    assert_eq!(lane.scalars[0].entity_indices(), [0, 2]);
    assert_eq!(
        lane.scalars[0].operands[0].entity_ref.as_deref(),
        Some(lane.sketch_entities[0].id())
    );
    assert_eq!(
        lane.scalars[0].operands[1].entity_ref.as_deref(),
        Some(lane.sketch_entities[2].id())
    );
    assert_eq!(
        dispositions.record_disposition("sldprt", "feature_input_scalars", &scalar_id),
        NativeDisposition::Regenerated
    );
    assert_eq!(source.feature_input_lanes[0].scalars[0].value.get(), 0.050);
}

#[test]
fn terminal_coordinate_identity_requires_a_complete_record_boundary() {
    let mut point = Vec::new();
    crate::resolved_features::write_generate::append_coordinate_marker(
        &mut point,
        SketchInputKind::Point,
        [0.003, 0.004],
        7,
    );
    assert_eq!(
        crate::resolved_features::markers::marker_local_id(&point, 0),
        Some(7)
    );
    let mut truncated_class = point.clone();
    truncated_class.extend_from_slice(crate::resolved_features::CLASS_MARKER);
    truncated_class.extend_from_slice(&20u16.to_le_bytes());
    truncated_class.extend_from_slice(b"short");
    assert_eq!(
        crate::resolved_features::markers::marker_local_id(&truncated_class, 0),
        None
    );
    let mut unknown_tail = point.clone();
    unknown_tail.extend_from_slice(b"unknown trailing fields");
    assert_eq!(
        crate::resolved_features::markers::marker_local_id(&unknown_tail, 0),
        None
    );
    let mut reserved_field = point;
    reserved_field[82] = 1;
    assert_eq!(
        crate::resolved_features::markers::marker_local_id(&reserved_field, 0),
        None
    );
}
