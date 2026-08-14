// SPDX-License-Identifier: Apache-2.0
//! Formula and relation-program CATPart builders.

#![allow(clippy::unwrap_used)]
use super::{
    be32, catalog_stream, entity_table_record, entity_table_record_with_definition_and_value,
    object_graph_from_records, object_graph_record, standard_catpart,
};

pub(crate) fn standard_catpart_with_relation_expression(parameter_role: &str) -> Vec<u8> {
    standard_catpart_with_relation_expression_signature(
        parameter_role,
        "#1_ ",
        "(#1_ : #In LENGTH) : LENGTH",
    )
}

pub(crate) fn standard_catpart_with_relation_expression_signature(
    parameter_role: &str,
    placeholder: &str,
    signature: &str,
) -> Vec<u8> {
    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 4, 0, 0, 0];
    let mut value = Vec::new();
    for ordinal in 5u32..=10 {
        value.push(0x32);
        value.extend_from_slice(&ordinal.to_le_bytes());
    }
    value.push(0xfe);
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe])];
    let mut stream = entity_table_record_with_definition_and_value(1, &definition, &value);
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "body",
        placeholder,
        "#1_ /2-2mm",
        parameter_role,
        signature,
        "opened",
        "RelationExpFct",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_parser_version_relation_expression(
    prefix_role: &str,
    parser_version_role: &str,
) -> Vec<u8> {
    standard_catpart_with_parser_version_relation_expression_roles(
        prefix_role,
        parser_version_role,
        None,
    )
}

pub(crate) fn standard_catpart_with_opened_parser_version_relation_expression(
    prefix_role: &str,
    parser_version_role: &str,
    state_role: &str,
) -> Vec<u8> {
    standard_catpart_with_parser_version_relation_expression_roles(
        prefix_role,
        parser_version_role,
        Some(state_role),
    )
}

pub(crate) fn standard_catpart_with_parser_version_relation_expression_roles(
    prefix_role: &str,
    parser_version_role: &str,
    state_role: Option<&str>,
) -> Vec<u8> {
    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 4, 0, 0, 0];
    let mut value = Vec::new();
    for ordinal in 5u32..=10 + u32::from(state_role.is_some()) {
        value.push(0x32);
        value.extend_from_slice(&ordinal.to_le_bytes());
    }
    value.push(0xfe);
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe])];
    let mut stream = entity_table_record_with_definition_and_value(1, &definition, &value);
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    let mut entries = vec![
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "body",
        prefix_role,
        "log(min(100,max(20*#1_,#2_)/#2_))/log(100)/2",
        parser_version_role,
        "param",
        "(#1_ : #In LENGTH,#2_ : #In LENGTH) : Real\n",
    ];
    entries.extend(state_role);
    entries.push("RelationExpFct");
    stream.extend(catalog_stream(&entries));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_unprefixed_parser_version_relation_expression(
    parser_version_role: &str,
) -> Vec<u8> {
    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 4, 0, 0, 0];
    let mut value = Vec::new();
    for ordinal in 5u32..=9 {
        value.push(0x32);
        value.extend_from_slice(&ordinal.to_le_bytes());
    }
    value.push(0xfe);
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe])];
    let mut stream = entity_table_record_with_definition_and_value(1, &definition, &value);
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "body",
        "360.0*1 deg/#1_",
        parser_version_role,
        "param",
        "(#1_ : #In Integer) : ANGLE\n",
        "RelationExpFct",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_relation_program_instance(
    program_entity_id: u32,
    repeated_reference_entity_id: u32,
    context_entity_id: u32,
    stored_self_entity_id: u32,
) -> Vec<u8> {
    standard_catpart_with_relation_program_instance_class(
        program_entity_id,
        repeated_reference_entity_id,
        context_entity_id,
        stored_self_entity_id,
        "body",
    )
}

