// SPDX-License-Identifier: Apache-2.0
//! Native-namespace tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::CatiaCodec;

#[test]
fn native_namespace_types_dimension_constraint_ranges() {
    use crate::native::{CatiaConstraintRangeFraming, CatiaEntityEvaluation};

    let scalar = 128.0_f64.to_bits();
    let mut suffix = vec![0x84, 0x96, 0x82, 0xc1, 0xe6];
    suffix.extend_from_slice(&scalar.to_le_bytes());
    let file = standard_catpart_with_two_selector_value("Range", "CstAttr_Dimension", &suffix);
    let native = crate::native::CatiaNative::decode(&file);
    let range = native.entity_records[0]
        .constraint_range
        .as_ref()
        .expect("complete dimension constraint range");
    assert!(!range.range.entry.is_empty());
    assert_eq!(range.range.value, "Range");
    assert!(!range.constraint.entry.is_empty());
    assert_eq!(range.constraint.value, "CstAttr_Dimension");
    assert_eq!(range.framing, CatiaConstraintRangeFraming::DimensionC1);
    assert_eq!(
        range.evaluation,
        CatiaEntityEvaluation::Scalar { bits: scalar }
    );
    assert_eq!(range.evaluation_opcode_offset, 4);

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode constraint range");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSTRAINT_RANGE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DIMENSION_CONSTRAINT_RANGE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_COMPLEX_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_EVALUATED_CONSTRAINT_RANGE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_UNSET_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSTRAINT_RANGE_INCOMING_REFERENCE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNREFERENCED_CONSTRAINT_RANGE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNIQUELY_REFERENCED_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::MULTIPLY_REFERENCED_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert!(!decoded
        .report()
        .coverage()
        .contains_key("decoded_structurally_owned_constraint_range_count"));
    assert!(!decoded
        .report()
        .coverage()
        .contains_key("unresolved_constraint_range_owner_count"));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == crate::loss::CatiaLossCode::AttributesDimensionQuantityUnresolved.kind()
            && loss.message.contains("1 finite")
    }));

    let referenced_file = |reference_count: usize, storage_reference: bool| {
        let value = [
            0x32, 4, 0, 0, 0, 0x82, 0xe8, 0xe0, 0x07, 0x37, 0x81, 0xfe, 0x32, 5, 0, 0, 0, 0xfe,
        ];
        let mut range_entity = entity_table_record_with_definition_and_value(1, &[0x01], &value);
        range_entity[6] = 2;
        range_entity.extend_from_slice(&suffix);
        let range_len = u32::try_from(range_entity.len()).expect("bounded range entity");
        range_entity[2..6].copy_from_slice(&range_len.to_le_bytes());
        let mut stream = range_entity;
        stream.extend(entity_table_record_with_definition_and_value(
            2,
            &[0x01],
            &[0xfe],
        ));
        let mut reference_payload = [0x81, 0x81].repeat(reference_count);
        reference_payload.push(0xfe);
        let reference_head = if storage_reference {
            vec![0x04, 0x01, 0x82, 0x84, 0x81]
        } else {
            vec![0x04, 0x01, 0x82, 0x84]
        };
        stream.push(0xde);
        stream.extend(object_graph_from_records(&[
            object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe]),
            object_graph_record(&reference_head, &reference_payload),
        ]));
        stream.extend(catalog_stream(&[
            "CATCatalogManager",
            "catalogManager",
            "catalogLinks",
            "",
            "Range",
            "CstAttr_Dimension",
        ]));
        let mut file = standard_catpart();
        file.splice(16..16, stream);
        let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
        file[8..12].copy_from_slice(&be32(file_len));
        file
    };
    let unique_file = referenced_file(1, false);
    let unique_native = crate::native::CatiaNative::decode(&unique_file);
    let incoming = &unique_native.entity_records[0]
        .constraint_range
        .as_ref()
        .expect("complete referenced constraint range")
        .incoming_references;
    assert_eq!(
        unique_native.entity_records[0]
            .range_interval
            .as_ref()
            .expect("complete referenced range interval")
            .incoming_references
            .as_slice(),
        incoming.as_slice()
    );
    assert_eq!(incoming.len(), 1);
    assert_eq!(
        incoming[0].object_record,
        unique_native.object_graphs[0].records[1].id
    );
    let source_entity = incoming[0]
        .source_entity
        .as_ref()
        .expect("source record has a paired entity");
    assert_eq!(source_entity.entity_id(), 2);
    assert_eq!(
        source_entity.entity(),
        Some(unique_native.entity_records[1].id.as_str())
    );
    assert_eq!(
        source_entity.class_name(),
        unique_native.object_graphs[0].records[1]
            .class_name
            .as_deref()
    );
    assert_eq!(
        incoming[0].payload_offset,
        unique_native.object_graphs[0].records[1].references[0].payload_offset()
    );
    assert_eq!(
        incoming[0].source,
        unique_native.object_graphs[0].records[1].references[0]
            .source()
            .clone()
    );

    let uniquely_referenced = CatiaCodec
        .decode(&mut Cursor::new(unique_file), &DecodeOptions::default())
        .expect("decode uniquely referenced constraint range");
    assert_eq!(
        uniquely_referenced
            .report()
            .coverage_count(crate::coverage::DECODED_CONSTRAINT_RANGE_INCOMING_REFERENCE_COUNT),
        1
    );
    assert_eq!(
        uniquely_referenced.report().coverage_count(
            crate::coverage::DECODED_CLASSIFIED_CONSTRAINT_RANGE_SOURCE_ENTITY_COUNT
        ),
        usize::from(source_entity.class_name().is_some())
    );
    assert_eq!(
        uniquely_referenced
            .report()
            .coverage_count(crate::coverage::UNCLASSIFIED_CONSTRAINT_RANGE_SOURCE_ENTITY_COUNT),
        usize::from(source_entity.class_name().is_none())
    );
    assert_eq!(
        uniquely_referenced
            .report()
            .coverage_count(crate::coverage::UNREFERENCED_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert_eq!(
        uniquely_referenced
            .report()
            .coverage_count(crate::coverage::UNIQUELY_REFERENCED_CONSTRAINT_RANGE_COUNT),
        1
    );
    assert_eq!(
        uniquely_referenced
            .report()
            .coverage_count(crate::coverage::MULTIPLY_REFERENCED_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert_eq!(
        uniquely_referenced
            .report()
            .coverage_count(crate::coverage::DECODED_RANGE_INTERVAL_INCOMING_REFERENCE_COUNT),
        1
    );
    assert_eq!(
        uniquely_referenced
            .report()
            .coverage_count(crate::coverage::UNIQUELY_REFERENCED_RANGE_INTERVAL_COUNT),
        1
    );

    let storage_file = referenced_file(0, true);
    let storage_native = crate::native::CatiaNative::decode(&storage_file);
    let incoming_storage = &storage_native.entity_records[0]
        .constraint_range
        .as_ref()
        .expect("complete storage-referenced constraint range")
        .incoming_storage_references;
    assert_eq!(
        storage_native.entity_records[0]
            .range_interval
            .as_ref()
            .expect("complete storage-referenced range interval")
            .incoming_storage_references
            .as_slice(),
        incoming_storage.as_slice()
    );
    assert_eq!(incoming_storage.len(), 1);
    assert_eq!(
        incoming_storage[0].object_record,
        storage_native.object_graphs[0].records[1].id
    );
    let storage_source_entity = incoming_storage[0]
        .source_entity
        .as_ref()
        .expect("storage source has a paired entity");
    assert_eq!(storage_source_entity.entity_id(), 2);
    assert_eq!(
        storage_source_entity.entity(),
        Some(storage_native.entity_records[1].id.as_str())
    );
    assert_eq!(
        storage_source_entity.class_name(),
        storage_native.object_graphs[0].records[1]
            .class_name
            .as_deref()
    );

    let storage_referenced = CatiaCodec
        .decode(&mut Cursor::new(storage_file), &DecodeOptions::default())
        .expect("decode storage-referenced constraint range");
    assert_eq!(
        storage_referenced
            .report()
            .coverage_count(crate::coverage::DECODED_CONSTRAINT_RANGE_INCOMING_REFERENCE_COUNT),
        1
    );
    assert_eq!(
        storage_referenced.report().coverage_count(
            crate::coverage::DECODED_CONSTRAINT_RANGE_INCOMING_PAYLOAD_REFERENCE_COUNT
        ),
        0
    );
    assert_eq!(
        storage_referenced.report().coverage_count(
            crate::coverage::DECODED_CONSTRAINT_RANGE_INCOMING_STORAGE_REFERENCE_COUNT
        ),
        1
    );
    assert_eq!(
        storage_referenced
            .report()
            .coverage_count(crate::coverage::UNREFERENCED_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert_eq!(
        storage_referenced
            .report()
            .coverage_count(crate::coverage::UNIQUELY_REFERENCED_CONSTRAINT_RANGE_COUNT),
        1
    );

    let combined = CatiaCodec
        .decode(
            &mut Cursor::new(referenced_file(1, true)),
            &DecodeOptions::default(),
        )
        .expect("decode constraint range with both incidence forms");
    assert_eq!(
        combined
            .report()
            .coverage_count(crate::coverage::DECODED_CONSTRAINT_RANGE_INCOMING_REFERENCE_COUNT),
        2
    );
    assert_eq!(
        combined
            .report()
            .coverage_count(crate::coverage::MULTIPLY_REFERENCED_CONSTRAINT_RANGE_COUNT),
        1
    );

    let multiple_file = referenced_file(2, false);
    let multiple_native = crate::native::CatiaNative::decode(&multiple_file);
    let incoming = &multiple_native.entity_records[0]
        .constraint_range
        .as_ref()
        .expect("complete multiply referenced constraint range")
        .incoming_references;
    assert_eq!(incoming.len(), 2);
    assert_eq!(
        incoming
            .iter()
            .map(|reference| reference.payload_offset)
            .collect::<Vec<_>>(),
        multiple_native.object_graphs[0].records[1]
            .references
            .iter()
            .map(|reference| reference.payload_offset())
            .collect::<Vec<_>>()
    );

    let multiply_referenced = CatiaCodec
        .decode(&mut Cursor::new(multiple_file), &DecodeOptions::default())
        .expect("decode multiply referenced constraint range");
    assert_eq!(
        multiply_referenced
            .report()
            .coverage_count(crate::coverage::DECODED_CONSTRAINT_RANGE_INCOMING_REFERENCE_COUNT),
        2
    );
    assert_eq!(
        multiply_referenced
            .report()
            .coverage_count(crate::coverage::UNREFERENCED_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert_eq!(
        multiply_referenced
            .report()
            .coverage_count(crate::coverage::UNIQUELY_REFERENCED_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert_eq!(
        multiply_referenced
            .report()
            .coverage_count(crate::coverage::MULTIPLY_REFERENCED_CONSTRAINT_RANGE_COUNT),
        1
    );
    assert_eq!(
        multiply_referenced
            .report()
            .coverage_count(crate::coverage::MULTIPLY_REFERENCED_RANGE_INTERVAL_COUNT),
        1
    );

    let mut malformed = native;
    malformed.entity_records[0]
        .constraint_range
        .as_mut()
        .expect("complete dimension constraint range")
        .framing = CatiaConstraintRangeFraming::DimensionB8;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed
        .store(&mut namespace)
        .expect("store malformed constraint range");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = crate::native::CatiaNative::decode(
        &standard_catpart_with_two_selector_value("Range", "CstAttr_Dimension", &suffix),
    );
    malformed.entity_records[0]
        .constraint_range
        .as_mut()
        .expect("complete dimension constraint range")
        .constraint
        .value = "changed".to_string();
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed
        .store(&mut namespace)
        .expect("store malformed constraint role");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = unique_native.clone();
    malformed.entity_records[0]
        .constraint_range
        .as_mut()
        .expect("complete referenced constraint range")
        .incoming_references[0]
        .payload_offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed
        .store(&mut namespace)
        .expect("store malformed constraint-range incidence");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = unique_native.clone();
    malformed.entity_records[0]
        .range_interval
        .as_mut()
        .expect("complete referenced range interval")
        .incoming_references[0]
        .payload_offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed
        .store(&mut namespace)
        .expect("store malformed range-interval incidence");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = storage_native.clone();
    malformed.entity_records[0]
        .constraint_range
        .as_mut()
        .expect("complete storage-referenced constraint range")
        .incoming_storage_references[0]
        .object_record = unique_native.object_graphs[0].records[0].id.clone();
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed
        .store(&mut namespace)
        .expect("store malformed constraint-range storage incidence");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut stored = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    unique_native
        .store(&mut stored)
        .expect("store older constraint-range namespace");
    stored.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_CONSTRAINT_RANGE_INCIDENCE_VERSION - 1)
            .unwrap(),
    );
    stored
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields()
        .get_mut("constraint_range")
        .expect("stored constraint range")
        .as_object_mut()
        .expect("stored constraint-range object")
        .remove("incoming_references");
    let migrated =
        crate::native::CatiaNative::load(&stored).expect("migrate constraint-range incidence");
    assert_eq!(
        migrated.entity_records[0]
            .constraint_range
            .as_ref()
            .expect("migrated constraint range")
            .incoming_references
            .len(),
        1
    );

    let mut stored = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    unique_native
        .store(&mut stored)
        .expect("store older range-interval incidence namespace");
    stored.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_RANGE_INTERVAL_INCIDENCE_VERSION - 1)
            .unwrap(),
    );
    stored
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields()
        .get_mut("range_interval")
        .expect("stored range interval")
        .as_object_mut()
        .expect("stored range-interval object")
        .remove("incoming_references");
    let migrated =
        crate::native::CatiaNative::load(&stored).expect("migrate range-interval incidence");
    assert_eq!(
        migrated.entity_records[0]
            .range_interval
            .as_ref()
            .expect("migrated range interval")
            .incoming_references
            .len(),
        1
    );

    let mut stored = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    unique_native
        .store(&mut stored)
        .expect("store older constraint-range source namespace");
    stored.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_CONSTRAINT_RANGE_SOURCE_ENTITY_VERSION - 1)
            .unwrap(),
    );
    stored
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields()
        .get_mut("constraint_range")
        .expect("stored constraint range")
        .as_object_mut()
        .expect("stored constraint-range object")
        .get_mut("incoming_references")
        .expect("stored incoming references")
        .as_array_mut()
        .expect("stored incoming-reference array")[0]
        .as_object_mut()
        .expect("stored incoming-reference object")
        .remove("source_entity");
    let migrated =
        crate::native::CatiaNative::load(&stored).expect("migrate constraint-range source entity");
    assert_eq!(
        migrated.entity_records[0]
            .constraint_range
            .as_ref()
            .expect("migrated constraint range")
            .incoming_references[0]
            .source_entity,
        unique_native.entity_records[0]
            .constraint_range
            .as_ref()
            .expect("source constraint range")
            .incoming_references[0]
            .source_entity
    );

    let mut stored = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    storage_native
        .store(&mut stored)
        .expect("store older constraint-range storage namespace");
    stored.set_version(
        std::num::NonZeroU32::new(
            crate::native::CATIA_CONSTRAINT_RANGE_STORAGE_INCIDENCE_VERSION - 1,
        )
        .unwrap(),
    );
    stored
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields()
        .get_mut("constraint_range")
        .expect("stored constraint range")
        .as_object_mut()
        .expect("stored constraint-range object")
        .remove("incoming_storage_references");
    let migrated = crate::native::CatiaNative::load(&stored)
        .expect("migrate constraint-range storage incidence");
    assert_eq!(
        migrated.entity_records[0]
            .constraint_range
            .as_ref()
            .expect("migrated constraint range")
            .incoming_storage_references,
        storage_native.entity_records[0]
            .constraint_range
            .as_ref()
            .expect("source constraint range")
            .incoming_storage_references
    );
}

#[test]
fn native_namespace_types_and_validates_range_intervals_independently_of_constraint_roles() {
    use crate::entity_table::{RangeIntervalPrefix, RangeIntervalSlot};
    use crate::native::CatiaRangeNominalFraming;

    let lower_bits = (-0.2032_f64).to_bits();
    let upper_bits = 0.2032_f64.to_bits();
    let mut encoded_range = vec![0x87, 0xe8, 0xe0, 0x07, 0x37, 0x83, 0x81, 0xe6];
    encoded_range.extend_from_slice(&lower_bits.to_le_bytes());
    encoded_range.push(0xe6);
    encoded_range.extend_from_slice(&upper_bits.to_le_bytes());
    encoded_range.extend_from_slice(&[0xfe, 0xfe]);
    let nominal_bits = 6.35_f64.to_bits();
    let mut suffix = vec![0x84, 0x96, 0x82, 0xdc, 0xe6];
    suffix.extend_from_slice(&nominal_bits.to_le_bytes());
    suffix.extend_from_slice(&[0x81, 0xdb]);

    let file = standard_catpart_with_range_interval(&encoded_range, &suffix);
    let native = crate::native::CatiaNative::decode(&file);
    let entity = &native.entity_records[0];
    assert!(entity.constraint_range.is_none());
    let range = entity
        .range_interval
        .as_ref()
        .expect("complete schema-selected range interval");
    assert_eq!(range.range.value, "Range");
    assert_eq!(
        range.interval.prefix,
        RangeIntervalPrefix::Compact { value: 7, width: 1 }
    );
    assert_eq!(
        range.interval.slots,
        Some([
            RangeIntervalSlot::Binary64 {
                bits: lower_bits,
                offset: 12,
            },
            RangeIntervalSlot::Binary64 {
                bits: upper_bits,
                offset: 21,
            },
        ])
    );
    let nominal = range.nominal.as_ref().expect("finite Range nominal");
    assert_eq!(nominal.framing, CatiaRangeNominalFraming::DCToken81DB);
    assert_eq!(nominal.bits, nominal_bits);
    assert_eq!(nominal.evaluation_opcode_offset, 4);

    let d8_nominal_bits = 12.7_f64.to_bits();
    let mut d8_suffix = vec![0x84, 0x96, 0x82, 0xd8, 0xe6];
    d8_suffix.extend_from_slice(&d8_nominal_bits.to_le_bytes());
    d8_suffix.extend_from_slice(&[0x81, 0xdb]);
    let d8_native = crate::native::CatiaNative::decode(&standard_catpart_with_range_interval(
        &encoded_range,
        &d8_suffix,
    ));
    let d8_range = d8_native.entity_records[0]
        .range_interval
        .as_ref()
        .expect("complete D8/81 DB Range interval");
    let d8_nominal = d8_range.nominal.as_ref().expect("finite D8 nominal");
    assert_eq!(d8_nominal.framing, CatiaRangeNominalFraming::D8Token81DB);
    assert_eq!(d8_nominal.bits, d8_nominal_bits);
    assert_eq!(d8_nominal.evaluation_opcode_offset, 4);

    let df_nominal_bits = 31.75_f64.to_bits();
    let mut df_suffix = vec![0x84, 0x96, 0x82, 0xdf, 0xe6];
    df_suffix.extend_from_slice(&df_nominal_bits.to_le_bytes());
    df_suffix.extend_from_slice(&[0x81, 0x92]);
    let df_native = crate::native::CatiaNative::decode(&standard_catpart_with_range_interval(
        &encoded_range,
        &df_suffix,
    ));
    let df_range = df_native.entity_records[0]
        .range_interval
        .as_ref()
        .expect("complete DF/81 92 Range interval");
    let df_nominal = df_range.nominal.as_ref().expect("finite DF nominal");
    assert_eq!(df_nominal.framing, CatiaRangeNominalFraming::DFToken8192);
    assert_eq!(df_nominal.bits, df_nominal_bits);
    assert_eq!(df_nominal.evaluation_opcode_offset, 4);

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode range interval");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RANGE_INTERVAL_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RANGE_INTERVAL_NO_SLOT_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RANGE_INTERVAL_NOMINAL_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RANGE_INTERVAL_FINITE_SLOT_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RANGE_INTERVAL_UNSET_SLOT_COUNT),
        0
    );
    let no_slot = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_range_interval(
                &[0x82, 0xe8, 0xe0, 0x07, 0x37, 0x81, 0xfe],
                &[],
            )),
            &DecodeOptions::default(),
        )
        .expect("decode no-slot range interval");
    assert_eq!(
        no_slot
            .report()
            .coverage_count(crate::coverage::DECODED_RANGE_INTERVAL_NO_SLOT_COUNT),
        1
    );
    assert_eq!(
        no_slot
            .report()
            .coverage_count(crate::coverage::DECODED_RANGE_INTERVAL_NOMINAL_COUNT),
        0
    );
    let unset = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_range_interval(
                &[
                    0x80, 0x6e, 0x89, 1, 0, 0xe8, 0xe0, 0x07, 0x37, 0x83, 0x81, 0xe8, 0xe8, 0xfe,
                    0xfe,
                ],
                &[],
            )),
            &DecodeOptions::default(),
        )
        .expect("decode unset range interval");
    assert_eq!(
        unset
            .report()
            .coverage_count(crate::coverage::DECODED_RANGE_INTERVAL_UNSET_SLOT_COUNT),
        2
    );

    let mut previous_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut previous_namespace)
        .expect("store range-interval namespace");
    previous_namespace.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_RANGE_INTERVAL_VERSION - 1).unwrap(),
    );
    previous_namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut()
        .remove("range_interval");
    let migrated = crate::native::CatiaNative::load(&previous_namespace)
        .expect("migrate range-interval production");
    assert_eq!(
        migrated.entity_records[0].range_interval,
        Some(range.clone())
    );

    let mut previous_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut previous_namespace)
        .expect("store pre-nominal range namespace");
    previous_namespace.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_RANGE_NOMINAL_VERSION - 1).unwrap(),
    );
    previous_namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut()
        .get_mut("range_interval")
        .expect("stored range interval")
        .as_object_mut()
        .expect("stored range interval object")
        .remove("nominal");
    let migrated =
        crate::native::CatiaNative::load(&previous_namespace).expect("migrate Range nominal");
    assert_eq!(
        migrated.entity_records[0]
            .range_interval
            .as_ref()
            .expect("migrated range interval")
            .nominal,
        Some(nominal.clone())
    );

    let mut malformed_nominal = native.clone();
    malformed_nominal.entity_records[0]
        .range_interval
        .as_mut()
        .expect("complete range interval")
        .nominal
        .as_mut()
        .expect("finite Range nominal")
        .bits = 12.0_f64.to_bits();
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed_nominal
        .store(&mut namespace)
        .expect("store malformed Range nominal");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = native;
    malformed.entity_records[0]
        .range_interval
        .as_mut()
        .expect("complete range interval")
        .interval
        .prefix = RangeIntervalPrefix::Compact { value: 8, width: 1 };
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed
        .store(&mut namespace)
        .expect("store malformed range interval");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn dimension_constraint_ranges_accept_db_terminated_dc_frames() {
    use crate::native::{
        CatiaConstraintRangeFraming, CatiaEntityEvaluation, CatiaEntitySuffixTrailer,
    };

    let bits = 15.875_f64.to_bits();
    let mut suffix = vec![0x84, 0x96, 0x82, 0xdc, 0xe6];
    suffix.extend_from_slice(&bits.to_le_bytes());
    suffix.extend_from_slice(&[0x81, 0xdb]);
    let file = standard_catpart_with_two_selector_value("Range", "CstAttr_Dimension", &suffix);
    let native = crate::native::CatiaNative::decode(&file);
    let entity = &native.entity_records[0];
    let range = entity
        .constraint_range
        .as_ref()
        .expect("DB-terminated dimension range");
    assert_eq!(range.framing, CatiaConstraintRangeFraming::DimensionDC);
    assert_eq!(range.evaluation, CatiaEntityEvaluation::Scalar { bits });
    assert_eq!(
        entity
            .suffix_value
            .as_ref()
            .expect("DB-terminated suffix value")
            .trailer,
        CatiaEntitySuffixTrailer::Token81DB
    );

    for suffix in [
        {
            let mut suffix = vec![0x84, 0x96, 0x82, 0xd8, 0xe6];
            suffix.extend_from_slice(&bits.to_le_bytes());
            suffix.extend_from_slice(&[0x81, 0xdb]);
            suffix
        },
        vec![0x84, 0x96, 0x82, 0xdc, 0xe7],
        vec![0x84, 0x96, 0x82, 0xc1, 0xe7, 0x81, 0xdb],
    ] {
        let native = crate::native::CatiaNative::decode(&standard_catpart_with_two_selector_value(
            "Range",
            "CstAttr_Dimension",
            &suffix,
        ));
        assert!(native.entity_records[0].constraint_range.is_none());
    }
}

