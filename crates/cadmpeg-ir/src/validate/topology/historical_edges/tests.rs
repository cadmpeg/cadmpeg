// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::features::evaluation::{HistoricalEdgeTopologyRef, HistoricalFaceTopologyRef};
use crate::features::selections::{DistinctMembers, NonEmptyMembers};
use crate::geometry::analytic::{CircleCurve, EllipseCurve};
use crate::ids::{FeatureInputTopologyId, HistoricalEdgeId, HistoricalFaceId};
use crate::math::Vector3;
use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

fn fixture() -> CadIr {
    let mut ir = crate::examples::unit_cube_with_history_body().unwrap();
    crate::examples::add_document_root(&mut ir);
    let body = &mut ir.model.history_bodies[0];
    body.consumed_by = body.produced_by.clone();
    let nest = &body.topology;
    let mut geometry = HistoricalEdgeGeometry::new(
        HistoricalEdgeId::mint("edge-0").unwrap(),
        crate::features::geometry::FinitePoint3::new(Point3::new(5.0, 0.0, 0.0)).unwrap(),
    );
    geometry.start = Some(crate::features::geometry::FinitePoint3::ZERO);
    geometry.end = crate::features::geometry::FinitePoint3::new(Point3::new(10.0, 0.0, 0.0));
    let faces = ["bottom", "front"]
        .into_iter()
        .map(|name| {
            let face = HistoricalFaceId::mint(name).unwrap();
            let target = nest
                .faces
                .iter()
                .find(|face| face.id.as_str().ends_with(&format!("#{name}")))
                .unwrap()
                .id
                .clone();
            HistoricalFaceTopologyRef {
                face,
                topology_faces: NonEmptyMembers::try_from(vec![target]).unwrap(),
            }
        })
        .collect::<Vec<_>>();
    geometry.faces = DistinctMembers::try_from(
        faces
            .iter()
            .map(|face| face.face.clone())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    geometry.topology = Some(HistoricalEdgeTopologyRef {
        history_body: body.id.clone(),
        path: HistoricalEdgePath::Edge {
            edge: nest.edges[0].id.clone(),
        },
        faces,
    });
    ir.model
        .feature_input_topologies
        .push(FeatureInputTopology {
            face_owners: Vec::new(),
            face_geometry: Vec::new(),
            body_writers: Vec::new(),
            id: FeatureInputTopologyId::mint("test:input:state#previous").unwrap(),
            input_of: body.consumed_by.clone().unwrap(),
            bodies: DistinctMembers::default(),
            faces: geometry.faces.clone(),
            edges: DistinctMembers::try_from(vec![geometry.edge.clone()]).unwrap(),
            vertices: DistinctMembers::default(),
            edge_geometry: vec![geometry],
            native_ref: None,
        });
    ir
}

fn findings(ir: &CadIr) -> Vec<Finding> {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let state = &ir.model.feature_input_topologies[0];
    let mut findings = Vec::new();
    check(&ctx, ir, state, &state.edge_geometry[0], &mut findings).unwrap();
    findings
}

fn planar_supports(ir: &mut CadIr) {
    let nest = &mut ir.model.history_bodies[0].topology;
    let plane = nest
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str().ends_with("#bottom"))
        .unwrap()
        .geometry
        .clone();
    for surface in &mut nest.surfaces {
        surface.geometry = plane.clone();
    }
}

