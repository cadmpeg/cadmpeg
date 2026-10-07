// SPDX-License-Identifier: Apache-2.0

use crate::deltas::inline_schema_fields::InlineSchemaFields;

use crate::test_support::test_bytes::put_ref;
use crate::test_support::test_prt::prt_with_partition;
use crate::test_support::test_streams::topology_partition_stream;
use cadmpeg_core::CodecError;

use std::io::Cursor;

use crate::parasolid::Stream;

#[test]
fn type38_leading_statuses_preserve_default_omission_and_nondefault_values() {
    let base = serde_json::json!({
        "schema": "type38", "xmt": 3, "node_id": 7,
        "leading_references": [1, 2, 3, 4, 5], "marker": 45,
        "linked_references": [2, 3], "state_references": [6, 7, 8], "numeric_values": null
    });
    for statuses in [None, Some([1; 5]), Some([1, 1, 1, 1, 0])] {
        let mut wire = base.clone();
        if let Some(statuses) = statuses {
            wire["leading_statuses"] = serde_json::json!(statuses);
        }
        let fields: InlineSchemaFields = serde_json::from_value(wire.clone()).unwrap();
        let InlineSchemaFields::Type38 { state } = &fields else {
            panic!("type38 wire must decode as Type38");
        };
        assert_eq!(state.leading_statuses(), statuses.unwrap_or([1; 5]));
        if state.leading_statuses() == [1; 5] {
            wire.as_object_mut().unwrap().remove("leading_statuses");
        }
        assert_eq!(serde_json::to_value(fields).unwrap(), wire);
    }
    let mut null = base.clone();
    null["leading_statuses"] = serde_json::Value::Null;
    let fields: InlineSchemaFields = serde_json::from_value(null).unwrap();
    assert_eq!(serde_json::to_value(fields).unwrap(), base);
}

