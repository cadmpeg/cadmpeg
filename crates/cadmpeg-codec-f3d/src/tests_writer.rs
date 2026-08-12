// SPDX-License-Identifier: Apache-2.0
//! Writer-domain synthetic tests and fixtures.

use super::*;

#[test]
fn generated_design_configuration_json_decodes_and_writes_source_less() {
    let name = "FusionAssetName[Active]/DesignConfigurationTable.123.dsgcfg";
    let payload = br#"{"configurations":{"Small":{},"Medium":{"parameters":{"width":"25 mm"},"suppressed":["slot"]},"Large":{}},"active":"Medium","extension":{"future":7}}"#;
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_configuration(
                &synthetic_geometry_smbh(),
                name,
                payload,
            )),
            &DecodeOptions::default(),
        )
        .expect("generated configuration decode");
    let native = f3d_native(decoded.ir());
    assert_eq!(native.design_configurations.len(), 1);
    assert_eq!(native.design_configurations[0].entry_name, name);
    assert_eq!(
        native.design_configurations[0].id,
        format!("f3d:configuration:entry#{name}")
    );
    assert_eq!(
        native.design_configurations[0].kind,
        crate::records::DesignConfigurationKind::Table
    );
    assert_eq!(
        native.design_configurations[0].variant_order,
        ["Small", "Medium", "Large"]
    );
    assert_eq!(native.design_configurations[0].payload["active"], "Medium");
    assert_eq!(
        native.design_configurations[0].payload["extension"]["future"],
        7
    );
    assert_eq!(decoded.ir().model.configurations.len(), 3);
    let mut authored = decoded
        .ir()
        .model
        .configurations
        .iter()
        .filter_map(|configuration| Some((configuration.name.resolved()?, configuration.ordinal)))
        .collect::<Vec<_>>();
    authored.sort_by_key(|(_, ordinal)| *ordinal);
    assert_eq!(authored, [("Small", 0), ("Medium", 1), ("Large", 2)]);
    let medium = decoded
        .ir()
        .model
        .configurations
        .iter()
        .find(|configuration| configuration.name == "Medium")
        .expect("active medium configuration");
    assert!(medium.active.is_active());
    assert_eq!(medium.properties["parameter:width"], "25 mm");
    assert_eq!(medium.properties["suppressed:slot"], "true");
    assert_eq!(
        medium.native_ref.as_deref(),
        Some(native.design_configurations[0].id.as_str())
    );
    let mut invalid_order = decoded.ir().clone();
    update_f3d_native(&mut invalid_order, |native| {
        native.design_configurations[0].variant_order.pop();
    });
    assert!(crate::validate::validate_native(&invalid_order)
        .iter()
        .any(|finding| finding
            .message
            .contains("invalid identity, payload, or variant order")));

    let mut retained = decoded.ir().clone();
    update_f3d_native(&mut retained, |native| {
        native.design_configurations[0].payload["active"] = "Narrow".into();
        native.design_configurations[0].payload["configurations"]["Narrow"] =
            serde_json::json!({"parameters":{"width":"12 mm"},"suppressed":[]});
        native.design_configurations[0]
            .variant_order
            .push("Narrow".into());
    });
    retained.model.configurations = crate::design::configurations::project_configurations(
        &f3d_native(&retained).design_configurations,
    )
    .expect("edited configuration order");
    let expected_retained = f3d_native(&retained).design_configurations;
    let mut retained_bytes = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(
            &retained,
            decoded.source_fidelity(),
            &mut retained_bytes,
        )
        .expect("retained configuration edit");
    let retained_round_trip = F3dCodec
        .decode(&mut Cursor::new(retained_bytes), &DecodeOptions::default())
        .expect("retained configuration round trip");
    assert_eq!(
        f3d_native(retained_round_trip.ir()).design_configurations,
        expected_retained
    );

    let expected_projected = decoded.ir().model.configurations.clone();
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less configuration encode");
    let mut inconsistent = source_less.clone();
    inconsistent
        .model
        .configurations
        .iter_mut()
        .find(|configuration| configuration.name == "Medium")
        .expect("active medium configuration")
        .active = false.into();
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &inconsistent,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("neutral/native configuration divergence must be rejected");
    assert!(error
        .to_string()
        .contains("must equal the projection of native configuration tables"));
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less configuration round trip");
    assert_eq!(
        f3d_native(round_trip.ir()).design_configurations,
        native.design_configurations
    );
    assert_eq!(round_trip.ir().model.configurations, expected_projected);

    let rule_name = "FusionAssetName[Active]/DesignConfigurationRule.456.dsgcfgrule";
    let rule_result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_configuration(
                &synthetic_geometry_smbh(),
                rule_name,
                br#"{"when":"width > 20 mm","activate":"wide"}"#,
            )),
            &DecodeOptions::default(),
        )
        .expect("generated configuration-rule decode");
    assert!(rule_result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(
            "configuration rule(s) were retained without an unambiguous neutral activation target"
        )));
    let rule = f3d_native(rule_result.ir()).design_configurations.remove(0);
    assert_eq!(rule.kind, crate::records::DesignConfigurationKind::Rule);
    assert_eq!(rule.payload["activate"], "wide");

    let invalid = F3dCodec.decode(
        &mut Cursor::new(f3d_with_configuration(
            &synthetic_geometry_smbh(),
            name,
            b"[]",
        )),
        &DecodeOptions::default(),
    );
    assert!(matches!(
        invalid,
        Err(cadmpeg_core::CodecError::Malformed(message))
            if message.contains("configuration JSON must be an object")
    ));

    for (payload, expected) in [
        (
            br#"{"configurations":{"wide":{}},"active":"missing"}"#.as_slice(),
            "is not a named variant",
        ),
        (
            br#"{"configurations":{"wide":{"parameters":[]}}}"#.as_slice(),
            "parameters must be an object",
        ),
        (
            br#"{"configurations":{"wide":{"suppressed":[7]}}}"#.as_slice(),
            "suppressed list must contain strings",
        ),
        (
            br#"{"configurations":{"wide":{"material":7}}}"#.as_slice(),
            "material must be a string",
        ),
    ] {
        let invalid = F3dCodec.decode(
            &mut Cursor::new(f3d_with_configuration(
                &synthetic_geometry_smbh(),
                name,
                payload,
            )),
            &DecodeOptions::default(),
        );
        assert!(matches!(
            invalid,
            Err(cadmpeg_core::CodecError::Malformed(message))
                if message.contains(expected)
        ));
    }

    let partial_rule = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_configuration(
                &synthetic_geometry_smbh(),
                rule_name,
                br#"{"when":"width > 20 mm","vendorExtension":7}"#,
            )),
            &DecodeOptions::default(),
        )
        .expect("partial native rule remains decodable");
    assert!(partial_rule.ir().model.configurations.is_empty());
    let partial_native = f3d_native(partial_rule.ir());
    assert_eq!(
        partial_native.design_configurations[0].payload["vendorExtension"],
        7
    );
    assert!(partial_rule
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(
            "configuration rule(s) were retained without an unambiguous neutral activation target"
        )));
}

#[test]
fn generated_f3d_replays_byte_exactly_and_rejects_semantic_edits() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .unwrap();

    let mut replayed = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut replayed,
        )
        .unwrap();
    assert_eq!(replayed, source);

    let mut point_edited = decoded.ir().clone();
    point_edited.model.points[0].position.x += 12.5;
    let cadmpeg_ir::geometry::SurfaceGeometry::Plane {
        origin,
        normal,
        u_axis,
    } = &mut point_edited.model.surfaces[0].geometry
    else {
        panic!("generated carrier must be a plane")
    };
    origin.z += 25.0;
    *normal = cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0);
    *u_axis = cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0);
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(
            &point_edited,
            decoded.source_fidelity(),
            &mut regenerated,
        )
        .unwrap();
    assert_ne!(regenerated, source);
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        round_trip.ir().model.points[0].position,
        point_edited.model.points[0].position
    );
    assert_eq!(
        round_trip.ir().model.surfaces[0].geometry,
        point_edited.model.surfaces[0].geometry
    );

    let (mut modified, _, fidelity) = decoded.into_parts();
    modified.model.bodies[0].name = Some("edited".into());
    let error = F3dCodec
        .write_preserved_with_source_fidelity(&modified, &fidelity, &mut Vec::new())
        .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