#[test]
fn input_topology_round_trip_and_typed_rewrite_preserve_local_members() {
    let ir = fixture();
    assert!(findings(&ir).is_empty());
    let mut targets = Vec::new();
    ir.model.feature_input_topologies[0]
        .visit_identity_references(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &mut |target| {
                targets.push(target.to_owned());
                Ok(())
            },
        )
        .unwrap();
    let reference = ir.model.feature_input_topologies[0].edge_geometry[0]
        .topology
        .as_ref()
        .unwrap();
    assert!(targets.contains(&reference.history_body.as_str().to_owned()));
    let HistoricalEdgePath::Edge { edge } = &reference.path else {
        panic!("edge path")
    };
    assert!(targets.contains(&edge.as_str().to_owned()));
    assert!(targets.contains(&reference.faces[0].topology_faces[0].as_str().to_owned()));
    assert!(!targets.contains(
        &ir.model.feature_input_topologies[0].edge_geometry[0]
            .edge
            .as_str()
            .to_owned()
    ));
    let wire = serde_json::to_vec(&ir.model.feature_input_topologies).unwrap();
    let decoded: Vec<FeatureInputTopology> = serde_json::from_slice(&wire).unwrap();
    assert_eq!(decoded, ir.model.feature_input_topologies);
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut map = IdentityMap::new(|id: &str| Ok(id.replace('#', "#renamed-")));
    let mut rewritten = ir.clone();
    rewritten.model.features = std::mem::take(&mut rewritten.model.features)
        .rewrite_identities(&ctx, &mut map)
        .unwrap();
    rewritten.model.feature_input_topologies =
        std::mem::take(&mut rewritten.model.feature_input_topologies)
            .rewrite_identities(&ctx, &mut map)
            .unwrap();
    rewritten.model.history_bodies = std::mem::take(&mut rewritten.model.history_bodies)
        .rewrite_identities(&ctx, &mut map)
        .unwrap();
    map.finish().unwrap();
    let before = &ir.model.feature_input_topologies[0].edge_geometry[0];
    let after = &rewritten.model.feature_input_topologies[0].edge_geometry[0];
    assert_eq!(before.edge, after.edge);
    assert_eq!(before.faces, after.faces);
    assert_ne!(before.topology, after.topology);
    assert!(findings(&rewritten).is_empty());
    let report =
        crate::validate::validate_neutral_for_decode(&ctx, &rewritten, Vec::new()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .all(|finding| finding.severity != crate::report::Severity::Error),
        "{:?}",
        report.findings
    );
}

#[test]
fn trimmed_ellipse_retains_exact_carrier_and_interval() {
    let mut ir = fixture();
    planar_supports(&mut ir);
    let nest = &mut ir.model.history_bodies[0].topology;
    nest.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
        EllipseCurve::try_new(
            Point3::new(5.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            5.0,
            2.0,
        )
        .unwrap(),
    ));
    nest.edges[0].carrier = EdgeCarrier::new(
        Some(nest.curves[0].id.clone()),
        Some([std::f64::consts::PI, std::f64::consts::TAU]),
    )
    .unwrap();
    ir.model.feature_input_topologies[0].edge_geometry[0].interior =
        crate::features::geometry::FinitePoint3::new(Point3::new(5.0, -2.0, 0.0)).unwrap();
    ir.model.feature_input_topologies[0].edge_geometry[0].tangent =
        Some(crate::units::UnitVector3::X_AXIS);
    assert!(findings(&ir).is_empty());
    assert_eq!(ir.model.history_bodies[0].state.identifier, 1);
    assert_eq!(
        ir.model.history_bodies[0].consumed_by.as_ref(),
        Some(&ir.model.feature_input_topologies[0].input_of)
    );
    assert_eq!(
        nest_interval(&ir),
        [std::f64::consts::PI, std::f64::consts::TAU]
    );
    let CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse)) =
        &ir.model.history_bodies[0].topology.curves[0].geometry
    else {
        panic!("ellipse")
    };
    assert_eq!(ellipse.major_radius().get(), 5.0);
    assert_eq!(ellipse.minor_radius().get(), 2.0);
}

fn nest_interval(ir: &CadIr) -> [f64; 2] {
    ir.model.history_bodies[0].topology.edges[0]
        .carrier
        .param_range()
        .unwrap()
        .get()
}

