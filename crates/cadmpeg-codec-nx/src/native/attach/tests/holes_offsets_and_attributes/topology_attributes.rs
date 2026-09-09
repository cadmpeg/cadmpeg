use super::*;

use crate::native::parasolid::topology_attribute_kind::TopologyAttributeKind;
use crate::parasolid::attribute_action::AttributeAction;
use crate::parasolid::attribute_field::AttributeField;
fn attribute_field_name(
    topology_reference: &crate::native::parasolid::ParasolidTopologyAttributeListReference,
    value_use: &str,
    class_uses: &[crate::native::parasolid::ParasolidTopologyAttributeClassUse],
    definitions: &[crate::native::parasolid::ParasolidAttributeDefinition],
    field_uses: &[crate::native::parasolid::ParasolidAttributeFieldUse],
    field_names: &[crate::native::parasolid::ParasolidAttributeFieldNames],
) -> Option<String> {
    super::ParasolidAttributeNameIndex::new(class_uses, definitions, field_uses, field_names)
        .field_name(topology_reference, value_use)
}

#[test]
fn topology_numeric_attribute_values_transfer_in_native_lane_order() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
    use cadmpeg_ir::ids::{FaceId, LoopId, ShellId};
    use cadmpeg_ir::AnnotationBuilder;

    use crate::native::parasolid::{
        ParasolidAttributeDefinition, ParasolidEntity51NumericKind, ParasolidEntity51NumericUse,
        ParasolidEntity52IntegerRecord, ParasolidEntity53DoubleRecord,
        ParasolidTopologyAttributeClassUse, ParasolidTopologyAttributeListReference,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.shells[0].id = ShellId::mint("nx:s3:shell#58").expect("identity grammar");
    ir.model.faces[0].id = FaceId::mint("nx:s3:face#60").expect("identity grammar");
    ir.model.loops[0].id = LoopId::mint("nx:s3:loop#59").expect("identity grammar");
    let references = [
        (TopologyAttributeKind::Shell, 58),
        (TopologyAttributeKind::Face, 60),
        (TopologyAttributeKind::Loop, 59),
    ]
    .map(
        |(topology_type, topology_xmt)| ParasolidTopologyAttributeListReference {
            id: format!("topology-reference-{}", topology_type.code()),
            stream_ordinal: 3,
            topology_type,
            topology_xmt,
            attribute_list_xmt: 50,
            attribute_list_record: Some("entity".into()),
            inflated_offset: 300,
        },
    );
    let integer = ParasolidEntity52IntegerRecord {
        id: "integers".into(),
        stream_ordinal: 3,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(70).unwrap(),
        values: crate::parasolid::counted_values::CountedValues::new(vec![4, u32::MAX]).unwrap(),
        byte_len: 18,
        inflated_offset: 400,
    };
    let double = ParasolidEntity53DoubleRecord {
        id: "doubles".into(),
        stream_ordinal: 3,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(71).unwrap(),
        values: crate::parasolid::counted_values::CountedValues::new(vec![0.25, 7.5]).unwrap(),
        byte_len: 26,
        inflated_offset: 500,
    };
    let uses = [
        ParasolidEntity51NumericUse {
            id: "double-use".into(),
            stream_ordinal: 3,
            entity_51_record: "entity".into(),
            position: crate::parasolid::entity_references::FieldPosition::try_from(6).unwrap(),
            referenced_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(71).unwrap(),
            kind: ParasolidEntity51NumericKind::Doubles,
            value_record: double.id.clone(),
            inflated_offset: 200,
        },
        ParasolidEntity51NumericUse {
            id: "integer-use".into(),
            stream_ordinal: 3,
            entity_51_record: "entity".into(),
            position: crate::parasolid::entity_references::FieldPosition::try_from(5).unwrap(),
            referenced_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(70).unwrap(),
            kind: ParasolidEntity51NumericKind::UnsignedIntegers,
            value_record: integer.id.clone(),
            inflated_offset: 200,
        },
    ];
    let definition = ParasolidAttributeDefinition {
        id: "definition".into(),
        stream_ordinal: 3,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(34).unwrap(),
        next_definition_xmt: None,
        identifier_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(35).unwrap(),
        identifier_inflated_offset: 90,
        name: crate::printable_string::PrintableString::new("SDL/TYSA_DENSITY".to_string())
            .unwrap(),
        type_id: std::num::NonZeroU32::new(8004).unwrap(),
        action_codes: [AttributeAction::Code0; 8],
        field_names_xmt: None,
        legal_owner_flags: crate::parasolid::LegalOwnerFlags::Sixteen([false; 16]),

        field_codes: vec![AttributeField::Real],
        inflated_offset: 100,
    };
    let class_use = ParasolidTopologyAttributeClassUse {
        stream_ordinal: 3,
        inflated_offset: 100,
        id: "class-use".into(),
        topology_attribute_reference: references[2].id.clone(),
        entity_51_record: "entity".into(),
        attribute_class_use: "attribute-class-use".into(),
        definition_xmt: definition.xmt,
        attribute_definition: definition.id.clone(),
    };
    let class_uses = [class_use];
    let definitions = [definition];
    let sources = super::ParasolidNumericAttributeSources {
        numeric_uses: &uses,
        integers: &[integer],
        doubles: &[double],
    };
    let topology_attribute_index = super::ParasolidTopologyAttributeIndex::new(
        &ir,
        &references,
        &class_uses,
        &definitions,
        &[],
        &[],
    );
    let mut annotations = AnnotationBuilder::new();

    super::attach_parasolid_topology_numeric_attributes(
        &mut ir,
        &sources,
        &topology_attribute_index,
        &mut annotations,
    )
    .expect("valid exactness fields");

    let attributes = ir
        .model
        .attributes
        .iter()
        .filter(|attribute| attribute.id.as_str().contains("topology-numeric-attribute"))
        .collect::<Vec<_>>();
    assert_eq!(attributes.len(), 6);
    assert_eq!(
        attributes[0].target,
        AttributeTarget::Shell(ShellId::mint("nx:s3:shell#58").expect("identity grammar"))
    );
    assert_eq!(attributes[0].name, "parasolid_type_integer_reference_5");
    assert_eq!(
        attributes[4].name,
        "SDL/TYSA_DENSITY.parasolid_type_integer_reference_5"
    );
    assert_eq!(
        attributes[0].values,
        [
            AttributeValue::Integer(4),
            AttributeValue::Integer(i64::from(u32::MAX))
        ]
    );
    for (attributes, target) in [
        (
            &attributes[0..2],
            AttributeTarget::Shell(ShellId::mint("nx:s3:shell#58").expect("identity grammar")),
        ),
        (
            &attributes[2..4],
            AttributeTarget::Face(FaceId::mint("nx:s3:face#60").expect("identity grammar")),
        ),
        (
            &attributes[4..6],
            AttributeTarget::Loop(LoopId::mint("nx:s3:loop#59").expect("identity grammar")),
        ),
    ] {
        assert!(attributes
            .iter()
            .all(|attribute| attribute.target == target));
        assert_eq!(
            attributes[1].values,
            [AttributeValue::Float(0.25), AttributeValue::Float(7.5)]
        );
    }
}