#[test]
fn generated_source_less_planar_triangle_writes_native_f3d() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    source_less.model.bodies[0].visible = Some(false);
    source_less.model.vertices[0].tolerance = Some(0.025);
    source_less.model.edges[0].tolerance = Some(0.035);
    let tangent_edge = source_less.model.edges[0].id.clone();
    let visible_body = source_less.model.bodies[0].id.clone();
    let tolerant_vertex = source_less.model.vertices[0].id.clone();
    let tolerant_edge = source_less.model.edges[0].id.clone();
    let owner_coedge = source_less.model.coedges[0].id.clone();
    let tolerant_coedge = source_less.model.coedges[1].id.clone();
    {
        let mut native = f3d_native_mut(&mut source_less);
        let metadata = native
            .edge_continuities
            .iter_mut()
            .find(|metadata| metadata.edge == tangent_edge)
            .expect("generated edge continuity");
        metadata.continuity = "tangent".into();
        metadata.sense = cadmpeg_ir::topology::Sense::Reversed;
        native.face_sidedness[0].containment =
            Some(cadmpeg_asm::brep::records::FaceContainment::In);
        native.edge_ownerships[0].owner_coedge = Some(owner_coedge);
        native.tolerant_vertex_tails = vec![cadmpeg_asm::brep::records::TolerantVertexTail {
            id: "f3d:asm:tolerant-vertex-tail#generated".into(),
            vertex: tolerant_vertex,
            record_index: 0,
            leading_tolerances: [-1.0, -1.0],
            trailing_field: Some(0),
            evaluated_unset: false,
        }];
        native.tolerant_edge_tails = vec![cadmpeg_asm::brep::records::TolerantEdgeTail {
            id: "f3d:asm:tolerant-edge-tail#generated".into(),
            edge: tolerant_edge,
            record_index: 0,
            entity_revision: 22800,
            trailing_field: Some(1),
        }];
        native.tolerant_coedge_parameters =
            vec![cadmpeg_asm::brep::records::TolerantCoedgeParameters {
                id: "f3d:asm:tolerant-coedge-parameters#generated".into(),
                coedge: tolerant_coedge,
                record_index: 0,
                parameter_range: [0.25, 0.75],
                extension: cadmpeg_asm::brep::records::TolerantCoedgeExtension::None,
            }];
        native.body_visibilities = vec![crate::records::BodyVisibility {
            id: "f3d:design:body-visibility#generated".into(),
            body: visible_body,
            stream: "FusionAssetName[Active]/Design1/BulkStream.dat".into(),
            byte_offset: 0,
            asm_body_key_offset: 0,
            asm_body_key: 42,
            entity_suffix: 42,
            visible: false,
        }];
    }
    let mut encoded = Vec::new();
    crate::native::reset_load_count();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less F3D encode");
    assert_eq!(crate::native::load_count(), 1);
    let mut archive = zip::ZipArchive::new(Cursor::new(&encoded)).expect("generated F3D ZIP");
    let mut properties = Vec::new();
    archive
        .by_name("Properties.dat")
        .expect("generated Properties.dat")
        .read_to_end(&mut properties)
        .expect("generated properties bytes");
    assert_eq!(properties, 0u32.to_le_bytes());
    let mut smbh = Vec::new();
    archive
        .by_name("FusionAssetName[Active]/Breps.BlobParts/BREP.generated.smbh")
        .expect("generated BREP stream")
        .read_to_end(&mut smbh)
        .expect("generated BREP bytes");
    let record_start = smbh
        .windows(b"\x0d\x09asmheader".len())
        .position(|window| window == b"\x0d\x09asmheader")
        .expect("generated ASM record table");
    let records = cadmpeg_asm::sab::frame(&smbh, record_start, smbh.len(), 8)
        .expect("generated ASM records must frame");
    let point_records = records
        .iter()
        .filter(|record| record.head == "point")
        .collect::<Vec<_>>();
    assert_eq!(point_records.len(), 3);
    assert!(point_records
        .iter()
        .all(|record| record.len == 60 && record.tokens.len() == 4));
    assert_eq!(
        records
            .iter()
            .filter(|record| record.head == "tcoedge")
            .count(),
        1
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| record.head == "tedge")
            .count(),
        1
    );
    drop(archive);
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less F3D round trip");

    {
        let mut invalid = source_less.clone();
        f3d_native_mut(&mut invalid).face_sidedness[0].normalized_sense =
            match source_less.model.faces[0].sense {
                cadmpeg_ir::topology::Sense::Forward => cadmpeg_ir::topology::Sense::Reversed,
                cadmpeg_ir::topology::Sense::Reversed => cadmpeg_ir::topology::Sense::Forward,
            };
        let error = F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &invalid,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut Vec::new()))
            .expect_err("stale normalized face sense must not be rewritten");
        assert!(error
            .to_string()
            .contains("normalized sense conflicts with face"));
    }
    {
        let mut invalid = source_less.clone();
        f3d_native_mut(&mut invalid).body_visibilities[0].asm_body_key = 43;
        let error = F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &invalid,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut Vec::new()))
            .expect_err("visibility must rejoin the emitted ASM body");
        assert!(error
            .to_string()
            .contains("uses an ASM key different from body"));
    }

    assert_eq!(round_trip.ir().model.bodies.len(), 1);
    assert_eq!(
        f3d_native(round_trip.ir()).body_native_keys[0].asm_body_key,
        Some(42)
    );
    assert_eq!(round_trip.ir().model.bodies[0].visible, Some(false));
    assert_eq!(f3d_native(round_trip.ir()).body_visibilities.len(), 1);
    assert!(!f3d_native(round_trip.ir()).body_visibilities[0].visible);
    assert_eq!(
        f3d_native(round_trip.ir()).body_visibilities[0].id,
        "f3d:FusionAssetName[Active]/Breps.BlobParts/BREP.generated.smbh:body-visibility#42"
    );
    assert_eq!(
        round_trip.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    assert_eq!(round_trip.ir().model.faces.len(), 1);
    assert_eq!(round_trip.ir().model.loops.len(), 1);
    assert_eq!(round_trip.ir().model.coedges.len(), 3);
    assert_eq!(round_trip.ir().model.edges.len(), 3);
    assert_eq!(round_trip.ir().model.vertices.len(), 3);
    assert_eq!(round_trip.ir().model.vertices[0].tolerance, Some(0.025));
    assert_eq!(round_trip.ir().model.edges[0].tolerance, Some(0.035));
    assert_eq!(
        f3d_native(round_trip.ir()).tolerant_edge_tails[0].entity_revision,
        22800
    );
    assert_eq!(
        f3d_native(round_trip.ir()).tolerant_edge_tails[0].trailing_field,
        Some(1)
    );
    assert_eq!(
        f3d_native(round_trip.ir()).tolerant_vertex_tails[0].leading_tolerances,
        [-1.0, -1.0]
    );
    assert_eq!(
        f3d_native(round_trip.ir()).tolerant_coedge_parameters[0].parameter_range,
        [0.25, 0.75]
    );
    let ownerships = f3d_native(round_trip.ir()).vertex_ownerships;
    assert_eq!(ownerships.len(), 3);
    assert_eq!(
        ownerships
            .iter()
            .map(|metadata| metadata.endpoint_index)
            .collect::<Vec<_>>(),
        [0, 1, 0]
    );
    let continuities = f3d_native(round_trip.ir()).edge_continuities;
    assert_eq!(continuities.len(), 3);
    assert_eq!(continuities[0].continuity, "tangent");
    assert_eq!(continuities[0].sense, cadmpeg_ir::topology::Sense::Reversed);
    assert_eq!(
        f3d_native(round_trip.ir()).edge_ownerships[0].owner_coedge,
        Some(round_trip.ir().model.coedges[0].id.clone())
    );
    assert!(continuities[1..]
        .iter()
        .all(|metadata| metadata.continuity == "unknown"));
    assert_eq!(
        f3d_native(round_trip.ir()).face_sidedness[0].containment,
        Some(cadmpeg_asm::brep::records::FaceContainment::In)
    );
    assert_eq!(round_trip.ir().model.points, source_less.model.points);
    assert_eq!(round_trip.ir().model.surfaces, source_less.model.surfaces);

    let (mut edited, _, fidelity) = round_trip.into_parts();
    edited.model.bodies[0].visible = Some(true);
    edited.model.vertices[0].tolerance = Some(0.05);
    edited.model.edges[0].tolerance = Some(0.06);
    {
        let mut native = f3d_native_mut(&mut edited);
        native.body_native_keys[0].asm_body_key = Some(84);
        native.face_sidedness[0].containment =
            Some(cadmpeg_asm::brep::records::FaceContainment::Out);
        native.tolerant_vertex_tails[0].leading_tolerances = [3.5, -4.5];
    }
    let mut retained = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut retained)
        .expect("retained double-sided containment edit");
    let retained = F3dCodec
        .decode(&mut Cursor::new(retained), &DecodeOptions::default())
        .expect("retained double-sided containment round trip");
    assert_eq!(
        f3d_native(retained.ir()).face_sidedness[0].containment,
        Some(cadmpeg_asm::brep::records::FaceContainment::Out)
    );
    assert_eq!(retained.ir().model.vertices[0].tolerance, Some(0.05));
    assert_eq!(retained.ir().model.edges[0].tolerance, Some(0.06));
    assert_eq!(
        f3d_native(retained.ir()).tolerant_edge_tails[0].entity_revision,
        22800
    );
    assert_eq!(
        f3d_native(retained.ir()).tolerant_edge_tails[0].trailing_field,
        Some(1)
    );
    assert_eq!(retained.ir().model.bodies[0].visible, Some(true));
    assert_eq!(
        f3d_native(retained.ir()).body_native_keys[0].asm_body_key,
        Some(84)
    );
    assert_eq!(
        f3d_native(retained.ir()).body_visibilities[0].asm_body_key,
        84
    );
    assert!(f3d_native(retained.ir()).body_visibilities[0].visible);
    assert_eq!(
        f3d_native(retained.ir()).tolerant_vertex_tails[0].leading_tolerances,
        [3.5, -4.5]
    );
}

#[test]
fn tolerant_edge_and_vertex_tails_round_trip_all_trailing_forms() {
    // The tedge tail carries the serializer revision stamp, then a trailing
    // LONG present only in newer streams and taking the values 0, 1, and 2.
    // The tvertex trailing LONG shares the gate and the value set. Each form
    // must survive a write/decode cycle byte-for-byte.
    for (edge_trailing, vertex_trailing) in [
        (Some(0), Some(0)),
        (Some(1), Some(1)),
        (Some(2), Some(2)),
        (None, None),
    ] {
        let source = f3d_with_smbh(&synthetic_geometry_smbh());
        let decoded = F3dCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("generated planar triangle decode");
        let (mut source_less, _, _) = decoded.into_parts();
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less.model.vertices[0].tolerance = Some(0.025);
        source_less.model.edges[0].tolerance = Some(0.035);
        let tolerant_vertex = source_less.model.vertices[0].id.clone();
        let tolerant_edge = source_less.model.edges[0].id.clone();
        {
            let mut native = f3d_native_mut(&mut source_less);
            native.tolerant_vertex_tails = vec![cadmpeg_asm::brep::records::TolerantVertexTail {
                id: "f3d:asm:tolerant-vertex-tail#generated".into(),
                vertex: tolerant_vertex,
                record_index: 0,
                leading_tolerances: [-1.0, -1.0],
                trailing_field: vertex_trailing,
                evaluated_unset: false,
            }];
            native.tolerant_edge_tails = vec![cadmpeg_asm::brep::records::TolerantEdgeTail {
                id: "f3d:asm:tolerant-edge-tail#generated".into(),
                edge: tolerant_edge,
                record_index: 0,
                entity_revision: 22800,
                trailing_field: edge_trailing,
            }];
        }
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("tolerant tail encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("tolerant tail round trip");
        assert_eq!(
            f3d_native(round_trip.ir()).tolerant_edge_tails[0].entity_revision,
            22800
        );
        assert_eq!(
            f3d_native(round_trip.ir()).tolerant_edge_tails[0].trailing_field,
            edge_trailing
        );
        assert_eq!(
            f3d_native(round_trip.ir()).tolerant_vertex_tails[0].trailing_field,
            vertex_trailing
        );
    }
}

#[test]
fn an_unset_tolerant_vertex_sentinel_round_trips_without_a_neutral_tolerance() {
    // The `-1` unset evaluated slot is a marker rather than a length: the
    // neutral vertex carries no tolerance, the native tail keeps the unset
    // fact, and generation writes the sentinel back into a tvertex record.
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let tolerant_vertex = source_less.model.vertices[0].id.clone();
    assert_eq!(source_less.model.vertices[0].tolerance, None);
    {
        let mut native = f3d_native_mut(&mut source_less);
        native.tolerant_vertex_tails = vec![cadmpeg_asm::brep::records::TolerantVertexTail {
            id: "f3d:asm:tolerant-vertex-tail#generated".into(),
            vertex: tolerant_vertex,
            record_index: 0,
            leading_tolerances: [-1.0, -1.0],
            trailing_field: Some(0),
            evaluated_unset: true,
        }];
    }
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("unset tolerant vertex encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("unset tolerant vertex round trip");
    let vertex = round_trip
        .ir()
        .model
        .vertices
        .iter()
        .find(|vertex| {
            f3d_native(round_trip.ir())
                .tolerant_vertex_tails
                .iter()
                .any(|tail| tail.vertex == vertex.id)
        })
        .expect("tolerant vertex survives");
    assert_eq!(vertex.tolerance, None);
    let tail = &f3d_native(round_trip.ir()).tolerant_vertex_tails[0];
    assert!(tail.evaluated_unset);
    assert_eq!(tail.leading_tolerances, [-1.0, -1.0]);
}

#[test]
fn generated_source_less_f3d_rejects_subds() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    source_less.model.subds.push(cadmpeg_ir::SubdSurface {
        id: cadmpeg_ir::ids::SubdId("test:f3d:subd#0".into()),
        scheme: cadmpeg_ir::SubdScheme::CatmullClark,
        vertices: Vec::new(),
        edges: Vec::new(),
        faces: Vec::new(),
        source_object: None,
    });

    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::NotImplemented(message)
            if message.contains("does not support SubD surfaces")
    ));
}

#[test]
fn generated_source_less_f3d_rejects_unbacked_design_parameters() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    source_less
        .model
        .parameters
        .push(cadmpeg_ir::features::DesignParameter {
            id: cadmpeg_ir::features::ParameterId("test:f3d:parameter#0".into()),
            owner: None,
            ordinal: 0,
            name: "Width".into(),
            expression: "60 mm".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Length(
                cadmpeg_ir::features::Length(60.0),
            )),
            dependencies: Vec::new(),
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });

    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::Malformed(message)
            if message.contains("must equal the projection")
    ));
}

