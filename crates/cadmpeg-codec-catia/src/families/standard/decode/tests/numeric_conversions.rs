use crate::families::standard::decode::{
    attach_standard_topology, AttachStandardTopologyInputs, EdgeTableForm, FamilyEntityAdmission,
    StandardTopologyDiagnostics, StandardTopologyError, StandardTopologyFailure,
};
use crate::families::standard::records::{StandardCurveGeometry, StandardCurveSupport};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::AnnotationBuilder;
use std::collections::HashMap;

fn visualization_refuses_coordinate(coordinate: f64, axis: usize) -> bool {
    crate::test_support::with_service_context(|ctx| {
        let spine = crate::test_support::test_topology::standard_quad_topology_stream();
        let mut ir = CadIr::empty();
        let mut coordinates = [0.0; 3];
        coordinates[axis] = coordinate;
        ir.model.points.push(Point::new(
            PointId::mint("catia:test:point#numeric-range").expect("identity grammar"),
            FinitePoint3::new(Point3::new(coordinates[0], coordinates[1], coordinates[2]))
                .expect("finite point"),
            None,
        ));
        let support = StandardCurveSupport {
            pos: 0,
            tag: 1,
            faces: [0, 0],
            geometry: StandardCurveGeometry::Line,
        };
        let supports = vec![support; 4];
        let mut annotations = AnnotationBuilder::new();
        let work_budget = ctx.work_budget(1_000_000);
        let mut diagnostics = StandardTopologyDiagnostics::default();
        let mut bound_limit_curve_count = 0;
        let mut refusal = crate::nurbs::LaneRefusals::new();
        let mut admission = FamilyEntityAdmission::new(ctx);
        let result = attach_standard_topology(
            ctx,
            AttachStandardTopologyInputs {
                ir: &mut ir,
                annotations: &mut annotations,
                bindings: &[],
                records: &[],
                face_bounds: &[],
                spine: &spine,
                edge_table_form: EdgeTableForm::Standard,
                brep: &[],
                support_override: Some(&supports),
                source: &[],
                e5_record_range: None,
                use_vertex_roster: true,
                native_edge_faces: &HashMap::new(),
                native_edge_supports: &HashMap::new(),
                limit_curves: &[],
                work_budget: &work_budget,
                diagnostics: &mut diagnostics,
                bound_limit_curve_count: &mut bound_limit_curve_count,
                refusal: &mut refusal,
                admission: &mut admission,
            },
        );
        matches!(
            result,
            Err(StandardTopologyError::Semantic(
                StandardTopologyFailure::ConflictingNativeEndpoints
            ))
        )
    })
}

#[test]
fn standard_visualization_refuses_positive_coordinate_overflow() {
    for axis in 0..3 {
        assert!(visualization_refuses_coordinate(f64::MAX, axis));
    }
}

#[test]
fn standard_visualization_refuses_negative_coordinate_overflow() {
    for axis in 0..3 {
        assert!(visualization_refuses_coordinate(-f64::MAX, axis));
    }
}
