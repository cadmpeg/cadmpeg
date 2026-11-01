// SPDX-License-Identifier: Apache-2.0
//! Surface cache checks evaluate the construction independently of its cache.

use crate::eval::admission::ModelEvaluationPass;
use crate::features::{
    DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation,
    FeatureOperation,
};
use crate::geometry::{
    analytic::{LineCurve, PlaneSurface},
    surface_payloads::LinearSweepSurfaceConstruction,
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, RecordBounds,
    SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use crate::math::{Point3, Vector3};
use crate::report::Severity;
use crate::CadIr;

fn swept_plane(height: f64) -> CadIr {
    let mut ir = crate::examples::unit_cube().unwrap();
    let directrix = "test:model:curve#surface-cache-directrix"
        .try_into()
        .unwrap();
    ir.model.curves.push(Curve {
        native_ref: None,
        id: directrix,
        parameter_range: None,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)).unwrap(),
        )),
        source_object: None,
    });
    let construction = ProceduralSurface::new(
        "test:model:procedural_surface#cache-check"
            .try_into()
            .unwrap(),
        ProceduralSurfaceDefinition::LinearSweep(
            LinearSweepSurfaceConstruction::try_new(
                ir.model.curves.last().unwrap().id.clone(),
                Vector3::new(0.0, 1.0, 0.0),
            )
            .unwrap(),
        ),
        Some(RecordBounds::try_new([Some(0.0), Some(0.0), Some(1.0), Some(1.0)]).unwrap()),
    );
    ir.model.surfaces[0].geometry = SurfaceGeometry::Procedural {
        construction: construction.id.clone(),
        cache: Some(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, height),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
    };
    ir.model.procedural_surfaces.push(construction);
    ir
}

fn findings(ir: &CadIr) -> Vec<crate::report::check::Finding> {
    let mut findings = Vec::new();
    super::check(
        &cadmpeg_test_support::service_decode_context(),
        &crate::index::ModelIndex::build(ir),
        &ModelEvaluationPass::new(),
        &mut findings,
    )
    .unwrap();
    findings
}

#[test]
fn matching_surface_cache_has_no_disagreement() {
    assert!(findings(&swept_plane(0.0)).is_empty());
}

#[test]
fn cache_is_not_used_as_its_own_construction_witness() {
    let findings = findings(&swept_plane(1.0));
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].severity, Severity::Error);
    assert!(findings[0]
        .message
        .contains("surface solved cache disagrees"));
}

#[test]
fn stale_producing_feature_permits_its_surface_cache_to_disagree() {
    let mut ir = swept_plane(1.0);
    ir.model.features.push(Feature {
        product_definition: None,
        id: "test:model:feature#stale-sweep".try_into().unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        needs_recompute: true,
        dependencies: DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
            DistinctMembers::try_from(vec![ir.model.bodies[0].id.clone()]).unwrap(),
        ),
        native_ref: None,
    });
    assert!(findings(&ir).is_empty());
    ir.model.features[0].needs_recompute = false;
    assert_eq!(findings(&ir).len(), 1);
}
