//! Rejected closed-topology allocations stay owned until their records drop.

use super::*;

fn retained_after_rejected_candidate(commit: bool) -> u64 {
    crate::test_support::with_service_context(|ctx| {
        let points = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ];
        let mut runs = vec![run(10, 1, points, false), run(11, 4, points, true)];
        runs[1]
            .face
            .as_mut()
            .expect("face")
            .loops
            .as_mut()
            .expect("loops")[0]
            .loop_class = ZeroEntityLoopClass::Bound50;
        let curve_ids: HashMap<_, _> = runs
            .iter()
            .flat_map(|run| &run.supports)
            .map(|support| {
                (
                    support.record_ordinal,
                    CurveId::mint(format!("catia:test:curve#{}", support.record_ordinal))
                        .expect("identity"),
                )
            })
            .collect();
        let mut annotations = AnnotationBuilder::new();
        let mut storage = ctx
            .provisional_retained("rejected topology candidate")
            .expect("scope");
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint("catia:test:surface#0").expect("identity"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    points[0],
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("plane"),
            )),
            source_object: None,
        });
        for run in &runs {
            for support in &run.supports {
                ir.model.curves.push(Curve {
                    id: curve_ids[&support.record_ordinal].clone(),
                    geometry: support.model_curve.clone().expect("line"),
                    source_object: None,
                });
            }
        }
        let count = transfer_closed_face_topology(
            &mut crate::families::FamilyEntityAdmission::new(ctx),
            &mut ir,
            &mut annotations,
            ZeroEntityClosedTopology {
                support_runs: &runs,
                surface_indices_by_position: &HashMap::from([(100, 0usize)]),
                support_curve_ids: &curve_ids,
                ownership_root: None,
            },
            &WorkBudget::new(super::super::super::topology::MAX_ZERO_ENTITY_TOPOLOGY_OPERATIONS),
            &mut crate::nurbs::LaneRefusals::new(),
            &mut storage,
        )
        .expect("service budget");
        assert!(count.is_none());
        assert!(
            !ir.model.edges.is_empty(),
            "rejection follows edge emission"
        );
        assert!(
            !ir.model.points.is_empty(),
            "rejection follows point emission"
        );
        assert_eq!(
            ir.model.faces.len(),
            1,
            "the first face precedes rejection of the second"
        );
        if commit {
            storage
                .commit()
                .expect("keep the partial model for comparison");
        } else {
            ir.model.bodies = Vec::new();
            ir.model.regions = Vec::new();
            ir.model.shells = Vec::new();
            ir.model.faces = Vec::new();
            ir.model.loops = Vec::new();
            ir.model.coedges = Vec::new();
            ir.model.edges = Vec::new();
            ir.model.vertices = Vec::new();
            ir.model.points = Vec::new();
            ir.model.pcurves = Vec::new();
            drop(storage);
        }
        assert_eq!(ir.model.surfaces.len(), 1);
        assert!(
            ir.model.curves.len() >= 6,
            "source geometry remains outside the candidate"
        );
        let error = ctx
            .charge_retained(u64::MAX, "retained candidate measurement")
            .expect_err("measurement exceeds the limit");
        let CodecError::ResourceLimit(limit) = error else {
            panic!("retained limit");
        };
        limit.used
    })
}

#[test]
fn rejected_closed_topology_releases_emitted_records_after_drop() {
    let released = retained_after_rejected_candidate(false);
    let kept = retained_after_rejected_candidate(true);
    assert!(
        released < kept,
        "rejected topology refunds retained storage"
    );
}
