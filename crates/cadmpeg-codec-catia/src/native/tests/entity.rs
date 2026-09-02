// SPDX-License-Identifier: Apache-2.0
//! Native-namespace tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::Annotations;

use crate::test_support::*;
use crate::CatiaCodec;

#[test]
fn inline_entity_and_object_records_pair_by_extent_and_cardinality() {
    let mut entity = vec![0x7c, 0x05, 12, 0, 0, 0, 0x03, 0xea];
    entity.extend_from_slice(&1_u32.to_le_bytes());
    let graph_offset = entity.len() + 1;
    entity.push(0xde);
    entity.extend(object_graph_from_records(&[inline_object_graph_record(&[
        0x00, 0x90, 0x81, 0x81, 0x00,
    ])]));

    let native = crate::native::CatiaNative::decode(&entity);
    let graph = native
        .object_graphs
        .iter()
        .find(|graph| graph.byte_offset == graph_offset as u64)
        .expect("entity-paired graph");
    assert_eq!(graph.records.len(), 1);
    assert_eq!(graph.records[0].entity_id, Some(1));
    let record = native
        .entity_records
        .iter()
        .find(|record| record.object_graph == graph.id)
        .expect("paired inline entity");
    assert_eq!(
        record.inline_body.as_deref(),
        Some(&[0x03, 0xea, 1, 0, 0, 0][..])
    );
    assert_eq!(record.object_record, graph.records[0].id);
}

#[test]
fn native_namespace_retains_and_validates_definition_schema_selections() {
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])];
    let mut bytes =
        entity_table_record_with_definition_and_value(1, &[0, 0, 0x32, 4, 0, 0, 0], &[]);
    bytes.push(0xde);
    bytes.extend(object_graph_from_records(&records));
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
    ]));

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(
        native.entity_records[0].definition_schema_selections,
        [crate::native::CatiaDefinitionSchemaSelection {
            offset: 2,
            ordinal: 4,
            entry: Some(native.catalogs[0].entries[4].id.clone()),
            name: Some("Sketch".to_string()),
        }]
    );

    let mut malformed = native;
    malformed.entity_records[0].definition_schema_selections[0].name = Some("Pad".to_string());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed definition-schema view");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_retains_and_validates_repeated_reference_suffixes() {
    let payload = [
        0xb0, 0x83, 0x81, 0xbc, 0x81, 0xbe, 0x81, 0xb1, 0x83, 0x81, 0xbc, 0x81, 0xbe, 0xd1, 0x80,
        0xfe,
    ];
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x81], &payload)];
    let native = crate::native::CatiaNative::decode(&entity_backed_object_graph(&records, &[1]));
    let suffix = native.object_graphs[0].records[0]
        .repeated_reference_suffix
        .as_ref()
        .expect("repeated reference suffix");
    assert_eq!(suffix.schema_preamble, None);
    assert_eq!(suffix.repeated_references, [60, 62]);
    assert_eq!(suffix.terminal_reference, 49);

    let mut malformed = native;
    malformed.object_graphs[0].records[0]
        .repeated_reference_suffix
        .as_mut()
        .expect("repeated reference suffix")
        .terminal_reference += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed repeated-reference-suffix view");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_resolves_and_validates_repeated_reference_schema_selections() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_repeated_reference_schema_selection(),
    );
    let selection = native.object_graphs[0].records[0]
        .repeated_reference_schema_selection
        .as_ref()
        .expect("reference schema selection");
    assert_eq!(
        selection.order,
        crate::native::CatiaRepeatedReferenceSchemaOrder::BlobThenSchema
    );
    assert_eq!(selection.ordinal, 4);
    assert_eq!(selection.offset, 67);
    assert_eq!(selection.name.as_deref(), Some("TargetSchema"));
    assert!(selection.entry.is_some());

    let mut malformed = native;
    malformed.object_graphs[0].records[0]
        .repeated_reference_schema_selection
        .as_mut()
        .expect("reference schema selection")
        .name = Some("WrongSchema".to_string());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed reference-schema view");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_retains_and_validates_complete_entity_numeric_pairs() {
    let value = [
        0x91, 0x84, 0xe8, 0xe4, 0x07, 0x37, 0x83, 0x81, 0xe6, 0, 0, 0, 0, 0, 0, 0x12, 0x40, 0xe8,
        0xfe, 0xfe,
    ];
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])];
    let mut bytes = entity_table_record_with_value(1, &value);
    bytes.push(0xde);
    bytes.extend(object_graph_from_records(&records));

    let native = crate::native::CatiaNative::decode(&bytes);
    let pair = native.entity_records[0]
        .numeric_pair
        .as_ref()
        .expect("complete numeric pair");
    assert_eq!(
        pair.slots,
        [
            crate::entity_table::NumericPairSlot::Binary64 {
                bits: 4.5_f64.to_bits(),
                offset: 8,
            },
            crate::entity_table::NumericPairSlot::ControlE8 { offset: 17 },
        ]
    );

    let mut legacy = native.clone();
    legacy.entity_records[0].numeric_pair = None;
    let mut legacy_namespace = cadmpeg_ir::NativeNamespace::default();
    legacy
        .store(&mut legacy_namespace)
        .expect("store legacy numeric-pair view");
    legacy_namespace.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_REFERENCE_SIGNATURE_COHORT_VERSION).unwrap(),
    );
    let migrated =
        crate::native::CatiaNative::load(&legacy_namespace).expect("migrate numeric-pair view");
    assert!(migrated.entity_records[0].numeric_pair.is_some());

    let mut malformed = native;
    malformed.entity_records[0]
        .numeric_pair
        .as_mut()
        .expect("complete numeric pair")
        .slots[0] = crate::entity_table::NumericPairSlot::ControlE8 { offset: 8 };
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed numeric-pair view");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn decode_reports_complete_numeric_entity_value_pairs_separately_from_packets() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_numeric_entity_value_pair()),
            &DecodeOptions::default(),
        )
        .expect("decode complete numeric entity-value pair");

    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_NUMERIC_ENTITY_VALUE_PAIR_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_NUMERIC_ENTITY_VALUE_PACKET_COUNT),
        0
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("1 complete numeric entity-value pair(s)")
            && loss
                .message
                .contains("0 embedded numeric entity-value packet(s)")
    }));
}

