// SPDX-License-Identifier: Apache-2.0
use super::*;
const EPS_CLOSED_SAMPLE: f64 = 1.0e-12;
#[test]
fn input_edge_bindings_scope_repeated_slots_and_refuse_sample_identity() {
    use cadmpeg_ir::features::evaluation::{
        FeatureInputTopology, HistoricalEdgeGeometry, HistoricalEdgePath,
    };
    use cadmpeg_ir::features::geometry::FinitePoint3;
    let first = feature("first");
    let second = feature("second");
    let cube = cadmpeg_ir::examples::unit_cube().unwrap().geometry;
    let mut nest = BodyTopology::new(cube.bodies[0].clone());
    nest.edges = cube.edges;
    nest.curves = cube.curves;
    nest.faces = cube.faces;
    let input = |owner: &Feature, state, slot| {
        let prefix = crate::ids::feature_input_prefix(&owner.id, state);
        let geometry = HistoricalEdgeGeometry::new(
            crate::ids::history_input_edge_id(&prefix, slot),
            FinitePoint3::ZERO,
        );
        FeatureInputTopology {
            face_owners: Vec::new(),
            id: crate::ids::history_input_state_id(&prefix),
            input_of: owner.id.clone(),
            bodies: DistinctMembers::default(),
            faces: DistinctMembers::default(),
            edges: DistinctMembers::try_from(vec![geometry.edge.clone()]).unwrap(),
            vertices: DistinctMembers::default(),
            edge_geometry: vec![geometry],
            native_ref: None,
        }
    };
    for (owner, state, nest_index) in [(&first, 12, 0), (&second, 12, 1), (&first, 13, 2)] {
        let input = input(owner, state, 0);
        let want = Want {
            origin: owner,
            state,
            slot: 0,
            capture_current: false,
            consumed: true,
            input: Some(&input),
            bindings: RefCell::default(),
            identity: None,
            producer: None,
            consumer: None,
        };
        let bindings = input_bindings(&want, &nest, nest_index);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].0, input.edge_geometry[0].edge);
        assert_eq!(
            bindings[0].1.path,
            HistoricalEdgePath::Edge {
                edge: nest.edges[0].id.clone()
            }
        );
        assert_eq!(
            bindings[0].1.history_body.as_str(),
            format!("f3d:history-body:nest#h{nest_index:04}")
        );
    }
    let input = input(&first, 12, 0);
    let wrong_state = Want {
        origin: &first,
        state: 13,
        slot: 0,
        capture_current: false,
        consumed: true,
        input: Some(&input),
        bindings: RefCell::default(),
        identity: None,
        producer: None,
        consumer: None,
    };
    assert!(input_bindings(&wrong_state, &nest, 0).is_empty());
    let wrong_history = Want {
        origin: &second,
        state: 12,
        ..wrong_state
    };
    assert!(input_bindings(&wrong_history, &nest, 0).is_empty());
}

