// SPDX-License-Identifier: Apache-2.0
//! Writer-domain synthetic tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::write::TargetRequest;
use std::io::{Cursor, Read};

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::F3dCodec;

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
    crate::test_support::plan_inherited_write(
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        .plan(
            EncodeInput::new(&inconsistent, None),
            TargetRequest::Inherit,
        )
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
        Err(cadmpeg_ir::DecodeFailure::Codec(
            cadmpeg_core::CodecError::Malformed(message)
        ))
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
            Err(cadmpeg_ir::DecodeFailure::Codec(
                cadmpeg_core::CodecError::Malformed(message)
            ))
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
    crate::test_support::plan_inherited_write(
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
    crate::test_support::plan_inherited_write(
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
    let error = crate::test_support::plan_inherited_write(&modified, &fidelity, &mut Vec::new())
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
            .plan(EncodeInput::new(&invalid, None), TargetRequest::Inherit)
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
            .plan(EncodeInput::new(&invalid, None), TargetRequest::Inherit)
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
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut retained)
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
        symmetries: Vec::new(),
        source_object: None,
    });

    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("open face cannot be emitted as a solid body");
    assert!(matches!(error, cadmpeg_core::CodecError::InvalidInput(_)));
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less circle-carrier encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less circle-carrier round trip");
    let mut round_trip = cadmpeg_test_support::EditableDecodeResult::from(round_trip);
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
    let error = crate::test_support::plan_inherited_write(
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
    crate::test_support::plan_inherited_write(&retained, &fidelity, &mut regenerated)
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
    crate::test_support::plan_inherited_write(&edited, decoded.source_fidelity(), &mut regenerated)
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

    let error = crate::test_support::plan_inherited_write(
        &edited,
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .expect_err("native plane record cannot silently retain a sphere edit");
    assert!(error
        .to_string()
        .contains("does not support edits to surface"));
}