pub(crate) fn standard_catpart_with_relation_program_instance_class(
    program_entity_id: u32,
    repeated_reference_entity_id: u32,
    context_entity_id: u32,
    stored_self_entity_id: u32,
    context_class: &str,
) -> Vec<u8> {
    let mut instance_payload = Vec::new();
    let reference = |payload: &mut Vec<u8>, value: u32| {
        payload.push(0x32);
        payload.extend_from_slice(&value.to_le_bytes());
    };
    let atom = |payload: &mut Vec<u8>, value: u32| {
        payload.push(0x80);
        payload.extend_from_slice(&value.to_le_bytes());
    };
    reference(&mut instance_payload, 20);
    atom(&mut instance_payload, 3);
    reference(&mut instance_payload, 21);
    atom(&mut instance_payload, 22);
    atom(&mut instance_payload, 0x3d7d_031f);
    atom(&mut instance_payload, 5);
    atom(&mut instance_payload, 89);
    atom(&mut instance_payload, 1_127_154_762);
    reference(&mut instance_payload, 23);
    atom(&mut instance_payload, repeated_reference_entity_id);
    reference(&mut instance_payload, 25);
    atom(&mut instance_payload, 2);
    reference(&mut instance_payload, repeated_reference_entity_id);
    reference(&mut instance_payload, context_entity_id);
    atom(&mut instance_payload, 2);
    reference(&mut instance_payload, 21);
    atom(&mut instance_payload, program_entity_id);
    reference(&mut instance_payload, 27);
    atom(&mut instance_payload, stored_self_entity_id);
    instance_payload.push(0xfe);

    standard_catpart_with_relation_program_payload(
        &[0x12, 0x8a, 0x80],
        &instance_payload,
        context_class,
    )
}

pub(crate) fn standard_catpart_with_lead54_relation_program_instance(
    program_entity_id: u32,
    repeated_reference_entity_id: u32,
    trailing_entity_id: u32,
    stored_self_entity_id: u32,
) -> Vec<u8> {
    standard_catpart_with_lead54_relation_program_instance_class(
        program_entity_id,
        repeated_reference_entity_id,
        trailing_entity_id,
        stored_self_entity_id,
        "body",
    )
}

pub(crate) fn standard_catpart_with_lead54_relation_program_instance_class(
    program_entity_id: u32,
    repeated_reference_entity_id: u32,
    trailing_entity_id: u32,
    stored_self_entity_id: u32,
    context_class: &str,
) -> Vec<u8> {
    let mut instance_payload = Vec::new();
    let reference = |payload: &mut Vec<u8>, value: u32| {
        payload.push(0x32);
        payload.extend_from_slice(&value.to_le_bytes());
    };
    let atom = |payload: &mut Vec<u8>, value: u32| {
        payload.push(0x80);
        payload.extend_from_slice(&value.to_le_bytes());
    };
    atom(&mut instance_payload, 244);
    atom(&mut instance_payload, 2);
    reference(&mut instance_payload, 5);
    atom(&mut instance_payload, program_entity_id);
    atom(&mut instance_payload, 2_142_008_808);
    atom(&mut instance_payload, 247);
    atom(&mut instance_payload, repeated_reference_entity_id);
    reference(&mut instance_payload, 20);
    atom(&mut instance_payload, stored_self_entity_id);
    atom(&mut instance_payload, 249);
    atom(&mut instance_payload, 2);
    reference(&mut instance_payload, repeated_reference_entity_id);
    reference(&mut instance_payload, 21);
    atom(&mut instance_payload, 2);
    reference(&mut instance_payload, 5);
    atom(&mut instance_payload, trailing_entity_id);
    atom(&mut instance_payload, 129);
    instance_payload.push(0xfe);

    standard_catpart_with_relation_program_payload(
        &[0x54, 0x01, 0x82, 0x80, 0x81],
        &instance_payload,
        context_class,
    )
}

