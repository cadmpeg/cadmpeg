use super::*;

#[test]
fn compact_version_six_body_preserves_equal_affine_maps_and_source_fields() {
    for equal in [true, false] {
        let mut graph = synthetic_mesh_graph(false);
        graph
            .meta
            .types
            .iter_mut()
            .find(|ty| {
                ty.type_guid
                    .as_str()
                    .eq_ignore_ascii_case(MESH_BODY_TYPE_GUID)
            })
            .unwrap()
            .version = 6;
        let frame = sole_typed_frame(&graph, MESH_BODY_TYPE_GUID);
        let start = frame.start;
        let end = frame.end;
        let old = &graph.bytes[start..end];
        let mut body = old[..25].to_vec();
        body[21..25].copy_from_slice(&1u32.to_le_bytes());
        body.extend_from_slice(&[0; 12]);
        let matrix = [
            0.2f64, 0.0, 0.0, 1.0, 0.0, 0.3, 0.0, 2.0, 0.0, 0.0, 0.4, 3.0, 0.0, 0.0, 0.0, 1.0,
        ];
        for ordinal in 0..2 {
            body.push(0);
            for (index, value) in matrix.iter().enumerate() {
                let value = if !equal && ordinal == 1 && index == 0 {
                    0.25
                } else {
                    *value
                };
                body.extend_from_slice(&value.to_le_bytes());
            }
        }
        body.extend_from_slice(&1.0f32.to_le_bytes());
        body.extend_from_slice(&[1, 1]);
        body.extend_from_slice(&[0; 9]);
        lp_utf16(&mut body, "11111111-2222-4333-8444-555555555555");
        lp_utf16(&mut body, "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C");
        body.extend_from_slice(&[0; 4]);
        lp_utf16(&mut body, "PrismMaterial-018");
        body.extend_from_slice(
            &old[mesh_body::FEATURE_SCOPE_REFERENCE..mesh_body::CONTAINER_GUID_REFERENCE + 11],
        );
        body.push(0);
        body.extend_from_slice(&old[mesh_body::SCENE_NODE_REFERENCE..mesh_body::LEN]);
        lp_utf16(&mut body, "Synthetic");
        body.extend_from_slice(&[0; 4]);
        body.extend_from_slice(&1.0f32.to_le_bytes());
        body.extend_from_slice(&[1, 1]);
        body.extend_from_slice(&[0; 9]);
        lp_utf16(&mut body, "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE");
        lp_utf16(&mut body, "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C");
        body.extend_from_slice(&[0; 4]);
        lp_utf16(&mut body, "PrismMaterial-018_physmat_aspects:Prism-256");
        body.extend_from_slice(&old[old.len() - 11..]);
        let expected = body.clone();
        let delta = i64::try_from(body.len()).unwrap() - i64::try_from(end - start).unwrap();
        graph.bytes.splice(start..end, body);
        for record in &mut graph.meta.records {
            if record.bulk_offset > u64_from_index(start) {
                record.bulk_offset = record.bulk_offset.checked_add_signed(delta).unwrap();
            }
        }
        let parsed = parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut no_texture_asset,
        );
        if !equal {
            assert!(parsed.is_err());
            continue;
        }
        let features = parsed.unwrap();
        let [feature] = features.as_slice() else {
            panic!("one mesh collection");
        };
        let wire = serde_json::to_value(feature).unwrap();
        let row = &wire["bodies"][0];
        assert_eq!(
            row["body_variable_layout"]["presence_offsets"],
            serde_json::json!([37, 166])
        );
        assert_eq!(
            row["transform_offsets"],
            serde_json::json!([start + 38, start + 167])
        );
        assert_eq!(row["scope_reference_offset"], start + 504);
        assert_eq!(row["scene_node_reference_offset"], start + 549);
        assert_eq!(row["body_words"]["body_word"], 1);
        assert_eq!(
            row["transform"],
            serde_json::json!([
                [0.2, 0.0, 0.0, 1.0],
                [0.0, 0.3, 0.0, 2.0],
                [0.0, 0.0, 0.4, 3.0],
                [0.0, 0.0, 0.0, 1.0]
            ])
        );
        let decoded: crate::records::mesh::DesignMeshFeature =
            serde_json::from_value(wire).unwrap();
        assert_eq!(
            decoded.bodies()[0]
                .placement
                .native_frame()
                .unwrap()
                .as_ref(),
            expected
        );
        assert_eq!(*feature, decoded);
    }
}

