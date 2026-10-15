use crate::families::e5::tests::e5_loop_members;

use crate::families::e5::decode::resolve_e5_ownership;

use crate::families::e5::graph::E5Loop;

use crate::families::e5::graph::E5Face;

use crate::families::e5::graph::E5OrientedMember;

use crate::families::e5::graph::E5Edge;

use crate::families::e5::graph::E5Topology;

#[test]
fn faces_sharing_an_edge_form_one_body_component() {
    use cadmpeg_core::decode::DecodeArena;
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_core::decode::DecodePolicy;
    use std::collections::BTreeMap;
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
        .expect("resolve E5 ownership")
        .expect("connected body plan");
    assert_eq!(ownership.bodies[0].components, vec![vec![1, 2]]);
}