pub(crate) fn standard_catpart_with_relation_program_payload(
    head: &[u8],
    instance_payload: &[u8],
    context_class: &str,
) -> Vec<u8> {
    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 4, 0, 0, 0];
    let mut expression_value = Vec::new();
    for ordinal in 5_u32..=10 {
        expression_value.push(0x32);
        expression_value.extend_from_slice(&ordinal.to_le_bytes());
    }
    expression_value.push(0xfe);

    let mut stream =
        entity_table_record_with_definition_and_value(1, &definition, &expression_value);
    stream.extend(entity_table_record(2));
    stream.push(0xde);
    stream.extend(object_graph_from_records(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe]),
        object_graph_record(head, instance_payload),
    ]));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        context_class,
        "Boolean",
        "log(min(100,max(20*#1_,#2_)/#2_))/log(100)/2",
        "ParserVersion",
        "param",
        "(#1_ : #In LENGTH,#2_ : #In LENGTH) : Real\n",
        "RelationExpFct",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_configuration_incidences(
    configuration_schema_ordinal: u32,
    second_configuration_entity_id: u32,
    row_successor_entity_id: u32,
) -> Vec<u8> {
    let reference = |payload: &mut Vec<u8>, value: u32| {
        payload.push(0x32);
        payload.extend_from_slice(&value.to_le_bytes());
    };
    let atom = |payload: &mut Vec<u8>, value: u32| {
        payload.push(0x80);
        payload.extend_from_slice(&value.to_le_bytes());
    };
    let mut configuration_payload = Vec::new();
    reference(&mut configuration_payload, configuration_schema_ordinal);
    atom(&mut configuration_payload, 2);
    reference(&mut configuration_payload, second_configuration_entity_id);
    atom(&mut configuration_payload, 129);
    configuration_payload.push(0xfe);
    let mut row_payload = Vec::new();
    atom(&mut row_payload, 250);
    atom(&mut row_payload, row_successor_entity_id);
    row_payload.push(0xfe);

    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 4, 0, 0, 0];
    let mut value = Vec::new();
    for ordinal in 8_u32..=13 {
        reference(&mut value, ordinal);
    }
    value.push(0xfe);
    let mut stream = entity_table_record_with_definition_and_value(5, &definition, &value);
    stream.extend(entity_table_record(6));
    stream.extend(entity_table_record(7));
    stream.push(0xde);
    stream.extend(object_graph_from_records(&[
        object_graph_record(&[0x12, 0x87, 0x85], &configuration_payload),
        object_graph_record(&[0x12, 0x87, 0x86], &row_payload),
        object_graph_record(&[0x12, 0x87, 0x87], &[0xfe]),
    ]));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "body",
        "Configuration",
        "configrow",
        "body",
        "Boolean",
        "log(min(100,max(20*#1_,#2_)/#2_))/log(100)/2",
        "ParserVersion",
        "param",
        "(#1_ : #In LENGTH,#2_ : #In LENGTH) : Real\n",
        "RelationExpFct",
        "opened",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_schema_configuration_row_chain() -> Vec<u8> {
    let row_payload = |successor: u32| {
        let mut payload = vec![0x80];
        payload.extend_from_slice(&250_u32.to_le_bytes());
        payload.push(0x80);
        payload.extend_from_slice(&successor.to_le_bytes());
        payload.push(0xfe);
        payload
    };
    let mut stream = entity_table_record(5);
    stream.extend(entity_table_record(6));
    stream.extend(entity_table_record(7));
    stream.extend(entity_table_record(8));
    stream.extend(entity_table_record(9));
    stream.extend(entity_table_record(10));
    stream.extend(entity_table_record(11));
    stream.push(0xde);
    stream.extend(object_graph_from_records(&[
        object_graph_record(&[0x12, 0x89, 0x85], &row_payload(7)),
        object_graph_record(&[0x12, 0x89, 0x86], &[0xfe]),
        object_graph_record(&[0x12, 0x89, 0x85], &row_payload(9)),
        object_graph_record(&[0x12, 0x89, 0x86], &[0xfe]),
        object_graph_record(&[0x12, 0x89, 0x85], &row_payload(11)),
        object_graph_record(&[0x12, 0x89, 0x86], &[0xfe]),
        object_graph_record(&[0x12, 0x89, 0x86], &[0xfe]),
    ]));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "body",
        "configrow",
        "body",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_parameter_value(suffix: &[u8]) -> Vec<u8> {
    standard_catpart_with_two_selector_value("Thickness", "#1_ /2", suffix)
}

pub(crate) fn standard_catpart_with_two_selector_value(
    first: &str,
    second: &str,
    suffix: &[u8],
) -> Vec<u8> {
    let value = [0x32, 4, 0, 0, 0, 0x32, 5, 0, 0, 0, 0xfe];
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe])];
    let mut entity = entity_table_record_with_definition_and_value(1, &[0x01], &value);
    entity[6] = 2;
    entity.extend_from_slice(suffix);
    let entity_len = u32::try_from(entity.len()).expect("bounded entity record");
    entity[2..6].copy_from_slice(&entity_len.to_le_bytes());
    let mut stream = entity;
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        first,
        second,
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_range_interval(encoded_range: &[u8], suffix: &[u8]) -> Vec<u8> {
    let mut value = vec![0x32, 4, 0, 0, 0];
    value.extend_from_slice(encoded_range);
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe])];
    let mut entity = entity_table_record_with_definition_and_value(1, &[0x01], &value);
    entity[6] = 2;
    entity.extend_from_slice(suffix);
    let entity_len = u32::try_from(entity.len()).expect("bounded entity record");
    entity[2..6].copy_from_slice(&entity_len.to_le_bytes());
    let mut stream = entity;
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Range",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_definition_value(
    definition: &[u8],
    value: &[u8],
    suffix: &[u8],
) -> Vec<u8> {
    let records = [object_graph_record(&[0x16, 0x84, 0x81, 0x81], &[0xfe])];
    let mut entity = entity_table_record_with_definition_and_value(1, definition, value);
    entity[6] = 2;
    entity.extend_from_slice(suffix);
    let entity_len = u32::try_from(entity.len()).expect("bounded entity record");
    entity[2..6].copy_from_slice(&entity_len.to_le_bytes());
    let mut stream = entity;
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Thickness",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_definition_chain_value(suffix: &[u8]) -> Vec<u8> {
    standard_catpart_with_definition_chain_type("Real", suffix)
}

