// SPDX-License-Identifier: Apache-2.0

use crate::features::evaluation::{
    HistoricalEdgeGeometry, HistoricalEdgePath, HistoricalEdgeTopologyRef,
    HistoricalFaceTopologyRef,
};
use crate::features::selections::NonEmptyMembers;
use crate::ids::{FaceId, HistoricalEdgeId, HistoricalFaceId, HistoryBodyId, LoopId};
use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

#[test]
fn sampled_constructor_omits_topology_and_refuses_explicit_null() {
    let sample = HistoricalEdgeGeometry::new(
        HistoricalEdgeId::mint("local-edge").unwrap(),
        crate::features::geometry::FinitePoint3::ZERO,
    );
    let mut wire = serde_json::to_value(&sample).unwrap();
    assert!(wire.get("topology").is_none());
    let decoded: HistoricalEdgeGeometry =
        cadmpeg_core::serde_erase::json::from_value(wire.clone()).unwrap();
    assert_eq!(sample, decoded);
    wire["topology"] = serde_json::Value::Null;
    assert!(cadmpeg_core::serde_erase::json::from_value::<HistoricalEdgeGeometry>(wire).is_err());
}

#[test]
fn loop_path_wire_and_rewrite_keep_local_face_keys() {
    let reference = HistoricalEdgeTopologyRef {
        history_body: HistoryBodyId::mint("test:history:nest#body").unwrap(),
        path: HistoricalEdgePath::Loop {
            loop_id: LoopId::mint("test:history:loop#outline").unwrap(),
        },
        faces: vec![HistoricalFaceTopologyRef {
            face: HistoricalFaceId::mint("local-face").unwrap(),
            topology_faces: NonEmptyMembers::try_from(vec![FaceId::mint(
                "test:history:face#support",
            )
            .unwrap()])
            .unwrap(),
        }],
    };
    let wire = serde_json::to_value(&reference).unwrap();
    assert_eq!(
        wire["path"],
        serde_json::json!({"kind": "loop", "loop_id": "test:history:loop#outline"})
    );
    let decoded: HistoricalEdgeTopologyRef =
        cadmpeg_core::serde_erase::json::from_value(wire).unwrap();
    assert_eq!(reference, decoded);
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut map = IdentityMap::new(|id: &str| Ok(id.replace(":history:", ":renamed:")));
    let rewritten = reference.rewrite_identities(&ctx, &mut map).unwrap();
    map.finish().unwrap();
    assert_eq!(rewritten.history_body.as_str(), "test:renamed:nest#body");
    assert_eq!(
        rewritten.path,
        HistoricalEdgePath::Loop {
            loop_id: LoopId::mint("test:renamed:loop#outline").unwrap()
        }
    );
    assert_eq!(rewritten.faces[0].face.as_str(), "local-face");
    assert_eq!(
        rewritten.faces[0].topology_faces[0].as_str(),
        "test:renamed:face#support"
    );
}