#[test]
fn topology_attribute_field_names_use_unique_declared_assignments() {
    use crate::native::parasolid::{
        ParasolidAttributeDefinition, ParasolidAttributeFieldNames, ParasolidAttributeFieldUse,
        ParasolidAttributeFieldValueKind, ParasolidTopologyAttributeClassUse,
        ParasolidTopologyAttributeListReference,
    };

    let reference = ParasolidTopologyAttributeListReference {
        id: "topology-reference".into(),
        stream_ordinal: 3,
        topology_type: TopologyAttributeKind::Face,
        topology_xmt: 60,
        attribute_list_xmt: 50,
        attribute_list_record: Some("entity".into()),
        inflated_offset: 300,
    };
    let definition = ParasolidAttributeDefinition {
        id: "definition".into(),
        stream_ordinal: 3,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(34).unwrap(),
        next_definition_xmt: None,
        identifier_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(35).unwrap(),
        identifier_inflated_offset: 90,
        name: crate::printable_string::PrintableString::new("SDL/TYSA_DENSITY".to_string())
            .unwrap(),
        type_id: std::num::NonZeroU32::new(8004).unwrap(),
        action_codes: [AttributeAction::Code0; 8],
        field_names_xmt: None,
        legal_owner_flags: crate::parasolid::LegalOwnerFlags::Sixteen([false; 16]),

        field_codes: vec![AttributeField::Real, AttributeField::Character],
        inflated_offset: 100,
    };
    let class_use = ParasolidTopologyAttributeClassUse {
        stream_ordinal: 3,
        inflated_offset: 100,
        id: "topology-class-use".into(),
        topology_attribute_reference: reference.id.clone(),
        entity_51_record: "entity".into(),
        attribute_class_use: "attribute-class-use".into(),
        definition_xmt: definition.xmt,
        attribute_definition: definition.id.clone(),
    };
    let field_use = ParasolidAttributeFieldUse {
        id: "field-use".into(),
        stream_ordinal: 3,
        attribute_class_use: "attribute-class-use".into(),
        entity_51_record: "entity".into(),
        attribute_definition: definition.id.clone(),
        position: crate::parasolid::entity_references::FieldPosition::try_from(5).unwrap(),
        value_kind: ParasolidAttributeFieldValueKind::Doubles,
        value_use: "double-use".into(),
        value_record: "double-record".into(),
        inflated_offset: 200,
    };

    assert_eq!(
        attribute_field_name(
            &reference,
            "double-use",
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&definition),
            std::slice::from_ref(&field_use),
            &[],
        )
        .as_deref(),
        Some("SDL/TYSA_DENSITY.density")
    );

    let units = ParasolidAttributeFieldUse {
        position: crate::parasolid::entity_references::FieldPosition::try_from(6).unwrap(),
        value_kind: ParasolidAttributeFieldValueKind::String,
        value_use: "string-use".into(),
        value_record: "string-record".into(),
        ..field_use.clone()
    };
    assert_eq!(
        attribute_field_name(
            &reference,
            "string-use",
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&definition),
            &[units],
            &[],
        )
        .as_deref(),
        Some("SDL/TYSA_DENSITY.units")
    );

    let generic_definition = ParasolidAttributeDefinition {
        name: crate::printable_string::PrintableString::new("CLASS".to_string()).unwrap(),
        field_names_xmt: crate::framing::xmt_reference::XmtTarget::from_wire(25),
        ..definition.clone()
    };
    assert_eq!(
        attribute_field_name(
            &reference,
            "double-use",
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&generic_definition),
            std::slice::from_ref(&field_use),
            &[],
        )
        .as_deref(),
        Some("CLASS.field_0.parasolid_type_2")
    );

    let named_definition = ParasolidAttributeDefinition {
        name: crate::printable_string::PrintableString::new("PVM/25_1".to_string()).unwrap(),
        field_names_xmt: crate::framing::xmt_reference::XmtTarget::from_wire(25),
        ..definition.clone()
    };
    let field_names = ParasolidAttributeFieldNames {
        id: "field-names-relation".into(),
        stream_ordinal: 3,
        attribute_definition: named_definition.id.clone(),
        field_names_record: "field-names-record".into(),
        fields: vec![
            crate::native::parasolid::named_fields::NamedField {
                value_record: "name-1".into(),
                name: "width".into(),
            },
            crate::native::parasolid::named_fields::NamedField {
                value_record: "name-2".into(),
                name: "units".into(),
            },
        ],
    };
    assert_eq!(
        attribute_field_name(
            &reference,
            "double-use",
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&named_definition),
            std::slice::from_ref(&field_use),
            std::slice::from_ref(&field_names),
        )
        .as_deref(),
        Some("PVM/25_1.width")
    );

    let duplicate_class = ParasolidTopologyAttributeClassUse {
        id: "duplicate-class-use".into(),
        ..class_use.clone()
    };
    assert!(attribute_field_name(
        &reference,
        "double-use",
        &[class_use, duplicate_class],
        &[definition],
        &[field_use],
        &[],
    )
    .is_none());
}