#[test]
fn closed_spline_retains_its_domain_and_seam_multiplicity() {
    use crate::geometry::nurbs::{NurbsCurve, NurbsPoles3};
    let mut ir = fixture();
    planar_supports(&mut ir);
    let nest = &mut ir.model.history_bodies[0].topology;
    let poles = vec![
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(-2.0, 0.0, 0.0),
        Point3::new(0.0, -1.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
    ];
    let spline = NurbsCurve::new(
        1,
        vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0],
        NurbsPoles3::from_lanes(poles, None).unwrap(),
        false,
    )
    .unwrap();
    nest.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(spline));
    nest.edges[0].carrier =
        EdgeCarrier::new(Some(nest.curves[0].id.clone()), Some([0.0, 4.0])).unwrap();
    nest.edges[0].ends = None;
    let geometry = &mut ir.model.feature_input_topologies[0].edge_geometry[0];
    geometry.interior =
        crate::features::geometry::FinitePoint3::new(Point3::new(-2.0, 0.0, 0.0)).unwrap();
    geometry.start = None;
    geometry.end = None;
    assert!(findings(&ir).is_empty());
    assert_eq!(nest_interval(&ir), [0.0, 4.0]);
    let nest = &mut ir.model.history_bodies[0].topology;
    let bottom = nest
        .faces
        .iter()
        .find(|face| face.id.as_str().ends_with("#bottom"))
        .unwrap()
        .id
        .clone();
    for coedge in nest
        .coedges
        .iter()
        .filter(|coedge| coedge.edge == nest.edges[0].id)
    {
        nest.loops
            .iter_mut()
            .find(|ring| ring.id == coedge.owner_loop)
            .unwrap()
            .face = bottom.clone();
    }
    let geometry = &mut ir.model.feature_input_topologies[0].edge_geometry[0];
    geometry.faces =
        DistinctMembers::try_from(vec![HistoricalFaceId::mint("bottom").unwrap()]).unwrap();
    geometry.topology.as_mut().unwrap().faces.truncate(1);
    assert!(findings(&ir).is_empty());
    let nest = &ir.model.history_bodies[0].topology;
    assert_eq!(
        nest.coedges
            .iter()
            .filter(|coedge| coedge.edge == nest.edges[0].id)
            .count(),
        2
    );
}

#[test]
fn line_arc_loop_keeps_source_coedge_order_and_direction() {
    let mut ir = fixture();
    planar_supports(&mut ir);
    let nest = &mut ir.model.history_bodies[0].topology;
    nest.curves[1].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        CircleCurve::try_new(
            Point3::new(10.0, 5.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, -1.0, 0.0),
            5.0,
        )
        .unwrap(),
    ));
    nest.edges[1].carrier = EdgeCarrier::new(
        Some(nest.curves[1].id.clone()),
        Some([0.0, std::f64::consts::PI]),
    )
    .unwrap();
    let ring = nest
        .loops
        .iter()
        .find(|ring| ring.face.as_str().ends_with("#bottom"))
        .unwrap();
    let order = ring.coedges().to_vec();
    let loop_id = ring.id.clone();
    let selected = ring
        .coedges()
        .iter()
        .map(|id| {
            nest.coedges
                .iter()
                .find(|coedge| coedge.id == *id)
                .unwrap()
                .edge
                .clone()
        })
        .collect::<HashSet<_>>();
    let mut targets = HashSet::new();
    for coedge in &nest.coedges {
        if selected.contains(&coedge.edge) {
            targets.insert(
                nest.loops
                    .iter()
                    .find(|ring| ring.id == coedge.owner_loop)
                    .unwrap()
                    .face
                    .clone(),
            );
        }
    }
    let geometry = &mut ir.model.feature_input_topologies[0].edge_geometry[0];
    geometry.tangent = Some(crate::units::UnitVector3::new(Vector3::new(-1.0, 0.0, 0.0)).unwrap());
    geometry.start = None;
    geometry.end = None;
    let reference = geometry.topology.as_mut().unwrap();
    reference.path = HistoricalEdgePath::Loop { loop_id };
    reference.faces = targets
        .into_iter()
        .enumerate()
        .map(|(i, face)| HistoricalFaceTopologyRef {
            face: HistoricalFaceId::mint(format!("face-{i}")).unwrap(),
            topology_faces: NonEmptyMembers::try_from(vec![face]).unwrap(),
        })
        .collect();
    geometry.faces = DistinctMembers::try_from(
        reference
            .faces
            .iter()
            .map(|face| face.face.clone())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert!(findings(&ir).is_empty());
    let nest = &ir.model.history_bodies[0].topology;
    assert_eq!(
        nest.loops
            .iter()
            .find(|ring| ring.face.as_str().ends_with("#bottom"))
            .unwrap()
            .coedges(),
        order
    );
    assert_eq!(
        order
            .iter()
            .map(crate::ids::CoedgeId::as_str)
            .collect::<Vec<_>>(),
        [
            "synthetic:history-body:coedge#bottom:0",
            "synthetic:history-body:coedge#bottom:1",
            "synthetic:history-body:coedge#bottom:2",
            "synthetic:history-body:coedge#bottom:3",
        ]
    );
    for id in &order {
        assert_eq!(
            nest.coedges
                .iter()
                .find(|coedge| coedge.id == *id)
                .unwrap()
                .sense,
            Sense::Reversed
        );
    }
    assert_eq!(
        nest.edges[1].carrier.param_range().unwrap().get(),
        [0.0, std::f64::consts::PI]
    );
}