#[test]
fn generated_source_less_f3d_writes_document_design_parameters() {
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let native_id = format!("f3d:{stream}:design-parameter#0");
    f3d_native_mut(&mut source_less)
        .design_parameters
        .push(crate::records::DesignParameter {
            id: native_id.clone(),
            byte_offset: 0,
            class_tag: "305".into(),
            record_index: 700,
            family_discriminator: Some(0),
            family_discriminator_offset: Some(22),
            source_ordinal: 0,
            owner_record_index: None,
            expression: "Width / 2".into(),
            expression_offset: 36,
            source_kind: "User Parameter".into(),
            source_kind_offset: 70,
            kind: crate::records::DesignParameterKind::User,
            unit: Some("mm".into()),
            unit_offset: Some(110),
            name: "HalfWidth".into(),
            name_offset: 120,
            evaluated_value: 3.0,
            evaluated_value_offset: 150,
        });
    f3d_native_mut(&mut source_less)
        .design_parameters
        .push(crate::records::DesignParameter {
            id: format!("f3d:{stream}:design-parameter#1"),
            byte_offset: 0,
            class_tag: "305".into(),
            record_index: 701,
            family_discriminator: Some(0),
            family_discriminator_offset: Some(22),
            source_ordinal: 1,
            owner_record_index: None,
            expression: "60 mm".into(),
            expression_offset: 36,
            source_kind: "User Parameter".into(),
            source_kind_offset: 70,
            kind: crate::records::DesignParameterKind::User,
            unit: Some("mm".into()),
            unit_offset: Some(110),
            name: "Width".into(),
            name_offset: 120,
            evaluated_value: 6.0,
            evaluated_value_offset: 150,
        });
    let (_, parameters) = crate::design::feature_project::project_parameter_design(
        &f3d_native(&source_less).design_parameters,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    source_less.model.parameters = parameters;

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less document parameter encode");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less document parameter round trip");
    let mut round_trip_parameters = decoded.ir().model.parameters.clone();
    let mut expected_parameters = source_less.model.parameters.clone();
    for parameter in &mut round_trip_parameters {
        parameter.native_ref = None;
    }
    for parameter in &mut expected_parameters {
        parameter.native_ref = None;
    }
    assert_eq!(round_trip_parameters, expected_parameters);
    assert_eq!(f3d_native(decoded.ir()).design_parameters.len(), 2);
    assert_eq!(
        decoded.ir().model.parameters[0].dependencies,
        [cadmpeg_ir::features::ParameterId(format!(
            "f3d:model:parameter#{}:f3d%3A{stream}701",
            "f3d%3A".len() + stream.len(),
        ))]
    );
    assert_eq!(
        f3d_native(decoded.ir()).design_parameters[0].evaluated_value,
        3.0
    );
}

#[test]
fn generated_source_less_writes_document_tolerance_contract() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    source_less.tolerances.linear = 2.5e-7;
    source_less.tolerances.angular = 4.0e-11;

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less tolerance encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less tolerance round trip");
    assert_eq!(round_trip.ir().tolerances, source_less.tolerances);
}

#[test]
fn generated_source_less_preserves_supported_topology_tolerances_or_refuses_loss() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();

    source_less.model.faces[0].tolerance = Some(0.02);
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("face tolerance must not disappear");
    assert!(
        error.to_string().contains("cannot serialize face")
            && error.to_string().contains("tolerance losslessly")
    );

    source_less.model.faces[0].tolerance = None;
    source_less.model.edges[0].tolerance = Some(0.03);
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("supported tolerant edge encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("supported tolerant edge round trip");
    assert_eq!(round_trip.ir().model.edges[0].tolerance, Some(0.03));

    source_less.model.edges[0].tolerance = None;
    source_less.model.vertices[0].tolerance = Some(0.04);
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("supported tolerant vertex encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("supported tolerant vertex round trip");
    assert_eq!(round_trip.ir().model.vertices[0].tolerance, Some(0.04));
}

#[test]
fn generated_source_less_refuses_auxiliary_geometry_and_source_identity_loss() {
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::tessellation::Tessellation;
    use cadmpeg_ir::SourceObjectAssociation;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let association = SourceObjectAssociation {
        format: "generated".into(),
        object_id: "object-1".into(),
        name: Some("exact carrier".into()),
        color: None,
        visible: Some(true),
        layer: None,
        instance_path: Vec::new(),
    };

    source_less.model.surfaces[0].source_object = Some(association.clone());
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("surface source identity must not disappear");
    assert!(error
        .to_string()
        .contains("source-object association on surface"));

    source_less.model.surfaces[0].source_object = None;
    source_less.model.curves.push(cadmpeg_ir::geometry::Curve {
        id: "generated:associated-curve#0".into(),
        geometry: cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: Some(association),
    });
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("curve source identity must not disappear");
    assert!(error
        .to_string()
        .contains("source-object association on curve"));

    source_less.model.curves.pop();
    source_less.model.tessellations.push(Tessellation {
        id: "generated:tessellation#0".into(),
        source_object: None,
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        vertices: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        triangles: vec![[0, 1, 2]],
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals: Vec::new(),
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    });
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("neutral tessellation must not disappear");
    assert!(error
        .to_string()
        .contains("cannot serialize neutral tessellation"));
}

#[test]
fn generated_source_less_rejects_body_kind_that_conflicts_with_incidence() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    assert_eq!(
        source_less.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    source_less.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Solid;

    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("open face cannot be emitted as a solid body");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn generated_source_less_planar_polygon_plans_dynamic_record_indices() {
    use cadmpeg_ir::ids::{CoedgeId, EdgeId, PointId, VertexId};

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();

    let point_id = PointId("generated:point#3".into());
    source_less.model.points.push(cadmpeg_ir::topology::Point {
        id: point_id.clone(),
        position: cadmpeg_ir::math::Point3::new(10.0, 10.0, 0.0),
        source_object: None,
    });
    let vertex_id = VertexId("generated:vertex#3".into());
    source_less
        .model
        .vertices
        .push(cadmpeg_ir::topology::Vertex {
            id: vertex_id.clone(),
            point: point_id,
            tolerance: None,
        });
    let first_vertex = source_less.model.edges[0].start.clone();
    source_less.model.edges[2].end = vertex_id.clone();
    let edge_id = EdgeId("generated:edge#3".into());
    source_less.model.edges.push(cadmpeg_ir::topology::Edge {
        id: edge_id.clone(),
        curve: None,
        start: vertex_id,
        end: first_vertex,
        param_range: Some([0.0, 1.0]),
        tolerance: None,
    });
    let coedge_id = CoedgeId("generated:coedge#3".into());
    let loop_id = source_less.model.loops[0].id.clone();
    source_less
        .model
        .coedges
        .push(cadmpeg_ir::topology::Coedge {
            id: coedge_id.clone(),
            owner_loop: loop_id,
            edge: edge_id,
            next: coedge_id.clone(),
            previous: coedge_id.clone(),
            radial_next: coedge_id.clone(),
            sense: cadmpeg_ir::topology::Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
            use_curve_parameter_range: None,
        });
    source_less.model.loops[0].coedges.push(coedge_id);
    let ring = source_less.model.loops[0].coedges.clone();
    for (index, id) in ring.iter().enumerate() {
        let coedge = source_less
            .model
            .coedges
            .iter_mut()
            .find(|coedge| coedge.id == *id)
            .unwrap();
        coedge.next = ring[(index + 1) % ring.len()].clone();
        coedge.previous = ring[(index + ring.len() - 1) % ring.len()].clone();
    }

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less polygon encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less polygon round trip");

    assert_eq!(round_trip.ir().model.coedges.len(), 4);
    assert_eq!(round_trip.ir().model.edges.len(), 4);
    assert_eq!(round_trip.ir().model.vertices.len(), 4);
    assert_eq!(round_trip.ir().model.points.len(), 4);
    assert_eq!(
        round_trip
            .ir()
            .model
            .points
            .iter()
            .map(|point| point.position)
            .collect::<Vec<_>>(),
        source_less
            .model
            .points
            .iter()
            .map(|point| point.position)
            .collect::<Vec<_>>()
    );
}

#[test]
fn generated_source_less_planar_face_writes_straight_edge_carriers() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry};
    use cadmpeg_ir::ids::CurveId;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();

    for index in 0..source_less.model.edges.len() {
        let edge = &source_less.model.edges[index];
        let start = source_less
            .model
            .vertices
            .iter()
            .find(|vertex| vertex.id == edge.start)
            .and_then(|vertex| {
                source_less
                    .model
                    .points
                    .iter()
                    .find(|point| point.id == vertex.point)
            })
            .unwrap()
            .position;
        let end = source_less
            .model
            .vertices
            .iter()
            .find(|vertex| vertex.id == edge.end)
            .and_then(|vertex| {
                source_less
                    .model
                    .points
                    .iter()
                    .find(|point| point.id == vertex.point)
            })
            .unwrap()
            .position;
        let delta =
            cadmpeg_ir::math::Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
        let length = delta.norm();
        let direction =
            cadmpeg_ir::math::Vector3::new(delta.x / length, delta.y / length, delta.z / length);
        let id = CurveId(format!("generated:curve#{index}"));
        source_less.model.curves.push(Curve {
            id: id.clone(),
            geometry: CurveGeometry::Line {
                origin: start,
                direction,
            },
            source_object: None,
        });
        source_less.model.edges[index].curve = Some(id);
        source_less.model.edges[index].param_range = Some([0.0, length]);
    }

    let expected = source_less
        .model
        .curves
        .iter()
        .map(|curve| curve.geometry.clone())
        .collect::<Vec<_>>();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less line-carrier encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less line-carrier round trip");
    assert_eq!(round_trip.ir().model.curves.len(), expected.len());
    for (actual, expected) in round_trip.ir().model.curves.iter().zip(expected) {
        let (
            CurveGeometry::Line {
                origin: actual_origin,
                direction: actual_direction,
            },
            CurveGeometry::Line {
                origin: expected_origin,
                direction: expected_direction,
            },
        ) = (&actual.geometry, expected)
        else {
            panic!("expected line carriers")
        };
        assert_eq!(*actual_origin, expected_origin);
        assert!((actual_direction.x - expected_direction.x).abs() < 1e-14);
        assert!((actual_direction.y - expected_direction.y).abs() < 1e-14);
        assert!((actual_direction.z - expected_direction.z).abs() < 1e-14);
    }
    assert!(round_trip
        .ir()
        .model
        .edges
        .iter()
        .all(|edge| edge.curve.is_some()));
}

#[test]
fn generated_source_less_planar_face_writes_circle_edge_carrier() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry};
    use cadmpeg_ir::ids::CurveId;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let curve_id = CurveId("generated:circle#0".into());
    let expected = CurveGeometry::Circle {
        center: cadmpeg_ir::math::Point3::new(4.0, -2.0, 0.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        radius: 6.5,
    };
    source_less.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: expected.clone(),
        source_object: None,
    });
    source_less.model.edges[0].curve = Some(curve_id);
    source_less.model.edges[0].param_range = Some([0.25, 1.75]);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less circle-carrier encode");
    let mut round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less circle-carrier round trip");
    assert_eq!(round_trip.ir().model.curves[0].geometry, expected);
    assert_eq!(
        round_trip.ir().model.edges[0].param_range,
        Some([0.25, 1.75])
    );
    assert!(round_trip.ir().model.edges[0].curve.is_some());
    assert!(
        !cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new())
            .findings
            .iter()
            .any(|finding| finding.check == cadmpeg_ir::Check::Annotations)
    );
    round_trip.ir_mut().model.curves[0].geometry = CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
    };
    let error = F3dCodec
        .write_preserved_with_source_fidelity(
            round_trip.ir(),
            round_trip.source_fidelity(),
            &mut Vec::new(),
        )
        .expect_err("native ellipse record cannot silently retain a line edit");
    assert!(error
        .to_string()
        .contains("does not support edits to curve"));
}

#[test]
fn generated_source_less_planar_face_writes_ellipse_edge_carrier() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry};
    use cadmpeg_ir::ids::CurveId;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let curve_id = CurveId("generated:ellipse#0".into());
    let expected = CurveGeometry::Ellipse {
        center: cadmpeg_ir::math::Point3::new(-3.0, 5.0, 0.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        major_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        major_radius: 8.0,
        minor_radius: 2.0,
    };
    source_less.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: expected.clone(),
        source_object: None,
    });
    source_less.model.edges[0].curve = Some(curve_id);
    source_less.model.edges[0].param_range = Some([0.5, 2.0]);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less ellipse-carrier encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less ellipse-carrier round trip");
    assert_eq!(round_trip.ir().model.curves[0].geometry, expected);
    assert_eq!(round_trip.ir().model.edges[0].param_range, Some([0.5, 2.0]));
    assert!(
        !cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new())
            .findings
            .iter()
            .any(|finding| finding.check == cadmpeg_ir::Check::Annotations)
    );
}