#[test]
fn topology_attribute_fields_use_declared_ordinal_and_type_for_every_class() {
    use crate::native::parasolid::{
        ParasolidAttributeDefinition, ParasolidAttributeFieldUse, ParasolidAttributeFieldValueKind,
        ParasolidTopologyAttributeClassUse, ParasolidTopologyAttributeListReference,
    };

    let reference = ParasolidTopologyAttributeListReference {
        id: "topology-reference".into(),
        stream_ordinal: 3,
        topology_type: TopologyAttributeKind::Face,
        topology_xmt: 60,
        attribute_list_xmt: 50,
        attribute_list_record: Some("entity".into()),
        inflated_offset: 300,
    };
    let definition = ParasolidAttributeDefinition {
        id: "definition".into(),
        stream_ordinal: 3,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(34).unwrap(),
        next_definition_xmt: None,
        identifier_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(35).unwrap(),
        identifier_inflated_offset: 90,
        name: crate::printable_string::PrintableString::new("SDL/TYSA_BLEND_ID".to_string())
            .unwrap(),
        type_id: std::num::NonZeroU32::new(8004).unwrap(),
        action_codes: [AttributeAction::Code0; 8],
        field_names_xmt: None,
        legal_owner_flags: crate::parasolid::LegalOwnerFlags::Sixteen([false; 16]),

        field_codes: vec![AttributeField::Character, AttributeField::Real],
        inflated_offset: 100,
    };
    let class_use = ParasolidTopologyAttributeClassUse {
        stream_ordinal: 3,
        inflated_offset: 100,
        id: "topology-class-use".into(),
        topology_attribute_reference: reference.id.clone(),
        entity_51_record: "entity".into(),
        attribute_class_use: "attribute-class-use".into(),
        definition_xmt: definition.xmt,
        attribute_definition: definition.id.clone(),
    };
    let text_field = ParasolidAttributeFieldUse {
        id: "text-field-use".into(),
        stream_ordinal: 3,
        attribute_class_use: class_use.attribute_class_use.clone(),
        entity_51_record: class_use.entity_51_record.clone(),
        attribute_definition: definition.id.clone(),
        position: crate::parasolid::entity_references::FieldPosition::try_from(5).unwrap(),
        value_kind: ParasolidAttributeFieldValueKind::String,
        value_use: "text-use".into(),
        value_record: "text-record".into(),
        inflated_offset: 200,
    };
    let numeric_field = ParasolidAttributeFieldUse {
        id: "numeric-field-use".into(),
        position: crate::parasolid::entity_references::FieldPosition::try_from(6).unwrap(),
        value_kind: ParasolidAttributeFieldValueKind::Doubles,
        value_use: "numeric-use".into(),
        value_record: "numeric-record".into(),
        ..text_field.clone()
    };

    assert_eq!(
        attribute_field_name(
            &reference,
            "text-use",
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&definition),
            std::slice::from_ref(&text_field),
            &[],
        )
        .as_deref(),
        Some("SDL/TYSA_BLEND_ID.field_0.parasolid_type_3")
    );
    assert_eq!(
        attribute_field_name(
            &reference,
            "numeric-use",
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&definition),
            std::slice::from_ref(&numeric_field),
            &[],
        )
        .as_deref(),
        Some("SDL/TYSA_BLEND_ID.field_1.parasolid_type_2")
    );
}