#[test]
fn variable_mesh_body_reads_matrix_flags_and_material_lengths() {
    for (compact, null_scope) in [(false, false), (true, false), (false, true)] {
        let mut graph = synthetic_mesh_graph(false);
        let frame = sole_typed_frame(&graph, MESH_BODY_TYPE_GUID);
        let start = frame.start;
        let old_end = frame.end;
        let old = &graph.bytes[start..old_end];
        let mut body = old[..mesh_body::FIRST_PRESENCE].to_vec();
        body[20] = 1;
        body.splice(21..21, [0; 4]);
        for offset in [mesh_body::FIRST_TRANSFORM, mesh_body::SECOND_TRANSFORM] {
            if compact {
                body.push(1);
            } else {
                body.push(0);
                body.extend_from_slice(&old[offset..offset + MATRIX_BYTES]);
            }
        }
        body.extend_from_slice(&1.0_f32.to_le_bytes());
        body.extend_from_slice(&[1, 1]);
        body.extend_from_slice(&[0; 9]);
        lp_utf16(&mut body, "11111111-2222-4333-8444-555555555555");
        lp_utf16(&mut body, "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C");
        body.extend_from_slice(&[0; 4]);
        lp_utf16(
            &mut body,
            if compact {
                "PrismMaterial-018"
            } else {
                "PrismMaterial-1018"
            },
        );
        if null_scope {
            body.push(0);
            body.extend_from_slice(
                &old[mesh_body::WRAPPER_REFERENCE
                    ..mesh_body::CONTAINER_GUID_REFERENCE + SAME_SEGMENT_REFERENCE_BYTES],
            );
            body.extend_from_slice(
                &old[mesh_body::FEATURE_SCOPE_REFERENCE..mesh_body::WRAPPER_REFERENCE],
            );
        } else {
            body.extend_from_slice(
                &old[mesh_body::FEATURE_SCOPE_REFERENCE
                    ..mesh_body::CONTAINER_GUID_REFERENCE + SAME_SEGMENT_REFERENCE_BYTES],
            );
            body.push(0);
        }
        body.extend_from_slice(&old[mesh_body::SCENE_NODE_REFERENCE..mesh_body::LEN]);
        lp_utf16(&mut body, "Axle");
        body.extend_from_slice(&[0; 4]);
        body.extend_from_slice(&1.0_f32.to_le_bytes());
        body.extend_from_slice(&[1, 1]);
        body.extend_from_slice(&[0; 9]);
        lp_utf16(&mut body, "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE");
        lp_utf16(&mut body, "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C");
        body.extend_from_slice(&[0; 4]);
        lp_utf16(&mut body, "PrismMaterial-018_physmat_aspects:Prism-256");
        body.extend_from_slice(&old[old.len() - SAME_SEGMENT_REFERENCE_BYTES..]);
        let expected = body.clone();
        let delta = i64::try_from(body.len()).unwrap() - i64::try_from(old_end - start).unwrap();
        graph.bytes.splice(start..old_end, body);
        for record in &mut graph.meta.records {
            if record.bulk_offset > u64_from_index(start) {
                record.bulk_offset = record.bulk_offset.checked_add_signed(delta).unwrap();
            }
        }
        let parsed = parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut no_texture_asset,
        );
        if null_scope {
            assert!(parsed.is_err());
            continue;
        }
        let features = parsed.expect("variable mesh body");
        let [feature] = features.as_slice() else {
            panic!("one feature")
        };
        let wire = serde_json::to_value(feature).unwrap();
        let row = &wire["bodies"][0];
        assert_eq!(row["body_name"], "Axle");
        if compact {
            assert!(row.get("transform_offsets").is_none());
            assert_eq!(
                row["body_variable_layout"]["presence_offsets"],
                serde_json::json!([45, 46])
            );
            assert_eq!(row["scope_reference_offset"], start + 256);
            assert_eq!(row["scene_node_reference_offset"], start + 301);
        } else {
            assert_eq!(
                row["transform_offsets"],
                serde_json::json!([start + 46, start + 175])
            );
            assert_eq!(
                row["body_variable_layout"]["presence_offsets"],
                serde_json::json!([45, 174])
            );
            assert_eq!(row["scope_reference_offset"], start + 514);
            assert_eq!(row["scene_node_reference_offset"], start + 559);
        }
        let decoded: crate::records::mesh::DesignMeshFeature =
            serde_json::from_value(wire).unwrap();
        assert_eq!(
            decoded.bodies()[0]
                .placement
                .native_frame()
                .unwrap()
                .as_ref(),
            expected.as_slice()
        );
        assert_eq!(decoded, *feature);
        assert_eq!(
            crate::design::decode::mesh::mesh_body_identity_losses(&features).count(),
            usize::from(compact)
        );
    }
}