pub(crate) fn standard_catpart_with_definition_chain_type(
    value_type: &str,
    suffix: &[u8],
) -> Vec<u8> {
    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 5, 0, 0, 0];
    let records = [object_graph_record(&[0x16, 0x84, 0x81, 0x81], &[0xfe])];
    let mut entity = entity_table_record_with_definition_and_value(1, &definition, &[0xfe]);
    entity[6] = 2;
    entity.extend_from_slice(suffix);
    let entity_len = u32::try_from(entity.len()).expect("bounded entity record");
    entity[2..6].copy_from_slice(&entity_len.to_le_bytes());
    let mut stream = entity;
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "FeatureFEDGE",
        value_type,
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_unassigned_definition_chain_value() -> Vec<u8> {
    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 5, 0, 0, 0];
    let records = [object_graph_record(
        &[0x16, 0x84, 0x80, 66, 23, 0, 0, 0x80, 0x81, 25, 0, 0],
        &[0xfe],
    )];
    let mut entity = entity_table_record_with_definition_and_value(1, &definition, &[0xfe]);
    entity[6] = 2;
    entity.extend_from_slice(&[0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe7]);
    let entity_len = u32::try_from(entity.len()).expect("bounded entity record");
    entity[2..6].copy_from_slice(&entity_len.to_le_bytes());
    let mut stream = entity;
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "FeatureFEDGE",
        "Real",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_two_definition_chain_values() -> Vec<u8> {
    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 5, 0, 0, 0];
    let suffix = |value: u8| [0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0x80 + value];
    let mut stream = Vec::new();
    for (entity_id, value) in [(1_u32, 1_u8), (2_u32, 2_u8)] {
        let mut entity =
            entity_table_record_with_definition_and_value(entity_id, &definition, &[0xfe]);
        entity[6] = 2;
        entity.extend_from_slice(&suffix(value));
        let entity_len = u32::try_from(entity.len()).expect("bounded entity record");
        entity[2..6].copy_from_slice(&entity_len.to_le_bytes());
        stream.extend(entity);
    }
    stream.push(0xde);
    stream.extend(object_graph_from_records(&[
        object_graph_record(&[0x16, 0x84, 0x81, 0x81], &[0xfe]),
        object_graph_record(&[0x16, 0x84, 0x82, 0x81], &[0xfe]),
    ]));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "FeatureFEDGE",
        "Real",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