#[test]
fn refuses_missing_target_wrong_consumer_missing_domain_and_duplicate_mapping() {
    let base = fixture();
    for mutation in 0..7 {
        let mut ir = base.clone();
        match mutation {
            0 => ir.model.history_bodies.clear(),
            1 => ir.model.history_bodies[0].consumed_by = None,
            2 => {
                ir.model.history_bodies[0].topology.edges[0].carrier =
                    EdgeCarrier::Curve(ir.model.history_bodies[0].topology.curves[0].id.clone());
            }
            3 => {
                let reference = ir.model.feature_input_topologies[0].edge_geometry[0]
                    .topology
                    .as_mut()
                    .unwrap();
                reference.faces.push(reference.faces[0].clone());
            }
            4 => ir.model.feature_input_topologies[0].edge_geometry[0]
                .topology
                .as_mut()
                .unwrap()
                .faces
                .clear(),
            5 => ir.model.history_bodies[0].topology.coedges.clear(),
            _ => {
                ir.model.feature_input_topologies[0].edge_geometry[0].tangent =
                    Some(crate::units::UnitVector3::Y_AXIS);
            }
        }
        assert!(!findings(&ir).is_empty(), "mutation {mutation}");
    }
}

#[test]
fn refuses_unresolved_carriers_duplicate_targets_and_incomplete_rings() {
    let base = fixture();
    for mutation in 0..4 {
        let mut ir = base.clone();
        match mutation {
            0 => {
                ir.model.history_bodies[0].topology.curves[0].geometry =
                    CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None });
            }
            1 => {
                let binding = &mut ir.model.feature_input_topologies[0].edge_geometry[0]
                    .topology
                    .as_mut()
                    .unwrap()
                    .faces[0];
                let face = binding.topology_faces[0].clone();
                binding.topology_faces =
                    NonEmptyMembers::try_from(vec![face.clone(), face]).unwrap();
            }
            2 => {
                ir.model.feature_input_topologies[0].edge_geometry[0]
                    .topology
                    .as_mut()
                    .unwrap()
                    .path = HistoricalEdgePath::Edge {
                    edge: crate::ids::EdgeId::mint("test:nest:edge#missing").unwrap(),
                }
            }
            _ => {
                ir.model.feature_input_topologies[0].edge_geometry[0]
                    .topology
                    .as_mut()
                    .unwrap()
                    .path = HistoricalEdgePath::Loop {
                    loop_id: crate::ids::LoopId::mint("test:nest:loop#missing").unwrap(),
                }
            }
        }
        assert!(!findings(&ir).is_empty());
    }
}

