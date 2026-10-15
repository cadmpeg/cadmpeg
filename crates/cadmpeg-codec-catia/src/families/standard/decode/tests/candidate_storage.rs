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
    build_standard_edge_curve, ensure_native_edge_support_surface, BuildStandardEdgeCurveInputs,
    NativeSurfaceIndex,
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

#[derive(Clone, Copy)]
enum IntersectionCase {
    PlanarIntersection,
    SameSurface,
    MissingSurface,
    UnboundedCircle,
    NativeUnboundedCircle,
}

fn retained_after_intersection_edges(case: IntersectionCase, suffix_len: usize) -> u64 {
    use cadmpeg_ir::geometry::analytic::{CylinderSurface, PlaneSurface, SphereSurface};
    use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
    use cadmpeg_ir::geometry::SolvedCurveGeometry;
    use cadmpeg_ir::math::Point2;

    let first_id = SurfaceId::mint(format!(
        "catia:test:surface#first{}",
        "x".repeat(suffix_len)
    ))
    .expect("identity");
    let second_id = SurfaceId::mint(format!(
        "catia:test:surface#second{}",
        "x".repeat(suffix_len)
    ))
    .expect("identity");
    let circle = matches!(
        case,
        IntersectionCase::UnboundedCircle | IntersectionCase::NativeUnboundedCircle
    );
    let planar = matches!(case, IntersectionCase::PlanarIntersection);
    let first_geometry = if circle {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
            SphereSurface::try_new(
                Point3::new(1.0, 2.0, 3.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .expect("sphere"),
        ))
    } else if planar {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("first plane"),
        ))
    } else {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .expect("cylinder"),
        ))
    };
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: first_id.clone(),
        geometry: first_geometry.clone(),
        source_object: None,
    });
    let mut bindings = vec![(first_id.clone(), false, 0)];
    let mut surface_indices = HashMap::from([(first_id, 0)]);
    let native = if circle || planar {
        let second_geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                if circle {
                    Point3::new(1.0, 3.0, 3.0)
                } else {
                    Point3::new(0.0, 0.0, 0.0)
                },
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("section plane"),
        ));
        ir.model.surfaces.push(Surface {
            id: second_id.clone(),
            geometry: second_geometry.clone(),
            source_object: None,
        });
        bindings.push((second_id.clone(), false, 1));
        surface_indices.insert(second_id, 1);
        if matches!(case, IntersectionCase::NativeUnboundedCircle) {
            let pcurve = PcurveGeometry::Nurbs {
                nurbs: PcurveNurbs::from_lanes(
                    &cadmpeg_test_support::service_decode_context(),
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                    None,
                    false,
                )
                .expect("pcurve admission")
                .expect("linear pcurve"),
            };
            Some(crate::families::standard::decode::StandardEdgeSupport {
                surface_object_ids: [20, 21],
                carriers: [
                    crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(first_geometry),
                    crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(second_geometry),
                ],
                pcurves: [pcurve.clone(), pcurve],
                parameter_range: [0.0, 1.0],
            })
        } else {
            None
        }
    } else {
        None
    };
    let endpoints = if circle {
        let radius = 3.0_f64.sqrt();
        [
            Point3::new(1.0 + radius, 3.0, 3.0),
            Point3::new(1.0 - radius, 3.0, 3.0),
        ]
    } else if planar {
        [Point3::new(1.0, 0.0, 0.0), Point3::new(4.0, 0.0, 0.0)]
    } else {
        [Point3::new(2.0, 0.0, -3.0), Point3::new(2.0, 0.0, 4.0)]
    };
    for (index, position) in endpoints.into_iter().enumerate() {
        ir.model.points.push(Point::new(
            PointId::mint(format!("catia:test:point#{index}")).expect("identity"),
            cadmpeg_ir::features::FinitePoint3::new(position).expect("point"),
            None,
        ));
    }
    crate::test_support::with_service_context(|ctx| {
        let mut surviving = ctx
            .provisional_retained("surviving source curves")
            .expect("owner");
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        let mut native_surfaces = NativeSurfaceIndex::new(ctx).expect("index");
        let mut annotations = AnnotationBuilder::new();
        for pos in 0..16 {
            let support = StandardCurveSupport {
                pos,
                tag: 7,
                faces: if matches!(case, IntersectionCase::SameSurface) {
                    [0, 0]
                } else {
                    [0, 1]
                },
                geometry: StandardCurveGeometry::Bspline,
            };
            let (id, range) = surviving
                .with_storage(|| {
                    build_standard_edge_curve(
                        ctx,
                        BuildStandardEdgeCurveInputs {
                            ir: &mut ir,
                            annotations: &mut annotations,
                            bindings: &bindings,
                            surface_indices: &surface_indices,
                            brep: &[],
                            support: &support,
                            points: [0, 1],
                            native_support: if matches!(
                                case,
                                IntersectionCase::NativeUnboundedCircle
                            ) {
                                native.as_ref()
                            } else {
                                None
                            },
                            limit_curve: None,
                            refusal: &mut crate::nurbs::LaneRefusals::new(),
                            native_surfaces: &mut native_surfaces,
                            admission: &mut admission,
                            support_storage: None,
                        },
                    )
                })
                .expect("source curve");
            assert_eq!(id.as_ref(), Some(&ir.model.curves[pos].id));
            match case {
                IntersectionCase::PlanarIntersection => {
                    assert_eq!(range, Some([0.0, 3.0]));
                    assert!(matches!(
                        ir.model.curves[pos].geometry.solved_cache(),
                        Some(SolvedCurveGeometry::Line(_))
                    ));
                    assert_eq!(ir.model.procedural_curves.len(), pos + 1);
                }
                IntersectionCase::SameSurface => {
                    assert_eq!(range, Some([0.0, 7.0]));
                    assert!(matches!(
                        ir.model.curves[pos].geometry.solved(),
                        Some(SolvedCurveGeometry::Line(_))
                    ));
                }
                IntersectionCase::MissingSurface => {
                    assert_eq!(range, None);
                    assert!(matches!(
                        ir.model.curves[pos].geometry.solved(),
                        Some(SolvedCurveGeometry::Unknown { .. })
                    ));
                }
                IntersectionCase::UnboundedCircle | IntersectionCase::NativeUnboundedCircle => {
                    assert_eq!(range, None);
                    assert!(matches!(
                        ir.model.curves[pos].geometry.solved(),
                        Some(SolvedCurveGeometry::Circle(_))
                    ));
                }
            }
            if !planar {
                assert!(ir.model.procedural_curves.is_empty());
            }
        }
        assert_eq!(ir.model.curves.len(), 16);
        assert_eq!(
            ir.model.surfaces.len(),
            if circle || planar { 2 } else { 1 }
        );
        surviving.commit().expect("keep source curves");
        drop(native_surfaces);
        let CodecError::ResourceLimit(limit) = ctx
            .charge_retained(u64::MAX, "retained measurement")
            .expect_err("measurement")
        else {
            panic!("retained refusal");
        };
        limit.used
    })
}