#[test]
fn generated_source_less_face_writes_cylinder_surface_carrier() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = SurfaceGeometry::Cylinder {
        origin: cadmpeg_ir::math::Point3::new(2.0, -4.0, 6.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        radius: 7.5,
    };
    source_less.model.surfaces[0].geometry = expected.clone();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less cylinder encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cylinder round trip");
    assert_eq!(round_trip.ir().model.surfaces[0].geometry, expected);
}

#[test]
fn generated_source_less_closed_cylinder_band_keeps_compact_periodic_topology() {
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
        VertexId,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::{
        Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
    };

    let mut source_less = CadIr::empty(Default::default());
    let body = BodyId("synthetic:cylinder-band:body#0".into());
    let region = RegionId("synthetic:cylinder-band:region#0".into());
    let shell = ShellId("synthetic:cylinder-band:shell#0".into());
    let face = FaceId("synthetic:cylinder-band:face#0".into());
    let surface = SurfaceId("synthetic:cylinder-band:surface#0".into());
    let loops = [
        LoopId("synthetic:cylinder-band:loop#bottom".into()),
        LoopId("synthetic:cylinder-band:loop#top".into()),
    ];
    let coedges = [
        CoedgeId("synthetic:cylinder-band:coedge#bottom".into()),
        CoedgeId("synthetic:cylinder-band:coedge#top".into()),
    ];
    let edges = [
        EdgeId("synthetic:cylinder-band:edge#bottom".into()),
        EdgeId("synthetic:cylinder-band:edge#top".into()),
    ];
    let curves = [
        CurveId("synthetic:cylinder-band:curve#bottom".into()),
        CurveId("synthetic:cylinder-band:curve#top".into()),
    ];
    let vertices = [
        VertexId("synthetic:cylinder-band:vertex#bottom".into()),
        VertexId("synthetic:cylinder-band:vertex#top".into()),
    ];
    let points = [
        PointId("synthetic:cylinder-band:point#bottom".into()),
        PointId("synthetic:cylinder-band:point#top".into()),
    ];

    source_less.model.bodies.push(Body {
        id: body.clone(),
        kind: BodyKind::Sheet,
        regions: vec![region.clone()],
        transform: None,
        name: Some("closed cylinder band".into()),
        color: None,
        visible: None,
    });
    source_less.model.regions.push(Region {
        id: region.clone(),
        body,
        shells: vec![shell.clone()],
    });
    source_less.model.shells.push(Shell {
        id: shell.clone(),
        region,
        faces: vec![face.clone()],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    source_less.model.faces.push(Face {
        id: face.clone(),
        shell,
        surface: surface.clone(),
        sense: Sense::Forward,
        loops: loops.to_vec(),
        name: None,
        color: None,
        tolerance: None,
    });
    source_less.model.surfaces.push(Surface {
        id: surface,
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 5.0,
        },
        source_object: None,
    });
    for index in 0..2 {
        let z = index as f64 * 10.0;
        source_less.model.loops.push(Loop {
            id: loops[index].clone(),
            face: face.clone(),
            coedges: vec![coedges[index].clone()],
            boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
            vertex_uses: Vec::new(),
        });
        source_less.model.coedges.push(Coedge {
            id: coedges[index].clone(),
            owner_loop: loops[index].clone(),
            edge: edges[index].clone(),
            next: coedges[index].clone(),
            previous: coedges[index].clone(),
            radial_next: coedges[index].clone(),
            sense: if index == 0 {
                Sense::Forward
            } else {
                Sense::Reversed
            },
            pcurves: Vec::new(),
            use_curve: None,
            use_curve_parameter_range: None,
        });
        source_less.model.edges.push(Edge {
            id: edges[index].clone(),
            curve: Some(curves[index].clone()),
            start: vertices[index].clone(),
            end: vertices[index].clone(),
            param_range: Some([-std::f64::consts::PI, std::f64::consts::PI]),
            tolerance: None,
        });
        source_less.model.curves.push(Curve {
            id: curves[index].clone(),
            geometry: CurveGeometry::Circle {
                center: Point3::new(0.0, 0.0, z),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 5.0,
            },
            source_object: None,
        });
        source_less.model.vertices.push(Vertex {
            id: vertices[index].clone(),
            point: points[index].clone(),
            tolerance: None,
        });
        source_less.model.points.push(Point {
            id: points[index].clone(),
            position: Point3::new(-5.0, 0.0, z),
            source_object: None,
        });
    }
    source_less.finalize();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less closed cylinder band encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less closed cylinder band round trip");

    assert_eq!(round_trip.ir().model.faces.len(), 1);
    assert_eq!(round_trip.ir().model.loops.len(), 2);
    assert_eq!(round_trip.ir().model.coedges.len(), 2);
    assert_eq!(round_trip.ir().model.edges.len(), 2);
    assert!(
        round_trip.ir().model.edges.iter().all(|edge| {
            edge.start == edge.end
                && edge.param_range.is_some_and(|range| {
                    (range[0] + std::f64::consts::PI).abs() < 1.0e-12
                        && (range[1] - std::f64::consts::PI).abs() < 1.0e-12
                })
        }),
        "{:?}",
        round_trip.ir().model.edges
    );
    assert!(round_trip.ir().model.loops.iter().all(|loop_| {
        loop_.coedges.len() == 1
            && round_trip
                .ir()
                .model
                .coedges
                .iter()
                .find(|coedge| coedge.id == loop_.coedges[0])
                .is_some_and(|coedge| {
                    coedge.next == coedge.id
                        && coedge.previous == coedge.id
                        && coedge.radial_next == coedge.id
                })
    }));
}

#[test]
fn generated_source_less_face_writes_signed_sphere_surface_carrier() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = SurfaceGeometry::Sphere {
        center: cadmpeg_ir::math::Point3::new(-2.0, 4.0, 8.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        radius: -3.5,
    };
    source_less.model.surfaces[0].geometry = expected.clone();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less sphere encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less sphere round trip");
    assert_eq!(round_trip.ir().model.surfaces[0].geometry, expected);
}

#[test]
fn generated_source_less_face_writes_cone_surface_carrier() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = SurfaceGeometry::Cone {
        origin: cadmpeg_ir::math::Point3::new(1.0, 3.0, -5.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        radius: 9.0,
        ratio: 1.0,
        half_angle: 0.5,
    };
    source_less.model.surfaces[0].geometry = expected.clone();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less cone encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cone round trip");
    assert_eq!(round_trip.ir().model.surfaces[0].geometry, expected);
}

#[test]
fn generated_f3d_rewrites_cone_ratio_and_half_angle() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    source_less.model.surfaces[0].geometry = SurfaceGeometry::Cone {
        origin: cadmpeg_ir::math::Point3::new(1.0, 3.0, -5.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        radius: 9.0,
        ratio: 0.6,
        half_angle: 0.5,
    };

    let mut initial = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut initial))
        .expect("source-less cone encode");
    let retained_decode = F3dCodec
        .decode(&mut Cursor::new(initial), &DecodeOptions::default())
        .expect("generated cone decode");
    let (mut retained, _, fidelity) = retained_decode.into_parts();
    let SurfaceGeometry::Cone {
        ratio, half_angle, ..
    } = &mut retained.model.surfaces[0].geometry
    else {
        panic!("expected cone")
    };
    *ratio = 0.4;
    *half_angle = 0.35;

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&retained, &fidelity, &mut regenerated)
        .expect("cone ratio regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated cone decode");
    assert!(matches!(
        round_trip.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Cone {
            ratio: 0.4,
            half_angle,
            ..
        } if (half_angle - 0.35).abs() < 1.0e-12
    ));
}

#[test]
fn generated_f3d_rewrites_plane_frame() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated planar triangle decode");
    let mut edited = decoded.ir().clone();
    let expected = SurfaceGeometry::Plane {
        origin: cadmpeg_ir::math::Point3::new(10.0, -20.0, 30.0),
        normal: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
    };
    edited.model.surfaces[0].geometry = expected.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, decoded.source_fidelity(), &mut regenerated)
        .expect("plane frame regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated plane decode");
    assert_eq!(round_trip.ir().model.surfaces[0].geometry, expected);
}

#[test]
fn generated_f3d_rejects_analytic_surface_family_changes() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated planar triangle decode");
    let mut edited = decoded.ir().clone();
    edited.model.surfaces[0].geometry = SurfaceGeometry::Sphere {
        center: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        radius: 5.0,
    };

    let error = F3dCodec
        .write_preserved_with_source_fidelity(&edited, decoded.source_fidelity(), &mut Vec::new())
        .expect_err("native plane record cannot silently retain a sphere edit");
    assert!(error
        .to_string()
        .contains("does not support edits to surface"));
}

#[test]
fn generated_source_less_face_writes_signed_torus_surface_carrier() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = SurfaceGeometry::Torus {
        center: cadmpeg_ir::math::Point3::new(3.0, -6.0, 9.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        major_radius: 4.5,
        minor_radius: -6.0,
    };
    source_less.model.surfaces[0].geometry = expected.clone();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less torus encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less torus round trip");
    assert_eq!(round_trip.ir().model.surfaces[0].geometry, expected);
}

#[test]
fn generated_source_less_face_writes_nurbs_surface_carrier() {
    use cadmpeg_ir::geometry::{NurbsSurface, SurfaceGeometry};

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![-1.0, -1.0, 2.0, 2.0],
        v_knots: vec![-2.0, -2.0, 3.0, 3.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Point3::new(0.0, 10.0, 2.0),
            cadmpeg_ir::math::Point3::new(20.0, 0.0, 3.0),
            cadmpeg_ir::math::Point3::new(20.0, 10.0, 4.0),
        ],
        weights: None,
        u_periodic: true,
        v_periodic: false,
    });
    source_less.model.surfaces[0].geometry = expected.clone();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less NURBS surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less NURBS surface round trip");
    assert_eq!(round_trip.ir().model.surfaces[0].geometry, expected);
}

#[test]
fn generated_source_less_face_writes_rational_nurbs_surface_carrier() {
    use cadmpeg_ir::geometry::{NurbsSurface, SurfaceGeometry};

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 8.0, 1.0),
            cadmpeg_ir::math::Point3::new(12.0, 0.0, 2.0),
            cadmpeg_ir::math::Point3::new(12.0, 8.0, 3.0),
        ],
        weights: Some(vec![1.0, 0.75, 1.25, 1.0]),
        u_periodic: false,
        v_periodic: true,
    });
    source_less.model.surfaces[0].geometry = expected.clone();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less rational NURBS surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less rational NURBS surface round trip");
    assert_eq!(round_trip.ir().model.surfaces[0].geometry, expected);
}

#[test]
fn generated_source_less_face_writes_rational_nurbs_edge_curve() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, NurbsCurve};
    use cadmpeg_ir::ids::CurveId;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let curve_id = CurveId("generated:nurbs_curve#0".into());
    let expected = CurveGeometry::Nurbs(NurbsCurve {
        degree: 2,
        knots: vec![-1.0, -1.0, -1.0, 2.0, 2.0, 2.0],
        control_points: vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(5.0, 8.0, 1.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 2.0),
        ],
        weights: Some(vec![1.0, 0.6, 1.0]),
        periodic: true,
    });
    source_less.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: expected.clone(),
        source_object: None,
    });
    source_less.model.edges[0].curve = Some(curve_id);
    source_less.model.edges[0].param_range = Some([-1.0, 2.0]);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less rational NURBS curve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less rational NURBS curve round trip");
    assert_eq!(round_trip.ir().model.curves[0].geometry, expected);
    assert_eq!(
        round_trip.ir().model.edges[0].param_range,
        Some([-1.0, 2.0])
    );
}

