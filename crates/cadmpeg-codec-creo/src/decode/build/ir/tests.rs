// SPDX-License-Identifier: Apache-2.0

use super::{transfer_display_tessellations, transfer_placed_plane_surfaces_into_ir};
use crate::container::{scan_bytes_ok, ContainerScan};
use crate::legacy::PrincipalUnitSystem;
use crate::primdata::PrimitiveTriangleStrip;
use crate::scalar::PlaneSupportFrameLayout;
use crate::surface::{LocalSystemClassification, PlaneLocalSystem};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::units::FiniteVector;

fn inch_strip(positions: Vec<[f64; 3]>) -> ContainerScan<'static> {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("strip", &[]));
    scan.framing.principal_unit = Some(PrincipalUnitSystem::InchPoundMassSecond);
    scan.primitives
        .triangle_strips
        .push(PrimitiveTriangleStrip {
            offset: 0,
            positions: positions
                .into_iter()
                .map(|position| FiniteVector::new(position).expect("finite strip position"))
                .collect(),
            normals: None,
            strip_lengths: vec![3],
        });
    scan
}

#[test]
fn display_tessellation_vertices_are_in_millimeters_at_ir_admission() {
    let scan = inch_strip(vec![[1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 4.0]]);
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        transfer_display_tessellations(ctx, &scan, &mut ir, &mut annotations)
            .expect("display tessellation transfer");
    });
    assert_eq!(
        ir.model.tessellations[0].vertices()[0].get(),
        Point3::new(25.4, 0.0, 0.0)
    );
    assert_eq!(
        ir.model.tessellations[0].vertices()[1].get(),
        Point3::new(0.0, 50.8, 0.0)
    );
    assert_eq!(
        ir.model.tessellations[0].vertices()[2].get(),
        Point3::new(0.0, 0.0, 101.6)
    );
    assert_eq!(
        scan.primitives.triangle_strips[0].positions[0],
        [1.0, 0.0, 0.0]
    );
    super::super::units::normalize_model_lengths(
        &mut ir,
        PositiveReal::new(25.4).expect("unit scale"),
    )
    .expect("remaining unit normalization");
    assert_eq!(
        ir.model.tessellations[0].vertices()[0].get(),
        Point3::new(25.4, 0.0, 0.0)
    );
}

#[test]
fn display_tessellation_vertex_overflow_refuses_unrepresentable_ir() {
    let scan = inch_strip(vec![[f64::MAX, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 4.0]]);
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = transfer_display_tessellations(ctx, &scan, &mut ir, &mut annotations)
            .expect_err("scaled display vertex overflows");
        assert!(matches!(error, CodecError::NotImplemented(_)));
        assert!(ir.model.tessellations.is_empty());
    });
}

#[test]
fn positional_plane_cross_overflow_refuses_at_ir_transfer() {
    let a = f64::from_bits(0x5fed_817d_bb14_96d1);
    let b = f64::from_bits(0x5fd8_c57e_64a4_a42f);
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("plane", &[]));
    scan.planes.local_systems.push(PlaneLocalSystem {
        surface_id: 17,
        body: Vec::new(),
        slots: [a, b, 0.0, -b, a, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0].map(Some),
        layout: Some(PlaneSupportFrameLayout::SupportTriples),
        classification: LocalSystemClassification::Simple,
        row_offset: 0,
        offset: 0,
    });
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = transfer_placed_plane_surfaces_into_ir(ctx, &scan, &mut ir, &mut annotations)
            .expect_err("finite plane support cross overflows the frame");
        assert!(matches!(error, CodecError::NotImplemented(_)));
        assert!(ir.model.surfaces.is_empty());
    });
}

#[test]
fn positional_plane_missing_slots_remain_unplaced() {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("plane", &[]));
    let mut slots = [Some(0.0); 12];
    slots[0] = None;
    scan.planes.local_systems.push(PlaneLocalSystem {
        surface_id: 17,
        body: Vec::new(),
        slots,
        layout: Some(PlaneSupportFrameLayout::SupportTriples),
        classification: LocalSystemClassification::Simple,
        row_offset: 0,
        offset: 0,
    });
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        transfer_placed_plane_surfaces_into_ir(ctx, &scan, &mut ir, &mut annotations)
            .expect("a missing support slot does not define a frame");
        assert!(ir.model.surfaces.is_empty());
    });
}