pub(crate) fn standard_catpart_with_formula_relation(
    parameter_entity_id: u8,
    duplicate_binding: bool,
) -> Vec<u8> {
    standard_catpart_with_typed_formula_relation(
        parameter_entity_id,
        duplicate_binding,
        "LENGTH",
        "LENGTH",
        35.0,
        33.0,
        "#1_ /2-2mm",
    )
}

pub(crate) fn standard_catpart_with_typed_formula_relation(
    parameter_entity_id: u8,
    duplicate_binding: bool,
    input_type: &str,
    result_type: &str,
    input_value: f64,
    output_value: f64,
    source_expression: &str,
) -> Vec<u8> {
    standard_catpart_with_typed_formula_inputs(
        parameter_entity_id,
        duplicate_binding,
        &[("#1_", input_type, "Thickness", "#1_ /2", input_value)],
        result_type,
        Some(output_value),
        source_expression,
    )
}

pub(crate) fn standard_catpart_with_typed_formula_inputs(
    parameter_entity_id: u8,
    duplicate_binding: bool,
    inputs: &[(&str, &str, &str, &str, f64)],
    result_type: &str,
    output_value: Option<f64>,
    source_expression: &str,
) -> Vec<u8> {
    standard_catpart_with_typed_formula_inputs_and_object_payload(
        parameter_entity_id,
        duplicate_binding,
        inputs,
        result_type,
        output_value,
        source_expression,
        (&[0xfe], None),
    )
}