#[test]
fn native_namespace_retains_and_validates_complete_entity_reference_signatures() {
    let value = [
        0x32, 3, 0, 0, 0, 0x82, 0xe8, 0xe0, 0x0a, 0x37, 0x85, 0x81, b'2', b'(', b'E', b')', 0xfe,
        0x32, 4, 0, 0, 0, 0x82, 0xe9, 0xe0, 0x17, 0x08, 0x37, 0xfe, 0xfe, 0xfe,
    ];
    let records = [
        object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x82, 0x81], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x81], &[0xfe]),
    ];
    let mut bytes = entity_table_record_with_value(1, &value);
    bytes.extend(entity_table_record_with_value(2, &value));
    bytes.extend(entity_table_record_with_definition_and_value(
        3,
        &[0x01],
        &[0xfe],
    ));
    bytes.push(0xde);
    bytes.extend(object_graph_from_records(&records));

    let native = crate::native::CatiaNative::decode(&bytes);
    let signature = native.entity_records[0]
        .reference_signature
        .as_ref()
        .expect("complete reference signature");
    assert_eq!(signature.production.first_reference, 3);
    assert_eq!(signature.first_entity.entity_id, 3);
    assert_eq!(
        signature.first_entity.entity.as_deref(),
        Some(native.entity_records[2].id.as_str())
    );
    assert!(!signature.first_entity.is_null);
    assert_eq!(signature.production.second_reference, 4);
    assert_eq!(signature.second_entity.entity_id, 4);
    assert!(signature.second_entity.entity.is_none());
    assert!(signature.second_entity.is_null);
    assert_eq!(signature.production.second_reference_offset, 17);
    assert_eq!(signature.production.signature, "2(E)");
    assert_eq!(signature.production.signature_offset, 12);
    let [cohort] = native.reference_signature_cohorts.as_slice() else {
        panic!("one reference-signature cohort");
    };
    let graph_key = cohort
        .parent
        .split_once('#')
        .expect("object graph identity")
        .1;
    assert_eq!(
        cohort.id,
        format!("catia:outer:reference-signature-cohort#{graph_key}:00000000")
    );
    assert_eq!(cohort.ordinal, 0);
    assert_eq!(cohort.first_reference, 3);
    assert_eq!(cohort.second_reference, 4);
    assert!(cohort.schema_selection.is_none());
    assert_eq!(
        cohort.members,
        [
            native.entity_records[0].id.clone(),
            native.entity_records[1].id.clone()
        ]
    );

    let expected = signature.clone();
    let expected_cohort = cohort.clone();
    let mut stored = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut stored)
        .expect("store reference-signature incidences");
    stored.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_REFERENCE_SIGNATURE_INCIDENCE_VERSION - 1)
            .unwrap(),
    );
    let mut stored_fields = stored
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut();
    let stored_signature = stored_fields
        .get_mut("reference_signature")
        .expect("stored reference signature")
        .as_object_mut()
        .expect("stored reference-signature object");
    stored_signature.remove("signature_offset");
    stored_signature.remove("second_reference_offset");
    drop(stored_fields);
    let migrated =
        crate::native::CatiaNative::load(&stored).expect("migrate reference-signature incidences");
    assert_eq!(
        migrated.entity_records[0].reference_signature,
        Some(expected.clone())
    );

    let mut stored = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut stored)
        .expect("store resolved reference-signature incidences");
    stored.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_REFERENCE_SIGNATURE_ENTITY_VERSION - 1)
            .unwrap(),
    );
    let mut stored_fields = stored
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut();
    let stored_signature = stored_fields
        .get_mut("reference_signature")
        .expect("stored reference signature")
        .as_object_mut()
        .expect("stored reference-signature object");
    stored_signature.remove("first_entity");
    stored_signature.remove("second_entity");
    drop(stored_fields);
    let migrated =
        crate::native::CatiaNative::load(&stored).expect("resolve reference-signature incidences");
    assert_eq!(
        migrated.entity_records[0].reference_signature,
        Some(expected.clone())
    );

    let mut stored = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut stored)
        .expect("store reference-signature program");
    stored.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_REFERENCE_SIGNATURE_FRAME_VERSION - 1)
            .unwrap(),
    );
    let mut stored_fields = stored
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut();
    let stored_signature = stored_fields
        .get_mut("reference_signature")
        .expect("stored reference signature")
        .as_object_mut()
        .expect("stored reference-signature object");
    stored_signature.remove("prefix");
    stored_signature.remove("signature_program");
    drop(stored_fields);
    let migrated =
        crate::native::CatiaNative::load(&stored).expect("parse reference-signature program");
    assert_eq!(
        migrated.entity_records[0].reference_signature,
        Some(expected.clone())
    );

    let mut stored = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut stored)
        .expect("store consecutive reference-signature pair");
    stored.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_REFERENCE_SIGNATURE_PAIR_VERSION - 1)
            .unwrap(),
    );
    let migrated =
        crate::native::CatiaNative::load(&stored).expect("validate reference-signature pair");
    assert_eq!(
        migrated.entity_records[0].reference_signature,
        Some(expected)
    );

    let mut stored = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut stored)
        .expect("store reference-signature schema incidence");
    stored.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_REFERENCE_SIGNATURE_SCHEMA_VERSION - 1)
            .unwrap(),
    );
    let migrated =
        crate::native::CatiaNative::load(&stored).expect("derive reference-signature schema");
    assert_eq!(
        migrated.reference_signature_cohorts.as_slice(),
        std::slice::from_ref(&expected_cohort)
    );

    let mut stored = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut stored)
        .expect("store reference-signature cohort");
    stored.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_REFERENCE_SIGNATURE_COHORT_VERSION - 1)
            .unwrap(),
    );
    stored.arenas.remove("reference_signature_cohorts");
    let migrated =
        crate::native::CatiaNative::load(&stored).expect("derive reference-signature cohort");
    assert_eq!(
        migrated.reference_signature_cohorts.as_slice(),
        std::slice::from_ref(&expected_cohort)
    );

    let mut file = standard_catpart();
    file.splice(16..16, bytes.clone());
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode reference-signature incidences");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_REFERENCE_SIGNATURE_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_REFERENCE_SIGNATURE_PREFIX_ATOM_2_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_REFERENCE_SIGNATURE_PREFIX_ATOM_35_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_REFERENCE_SIGNATURE_COHORT_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_MULTI_MEMBER_REFERENCE_SIGNATURE_COHORT_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_REFERENCE_SIGNATURE_COHORT_MEMBER_COUNT),
        2
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_SELECTED_REFERENCE_SIGNATURE_COHORT_COUNT
        ),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_REFERENCE_SIGNATURE_INSTRUCTION_COUNT),
        8
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_REFERENCE_SIGNATURE_TOKEN_COUNT),
        8
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RESOLVED_REFERENCE_SIGNATURE_ENTITY_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_NULL_REFERENCE_SIGNATURE_ENTITY_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_UNRESOLVED_REFERENCE_SIGNATURE_ENTITY_COUNT),
        0
    );

    let mut malformed = native;
    malformed.entity_records[0]
        .reference_signature
        .as_mut()
        .expect("complete reference signature")
        .second_entity
        .entity_id += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed reference-signature view");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = crate::native::CatiaNative::decode(&bytes);
    malformed.entity_records[0]
        .reference_signature
        .as_mut()
        .expect("complete reference signature")
        .production
        .signature_offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed reference-signature incidence");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = crate::native::CatiaNative::decode(&bytes);
    malformed.entity_records[0]
        .reference_signature
        .as_mut()
        .expect("complete reference signature")
        .production
        .signature_program
        .clear();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed reference-signature program");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = crate::native::CatiaNative::decode(&bytes);
    malformed.reference_signature_cohorts[0].members.clear();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed reference-signature cohort");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_tokenizes_and_validates_complete_entity_values() {
    let mut value = vec![0x32, 4, 0, 0, 0, 0x87, 0xe6];
    value.extend_from_slice(&12.5_f64.to_bits().to_le_bytes());
    value.extend_from_slice(&[0x87, 0xe8, 0xfe]);
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])];
    let mut bytes = entity_table_record_with_value(1, &value);
    bytes.push(0xde);
    bytes.extend(object_graph_from_records(&records));

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(
        native.entity_records[0].value_fields,
        [
            crate::value_block::ValueField::SchemaSelector {
                ordinal: 4,
                offset: 0,
            },
            crate::value_block::ValueField::Binary64 {
                bits: 12.5_f64.to_bits(),
                offset: 5,
            },
            crate::value_block::ValueField::Marker {
                code: 0xe8,
                offset: 15,
            },
            crate::value_block::ValueField::Terminator { offset: 17 },
        ]
    );

    let mut malformed = native;
    malformed.entity_records[0].value_fields.pop();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed entity-value view");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_resolves_and_validates_entity_value_schema_selections() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_entity_value_schema_selection());
    let selection = &native.entity_records[0].value_schema_selections[0];
    assert_eq!(selection.ordinal, 4);
    assert_eq!(selection.offset, 0);
    assert_eq!(selection.name, "TargetValue");
    assert!(!selection.entry.is_empty());
    assert_eq!(
        selection.encoded_value,
        [
            crate::value_block::ValueField::Binary64 {
                bits: 12.5_f64.to_bits(),
                offset: 5,
            },
            crate::value_block::ValueField::Opcode {
                code: 0xe8,
                offset: 15,
            },
            crate::value_block::ValueField::Atom {
                value: 3851,
                width: 2,
                offset: 16,
            },
            crate::value_block::ValueField::Separator { offset: 18 },
            crate::value_block::ValueField::Terminator { offset: 19 },
            crate::value_block::ValueField::Terminator { offset: 20 },
        ]
    );
    assert_eq!(
        selection.packets,
        [crate::entity_table::EntityValuePacket::Compact {
            offset: 15,
            value_selector: 0x0ae0,
        }]
    );

    let assert_rejected = |malformed: crate::native::CatiaNative| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed entity-value schema view");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    };

    let mut wrong_name = native.clone();
    wrong_name.entity_records[0].value_schema_selections[0].name = "WrongValue".to_string();
    assert_rejected(wrong_name);

    let mut wrong_packet = native;
    let crate::entity_table::EntityValuePacket::Compact { value_selector, .. } =
        &mut wrong_packet.entity_records[0].value_schema_selections[0].packets[0]
    else {
        panic!("compact value packet");
    };
    *value_selector += 1;
    assert_rejected(wrong_packet);
}

