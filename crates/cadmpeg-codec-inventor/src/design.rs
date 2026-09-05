// SPDX-License-Identifier: Apache-2.0
//! Typed `PmDc` parameters, expression nodes, and unit records.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{Angle, DesignParameter, Length, ParameterId, ParameterValue};
use serde::{Deserialize, Serialize};

use crate::pmdc::{type_id_string, Cursor, PmDcContentHeader, PmDcReference};
use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind};

const EXPRESSION_VALUE_TYPE: [u8; 16] = id(0xf8a7_7a04);
const EXPRESSION_REFERENCE_TYPE: [u8; 16] = id(0xf8a7_7a05);
const EXPRESSION_ADD_TYPE: [u8; 16] = id(0xf8a7_7a06);
const EXPRESSION_SUBTRACT_TYPE: [u8; 16] = id(0xf8a7_7a07);
const EXPRESSION_MULTIPLY_TYPE: [u8; 16] = id(0xf8a7_7a08);
const EXPRESSION_DIVIDE_TYPE: [u8; 16] = id(0xf8a7_7a09);
const EXPRESSION_MODULO_TYPE: [u8; 16] = id(0xf8a7_7a0a);
const EXPRESSION_POWER_TYPE: [u8; 16] = id(0xf8a7_7a0b);
const EXPRESSION_NEGATE_TYPE: [u8; 16] = id(0xf8a7_7a0c);
const EXPRESSION_POWER_IDENTITY_TYPE: [u8; 16] = id(0xf8a7_7a0d);
const UNIT_TYPE: [u8; 16] = id(0xf8a7_79fd);

const fn id(time_low: u32) -> [u8; 16] {
    let first = time_low.to_le_bytes();
    [
        first[0], first[1], first[2], first[3], 0xd2, 0x11, 0x8f, 0x09, 0xc0, 0x00, 0x5a, 0x9a,
        0x23, 0x78, 0xd0, 0x4f,
    ]
}

const PARAMETER_FULL_TYPE: [u8; 16] = [
    0x26, 0x4d, 0x87, 0x90, 0xd0, 0x11, 0xf8, 0xd1, 0x00, 0x08, 0xca, 0xbc, 0x06, 0x63, 0xdc, 0x09,
];
const MILLIMETRE_TYPE: [u8; 16] = [
    0xbc, 0x20, 0x41, 0x62, 0xd2, 0x11, 0x9b, 0x0b, 0x60, 0x00, 0x6a, 0xb7, 0x60, 0xfe, 0xc3, 0xb0,
];
const METRE_TYPE: [u8; 16] = id(0xf8a7_79f5);
const INCH_TYPE: [u8; 16] = id(0xf8a7_79f6);
const FOOT_TYPE: [u8; 16] = id(0xf8a7_79f7);
const RADIAN_TYPE: [u8; 16] = [
    0xf2, 0xcd, 0x30, 0x5c, 0xd2, 0x11, 0x3f, 0x0d, 0x60, 0x00, 0x6a, 0xb7, 0x60, 0xfe, 0xc3, 0xb0,
];
const DEGREE_TYPE: [u8; 16] = [
    0xf0, 0xcd, 0x30, 0x5c, 0xd2, 0x11, 0x3f, 0x0d, 0x60, 0x00, 0x6a, 0xb7, 0x60, 0xfe, 0xc3, 0xb0,
];
const GRAD_TYPE: [u8; 16] = [
    0xf6, 0xcd, 0x30, 0x5c, 0xd2, 0x11, 0x3f, 0x0d, 0x60, 0x00, 0x6a, 0xb7, 0x60, 0xfe, 0xc3, 0xb0,
];
const DIMENSIONLESS_TYPE: [u8; 16] = [
    0x23, 0x00, 0x9d, 0x5f, 0xd2, 0x11, 0x8e, 0x09, 0xc0, 0x00, 0x5a, 0x9a, 0x23, 0x78, 0xd0, 0x4f,
];