#[test]
fn emitted_intersection_retains_both_side_ids() {
    let short = retained_after_intersection_edges(IntersectionCase::PlanarIntersection, 0);
    let long = retained_after_intersection_edges(IntersectionCase::PlanarIntersection, 4096);
    assert_eq!(long.checked_sub(short), Some(16 * 2 * 4096));
}

#[test]
fn same_surface_intersection_does_not_retain_side_ids() {
    assert_eq!(
        retained_after_intersection_edges(IntersectionCase::SameSurface, 0),
        retained_after_intersection_edges(IntersectionCase::SameSurface, 4096)
    );
}

#[test]
fn missing_surface_intersection_does_not_retain_side_ids() {
    assert_eq!(
        retained_after_intersection_edges(IntersectionCase::MissingSurface, 0),
        retained_after_intersection_edges(IntersectionCase::MissingSurface, 4096)
    );
}

#[test]
fn unbounded_intersection_does_not_retain_side_ids() {
    assert_eq!(
        retained_after_intersection_edges(IntersectionCase::UnboundedCircle, 0),
        retained_after_intersection_edges(IntersectionCase::UnboundedCircle, 4096)
    );
}

#[test]
fn unbounded_native_intersection_does_not_retain_side_ids() {
    assert_eq!(
        retained_after_intersection_edges(IntersectionCase::NativeUnboundedCircle, 0),
        retained_after_intersection_edges(IntersectionCase::UnboundedCircle, 0),
        "discarded native pcurve backing is released",
    );
    assert_eq!(
        retained_after_intersection_edges(IntersectionCase::NativeUnboundedCircle, 0),
        retained_after_intersection_edges(IntersectionCase::NativeUnboundedCircle, 4096)
    );
}