fn chart_record_route_limit_error(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> CodecError {
    let streams = [stream(
        crate::parasolid::ParasolidSubtype::Partition,
        "SCH_TEST",
        crate::test_support::test_streams::charted_intersection_curve_topology_partition_stream(),
    )];

    crate::test_support::with_decode_context_over(
        &streams[0].inflated,
        |policy| {
            configure(policy);
        },
        |ctx| {
            super::parasolid_chart_records(ctx, &streams)
                .expect_err("chart record route limit refusal")
        },
    )
}

#[test]
fn chart_record_route_refuses_collection_limit() {
    let error = chart_record_route_limit_error(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn chart_record_route_refuses_retained_limit() {
    let error = chart_record_route_limit_error(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn chart_record_route_refuses_scoped_limit() {
    let error = chart_record_route_limit_error(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn chart_record_route_refuses_work_limit() {
    let error = chart_record_route_limit_error(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

fn stream(subtype: crate::parasolid::ParasolidSubtype, schema: &str, inflated: Vec<u8>) -> Stream {
    Stream {
        file_offset: 0,
        consumed: 0,
        inflated,
        body: crate::parasolid::StreamBody::Parasolid {
            subtype,
            schema: Some(
                cadmpeg_parasolid::OwnedSchemaToken::parse(
                    &cadmpeg_test_support::service_decode_context(),
                    schema.into(),
                )
                .expect("service token admission")
                .expect("the fixture text is a schema token"),
            ),
        },
    }
}

#[test]
fn native_value_records_use_only_ledger_owned_offsets() {
    let mut outer = vec![0x00, 0x52];
    outer.extend_from_slice(&4u32.to_be_bytes());
    outer.extend_from_slice(&10u16.to_be_bytes());
    outer.extend_from_slice(&[0x00, 0x53]);
    outer.extend_from_slice(&1u32.to_be_bytes());
    outer.extend_from_slice(&20u16.to_be_bytes());
    outer.extend_from_slice(&0.25f64.to_be_bytes());

    let streams = [stream(
        crate::parasolid::ParasolidSubtype::Deltas,
        "SCH_TEST",
        outer,
    )];
    let events = super::parasolid_deltas_events(&streams);
    let records = crate::test_support::with_decode_context(|ctx| {
        super::parasolid_entity_value_records(ctx, &streams, &events.records)
    })
    .unwrap();

    assert_eq!(records.integers.len(), 1);
    assert_eq!(records.integers[0].values.as_slice().len(), 4);
    assert!(records.doubles.is_empty());
}

#[test]
fn native_value_records_refuse_collection_at_caller_limit() {
    crate::test_support::with_decode_context(|ctx| {
        let inflated = crate::test_support::test_streams::parasolid_entity_records_stream();
        let offset_count = crate::parasolid::referenced_value_record_offsets(ctx, &inflated)
            .unwrap()
            .0
            .len();
        assert_eq!(offset_count, 0);
        let candidates =
            crate::parasolid::value_records::entity_value_records(ctx, &inflated).unwrap();
        let candidate_count =
            candidates.integers.len() + candidates.doubles.len() + candidates.strings.len();
        assert_eq!(candidate_count, 3);
        let streams = [stream(
            crate::parasolid::ParasolidSubtype::Partition,
            "SCH_TEST",
            inflated,
        )];

        crate::test_support::with_decode_context_over(
            &streams[0].inflated,
            |policy| {
                policy.limits.max_collection_items =
                    cadmpeg_core::decode::u64_from_index(candidate_count);
            },
            |ctx| {
                let error = super::parasolid_entity_value_records(ctx, &streams, &[])
                    .err()
                    .expect("owned-value discovery exceeds the collection allowance");
                assert!(matches!(
                    error,
                    cadmpeg_core::CodecError::ResourceLimit(limit)
                        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                            && limit.operation == "NX attribute ownership groups"
                ));
            },
        );
    });
}

fn record(
    kind: u16,
    xmt: u32,
    node_id: Option<u32>,
    references: Vec<u32>,
) -> crate::deltas::Record {
    use crate::deltas::record_family::RecordFamily;
    let family = match kind {
        14 => RecordFamily::Face {
            references: [1; 11],
            node_id: node_id.expect("face node"),
        },
        16 => RecordFamily::Edge {
            references: [1; 8],
            node_id: node_id.expect("edge node"),
        },
        90 => RecordFamily::Group {
            references: references.try_into().unwrap(),
            node_id: node_id.expect("group node"),
            selector: GroupSelector::Form4,
            linked_reference_status: GroupReferenceStatus::Form0,
        },
        91 => RecordFamily::Type91 {
            references: references.try_into().unwrap(),
        },
        other => panic!("unexpected test kind {other}"),
    };
    crate::deltas::Record {
        family,
        xmt,
        canonical_bytes: if kind == 90 { vec![0, 90] } else { Vec::new() },
        offset: 0,
        end: 1,
    }
}

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::NxCodec;

use super::structured_value_kind::StructuredValueKind;
use super::topology_attribute_kind::TopologyAttributeKind;
use super::{
    parasolid_attribute_field_names, parasolid_attribute_field_uses,
    parasolid_entity_51_structured_uses,
    parasolid_topology_attribute_fields_have_untransferred_values, GroupReferenceStatus,
    ParasolidAttributeClassUse, ParasolidAttributeDefinition, ParasolidAttributeFieldUse,
    ParasolidAttributeFieldValueKind, ParasolidEntity51NumericKind, ParasolidEntity51NumericUse,
    ParasolidEntity51Record, ParasolidEntity51StringUse, ParasolidEntity51StructuredUse,
    ParasolidEntity54StringRecord, ParasolidEntity58TagRecord, ParasolidEntity62UnicodeRecord,
    ParasolidEntityVectorRecord, ParasolidFieldNamesRecord, ParasolidTopologyAttributeClassUse,
    ParasolidTopologyAttributeListReference, ParasolidVectorValueKind,
};
use crate::deltas::group::GroupSelector;
use crate::framing::xmt_reference::NonNullXmt;
use crate::framing::xmt_reference::XmtTarget;
use crate::parasolid::attribute_action::AttributeAction;
use crate::parasolid::attribute_field::AttributeField;
use crate::parasolid::entity_references::EntityReferences;
use crate::parasolid::name_references::NameReferences;
use crate::printable_string::PrintableString;
use std::num::NonZeroU32;

#[test]
fn attribute_value_uses_are_assigned_to_compatible_declared_fields() {
    crate::test_support::with_decode_context(|ctx| {
        let definition = ParasolidAttributeDefinition {
            id: "definition".into(),
            stream_ordinal: 2,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(9).unwrap(),
            next_definition_xmt: None,
            identifier_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(10).unwrap(),
            identifier_inflated_offset: 32,
            name: crate::printable_string::PrintableString::new("CLASS".to_string()).unwrap(),
            type_id: std::num::NonZeroU32::new(8000).unwrap(),
            action_codes: [AttributeAction::Code0; 8],
            field_names_xmt: None,
            legal_owner_flags: crate::parasolid::LegalOwnerFlags::Sixteen([false; 16]),

            field_codes: vec![
                AttributeField::Integer,
                AttributeField::Real,
                AttributeField::Character,
            ],
            inflated_offset: 40,
        };
        let class_use = ParasolidAttributeClassUse {
            inflated_offset: 30,
            id: "nx:s2:attribute-class-use#class-use".into(),
            stream_ordinal: 2,
            entity_51_record: "entity".into(),
            definition_xmt: NonNullXmt::try_from(9).unwrap(),
            attribute_definition: "definition".into(),
        };
        let numeric_use = ParasolidEntity51NumericUse {
            id: "numeric-use".into(),
            stream_ordinal: 2,
            entity_51_record: "entity".into(),
            position: crate::parasolid::entity_references::FieldPosition::try_from(5).unwrap(),
            referenced_xmt: NonNullXmt::try_from(12).unwrap(),
            kind: ParasolidEntity51NumericKind::UnsignedIntegers,
            value_record: "integers".into(),
            inflated_offset: 48,
        };
        let double_use = ParasolidEntity51NumericUse {
            id: "double-use".into(),
            position: crate::parasolid::entity_references::FieldPosition::try_from(6).unwrap(),
            kind: ParasolidEntity51NumericKind::Doubles,
            value_record: "doubles".into(),
            ..numeric_use.clone()
        };
        let string_use = ParasolidEntity51StringUse {
            id: "string-use".into(),
            stream_ordinal: 2,
            entity_51_record: "entity".into(),
            position: crate::parasolid::entity_references::FieldPosition::try_from(7).unwrap(),
            referenced_xmt: NonNullXmt::try_from(14).unwrap(),
            string_record: "string".into(),
            inflated_offset: 48,
        };

        let uses = parasolid_attribute_field_uses(
            ctx,
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&definition),
            &[numeric_use.clone(), double_use],
            std::slice::from_ref(&string_use),
            &[],
        )
        .unwrap();

        assert_eq!(uses.len(), 3);
        assert_eq!(uses[0].id, "nx:s2:attribute-field-use#class-use-0");
        assert_eq!(
            uses[0].attribute_class_use,
            "nx:s2:attribute-class-use#class-use"
        );
        assert_eq!(uses[0].attribute_definition, "definition");
        assert_eq!(uses[0].position.field_ordinal(), 0);
        assert_eq!(uses[0].value_kind.field_code().code(), 1);
        assert_eq!(uses[0].position.reference_ordinal(), 5);
        assert_eq!(
            uses[0].value_kind,
            ParasolidAttributeFieldValueKind::UnsignedIntegers
        );
        assert_eq!(uses[0].value_use, "numeric-use");
        assert_eq!(uses[0].value_record, "integers");
        assert_eq!(uses[1].position.field_ordinal(), 1);
        assert_eq!(uses[1].value_kind.field_code().code(), 2);
        assert_eq!(
            uses[1].value_kind,
            ParasolidAttributeFieldValueKind::Doubles
        );
        assert_eq!(uses[1].value_record, "doubles");
        assert_eq!(uses[2].position.field_ordinal(), 2);
        assert_eq!(uses[2].value_kind.field_code().code(), 3);
        assert_eq!(uses[2].value_kind, ParasolidAttributeFieldValueKind::String);
        assert_eq!(uses[2].value_record, "string");

        let duplicate = ParasolidAttributeClassUse {
            inflated_offset: 30,
            id: "duplicate".into(),
            stream_ordinal: 2,
            entity_51_record: "entity".into(),
            definition_xmt: NonNullXmt::try_from(11).unwrap(),
            attribute_definition: "other-definition".into(),
        };
        assert!(parasolid_attribute_field_uses(
            ctx,
            &[class_use.clone(), duplicate.clone()],
            std::slice::from_ref(&definition),
            std::slice::from_ref(&numeric_use),
            &[],
            &[],
        )
        .unwrap()
        .is_empty());

        let wrong_stream = ParasolidAttributeClassUse {
            stream_ordinal: 3,
            ..duplicate
        };
        assert!(parasolid_attribute_field_uses(
            ctx,
            &[wrong_stream],
            std::slice::from_ref(&definition),
            std::slice::from_ref(&numeric_use),
            &[],
            &[],
        )
        .unwrap()
        .is_empty());

        let mismatched = ParasolidEntity51NumericUse {
            kind: ParasolidEntity51NumericKind::Doubles,
            ..numeric_use.clone()
        };
        assert!(parasolid_attribute_field_uses(
            ctx,
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&definition),
            &[mismatched],
            &[],
            &[],
        )
        .unwrap()
        .is_empty());

        let ambiguous_string = ParasolidEntity51StringUse {
            position: crate::parasolid::entity_references::FieldPosition::try_from(5).unwrap(),
            ..string_use
        };
        assert!(parasolid_attribute_field_uses(
            ctx,
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&definition),
            std::slice::from_ref(&numeric_use),
            &[ambiguous_string],
            &[],
        )
        .unwrap()
        .is_empty());
    });
}

#[test]
fn structured_value_uses_require_one_same_stream_family() {
    let entity = ParasolidEntity51Record {
        id: "entity".into(),
        stream_ordinal: 2,
        xmt: NonNullXmt::try_from(10).unwrap(),
        sequence: NonZeroU32::new(1).unwrap(),
        definition_xmt: 9,
        leading_references: [1; 5],
        trailing_references: EntityReferences::new(vec![12]).unwrap(),
        byte_len: 32,
        inflated_offset: 40,
    };
    let point = ParasolidEntityVectorRecord {
        id: "point".into(),
        stream_ordinal: 2,
        kind: ParasolidVectorValueKind::Points,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(12).unwrap(),
        values: crate::parasolid::counted_values::CountedValues::new(vec![[1.0, 2.0, 3.0]])
            .unwrap(),
        byte_len: 36,
        inflated_offset: 80,
    };
    let uses = crate::test_support::with_decode_context(|ctx| {
        parasolid_entity_51_structured_uses(
            ctx,
            std::slice::from_ref(&entity),
            std::slice::from_ref(&point),
            &[],
            &[],
            &[],
        )
        .unwrap()
    });
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].position.reference_ordinal(), 5);
    assert_eq!(uses[0].kind, StructuredValueKind::Points);
    assert_eq!(uses[0].value_record, "point");

    let colliding_tag = ParasolidEntity58TagRecord {
        id: "tag".into(),
        stream_ordinal: 2,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(12).unwrap(),
        values: crate::parasolid::counted_values::CountedValues::new(vec![7]).unwrap(),
        byte_len: 16,
        inflated_offset: 90,
    };
    assert!(crate::test_support::with_decode_context(|ctx| {
        parasolid_entity_51_structured_uses(
            ctx,
            std::slice::from_ref(&entity),
            std::slice::from_ref(&point),
            &[],
            std::slice::from_ref(&colliding_tag),
            &[],
        )
        .unwrap()
    })
    .is_empty());

    let other_stream = ParasolidEntityVectorRecord {
        stream_ordinal: 3,
        ..point
    };
    assert!(crate::test_support::with_decode_context(|ctx| {
        parasolid_entity_51_structured_uses(ctx, &[entity], &[other_stream], &[], &[], &[]).unwrap()
    })
    .is_empty());
}

#[test]
fn structured_value_families_match_only_their_declared_field_codes() {
    crate::test_support::with_decode_context(|ctx| {
        let kinds = [
            StructuredValueKind::Points,
            StructuredValueKind::Vectors,
            StructuredValueKind::Directions,
            StructuredValueKind::Axes,
            StructuredValueKind::Tags,
            StructuredValueKind::Unicode,
        ];
        let definition = ParasolidAttributeDefinition {
            id: "definition".into(),
            stream_ordinal: 2,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(9).unwrap(),
            next_definition_xmt: None,
            identifier_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(10).unwrap(),
            identifier_inflated_offset: 32,
            name: crate::printable_string::PrintableString::new("CLASS".to_string()).unwrap(),
            type_id: std::num::NonZeroU32::new(8000).unwrap(),
            action_codes: [AttributeAction::Code0; 8],
            field_names_xmt: None,
            legal_owner_flags: crate::parasolid::LegalOwnerFlags::Sixteen([false; 16]),

            field_codes: vec![
                AttributeField::Point,
                AttributeField::Vector,
                AttributeField::Direction,
                AttributeField::Axis,
                AttributeField::Tag,
                AttributeField::Unicode,
            ],
            inflated_offset: 40,
        };
        let class_use = ParasolidAttributeClassUse {
            inflated_offset: 30,
            id: "nx:s2:attribute-class-use#class-use".into(),
            stream_ordinal: 2,
            entity_51_record: "entity".into(),
            definition_xmt: NonNullXmt::try_from(9).unwrap(),
            attribute_definition: definition.id.clone(),
        };
        let structured = kinds
            .iter()
            .enumerate()
            .map(|(ordinal, kind)| ParasolidEntity51StructuredUse {
                id: format!("use-{ordinal}"),
                stream_ordinal: 2,
                entity_51_record: "entity".into(),
                position: crate::parasolid::entity_references::FieldPosition::try_from(
                    u32::try_from(ordinal).expect("test ordinal fits u32") + 5,
                )
                .unwrap(),
                referenced_xmt: NonNullXmt::try_from(
                    u32::try_from(ordinal).expect("test ordinal fits u32") + 20,
                )
                .unwrap(),
                kind: *kind,
                value_record: format!("value-{ordinal}"),
                inflated_offset: 48,
            })
            .collect::<Vec<_>>();
        let uses = parasolid_attribute_field_uses(
            ctx,
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&definition),
            &[],
            &[],
            &structured,
        )
        .unwrap();
        assert_eq!(
            uses.iter().map(|use_| use_.value_kind).collect::<Vec<_>>(),
            kinds.map(ParasolidAttributeFieldValueKind::from)
        );

        let mut mismatched = structured;
        mismatched[0].kind = StructuredValueKind::Vectors;
        let uses =
            parasolid_attribute_field_uses(ctx, &[class_use], &[definition], &[], &[], &mismatched)
                .unwrap();
        assert_eq!(uses.len(), 5);
        assert!(uses.iter().all(|use_| use_.position.field_ordinal() != 0));
    });
}

#[test]
fn attribute_loss_requires_concrete_unresolved_references() {
    crate::test_support::with_decode_context(|ctx| {
        let definition = |field_names_xmt, field_codes: Vec<u8>| ParasolidAttributeDefinition {
            id: "definition".into(),
            stream_ordinal: 0,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(20).unwrap(),
            next_definition_xmt: None,
            identifier_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(21).unwrap(),
            identifier_inflated_offset: 10,
            name: crate::printable_string::PrintableString::new("CLASS".to_string()).unwrap(),
            type_id: std::num::NonZeroU32::new(8000).unwrap(),
            action_codes: [AttributeAction::Code0; 8],
            field_names_xmt: XmtTarget::from_wire(field_names_xmt),
            legal_owner_flags: crate::parasolid::LegalOwnerFlags::Sixteen([false; 16]),

            field_codes: field_codes
                .into_iter()
                .map(|code| AttributeField::try_from(code).unwrap())
                .collect(),
            inflated_offset: 20,
        };

        let entity = ParasolidEntity51Record {
            id: "entity".into(),
            stream_ordinal: 0,
            xmt: NonNullXmt::try_from(30).unwrap(),
            sequence: NonZeroU32::new(1).unwrap(),
            definition_xmt: 20,
            leading_references: [1; 5],
            trailing_references: EntityReferences::new(vec![40]).unwrap(),
            byte_len: 32,
            inflated_offset: 30,
        };
        let class_use = ParasolidAttributeClassUse {
            inflated_offset: 30,
            id: "class-use".into(),
            stream_ordinal: 0,
            entity_51_record: entity.id.clone(),
            definition_xmt: NonNullXmt::try_from(20).unwrap(),
            attribute_definition: "definition".into(),
        };
        let field_use = ParasolidAttributeFieldUse {
            id: "field-use".into(),
            stream_ordinal: 0,
            attribute_class_use: class_use.id.clone(),
            entity_51_record: entity.id.clone(),
            attribute_definition: "definition".into(),
            position: crate::parasolid::entity_references::FieldPosition::try_from(5).unwrap(),
            value_kind: ParasolidAttributeFieldValueKind::Points,
            value_use: "point-use".into(),
            value_record: "points".into(),
            inflated_offset: 30,
        };
        let topology_reference = ParasolidTopologyAttributeListReference {
            id: "topology-reference".into(),
            stream_ordinal: 0,
            topology_type: TopologyAttributeKind::Face,
            topology_xmt: 50,
            attribute_list_xmt: entity.xmt.into(),
            attribute_list_record: Some(entity.id.clone()),
            inflated_offset: 28,
        };
        let topology_class_use = ParasolidTopologyAttributeClassUse {
            stream_ordinal: topology_reference.stream_ordinal,
            inflated_offset: entity.inflated_offset,
            id: "topology-class-use".into(),
            topology_attribute_reference: topology_reference.id.clone(),
            entity_51_record: entity.id.clone(),
            attribute_class_use: class_use.id.clone(),
            definition_xmt: NonNullXmt::try_from(20).unwrap(),
            attribute_definition: "definition".into(),
        };

        // An unused declaration carries no value-loss evidence.
        assert!(
            !parasolid_topology_attribute_fields_have_untransferred_values(
                ctx,
                &[definition(1, vec![4])],
                &[],
                &[],
                &[],
            )
            .unwrap()
        );
        // A non-null instance reference must have exactly one resolved field use.
        assert!(
            parasolid_topology_attribute_fields_have_untransferred_values(
                ctx,
                &[definition(1, vec![4])],
                std::slice::from_ref(&entity),
                &[],
                std::slice::from_ref(&topology_class_use),
            )
            .unwrap()
        );
        assert!(
            !parasolid_topology_attribute_fields_have_untransferred_values(
                ctx,
                &[definition(1, vec![4])],
                std::slice::from_ref(&entity),
                std::slice::from_ref(&field_use),
                std::slice::from_ref(&topology_class_use),
            )
            .unwrap()
        );
        // Null values and always-empty pointer fields require no value relation.
        let mut null_entity = entity.clone();
        null_entity.trailing_references.values_mut()[0] = 1;
        assert!(
            !parasolid_topology_attribute_fields_have_untransferred_values(
                ctx,
                &[definition(1, vec![4])],
                &[null_entity],
                &[],
                std::slice::from_ref(&topology_class_use),
            )
            .unwrap()
        );
        assert!(
            !parasolid_topology_attribute_fields_have_untransferred_values(
                ctx,
                &[definition(1, vec![9])],
                std::slice::from_ref(&entity),
                &[],
                std::slice::from_ref(&topology_class_use),
            )
            .unwrap()
        );

        let named_definition = definition(22, vec![4]);
        // An unresolved optional field-name list uses the specification's
        // deterministic ordinal/code fallback and does not lose the value.
        assert!(
            !parasolid_topology_attribute_fields_have_untransferred_values(
                ctx,
                std::slice::from_ref(&named_definition),
                std::slice::from_ref(&entity),
                std::slice::from_ref(&field_use),
                std::slice::from_ref(&topology_class_use),
            )
            .unwrap()
        );
        assert!(
            parasolid_topology_attribute_fields_have_untransferred_values(
                ctx,
                &[named_definition],
                std::slice::from_ref(&entity),
                &[],
                std::slice::from_ref(&topology_class_use),
            )
            .unwrap()
        );
    });
}

fn topology_attribute_validation_limit_error(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> CodecError {
    let definition = ParasolidAttributeDefinition {
        id: "definition".into(),
        stream_ordinal: 0,
        xmt: NonNullXmt::try_from(20).unwrap(),
        next_definition_xmt: None,
        identifier_xmt: NonNullXmt::try_from(21).unwrap(),
        identifier_inflated_offset: 10,
        name: PrintableString::new("CLASS".to_owned()).unwrap(),
        type_id: NonZeroU32::new(8000).unwrap(),
        action_codes: [AttributeAction::Code0; 8],
        field_names_xmt: None,
        legal_owner_flags: crate::parasolid::LegalOwnerFlags::Sixteen([false; 16]),
        field_codes: vec![AttributeField::Point],
        inflated_offset: 20,
    };
    let entity = ParasolidEntity51Record {
        id: "entity".into(),
        stream_ordinal: 0,
        xmt: NonNullXmt::try_from(30).unwrap(),
        sequence: NonZeroU32::new(1).unwrap(),
        definition_xmt: 20,
        leading_references: [1; 5],
        trailing_references: EntityReferences::new(vec![40]).unwrap(),
        byte_len: 32,
        inflated_offset: 30,
    };
    let class_use = ParasolidTopologyAttributeClassUse {
        id: "topology-class-use".into(),
        topology_attribute_reference: "topology-reference".into(),
        entity_51_record: entity.id.clone(),
        attribute_class_use: "class-use".into(),
        definition_xmt: definition.xmt,
        attribute_definition: definition.id.clone(),
        stream_ordinal: 0,
        inflated_offset: 30,
    };

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            parasolid_topology_attribute_fields_have_untransferred_values(
                ctx,
                &[definition],
                &[entity],
                &[],
                &[class_use],
            )
            .expect_err("topology attribute validation limit refusal")
        },
    )
}

fn attribute_class_use_limit_error(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> CodecError {
    let definition = ParasolidAttributeDefinition {
        id: "definition".into(),
        stream_ordinal: 0,
        xmt: NonNullXmt::try_from(20).unwrap(),
        next_definition_xmt: None,
        identifier_xmt: NonNullXmt::try_from(21).unwrap(),
        identifier_inflated_offset: 10,
        name: PrintableString::new("CLASS".to_owned()).unwrap(),
        type_id: NonZeroU32::new(8000).unwrap(),
        action_codes: [AttributeAction::Code0; 8],
        field_names_xmt: None,
        legal_owner_flags: crate::parasolid::LegalOwnerFlags::Sixteen([false; 16]),
        field_codes: vec![AttributeField::Point],
        inflated_offset: 20,
    };
    let entity = ParasolidEntity51Record {
        id: "entity".into(),
        stream_ordinal: 0,
        xmt: NonNullXmt::try_from(30).unwrap(),
        sequence: NonZeroU32::new(1).unwrap(),
        definition_xmt: 20,
        leading_references: [1; 5],
        trailing_references: EntityReferences::new(vec![40]).unwrap(),
        byte_len: 32,
        inflated_offset: 30,
    };

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            super::parasolid_attribute_class_uses(ctx, &[entity], &[definition])
                .expect_err("attribute class use limit refusal")
        },
    )
}

fn attribute_field_use_limit_error(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> CodecError {
    let definition = ParasolidAttributeDefinition {
        id: "definition".into(),
        stream_ordinal: 0,
        xmt: NonNullXmt::try_from(20).unwrap(),
        next_definition_xmt: None,
        identifier_xmt: NonNullXmt::try_from(21).unwrap(),
        identifier_inflated_offset: 10,
        name: PrintableString::new("CLASS".to_owned()).unwrap(),
        type_id: NonZeroU32::new(8000).unwrap(),
        action_codes: [AttributeAction::Code0; 8],
        field_names_xmt: None,
        legal_owner_flags: crate::parasolid::LegalOwnerFlags::Sixteen([false; 16]),
        field_codes: vec![AttributeField::Integer],
        inflated_offset: 20,
    };
    let class_use = ParasolidAttributeClassUse {
        id: "nx:s0:attribute-class-use#class".into(),
        stream_ordinal: 0,
        entity_51_record: "entity".into(),
        definition_xmt: definition.xmt,
        attribute_definition: definition.id.clone(),
        inflated_offset: 30,
    };
    let numeric_use = ParasolidEntity51NumericUse {
        id: "numeric".into(),
        stream_ordinal: 0,
        entity_51_record: "entity".into(),
        position: crate::parasolid::entity_references::FieldPosition::try_from(5).unwrap(),
        referenced_xmt: NonNullXmt::try_from(40).unwrap(),
        kind: ParasolidEntity51NumericKind::UnsignedIntegers,
        value_record: "value".into(),
        inflated_offset: 30,
    };

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            super::parasolid_attribute_field_uses(
                ctx,
                &[class_use],
                &[definition],
                &[numeric_use],
                &[],
                &[],
            )
            .expect_err("attribute field use limit refusal")
        },
    )
}