#[test]
fn native_namespace_types_and_validates_named_parameter_values() {
    use crate::native::{
        CatiaEntityEvaluation, CatiaEntityEvaluationEncoding, CatiaEntitySuffixPayload,
        CatiaEntitySuffixTrailer, CatiaEntitySuffixValue,
    };

    let scalar = 35.0_f64.to_bits();
    let mut scalar_suffix = vec![0x85, 0x96, 0x82, 0x6a, 0xe6];
    scalar_suffix.extend_from_slice(&scalar.to_le_bytes());
    scalar_suffix.extend_from_slice(&[0x81, 0x52]);
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&scalar_suffix));
    let parameter = native.entity_records[0]
        .parameter_value
        .as_ref()
        .expect("complete named parameter value");
    assert_eq!(parameter.name.value, "Thickness");
    assert_eq!(parameter.binding.value, "#1_ /2");
    assert_eq!(
        parameter.evaluation,
        CatiaEntityEvaluation::Scalar { bits: scalar }
    );
    assert_eq!(
        native.entity_records[0].suffix_value,
        Some(CatiaEntitySuffixValue {
            prefix_atoms: [5, 22, 2],
            prefix_atom_widths: [1, 1, 1],
            prefix_code: 0x6a,
            payload: CatiaEntitySuffixPayload::Evaluation {
                opcode_offset: 4,
                evaluation: CatiaEntityEvaluation::Scalar { bits: scalar },
                encoding: CatiaEntityEvaluationEncoding::Direct,
            },
            trailer: CatiaEntitySuffixTrailer::Token8152,
        })
    );
    assert_eq!(parameter.evaluation_opcode_offset, 4);

    let unset = crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
        0x85, 0x96, 0x82, 0x6a, 0xe7, 0x81, 0x52,
    ]));
    assert_eq!(
        unset.entity_records[0]
            .parameter_value
            .as_ref()
            .expect("complete unset parameter")
            .evaluation,
        CatiaEntityEvaluation::Unset
    );

    let mut stale_offsets = native.clone();
    let CatiaEntitySuffixPayload::Evaluation { opcode_offset, .. } = &mut stale_offsets
        .entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete named parameter suffix")
        .payload
    else {
        panic!("named parameter evaluation");
    };
    *opcode_offset = 0;
    stale_offsets.entity_records[0]
        .parameter_value
        .as_mut()
        .expect("complete named parameter value")
        .evaluation_opcode_offset = 0;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    stale_offsets
        .store(&mut namespace)
        .expect("store stale named parameter offsets");
    namespace.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_SUFFIX_EVALUATION_OFFSET_VERSION - 1)
            .unwrap(),
    );
    let migrated =
        crate::native::CatiaNative::load(&namespace).expect("migrate named parameter offsets");
    assert_eq!(
        migrated.entity_records[0].parameter_value,
        native.entity_records[0].parameter_value
    );

    let mut malformed_offset = native.clone();
    malformed_offset.entity_records[0]
        .parameter_value
        .as_mut()
        .expect("complete named parameter value")
        .evaluation_opcode_offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_offset
        .store(&mut namespace)
        .expect("store malformed named parameter offset");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = native;
    malformed.entity_records[0]
        .parameter_value
        .as_mut()
        .expect("complete named parameter value")
        .name
        .value = "changed".to_string();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed parameter value");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_binds_two_definition_value_chains() {
    use crate::native::{
        CatiaDefinitionChainValue, CatiaEntityEvaluation, CatiaEntitySchemaValue,
        CatiaEntitySuffixSchemaValue,
    };

    let bits = 12.5_f64.to_bits();
    let mut suffix = vec![0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe6];
    suffix.extend_from_slice(&bits.to_le_bytes());
    suffix.extend_from_slice(&[0x81, 0x49]);
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_definition_chain_value(&suffix)),
            &DecodeOptions::default(),
        )
        .expect("decode definition-chain evaluation");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DEFINITION_CHAIN_VALUE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DEFINITION_CHAIN_EVALUATION_COUNT),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_STRUCTURALLY_OWNED_DEFINITION_CHAIN_VALUE_COUNT
        ),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_DEFINITION_CHAIN_VALUE_OWNER_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_EVALUATED_DEFINITION_CHAIN_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_UNSET_DEFINITION_CHAIN_COUNT),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_STRUCTURALLY_OWNED_DEFINITION_CHAIN_EVALUATION_COUNT
        ),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_DEFINITION_CHAIN_EVALUATION_OWNER_COUNT),
        0
    );
    let mut native = crate::native::CatiaNative::load(
        decoded.ir().native.namespace("catia").expect("namespace"),
    )
    .expect("load definition-chain evaluation");
    assert_eq!(
        native.entity_records[0].definition_chain_value,
        Some(CatiaDefinitionChainValue {
            selector: CatiaEntitySchemaValue {
                offset: native.entity_records[0].definition_schema_selections[0].offset,
                ordinal: native.entity_records[0].definition_schema_selections[0].ordinal,
                entry: native.catalogs[0].entries[4].id.clone(),
                value: "FeatureFEDGE".to_string(),
            },
            role: CatiaEntitySchemaValue {
                offset: native.entity_records[0].definition_schema_selections[1].offset,
                ordinal: native.entity_records[0].definition_schema_selections[1].ordinal,
                entry: native.catalogs[0].entries[5].id.clone(),
                value: "Real".to_string(),
            },
            value: CatiaEntitySuffixSchemaValue::Evaluation {
                opcode_offset: 8,
                evaluation: CatiaEntityEvaluation::Scalar { bits },
            },
        })
    );
    assert_eq!(
        native.design_objects[0].definition_chain_values,
        [native.entity_records[0].id.clone()]
    );

    let mut malformed_ownership = native.clone();
    malformed_ownership.design_objects[0]
        .definition_chain_values
        .clear();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_ownership
        .store(&mut namespace)
        .expect("store malformed definition-chain ownership");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    native.entity_records[0]
        .definition_chain_value
        .as_mut()
        .expect("definition-chain evaluation")
        .role
        .value = "changed".to_string();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store malformed definition-chain evaluation");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let wrong_selector =
        crate::native::CatiaNative::decode(&standard_catpart_with_definition_chain_value(&[
            0x84, 0x88, 0x82, 0x32, 5, 0, 0, 0, 0xe7,
        ]));
    assert!(wrong_selector.entity_records[0]
        .definition_chain_value
        .is_none());

    let atom = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_definition_chain_value(&[
                0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0x87,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode definition-chain atom");
    assert_eq!(
        atom.report()
            .coverage_count(crate::coverage::DECODED_DEFINITION_CHAIN_VALUE_COUNT),
        1
    );
    assert_eq!(
        atom.report()
            .coverage_count(crate::coverage::DECODED_DEFINITION_CHAIN_ATOM_COUNT),
        1
    );
    assert_eq!(
        atom.report()
            .coverage_count(crate::coverage::DECODED_DEFINITION_CHAIN_EVALUATION_COUNT),
        0
    );
    let atom_native =
        crate::native::CatiaNative::load(atom.ir().native.namespace("catia").expect("namespace"))
            .expect("load definition-chain atom");
    assert_eq!(
        atom_native.entity_records[0]
            .definition_chain_value
            .as_ref()
            .map(|value| &value.value),
        Some(&CatiaEntitySuffixSchemaValue::Atom { value: 7 })
    );

    for (payload, coverage) in [
        (0xe8, "decoded_definition_chain_control_count"),
        (0x37, "decoded_definition_chain_separator_count"),
    ] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(standard_catpart_with_definition_chain_value(&[
                    0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, payload,
                ])),
                &DecodeOptions::default(),
            )
            .expect("decode definition-chain state");
        assert_eq!(
            decoded
                .report()
                .coverage()
                .get(coverage)
                .copied()
                .unwrap_or(0),
            1
        );
    }

    let nested = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_definition_chain_value(&[
                0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0x32, 5, 0, 0, 0,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode nested definition-chain selector");
    assert_eq!(
        nested
            .report()
            .coverage_count(crate::coverage::DECODED_DEFINITION_CHAIN_SCHEMA_SELECTOR_COUNT),
        1
    );
    let nested_native =
        crate::native::CatiaNative::load(nested.ir().native.namespace("catia").expect("namespace"))
            .expect("load nested definition-chain selector");
    assert_eq!(
        nested_native.entity_records[0]
            .definition_chain_value
            .as_ref()
            .map(|value| &value.value),
        Some(&CatiaEntitySuffixSchemaValue::SchemaSelector {
            offset: 8,
            ordinal: 5,
            entry: Some(nested_native.catalogs[0].entries[5].id.clone()),
            name: Some("Real".to_string()),
        })
    );
}

#[test]
fn typed_definition_chain_values_transfer_as_parameters() {
    let bits = 12.5_f64.to_bits();
    let mut suffix = vec![0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe6];
    suffix.extend_from_slice(&bits.to_le_bytes());
    suffix.extend_from_slice(&[0x81, 0x49]);
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_definition_chain_value(&suffix)),
            &DecodeOptions::default(),
        )
        .expect("decode typed definition-chain parameter");

    let [parameter] = decoded.ir().model.parameters.as_slice() else {
        panic!("expected one typed definition-chain parameter");
    };
    assert_eq!(parameter.name, "FeatureFEDGE");
    assert_eq!(parameter.expression, "12.5");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(12.5))
    );
    assert_eq!(parameter.owner, None);
    assert_eq!(parameter.properties["value_type"], "Real");
    assert!(!parameter.properties.contains_key("catia_binding"));
    assert_eq!(
        parameter.properties["catia_definition_evaluation_opcode_offset"],
        "8"
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_DEFINITION_CHAIN_PARAMETER_COUNT),
        1
    );

    let boolean = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_definition_chain_type(
                "Boolean",
                &[0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0x81],
            )),
            &DecodeOptions::default(),
        )
        .expect("decode Boolean definition-chain parameter");
    let [boolean_parameter] = boolean.ir().model.parameters.as_slice() else {
        panic!("expected one Boolean definition-chain parameter");
    };
    assert_eq!(
        boolean_parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Boolean(true))
    );
    assert_eq!(boolean_parameter.expression, "true");
    assert_eq!(boolean_parameter.properties["value_type"], "Boolean");
    assert_eq!(
        boolean_parameter.properties["catia_definition_value_kind"],
        "atom"
    );
    assert_eq!(
        boolean_parameter.properties["catia_definition_atom_value"],
        "1"
    );
    assert!(!boolean_parameter
        .properties
        .contains_key("catia_definition_evaluation_opcode_offset"));
    assert_eq!(
        boolean
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_DEFINITION_CHAIN_PARAMETER_COUNT),
        1
    );

    let invalid_boolean = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_definition_chain_type(
                "Boolean",
                &[0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0x82],
            )),
            &DecodeOptions::default(),
        )
        .expect("decode invalid Boolean definition-chain atom");
    assert!(invalid_boolean.ir().model.parameters.is_empty());
    assert_eq!(
        invalid_boolean
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_DEFINITION_CHAIN_PARAMETER_COUNT),
        0
    );

    let unset = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_definition_chain_value(&[
                0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe7,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode unset definition-chain parameter");
    let [unset_parameter] = unset.ir().model.parameters.as_slice() else {
        panic!("expected one unset definition-chain parameter");
    };
    assert!(unset_parameter.value.is_none());
    assert!(unset_parameter.expression.is_empty());
    assert_eq!(
        unset
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_DEFINITION_CHAIN_PARAMETER_COUNT),
        1
    );

    let mut native =
        crate::native::CatiaNative::decode(&standard_catpart_with_definition_chain_value(&[
            0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe6, 0, 0, 0, 0, 0, 0, 0, 0,
        ]));
    let parameter_entity = native.entity_records[0].clone();
    native.entity_records[0].relation_program_instance =
        Some(crate::native::CatiaRelationProgramInstance {
            framing: crate::native::CatiaRelationProgramInstanceFraming::Lead12,
            program_entity: crate::native::CatiaEntityReference::default(),
            repeated_entity: crate::native::CatiaEntityReference::default(),
            reference_incidences: Vec::new(),
            relation_expression: None,
            parameter_dependencies: Vec::new(),
            inputs: Some(vec![crate::native::CatiaRelationProgramInput {
                parameter: "#1_".to_string(),
                value_type: "Real".to_string(),
                entity: crate::native::CatiaEntityReference {
                    entity_id: parameter_entity.entity_id,
                    is_null: false,
                    entity: Some(parameter_entity.id.clone()),
                    class_name: Some("param".to_string()),
                },
            }]),
            output_entity: None,
            lead12_context_entity: None,
            lead54_trailing_entity: None,
        });
    let mut relation_ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let relation_transfer = crate::formula::transfer_parameters(
        &mut relation_ir,
        &native,
        &mut Annotations::default(),
        None,
    );
    assert_eq!(relation_transfer.definition_chain_parameter_count, 1);
    assert_eq!(relation_transfer.relation_program_parameter_count, 1);
    assert_eq!(relation_ir.model.parameters.len(), 1);
}

