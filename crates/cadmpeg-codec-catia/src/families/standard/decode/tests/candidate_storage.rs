//! Candidate topology storage and surviving source geometry.

use std::collections::HashMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{FaceId, PointId, ShellId, SurfaceId, UnknownId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Face, Point, Sense};
use cadmpeg_ir::AnnotationBuilder;

use super::super::{emit_standard_topology, EmitStandardTopologyInputs, StandardTopologyStorage};
use crate::families::standard::decode::edge_geometry::{
    ensure_native_edge_support_surface, NativeSurfaceIndex,
};
use crate::families::standard::records::{StandardCurveGeometry, StandardCurveSupport};
use crate::families::standard::topology::{
    BoundaryDraft, CoedgeUse, EdgeBoundaryLayout, EdgeRow, FaceTopologyDraft, StandardTopologyDraft,
};

fn retained_after_rejected_standard_candidate(keep: bool) -> u64 {
    crate::test_support::with_service_context(|ctx| {
        let topology = StandardTopologyDraft {
            faces: vec![FaceTopologyDraft {
                boundaries: vec![BoundaryDraft::new(vec![
                    CoedgeUse {
                        edge_row: 0,
                        reversed: false,
                        start_vertex: 0,
                        end_vertex: 1,
                    },
                    CoedgeUse {
                        edge_row: 0,
                        reversed: true,
                        start_vertex: 1,
                        end_vertex: 0,
                    },
                ])
                .expect("nonempty boundary")],
            }],
            edge_rows: vec![
                EdgeRow::new(1, vec![0, 1], EdgeBoundaryLayout::CompleteBoundaryRun).expect("row"),
            ],
            vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            logical_vertex_count: 2,
        };
        let topology = crate::families::standard::topology::admitted::StandardTopology::new(
            &cadmpeg_test_support::service_decode_context(),
            topology,
        )
        .expect("admission")
        .expect("closed boundary");
        let mut candidate = ctx
            .provisional_retained("standard topology candidate test")
            .expect("scope");
        let mut storage = StandardTopologyStorage {
            surviving: ctx
                .provisional_retained("standard source geometry test")
                .expect("scope"),
            supports: ctx
                .provisional_retained("standard support payloads test")
                .expect("scope"),
        };
        let mut ir = CadIr::empty();
        let source_surface = SurfaceId::mint("catia:test:surface#0").expect("identity");
        let plane = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("plane"),
        ));
        ir.model.surfaces.push(Surface {
            id: source_surface.clone(),
            geometry: plane.clone(),
            source_object: None,
        });
        for (index, x) in [0.0, 1.0].into_iter().enumerate() {
            ir.model.points.push(Point::new(
                PointId::mint(format!("catia:test:point#{index}")).expect("identity"),
                crate::test_support::test_b5::point([x, 0.0, 0.0]),
                None,
            ));
        }
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        let annotations = AnnotationBuilder::new();
        let mut annotations = annotations
            .copy_transaction(ctx, "standard candidate annotations")
            .expect("transaction");
        let mut index = NativeSurfaceIndex::new(ctx).expect("index");
        for ordinal in 10..12 {
            let carrier = crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                    record: Some(
                        UnknownId::mint(format!(
                            "catia:test:unknown#{ordinal}{}",
                            "x".repeat(4096)
                        ))
                        .expect("identity"),
                    ),
                }),
            );
            storage
                .surviving
                .with_storage(|| {
                    ensure_native_edge_support_surface(
                        &mut ir,
                        &mut annotations,
                        ordinal,
                        &carrier,
                        &mut index,
                        &mut admission,
                        Some(&mut storage.supports),
                    )
                })
                .expect("support payload");
        }
        candidate
            .with_storage(|| {
                ctx.push_vec(
                    &mut ir.model.faces,
                    Face {
                        id: FaceId::mint("catia:standard:face#0").expect("identity"),
                        shell: ShellId::mint("catia:test:shell#0").expect("identity"),
                        surface: source_surface.clone(),
                        sense: Sense::Forward,
                        loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
                        name: None,
                        color: None,
                        tolerance: None,
                    },
                    "candidate face",
                )?;
                emit_standard_topology(
                    ctx,
                    EmitStandardTopologyInputs {
                        ir: &mut ir,
                        annotations: &mut annotations,
                        bindings: &[(source_surface.clone(), false, 0)],
                        brep: &[],
                        surface_indices: &HashMap::from([(source_surface.clone(), 0)]),
                        supports: &[StandardCurveSupport {
                            pos: 0,
                            tag: 1,
                            faces: [0, 0],
                            geometry: StandardCurveGeometry::Line,
                        }],
                        edge_vertices: &[[0, 1]],
                        point_assignment: &[0, 1],
                        topology: &topology,
                        native_edge_supports: &[None],
                        limit_curve_bindings: &[None],
                        limit_curves: &[],
                        refusal: &mut crate::nurbs::LaneRefusals::new(),
                        admission: &mut admission,
                        storage: Some(&mut storage),
                    },
                )
            })
            .expect("partial topology emission");
        assert_eq!(ir.model.faces.len(), 1);
        assert_eq!(ir.model.edges.len(), 1);
        assert_eq!(ir.model.loops.len(), 1);
        assert_eq!(ir.model.coedges.len(), 2);
        assert_eq!(ir.model.pcurves.len(), 2);
        assert_eq!(ir.model.curves.len(), 1);
        assert_eq!(ir.model.surfaces.len(), 3);
        assert!(
            !crate::assemble::neutral_model_is_admissible(ctx, &mut ir, &[])
                .expect("neutral admission")
        );
        storage
            .surviving
            .commit()
            .expect("source geometry survives rejection");
        drop(index);
        drop(annotations);
        if keep {
            candidate.commit().expect("keep topology for comparison");
            storage
                .supports
                .commit()
                .expect("keep supports for comparison");
        } else {
            ir.model.bodies = Vec::new();
            ir.model.regions = Vec::new();
            ir.model.shells = Vec::new();
            ir.model.faces = Vec::new();
            ir.model.loops = Vec::new();
            ir.model.coedges = Vec::new();
            ir.model.edges = Vec::new();
            ir.model.pcurves = Vec::new();
            ir.model
                .surfaces
                .retain(|surface| surface.id == source_surface);
            drop(candidate);
            drop(storage.supports);
            assert_eq!(ir.model.edges.capacity(), 0);
            assert_eq!(ir.model.faces.capacity(), 0);
            assert_eq!(ir.model.pcurves.capacity(), 0);
        }
        assert_eq!(ir.model.points.len(), 2);
        assert_eq!(
            ir.model
                .surfaces
                .iter()
                .find(|surface| surface.id == source_surface)
                .expect("source surface survives")
                .geometry,
            plane
        );
        assert_eq!(ir.model.curves.len(), 1);
        let CodecError::ResourceLimit(limit) = ctx
            .charge_retained(u64::MAX, "standard retained measurement")
            .expect_err("measurement exceeds limit")
        else {
            panic!("retained refusal")
        };
        limit.used
    })
}

#[test]
fn rejected_standard_topology_releases_payloads_and_keeps_source_curve_storage() {
    let released = retained_after_rejected_standard_candidate(false);
    let kept = retained_after_rejected_standard_candidate(true);
    assert!(
        released > 0,
        "source curve and source arena backing remain retained"
    );
    assert!(
        kept > released + 8192,
        "topology and both owned support IDs are released"
    );
}
