// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::features::evaluation::{FeatureInputTopology, HistoricalEdgeGeometry};
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::{BodyId, FeatureInputTopologyId, HistoricalEdgeId};
use cadmpeg_ir::units::UnitVector3;

#[test]
fn resolved_bodies_keep_only_the_source_name() {
    let id = BodyId::mint("inventor:brep:body#synthetic").expect("id");
    let value = bodies(&BodySelection::Resolved {
        bodies: vec![id].try_into().expect("bodies"),
        native: "inventor:pmdc:selection#synthetic".into(),
    })
    .expect("selection");
    let Picks::Named(picks) = value.picks else {
        panic!("named picks")
    };
    let pick = &picks.as_slice()[0];
    assert_eq!(pick.resolved, None);
    assert_eq!(
        pick.native.as_ref().expect("name").value,
        NativeNameValue::Text(cadmpeg_core::nonblank_literal!(
            "inventor:pmdc:selection#synthetic"
        ))
    );
}

#[test]
fn historical_edges_keep_samples_and_require_one_input_state() {
    let state =
        FeatureInputTopologyId::mint("inventor:design:feature-input#synthetic").expect("id");
    let id = HistoricalEdgeId::mint("inventor:input-edge#synthetic-7").expect("id");
    let point = |values| json::from_value(cadmpeg_core::json!(values)).expect("point");
    let sample = HistoricalEdgeGeometry {
        tangent: Some(UnitVector3::X_AXIS),
        start: Some(point([0., 2., 3.])),
        end: Some(point([2., 2., 3.])),
        faces: DistinctMembers::default(),
        carrier: None,
        supports: Vec::new(),
        ..HistoricalEdgeGeometry::new(id.clone(), point([1., 2., 3.]))
    };
    let input = FeatureInputTopology {
        id: state.clone(),
        input_of: FeatureId::mint("inventor:design:feature#fillet").expect("id"),
        bodies: DistinctMembers::default(),
        faces: DistinctMembers::default(),
        edges: std::iter::once(id.clone()).collect(),
        vertices: DistinctMembers::default(),
        face_owners: Vec::new(),
        face_geometry: Vec::new(),
        body_writers: Vec::new(),
        edge_geometry: vec![sample.clone()],
        native_ref: None,
    };
    let source = EdgeSelection::historical(
        state,
        vec![id.clone()],
        "inventor:pmdc:selection#synthetic".into(),
    )
    .expect("source selection");
    let value = edge(&source, std::slice::from_ref(&input)).expect("selection");
    let Picks::Named(picks) = value.picks else {
        panic!("named picks")
    };
    let pick = &picks.as_slice()[0];
    assert_eq!(
        pick.native.as_ref().expect("name").value,
        NativeNameValue::Integers(vec![7])
    );
    let witness = pick.witness.as_ref().expect("witness");
    assert_eq!(witness.point, Some(sample.interior));
    assert_eq!(witness.direction, sample.tangent);
    assert_eq!(
        witness.endpoints,
        Some([sample.start.expect("start"), sample.end.expect("end")])
    );
    assert!(edge(&source, &[]).is_err());
    assert!(edge(&source, &[input.clone(), input]).is_err());
}

#[test]
fn oriented_chamfer_support_is_the_one_face_of_its_input_state() {
    use cadmpeg_ir::ids::HistoricalFaceId;
    let state =
        FeatureInputTopologyId::mint("inventor:design:feature-input#synthetic").expect("id");
    let edge_id = HistoricalEdgeId::mint("inventor:input-edge#synthetic-7").expect("id");
    let face_id = HistoricalFaceId::mint("inventor:input-face#synthetic-9").expect("id");
    let input = FeatureInputTopology {
        id: state.clone(),
        input_of: FeatureId::mint("inventor:design:feature#chamfer").expect("id"),
        bodies: DistinctMembers::default(),
        faces: std::iter::once(face_id).collect(),
        edges: std::iter::once(edge_id.clone()).collect(),
        vertices: DistinctMembers::default(),
        face_owners: Vec::new(),
        face_geometry: Vec::new(),
        body_writers: Vec::new(),
        edge_geometry: Vec::new(),
        native_ref: None,
    };
    let source = EdgeSelection::historical(
        state,
        vec![edge_id],
        "inventor:pmdc:selection#synthetic".into(),
    )
    .expect("source selection");
    let value = support_face(&source, std::slice::from_ref(&input)).expect("support");
    let Picks::Named(picks) = value.picks else {
        panic!("named picks")
    };
    assert_eq!(
        picks.as_slice()[0].native.as_ref().expect("name").value,
        NativeNameValue::Integers(vec![9])
    );
    assert!(support_face(&source, &[]).is_err());
    let mut two = input.clone();
    two.faces = [
        "inventor:input-face#synthetic-9",
        "inventor:input-face#synthetic-10",
    ]
    .into_iter()
    .map(|id| HistoricalFaceId::mint(id).expect("id"))
    .collect();
    assert!(support_face(&source, &[two]).is_err());
    let native = EdgeSelection::Native("inventor:pmdc:selection#synthetic".into());
    assert!(support_face(&native, &[input]).is_err());
}