#[test]
fn generated_source_less_face_writes_inline_nurbs_pcurve() {
    let source = f3d_with_smbh(&synthetic_geometry_with_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated inline pcurve decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = source_less.model.pcurves[0].clone();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less inline pcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less inline pcurve round trip");
    assert_eq!(round_trip.ir().model.pcurves.len(), 1);
    assert_eq!(round_trip.ir().model.pcurves[0].geometry, expected.geometry);
    assert_eq!(
        round_trip.ir().model.pcurves[0].wrapper_reversed,
        expected.wrapper_reversed
    );
    assert_eq!(
        round_trip.ir().model.pcurves[0].native_tail_flags,
        expected.native_tail_flags
    );
    assert_eq!(
        round_trip.ir().model.pcurves[0].parameter_range,
        expected.parameter_range
    );
    assert_eq!(
        round_trip.ir().model.pcurves[0].fit_tolerance,
        expected.fit_tolerance
    );
    assert_eq!(
        round_trip
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    let pcurve_coedge = round_trip
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| !coedge.pcurves.is_empty())
        .expect("generated coedge with pcurve");
    assert!(pcurve_coedge
        .pcurves
        .first()
        .is_some_and(|use_| use_.parameter_range.is_some()));
    assert!(crate::validate::validate_native(round_trip.ir()).is_empty());
}

#[test]
fn generated_source_less_face_lowers_line_pcurve_exactly() {
    use cadmpeg_ir::geometry::PcurveGeometry;
    use cadmpeg_ir::math::Point2;

    let source = f3d_with_smbh(&synthetic_geometry_with_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated inline pcurve decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let pcurve = &mut source_less.model.pcurves[0];
    pcurve.geometry = PcurveGeometry::Line {
        origin: Point2::new(2.0, -1.0),
        direction: Point2::new(0.5, 2.0),
    };
    pcurve.parameter_range = Some([-2.0, 3.0]);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less line pcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less line pcurve round trip");
    assert_eq!(
        round_trip.ir().model.pcurves[0].parameter_range,
        Some([-2.0, 3.0])
    );
    assert_eq!(
        round_trip.ir().model.pcurves[0].geometry,
        PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![-2.0, -2.0, 3.0, 3.0],
            control_points: vec![Point2::new(1.0, -5.0), Point2::new(3.5, 5.0)],
            weights: None,
            periodic: false,
        }
    );
}

#[test]
fn generated_source_less_face_writes_rational_nurbs_pcurve() {
    let source = f3d_with_smbh(&synthetic_geometry_with_rational_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated rational pcurve decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = source_less.model.pcurves[0].clone();
    assert!(matches!(
        &expected.geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
            weights: Some(weights),
            ..
        } if weights == &vec![1.0, 0.5]
    ));

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less rational pcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less rational pcurve round trip");
    assert_eq!(round_trip.ir().model.pcurves.len(), 1);
    let actual = &round_trip.ir().model.pcurves[0];
    assert_eq!(actual.geometry, expected.geometry);
    assert_eq!(actual.wrapper_reversed, expected.wrapper_reversed);
    assert_eq!(actual.native_tail_flags, expected.native_tail_flags);
    assert_eq!(actual.parameter_range, expected.parameter_range);
    assert_eq!(actual.fit_tolerance, expected.fit_tolerance);
}

#[test]
fn generated_source_less_two_faces_preserve_shared_radial_edge() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SurfaceGeometry};
    use cadmpeg_ir::ids::CurveId;

    let source = f3d_with_smbh(&synthetic_mixed_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated shared-edge decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected_surface = SurfaceGeometry::Cylinder {
        origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        radius: 5.0,
    };
    source_less.model.surfaces[1].geometry = expected_surface.clone();
    let curve_id = CurveId("generated:shared_line#0".into());
    let expected_curve = CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
    };
    source_less.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: expected_curve.clone(),
        source_object: None,
    });
    source_less.model.edges[0].curve = Some(curve_id);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less shared-edge encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less shared-edge round trip");
    assert_eq!(round_trip.ir().model.faces.len(), 2);
    assert_eq!(round_trip.ir().model.loops.len(), 2);
    assert_eq!(round_trip.ir().model.coedges.len(), 6);
    assert_eq!(round_trip.ir().model.edges.len(), 5);
    assert_eq!(round_trip.ir().model.vertices.len(), 4);
    assert_eq!(round_trip.ir().model.surfaces.len(), 2);
    assert_eq!(round_trip.ir().model.surfaces[1].geometry, expected_surface);
    assert_eq!(round_trip.ir().model.curves[0].geometry, expected_curve);
    assert!(round_trip.ir().model.edges[0].curve.is_some());
    let shared = round_trip
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| {
            round_trip
                .ir()
                .model
                .coedges
                .iter()
                .filter(|coedge| coedge.edge == edge.id)
                .count()
                == 2
        })
        .expect("shared radial edge");
    let radial = round_trip
        .ir()
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.edge == shared.id)
        .collect::<Vec<_>>();
    assert_eq!(radial.len(), 2);
    assert_eq!(radial[0].radial_next, radial[1].id);
    assert_eq!(radial[1].radial_next, radial[0].id);
}

#[test]
fn generated_source_less_face_preserves_multiple_loop_chain() {
    use cadmpeg_ir::ids::{CoedgeId, EdgeId, LoopId, PointId, VertexId};

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();

    let loop_id = LoopId("generated:loop#1".into());
    let mut coedge_ids = Vec::new();
    let coordinates = [[2.0, 2.0, 0.0], [4.0, 2.0, 0.0], [2.0, 4.0, 0.0]];
    for (index, [x, y, z]) in coordinates.into_iter().enumerate() {
        let point_id = PointId(format!("generated:inner_point#{index}"));
        source_less.model.points.push(cadmpeg_ir::topology::Point {
            id: point_id.clone(),
            position: cadmpeg_ir::math::Point3::new(x, y, z),
            source_object: None,
        });
        let vertex_id = VertexId(format!("generated:inner_vertex#{index}"));
        source_less
            .model
            .vertices
            .push(cadmpeg_ir::topology::Vertex {
                id: vertex_id,
                point: point_id,
                tolerance: None,
            });
    }
    let inner_vertices = source_less.model.vertices[3..]
        .iter()
        .map(|vertex| vertex.id.clone())
        .collect::<Vec<_>>();
    for index in 0..3 {
        let edge_id = EdgeId(format!("generated:inner_edge#{index}"));
        source_less.model.edges.push(cadmpeg_ir::topology::Edge {
            id: edge_id.clone(),
            curve: None,
            start: inner_vertices[index].clone(),
            end: inner_vertices[(index + 1) % 3].clone(),
            param_range: Some([0.0, 1.0]),
            tolerance: None,
        });
        let coedge_id = CoedgeId(format!("generated:inner_coedge#{index}"));
        coedge_ids.push(coedge_id.clone());
        source_less
            .model
            .coedges
            .push(cadmpeg_ir::topology::Coedge {
                id: coedge_id.clone(),
                owner_loop: loop_id.clone(),
                edge: edge_id,
                next: coedge_id.clone(),
                previous: coedge_id.clone(),
                radial_next: coedge_id,
                sense: cadmpeg_ir::topology::Sense::Reversed,
                pcurves: Vec::new(),
                use_curve: None,
                use_curve_parameter_range: None,
            });
    }
    for index in 0..3 {
        let coedge = source_less
            .model
            .coedges
            .iter_mut()
            .find(|coedge| coedge.id == coedge_ids[index])
            .unwrap();
        coedge.next = coedge_ids[(index + 1) % 3].clone();
        coedge.previous = coedge_ids[(index + 2) % 3].clone();
    }
    let face_id = source_less.model.faces[0].id.clone();
    source_less.model.loops.push(cadmpeg_ir::topology::Loop {
        id: loop_id.clone(),
        face: face_id,
        boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
        coedges: coedge_ids,
        vertex_uses: Vec::new(),
    });
    source_less.model.faces[0].loops.push(loop_id);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less multiple-loop encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multiple-loop round trip");
    assert_eq!(round_trip.ir().model.faces.len(), 1);
    assert_eq!(round_trip.ir().model.loops.len(), 2);
    assert_eq!(round_trip.ir().model.faces[0].loops.len(), 2);
    assert_eq!(round_trip.ir().model.coedges.len(), 6);
    assert_eq!(round_trip.ir().model.edges.len(), 6);
}

#[test]
fn generated_source_less_multi_face_writes_nurbs_carriers_and_pcurve() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, NurbsCurve, NurbsSurface, SurfaceGeometry};
    use cadmpeg_ir::ids::{CurveId, PcurveId};

    let source = f3d_with_smbh(&synthetic_mixed_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated shared-edge decode");
    let pcurve_source = f3d_with_smbh(&synthetic_geometry_with_pcurve_smbh());
    let pcurve = F3dCodec
        .decode(&mut Cursor::new(pcurve_source), &DecodeOptions::default())
        .expect("generated pcurve decode")
        .into_parts()
        .0
        .model
        .pcurves
        .into_iter()
        .next()
        .expect("generated pcurve");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();

    let expected_surface = SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 10.0, 1.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 2.0),
            cadmpeg_ir::math::Point3::new(10.0, 10.0, 3.0),
        ],
        weights: Some(vec![1.0, 0.8, 1.2, 1.0]),
        u_periodic: false,
        v_periodic: true,
    });
    source_less.model.surfaces[1].geometry = expected_surface.clone();
    let curve_id = CurveId("generated:shared_nurbs#0".into());
    let expected_curve = CurveGeometry::Nurbs(NurbsCurve {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(5.0, 3.0, 1.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        weights: Some(vec![1.0, 0.7, 1.0]),
        periodic: false,
    });
    source_less.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: expected_curve.clone(),
        source_object: None,
    });
    source_less.model.edges[0].curve = Some(curve_id);
    let pcurve_id = PcurveId("generated:pcurve#0".into());
    let mut pcurve = pcurve;
    pcurve.id = pcurve_id.clone();
    let expected_pcurve = pcurve.geometry.clone();
    source_less.model.pcurves.push(pcurve);
    source_less.model.coedges[0].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
        pcurve: pcurve_id,
        isoparametric: None,
        parameter_range: None,
    }];

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less multi-face NURBS encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multi-face NURBS round trip");
    assert_eq!(round_trip.ir().model.surfaces[1].geometry, expected_surface);
    assert_eq!(round_trip.ir().model.curves[0].geometry, expected_curve);
    assert_eq!(round_trip.ir().model.pcurves[0].geometry, expected_pcurve);
    assert_eq!(
        round_trip
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
}

