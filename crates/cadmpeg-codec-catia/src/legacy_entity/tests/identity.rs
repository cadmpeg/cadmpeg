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
        .all(|identity| identity.lead == 0x81));
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
                Some(crate::native::CatiaLegacyRoleSelectorEncoding::FixedU32),
                4,
            ),
            (
                4,
                "param",
                Some(crate::native::CatiaLegacyRoleSelectorEncoding::Paged),
                9,
            ),
            (
                9,
                "paramin",
                Some(crate::native::CatiaLegacyRoleSelectorEncoding::FixedU32),
                4134,
            ),
            (
                9,
                "name",
                Some(crate::native::CatiaLegacyRoleSelectorEncoding::Paged),
                10,
            ),
            (
                12,
                "name",
                Some(crate::native::CatiaLegacyRoleSelectorEncoding::Paged),
                12,
            ),
            (
                13,
                "name",
                Some(crate::native::CatiaLegacyRoleSelectorEncoding::Paged),
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

    let mut previous_schema_namespace = namespace.clone();
    let mut previous_schema_runs: Vec<crate::native::CatiaLegacyEntityRun> =
        previous_schema_namespace
            .arena_as("legacy_entity_runs")
            .expect("load previous schema-program runs");
    previous_schema_runs[0]
        .schema_program
        .as_mut()
        .expect("schema program")
        .identifiers
        .clear();
    previous_schema_namespace
        .set_arena("legacy_entity_runs", &previous_schema_runs)
        .expect("store previous schema-program runs");
    previous_schema_namespace.set_version(std::num::NonZeroU32::new(221).unwrap());
    let migrated_schema = crate::native::CatiaNative::load(&previous_schema_namespace)
        .expect("migrate schema identifiers");
    assert_eq!(
        migrated_schema.legacy_entity_runs[0]
            .schema_program
            .as_ref()
            .expect("migrated schema program")
            .identifiers,
        schema_program.identifiers
    );

    let mut previous_boundary_namespace = namespace.clone();
    let mut previous_boundary_runs: Vec<crate::native::CatiaLegacyEntityRun> =
        previous_boundary_namespace
            .arena_as("legacy_entity_runs")
            .expect("load previous schema-program boundary");
    previous_boundary_runs[0]
        .schema_program
        .as_mut()
        .expect("schema program")
        .boundary = crate::native::CatiaLegacySchemaProgramBoundary::StreamDirectory;
    previous_boundary_namespace
        .set_arena("legacy_entity_runs", &previous_boundary_runs)
        .expect("store previous schema-program boundary");
    previous_boundary_namespace.set_version(std::num::NonZeroU32::new(222).unwrap());
    let migrated_boundary = crate::native::CatiaNative::load(&previous_boundary_namespace)
        .expect("migrate schema-program boundary");
    assert_eq!(
        migrated_boundary.legacy_entity_runs[0]
            .schema_program
            .as_ref()
            .expect("migrated schema program")
            .boundary,
        crate::native::CatiaLegacySchemaProgramBoundary::VendorFooter
    );

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

    let mut previous_field_namespace = namespace.clone();
    let mut previous_field_runs: Vec<crate::native::CatiaLegacyEntityRun> =
        previous_field_namespace
            .arena_as("legacy_entity_runs")
            .expect("load previous field-binding runs");
    for run in &mut previous_field_runs {
        for role in &mut run.role_selectors {
            role.field_code = None;
        }
        for role in run
            .text_fields
            .iter_mut()
            .filter_map(|field| field.role.as_mut())
        {
            role.field_code = None;
        }
    }
    previous_field_namespace
        .set_arena("legacy_entity_runs", &previous_field_runs)
        .expect("store previous field-binding runs");
    previous_field_namespace.set_version(std::num::NonZeroU32::new(219).unwrap());
    let migrated_field_bindings = crate::native::CatiaNative::load(&previous_field_namespace)
        .expect("load previous field bindings");
    assert!(migrated_field_bindings.legacy_entity_runs[0]
        .role_selectors
        .iter()
        .all(|role| role.field_code.is_none()));

    let mut previous_identity_namespace = namespace.clone();
    let mut previous_identity_runs: Vec<crate::native::CatiaLegacyEntityRun> =
        previous_identity_namespace
            .arena_as("legacy_entity_runs")
            .expect("load previous identity runs");
    for identity in previous_identity_runs
        .iter_mut()
        .flat_map(|run| &mut run.identities)
    {
        identity.lead = 0;
    }
    previous_identity_namespace
        .set_arena("legacy_entity_runs", &previous_identity_runs)
        .expect("store previous identity runs");
    previous_identity_namespace.set_version(std::num::NonZeroU32::new(215).unwrap());
    let migrated_identity = crate::native::CatiaNative::load(&previous_identity_namespace)
        .expect("migrate legacy identity leads");
    assert!(migrated_identity.legacy_entity_runs[0]
        .identities
        .iter()
        .all(|identity| identity.lead == 0x81));

    let mut previous_namespace = namespace.clone();
    let mut previous_runs: Vec<crate::native::CatiaLegacyEntityRun> = previous_namespace
        .arena_as("legacy_entity_runs")
        .expect("load legacy entity runs");
    previous_runs[0].role_selectors.clear();
    previous_runs[0].schema_fields.clear();
    for field in &mut previous_runs[0].text_fields {
        if let Some(role) = &mut field.role {
            role.entity_id = 0;
        }
    }
    previous_namespace
        .set_arena("legacy_entity_runs", &previous_runs)
        .expect("store previous legacy entity runs");
    previous_namespace.set_version(std::num::NonZeroU32::new(211).unwrap());
    let migrated =
        crate::native::CatiaNative::load(&previous_namespace).expect("migrate legacy text roles");
    assert_eq!(migrated.legacy_entity_runs[0].role_selectors.len(), 5);
    assert!(migrated.legacy_entity_runs[0]
        .role_selectors
        .iter()
        .all(|role| role.entity_id != 0));

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

    let mut invalid_lead = native.clone();
    invalid_lead.legacy_entity_runs[0].identities[0].lead = 0xe6;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_lead
        .store(&mut namespace)
        .expect("store invalid legacy identity lead");
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