#[test]
fn body_transform_and_document_tolerances_control_sample_comparison() {
    let mut ir = fixture();
    let transform = crate::transform::Transform::affine([
        [1.0, 0.0, 0.0, 20.0],
        [0.0, 1.0, 0.0, 30.0],
        [0.0, 0.0, 1.0, 40.0],
    ])
    .unwrap();
    ir.model.history_bodies[0].topology.body.transform = Some(transform);
    let geometry = &mut ir.model.feature_input_topologies[0].edge_geometry[0];
    geometry.interior = transform.apply_point(geometry.interior.get()).unwrap();
    geometry.start = geometry
        .start
        .and_then(|point| transform.apply_point(point.get()));
    geometry.end = geometry
        .end
        .and_then(|point| transform.apply_point(point.get()));
    assert!(findings(&ir).is_empty());
    ir.model.feature_input_topologies[0].edge_geometry[0].interior =
        crate::features::geometry::FinitePoint3::new(Point3::new(25.0, 30.1, 40.0)).unwrap();
    ir.tolerances = crate::units::Tolerances::new(0.2, 0.01).unwrap();
    assert!(findings(&ir).is_empty());
    ir.tolerances = crate::units::Tolerances::new(0.05, 0.01).unwrap();
    assert!(!findings(&ir).is_empty());
}

#[test]
fn incomplete_ordered_ring_is_refused() {
    let mut ir = fixture();
    let nest = &mut ir.model.history_bodies[0].topology;
    let ring = nest
        .loops
        .iter_mut()
        .find(|ring| ring.face.as_str().ends_with("#bottom"))
        .unwrap();
    let loop_id = ring.id.clone();
    ring.boundary = crate::topology::LoopBoundary::Ring(
        crate::topology::LoopRing::new(ring.coedges()[..3].to_vec(), Vec::new()).unwrap(),
    );
    ir.model.feature_input_topologies[0].edge_geometry[0]
        .topology
        .as_mut()
        .unwrap()
        .path = HistoricalEdgePath::Loop { loop_id };
    assert!(findings(&ir)
        .iter()
        .any(|finding| finding.message.contains("incomplete or open")));
}

