// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::features::evaluation::{FeatureEvaluation, HistoricalEdgeGeometry};
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::features::selections::{DistinctMembers, EdgeSelection};
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::ids::HistoricalFaceId;
use cadmpeg_ir::math::Point3;

#[test]
fn a_failed_binding_preserves_an_independent_admitted_binding() {
    let mut ir = cadmpeg_ir::examples::unit_cube_with_history_body().unwrap();
    let feature = &mut ir.model.features[0];
    let prefix = crate::ids::feature_input_prefix(&feature.id, 12);
    let edges = [
        crate::ids::history_input_edge_id(&prefix, 0),
        crate::ids::history_input_edge_id(&prefix, 1),
    ];
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Fillet {
            groups: NonEmptyMembers::try_from(vec![
                cadmpeg_ir::features::edge_treatments::FilletGroup {
                    edges: EdgeSelection::historical(
                        crate::ids::history_input_state_id(&prefix),
                        edges.to_vec(),
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
    let owner = feature.id.clone();
    let body = &mut ir.model.history_bodies[0];
    body.produced_by = None;
    body.consumed_by = Some(owner.clone());
    let mut geometry = Vec::new();
    let mut faces = DistinctMembers::default();
    for (slot, names, point) in [
        (0, ["bottom", "front"], Point3::new(5.0, 0.0, 0.0)),
        (1, ["bottom", "right"], Point3::new(0.0, 0.0, 0.0)),
    ] {
        let mut sample =
            HistoricalEdgeGeometry::new(edges[slot].clone(), FinitePoint3::new(point).unwrap());
        let bindings = names
            .into_iter()
            .map(|name| {
                let face = HistoricalFaceId::mint(format!("local-{name}")).unwrap();
                faces.insert(face.clone());
                let target = body
                    .topology
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
        sample.faces = DistinctMembers::try_from(
            bindings
                .iter()
                .map(|binding| binding.face.clone())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        sample.topology = Some(HistoricalEdgeTopologyRef {
            history_body: body.id.clone(),
            path: HistoricalEdgePath::Edge {
                edge: body.topology.edges[slot].id.clone(),
            },
            faces: bindings,
        });
        geometry.push(sample);
    }
    let bad_sample = geometry[1].clone();
    ir.model
        .feature_input_topologies
        .push(FeatureInputTopology {
            face_owners: Vec::new(),
            face_geometry: Vec::new(),
            body_writers: Vec::new(),
            id: crate::ids::history_input_state_id(&prefix),
            input_of: owner,
            bodies: DistinctMembers::default(),
            faces,
            edges: DistinctMembers::try_from(edges.to_vec()).unwrap(),
            vertices: DistinctMembers::default(),
            edge_geometry: geometry,
            native_ref: Some("f3d:native:state#12".into()),
        });
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let findings = binding_findings(
        &ctx,
        &ir.model,
        &ir.model.feature_input_topologies,
        ir.tolerances,
    )
    .unwrap();
    assert!(findings
        .iter()
        .any(|finding| finding.related.iter().any(|id| id == edges[1].as_str())));
    assert!(findings
        .iter()
        .all(|finding| !finding.related.iter().any(|id| id == edges[0].as_str())));
    let mut losses = Vec::new();
    admit_bound_edges(
        &ir.model.features,
        &mut ir.model.feature_input_topologies,
        &findings,
        &mut losses,
    );
    let samples = &ir.model.feature_input_topologies[0].edge_geometry;
    assert!(samples[0].topology.is_some());
    assert!(samples[1].topology.is_none());
    assert_eq!(samples[1].interior, bad_sample.interior);
    assert_eq!(samples[1].faces, bad_sample.faces);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains(edges[1].as_str()));
}