#[test]
fn design_objects_retain_definition_chain_values_in_field_order() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_two_definition_chain_values());
    assert_eq!(native.design_objects.len(), 1);
    assert_eq!(
        native.design_objects[0].definition_chain_values,
        native
            .entity_records
            .iter()
            .map(|entity| entity.id.clone())
            .collect::<Vec<_>>()
    );

    let mut reversed = native;
    reversed.design_objects[0].definition_chain_values.reverse();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    reversed
        .store(&mut namespace)
        .expect("store misordered definition-chain ownership");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_two_definition_chain_values());
    let expected = native.design_objects[0].definition_chain_values.clone();
    let mut previous_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut previous_namespace)
        .expect("store current definition-chain ownership");
    let mut previous_design_objects: Vec<crate::native::CatiaDesignObject> = previous_namespace
        .arena_as("design_objects")
        .expect("load stored design objects");
    for object in &mut previous_design_objects {
        object.definition_chain_values.clear();
    }
    previous_namespace
        .set_arena("design_objects", &previous_design_objects)
        .expect("store previous design objects");
    previous_namespace.set_version(std::num::NonZeroU32::new(195).unwrap());
    let migrated = crate::native::CatiaNative::load(&previous_namespace)
        .expect("migrate previous definition-chain ownership");
    assert_eq!(migrated.design_objects[0].definition_chain_values, expected);
}