#[test]
fn generated_source_less_unit_cube_writes_closed_shared_edge_shell() {
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let tolerant_coedge = source_less.model.coedges[7].id.clone();
    f3d_native_mut(&mut source_less).tolerant_coedge_parameters =
        vec![cadmpeg_asm::brep::records::TolerantCoedgeParameters {
            id: "f3d:asm:tolerant-coedge-parameters#cube".into(),
            coedge: tolerant_coedge,
            record_index: 0,
            parameter_range: [-1.5, 2.25],
            extension: cadmpeg_asm::brep::records::TolerantCoedgeExtension::None,
        }];
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less unit cube encode");
    {
        let mut archive = zip::ZipArchive::new(Cursor::new(&encoded)).unwrap();
        let mut stream = Vec::new();
        archive
            .by_name("FusionAssetName[Active]/Breps.BlobParts/BREP.generated.smbh")
            .unwrap()
            .read_to_end(&mut stream)
            .unwrap();
        let records = cadmpeg_asm::sab::frame(&stream, 47, stream.len(), 8).unwrap();
        let tolerant = records
            .iter()
            .find(|record| record.head == "tcoedge")
            .expect("canonical tolerant coedge record");
        assert!(matches!(
            tolerant.chunk(13),
            Some(cadmpeg_asm::sab::Token::Ref(-1))
        ));
        assert!(matches!(
            tolerant.chunk(14),
            Some(cadmpeg_asm::sab::Token::Long(0))
        ));
        assert!(matches!(
            tolerant.chunk(15),
            Some(cadmpeg_asm::sab::Token::Long(0))
        ));
    }
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less unit cube round trip");
    assert_eq!(round_trip.ir().model.bodies.len(), 1);
    assert_eq!(
        round_trip.ir().model.bodies[0].name.as_deref(),
        source_less.model.bodies[0].name.as_deref()
    );
    assert_eq!(
        round_trip.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Solid
    );
    assert_eq!(round_trip.ir().model.regions.len(), 1);
    assert_eq!(round_trip.ir().model.shells.len(), 1);
    assert_eq!(round_trip.ir().model.faces.len(), 6);
    assert_eq!(
        round_trip
            .ir()
            .model
            .faces
            .iter()
            .map(|face| face.name.as_deref())
            .collect::<Vec<_>>(),
        source_less
            .model
            .faces
            .iter()
            .map(|face| face.name.as_deref())
            .collect::<Vec<_>>()
    );
    assert_eq!(round_trip.ir().model.loops.len(), 6);
    assert_eq!(round_trip.ir().model.coedges.len(), 24);
    assert_eq!(round_trip.ir().model.edges.len(), 12);
    assert_eq!(round_trip.ir().model.vertices.len(), 8);
    assert_eq!(round_trip.ir().model.points.len(), 8);
    assert_eq!(
        f3d_native(round_trip.ir()).tolerant_coedge_parameters[0].parameter_range,
        [-1.5, 2.25]
    );
    assert!(round_trip.ir().model.edges.iter().all(|edge| {
        round_trip
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.edge == edge.id)
            .count()
            == 2
    }));
    let report = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
}

#[test]
fn generated_source_less_multi_face_writes_torus_and_circle_carriers() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SurfaceGeometry};
    use cadmpeg_ir::ids::CurveId;

    let source = f3d_with_smbh(&synthetic_mixed_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated shared-edge decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected_surface = SurfaceGeometry::Torus {
        center: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        major_radius: 8.0,
        minor_radius: -3.0,
    };
    source_less.model.surfaces[1].geometry = expected_surface.clone();
    let curve_id = CurveId("generated:shared_circle#0".into());
    let expected_curve = CurveGeometry::Circle {
        center: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        radius: 5.0,
    };
    source_less.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: expected_curve.clone(),
        source_object: None,
    });
    source_less.model.edges[0].curve = Some(curve_id);
    source_less.model.edges[0].param_range = Some([0.25, 1.5]);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less multi-face torus encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multi-face torus round trip");
    assert_eq!(round_trip.ir().model.surfaces[1].geometry, expected_surface);
    assert_eq!(round_trip.ir().model.curves[0].geometry, expected_curve);
    assert_eq!(
        round_trip.ir().model.edges[0].param_range,
        Some([0.25, 1.5])
    );
}

#[test]
fn generated_source_less_multi_face_writes_cone_sphere_and_ellipse_carriers() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SurfaceGeometry};
    use cadmpeg_ir::ids::CurveId;
    use cadmpeg_ir::math::{Point3, Vector3};

    let source = f3d_with_smbh(&synthetic_mixed_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated shared-edge decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let cone = SurfaceGeometry::Cone {
        origin: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 8.0,
        ratio: 1.0,
        half_angle: 0.35,
    };
    let sphere = SurfaceGeometry::Sphere {
        center: Point3::new(-1.0, 4.0, 2.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: -12.0,
    };
    source_less.model.surfaces[0].geometry = cone.clone();
    source_less.model.surfaces[1].geometry = sphere.clone();
    let curve_id = CurveId("generated:shared_ellipse#0".into());
    let ellipse = CurveGeometry::Ellipse {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 9.0,
        minor_radius: 4.0,
    };
    source_less.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: ellipse.clone(),
        source_object: None,
    });
    source_less.model.edges[0].curve = Some(curve_id);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less multi-face analytic encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multi-face analytic round trip");
    assert_eq!(round_trip.ir().model.surfaces[0].geometry, cone);
    assert_eq!(round_trip.ir().model.surfaces[1].geometry, sphere);
    assert_eq!(round_trip.ir().model.curves[0].geometry, ellipse);
}

#[test]
fn generated_source_less_writes_translational_extrusion_definition() {
    let source = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated extrusion decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = source_less.model.procedural_surfaces[0].clone();
    let directrix_id = match &expected.definition {
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion { directrix, .. } => {
            directrix.clone()
        }
        _ => unreachable!(),
    };
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == directrix_id)
        .expect("extrusion directrix")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(5.0, 10.0, -5.0),
        direction: cadmpeg_ir::math::Vector3::new(2.0, -4.0, 1.0),
    };

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less extrusion encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less extrusion round trip");
    assert_eq!(round_trip.ir().model.procedural_surfaces.len(), 1);
    let actual = &round_trip.ir().model.procedural_surfaces[0];
    assert_eq!(actual.definition, expected.definition);
    assert_eq!(actual.cache_fit_tolerance, expected.cache_fit_tolerance);
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion {
        directrix,
        direction,
        parameter_interval,
        native_position,
        revision_form: None,
    } = &actual.definition
    else {
        panic!("expected extrusion definition")
    };
    assert!(round_trip
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *directrix));
    assert!(matches!(
        round_trip
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *directrix)
            .map(|curve| &curve.geometry),
        Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
            if curve.degree == 1
                && curve.knots == [0.25, 0.25, 0.75, 0.75]
                && curve.control_points == [
                    cadmpeg_ir::math::Point3::new(5.5, 9.0, -4.75),
                    cadmpeg_ir::math::Point3::new(6.5, 7.0, -4.25),
                ]
    ));
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 20.0));
    assert_eq!(*parameter_interval, Some([0.25, 0.75]));
    assert_eq!(
        *native_position,
        Some(cadmpeg_ir::math::Point3::new(40.0, 50.0, 60.0))
    );
}

/// The revision-gated `cyl_spl_sur` layout carries the shared surface tail, so
/// the tail's enum, discontinuity arrays, and closing boolean reach the IR and
/// come back byte-identical through source-less generation. The compact layout
/// has no tail and keeps writing the compact record.
#[test]
fn generated_source_less_writes_revision_gated_extrusion_definition() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_versioned_cyl_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("revision-gated extrusion decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = source_less.model.procedural_surfaces[0].clone();
    let ProceduralSurfaceDefinition::Extrusion {
        revision_form: Some(form),
        ..
    } = &expected.definition
    else {
        panic!("expected a revision-gated extrusion")
    };
    assert_eq!(form.revision, 23100);
    assert_eq!(form.flags, [true]);
    assert_eq!(form.tail_enum, 0);
    assert_eq!(form.tail_parameterization, None);
    assert_eq!(
        form.discontinuities,
        expected_revision_surface_tail_discontinuities()
    );
    assert!(!form.tail_flag);
    assert_eq!(expected.cache_fit_tolerance, Some(0.02));

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("revision-gated extrusion encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("revision-gated extrusion round trip");
    let actual = &round_trip.ir().model.procedural_surfaces[0];
    assert_eq!(actual.definition, expected.definition);
    assert_eq!(actual.cache_fit_tolerance, expected.cache_fit_tolerance);

    // The directrix sense Boolean is stored, not assumed: the opposite value
    // survives the same round trip.
    let ProceduralSurfaceDefinition::Extrusion {
        revision_form: Some(form),
        ..
    } = &mut source_less.model.procedural_surfaces[0].definition
    else {
        unreachable!("revision-gated extrusion")
    };
    form.flags = vec![false];
    let expected = source_less.model.procedural_surfaces[0].clone();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("reversed-directrix extrusion encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("reversed-directrix extrusion round trip");
    assert_eq!(
        round_trip.ir().model.procedural_surfaces[0].definition,
        expected.definition
    );
}

/// A form-`2` extrusion stores its parameterization in place of a solved cache.
/// It regenerates from that parameterization, with no cache to draw on.
#[test]
fn generated_source_less_writes_parameterized_extrusion_definition() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_versioned_cyl_spl_sur_with_tail_smbh(2),
            )),
            &DecodeOptions::default(),
        )
        .expect("parameterized extrusion decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = source_less.model.procedural_surfaces[0].clone();
    assert_eq!(expected.cache_fit_tolerance, None);

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("parameterized extrusion encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("parameterized extrusion round trip");
    let actual = &round_trip.ir().model.procedural_surfaces[0];
    assert_eq!(actual.cache_fit_tolerance, None);
    let ProceduralSurfaceDefinition::Extrusion {
        revision_form: Some(form),
        ..
    } = &actual.definition
    else {
        panic!("expected a parameterized revision-gated extrusion")
    };
    assert_eq!(form.tail_enum, 2);
    assert_eq!(
        form.tail_parameterization,
        Some(expected_revision_surface_tail_parameterization())
    );
    assert_eq!(actual.definition, expected.definition);
}

#[test]
fn generated_cacheless_translational_extrusion_retains_exact_construction() {
    use cadmpeg_ir::geometry::{CurveGeometry, ProceduralSurfaceDefinition, SurfaceGeometry};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_cacheless_cyl_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated cache-less extrusion decode");

    assert_eq!(decoded.ir().model.procedural_surfaces.len(), 1);
    let procedural = &decoded.ir().model.procedural_surfaces[0];
    assert_eq!(procedural.cache_fit_tolerance, None);
    let ProceduralSurfaceDefinition::Extrusion {
        directrix,
        direction,
        parameter_interval,
        native_position,
        revision_form: None,
    } = &procedural.definition
    else {
        panic!("expected extrusion definition")
    };
    assert_eq!(*parameter_interval, Some([0.25, 0.75]));
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 20.0));
    assert_eq!(
        *native_position,
        Some(cadmpeg_ir::math::Point3::new(40.0, 50.0, 60.0))
    );
    let directrix_geometry = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *directrix)
        .map(|curve| &curve.geometry);
    assert!(
        matches!(directrix_geometry, Some(CurveGeometry::Nurbs(_))),
        "unexpected extrusion directrix: {directrix_geometry:?}"
    );
    let u = 0.5;
    let v = 0.25;
    let directrix_point =
        cadmpeg_ir::eval::curve_point(directrix_geometry.expect("typed extrusion directrix"), u)
            .expect("directrix evaluation");
    let surface_geometry = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == procedural.surface)
        .map(|surface| &surface.geometry)
        .expect("extrusion surface carrier");
    let surface_point = cadmpeg_ir::eval::model_surface_point(decoded.ir(), surface_geometry, u, v)
        .expect("procedural extrusion evaluation");
    assert_eq!(surface_point.x, directrix_point.x + v * direction.x);
    assert_eq!(surface_point.y, directrix_point.y + v * direction.y);
    assert_eq!(surface_point.z, directrix_point.z + v * direction.z);
    assert!(matches!(
        decoded
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == procedural.surface)
            .map(|surface| &surface.geometry),
        Some(SurfaceGeometry::Procedural { construction }) if *construction == procedural.id
    ));

    let expected_definition = procedural.definition.clone();
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less cache-less extrusion encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cache-less extrusion round trip");
    assert_eq!(round_trip.ir().model.procedural_surfaces.len(), 1);
    assert_eq!(
        round_trip.ir().model.procedural_surfaces[0].definition,
        expected_definition
    );
    assert_eq!(
        round_trip.ir().model.procedural_surfaces[0].cache_fit_tolerance,
        None
    );
    assert!(matches!(
        round_trip
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == round_trip.ir().model.procedural_surfaces[0].surface)
            .map(|surface| &surface.geometry),
        Some(SurfaceGeometry::Procedural { construction })
            if *construction == round_trip.ir().model.procedural_surfaces[0].id
    ));

    source_less.model.procedural_surfaces[0].cache_fit_tolerance = Some(0.01);
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("cache-less extrusion tolerance must be rejected");
    assert!(error
        .to_string()
        .contains("cache-less F3D extrusion cannot carry a cache-fit tolerance"));
}