#[test]
fn topology_attribute_index_retains_linked_type_81_records() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
    use cadmpeg_ir::ids::FaceId;
    use cadmpeg_ir::AnnotationBuilder;

    use crate::native::parasolid::{
        ParasolidAttributeDefinition, ParasolidAttributeFieldUse, ParasolidAttributeFieldValueKind,
        ParasolidEntity51NumericKind, ParasolidEntity51NumericUse, ParasolidEntity53DoubleRecord,
        ParasolidTopologyAttributeClassUse, ParasolidTopologyAttributeListReference,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.faces[0].id = FaceId::mint("nx:s3:face#60").expect("identity grammar");
    let reference = ParasolidTopologyAttributeListReference {
        id: "topology-reference".into(),
        stream_ordinal: 3,
        topology_type: TopologyAttributeKind::Face,
        topology_xmt: 60,
        attribute_list_xmt: 50,
        attribute_list_record: Some("head".into()),
        inflated_offset: 300,
    };
    let definition = ParasolidAttributeDefinition {
        id: "definition".into(),
        stream_ordinal: 3,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(34).unwrap(),
        next_definition_xmt: None,
        identifier_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(35).unwrap(),
        identifier_inflated_offset: 90,
        name: crate::printable_string::PrintableString::new("CLASS".to_string()).unwrap(),
        type_id: std::num::NonZeroU32::new(8000).unwrap(),
        action_codes: [AttributeAction::Code0; 8],
        field_names_xmt: None,
        legal_owner_flags: crate::parasolid::LegalOwnerFlags::Sixteen([false; 16]),

        field_codes: vec![AttributeField::Real],
        inflated_offset: 100,
    };
    let class_uses = [
        ParasolidTopologyAttributeClassUse {
            stream_ordinal: 3,
            inflated_offset: 100,
            id: "head-class".into(),
            topology_attribute_reference: reference.id.clone(),
            entity_51_record: "head".into(),
            attribute_class_use: "head-class-use".into(),
            definition_xmt: definition.xmt,
            attribute_definition: definition.id.clone(),
        },
        ParasolidTopologyAttributeClassUse {
            stream_ordinal: 3,
            inflated_offset: 100,
            id: "child-class".into(),
            topology_attribute_reference: reference.id.clone(),
            entity_51_record: "child".into(),
            attribute_class_use: "child-class-use".into(),
            definition_xmt: definition.xmt,
            attribute_definition: definition.id.clone(),
        },
    ];
    let field_uses = [
        ParasolidAttributeFieldUse {
            id: "head-field".into(),
            stream_ordinal: 3,
            attribute_class_use: "head-class-use".into(),
            entity_51_record: "head".into(),
            attribute_definition: definition.id.clone(),
            position: crate::parasolid::entity_references::FieldPosition::try_from(5).unwrap(),
            value_kind: ParasolidAttributeFieldValueKind::Doubles,
            value_use: "head-use".into(),
            value_record: "head-value".into(),
            inflated_offset: 200,
        },
        ParasolidAttributeFieldUse {
            id: "child-field".into(),
            stream_ordinal: 3,
            attribute_class_use: "child-class-use".into(),
            entity_51_record: "child".into(),
            attribute_definition: definition.id.clone(),
            position: crate::parasolid::entity_references::FieldPosition::try_from(5).unwrap(),
            value_kind: ParasolidAttributeFieldValueKind::Doubles,
            value_use: "child-use".into(),
            value_record: "child-value".into(),
            inflated_offset: 210,
        },
    ];
    let numeric_uses = [
        ParasolidEntity51NumericUse {
            id: "head-use".into(),
            stream_ordinal: 3,
            entity_51_record: "head".into(),
            position: crate::parasolid::entity_references::FieldPosition::try_from(5).unwrap(),
            referenced_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(70).unwrap(),
            kind: ParasolidEntity51NumericKind::Doubles,
            value_record: "head-value".into(),
            inflated_offset: 200,
        },
        ParasolidEntity51NumericUse {
            id: "child-use".into(),
            stream_ordinal: 3,
            entity_51_record: "child".into(),
            position: crate::parasolid::entity_references::FieldPosition::try_from(5).unwrap(),
            referenced_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(71).unwrap(),
            kind: ParasolidEntity51NumericKind::Doubles,
            value_record: "child-value".into(),
            inflated_offset: 210,
        },
    ];
    let doubles = [
        ParasolidEntity53DoubleRecord {
            id: "head-value".into(),
            stream_ordinal: 3,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(70).unwrap(),
            values: crate::parasolid::counted_values::CountedValues::new(vec![1.0]).unwrap(),
            byte_len: 18,
            inflated_offset: 400,
        },
        ParasolidEntity53DoubleRecord {
            id: "child-value".into(),
            stream_ordinal: 3,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(71).unwrap(),
            values: crate::parasolid::counted_values::CountedValues::new(vec![2.0]).unwrap(),
            byte_len: 18,
            inflated_offset: 410,
        },
    ];
    let index = super::ParasolidTopologyAttributeIndex::new(
        &ir,
        std::slice::from_ref(&reference),
        &class_uses,
        std::slice::from_ref(&definition),
        &field_uses,
        &[],
    );

    assert_eq!(index.contexts.len(), 2);
    assert_eq!(
        index.class_names.get(reference.id.as_str()).copied(),
        Some("CLASS")
    );
    assert_eq!(
        index
            .attribute_names
            .field_name(&reference, "head-use")
            .as_deref(),
        Some("CLASS.field_0.parasolid_type_2")
    );
    assert_eq!(
        index
            .attribute_names
            .field_name(&reference, "child-use")
            .as_deref(),
        Some("CLASS.field_0.parasolid_type_2")
    );

    let sources = super::ParasolidNumericAttributeSources {
        numeric_uses: &numeric_uses,
        integers: &[],
        doubles: &doubles,
    };
    let mut annotations = AnnotationBuilder::new();
    super::attach_parasolid_topology_numeric_attributes(
        &mut ir,
        &sources,
        &index,
        &mut annotations,
    )
    .expect("valid exactness fields");
    let attributes = ir
        .model
        .attributes
        .iter()
        .filter(|attribute| attribute.id.as_str().contains("topology-numeric-attribute"))
        .collect::<Vec<_>>();
    assert_eq!(attributes.len(), 2);
    assert!(attributes.iter().all(|attribute| {
        attribute.target
            == AttributeTarget::Face(FaceId::mint("nx:s3:face#60").expect("identity grammar"))
            && attribute.name == "CLASS.field_0.parasolid_type_2"
    }));
    assert_ne!(attributes[0].id, attributes[1].id);
    assert!(attributes
        .iter()
        .any(|attribute| { attribute.values == [AttributeValue::Float(1.0)] }));
    assert!(attributes
        .iter()
        .any(|attribute| { attribute.values == [AttributeValue::Float(2.0)] }));
}

