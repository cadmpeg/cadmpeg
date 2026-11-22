// SPDX-License-Identifier: Apache-2.0
use super::*;
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
    let point = |values| serde_json::from_value(serde_json::json!(values)).expect("point");
    let sample = HistoricalEdgeGeometry {
        edge: id.clone(),
        interior: point([1., 2., 3.]),
        tangent: Some(UnitVector3::X_AXIS),
        start: Some(point([0., 2., 3.])),
        end: Some(point([2., 2., 3.])),
        faces: DistinctMembers::default(),
        carrier: None,
        supports: Vec::new(),
    };
    let input = FeatureInputTopology {
        id: state.clone(),
        input_of: FeatureId::mint("inventor:design:feature#fillet").expect("id"),
        bodies: DistinctMembers::default(),
        faces: DistinctMembers::default(),
        edges: std::iter::once(id.clone()).collect(),
        vertices: DistinctMembers::default(),
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