#[test]
fn literal_owner_slots_remain_unassigned_and_migrate_from_previous_namespaces() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_unassigned_definition_chain_value()),
            &DecodeOptions::default(),
        )
        .expect("decode literal owner slot");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DEFINITION_CHAIN_VALUE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_DEFINITION_CHAIN_VALUE_OWNER_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_UNASSIGNED_DEFINITION_CHAIN_VALUE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_UNASSIGNED_DEFINITION_CHAIN_EVALUATION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_UNASSIGNED_OBJECT_OWNER_SLOT_COUNT),
        1
    );

    let native = crate::native::CatiaNative::load(
        decoded.ir().native.namespace("catia").expect("namespace"),
    )
    .expect("load literal owner slot");
    let record = &native.object_graphs[0].records[0];
    assert_eq!(
        record.owner,
        Some(crate::native::CatiaObjectOwner::UnassignedLiteral(66))
    );
    assert!(record.design_object.is_none());
    assert!(native.design_objects.is_empty());

    let mut malformed = native.clone();
    malformed.object_graphs[0].records[0].owner =
        Some(crate::native::CatiaObjectOwner::UnassignedLiteral(67));
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed literal owner slot");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut previous_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut previous_namespace)
        .expect("store current literal owner slot");
    let mut previous_records: Vec<crate::native::CatiaObjectRecord> = previous_namespace
        .arena_as("object_graph_records")
        .expect("load stored object records");
    previous_records[0].owner = None;
    previous_namespace
        .set_arena("object_graph_records", &previous_records)
        .expect("store previous object records");
    previous_namespace.set_version(std::num::NonZeroU32::new(197).unwrap());
    let migrated = crate::native::CatiaNative::load(&previous_namespace)
        .expect("migrate previous literal owner slot");
    assert_eq!(
        migrated.object_graphs[0].records[0].owner,
        Some(crate::native::CatiaObjectOwner::UnassignedLiteral(66))
    );
}