#[test]
fn generated_cacheless_circle_extrusion_decodes_as_analytic_cylinder() {
    use cadmpeg_ir::geometry::{CurveGeometry, ProceduralSurfaceDefinition, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_cacheless_cyl_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated cache-less extrusion decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let ProceduralSurfaceDefinition::Extrusion {
        directrix,
        parameter_interval,
        direction,
        ..
    } = &mut source_less.model.procedural_surfaces[0].definition
    else {
        panic!("expected extrusion definition")
    };
    *parameter_interval = Some([0.0, std::f64::consts::TAU]);
    *direction = Vector3::new(0.0, 0.0, -20.0);
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == *directrix)
        .expect("extrusion directrix")
        .geometry = CurveGeometry::Circle {
        center: Point3::new(2.0, 3.0, 4.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 5.0,
    };

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less circle extrusion encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less circle extrusion round trip");
    let surface = round_trip
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == round_trip.ir().model.procedural_surfaces[0].surface)
        .expect("extrusion carrier");
    let SurfaceGeometry::Cylinder {
        origin,
        axis,
        ref_direction,
        radius,
    } = surface.geometry
    else {
        panic!("unexpected extrusion carrier: {:?}", surface.geometry)
    };
    assert!((origin.x - 2.0).abs() < 1.0e-12);
    assert!((origin.y - 3.0).abs() < 1.0e-12);
    assert!((origin.z - 4.0).abs() < 1.0e-12);
    assert_eq!(axis, Vector3::new(0.0, 0.0, -1.0));
    assert!((ref_direction.x - 1.0).abs() < 1.0e-12);
    assert!(ref_direction.y.abs() < 1.0e-12);
    assert!(ref_direction.z.abs() < 1.0e-12);
    assert!((radius - 5.0).abs() < 1.0e-12);
}

#[test]
fn generated_source_less_writes_rolling_ball_blend_definition() {
    let source = f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated rolling-ball decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let supports = match &source_less.model.procedural_surfaces[0].definition {
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Blend { supports, .. } => {
            supports.each_ref().map(|support| {
                support
                    .as_ref()
                    .expect("rolling-ball support")
                    .surface
                    .clone()
            })
        }
        _ => panic!("expected rolling-ball definition"),
    };
    let spine = match &source_less.model.procedural_surfaces[0].definition {
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Blend { spine, .. } => {
            spine.clone().expect("rolling-ball spine")
        }
        _ => unreachable!(),
    };
    let support_geometries = [
        cadmpeg_ir::geometry::SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        },
        cadmpeg_ir::geometry::SurfaceGeometry::Sphere {
            center: cadmpeg_ir::math::Point3::new(10.0, -5.0, 2.0),
            axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            radius: 7.5,
        },
    ];
    for (support, geometry) in supports.iter().zip(&support_geometries) {
        source_less
            .model
            .surfaces
            .iter_mut()
            .find(|surface| surface.id == *support)
            .expect("rolling-ball support carrier")
            .geometry = geometry.clone();
    }
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine)
        .expect("rolling-ball spine carrier")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(-2.0, 4.0, 1.0),
        direction: cadmpeg_ir::math::Vector3::new(3.0, -1.0, 2.0),
    };
    let expected = source_less.model.procedural_surfaces[0].clone();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less rolling-ball encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less rolling-ball round trip");
    assert_eq!(round_trip.ir().model.procedural_surfaces.len(), 1);
    let actual = &round_trip.ir().model.procedural_surfaces[0];
    assert_eq!(actual.definition, expected.definition);
    assert_eq!(actual.cache_fit_tolerance, expected.cache_fit_tolerance);
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Blend {
        supports, spine, ..
    } = &actual.definition
    else {
        unreachable!()
    };
    for (support, expected) in supports.iter().zip(support_geometries) {
        let support = support.as_ref().expect("round-trip rolling-ball support");
        let actual = round_trip
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == support.surface)
            .expect("round-trip rolling-ball support carrier");
        assert_eq!(actual.geometry, expected);
    }
    let spine = spine.as_ref().expect("round-trip rolling-ball spine");
    assert!(matches!(
        round_trip
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *spine)
            .map(|curve| &curve.geometry),
        Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
            if curve.degree == 1
                && curve.knots == [0.0, 0.0, 1.0, 1.0]
                && curve.control_points == [
                    cadmpeg_ir::math::Point3::new(-2.0, 4.0, 1.0),
                    cadmpeg_ir::math::Point3::new(1.0, 3.0, 3.0),
                ]
    ));
}

