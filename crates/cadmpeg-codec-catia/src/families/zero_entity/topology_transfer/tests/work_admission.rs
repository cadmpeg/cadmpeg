// SPDX-License-Identifier: Apache-2.0
//! Work-refusal tests for zero-entity topology transfer scans.

use std::collections::HashMap;

use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::AnnotationBuilder;

use super::super::{transfer_closed_face_topology, ZeroEntityClosedTopology};

#[test]
fn transfer_closed_face_topology_refuses_occurrence_curve_scan() {
    assert_paired_face_lookup_refusal("catia_zero_topology_occurrence_curve_visits", false);
}

fn paired_face_fixture(
    ctx: &DecodeContext<'_>,
    unknown_carrier: bool,
) -> Result<usize, CodecError> {
    let points = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0)];
    let runs = [super::run(10, 1, points, false), super::run(11, 4, points, true)];
    let mut ir = CadIr::empty();
    let surface_id = SurfaceId::mint("catia:test:surface#0").expect("identity grammar");
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                points[0], Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0),
            ).expect("valid plane fixture"),
        )),
        source_object: None,
    });
    let surfaces = HashMap::from([(100, surface_id)]);
    let mut curves = HashMap::new();
    for run in &runs {
        for support in &run.supports {
            let id = CurveId::mint(format!("catia:test:curve#{}", support.record_ordinal))
                .expect("identity grammar");
            curves.insert(support.record_ordinal, id.clone());
            ir.model.curves.push(Curve {
                id,
                geometry: if unknown_carrier && support.record_ordinal == 1 {
                    cadmpeg_ir::geometry::CurveGeometry::Solved(
                        cadmpeg_ir::geometry::SolvedCurveGeometry::Unknown { record: None },
                    )
                } else if support.record_ordinal == 1 {
                    let [start, end] = support.model_endpoints.expect("fixture model endpoints");
                    cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(
                        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                            end.get(), start.get().vector_from(end.get()),
                        ).expect("finite reversed fixture line"),
                    ))
                } else {
                    support.model_curve.clone().expect("fixture model curve")
                },
                source_object: None,
            });
        }
    }
    let counts = transfer_closed_face_topology(
        &mut crate::families::FamilyEntityAdmission::new(ctx),
        &mut ir,
        &mut AnnotationBuilder::new(),
        ZeroEntityClosedTopology {
            support_runs: &runs,
            surface_ids_by_position: &surfaces,
            support_curve_ids: &curves,
            ownership_root: None,
        },
        &WorkBudget::new(100_000),
        &mut crate::nurbs::LaneRefusals::new(),
    )?
    .expect("complete paired-face fixture");
    Ok(counts.faces)
}

fn assert_paired_face_lookup_refusal(operation: &'static str, unknown_carrier: bool) {
    assert_eq!(crate::test_support::with_service_context(|ctx| paired_face_fixture(ctx, unknown_carrier))
        .expect("service paired-face fixture"), 2);
    let result = crate::test_support::with_work_refusal(operation, |ctx| {
        let result = paired_face_fixture(ctx, unknown_carrier);
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal(), Some(*limit));
        }
        result
    });
    assert!(matches!(result,
        Err(CodecError::ResourceLimit(limit)) if limit.operation == operation));
}

#[test]
fn zero_topology_reversed_curve_lookup_preserves_work_refusal() {
    assert_paired_face_lookup_refusal("catia_zero_topology_reversed_curve_lookup", false);
}

#[test]
fn zero_topology_unknown_curve_lookup_preserves_work_refusal() {
    assert_paired_face_lookup_refusal("catia_zero_topology_unknown_curve_lookup", true);
}

#[test]
fn zero_topology_first_radial_lookup_preserves_work_refusal() {
    assert_paired_face_lookup_refusal("catia_zero_topology_first_radial_coedge_lookup", false);
}

#[test]
fn zero_topology_second_radial_lookup_preserves_work_refusal() {
    assert_paired_face_lookup_refusal("catia_zero_topology_second_radial_coedge_lookup", false);
}