#[test]
fn two_closed_carriers_with_equal_samples_require_the_native_edge_slot() {
    use cadmpeg_ir::features::evaluation::{
        FeatureInputTopology, HistoricalEdgeGeometry, HistoricalEdgePath,
    };
    use cadmpeg_ir::features::geometry::FinitePoint3;
    use cadmpeg_ir::geometry::analytic::CircleCurve;
    use cadmpeg_ir::math::{Point3, Vector3};
    let owner = feature("closed");
    let cube = cadmpeg_ir::examples::unit_cube().unwrap().geometry;
    let mut nest = BodyTopology::new(cube.bodies[0].clone());
    nest.edges = cube.edges[..2].to_vec();
    nest.curves = cube.curves[..2].to_vec();
    for (index, sign) in [(0, 1.0), (1, -1.0)] {
        nest.curves[index].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                Point3::new(sign, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(sign, 0.0, 0.0),
                1.0,
            )
            .unwrap(),
        ));
        nest.edges[index].ends = None;
        nest.edges[index].carrier = cadmpeg_ir::topology::EdgeCarrier::new(
            Some(nest.curves[index].id.clone()),
            Some([0.0, std::f64::consts::TAU]),
        )
        .unwrap();
    }
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    for curve in &nest.curves {
        let point = cadmpeg_ir::eval::decode::curve_point_solved(
            &ctx,
            curve.geometry.solved().unwrap(),
            std::f64::consts::PI,
        )
        .unwrap();
        assert!(point.get().vector_from(Point3::new(0.0, 0.0, 0.0)).norm() <= EPS_CLOSED_SAMPLE);
    }
    let prefix = crate::ids::feature_input_prefix(&owner.id, 12);
    let geometry = HistoricalEdgeGeometry::new(
        crate::ids::history_input_edge_id(&prefix, 0),
        FinitePoint3::ZERO,
    );
    let mut input = FeatureInputTopology {
        face_owners: Vec::new(),
        id: crate::ids::history_input_state_id(&prefix),
        input_of: owner.id.clone(),
        bodies: DistinctMembers::default(),
        faces: DistinctMembers::default(),
        edges: DistinctMembers::try_from(vec![geometry.edge.clone()]).unwrap(),
        vertices: DistinctMembers::default(),
        edge_geometry: vec![geometry],
        native_ref: None,
    };
    let bind = |input: &FeatureInputTopology| {
        input_bindings(
            &Want {
                origin: &owner,
                state: 12,
                slot: 0,
                capture_current: false,
                consumed: true,
                input: Some(input),
                bindings: RefCell::default(),
                identity: None,
                producer: None,
                consumer: None,
            },
            &nest,
            0,
        )
    };
    assert_eq!(
        bind(&input)[0].1.path,
        HistoricalEdgePath::Edge {
            edge: nest.edges[0].id.clone()
        }
    );
    input.edge_geometry[0].edge = crate::ids::history_input_edge_id(&prefix, 99);
    assert!(bind(&input).is_empty());
}

