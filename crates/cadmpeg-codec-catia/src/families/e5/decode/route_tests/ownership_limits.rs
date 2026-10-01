use super::{e5_loop_members, BTreeMap, E5Edge, E5Face, E5Loop, E5OrientedMember, E5Topology};
use crate::families::e5::decode::resolve_e5_ownership;

#[test]
fn e5_ownership_refuses_each_counted_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::HashSet;

    let face = |record_id| E5Face {
        record_id,
        surface: 100 + record_id,
        trailer_sign: crate::families::e5::graph::Sign::Positive,
        loops: vec![E5Loop {
            record_id: 200 + record_id,
            surface: 100 + record_id,
            members: e5_loop_members(&[300 + record_id], &[10], &[false]),
            oriented_members: Some(vec![E5OrientedMember {
                serialized_index: 0,
                reversed: false,
            }]),
            outer: Some(true),
            orientation_hint: None,
        }],
    };
    let topology = E5Topology {
        bodies: Vec::new(),
        faces: vec![face(1), face(2)],
        edges: [(
            10,
            E5Edge {
                support: 20,
                start_vertex: 30,
                end_vertex: 31,
                parameter_start: 40,
                parameter_end: 41,
                tail: Vec::new(),
            },
        )]
        .into(),
        pcurves: BTreeMap::new(),
        bounds: BTreeMap::new(),
        curve_supports: BTreeMap::new(),
        vertex_refs: Vec::new(),
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("fixture fits input limit");
    let ownership = resolve_e5_ownership(&ctx, &topology)
        .expect("service resource budget")
        .expect("connected body plan");
    assert_eq!(ownership.bodies[0].components, vec![vec![1, 2]]);

    let mut operations = HashSet::new();
    for cap in 0..=128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits input limit");
        match resolve_e5_ownership(&ctx, &topology) {
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                operations.insert(limit.operation);
            }
            Ok(Some(ownership)) => {
                assert_eq!(ownership.bodies[0].components, vec![vec![1, 2]]);
            }
            Ok(None) => panic!("connected body must have an ownership plan"),
            Err(error) => panic!("unexpected ownership refusal: {error}"),
        }
    }
    for operation in [
        "catia_e5_ownership_faces",
        "catia_e5_ownership_bodies",
        "catia_e5_body_faces",
        "catia_e5_body_uses",
        "catia_e5_edge_bodies",
        "catia_e5_edge_body_members",
        "catia_e5_body_edge_uses",
        "catia_e5_face_indices",
        "catia_e5_face_union",
        "catia_e5_first_edge_face",
        "catia_e5_component_labels",
        "catia_e5_face_components",
        "catia_e5_components",
        "catia_e5_component_faces",
        "catia_e5_closed_components",
        "catia_e5_component_edges",
        "catia_e5_body_plans",
        "catia_e5_face_shells",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
}