#[test]
fn attribute_field_use_refuses_collection_limit() {
    let error = attribute_field_use_limit_error(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn attribute_field_use_refuses_retained_limit() {
    let error = attribute_field_use_limit_error(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn attribute_field_use_refuses_scoped_limit() {
    let error = attribute_field_use_limit_error(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn attribute_field_use_refuses_work_limit() {
    let error = attribute_field_use_limit_error(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

fn topology_class_use_limit_error(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> CodecError {
    let entity = ParasolidEntity51Record {
        id: "head".into(),
        stream_ordinal: 0,
        xmt: NonNullXmt::try_from(30).unwrap(),
        sequence: NonZeroU32::new(1).unwrap(),
        definition_xmt: 20,
        leading_references: [40, 1, 1, 1, 1],
        trailing_references: EntityReferences::new(vec![50]).unwrap(),
        byte_len: 32,
        inflated_offset: 30,
    };
    let class_use = ParasolidAttributeClassUse {
        id: "class".into(),
        stream_ordinal: 0,
        entity_51_record: entity.id.clone(),
        definition_xmt: NonNullXmt::try_from(20).unwrap(),
        attribute_definition: "definition".into(),
        inflated_offset: 30,
    };
    let reference = ParasolidTopologyAttributeListReference {
        id: "reference".into(),
        stream_ordinal: 0,
        topology_type: TopologyAttributeKind::Face,
        topology_xmt: 40,
        attribute_list_xmt: 30,
        attribute_list_record: Some(entity.id.clone()),
        inflated_offset: 80,
    };

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            super::parasolid_topology_attribute_class_uses(
                ctx,
                &[reference],
                &[entity],
                &[class_use],
            )
            .expect_err("topology attribute class use limit refusal")
        },
    )
}

#[test]
fn topology_class_use_refuses_collection_limit() {
    let error = topology_class_use_limit_error(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn topology_class_use_refuses_retained_limit() {
    let error = topology_class_use_limit_error(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn topology_class_use_refuses_scoped_limit() {
    let error = topology_class_use_limit_error(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn topology_class_use_refuses_work_limit() {
    let error = topology_class_use_limit_error(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

#[test]
fn attribute_class_use_refuses_collection_limit() {
    let error = attribute_class_use_limit_error(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn attribute_class_use_refuses_retained_limit() {
    let error = attribute_class_use_limit_error(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn attribute_class_use_refuses_scoped_limit() {
    let error = attribute_class_use_limit_error(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn attribute_class_use_refuses_work_limit() {
    let error = attribute_class_use_limit_error(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

#[test]
fn topology_attribute_validation_refuses_collection_limit() {
    let error = topology_attribute_validation_limit_error(|policy| {
        policy.limits.max_collection_items = 0;
    });
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn topology_attribute_validation_refuses_scoped_limit() {
    let error = topology_attribute_validation_limit_error(|policy| {
        policy.limits.max_materialized_bytes = 0;
    });
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn topology_attribute_validation_refuses_work_limit() {
    let error =
        topology_attribute_validation_limit_error(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

#[test]
fn attribute_field_names_require_complete_unambiguous_same_stream_relations() {
    crate::test_support::with_decode_context(|ctx| {
        let definition = ParasolidAttributeDefinition {
            id: "definition".into(),
            stream_ordinal: 3,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(20).unwrap(),
            next_definition_xmt: None,
            identifier_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(21).unwrap(),
            identifier_inflated_offset: 10,
            name: crate::printable_string::PrintableString::new("CLASS".to_string()).unwrap(),
            type_id: std::num::NonZeroU32::new(8000).unwrap(),
            action_codes: [AttributeAction::Code0; 8],
            field_names_xmt: XmtTarget::from_wire(25),
            legal_owner_flags: crate::parasolid::LegalOwnerFlags::Sixteen([false; 16]),

            field_codes: vec![
                AttributeField::Real,
                AttributeField::Integer,
                AttributeField::Integer,
            ],
            inflated_offset: 20,
        };
        let list = ParasolidFieldNamesRecord {
            id: "field-names".into(),
            stream_ordinal: 3,
            xmt: NonNullXmt::try_from(25).unwrap(),
            name_xmts: NameReferences::try_from(
                [28, 29, 30]
                    .map(|xmt| NonNullXmt::try_from(xmt).unwrap())
                    .to_vec(),
            )
            .unwrap(),
            byte_len: 15,
            inflated_offset: 30,
        };
        let strings = [28, 30].map(|xmt| ParasolidEntity54StringRecord {
            id: format!("string-{xmt}"),
            stream_ordinal: 3,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(xmt).unwrap(),
            value: PrintableString::new((xmt - 27).to_string()).unwrap(),
            byte_len: 10,
            inflated_offset: u64::from(xmt),
        });
        let unicode = ParasolidEntity62UnicodeRecord {
            id: "unicode-29".into(),
            stream_ordinal: 3,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(29).unwrap(),
            value: crate::parasolid::unicode_value::UnicodeValue::new("μ".into()).unwrap(),
            byte_len: 12,
            inflated_offset: 29,
        };

        let relations = parasolid_attribute_field_names(
            ctx,
            std::slice::from_ref(&definition),
            std::slice::from_ref(&list),
            &strings,
            std::slice::from_ref(&unicode),
        )
        .unwrap();
        assert_eq!(relations.len(), 1);
        assert_eq!(
            relations[0]
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            ["1", "μ", "3"]
        );

        let mut incomplete = list.clone();
        incomplete.name_xmts =
            NameReferences::try_from(incomplete.name_xmts.as_slice()[..2].to_vec()).unwrap();
        assert!(parasolid_attribute_field_names(
            ctx,
            std::slice::from_ref(&definition),
            &[incomplete],
            &strings,
            std::slice::from_ref(&unicode),
        )
        .unwrap()
        .is_empty());
        assert!(parasolid_attribute_field_names(
            ctx,
            &[definition.clone(), definition.clone()],
            std::slice::from_ref(&list),
            &strings,
            std::slice::from_ref(&unicode),
        )
        .unwrap()
        .is_empty());

        let ambiguous = ParasolidEntity62UnicodeRecord {
            xmt: NonNullXmt::try_from(28).unwrap(),
            ..unicode.clone()
        };
        assert!(parasolid_attribute_field_names(
            ctx,
            std::slice::from_ref(&definition),
            &[ParasolidFieldNamesRecord {
                id: "field-names".into(),
                stream_ordinal: 3,
                xmt: NonNullXmt::try_from(25).unwrap(),
                name_xmts: NameReferences::try_from(
                    [28, 29, 30]
                        .map(|xmt| NonNullXmt::try_from(xmt).unwrap())
                        .to_vec()
                )
                .unwrap(),
                byte_len: 15,
                inflated_offset: 30,
            }],
            &strings,
            &[unicode, ambiguous],
        )
        .unwrap()
        .is_empty());
    });
}

fn attribute_field_names_limit_error(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> CodecError {
    let definition = ParasolidAttributeDefinition {
        id: "definition".into(),
        stream_ordinal: 0,
        xmt: NonNullXmt::try_from(20).unwrap(),
        next_definition_xmt: None,
        identifier_xmt: NonNullXmt::try_from(21).unwrap(),
        identifier_inflated_offset: 10,
        name: PrintableString::new("CLASS".to_owned()).unwrap(),
        type_id: NonZeroU32::new(8000).unwrap(),
        action_codes: [AttributeAction::Code0; 8],
        field_names_xmt: XmtTarget::from_wire(25),
        legal_owner_flags: crate::parasolid::LegalOwnerFlags::Sixteen([false; 16]),
        field_codes: vec![AttributeField::Integer],
        inflated_offset: 20,
    };
    let list = ParasolidFieldNamesRecord {
        id: "list".into(),
        stream_ordinal: 0,
        xmt: NonNullXmt::try_from(25).unwrap(),
        name_xmts: NameReferences::try_from(vec![NonNullXmt::try_from(28).unwrap()]).unwrap(),
        byte_len: 8,
        inflated_offset: 30,
    };
    let value = ParasolidEntity54StringRecord {
        id: "value".into(),
        stream_ordinal: 0,
        xmt: NonNullXmt::try_from(28).unwrap(),
        value: PrintableString::new("field".to_owned()).unwrap(),
        byte_len: 10,
        inflated_offset: 40,
    };

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            parasolid_attribute_field_names(ctx, &[definition], &[list], &[value], &[])
                .expect_err("attribute field-name relation limit refusal")
        },
    )
}

fn topology_list_reference_limit_error(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> CodecError {
    let mut stream = topology_partition_stream();
    let prefix_len = b"PS\x00\x00".len()
        + b"XX: TRANSMIT FILE (partition) created by modeller\x00SCH_TEST_1_9999\x00".len();
    put_ref(&mut stream, prefix_len + 24 + 8, 50);
    let bytes = prt_with_partition(&stream);

    crate::test_support::with_decode_context_over(
        &bytes,
        |_| {},
        |ctx| {
            let root = cadmpeg_core::decode::View::over_retained(&bytes);

            let scan = crate::decode::scan(ctx, root).unwrap();
            let parsed = crate::native::substrate::ParsedStreams::parse(ctx, &scan).unwrap();
            let entity = super::ParasolidEntity51Record {
                id: "entity".into(),
                stream_ordinal: 0,
                xmt: NonNullXmt::try_from(50).unwrap(),
                sequence: NonZeroU32::new(1).unwrap(),
                definition_xmt: 20,
                leading_references: [1; 5],
                trailing_references: EntityReferences::new(vec![1]).unwrap(),
                byte_len: 32,
                inflated_offset: 40,
            };

            crate::test_support::with_decode_context_over(
                &bytes,
                |policy| {
                    configure(policy);
                },
                |limited_ctx| {
                    super::parasolid_topology_attribute_list_references(
                        limited_ctx,
                        &parsed,
                        &[entity],
                    )
                    .expect_err("topology attribute list reference limit refusal")
                },
            )
        },
    )
}

#[test]
fn topology_list_reference_refuses_collection_limit() {
    let error =
        topology_list_reference_limit_error(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn topology_list_reference_refuses_retained_limit() {
    let error = topology_list_reference_limit_error(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn topology_list_reference_refuses_scoped_limit() {
    let error =
        topology_list_reference_limit_error(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn topology_list_reference_refuses_work_limit() {
    let error = topology_list_reference_limit_error(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

#[test]
fn attribute_field_names_refuse_collection_limit() {
    let error = attribute_field_names_limit_error(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn attribute_field_names_refuse_retained_limit() {
    let error = attribute_field_names_limit_error(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn attribute_field_names_refuse_scoped_limit() {
    let error =
        attribute_field_names_limit_error(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn attribute_field_names_refuse_work_limit() {
    let error = attribute_field_names_limit_error(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

#[test]
fn topology_retains_entity_attribute_list_references() {
    let mut stream = topology_partition_stream();
    for (kind, attribute) in [(14, 41), (15, 42), (17, 43), (16, 44), (18, 45)] {
        let at = stream
            .windows(2)
            .position(|window| window == [0, kind])
            .expect("topology record");
        put_ref(&mut stream, at + if kind == 17 { 4 } else { 8 }, attribute);
    }
    stream.extend_from_slice(&[0, 0x51]);
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&41u16.to_be_bytes());
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&0x21u16.to_be_bytes());
    for reference in [4u16, 1, 1, 1, 1, 42] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream.extend_from_slice(&[0, 0x54]);
    stream.extend_from_slice(&8u32.to_be_bytes());
    stream.extend_from_slice(&42u16.to_be_bytes());
    stream.extend_from_slice(b"deadbeef\0");

    let graph =
        crate::test_support::with_decode_context(|ctx| crate::topology::Graph::parse(ctx, &stream))
            .unwrap();
    assert_eq!(
        graph
            .node(crate::framing::node_kind::NodeKind::Face, 4)
            .expect("required invariant")
            .face_fields()
            .expect("required invariant")
            .attributes
            .map(u32::from)
            .expect("non-null attribute target"),
        41
    );
    assert_eq!(
        graph
            .node(crate::framing::node_kind::NodeKind::Loop, 5)
            .expect("required invariant")
            .loop_fields()
            .expect("required invariant")
            .attributes
            .map(u32::from)
            .expect("non-null attribute target"),
        42
    );
    assert_eq!(
        graph
            .node(crate::framing::node_kind::NodeKind::Fin, 7)
            .expect("required invariant")
            .fin_fields()
            .expect("required invariant")
            .attributes
            .map(u32::from)
            .expect("non-null attribute target"),
        43
    );
    assert_eq!(
        graph
            .node(crate::framing::node_kind::NodeKind::Edge, 8)
            .expect("required invariant")
            .edge_fields()
            .expect("required invariant")
            .attributes
            .map(u32::from)
            .expect("non-null attribute target"),
        44
    );
    assert_eq!(
        graph
            .node(crate::framing::node_kind::NodeKind::Vertex, 10)
            .expect("required invariant")
            .vertex_fields()
            .expect("required invariant")
            .attributes
            .map(u32::from)
            .expect("non-null attribute target"),
        45
    );

    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_partition(&stream)),
            &DecodeOptions::default(),
        )
        .expect("required invariant");
    let references = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::ParasolidTopologyAttributeListReference>(
            "parasolid_topology_attribute_list_references",
        )
        .expect("required invariant");
    assert_eq!(references.len(), 5);
    assert_eq!(references[0].topology_type.code(), 14);
    assert_eq!(references[0].topology_xmt, 4);
    assert_eq!(references[0].attribute_list_xmt, 41);
    assert!(references[0].attribute_list_record.is_some());
    assert_eq!(result.ir().model.attributes.len(), 1);
    assert_eq!(
        result.ir().model.attributes[0].target,
        cadmpeg_ir::attributes::AttributeTarget::Face(
            cadmpeg_ir::ids::FaceId::mint("nx:s0:face#4").expect("identity grammar")
        )
    );
    assert_eq!(
        result.ir().model.attributes[0].name,
        "parasolid_type_84_reference_5"
    );
    assert_eq!(
        result.ir().model.attributes[0].values,
        [cadmpeg_ir::attributes::AttributeValue::String(
            "deadbeef".into()
        )]
    );
}

#[test]
fn topology_attribute_class_uses_resolve_type_80_definitions_by_xmt() {
    use super::{
        ParasolidAttributeDefinition, ParasolidEntity51Record,
        ParasolidTopologyAttributeListReference,
    };

    let definition = ParasolidAttributeDefinition {
        id: "definition".into(),
        stream_ordinal: 3,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(34).unwrap(),
        next_definition_xmt: None,
        identifier_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(35).unwrap(),
        identifier_inflated_offset: 80,
        name: crate::printable_string::PrintableString::new("UG2/PMARK_ATTRIBUTE".to_string())
            .unwrap(),
        type_id: std::num::NonZeroU32::new(9000).unwrap(),
        action_codes: [AttributeAction::Code0; 8],
        field_names_xmt: None,
        legal_owner_flags: crate::parasolid::LegalOwnerFlags::Sixteen([false; 16]),

        field_codes: vec![AttributeField::Integer],
        inflated_offset: 100,
    };
    let entity = ParasolidEntity51Record {
        id: "entity".into(),
        stream_ordinal: 3,
        xmt: NonNullXmt::try_from(50).unwrap(),
        sequence: NonZeroU32::new(7).unwrap(),
        definition_xmt: 34,
        leading_references: [60, 61, 1, 62, 63],
        trailing_references: EntityReferences::new(vec![64]).unwrap(),
        byte_len: 26,
        inflated_offset: 200,
    };
    let reference = ParasolidTopologyAttributeListReference {
        id: "topology-reference".into(),
        stream_ordinal: 3,
        topology_type: TopologyAttributeKind::Face,
        topology_xmt: 60,
        attribute_list_xmt: 50,
        attribute_list_record: Some(entity.id.clone()),
        inflated_offset: 300,
    };

    let instance_uses = crate::test_support::with_decode_context(|ctx| {
        super::parasolid_attribute_class_uses(
            ctx,
            std::slice::from_ref(&entity),
            std::slice::from_ref(&definition),
        )
        .unwrap()
    });
    assert_eq!(instance_uses.len(), 1);
    assert_eq!(instance_uses[0].entity_51_record, entity.id);
    assert_eq!(u32::from(instance_uses[0].definition_xmt), 34);
    assert_eq!(instance_uses[0].attribute_definition, definition.id);

    let uses = crate::test_support::with_decode_context(|ctx| {
        super::parasolid_topology_attribute_class_uses(
            ctx,
            std::slice::from_ref(&reference),
            std::slice::from_ref(&entity),
            &instance_uses,
        )
        .unwrap()
    });
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].attribute_class_use, instance_uses[0].id);
    assert_eq!(u32::from(uses[0].definition_xmt), 34);
    assert_eq!(uses[0].attribute_definition, definition.id);
    assert!(crate::test_support::with_decode_context(|ctx| {
        super::parasolid_topology_attribute_class_uses(
            ctx,
            std::slice::from_ref(&reference),
            std::slice::from_ref(&entity),
            &[instance_uses[0].clone(), instance_uses[0].clone()],
        )
        .unwrap()
    })
    .is_empty());

    let mut invalid = entity;
    invalid.definition_xmt = 33;
    assert!(crate::test_support::with_decode_context(|ctx| {
        super::parasolid_attribute_class_uses(
            ctx,
            std::slice::from_ref(&invalid),
            std::slice::from_ref(&definition),
        )
        .unwrap()
    })
    .is_empty());
    let invalid_uses = crate::test_support::with_decode_context(|ctx| {
        super::parasolid_attribute_class_uses(ctx, &[invalid.clone()], &[definition]).unwrap()
    });
    assert!(crate::test_support::with_decode_context(|ctx| {
        super::parasolid_topology_attribute_class_uses(
            ctx,
            &[reference],
            std::slice::from_ref(&invalid),
            &invalid_uses,
        )
        .unwrap()
    })
    .is_empty());
}

#[test]
fn topology_attribute_class_uses_follow_type_81_owner_references() {
    let definition = ParasolidAttributeDefinition {
        id: "definition".into(),
        stream_ordinal: 0,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(20).unwrap(),
        next_definition_xmt: None,
        identifier_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(21).unwrap(),
        identifier_inflated_offset: 10,
        name: crate::printable_string::PrintableString::new("CLASS".to_string()).unwrap(),
        type_id: std::num::NonZeroU32::new(8000).unwrap(),
        action_codes: [AttributeAction::Code0; 8],
        field_names_xmt: None,
        legal_owner_flags: crate::parasolid::LegalOwnerFlags::Sixteen([false; 16]),

        field_codes: vec![AttributeField::Integer],
        inflated_offset: 20,
    };
    let head = ParasolidEntity51Record {
        id: "head".into(),
        stream_ordinal: 0,
        xmt: NonNullXmt::try_from(30).unwrap(),
        sequence: NonZeroU32::new(1).unwrap(),
        definition_xmt: 20,
        leading_references: [40, 1, 1, 1, 1],
        trailing_references: EntityReferences::new(vec![50]).unwrap(),
        byte_len: 26,
        inflated_offset: 30,
    };
    let child = ParasolidEntity51Record {
        id: "child".into(),
        xmt: NonNullXmt::try_from(31).unwrap(),
        sequence: NonZeroU32::new(2).unwrap(),
        leading_references: [40, 1, 999, 1, 1],
        inflated_offset: 60,
        ..head.clone()
    };
    let reference = ParasolidTopologyAttributeListReference {
        id: "topology-reference".into(),
        stream_ordinal: 0,
        topology_type: TopologyAttributeKind::Face,
        topology_xmt: 40,
        attribute_list_xmt: 30,
        attribute_list_record: Some(head.id.clone()),
        inflated_offset: 80,
    };
    let class_uses = crate::test_support::with_decode_context(|ctx| {
        super::parasolid_attribute_class_uses(
            ctx,
            &[head.clone(), child.clone()],
            std::slice::from_ref(&definition),
        )
        .unwrap()
    });

    let uses = crate::test_support::with_decode_context(|ctx| {
        super::parasolid_topology_attribute_class_uses(
            ctx,
            std::slice::from_ref(&reference),
            &[head, child],
            &class_uses,
        )
        .unwrap()
    });

    assert_eq!(uses.len(), 2);
    assert!(uses.iter().any(|use_| use_.entity_51_record == "head"));
    assert!(uses.iter().any(|use_| use_.entity_51_record == "child"));
    assert!(uses.iter().any(|use_| use_.id.ends_with("-31")));
}

#[test]
fn entity_51_value_uses_exclude_fixed_leading_references() {
    use super::{
        ParasolidEntity51Record, ParasolidEntity52IntegerRecord, ParasolidEntity54StringRecord,
    };

    let entity = ParasolidEntity51Record {
        id: "entity".into(),
        stream_ordinal: 3,
        xmt: NonNullXmt::try_from(50).unwrap(),
        sequence: NonZeroU32::new(7).unwrap(),
        definition_xmt: 34,
        leading_references: [60, 61, 70, 71, 72],
        trailing_references: EntityReferences::new(vec![70, 71]).unwrap(),
        byte_len: 28,
        inflated_offset: 200,
    };
    let integers = [ParasolidEntity52IntegerRecord {
        id: "integers".into(),
        stream_ordinal: 3,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(70).unwrap(),
        values: crate::parasolid::counted_values::CountedValues::new(vec![1]).unwrap(),
        byte_len: 12,
        inflated_offset: 300,
    }];
    let strings = [ParasolidEntity54StringRecord {
        id: "string".into(),
        stream_ordinal: 3,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(71).unwrap(),
        value: PrintableString::new("value".to_owned()).unwrap(),
        byte_len: 14,
        inflated_offset: 400,
    }];

    let numeric_uses = crate::test_support::with_decode_context(|ctx| {
        super::parasolid_entity_51_numeric_uses(ctx, std::slice::from_ref(&entity), &integers, &[])
            .unwrap()
    });
    assert_eq!(numeric_uses.len(), 1);
    assert_eq!(numeric_uses[0].position.reference_ordinal(), 5);
    assert_eq!(u32::from(numeric_uses[0].referenced_xmt), 70);

    let string_uses = crate::test_support::with_decode_context(|ctx| {
        super::parasolid_entity_51_string_uses(ctx, std::slice::from_ref(&entity), &strings)
            .unwrap()
    });
    assert_eq!(string_uses.len(), 1);
    assert_eq!(string_uses[0].position.reference_ordinal(), 6);
    assert_eq!(u32::from(string_uses[0].referenced_xmt), 71);
}

#[derive(Clone, Copy)]
enum ValueUseRoute {
    Numeric,
    String,
    Structured,
}

fn entity_51_value_use_limit_error(
    route: ValueUseRoute,
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> CodecError {
    let entity = super::ParasolidEntity51Record {
        id: "entity".into(),
        stream_ordinal: 1,
        xmt: NonNullXmt::try_from(10).unwrap(),
        sequence: NonZeroU32::new(1).unwrap(),
        definition_xmt: 9,
        leading_references: [1; 5],
        trailing_references: EntityReferences::new(vec![12]).unwrap(),
        byte_len: 32,
        inflated_offset: 40,
    };
    let xmt = NonNullXmt::try_from(12).unwrap();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            match route {
                ValueUseRoute::Numeric => {
                    let value = super::ParasolidEntity52IntegerRecord {
                        id: "value".into(),
                        stream_ordinal: 1,
                        xmt,
                        values: crate::parasolid::counted_values::CountedValues::new(vec![1])
                            .unwrap(),
                        byte_len: 12,
                        inflated_offset: 80,
                    };
                    super::parasolid_entity_51_numeric_uses(ctx, &[entity], &[value], &[]).err()
                }
                ValueUseRoute::String => {
                    let value = super::ParasolidEntity54StringRecord {
                        id: "value".into(),
                        stream_ordinal: 1,
                        xmt,
                        value: PrintableString::new("text".to_owned()).unwrap(),
                        byte_len: 12,
                        inflated_offset: 80,
                    };
                    super::parasolid_entity_51_string_uses(ctx, &[entity], &[value]).err()
                }
                ValueUseRoute::Structured => {
                    let value = super::ParasolidEntityVectorRecord {
                        id: "value".into(),
                        stream_ordinal: 1,
                        kind: super::ParasolidVectorValueKind::Points,
                        xmt,
                        values: crate::parasolid::counted_values::CountedValues::new(vec![[
                            1.0, 2.0, 3.0,
                        ]])
                        .unwrap(),
                        byte_len: 36,
                        inflated_offset: 80,
                    };
                    super::parasolid_entity_51_structured_uses(
                        ctx,
                        &[entity],
                        &[value],
                        &[],
                        &[],
                        &[],
                    )
                    .err()
                }
            }
            .expect("entity 51 value use route limit refusal")
        },
    )
}

macro_rules! value_use_limit_test {
    ($name:ident, $route:ident, $limit:ident, $dimension:ident) => {
        #[test]
        fn $name() {
            let error = entity_51_value_use_limit_error(ValueUseRoute::$route,
                |policy| policy.limits.$limit = 0);
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::$dimension));
        }
    };
}

value_use_limit_test!(
    numeric_use_route_refuses_collection_limit,
    Numeric,
    max_collection_items,
    CollectionItems
);
value_use_limit_test!(
    numeric_use_route_refuses_retained_limit,
    Numeric,
    max_retained_bytes,
    RetainedBytes
);
value_use_limit_test!(
    numeric_use_route_refuses_scoped_limit,
    Numeric,
    max_materialized_bytes,
    MaterializedBytes
);
value_use_limit_test!(
    numeric_use_route_refuses_work_limit,
    Numeric,
    max_work_units,
    WorkUnits
);
value_use_limit_test!(
    string_use_route_refuses_collection_limit,
    String,
    max_collection_items,
    CollectionItems
);
value_use_limit_test!(
    string_use_route_refuses_retained_limit,
    String,
    max_retained_bytes,
    RetainedBytes
);
value_use_limit_test!(
    string_use_route_refuses_scoped_limit,
    String,
    max_materialized_bytes,
    MaterializedBytes
);
value_use_limit_test!(
    string_use_route_refuses_work_limit,
    String,
    max_work_units,
    WorkUnits
);
value_use_limit_test!(
    structured_use_route_refuses_collection_limit,
    Structured,
    max_collection_items,
    CollectionItems
);
value_use_limit_test!(
    structured_use_route_refuses_retained_limit,
    Structured,
    max_retained_bytes,
    RetainedBytes
);
value_use_limit_test!(
    structured_use_route_refuses_scoped_limit,
    Structured,
    max_materialized_bytes,
    MaterializedBytes
);
value_use_limit_test!(
    structured_use_route_refuses_work_limit,
    Structured,
    max_work_units,
    WorkUnits
);

mod attribute_wire;
mod carrier_and_attribute_resolution;

mod delta_events;

mod group_records;

mod group_members;

mod native_record_limits;