fn bottom_outline(ir: &mut CadIr) {
    let body = &ir.model.history_bodies[0];
    let ring = body
        .topology
        .loops
        .iter()
        .find(|ring| ring.face.as_str().ends_with("#bottom"))
        .unwrap();
    let bindings = body
        .topology
        .faces
        .iter()
        .filter(|face| !face.id.as_str().ends_with("#top"))
        .enumerate()
        .map(|(index, face)| HistoricalFaceTopologyRef {
            face: HistoricalFaceId::mint(format!("outline-face-{index}")).unwrap(),
            topology_faces: NonEmptyMembers::try_from(vec![face.id.clone()]).unwrap(),
        })
        .collect::<Vec<_>>();
    let state = &mut ir.model.feature_input_topologies[0];
    let geometry = &mut state.edge_geometry[0];
    geometry.start = None;
    geometry.end = None;
    geometry.tangent = Some(crate::units::UnitVector3::new(Vector3::new(-1.0, 0.0, 0.0)).unwrap());
    geometry.faces = DistinctMembers::try_from(
        bindings
            .iter()
            .map(|binding| binding.face.clone())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    geometry.topology = Some(HistoricalEdgeTopologyRef {
        history_body: body.id.clone(),
        path: HistoricalEdgePath::Loop {
            loop_id: ring.id.clone(),
        },
        faces: bindings,
    });
    state.faces = geometry.faces.clone();
}

#[test]
fn loop_samples_and_tangents_use_the_body_transform_and_coedge_sense() {
    let mut ir = fixture();
    bottom_outline(&mut ir);
    ir.tolerances = crate::units::Tolerances::new(0.001, 0.01).unwrap();
    ir.model.history_bodies[0].topology.body.transform = crate::transform::Transform::affine([
        [10.0, 0.0, 0.0, 0.0],
        [0.0, 10.0, 0.0, 0.0],
        [0.0, 0.0, 10.0, 0.0],
    ]);
    ir.model.feature_input_topologies[0].edge_geometry[0].interior =
        crate::features::geometry::FinitePoint3::new(Point3::new(50.0, 0.0, 0.0)).unwrap();
    assert!(findings(&ir).is_empty());
    ir.model.feature_input_topologies[0].edge_geometry[0].interior =
        crate::features::geometry::FinitePoint3::new(Point3::new(50.0, 0.005, 0.0)).unwrap();
    assert!(!findings(&ir).is_empty());
    ir.model.history_bodies[0].topology.body.transform = crate::transform::Transform::affine([
        [0.1, 0.0, 0.0, 0.0],
        [0.0, 0.1, 0.0, 0.0],
        [0.0, 0.0, 0.1, 0.0],
    ]);
    ir.model.feature_input_topologies[0].edge_geometry[0].interior =
        crate::features::geometry::FinitePoint3::new(Point3::new(0.5, 0.0005, 0.0)).unwrap();
    assert!(findings(&ir).is_empty());
    ir.model.feature_input_topologies[0].edge_geometry[0].tangent =
        Some(crate::units::UnitVector3::X_AXIS);
    assert!(!findings(&ir).is_empty());
}

#[test]
fn surface_incidence_uses_document_tolerance_in_the_comparison_frame() {
    let mut ir = fixture();
    ir.tolerances = crate::units::Tolerances::new(0.001, 0.01).unwrap();
    ir.model.history_bodies[0].topology.curves[0].geometry =
        CurveGeometry::Solved(SolvedCurveGeometry::Line(
            crate::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0005, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ));
    let geometry = &mut ir.model.feature_input_topologies[0].edge_geometry[0];
    geometry.start = None;
    geometry.end = None;
    geometry.interior =
        crate::features::geometry::FinitePoint3::new(Point3::new(5.0, 0.0005, 0.0)).unwrap();
    assert!(findings(&ir).is_empty());
    ir.model.history_bodies[0].topology.body.transform = crate::transform::Transform::affine([
        [10.0, 0.0, 0.0, 0.0],
        [0.0, 10.0, 0.0, 0.0],
        [0.0, 0.0, 10.0, 0.0],
    ]);
    ir.model.feature_input_topologies[0].edge_geometry[0].interior =
        crate::features::geometry::FinitePoint3::new(Point3::new(50.0, 0.005, 0.0)).unwrap();
    assert!(findings(&ir)
        .iter()
        .any(|finding| finding.message.contains("adjacent surface")));
}

#[test]
fn a_vertex_free_edge_requires_a_closed_carrier_domain_for_a_loop_path() {
    let mut ir = fixture();
    let nest = &mut ir.model.history_bodies[0].topology;
    nest.edges[0].ends = None;
    let coedge = nest
        .coedges
        .iter()
        .find(|coedge| {
            coedge.edge == nest.edges[0].id && coedge.owner_loop.as_str().ends_with("#bottom")
        })
        .unwrap()
        .id
        .clone();
    let ring = nest
        .loops
        .iter_mut()
        .find(|ring| ring.id.as_str().ends_with("#bottom"))
        .unwrap();
    ring.boundary = crate::topology::LoopBoundary::Ring(crate::topology::LoopRing::single(coedge));
    ir.model.feature_input_topologies[0].edge_geometry[0]
        .topology
        .as_mut()
        .unwrap()
        .path = HistoricalEdgePath::Loop {
        loop_id: ring.id.clone(),
    };
    assert!(findings(&ir)
        .iter()
        .any(|finding| finding.message.contains("closed carrier domains")));
}