#[test]
fn native_namespace_binds_and_validates_definition_values() {
    use crate::native::{
        CatiaDefinitionValue, CatiaEntityEvaluation, CatiaEntityEvaluationEncoding,
        CatiaEntitySchemaValue, CatiaEntitySuffixPayload,
    };

    let bits = 12.5_f64.to_bits();
    let mut suffix = vec![0xd1, 0x53, 0x96, 0x82, 0xa6, 0xe6];
    suffix.extend_from_slice(&bits.to_le_bytes());
    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0];
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_definition_value(
                &definition,
                &[0xfe],
                &suffix,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode definition-bound value");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DEFINITION_VALUE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_OWNED_DEFINITION_VALUE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_DEFINITION_VALUE_OWNER_COUNT),
        0
    );
    let mut native = crate::native::CatiaNative::load(
        decoded.ir().native.namespace("catia").expect("namespace"),
    )
    .expect("load definition-bound value");
    assert_eq!(
        native.entity_records[0].definition_value,
        Some(CatiaDefinitionValue {
            definition: CatiaEntitySchemaValue {
                offset: native.entity_records[0].definition_schema_selections[0].offset,
                ordinal: native.entity_records[0].definition_schema_selections[0].ordinal,
                entry: native.catalogs[0].entries[4].id.clone(),
                value: "Thickness".to_string(),
            },
            payload: CatiaEntitySuffixPayload::Evaluation {
                opcode_offset: 5,
                evaluation: CatiaEntityEvaluation::Scalar { bits },
                encoding: CatiaEntityEvaluationEncoding::Direct,
            },
            schema_selection: None,
        })
    );
    assert_eq!(
        native.design_objects[0].definition_values,
        [native.entity_records[0].id.clone()]
    );
    assert_eq!(
        native.object_graphs[0].records[0].storage_record,
        Some(native.object_graphs[0].records[0].id.clone())
    );
    assert_eq!(
        native.object_graphs[0].records[0].storage_design_object,
        Some(native.design_objects[0].id.clone())
    );

    let mut malformed_storage = native.clone();
    malformed_storage.object_graphs[0].records[0].storage_record = None;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_storage
        .store(&mut namespace)
        .expect("store malformed storage link");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed_ownership = native.clone();
    malformed_ownership.design_objects[0]
        .definition_values
        .clear();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_ownership
        .store(&mut namespace)
        .expect("store malformed definition-value ownership");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let definition_value = native.entity_records[0]
        .definition_value
        .as_mut()
        .expect("definition-bound value");
    definition_value.payload = CatiaEntitySuffixPayload::Evaluation {
        opcode_offset: 5,
        evaluation: CatiaEntityEvaluation::Unset,
        encoding: CatiaEntityEvaluationEncoding::Direct,
    };
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store malformed definition value");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let control = crate::native::CatiaNative::decode(&standard_catpart_with_definition_value(
        &definition,
        &[0xfe],
        &[0xd1, 0x67, 0x88, 0x81, 0xbd, 0xe8, 0x81, 0x49],
    ));
    assert!(matches!(
        control.entity_records[0]
            .definition_value
            .as_ref()
            .expect("definition-bound control")
            .payload,
        CatiaEntitySuffixPayload::ControlE8
    ));

    let schema_selected =
        crate::native::CatiaNative::decode(&standard_catpart_with_definition_value(
            &definition,
            &[0xfe],
            &[0x84, 0x96, 0x82, 0x32, 4, 0, 0, 0, 0xe7, 0x81, 0x49],
        ));
    let definition_value = schema_selected.entity_records[0]
        .definition_value
        .as_ref()
        .expect("definition-bound schema-selected value");
    assert!(matches!(
        definition_value.payload,
        CatiaEntitySuffixPayload::SchemaSelected { selector: 4, .. }
    ));
    assert_eq!(
        definition_value
            .schema_selection
            .as_ref()
            .expect("resolved suffix schema")
            .name,
        "Thickness"
    );

    for (definition, value) in [
        (
            vec![0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 4, 0, 0, 0],
            vec![0xfe],
        ),
        (definition.to_vec(), vec![0x80, 0xfe]),
    ] {
        let native = crate::native::CatiaNative::decode(&standard_catpart_with_definition_value(
            &definition,
            &value,
            &suffix,
        ));
        assert_eq!(native.entity_records[0].definition_value, None);
    }
}

