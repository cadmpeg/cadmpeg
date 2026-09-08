// SPDX-License-Identifier: Apache-2.0
//! Legacy-entity dump tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::CatiaCodec;

#[test]
fn native_round_trips_legacy_entity_identity_runs() {
    let mut bytes = Vec::new();
    for entity_id in [1_u32, 4, 9, 12, 13] {
        bytes.push(0xea);
        bytes.extend(entity_id.to_le_bytes());
        bytes.extend([0x81, 0xfd, 0x8c]);
        if entity_id == 4 {
            for (role, selector, value) in [
                ("body", vec![0x80, 4, 0, 0, 0], "#1_ + 2"),
                ("param", vec![0xd1, 8], "(#1_ : #In Real) : Real\n"),
            ] {
                bytes.push(u8::try_from(role.len() + 1).expect("short role"));
                bytes.extend(role.as_bytes());
                bytes.extend(selector);
                bytes.extend(b"\xe8\x00\x12\x01");
                bytes.push(u8::try_from(value.len() + 1).expect("short text"));
                bytes.extend(value.as_bytes());
                bytes.push(0xfe);
            }
        } else if entity_id == 9 {
            bytes.extend([8, b'p', b'a', b'r', b'a', b'm', b'i', b'n', 0x80]);
            bytes.extend(4134_u32.to_le_bytes());
            bytes.extend([0xe8, 0xe4, 0x0b, 0x01]);
            bytes.extend(b"\xfe\x84\x92\x82\x08Boolean\x83");
            bytes.extend(b"\xfe\x84\x92\x82\x96\x83");
            bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 9]);
            bytes.extend(b"\xe8\x00\x12\x01\x07Result\xfe");
            bytes.extend(b"\xfe\x84\x88\x82\xfe\xe6");
            bytes.extend(3.5_f64.to_bits().to_le_bytes());
        } else if entity_id == 12 {
            bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 11]);
            bytes.extend(b"\xe8\x00\x12\x01\x0cResponsible\xfe");
            bytes.extend(b"\xfe\x84\x92\x82\x07String\x83");
            bytes.extend(b"\xfe\x85\x93\x82\xfe\x0cCilas Evans");
        } else if entity_id == 13 {
            bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 12]);
            bytes.extend(b"\xe8\x00\x12\x01\x06Count\xfe");
            bytes.extend(b"\xfe\x84\x92\x82\x08Integer\x83");
            bytes.extend(b"\xfe\x85\x9d\x82\xfe\x8c");
        }
    }
    let catalog_offset = bytes.len();
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");
    bytes.extend(b"\xfe\xfe\xfe");
    let schema_program_offset = bytes.len();
    bytes.extend([0x81, 0x04, b'F', b'o', b'o', 0x84, 0xfe]);
    let schema_footer_offset = bytes.len();
    bytes.extend(b"\x4e\x11\x00\x00\x00DASSAULT-SYSTEMES\x05\x00\x00\x00CATIA");

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.legacy_entity_runs.len(), 1);
    assert_eq!(
        native.legacy_entity_runs[0]
            .identities
            .iter()
            .map(|identity| identity.entity_id)
            .collect::<Vec<_>>(),
        [1, 4, 9, 12, 13]
    );
    assert!(native.legacy_entity_runs[0]
        .identities
        .iter()
        .all(|identity| u8::from(identity.lead) == 0x81));
    assert_eq!(
        native.legacy_entity_runs[0].catalog_offset,
        catalog_offset as u64
    );
    let schema_program = native.legacy_entity_runs[0]
        .schema_program
        .as_ref()
        .expect("complete compact schema program");
    assert_eq!(schema_program.byte_offset, schema_program_offset as u64);
    assert_eq!(
        schema_program.boundary_byte_offset,
        schema_footer_offset as u64
    );
    assert_eq!(
        schema_program.boundary,
        crate::native::CatiaLegacySchemaProgramBoundary::VendorFooter
    );
    assert_eq!(
        schema_program.data,
        [0x81, 0x04, b'F', b'o', b'o', 0x84, 0xfe]
    );
    assert_eq!(schema_program.identifiers.len(), 1);
    assert_eq!(
        schema_program.identifiers[0].byte_offset,
        schema_program_offset as u64 + 1
    );
    assert_eq!(schema_program.identifiers[0].value, "Foo");
    assert_eq!(native.legacy_entity_runs[0].text_fields.len(), 5);
    assert_eq!(
        native.legacy_entity_runs[0]
            .role_selectors
            .iter()
            .map(|role| {
                (
                    role.entity_id,
                    role.name.literal().expect("literal role"),
                    role.encoding,
                    role.selector,
                )
            })
            .collect::<Vec<_>>(),
        [
            (
                4,
                "body",
                crate::native::CatiaLegacyRoleSelectorEncoding::FixedU32,
                4,
            ),
            (
                4,
                "param",
                crate::native::CatiaLegacyRoleSelectorEncoding::Paged,
                9,
            ),
            (
                9,
                "paramin",
                crate::native::CatiaLegacyRoleSelectorEncoding::FixedU32,
                4134,
            ),
            (
                9,
                "name",
                crate::native::CatiaLegacyRoleSelectorEncoding::Paged,
                10,
            ),
            (
                12,
                "name",
                crate::native::CatiaLegacyRoleSelectorEncoding::Paged,
                12,
            ),
            (
                13,
                "name",
                crate::native::CatiaLegacyRoleSelectorEncoding::Paged,
                13,
            ),
        ]
    );
    assert_eq!(native.legacy_entity_runs[0].text_fields[0].entity_id, 4);
    assert_eq!(native.legacy_entity_runs[0].text_fields[0].value, "#1_ + 2");
    assert_eq!(
        native.legacy_entity_runs[0].text_fields[0]
            .role
            .as_ref()
            .map(|role| { (role.name.literal().expect("literal role"), role.selector,) }),
        Some(("body", 4))
    );
    assert_eq!(
        native.legacy_entity_runs[0].text_fields[1]
            .role
            .as_ref()
            .map(|role| { (role.name.literal().expect("literal role"), role.selector,) }),
        Some(("param", 9))
    );
    assert_eq!(native.legacy_entity_runs[0].relations.len(), 1);
    assert_eq!(
        native.legacy_entity_runs[0].relations[0].parameter_entity_id,
        Some(9)
    );
    assert_eq!(
        native.legacy_entity_runs[0].relations[0].inputs[0].parameter,
        "#1_"
    );
    assert_eq!(native.legacy_entity_runs[0].type_descriptors.len(), 4);
    assert_eq!(
        native.legacy_entity_runs[0].type_descriptors[0].value,
        crate::native::CatiaLegacyTypeValue::Name {
            value: "Boolean".to_string()
        }
    );
    assert_eq!(
        native.legacy_entity_runs[0].type_descriptors[1].value,
        crate::native::CatiaLegacyTypeValue::Selector { value: 22 }
    );
    assert_eq!(
        native.legacy_entity_runs[0].type_descriptors[2].value,
        crate::native::CatiaLegacyTypeValue::Name {
            value: "String".to_string()
        }
    );
    assert_eq!(
        native.legacy_entity_runs[0].type_descriptors[3].value,
        crate::native::CatiaLegacyTypeValue::Name {
            value: "Integer".to_string()
        }
    );
    assert_eq!(native.legacy_entity_runs[0].scalar_values.len(), 1);
    assert_eq!(
        native.legacy_entity_runs[0].scalar_values[0]
            .name
            .as_deref(),
        Some("Result")
    );
    assert_eq!(
        native.legacy_entity_runs[0].scalar_values[0].encoding,
        crate::native::CatiaLegacyScalarEncoding::Named84
    );
    assert!(native.legacy_entity_runs[0].scalar_values[0]
        .id
        .starts_with("catia:legacy:scalar#00000000-"));
    assert!(matches!(
        native.legacy_entity_runs[0].scalar_values[0].evaluation,
        crate::native::CatiaLegacyScalarEvaluation::Value { bits }
            if bits == 3.5_f64.to_bits()
    ));
    assert_eq!(native.legacy_entity_runs[0].string_values.len(), 1);
    assert_eq!(
        native.legacy_entity_runs[0].string_values[0]
            .name
            .as_deref(),
        Some("Responsible")
    );
    assert_eq!(
        native.legacy_entity_runs[0].string_values[0].value,
        "Cilas Evans"
    );
    assert_eq!(native.legacy_entity_runs[0].integer_values.len(), 1);
    assert_eq!(
        native.legacy_entity_runs[0].integer_values[0]
            .name
            .as_deref(),
        Some("Count")
    );
    assert_eq!(native.legacy_entity_runs[0].integer_values[0].value, 11);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store legacy entity run");
    let loaded = crate::native::CatiaNative::load(&namespace).expect("load legacy entity run");
    assert_eq!(loaded.legacy_entity_runs, native.legacy_entity_runs);

    let mut invalid_schema_program = native.clone();
    invalid_schema_program.legacy_entity_runs[0]
        .schema_program
        .as_mut()
        .expect("schema program")
        .data
        .pop();
    let mut invalid_schema_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_schema_program
        .store(&mut invalid_schema_namespace)
        .expect("store invalid schema program");
    assert!(crate::native::CatiaNative::load(&invalid_schema_namespace).is_err());

    let mut invalid_schema_identifier = native.clone();
    invalid_schema_identifier.legacy_entity_runs[0]
        .schema_program
        .as_mut()
        .expect("schema program")
        .identifiers[0]
        .value = "Bar".to_string();
    let mut invalid_identifier_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_schema_identifier
        .store(&mut invalid_identifier_namespace)
        .expect("store invalid schema identifier");
    assert!(crate::native::CatiaNative::load(&invalid_identifier_namespace).is_err());

    let mut invalid_type_name = native.clone();
    invalid_type_name.legacy_entity_runs[0].type_descriptors[0].value =
        crate::native::CatiaLegacyTypeValue::Name {
            value: "1Boolean".to_string(),
        };
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_type_name
        .store(&mut namespace)
        .expect("store invalid legacy type name");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid_name = native.clone();
    invalid_name.legacy_entity_runs[0].scalar_values[0].name = Some("Other".to_string());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_name
        .store(&mut namespace)
        .expect("store invalid legacy scalar name");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid_scalar_id = native.clone();
    invalid_scalar_id.legacy_entity_runs[0].scalar_values[0].id =
        "catia:legacy:scalar#00000000-0".to_string();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_scalar_id
        .store(&mut namespace)
        .expect("store invalid legacy scalar identity");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid_integer = native.clone();
    invalid_integer.legacy_entity_runs[0].integer_values[0].value = -1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_integer
        .store(&mut namespace)
        .expect("store invalid inline legacy integer");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid_parameter = native.clone();
    invalid_parameter.legacy_entity_runs[0].relations[0].parameter_entity_id = Some(4);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_parameter
        .store(&mut namespace)
        .expect("store invalid legacy relation parameter");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid = native;
    invalid.legacy_entity_runs[0].identities[1].entity_id = 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store invalid legacy entity run");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn legacy_parameters_retain_and_require_the_part_container_binding() {
    let graph = object_graph_stream();
    let legacy_offset = graph.len();
    let mut stream = graph;
    stream.push(0xea);
    stream.extend(1_u32.to_le_bytes());
    stream.push(0x81);
    stream.extend([0xfd, 0x8c]);
    stream.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    stream.extend(b"\xe8\x00\x12\x01");
    stream.extend([6, b'W', b'i', b'd', b't', b'h', 0xfe]);
    stream.extend(b"\xfe\x84\x92\x82");
    stream.extend([7, b'L', b'E', b'N', b'G', b'T', b'H', 0x83]);
    stream.extend(b"\xfe\x84\x88\x82\xfe\xe6");
    stream.extend(12.5_f64.to_bits().to_le_bytes());
    stream.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");
    let (bytes, stream_offset) = outer_container_catpart(&stream);

    let native = crate::native::CatiaNative::decode(&bytes);
    let run = native
        .legacy_entity_runs
        .iter()
        .find(|run| run.byte_offset == stream_offset + legacy_offset as u64)
        .expect("declared-stream legacy run");
    assert_eq!(
        run.outer_container.as_ref(),
        native.object_graphs[0].outer_container.as_ref()
    );
    let expected_binding = run.outer_container.clone();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store container-bound legacy run");
    let loaded =
        crate::native::CatiaNative::load(&namespace).expect("load container-bound legacy run");
    assert_eq!(
        loaded.legacy_entity_runs[0].outer_container,
        expected_binding
    );

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode container-bound legacy parameter");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_PARAMETER_COUNT),
        1
    );
    assert_eq!(decoded.ir().model.parameters.len(), 1);
}

#[test]
fn identity_lead_wire_admits_only_defined_bytes() {
    for lead in u8::MIN..=u8::MAX {
        let wire = serde_json::json!({"byte_offset": 0, "entity_id": 1, "lead": lead});
        let identity =
            serde_json::from_value::<crate::native::CatiaLegacyEntityIdentity>(wire.clone());
        assert_eq!(identity.is_ok(), matches!(lead, 0x81 | 0x82 | 0xe5 | 0xfd));
        match identity {
            Ok(identity) => assert_eq!(serde_json::to_value(identity).unwrap(), wire),
            Err(error) => assert!(error.to_string().contains("lead")),
        }
    }
    let missing = serde_json::json!({"byte_offset": 0, "entity_id": 1});
    assert!(
        serde_json::from_value::<crate::native::CatiaLegacyEntityIdentity>(missing)
            .unwrap_err()
            .to_string()
            .contains("lead")
    );
}