#[test]
fn dimension_constraint_ranges_accept_8192_terminated_df_frames() {
    use crate::native::{
        CatiaConstraintRangeFraming, CatiaEntityEvaluation, CatiaEntitySuffixTrailer,
    };

    let bits = 22.225_f64.to_bits();
    let mut suffix = vec![0x84, 0x96, 0x82, 0xdf, 0xe6];
    suffix.extend_from_slice(&bits.to_le_bytes());
    suffix.extend_from_slice(&[0x81, 0x92]);
    let file = standard_catpart_with_two_selector_value("Range", "CstAttr_Dimension", &suffix);
    let native = crate::native::CatiaNative::decode(&file);
    let entity = &native.entity_records[0];
    let range = entity
        .constraint_range
        .as_ref()
        .expect("81 92-terminated dimension range");
    assert_eq!(range.framing, CatiaConstraintRangeFraming::DimensionDF);
    assert_eq!(range.evaluation, CatiaEntityEvaluation::Scalar { bits });
    assert_eq!(
        entity
            .suffix_value
            .as_ref()
            .expect("81 92-terminated suffix value")
            .trailer,
        CatiaEntitySuffixTrailer::Token8192
    );

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode 81 92-terminated dimension range");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DIMENSION_CONSTRAINT_RANGE_COUNT),
        1
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == crate::loss::CatiaLossCode::AttributesDimensionQuantityUnresolved.kind()
            && loss.message.contains("1 finite")
    }));

    for suffix in [
        {
            let mut suffix = vec![0x84, 0x96, 0x82, 0xdf, 0xe6];
            suffix.extend_from_slice(&bits.to_le_bytes());
            suffix.extend_from_slice(&[0x81, 0xdb]);
            suffix
        },
        {
            let mut suffix = vec![0x84, 0x96, 0x82, 0xdc, 0xe6];
            suffix.extend_from_slice(&bits.to_le_bytes());
            suffix.extend_from_slice(&[0x81, 0x92]);
            suffix
        },
    ] {
        let native = crate::native::CatiaNative::decode(&standard_catpart_with_two_selector_value(
            "Range",
            "CstAttr_Dimension",
            &suffix,
        ));
        assert!(native.entity_records[0].constraint_range.is_none());
    }
}