#[test]
fn named_parameter_value_requires_the_complete_finite_suffix() {
    let nonfinite = f64::NAN.to_bits();
    let mut suffix = vec![0x85, 0x96, 0x82, 0x6a, 0xe6];
    suffix.extend_from_slice(&nonfinite.to_le_bytes());
    suffix.extend_from_slice(&[0x81, 0x52]);

    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&suffix));
    assert!(native.entity_records[0].suffix_value.is_none());
    assert!(native.entity_records[0].parameter_value.is_none());

    let control = crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
        0x85, 0x96, 0x82, 0x6a, 0xe8, 0x81, 0x52,
    ]));
    assert!(matches!(
        control.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete control suffix")
            .payload,
        crate::native::CatiaEntitySuffixPayload::ControlE8
    ));
    assert!(control.entity_records[0].parameter_value.is_none());
}

#[test]
fn native_retains_migrates_and_validates_typed_schema_selector_incidences() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_formula_relation(4, false));
    let expression_entity = &native.entity_records[1];
    let expression = expression_entity
        .relation_expression
        .as_ref()
        .expect("complete relation expression");
    assert_eq!(
        (expression.expression.offset, expression.expression.ordinal),
        (
            expression_entity.value_schema_selections[1].offset,
            expression_entity.value_schema_selections[1].ordinal,
        )
    );
    let parameter_entity = &native.entity_records[2];
    let parameter = parameter_entity
        .parameter_value
        .as_ref()
        .expect("complete named parameter");
    assert_eq!(
        (parameter.name.offset, parameter.name.ordinal),
        (
            parameter_entity.value_schema_selections[0].offset,
            parameter_entity.value_schema_selections[0].ordinal,
        )
    );
    assert_eq!(
        (parameter.binding.offset, parameter.binding.ordinal),
        (
            parameter_entity.value_schema_selections[1].offset,
            parameter_entity.value_schema_selections[1].ordinal,
        )
    );

    let mut stale = native.clone();
    let expression = stale.entity_records[1]
        .relation_expression
        .as_mut()
        .expect("complete relation expression");
    expression.expression.offset = 0;
    expression.expression.ordinal = 0;
    let parameter = stale.entity_records[2]
        .parameter_value
        .as_mut()
        .expect("complete named parameter");
    parameter.name.offset = 0;
    parameter.name.ordinal = 0;
    parameter.binding.offset = 0;
    parameter.binding.ordinal = 0;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    stale
        .store(&mut namespace)
        .expect("store stale typed schema incidences");
    namespace.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_ENTITY_SCHEMA_VALUE_INCIDENCE_VERSION - 1)
            .unwrap(),
    );
    let migrated =
        crate::native::CatiaNative::load(&namespace).expect("migrate typed schema incidences");
    assert_eq!(
        migrated.entity_records[1].relation_expression,
        native.entity_records[1].relation_expression
    );
    assert_eq!(
        migrated.entity_records[2].parameter_value,
        native.entity_records[2].parameter_value
    );

    let mut malformed = native;
    malformed.entity_records[2]
        .parameter_value
        .as_mut()
        .expect("complete named parameter")
        .name
        .offset = u64::MAX;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed typed schema incidence");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn entity_value_schema_selection_excludes_a_packet_crossing_its_boundary() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_crossing_entity_value_packet());
    assert_eq!(native.entity_records[0].value_packets.len(), 1);
    assert_eq!(native.entity_records[0].value_schema_selections.len(), 2);
    assert!(native.entity_records[0]
        .value_schema_selections
        .iter()
        .all(|selection| selection.packets.is_empty()));

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store crossing packet fixture");
    crate::native::CatiaNative::load(&namespace).expect("validate canonical packet ownership");
}
