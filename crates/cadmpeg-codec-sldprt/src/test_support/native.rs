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
            .map(|name| {
                (
                    name.value.clone(),
                    name.object_id.and_then(crate::records::ObjectId::value),
                )
            })
            .collect::<Vec<_>>()
    };
    let expected_names = source
        .feature_input_lanes
        .iter()
        .flat_map(|lane| {
            lane.names.iter().filter(|name| {
                name.object_id
                    .and_then(crate::records::ObjectId::value)
                    .is_some()
                    && !lane.scalars.iter().any(|scalar| scalar.name == name.id)
            })
        })
        .map(|name| {
            (
                name.value.clone(),
                name.object_id.and_then(crate::records::ObjectId::value),
            )
        })
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