#[test]
fn topology_structured_attribute_values_preserve_serialized_lanes() {
    use crate::native::parasolid::structured_value_kind::StructuredValueKind as Kind;
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
    use cadmpeg_ir::ids::FaceId;
    use cadmpeg_ir::AnnotationBuilder;

    use crate::native::parasolid::{
        ParasolidEntity51StructuredUse, ParasolidEntity57AxisRecord, ParasolidEntity58TagRecord,
        ParasolidEntity62UnicodeRecord, ParasolidEntityVectorRecord,
        ParasolidTopologyAttributeListReference, ParasolidVectorValueKind,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.faces[0].id = FaceId::mint("nx:s3:face#60").expect("identity grammar");
    let reference = ParasolidTopologyAttributeListReference {
        id: "topology-reference".into(),
        stream_ordinal: 3,
        topology_type: TopologyAttributeKind::Face,
        topology_xmt: 60,
        attribute_list_xmt: 50,
        attribute_list_record: Some("entity".into()),
        inflated_offset: 300,
    };
    let vectors = [
        (ParasolidVectorValueKind::Points, "point", [1.0, 2.0, 3.0]),
        (ParasolidVectorValueKind::Vectors, "vector", [4.0, 5.0, 6.0]),
        (
            ParasolidVectorValueKind::Directions,
            "direction",
            [7.0, 8.0, 9.0],
        ),
    ]
    .map(|(kind, id, value)| ParasolidEntityVectorRecord {
        id: id.into(),
        stream_ordinal: 3,
        kind,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(70).unwrap(),
        values: crate::parasolid::counted_values::CountedValues::new(vec![value]).unwrap(),
        byte_len: 36,
        inflated_offset: 400,
    });
    let axis = ParasolidEntity57AxisRecord {
        id: "axis".into(),
        stream_ordinal: 3,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(73).unwrap(),
        values: crate::parasolid::counted_values::CountedValues::new(vec![[
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        ]])
        .unwrap(),
        byte_len: 60,
        inflated_offset: 430,
    };
    let tag = ParasolidEntity58TagRecord {
        id: "tag".into(),
        stream_ordinal: 3,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(74).unwrap(),
        values: crate::parasolid::counted_values::CountedValues::new(vec![u32::MAX]).unwrap(),
        byte_len: 16,
        inflated_offset: 440,
    };
    let unicode = ParasolidEntity62UnicodeRecord {
        id: "unicode".into(),
        stream_ordinal: 3,
        xmt: crate::framing::xmt_reference::NonNullXmt::try_from(75).unwrap(),
        value: crate::parasolid::unicode_value::UnicodeValue::new("μ".into()).unwrap(),
        byte_len: 14,
        inflated_offset: 450,
    };
    let uses = [
        (Kind::Points, "point"),
        (Kind::Vectors, "vector"),
        (Kind::Directions, "direction"),
        (Kind::Axes, "axis"),
        (Kind::Tags, "tag"),
        (Kind::Unicode, "unicode"),
    ]
    .into_iter()
    .enumerate()
    .map(|(ordinal, (kind, record))| ParasolidEntity51StructuredUse {
        id: format!("use-{ordinal}"),
        stream_ordinal: 3,
        entity_51_record: "entity".into(),
        position: crate::parasolid::entity_references::FieldPosition::try_from(
            u32::try_from(ordinal).expect("test ordinal fits u32") + 5,
        )
        .unwrap(),
        referenced_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(
            u32::try_from(ordinal).expect("test ordinal fits u32") + 70,
        )
        .unwrap(),
        kind,
        value_record: record.into(),
        inflated_offset: 200,
    })
    .collect::<Vec<_>>();
    let mut annotations = AnnotationBuilder::new();
    let sources = super::ParasolidStructuredAttributeSources {
        structured_uses: &uses,
        vectors: &vectors,
        axes: &[axis],
        tags: &[tag],
        unicode: &[unicode],
    };
    let topology_attribute_index = super::ParasolidTopologyAttributeIndex::new(
        &ir,
        std::slice::from_ref(&reference),
        &[],
        &[],
        &[],
        &[],
    );
    super::attach_parasolid_topology_structured_attributes(
        &mut ir,
        &sources,
        &topology_attribute_index,
        &mut annotations,
    )
    .expect("valid exactness fields");

    let attributes = ir
        .model
        .attributes
        .iter()
        .filter(|attribute| {
            attribute
                .id
                .as_str()
                .contains("topology-structured-attribute")
        })
        .collect::<Vec<_>>();
    assert_eq!(attributes.len(), 6);
    assert!(attributes.iter().all(|attribute| {
        attribute.target
            == AttributeTarget::Face(FaceId::mint("nx:s3:face#60").expect("identity grammar"))
    }));
    let values = attributes
        .iter()
        .map(|attribute| (attribute.name.as_str(), attribute.values.as_slice()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        values["parasolid_type_85_point_reference_5"],
        [AttributeValue::Vector(vec![1.0, 2.0, 3.0])]
    );
    assert_eq!(
        values["parasolid_type_87_axis_reference_8"],
        [AttributeValue::Vector(vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0])]
    );
    assert_eq!(
        values["parasolid_type_88_tag_reference_9"],
        [AttributeValue::Integer(i64::from(u32::MAX))]
    );
    assert_eq!(
        values["parasolid_type_98_unicode_reference_10"],
        [AttributeValue::String("μ".into())]
    );
}
