// SPDX-License-Identifier: Apache-2.0
//! Native-namespace load and store helpers for crate tests.
#![allow(clippy::unwrap_used)]

pub(crate) fn sldprt_native(ir: &cadmpeg_ir::CadIr) -> crate::native::SldprtNative {
    crate::native::SldprtNative::load(
        ir.native
            .namespace("sldprt")
            .expect("SLDPRT native namespace"),
    )
    .unwrap()
}

pub(crate) fn update_sldprt_native<R>(
    ir: &mut cadmpeg_ir::CadIr,
    update: impl FnOnce(&mut crate::native::SldprtNative) -> R,
) -> R {
    let mut native = sldprt_native(ir);
    let result = update(&mut native);
    native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            ir.native.namespace_mut("sldprt"),
        )
        .unwrap();
    result
}

/// Read byte-backed statements through their retained source-fidelity boundary.
pub(crate) fn sldprt_native_with_source(
    ir: &cadmpeg_ir::CadIr,
    fidelity: &cadmpeg_ir::SourceFidelity,
) -> crate::native::SldprtNative {
    crate::native::source::load(
        ir.native
            .namespace("sldprt")
            .expect("SLDPRT native namespace"),
        fidelity,
    )
    .expect("source-fidelity admission")
}

/// Rule B preserves independent typed declarations while opaque marker framing
/// and scalar relations without reconstructible operands are omitted explicitly.
pub(crate) fn assert_marker_framing_omission(
    report: &cadmpeg_ir::report::export::ExportReport,
    source: &crate::native::SldprtNative,
    output: &crate::native::SldprtNative,
) {
    use cadmpeg_ir::codec::write::NativeDisposition::{Dropped, Regenerated};
    let classes = |native: &crate::native::SldprtNative| {
        native
            .feature_input_lanes
            .iter()
            .flat_map(|lane| &lane.classes)
            .map(|class| class.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        classes(source),
        [
            "sgPointHandle",
            "sgLineHandle",
            "sgArcHandle",
            "sgPntPntDist"
        ]
    );
    assert_eq!(classes(output), classes(source));
    let names = |native: &crate::native::SldprtNative| {
        native
            .feature_input_lanes
            .iter()
            .flat_map(|lane| &lane.names)
            .map(|name| (name.value.clone(), name.object_id))
            .collect::<Vec<_>>()
    };
    let expected_names = source
        .feature_input_lanes
        .iter()
        .flat_map(|lane| lane.names.iter().filter(|name| name.object_id.is_some()))
        .map(|name| (name.value.clone(), name.object_id))
        .collect::<Vec<_>>();
    assert_eq!(
        names(output),
        expected_names,
        "source={source:#?}; output={output:#?}; report={report:#?}"
    );
    for lane in &output.feature_input_lanes {
        assert!(lane.sketch_entities.is_empty());
        assert!(lane.references.is_empty());
        assert!(lane.scalars.is_empty());
        assert!(lane.relation_bindings.is_empty());
        assert!(lane.relation_instances.is_empty());
    }
    for lane in &source.feature_input_lanes {
        for class in &lane.classes {
            assert_eq!(
                report.native_dispositions.record_disposition(
                    "sldprt",
                    "feature_input_classes",
                    &class.id
                ),
                Regenerated
            );
        }
        for name in lane.names.iter().filter(|name| name.object_id.is_some()) {
            assert_eq!(
                report.native_dispositions.record_disposition(
                    "sldprt",
                    "feature_input_names",
                    &name.id
                ),
                Regenerated
            );
        }
        for marker in &lane.sketch_entities {
            assert_eq!(
                report.native_dispositions.record_disposition(
                    "sldprt",
                    "sketch_input_entities",
                    marker.id()
                ),
                Dropped
            );
        }
        for (arena, id) in lane
            .scalars
            .iter()
            .map(|scalar| ("feature_input_scalars", scalar.id.as_str()))
            .chain(
                lane.references
                    .iter()
                    .map(|reference| ("feature_input_references", reference.id.as_str())),
            )
        {
            assert_eq!(
                report
                    .native_dispositions
                    .record_disposition("sldprt", arena, id),
                Dropped
            );
            assert!(report.losses.iter().any(|loss| loss.code
                == crate::loss::SldprtLossCode::WriterNativeRecordsOmitted.kind()
                && loss.message.contains(arena)
                && loss.message.contains(id)));
        }
    }
}

/// A complete native point-distance lane with real ordinal operands 0 and 2.
pub(crate) fn complete_marker_source() -> crate::native::SldprtNative {
    use crate::native::SldprtNative;
    use crate::records::SketchInputKind;
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
    // D6 operands address compatible point ordinals, not object indexes.
    // Keep original operands [0, 2] and provide the actual middle point.
    for (index, id, coordinates) in [
        (0u32, 1u32, [0.001, 0.002]),
        (1, 2, [0.002, 0.003]),
        (2, 3, [0.003, 0.004]),
    ] {
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
    assert_eq!(lane.sketch_entities.len(), 3);
    assert_eq!(lane.scalars.len(), 1);
    assert!(lane.scalars[0]
        .operands
        .iter()
        .all(|operand| operand.entity_ref.is_some()));
    assert_eq!(
        lane.scalars[0].operands[0].entity_ref.as_deref(),
        Some(lane.sketch_entities[0].id())
    );
    assert_eq!(
        lane.scalars[0].operands[1].entity_ref.as_deref(),
        Some(lane.sketch_entities[2].id())
    );
    SldprtNative {
        feature_histories: vec![history],
        feature_input_lanes: vec![lane],
        ..Default::default()
    }
}