#[test]
fn treatment_snapshot_requests_join_history_state_and_body_membership() {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalCarrierBinding, AsmHistoricalEdge,
        AsmHistoricalOptionalCarrierBinding, AsmHistoricalRelation, AsmHistoricalTopology,
    };
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use cadmpeg_ir::features::evaluation::{FeatureInputTopology, HistoricalEdgeGeometry};
    use cadmpeg_ir::features::geometry::FinitePoint3;
    use cadmpeg_ir::features::selections::NonEmptyMembers;
    let relation = |owner_ref, member_refs| AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    let topology = AsmHistoricalTopology {
        bodies: vec![1],
        regions: vec![2],
        shells: vec![3],
        edges: vec![4],
        vertices: vec![5, 6],
        points: vec![7, 8],
        curves: vec![9],
        body_regions: vec![relation(1, vec![2])],
        region_shells: vec![relation(2, vec![3])],
        shell_faces: vec![relation(3, Vec::new())],
        shell_wire_edges: vec![relation(3, vec![4])],
        shell_free_vertices: vec![relation(3, Vec::new())],
        edge_vertices: vec![AsmHistoricalEdge {
            edge: 4,
            start_vertex: 5,
            end_vertex: 6,
        }],
        vertex_points: vec![
            AsmHistoricalCarrierBinding {
                entity: 5,
                carrier: 7,
            },
            AsmHistoricalCarrierBinding {
                entity: 6,
                carrier: 8,
            },
        ],
        edge_curves: vec![AsmHistoricalOptionalCarrierBinding {
            entity: 4,
            carrier: Some(9),
        }],
        ..AsmHistoricalTopology::default()
    };
    let history = |source: &str| {
        crate::test_support::history::history(
            format!("f3d:{source}:asm-history#0"),
            0,
            None,
            false,
            [14, 13, 12]
                .into_iter()
                .map(|value| {
                    (
                        AsmDeltaState {
                            id: format!("f3d:{source}:state#{value}"),
                            parent: format!("f3d:{source}:asm-history#0"),
                            byte_offset: 0,
                            state_id: value,
                            version_flag: 1,
                            state_flag: 0,
                            previous_ref: None,
                            next_ref: None,
                            node_index: 14 - value,
                            partner_ref: None,
                            owner_ref: 0,
                            bulletin_boards: Vec::new(),
                            records: Vec::new(),
                            transition: None,
                        },
                        Some(topology.clone()),
                        Vec::new(),
                    )
                })
                .collect(),
        )
    };
    let histories = [history("a"), history("b")];
    let mut scope =
        DesignParameterScope::empty("f3d:design:scope#treatment", DesignFeatureKind::Fillet, 1);
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(13);
            draft.previous_history_state_id = Some(12);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut feature = feature("treatment");
    feature.native_ref = Some(scope.id.clone());
    let prefix = crate::ids::feature_input_prefix(&feature.id, 12);
    let edge = crate::ids::history_input_edge_id(&prefix, 4);
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Fillet {
            groups: NonEmptyMembers::try_from(vec![
                cadmpeg_ir::features::edge_treatments::FilletGroup {
                    edges: cadmpeg_ir::features::selections::EdgeSelection::historical(
                        crate::ids::history_input_state_id(&prefix),
                        vec![edge.clone()],
                        "native-group".into(),
                    )
                    .unwrap(),
                    radius: cadmpeg_ir::features::edge_treatments::RadiusSpec::Unresolved {
                        form: None,
                    },
                    tangency_weight: None,
                },
            ])
            .unwrap(),
        },
    ));
    let mut input = FeatureInputTopology {
        face_owners: Vec::new(),
        id: crate::ids::history_input_state_id(&prefix),
        input_of: feature.id.clone(),
        bodies: DistinctMembers::try_from(vec![crate::ids::history_input_body_id(&prefix, 1)])
            .unwrap(),
        faces: DistinctMembers::default(),
        edges: DistinctMembers::try_from(vec![edge.clone()]).unwrap(),
        vertices: DistinctMembers::default(),
        edge_geometry: vec![HistoricalEdgeGeometry::new(edge, FinitePoint3::ZERO)],
        native_ref: Some("f3d:b:state#12".into()),
    };
    let bindings = HashMap::from([(scope.id.clone(), histories[1].id.clone())]);
    let features = [feature.clone()];
    let scopes = [scope];
    let inputs = Inputs {
        features: &features,
        scopes: &scopes,
        histories: &histories,
        bindings: &bindings,
        primary_brep: None,
        groups: &[],
        operands: &[],
        recipes: &[],
    };
    let wants = input_edge_bodies(&feature, inputs, std::slice::from_ref(&input));
    assert_eq!(wants.len(), 1);
    assert_eq!(
        (
            wants[0].0,
            wants[0].1.state,
            wants[0].1.slot,
            wants[0].1.consumed
        ),
        (1, 12, 1, true)
    );
    drop(wants);
    input.bodies.clear();
    assert!(input_edge_bodies(&feature, inputs, std::slice::from_ref(&input)).is_empty());
    input
        .bodies
        .insert(crate::ids::history_input_body_id(&prefix, 1));
    let other = HashMap::from([(scopes[0].id.clone(), histories[0].id.clone())]);
    let inputs = Inputs {
        bindings: &other,
        ..inputs
    };
    assert!(input_edge_bodies(&feature, inputs, std::slice::from_ref(&input)).is_empty());
    let unbound = HashMap::new();
    let inputs = Inputs {
        bindings: &unbound,
        ..inputs
    };
    assert!(input_edge_bodies(&feature, inputs, std::slice::from_ref(&input)).is_empty());
}