#[test]
fn generated_source_less_unit_cube_writes_body_transform() {
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let expected = cadmpeg_ir::transform::Transform {
        rows: [
            [0.0, -1.0, 0.0, 20.0],
            [1.0, 0.0, 0.0, -30.0],
            [0.0, 0.0, 1.0, 40.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    source_less.model.bodies[0].transform = Some(expected);
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less transformed cube encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less transformed cube round trip");
    assert_eq!(round_trip.ir().model.bodies[0].transform, Some(expected));
    let hints = &f3d_native(round_trip.ir()).transform_hints[0];
    assert!(hints.rotation);
    assert!(!hints.reflection);
    assert!(!hints.shear);
}

#[test]
fn generated_source_less_unit_cube_writes_body_and_face_colors() {
    use cadmpeg_ir::topology::Color;

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let body_color = Color {
        r: 0.1,
        g: 0.2,
        b: 0.3,
        a: 1.0,
    };
    let face_color = Color {
        r: 0.65,
        g: 0.45,
        b: 0.25,
        a: 1.0,
    };
    source_less.model.bodies[0].color = Some(body_color);
    source_less.model.faces[2].color = Some(face_color);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less colored cube encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less colored cube round trip");
    assert_eq!(round_trip.ir().model.bodies[0].color, Some(body_color));
    assert_eq!(round_trip.ir().model.faces[2].color, Some(face_color));
    assert!(round_trip
        .ir()
        .model
        .faces
        .iter()
        .enumerate()
        .all(|(ordinal, face)| ordinal == 2 || face.color.is_none()));
}

#[test]
fn generated_source_less_rejects_translucent_direct_color() {
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    source_less.model.bodies[0].color = Some(cadmpeg_ir::topology::Color {
        r: 0.1,
        g: 0.2,
        b: 0.3,
        a: 0.5,
    });

    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

#[test]
fn generated_source_less_writes_persistent_body_and_sketch_provenance_attributes() {
    use crate::records::{
        CreationTimestamp, PersistentDesignLink, PersistentSubentityTag, SketchCurveLink,
    };
    use cadmpeg_ir::attributes::AttributeTarget;
    use cadmpeg_ir::topology::Color;

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    source_less.model.bodies[0].color = Some(Color {
        r: 0.2,
        g: 0.4,
        b: 0.6,
        a: 1.0,
    });
    source_less.model.faces[0].color = Some(Color {
        r: 0.7,
        g: 0.3,
        b: 0.1,
        a: 1.0,
    });
    let body_id = source_less.model.bodies[0].id.clone();
    let face_id = source_less.model.faces[0].id.clone();
    let edge_id = source_less.model.edges[0].id.clone();
    let coedge_id = source_less.model.coedges[0].id.clone();
    let vertex_id = source_less.model.vertices[0].id.clone();
    let mut native = f3d_native_mut(&mut source_less);
    native.persistent_design_links = vec![
        PersistentDesignLink {
            id: "generated:persistent-design-link#0".into(),
            target: AttributeTarget::Body(body_id.clone()),
            design_id: "311".into(),
            entity_kind: 3,
            design_reference: 7,
            ordinal: 0,
            is_current: false,
        },
        PersistentDesignLink {
            id: "generated:persistent-design-link#1".into(),
            target: AttributeTarget::Body(body_id.clone()),
            design_id: "322".into(),
            entity_kind: 3,
            design_reference: 8,
            ordinal: 1,
            is_current: true,
        },
    ];
    native.persistent_subentity_tags = vec![
        PersistentSubentityTag {
            id: "generated:persistent-subentity-tag#0".into(),
            target: AttributeTarget::Face(face_id.clone()),
            selector: 1,
            token: "8".into(),
            design_references: vec![301, -314, 411],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "generated:persistent-subentity-tag#1".into(),
            target: AttributeTarget::Edge(edge_id.clone()),
            selector: 2,
            token: "-1".into(),
            design_references: vec![511],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "generated:persistent-subentity-tag#2".into(),
            target: AttributeTarget::Face(face_id.clone()),
            selector: 3,
            token: "42".into(),
            design_references: Vec::new(),
            ordinal: 1,
        },
    ];
    native.sketch_curve_links = vec![SketchCurveLink {
        id: "generated:sketch-curve-link#0".into(),
        target: AttributeTarget::Coedge(coedge_id.clone()),
        sketch_curve_id: 113,
        ref_b: 0,
        sense: Some(1),
        role: 2,
        closure: 3,
    }];
    native.creation_timestamps = [
        (AttributeTarget::Body(body_id), 1_579_392_000_000_001.0),
        (AttributeTarget::Face(face_id), 1_579_392_000_000_002.0),
        (AttributeTarget::Edge(edge_id), 1_579_392_000_000_003.0),
        (AttributeTarget::Coedge(coedge_id), 1_579_392_000_000_004.0),
        (AttributeTarget::Vertex(vertex_id), 1_579_392_000_000_005.0),
    ]
    .into_iter()
    .enumerate()
    .map(|(ordinal, (target, unix_microseconds))| CreationTimestamp {
        id: format!("generated:creation-timestamp#{ordinal}"),
        target,
        record_index: 0,
        unix_microseconds,
    })
    .collect();

    drop(native);
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less provenance attribute encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less provenance attribute round trip");
    {
        use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};

        fn suffix_index(value: &str) -> i64 {
            value
                .rsplit_once('#')
                .and_then(|(_, suffix)| suffix.parse().ok())
                .expect("generated native id has a numeric record suffix")
        }

        fn attribute_index(attribute: &SourceAttribute) -> i64 {
            suffix_index(&attribute.id.0)
        }

        fn reference_index(value: &AttributeValue) -> i64 {
            let AttributeValue::Reference(value) = value else {
                panic!("generated attribute link is not a reference");
            };
            suffix_index(value)
        }

        fn target_index(target: &AttributeTarget) -> i64 {
            let id = match target {
                AttributeTarget::Body(id) => id.as_str(),
                AttributeTarget::Face(id) => id.as_str(),
                AttributeTarget::Coedge(id) => id.as_str(),
                AttributeTarget::Edge(id) => id.as_str(),
                AttributeTarget::Vertex(id) => id.as_str(),
                _ => panic!("source-less attribute has an unsupported topology owner"),
            };
            suffix_index(id)
        }

        let attributes = &round_trip.ir().model.attributes;
        assert!(!attributes.is_empty());
        for attribute in attributes {
            assert!(attribute.values.len() >= 5);
            assert_eq!(reference_index(&attribute.values[0]), -1);
            assert_eq!(attribute.values[1], AttributeValue::Integer(-1));
            assert_eq!(
                reference_index(&attribute.values[4]),
                target_index(&attribute.target)
            );

            let index = attribute_index(attribute);
            for (field, reciprocal) in [(2usize, 3usize), (3, 2)] {
                let linked = reference_index(&attribute.values[field]);
                if linked < 0 {
                    continue;
                }
                let linked_attribute = attributes
                    .iter()
                    .find(|candidate| attribute_index(candidate) == linked)
                    .expect("generated attribute link resolves");
                assert_eq!(linked_attribute.target, attribute.target);
                assert_eq!(reference_index(&linked_attribute.values[reciprocal]), index);
            }
        }
        for (ordinal, attribute) in attributes.iter().enumerate() {
            if attributes[..ordinal]
                .iter()
                .any(|before| before.target == attribute.target)
            {
                continue;
            }
            let owned = attributes
                .iter()
                .filter(|candidate| candidate.target == attribute.target);
            assert_eq!(
                owned
                    .clone()
                    .filter(|candidate| reference_index(&candidate.values[3]) == -1)
                    .count(),
                1
            );
            assert_eq!(
                owned
                    .filter(|candidate| reference_index(&candidate.values[2]) == -1)
                    .count(),
                1
            );
        }
    }
    let native = f3d_native(round_trip.ir());
    assert_eq!(native.persistent_design_links.len(), 2);
    assert_eq!(native.persistent_design_links[0].design_id, "311");
    assert_eq!(native.persistent_design_links[0].entity_kind, 3);
    assert_eq!(native.persistent_design_links[0].design_reference, 7);
    assert_eq!(native.persistent_design_links[1].design_id, "322");
    assert_eq!(native.persistent_design_links[1].design_reference, 8);
    assert!(native.persistent_design_links[1].is_current);
    assert_eq!(native.persistent_subentity_tags.len(), 3);
    assert!(native.persistent_subentity_tags.iter().any(|tag| {
        tag.design_references == [301, -314, 411] && matches!(tag.target, AttributeTarget::Face(_))
    }));
    assert!(crate::validate::validate_native(round_trip.ir()).is_empty());
    assert!(native.persistent_subentity_tags.iter().any(|tag| {
        tag.token == "-1"
            && tag.design_references == [511]
            && matches!(tag.target, AttributeTarget::Edge(_))
    }));
    assert!(native.persistent_subentity_tags.iter().any(|tag| {
        tag.token == "42"
            && tag.design_references.is_empty()
            && matches!(tag.target, AttributeTarget::Face(_))
    }));
    assert_eq!(native.sketch_curve_links.len(), 1);
    assert_eq!(native.sketch_curve_links[0].sketch_curve_id, 113);
    assert_eq!(native.sketch_curve_links[0].sense, Some(1));
    assert_eq!(native.sketch_curve_links[0].role, 2);
    assert_eq!(native.sketch_curve_links[0].closure, 3);
    assert_eq!(native.creation_timestamps.len(), 5);
    assert!(native.creation_timestamps.iter().any(|timestamp| {
        matches!(timestamp.target, AttributeTarget::Vertex(_))
            && timestamp.unix_microseconds == 1_579_392_000_000_005.0
    }));
    assert_eq!(
        round_trip.ir().model.bodies[0].color,
        source_less.model.bodies[0].color
    );
    assert_eq!(
        round_trip.ir().model.faces[0].color,
        source_less.model.faces[0].color
    );

    let duplicate = f3d_native(&source_less).creation_timestamps[0].clone();
    f3d_native_mut(&mut source_less)
        .creation_timestamps
        .push(duplicate);
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("duplicate generated timestamp target must be rejected");
    assert!(error
        .to_string()
        .contains("multiple F3D creation timestamps target the same entity"));
}

#[test]
fn generated_source_less_rejects_lossy_design_link_metadata() {
    use crate::records::{PersistentDesignLink, SketchCurveLink};
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let body = source_less.model.bodies[0].id.clone();
    let coedge = source_less.model.coedges[0].id.clone();
    let mut native = f3d_native_mut(&mut source_less);
    native.persistent_design_links = vec![PersistentDesignLink {
        id: "generated:persistent-design-link#0".into(),
        target: AttributeTarget::Body(body),
        design_id: "311".into(),
        entity_kind: 3,
        design_reference: 7,
        ordinal: 1,
        is_current: false,
    }];
    native.sketch_curve_links = [0, 1]
        .map(|ordinal| SketchCurveLink {
            id: format!("generated:sketch-curve-link#{ordinal}"),
            target: AttributeTarget::Coedge(coedge.clone()),
            sketch_curve_id: 113 + ordinal,
            ref_b: 0,
            sense: Some(1),
            role: 2,
            closure: 3,
        })
        .into();
    drop(native);

    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("duplicate sketch links must not be collapsed");
    assert!(error
        .to_string()
        .contains("one sketch-curve link per coedge"));

    f3d_native_mut(&mut source_less).sketch_curve_links.pop();
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("noncanonical persistent link order must not be rewritten");
    assert!(error
        .to_string()
        .contains("contiguous ordinals and only the final link current"));
}

#[test]
fn generated_source_less_rejects_collapsed_native_topology_metadata() {
    use cadmpeg_asm::brep::records::{EdgeContinuity, TolerantVertexTail};

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let edge = source_less.model.edges[0].id.clone();
    let vertex = source_less.model.vertices[0].id.clone();
    {
        let mut native = f3d_native_mut(&mut source_less);
        native.edge_continuities = [0, 1]
            .map(|ordinal| EdgeContinuity {
                id: format!("f3d:asm:edge-continuity#generated-{ordinal}"),
                edge: edge.clone(),
                record_index: ordinal,
                sense: cadmpeg_ir::topology::Sense::Forward,
                continuity: "unknown".into(),
            })
            .into();
    }
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("duplicate edge metadata must not collapse");
    assert!(error
        .to_string()
        .contains("multiple F3D edge-continuity records"));

    {
        let mut native = f3d_native_mut(&mut source_less);
        native.edge_continuities.truncate(1);
        native.tolerant_vertex_tails = vec![TolerantVertexTail {
            id: "f3d:asm:tolerant-vertex-tail#generated".into(),
            vertex,
            record_index: 0,
            leading_tolerances: [1.0, 2.0],
            trailing_field: Some(0),
            evaluated_unset: false,
        }];
    }
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("tolerant metadata on an ordinary vertex must not be dropped");
    assert!(error
        .to_string()
        .contains("requires finite fields and a tolerant vertex"));
}

#[test]
fn generated_source_less_writes_two_independent_cube_bodies() {
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let second_json = source_less
        .to_canonical_json()
        .expect("canonical cube JSON")
        .replace("synthetic:cube:", "synthetic:cube_two:");
    let mut second =
        cadmpeg_ir::document::CadIr::from_json(&second_json).expect("renamed second cube IR");
    second.model.bodies[0].transform = Some(cadmpeg_ir::transform::Transform {
        rows: [
            [1.0, 0.0, 0.0, 30.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    });
    source_less.model.bodies.append(&mut second.model.bodies);
    source_less.model.regions.append(&mut second.model.regions);
    source_less.model.shells.append(&mut second.model.shells);
    source_less.model.faces.append(&mut second.model.faces);
    source_less.model.loops.append(&mut second.model.loops);
    source_less.model.coedges.append(&mut second.model.coedges);
    source_less.model.edges.append(&mut second.model.edges);
    source_less
        .model
        .vertices
        .append(&mut second.model.vertices);
    source_less.model.points.append(&mut second.model.points);
    source_less
        .model
        .surfaces
        .append(&mut second.model.surfaces);
    source_less.model.curves.append(&mut second.model.curves);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less two-body encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less two-body round trip");
    assert_eq!(round_trip.ir().model.bodies.len(), 2);
    assert_eq!(round_trip.ir().model.regions.len(), 2);
    assert_eq!(round_trip.ir().model.shells.len(), 2);
    assert_eq!(round_trip.ir().model.faces.len(), 12);
    assert_eq!(round_trip.ir().model.edges.len(), 24);
    assert_eq!(round_trip.ir().model.points.len(), 16);
    assert_eq!(
        round_trip.ir().model.bodies[1]
            .transform
            .expect("second body transform")
            .rows[0][3],
        30.0
    );
    let report = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
}

#[test]
fn generated_source_less_writes_typed_asm_history_graph() {
    let source = f3d_with_smbh(&synthetic_geometry_with_history_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated history decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = f3d_native(&source_less).asm_histories[0].clone();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less history encode");
    let mut preambleless = source_less.clone();
    {
        let mut native = f3d_native_mut(&mut preambleless);
        native.asm_histories[0].stream_size = None;
        native.asm_histories[0].history_entry_count = None;
    }
    let mut preambleless_bytes = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &preambleless,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut preambleless_bytes))
        .expect("source-less preambleless history encode");
    let preambleless_round_trip = F3dCodec
        .decode(
            &mut Cursor::new(preambleless_bytes),
            &DecodeOptions::default(),
        )
        .expect("source-less preambleless history round trip");
    assert_eq!(
        f3d_native(preambleless_round_trip.ir()).asm_histories[0].stream_size,
        None
    );
    assert_eq!(
        f3d_native(preambleless_round_trip.ir()).asm_histories[0].history_entry_count,
        None
    );
    f3d_native_mut(&mut source_less).asm_histories[0].states[0].bulletin_boards[0].changes[0]
        .kind = crate::history_records::AsmEntityChangeKind::Delete;
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("inconsistent generated history change kind must be rejected");
    assert!(error
        .to_string()
        .contains("kind inconsistent with its references"));
    {
        let mut native = f3d_native_mut(&mut source_less);
        native.asm_histories[0].states[0].bulletin_boards[0].changes[0].kind =
            crate::history_records::AsmEntityChangeKind::Update;
        native.asm_histories[0].stream_size = Some(3);
    }
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("incoherent generated history preamble must be rejected");
    assert!(error
        .to_string()
        .contains("head state_id == stream_size and nonnegative history_entry_count"));
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less history round trip");
    let actual = &f3d_native(round_trip.ir()).asm_histories[0];
    assert_eq!(actual.stream_size, expected.stream_size);
    assert_eq!(actual.history_entry_count, expected.history_entry_count);
    assert_eq!(actual.states.len(), expected.states.len());
    assert_eq!(actual.states[0].state_id, expected.states[0].state_id);
    assert_eq!(actual.states[0].bulletin_boards.len(), 1);
    assert_eq!(actual.states[0].bulletin_boards[0].changes.len(), 2);
    assert_eq!(actual.states[0].records.len(), 1);
    assert_eq!(actual.states[0].records[0].name, "history_payload");
}

#[test]
fn generated_source_less_rejects_lossy_asm_history_graphs() {
    let source = f3d_with_smbh(&synthetic_geometry_with_history_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated history decode");
    let mut orphaned = decoded.ir().clone();
    orphaned.source = None;
    orphaned.set_native_unknowns("f3d", &[]).unwrap();
    let orphan = &mut orphaned
        .native
        .namespace_mut("f3d")
        .arenas
        .get_mut("asm_history_records")
        .expect("history-record arena")[0];
    let mut orphan_fields = orphan.fields();
    orphan_fields.insert("parent".into(), serde_json::json!("missing-state"));
    *orphan = cadmpeg_ir::NativeRecord::new(orphan.id().to_string(), orphan_fields);
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &orphaned,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("orphan history records must not be discarded");
    assert!(error
        .to_string()
        .contains("orphaned or ambiguously parented records"));

    let mut duplicate = decoded.ir().clone();
    duplicate.source = None;
    duplicate.set_native_unknowns("f3d", &[]).unwrap();
    let states = duplicate
        .native
        .namespace_mut("f3d")
        .arenas
        .get_mut("asm_delta_states")
        .expect("delta-state arena");
    states.push(states[0].clone());
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &duplicate,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("duplicate history identities must not multiply children");
    assert!(error
        .to_string()
        .contains("asm_delta_states contains duplicate record ids"));

    let (mut broken_chain, _, _) = decoded.into_parts();
    broken_chain.source = None;
    broken_chain.set_native_unknowns("f3d", &[]).unwrap();
    f3d_native_mut(&mut broken_chain).asm_histories[0].states[0].next_ref = Some(99);
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &broken_chain,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("unresolved history links must be rejected");
    assert!(error
        .to_string()
        .contains("not a coherent doubly linked state chain"));
}
