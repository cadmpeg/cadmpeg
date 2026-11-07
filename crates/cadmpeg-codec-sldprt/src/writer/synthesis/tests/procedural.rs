// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn procedural_surface_cache_is_written_despite_construction_disagreement() {
    use cadmpeg_ir::geometry::{
        surface_payloads::AxisRevolutionSurfaceConstruction, ProceduralSurface,
        ProceduralSurfaceDefinition, RecordBounds,
    };
    use cadmpeg_ir::ids::ProceduralSurfaceId;

    let mut ir = crate::test_support::ir::source_less_cube();
    let surface = ir.model.surfaces[0].id.clone();
    let construction = AxisRevolutionSurfaceConstruction::try_new(
        ir.model.curves[0].id.clone(),
        cadmpeg_ir::math::Point3::new(100.0, 100.0, 100.0),
        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
    )
    .unwrap();
    let procedural = ProceduralSurface::new(
        ProceduralSurfaceId::mint("synthetic:test:procedural-surface#revolution").unwrap(),
        ProceduralSurfaceDefinition::AxisRevolution(construction),
        Some(RecordBounds::try_new([Some(0.0), Some(0.0), Some(1.0), Some(1.0)]).unwrap()),
    );
    ir.model
        .add_procedural_surface(&surface, procedural)
        .unwrap();
    let findings = cadmpeg_ir::validate::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(
        findings.findings.iter().any(|finding| finding.check
            == cadmpeg_ir::report::check::Check::GeometricConsistency
            && finding.entity.as_deref() == Some(surface.as_str())),
        "{:?}",
        findings.findings
    );
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::SldprtLossCode::WriterProceduralReduced.note("").code
            && loss.message.contains(surface.as_str())));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.faces.len(), ir.model.faces.len());
    let cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(expected) =
        ir.model.surfaces[0].geometry.solved_cache().unwrap()
    else {
        panic!("cube fixture uses planes");
    };
    assert!(decoded.ir().model.surfaces.iter().any(|output| {
        let cadmpeg_ir::geometry::SurfaceGeometry::Solved(
            cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(actual),
        ) = &output.geometry else {
            return false;
        };
        cadmpeg_ir::math::Point3::distance(actual.origin().get(), expected.origin().get()) < 1e-12
            && (*actual.frame().axis().as_raw() - *expected.frame().axis().as_raw()).norm() < 1e-12
            && (*actual.frame().reference().as_raw() - *expected.frame().reference().as_raw()).norm() < 1e-12
    }));
}
