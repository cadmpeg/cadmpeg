// SPDX-License-Identifier: Apache-2.0
//! Source pools that end at the freeform model transfer.

use crate::families::FamilyEntityAdmission;
use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
use cadmpeg_ir::{AnnotationBuilder, CadIr};

fn decode_stream(stream: &[u8]) -> crate::families::FamilyOutput {
    let bytes = crate::test_support::test_container::standard_catpart_from_streams(stream, &[]);
    crate::test_support::with_service_context(|ctx| {
        let scan = crate::container::scan_bytes(ctx, bytes.as_slice())?;
        super::super::try_decode_freeform_surfaces(
            ctx,
            &scan,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("source pool is temporary")
    .expect("recognized freeform family")
}

#[test]
fn freeform_line_and_wire_source_backing_is_temporary() {
    let bytes = crate::test_support::test_b2::b2_line_profile_stream();
    for operation in [
        "catia_consolidated_line_profiles",
        "catia_freeform_standalone_wires",
        "catia_freeform_standalone_wire_id",
    ] {
        let _probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, operation, None);
        let output = decode_stream(&bytes);
        assert_eq!(output.ir.model.curves.len(), 1);
        assert_eq!(output.ir.model.edges.len(), 1);
        assert_eq!(
            output.ir.model.edges[0]
                .param_range()
                .map(cadmpeg_ir::units::FiniteVector::get),
            Some([-4.0, 9.0])
        );
    }
}

#[test]
fn freeform_spatial_circle_source_backing_is_temporary() {
    let mut bytes = vec![0xb2, 0x03, 0x0f, 112, 0x05];
    for value in [
        0.0_f64, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 7.0, 0.0, 11.2, 1.0, 0.0,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for operation in [
        "catia_freeform_spatial_circles",
        "catia_freeform_standalone_wires",
        "catia_freeform_b2_circle_id",
    ] {
        let _probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, operation, None);
        let output = decode_stream(&bytes);
        assert_eq!(output.ir.model.curves.len(), 1);
        assert_eq!(output.ir.model.edges.len(), 1);
        assert_eq!(
            output.ir.model.edges[0]
                .param_range()
                .map(cadmpeg_ir::units::FiniteVector::get),
            Some([0.0, 11.2 / 7.0])
        );
    }
}

#[test]
fn freeform_resolved_revolution_source_backing_is_temporary() {
    let _probe = RefusalProbe::arm(
        ResourceDimension::RetainedBytes,
        "catia_b2_resolved_revolutions",
        None,
    );
    let output = decode_stream(&crate::test_support::test_b2::b2_resolved_revolution_stream());
    assert_eq!(output.ir.model.curves.len(), 1);
    assert_eq!(output.ir.model.surfaces.len(), 1);
    assert_eq!(output.ir.model.procedural_surfaces.len(), 1);
}

#[test]
fn freeform_recognition_keeps_a8_jets_in_temporary_storage() {
    for operation in ["catia_a8_freeform_curves", "catia_a8_freeform_sites"] {
        let _probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, operation, None);
        let output = decode_stream(&crate::test_support::test_a5a8::a8_freeform_curve_stream());
        assert_eq!(output.ir.model.surfaces.len(), 1);
        assert_eq!(output.ir.model.procedural_surfaces.len(), 1);
    }
}

#[test]
fn cached_a8_jets_emit_without_another_frame_scan() {
    let bytes = crate::test_support::test_a5a8::a8_freeform_curve_stream();
    crate::test_support::with_service_context(|ctx| {
        let jets = ctx
            .with_scoped_storage("cached test jets", || {
                crate::families::a5a8::records::a8_freeform_curves(ctx, &bytes)
            })
            .expect("cached jet pool");
        assert_eq!(jets.0.len(), 1);
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "catia_a8_frame_scan", None);
        let mut ir = CadIr::empty();
        super::super::append_a8_rolling_ball_curves(
            &mut ir,
            &mut AnnotationBuilder::new(),
            &jets.0,
            &mut FamilyEntityAdmission::new(ctx),
        )
        .expect("cached emitter never scans frames");
        assert_eq!(ir.model.surfaces.len(), 1);
        assert_eq!(ir.model.procedural_surfaces.len(), 1);
    });
}

#[test]
fn revolution_binding_collection_requires_a_reader_and_scoped_storage() {
    let bytes = crate::test_support::test_b2::b2_resolved_revolution_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    crate::test_support::with_service_context(|ctx| {
        let resolved = ctx
            .with_scoped_storage("test resolved revolutions", || {
                crate::families::b2::records::b2_resolved_revolutions_from_records(
                    ctx, &bytes, &records,
                )
            })
            .expect("resolved revolution");
        for collect_bindings in [false, true] {
            let dimension = if collect_bindings {
                ResourceDimension::RetainedBytes
            } else {
                ResourceDimension::CollectionItems
            };
            let _probe = RefusalProbe::arm(dimension, "catia_revolution_bindings", None);
            let mut ir = CadIr::empty();
            let bindings = super::super::append_consolidated_revolutions(
                &mut ir,
                &mut AnnotationBuilder::new(),
                &resolved.0,
                collect_bindings,
                &mut FamilyEntityAdmission::new(ctx),
            )
            .expect("only a binding reader allocates temporary bindings");
            assert_eq!(bindings.0.len(), usize::from(collect_bindings));
            assert_eq!(ir.model.curves.len(), 1);
            assert_eq!(ir.model.surfaces.len(), 1);
            assert_eq!(ir.model.procedural_surfaces.len(), 1);
        }
    });
}