pub(crate) fn standard_catpart_with_typed_formula_inputs_and_object_payload(
    parameter_entity_id: u8,
    duplicate_binding: bool,
    inputs: &[(&str, &str, &str, &str, f64)],
    result_type: &str,
    output_value: Option<f64>,
    source_expression: &str,
    input_options: (&[u8], Option<usize>),
) -> Vec<u8> {
    let (input_object_payload, unset_input_index) = input_options;
    assert!(!duplicate_binding || !inputs.is_empty());
    let formula_definition = [0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 4, 0, 0, 0];
    let expression_definition = [0x00, 0x08, 0x32, 5, 0, 0, 0, 0x32, 5, 0, 0, 0];
    let mut expression_value = Vec::new();
    for ordinal in 6u32..=11 {
        expression_value.push(0x32);
        expression_value.extend_from_slice(&ordinal.to_le_bytes());
    }
    expression_value.push(0xfe);

    let mut stream = entity_table_record_with_definition_and_value(1, &formula_definition, &[0xfe]);
    stream.extend(entity_table_record_with_definition_and_value(
        2,
        &expression_definition,
        &expression_value,
    ));
    let parameter = |entity_id, name_ordinal: u32, binding_ordinal: u32, value: Option<f64>| {
        let mut parameter_value = vec![0x32];
        parameter_value.extend_from_slice(&name_ordinal.to_le_bytes());
        parameter_value.push(0x32);
        parameter_value.extend_from_slice(&binding_ordinal.to_le_bytes());
        parameter_value.push(0xfe);
        let mut parameter =
            entity_table_record_with_definition_and_value(entity_id, &[0x01], &parameter_value);
        parameter[6] = 2;
        parameter.extend_from_slice(&[0x85, 0x96, 0x82, 0x6a]);
        match value {
            Some(value) => {
                parameter.push(0xe6);
                parameter.extend_from_slice(&value.to_bits().to_le_bytes());
            }
            None => parameter.push(0xe7),
        }
        parameter.extend_from_slice(&[0x81, 0x52]);
        let parameter_len = u32::try_from(parameter.len()).expect("bounded parameter entity");
        parameter[2..6].copy_from_slice(&parameter_len.to_le_bytes());
        parameter
    };
    for (index, (_, _, _, _, value)) in inputs.iter().enumerate() {
        let entity_id = 3 + u8::try_from(index).expect("bounded input count");
        let name_ordinal = 12 + 2 * u32::try_from(index).expect("bounded input count");
        stream.extend(parameter(
            entity_id.into(),
            name_ordinal,
            name_ordinal + 1,
            (unset_input_index != Some(index)).then_some(*value),
        ));
    }
    if duplicate_binding {
        let entity_id = 3 + u8::try_from(inputs.len()).expect("bounded input count");
        stream.extend(parameter(
            entity_id.into(),
            12_u32,
            13_u32,
            Some(inputs[0].4),
        ));
    }
    let output_entity_id =
        3 + u8::try_from(inputs.len()).expect("bounded input count") + u8::from(duplicate_binding);
    let output_name_ordinal = 12 + 2 * u32::try_from(inputs.len()).expect("bounded input count");
    stream.extend(parameter(
        output_entity_id.into(),
        output_name_ordinal,
        output_name_ordinal + 1,
        output_value,
    ));
    stream.push(0xde);
    let mut records = vec![
        object_graph_record(
            &[0x04, 0x01, 0x81, 0x84],
            &[
                0xf9,
                0x84,
                0x81,
                0x81,
                0x81,
                0x82,
                0x81,
                parameter_entity_id,
                0xd1,
                0x80,
                0xfe,
            ],
        ),
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe]),
    ];
    records.extend(
        inputs
            .iter()
            .map(|_| object_graph_record(&[0x04, 0x01, 0x81, 0x84], input_object_payload)),
    );
    if duplicate_binding {
        records.push(object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe]));
    }
    records.push(object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe]));
    stream.extend(object_graph_from_records(&records));
    let input_signature = inputs
        .iter()
        .map(|(symbol, input_type, _, _, _)| format!("{symbol} : #In {input_type}"))
        .collect::<Vec<_>>()
        .join(",");
    let type_signature = format!("({input_signature}) : {result_type}");
    let mut catalog = vec![
        "CATCatalogManager".to_string(),
        "catalogManager".to_string(),
        "catalogLinks".to_string(),
        String::new(),
        "Formula".to_string(),
        "body".to_string(),
        inputs
            .first()
            .map_or_else(String::new, |input| format!("{} ", input.0)),
        source_expression.to_string(),
        "param".to_string(),
        type_signature,
        "opened".to_string(),
        "RelationExpFct".to_string(),
    ];
    for (_, _, name, binding, _) in inputs {
        catalog.push((*name).to_string());
        catalog.push((*binding).to_string());
    }
    catalog.push("Result".to_string());
    catalog.push("#result_ /1".to_string());
    stream.extend(catalog_stream(
        &catalog.iter().map(String::as_str).collect::<Vec<_>>(),
    ));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

#[derive(Clone, Copy)]
pub(crate) enum FormulaChainCase {
    Linear,
    Cyclic,
    DuplicateTerminal,
    DuplicateIntermediate,
    IncompatibleDownstream,
    AmbiguousIntermediateWithIncompatibleDownstream,
}