#[test]
fn source_procedural_curve_with_a_solved_cache_keeps_the_sample_representation() {
    use cadmpeg_ir::features::evaluation::{HistoricalEdgePath, HistoricalEdgeTopologyRef};
    let mut source = cadmpeg_ir::examples::unit_cube().unwrap().geometry;
    let reference = HistoricalEdgeTopologyRef {
        history_body: HistoryBodyId::mint("test:history-body:nest#input").unwrap(),
        path: HistoricalEdgePath::Edge {
            edge: source.edges[0].id.clone(),
        },
        faces: Vec::new(),
    };
    assert!(source_carriers_solved(&source, &reference));
    let cache = source.curves[0].geometry.solved().cloned();
    source.curves[0].geometry = CurveGeometry::Procedural {
        construction: cadmpeg_ir::ids::ProceduralCurveId::mint("test:history:construction#curve")
            .unwrap(),
        cache,
    };
    assert!(!source_carriers_solved(&source, &reference));
}

#[test]
fn failed_input_binding_preserves_samples_supports_and_operands_with_a_loss() {
    use cadmpeg_ir::features::evaluation::{
        EdgeSupport, FeatureInputTopology, HistoricalEdgeGeometry,
    };
    use cadmpeg_ir::features::geometry::FinitePoint3;
    use cadmpeg_ir::features::selections::{EdgeSelection, NonEmptyMembers};
    use std::io::{Cursor, Write};
    let mut ir = CadIr::empty();
    let mut feature = feature("unbound-treatment");
    let prefix = crate::ids::feature_input_prefix(&feature.id, 12);
    let edge = crate::ids::history_input_edge_id(&prefix, 4);
    let face = crate::ids::history_input_face_id(&prefix, 10);
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Fillet {
            groups: NonEmptyMembers::try_from(vec![
                cadmpeg_ir::features::edge_treatments::FilletGroup {
                    edges: EdgeSelection::historical(
                        crate::ids::history_input_state_id(&prefix),
                        vec![edge.clone()],
                        "native-group".into(),
                    )
                    .unwrap(),
                    radius: cadmpeg_ir::features::edge_treatments::RadiusSpec::Unresolved {
                        form: None,
                    },
                    tangency_weight: None,
                },
            ])
            .unwrap(),
        },
    ));
    let mut geometry = HistoricalEdgeGeometry::new(edge.clone(), FinitePoint3::ZERO);
    geometry.start = Some(FinitePoint3::ZERO);
    geometry.end = Some(FinitePoint3::ZERO);
    geometry.tangent = Some(cadmpeg_ir::units::UnitVector3::X_AXIS);
    geometry.faces.insert(face.clone());
    geometry.supports.push(EdgeSupport {
        face: face.clone(),
        normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
        with_edge: true,
    });
    ir.model
        .feature_input_topologies
        .push(FeatureInputTopology {
            face_owners: Vec::new(),
            id: crate::ids::history_input_state_id(&prefix),
            input_of: feature.id.clone(),
            bodies: DistinctMembers::default(),
            faces: DistinctMembers::try_from(vec![face]).unwrap(),
            edges: DistinctMembers::try_from(vec![edge.clone()]).unwrap(),
            vertices: DistinctMembers::default(),
            edge_geometry: vec![geometry.clone()],
            native_ref: Some("f3d:native:state#12".into()),
        });
    let evaluation = feature.evaluation.clone();
    ir.model.features.push(feature.clone());
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, options);
    zip.start_file("marker", options).unwrap();
    zip.write_all(b"fixture").unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let mut losses = Vec::new();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        fill_consumed(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            scan,
            &mut ir,
            &F3dNative::default(),
            None,
            &ConsumerContext::default(),
            &mut losses,
        )
        .unwrap();
    });
    assert_eq!(
        ir.model.feature_input_topologies[0].edge_geometry,
        vec![geometry]
    );
    assert_eq!(ir.model.features[0].evaluation, evaluation);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureHistoricalEdgeTopologyUnbound.kind()
    );
    assert!(losses[0].message.contains(feature.id.as_str()));
    assert!(losses[0].message.contains("f3d:native:state#12"));
    assert!(losses[0].message.contains(edge.as_str()));
}