#[test]
fn constraint_range_requires_an_exact_role_and_framing_pair() {
    use crate::native::CatiaConstraintRangeFraming;

    for (constraint, code, expected) in [
        (
            "CstAttr_Dimension",
            0xb8,
            CatiaConstraintRangeFraming::DimensionB8,
        ),
        ("ComplexCst", 0xc9, CatiaConstraintRangeFraming::ComplexC9),
    ] {
        let native = crate::native::CatiaNative::decode(&standard_catpart_with_two_selector_value(
            "Range",
            constraint,
            &[0x84, 0x96, 0x82, code, 0xe7],
        ));
        assert_eq!(
            native.entity_records[0]
                .constraint_range
                .as_ref()
                .expect("complete constraint range")
                .framing,
            expected
        );
    }

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_two_selector_value(
                "Range",
                "ComplexCst",
                &[0x84, 0x96, 0x82, 0xc9, 0xe7],
            )),
            &DecodeOptions::default(),
        )
        .expect("decode unset complex constraint range");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSTRAINT_RANGE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DIMENSION_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_COMPLEX_CONSTRAINT_RANGE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_EVALUATED_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_UNSET_CONSTRAINT_RANGE_COUNT),
        1
    );

    for (range, constraint, code) in [
        ("Tolerance", "CstAttr_Dimension", 0xc1),
        ("Range", "ComplexCst", 0xc1),
        ("Range", "CstAttr_Dimension", 0xc9),
    ] {
        let native = crate::native::CatiaNative::decode(&standard_catpart_with_two_selector_value(
            range,
            constraint,
            &[0x84, 0x96, 0x82, code, 0xe7],
        ));
        assert!(native.entity_records[0].constraint_range.is_none());
    }
}