pub(crate) fn standard_catpart_with_formula_chain(case: FormulaChainCase) -> Vec<u8> {
    let cyclic = matches!(case, FormulaChainCase::Cyclic);
    let duplicate_terminal = matches!(case, FormulaChainCase::DuplicateTerminal);
    let duplicate_intermediate = matches!(
        case,
        FormulaChainCase::DuplicateIntermediate
            | FormulaChainCase::AmbiguousIntermediateWithIncompatibleDownstream
    );
    let incompatible_downstream = matches!(
        case,
        FormulaChainCase::IncompatibleDownstream
            | FormulaChainCase::AmbiguousIntermediateWithIncompatibleDownstream
    );
    let definition = |ordinal: u32| {
        let mut bytes = vec![0x00, 0x08, 0x32];
        bytes.extend_from_slice(&ordinal.to_le_bytes());
        bytes.push(0x32);
        bytes.extend_from_slice(&ordinal.to_le_bytes());
        bytes
    };
    let expression_value = |ordinals: [u32; 6]| {
        let mut bytes = Vec::new();
        for ordinal in ordinals {
            bytes.push(0x32);
            bytes.extend_from_slice(&ordinal.to_le_bytes());
        }
        bytes.push(0xfe);
        bytes
    };
    let parameter = |entity_id: u32, name: u32, binding: u32, value: f64| {
        let mut payload = vec![0x32];
        payload.extend_from_slice(&name.to_le_bytes());
        payload.push(0x32);
        payload.extend_from_slice(&binding.to_le_bytes());
        payload.push(0xfe);
        let mut entity =
            entity_table_record_with_definition_and_value(entity_id, &[0x01], &payload);
        entity[6] = 2;
        entity.extend_from_slice(&[0x85, 0x96, 0x82, 0x6a, 0xe6]);
        entity.extend_from_slice(&value.to_bits().to_le_bytes());
        entity.extend_from_slice(&[0x81, 0x52]);
        let len = u32::try_from(entity.len()).expect("bounded parameter entity");
        entity[2..6].copy_from_slice(&len.to_le_bytes());
        entity
    };
    let formula_object = |owner: u8, expression: u8, output: u8| {
        object_graph_record(
            &[0x04, 0x01, 0x81, 0x84],
            &[
                0xf9,
                0x84,
                0x81,
                0x80 + owner,
                0x81,
                0x80 + expression,
                0x81,
                output,
                0xd1,
                0x80,
                0xfe,
            ],
        )
    };
    let empty_object = || object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe]);

    let mut stream = entity_table_record_with_definition_and_value(1, &definition(4), &[0xfe]);
    stream.extend(entity_table_record_with_definition_and_value(
        2,
        &definition(5),
        &expression_value([6, 7, 8, 9, 10, 11]),
    ));
    stream.extend(parameter(3, 12, 13, 1.0));
    stream.extend(parameter(4, 14, 15, if cyclic { 3.0 } else { 2.0 }));
    stream.extend(entity_table_record_with_definition_and_value(
        5,
        &definition(4),
        &[0xfe],
    ));
    stream.extend(entity_table_record_with_definition_and_value(
        6,
        &definition(5),
        &expression_value(if duplicate_terminal {
            [6, 7, 8, 9, 10, 11]
        } else {
            [16, 17, 8, 18, 10, 11]
        }),
    ));
    stream.extend(parameter(7, 19, 20, 3.0));
    if duplicate_intermediate {
        stream.extend(entity_table_record_with_definition_and_value(
            8,
            &definition(4),
            &[0xfe],
        ));
        stream.extend(entity_table_record_with_definition_and_value(
            9,
            &definition(5),
            &expression_value([6, 7, 8, 9, 10, 11]),
        ));
    }
    stream.push(0xde);
    let mut objects = vec![
        formula_object(1, 2, 4),
        empty_object(),
        empty_object(),
        empty_object(),
        formula_object(5, 6, if duplicate_terminal { 4 } else { 7 }),
        empty_object(),
        empty_object(),
    ];
    if duplicate_intermediate {
        objects.extend([formula_object(8, 9, 4), empty_object()]);
    }
    stream.extend(object_graph_from_records(&objects));
    let first_expression = if cyclic { "#3_ /4" } else { "#1_ /2+1mm" };
    let first_placeholder = if cyclic { "#3_ " } else { "#1_ " };
    let first_signature = if cyclic {
        "(#3_ : #In LENGTH) : LENGTH"
    } else {
        "(#1_ : #In LENGTH) : LENGTH"
    };
    let catalog = vec![
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Formula",
        "body",
        first_placeholder,
        first_expression,
        "param",
        first_signature,
        "opened",
        "RelationExpFct",
        "Input",
        "#1_ /2",
        "Intermediate",
        "#2_ /3",
        "#2_ ",
        if cyclic {
            "#2_ /3"
        } else if incompatible_downstream {
            "#2_ /3+1"
        } else {
            "#2_ /3+1mm"
        },
        if incompatible_downstream {
            "(#2_ : #In Real) : Real"
        } else {
            "(#2_ : #In LENGTH) : LENGTH"
        },
        "Final",
        "#3_ /4",
    ];
    stream.extend(catalog_stream(&catalog));

    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}
