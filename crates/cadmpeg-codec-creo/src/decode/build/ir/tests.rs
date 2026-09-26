// SPDX-License-Identifier: Apache-2.0

use super::transfer_display_tessellations;
use crate::container::{scan_bytes_ok, ContainerScan};
use crate::legacy::PrincipalUnitSystem;
use crate::primdata::PrimitiveTriangleStrip;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::PositiveReal;

fn inch_strip(positions: Vec<[f64; 3]>) -> ContainerScan<'static> {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("strip", &[]));
    scan.framing.principal_unit = Some(PrincipalUnitSystem::InchPoundMassSecond);
    scan.primitives
        .triangle_strips
        .push(PrimitiveTriangleStrip {
            offset: 0,
            positions,
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