#[derive(Debug)]
pub(crate) struct DesignInventory {
    pub(crate) parameters: Vec<PmDcParameter>,
    pub(crate) expressions: Vec<PmDcExpression>,
    pub(crate) units: Vec<PmDcUnit>,
    pub(crate) issues: Vec<DesignRecordIssue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcParameter {
    pub(crate) id: String,
    pub(crate) type_id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) save_version_major: u8,
    #[serde(flatten)]
    pub(crate) header: PmDcContentHeader,
    pub(crate) name: String,
    pub(crate) name_value: u32,
    pub(crate) unit: PmDcReference,
    pub(crate) formula: PmDcReference,
    pub(crate) nominal_value: f64,
    pub(crate) model_value: f64,
    pub(crate) tolerance: u16,
    pub(crate) terminal_value: i16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcExpression {
    pub(crate) id: String,
    pub(crate) type_id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) save_version_major: u8,
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) unit: PmDcReference,
    pub(crate) kind: PmDcExpressionKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum PmDcExpressionKind {
    Value {
        value: f64,
        value_type: u16,
        state: Option<u32>,
    },
    ParameterReference {
        operand: PmDcReference,
    },
    Unary {
        operation: PmDcUnaryOperation,
        operand: PmDcReference,
    },
    Binary {
        operation: PmDcBinaryOperation,
        left: PmDcReference,
        right: PmDcReference,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PmDcUnaryOperation {
    Negate,
    PowerIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PmDcBinaryOperation {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Power,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcUnit {
    pub(crate) id: String,
    pub(crate) type_id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) save_version_major: u8,
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) kind: PmDcUnitKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum PmDcUnitKind {
    Definition {
        numerators: Vec<PmDcReference>,
        numerator_metadata: Option<[u16; 2]>,
        denominators: Vec<PmDcReference>,
        denominator_metadata: Option<[u16; 2]>,
        visible: bool,
        derived: PmDcReference,
    },
    Base {
        dimension: PmDcUnitDimension,
        symbol: String,
        scale_to_internal: f64,
        magnitude: f64,
        factor: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PmDcUnitDimension {
    Length,
    Angle,
    Dimensionless,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignRecordIssue {
    pub(crate) id: String,
    pub(crate) type_id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) detail: String,
}

pub(crate) fn inventory(
    ctx: &DecodeContext<'_>,
    document: &RseInventory<'_>,
) -> Result<DesignInventory, CodecError> {
    let mut inventory = DesignInventory {
        parameters: Vec::new(),
        expressions: Vec::new(),
        units: Vec::new(),
        issues: Vec::new(),
    };
    for segment in &document.segments {
        if segment.kind != SegmentKind::PmDc {
            continue;
        }
        let Some(version) = segment.registry.map(|join| join.version_major) else {
            continue;
        };
        if !(15..=22).contains(&version) {
            continue;
        }
        let SegmentBulkState::Framed(bulk) = &segment.bulk else {
            continue;
        };
        let Some(RecordFrameState::Framed(table)) = &bulk.records else {
            continue;
        };
        for record in &table.records {
            let result = if record.type_id == PARAMETER_FULL_TYPE {
                parse_parameter(ctx, record.payload, version).map(|mut value| {
                    value.segment_token = segment.pair.token.as_str().into();
                    value.record_ordinal = record.ordinal;
                    value.type_id = type_id_string(record.type_id);
                    value.id = format!(
                        "inventor:pmdc:parameter#{}-{}",
                        value.segment_token, value.record_ordinal
                    );
                    inventory.parameters.push(value);
                })
            } else if let Some(operation) = binary_operation(record.type_id) {
                parse_binary_expression(record.payload, version, operation).map(|mut value| {
                    value.segment_token = segment.pair.token.as_str().into();
                    value.record_ordinal = record.ordinal;
                    value.type_id = type_id_string(record.type_id);
                    value.id = format!(
                        "inventor:pmdc:expression#{}-{}",
                        value.segment_token, value.record_ordinal
                    );
                    inventory.expressions.push(value);
                })
            } else if let Some(operation) = unary_operation(record.type_id) {
                parse_unary_expression(record.payload, version, operation).map(|mut value| {
                    value.segment_token = segment.pair.token.as_str().into();
                    value.record_ordinal = record.ordinal;
                    value.type_id = type_id_string(record.type_id);
                    value.id = format!(
                        "inventor:pmdc:expression#{}-{}",
                        value.segment_token, value.record_ordinal
                    );
                    inventory.expressions.push(value);
                })
            } else if record.type_id == EXPRESSION_VALUE_TYPE {
                parse_value_expression(record.payload, version).map(|mut value| {
                    value.segment_token = segment.pair.token.as_str().into();
                    value.record_ordinal = record.ordinal;
                    value.type_id = type_id_string(record.type_id);
                    value.id = format!(
                        "inventor:pmdc:expression#{}-{}",
                        value.segment_token, value.record_ordinal
                    );
                    inventory.expressions.push(value);
                })
            } else if record.type_id == EXPRESSION_REFERENCE_TYPE {
                parse_reference_expression(record.payload, version).map(|mut value| {
                    value.segment_token = segment.pair.token.as_str().into();
                    value.record_ordinal = record.ordinal;
                    value.type_id = type_id_string(record.type_id);
                    value.id = format!(
                        "inventor:pmdc:expression#{}-{}",
                        value.segment_token, value.record_ordinal
                    );
                    inventory.expressions.push(value);
                })
            } else if record.type_id == UNIT_TYPE {
                parse_unit_definition(ctx, record.payload, version).map(|mut value| {
                    value.segment_token = segment.pair.token.as_str().into();
                    value.record_ordinal = record.ordinal;
                    value.type_id = type_id_string(record.type_id);
                    value.id = format!(
                        "inventor:pmdc:unit#{}-{}",
                        value.segment_token, value.record_ordinal
                    );
                    inventory.units.push(value);
                })
            } else if let Some((dimension, symbol, scale)) = base_unit(record.type_id) {
                parse_base_unit(record.payload, version, dimension, symbol, scale).map(
                    |mut value| {
                        value.segment_token = segment.pair.token.as_str().into();
                        value.record_ordinal = record.ordinal;
                        value.type_id = type_id_string(record.type_id);
                        value.id = format!(
                            "inventor:pmdc:unit#{}-{}",
                            value.segment_token, value.record_ordinal
                        );
                        inventory.units.push(value);
                    },
                )
            } else {
                continue;
            };
            if let Err(error) = result {
                inventory.issues.push(DesignRecordIssue {
                    id: format!(
                        "inventor:pmdc:record-issue#{}-{}",
                        segment.pair.token.as_str(),
                        record.ordinal
                    ),
                    type_id: type_id_string(record.type_id),
                    segment_token: segment.pair.token.as_str().into(),
                    record_ordinal: record.ordinal,
                    detail: crate::issue_detail(error)?,
                });
            }
        }
    }
    ctx.charge_collection_items(
        inventory
            .parameters
            .len()
            .saturating_add(inventory.expressions.len())
            .saturating_add(inventory.units.len())
            .saturating_add(inventory.issues.len()) as u64,
        "admit Inventor PmDc design records",
    )?;
    Ok(inventory)
}

pub(crate) fn project_parameters(inventory: &DesignInventory) -> (Vec<DesignParameter>, usize) {
    let expressions = unique_by_ordinal(&inventory.expressions, |record| {
        (&record.segment_token, record.record_ordinal)
    });
    let units = unique_by_ordinal(&inventory.units, |record| {
        (&record.segment_token, record.record_ordinal)
    });
    let parameters = unique_by_ordinal(&inventory.parameters, |record| {
        (&record.segment_token, record.record_ordinal)
    });
    let mut projected = Vec::new();
    let mut unresolved = 0usize;
    for parameter in &inventory.parameters {
        if !parameters.contains_key(&(parameter.segment_token.clone(), parameter.record_ordinal)) {
            unresolved += 1;
            continue;
        }
        let Some(unit) = resolve_unit(&parameter.segment_token, parameter.unit.index, &units)
        else {
            unresolved += 1;
            continue;
        };
        let mut dependencies = Vec::new();
        let mut visiting = HashSet::new();
        let Some(expression) = render_expression(
            &parameter.segment_token,
            parameter.formula.index,
            &expressions,
            &units,
            &parameters,
            &mut dependencies,
            &mut visiting,
        ) else {
            unresolved += 1;
            continue;
        };
        let value = match unit.dimension {
            PmDcUnitDimension::Length => {
                ParameterValue::Length(Length(parameter.model_value * 10.0))
            }
            PmDcUnitDimension::Angle => ParameterValue::Angle(Angle(parameter.model_value)),
            PmDcUnitDimension::Dimensionless => ParameterValue::Real(parameter.model_value),
        };
        if !parameter.model_value.is_finite() {
            unresolved += 1;
            continue;
        }
        projected.push(DesignParameter {
            id: parameter_id(parameter),
            owner: None,
            ordinal: parameter.header.source_index,
            name: parameter.name.clone(),
            expression,
            display: None,
            value: Some(value),
            dependencies,
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: Some(native_parameter_id(parameter)),
        });
    }
    let (projected, graph_rejections) = close_parameter_graph(projected);
    (projected, unresolved.saturating_add(graph_rejections))
}

fn close_parameter_graph(parameters: Vec<DesignParameter>) -> (Vec<DesignParameter>, usize) {
    let candidate_ids = parameters
        .iter()
        .map(|parameter| parameter.id.clone())
        .collect::<HashSet<_>>();
    let mut closed = HashSet::new();
    loop {
        let previous = closed.len();
        for parameter in &parameters {
            if candidate_ids.contains(&parameter.id)
                && parameter
                    .dependencies
                    .iter()
                    .all(|dependency| closed.contains(dependency))
            {
                closed.insert(parameter.id.clone());
            }
        }
        if closed.len() == previous {
            break;
        }
    }
    let rejected = parameters.len().saturating_sub(
        parameters
            .iter()
            .filter(|parameter| closed.contains(&parameter.id))
            .count(),
    );
    (
        parameters
            .into_iter()
            .filter(|parameter| closed.contains(&parameter.id))
            .collect(),
        rejected,
    )
}

fn native_parameter_id(parameter: &PmDcParameter) -> String {
    parameter.id.clone()
}

fn parameter_id(parameter: &PmDcParameter) -> ParameterId {
    ParameterId(format!(
        "inventor:design:parameter#{}-{}",
        parameter.segment_token, parameter.record_ordinal
    ))
}

struct ResolvedUnit {
    dimension: PmDcUnitDimension,
    symbol: String,
    scale_to_internal: f64,
}

fn resolve_unit(
    token: &str,
    reference: u32,
    units: &HashMap<(String, u32), &PmDcUnit>,
) -> Option<ResolvedUnit> {
    let ordinal = reference.checked_sub(1)?;
    let definition = units.get(&(token.to_string(), ordinal))?;
    let PmDcUnitKind::Definition {
        numerators,
        denominators,
        derived,
        ..
    } = &definition.kind
    else {
        return None;
    };
    if numerators.len() != 1 || !denominators.is_empty() || derived.index != 0 {
        return None;
    }
    let base_ordinal = numerators[0].index.checked_sub(1)?;
    let base = units.get(&(token.to_string(), base_ordinal))?;
    let PmDcUnitKind::Base {
        dimension,
        symbol,
        scale_to_internal,
        magnitude,
        factor,
    } = &base.kind
    else {
        return None;
    };
    if !magnitude.is_finite() || !factor.is_finite() {
        return None;
    }
    Some(ResolvedUnit {
        dimension: *dimension,
        symbol: symbol.clone(),
        scale_to_internal: *scale_to_internal,
    })
}

fn render_expression<'a>(
    token: &str,
    reference: u32,
    expressions: &HashMap<(String, u32), &'a PmDcExpression>,
    units: &HashMap<(String, u32), &'a PmDcUnit>,
    parameters: &HashMap<(String, u32), &'a PmDcParameter>,
    dependencies: &mut Vec<ParameterId>,
    visiting: &mut HashSet<u32>,
) -> Option<String> {
    let ordinal = reference.checked_sub(1)?;
    if !visiting.insert(ordinal) {
        return None;
    }
    let expression = expressions.get(&(token.to_string(), ordinal))?;
    let result = match &expression.kind {
        PmDcExpressionKind::Value { value, .. } => {
            let unit = resolve_unit(token, expression.unit.index, units)?;
            if !value.is_finite()
                || !unit.scale_to_internal.is_finite()
                || unit.scale_to_internal == 0.0
            {
                return None;
            }
            let scalar = format_scalar(value / unit.scale_to_internal);
            if unit.symbol.is_empty() {
                scalar
            } else {
                format!("{scalar} {}", unit.symbol)
            }
        }
        PmDcExpressionKind::ParameterReference { operand } => {
            let target = parameters.get(&(token.to_string(), operand.index.checked_sub(1)?))?;
            let id = parameter_id(target);
            if !dependencies.contains(&id) {
                dependencies.push(id);
            }
            target.name.clone()
        }
        PmDcExpressionKind::Unary { operation, operand } => {
            let value = render_expression(
                token,
                operand.index,
                expressions,
                units,
                parameters,
                dependencies,
                visiting,
            )?;
            match operation {
                PmDcUnaryOperation::Negate => format!("-({value})"),
                PmDcUnaryOperation::PowerIdentity => return None,
            }
        }
        PmDcExpressionKind::Binary {
            operation,
            left,
            right,
        } => {
            let left = render_expression(
                token,
                left.index,
                expressions,
                units,
                parameters,
                dependencies,
                visiting,
            )?;
            let right = render_expression(
                token,
                right.index,
                expressions,
                units,
                parameters,
                dependencies,
                visiting,
            )?;
            let symbol = match operation {
                PmDcBinaryOperation::Add => "+",
                PmDcBinaryOperation::Subtract => "-",
                PmDcBinaryOperation::Multiply => "*",
                PmDcBinaryOperation::Divide => "/",
                PmDcBinaryOperation::Modulo => "%",
                PmDcBinaryOperation::Power => "^",
            };
            format!("({left}) {symbol} ({right})")
        }
    };
    visiting.remove(&ordinal);
    Some(result)
}

fn format_scalar(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else {
        value.to_string()
    }
}

fn parse_parameter(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcParameter, CodecError> {
    let mut cursor = Cursor::new(source);
    let header_value = cursor.u32("parameter header value")?;
    let header_id = cursor.u16("parameter header id")?;
    let next = cursor.reference("parameter next reference")?;
    let flags = cursor.u32("parameter flags")?;
    let context = cursor.reference("parameter context reference")?;
    let source_index = cursor.u32("parameter source index")?;
    let name = cursor.utf16(ctx, "parameter name")?;
    let name_value = cursor.u32("parameter name value")?;
    let unit = cursor.reference("parameter unit reference")?;
    let formula = cursor.reference("parameter formula reference")?;
    let nominal_value = cursor.f64("parameter nominal value")?;
    let model_value = cursor.f64("parameter model value")?;
    let tolerance = cursor.u16("parameter tolerance")?;
    let terminal_value = cursor.i16("parameter terminal value")?;
    cursor.finish("parameter")?;
    Ok(PmDcParameter {
        id: String::new(),
        type_id: String::new(),
        segment_token: String::new(),
        record_ordinal: 0,
        save_version_major: version,
        header: PmDcContentHeader {
            header_value,
            header_id,
            next,
            flags,
            context,
            source_index,
        },
        name,
        name_value,
        unit,
        formula,
        nominal_value,
        model_value,
        tolerance,
        terminal_value,
    })
}

fn expression_header(
    source: View<'_>,
) -> Result<(Cursor<'_>, u32, u16, PmDcReference), CodecError> {
    let mut cursor = Cursor::new(source);
    let header_value = cursor.u32("expression header value")?;
    let header_id = cursor.u16("expression header id")?;
    let unit = cursor.reference("expression unit reference")?;
    Ok((cursor, header_value, header_id, unit))
}

fn parse_value_expression(source: View<'_>, version: u8) -> Result<PmDcExpression, CodecError> {
    let (mut cursor, header_value, header_id, unit) = expression_header(source)?;
    let value = cursor.f64("literal expression value")?;
    let value_type = cursor.u16("literal expression type")?;
    let state = if version > 14 {
        Some(cursor.u32("literal expression state")?)
    } else {
        None
    };
    cursor.finish("literal expression")?;
    Ok(PmDcExpression {
        id: String::new(),
        type_id: String::new(),
        segment_token: String::new(),
        record_ordinal: 0,
        save_version_major: version,
        header_value,
        header_id,
        unit,
        kind: PmDcExpressionKind::Value {
            value,
            value_type,
            state,
        },
    })
}

fn parse_reference_expression(source: View<'_>, version: u8) -> Result<PmDcExpression, CodecError> {
    let (mut cursor, header_value, header_id, unit) = expression_header(source)?;
    let operand = cursor.reference("parameter-reference operand")?;
    cursor.finish("parameter-reference expression")?;
    Ok(PmDcExpression {
        id: String::new(),
        type_id: String::new(),
        segment_token: String::new(),
        record_ordinal: 0,
        save_version_major: version,
        header_value,
        header_id,
        unit,
        kind: PmDcExpressionKind::ParameterReference { operand },
    })
}

fn parse_unary_expression(
    source: View<'_>,
    version: u8,
    operation: PmDcUnaryOperation,
) -> Result<PmDcExpression, CodecError> {
    let (mut cursor, header_value, header_id, unit) = expression_header(source)?;
    let operand = cursor.reference("unary expression operand")?;
    cursor.finish("unary expression")?;
    Ok(PmDcExpression {
        id: String::new(),
        type_id: String::new(),
        segment_token: String::new(),
        record_ordinal: 0,
        save_version_major: version,
        header_value,
        header_id,
        unit,
        kind: PmDcExpressionKind::Unary { operation, operand },
    })
}

fn parse_binary_expression(
    source: View<'_>,
    version: u8,
    operation: PmDcBinaryOperation,
) -> Result<PmDcExpression, CodecError> {
    let (mut cursor, header_value, header_id, unit) = expression_header(source)?;
    let left = cursor.reference("binary expression left operand")?;
    let right = cursor.reference("binary expression right operand")?;
    cursor.finish("binary expression")?;
    Ok(PmDcExpression {
        id: String::new(),
        type_id: String::new(),
        segment_token: String::new(),
        record_ordinal: 0,
        save_version_major: version,
        header_value,
        header_id,
        unit,
        kind: PmDcExpressionKind::Binary {
            operation,
            left,
            right,
        },
    })
}

fn parse_unit_definition(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcUnit, CodecError> {
    let mut cursor = Cursor::new(source);
    let header_value = cursor.u32("unit header value")?;
    let header_id = cursor.u16("unit header id")?;
    let numerators = cursor.reference_array(ctx, "unit numerators")?;
    let denominators = cursor.reference_array(ctx, "unit denominators")?;
    let visible = cursor.u8("unit visibility")? != 0;
    let derived = cursor.reference("unit derived reference")?;
    cursor.finish("unit")?;
    Ok(PmDcUnit {
        id: String::new(),
        type_id: String::new(),
        segment_token: String::new(),
        record_ordinal: 0,
        save_version_major: version,
        header_value,
        header_id,
        kind: PmDcUnitKind::Definition {
            numerators: numerators.references().to_vec(),
            numerator_metadata: numerators.metadata(),
            denominators: denominators.references().to_vec(),
            denominator_metadata: denominators.metadata(),
            visible,
            derived,
        },
    })
}

fn parse_base_unit(
    source: View<'_>,
    version: u8,
    dimension: PmDcUnitDimension,
    symbol: &str,
    scale_to_internal: f64,
) -> Result<PmDcUnit, CodecError> {
    let mut cursor = Cursor::new(source);
    let header_value = cursor.u32("base-unit header value")?;
    let header_id = cursor.u16("base-unit header id")?;
    let magnitude = cursor.f64("base-unit magnitude")?;
    let factor = cursor.f64("base-unit factor")?;
    cursor.finish("base unit")?;
    Ok(PmDcUnit {
        id: String::new(),
        type_id: String::new(),
        segment_token: String::new(),
        record_ordinal: 0,
        save_version_major: version,
        header_value,
        header_id,
        kind: PmDcUnitKind::Base {
            dimension,
            symbol: symbol.into(),
            scale_to_internal,
            magnitude,
            factor,
        },
    })
}

fn base_unit(type_id: [u8; 16]) -> Option<(PmDcUnitDimension, &'static str, f64)> {
    match type_id {
        MILLIMETRE_TYPE => Some((PmDcUnitDimension::Length, "mm", 0.1)),
        METRE_TYPE => Some((PmDcUnitDimension::Length, "m", 100.0)),
        INCH_TYPE => Some((PmDcUnitDimension::Length, "in", 2.54)),
        FOOT_TYPE => Some((PmDcUnitDimension::Length, "ft", 30.48)),
        RADIAN_TYPE => Some((PmDcUnitDimension::Angle, "rad", 1.0)),
        DEGREE_TYPE | GRAD_TYPE => Some((
            PmDcUnitDimension::Angle,
            "deg",
            std::f64::consts::PI / 180.0,
        )),
        DIMENSIONLESS_TYPE => Some((PmDcUnitDimension::Dimensionless, "", 1.0)),
        _ => None,
    }
}

fn unary_operation(type_id: [u8; 16]) -> Option<PmDcUnaryOperation> {
    match type_id {
        EXPRESSION_NEGATE_TYPE => Some(PmDcUnaryOperation::Negate),
        EXPRESSION_POWER_IDENTITY_TYPE => Some(PmDcUnaryOperation::PowerIdentity),
        _ => None,
    }
}

fn binary_operation(type_id: [u8; 16]) -> Option<PmDcBinaryOperation> {
    match type_id {
        EXPRESSION_ADD_TYPE => Some(PmDcBinaryOperation::Add),
        EXPRESSION_SUBTRACT_TYPE => Some(PmDcBinaryOperation::Subtract),
        EXPRESSION_MULTIPLY_TYPE => Some(PmDcBinaryOperation::Multiply),
        EXPRESSION_DIVIDE_TYPE => Some(PmDcBinaryOperation::Divide),
        EXPRESSION_MODULO_TYPE => Some(PmDcBinaryOperation::Modulo),
        EXPRESSION_POWER_TYPE => Some(PmDcBinaryOperation::Power),
        _ => None,
    }
}

fn unique_by_ordinal<'a, T>(
    values: &'a [T],
    key: impl Fn(&'a T) -> (&'a String, u32),
) -> HashMap<(String, u32), &'a T> {
    let mut counts = HashMap::new();
    for value in values {
        let (token, ordinal) = key(value);
        let entry = counts
            .entry((token.clone(), ordinal))
            .or_insert((value, 0usize));
        entry.1 += 1;
    }
    counts
        .into_iter()
        .filter_map(|(key, (value, count))| (count == 1).then_some((key, value)))
        .collect()
}

struct ReferenceArray {
    items: Option<([u16; 2], Vec<PmDcReference>)>,
}

impl ReferenceArray {
    fn new(metadata: Option<[u16; 2]>, references: Vec<PmDcReference>) -> Option<Self> {
        Some(Self {
            items: crate::pmdc::paired_items(metadata, references)?,
        })
    }

    fn metadata(&self) -> Option<[u16; 2]> {
        self.items.as_ref().map(|(metadata, _)| *metadata)
    }

    fn references(&self) -> &[PmDcReference] {
        self.items
            .as_ref()
            .map(|(_, references)| references.as_slice())
            .unwrap_or(&[])
    }
}

impl Cursor<'_> {
    fn reference_array(
        &mut self,
        ctx: &DecodeContext<'_>,
        field: &str,
    ) -> Result<ReferenceArray, CodecError> {
        let marker = [
            self.u16(&format!("{field} marker 0"))?,
            self.u16(&format!("{field} marker 1"))?,
        ];
        if marker != [3, 0x3000] {
            return Err(CodecError::malformed(format_args!(
                "Inventor PmDc {field} marker is {marker:?}"
            )));
        }
        let count = self.u32(&format!("{field} count"))? as usize;
        ctx.charge_collection_items(count as u64, "admit Inventor PmDc unit references")?;
        let metadata = if count == 0 {
            None
        } else {
            Some([
                self.u16(&format!("{field} metadata 0"))?,
                self.u16(&format!("{field} metadata 1"))?,
            ])
        };
        let mut references = Vec::with_capacity(count);
        for index in 0..count {
            references.push(self.reference(&format!("{field} reference {index}"))?);
        }
        ReferenceArray::new(metadata, references).ok_or_else(|| {
            CodecError::Malformed(
                "Inventor PmDc unit reference list metadata disagrees with length".into(),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

    const fn reference(index: u32, qualified: bool) -> PmDcReference {
        PmDcReference { index, qualified }
    }

    #[test]
    fn parses_generated_parameter_record() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&42u16.to_le_bytes());
        bytes.extend_from_slice(&0x8000_0002u32.to_le_bytes());
        bytes.extend_from_slice(&0x0603_4200u32.to_le_bytes());
        bytes.extend_from_slice(&0x8000_0000u32.to_le_bytes());
        bytes.extend_from_slice(&13u32.to_le_bytes());
        bytes.extend_from_slice(&6u32.to_le_bytes());
        for unit in "length".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&60.96f64.to_le_bytes());
        bytes.extend_from_slice(&60.96f64.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&(-1i16).to_le_bytes());
        let arena = DecodeArena::new();
        let (ctx, source) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                .expect("fixture view");
        let parameter = parse_parameter(&ctx, source, 22).expect("generated parameter parses");
        assert_eq!(parameter.name, "length");
        assert_eq!(parameter.model_value, 60.96);
        assert_eq!(parameter.formula, reference(4, false));
    }

    #[test]
    fn parses_generated_expression_grammar() {
        fn header(unit: u32) -> Vec<u8> {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&7u32.to_le_bytes());
            bytes.extend_from_slice(&9u16.to_le_bytes());
            bytes.extend_from_slice(&unit.to_le_bytes());
            bytes
        }
        let arena = DecodeArena::new();
        let mut literal = header(3);
        literal.extend_from_slice(&25.4f64.to_le_bytes());
        literal.extend_from_slice(&2u16.to_le_bytes());
        literal.extend_from_slice(&0u32.to_le_bytes());
        let (_, source) =
            DecodeContext::from_root_bytes(&literal, &arena, &DecodePolicy::default())
                .expect("literal view");
        let parsed = parse_value_expression(source, 22).expect("literal expression parses");
        assert!(matches!(
            parsed.kind,
            PmDcExpressionKind::Value {
                value: 25.4,
                state: Some(0),
                ..
            }
        ));

        for operation in [
            PmDcBinaryOperation::Add,
            PmDcBinaryOperation::Subtract,
            PmDcBinaryOperation::Multiply,
            PmDcBinaryOperation::Divide,
            PmDcBinaryOperation::Modulo,
            PmDcBinaryOperation::Power,
        ] {
            let mut bytes = header(3);
            bytes.extend_from_slice(&0x8000_0004u32.to_le_bytes());
            bytes.extend_from_slice(&5u32.to_le_bytes());
            let arena = DecodeArena::new();
            let (_, source) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                    .expect("binary view");
            let parsed =
                parse_binary_expression(source, 22, operation).expect("binary expression parses");
            assert!(matches!(
                parsed.kind,
                PmDcExpressionKind::Binary {
                    left: PmDcReference {
                        index: 4,
                        qualified: true
                    },
                    right: PmDcReference {
                        index: 5,
                        qualified: false
                    },
                    ..
                }
            ));
        }

        for operation in [
            PmDcUnaryOperation::Negate,
            PmDcUnaryOperation::PowerIdentity,
        ] {
            let mut bytes = header(3);
            bytes.extend_from_slice(&0x8000_0004u32.to_le_bytes());
            let arena = DecodeArena::new();
            let (_, source) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                    .expect("unary view");
            let parsed =
                parse_unary_expression(source, 22, operation).expect("unary expression parses");
            assert!(matches!(
                parsed.kind,
                PmDcExpressionKind::Unary {
                    operand: PmDcReference {
                        index: 4,
                        qualified: true
                    },
                    ..
                }
            ));
        }
    }

    #[test]
    fn parses_generated_unit_definition() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&11u16.to_le_bytes());
        bytes.extend_from_slice(&3u16.to_le_bytes());
        bytes.extend_from_slice(&0x3000u16.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 4]);
        bytes.extend_from_slice(&0x8000_0007u32.to_le_bytes());
        bytes.extend_from_slice(&3u16.to_le_bytes());
        bytes.extend_from_slice(&0x3000u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        let arena = DecodeArena::new();
        let (ctx, source) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                .expect("unit view");
        let unit = parse_unit_definition(&ctx, source, 22).expect("unit definition parses");
        assert!(
            matches!(unit.kind, PmDcUnitKind::Definition { ref numerators, visible: true, .. } if numerators == &[reference(7, true)])
        );
    }

    #[test]
    fn projects_closed_parameter_dependencies_and_units() {
        let token = "segment".to_string();
        let base = PmDcUnit {
            id: "unit-base".into(),
            type_id: String::new(),
            segment_token: token.clone(),
            record_ordinal: 0,
            save_version_major: 22,
            header_value: 0,
            header_id: 0,
            kind: PmDcUnitKind::Base {
                dimension: PmDcUnitDimension::Length,
                symbol: "in".into(),
                scale_to_internal: 2.54,
                magnitude: 1.0,
                factor: 1.0,
            },
        };
        let unit = PmDcUnit {
            id: "unit".into(),
            type_id: String::new(),
            segment_token: token.clone(),
            record_ordinal: 1,
            save_version_major: 22,
            header_value: 0,
            header_id: 0,
            kind: PmDcUnitKind::Definition {
                numerators: vec![reference(1, false)],
                numerator_metadata: Some([0, 0]),
                denominators: Vec::new(),
                denominator_metadata: None,
                visible: true,
                derived: reference(0, false),
            },
        };
        let literal = PmDcExpression {
            id: "literal".into(),
            type_id: String::new(),
            segment_token: token.clone(),
            record_ordinal: 2,
            save_version_major: 22,
            header_value: 0,
            header_id: 0,
            unit: reference(2, false),
            kind: PmDcExpressionKind::Value {
                value: 60.96,
                value_type: 0,
                state: Some(0),
            },
        };
        let first = PmDcParameter {
            id: "native-first".into(),
            type_id: String::new(),
            segment_token: token.clone(),
            record_ordinal: 3,
            save_version_major: 22,
            header: PmDcContentHeader {
                header_value: 0,
                header_id: 0,
                next: reference(0, false),
                flags: 0,
                context: reference(0, false),
                source_index: 0,
            },
            name: "width".into(),
            name_value: 0,
            unit: reference(2, false),
            formula: reference(3, false),
            nominal_value: 60.96,
            model_value: 60.96,
            tolerance: 0,
            terminal_value: -1,
        };
        let reference_expression = PmDcExpression {
            id: "reference".into(),
            type_id: String::new(),
            segment_token: token.clone(),
            record_ordinal: 4,
            save_version_major: 22,
            header_value: 0,
            header_id: 0,
            unit: reference(2, false),
            kind: PmDcExpressionKind::ParameterReference {
                operand: reference(4, true),
            },
        };
        let second = PmDcParameter {
            id: "native-second".into(),
            type_id: String::new(),
            segment_token: token,
            record_ordinal: 5,
            save_version_major: 22,
            header: PmDcContentHeader {
                header_value: 0,
                header_id: 0,
                next: reference(0, false),
                flags: 0,
                context: reference(0, false),
                source_index: 1,
            },
            name: "height".into(),
            name_value: 0,
            unit: reference(2, false),
            formula: reference(5, false),
            nominal_value: 60.96,
            model_value: 60.96,
            tolerance: 0,
            terminal_value: -1,
        };
        let inventory = DesignInventory {
            parameters: vec![first, second],
            expressions: vec![literal, reference_expression],
            units: vec![base, unit],
            issues: Vec::new(),
        };
        let (parameters, unresolved) = project_parameters(&inventory);
        assert_eq!(unresolved, 0);
        assert_eq!(parameters[0].expression, "24 in");
        assert_eq!(parameters[1].expression, "width");
        assert_eq!(parameters[1].dependencies, vec![parameters[0].id.clone()]);
        assert_eq!(
            parameters[0].value,
            Some(ParameterValue::Length(Length(609.6)))
        );
    }

    #[test]
    fn rejects_parameter_cycles_and_their_dependents() {
        let make = |name: &str, dependencies: Vec<ParameterId>| DesignParameter {
            id: ParameterId(name.into()),
            owner: None,
            ordinal: 0,
            name: name.into(),
            expression: name.into(),
            display: None,
            value: Some(ParameterValue::Real(1.0)),
            dependencies,
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: None,
        };
        let parameters = vec![
            make("a", vec![ParameterId("b".into())]),
            make("b", vec![ParameterId("a".into())]),
            make("c", vec![ParameterId("a".into())]),
            make("d", Vec::new()),
        ];
        let (closed, rejected) = close_parameter_graph(parameters);
        assert_eq!(rejected, 3);
        assert_eq!(
            closed
                .into_iter()
                .map(|parameter| parameter.id.0)
                .collect::<Vec<_>>(),
            ["d"]
        );
    }
}
