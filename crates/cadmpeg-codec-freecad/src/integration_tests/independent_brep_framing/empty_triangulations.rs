// SPDX-License-Identifier: Apache-2.0
//! Empty native triangulations do not introduce an empty neutral mesh.
use super::*;

#[test]
fn empty_triangulations_keep_native_values_and_nonempty_unshaded_meshes() {
    for binary in [false, true] {
        let mut member = if binary {
            b"Open CASCADE Topology V4, (c) Open Cascade\n".to_vec()
        } else {
            b"CASCADE Topology V3, (c) Open Cascade\n".to_vec()
        };
        member.extend_from_slice(b"Locations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 3\n");
        for (nodes, triangles) in [(0_i32, 0_i32), (1, 0), (3, 1)] {
            if binary {
                member.extend_from_slice(&nodes.to_le_bytes());
                member.extend_from_slice(&triangles.to_le_bytes());
                member.extend_from_slice(&[0, 0]);
                member.extend_from_slice(&0.03_f64.to_le_bytes());
                for index in 0..nodes {
                    for value in [f64::from(index), 0.0, 0.0] {
                        member.extend_from_slice(&value.to_le_bytes());
                    }
                }
                if triangles != 0 {
                    for index in [1_i32, 2, 3] {
                        member.extend_from_slice(&index.to_le_bytes());
                    }
                }
            } else {
                member.extend_from_slice(format!("{nodes} {triangles} 0 0 0.03").as_bytes());
                for index in 0..nodes {
                    member.extend_from_slice(format!(" {index} 0 0").as_bytes());
                }
                if triangles != 0 {
                    member.extend_from_slice(b" 1 2 3");
                }
            }
            if !binary {
                member.push(b'\n');
            }
        }
        member.extend_from_slice(b"TShapes 0\n");
        if binary {
            for value in [-1_i32; 3] {
                member.extend_from_slice(&value.to_le_bytes());
            }
        } else {
            member.extend_from_slice(b"*\n");
        }
        let bytes = fixtures::archive(binary, &member);
        let decoded = decode(&bytes);
        assert_parts_agree(&bytes, &decoded);
        let payloads = crate::brep::records::payloads_untracked(
            decoded.ir().native.namespace("fcstd").unwrap(),
        )
        .unwrap();
        let payload = &payloads[0];
        let tables = crate::brep::Tables::from_payload(payload).unwrap();
        assert_eq!(tables.triangulations.len(), 3);
        for (row, (nodes, triangles)) in tables.triangulations.iter().zip([(0, 0), (1, 0), (3, 1)])
        {
            assert_eq!(row.nodes().len(), nodes);
            assert_eq!(row.triangles().len(), triangles);
            assert!(row.normals().is_none());
        }
        let meshes = &decoded.ir().model.tessellations;
        assert_eq!(meshes.len(), 2);
        assert_eq!(meshes[0].vertex_count(), 1);
        assert_eq!(meshes[0].triangle_count(), 0);
        assert_eq!(meshes[1].vertex_count(), 3);
        assert_eq!(meshes[1].triangle_count(), 1);
        for mesh in meshes {
            assert_eq!(serde_json::to_value(mesh).unwrap()["mesh"]["kind"], "list");
        }
        let empty_id = crate::native::model_id("tessellation", &payload.id, "1");
        assert!(!decoded
            .source_fidelity()
            .record_layout()
            .contains_key(&empty_id));
        assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
    }
}
