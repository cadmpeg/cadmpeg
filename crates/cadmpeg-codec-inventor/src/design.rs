// SPDX-License-Identifier: Apache-2.0
//! Typed `PmDc` parameters, expression nodes, and unit records.

use cadmpeg_core::decode::unique_index;
use std::collections::{HashMap, HashSet, VecDeque};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    features::{DesignParameter, ParameterId, ParameterValue},
    report::loss::LossNote,
    scalar::{Angle, FiniteReal, Length},
};
use serde::{Deserialize, Serialize};

use crate::loss::InventorLossCode;
use crate::pmdc::{inventor_id, Cursor, PmDcContentHeader, PmDcPairedReferenceList, PmDcReference};
use crate::record_identity::{push_record, Located, RecordPayload};
use crate::record_issue::{RecordIssue, RecordIssueFamily};
use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind};

const EXPRESSION_VALUE_TYPE: [u8; 16] = expression_id(0xf8a7_7a04);
const EXPRESSION_REFERENCE_TYPE: [u8; 16] = expression_id(0xf8a7_7a05);
const EXPRESSION_ADD_TYPE: [u8; 16] = expression_id(0xf8a7_7a06);
const EXPRESSION_SUBTRACT_TYPE: [u8; 16] = expression_id(0xf8a7_7a07);
const EXPRESSION_MULTIPLY_TYPE: [u8; 16] = expression_id(0xf8a7_7a08);
const EXPRESSION_DIVIDE_TYPE: [u8; 16] = expression_id(0xf8a7_7a09);
const EXPRESSION_MODULO_TYPE: [u8; 16] = expression_id(0xf8a7_7a0a);
const EXPRESSION_POWER_TYPE: [u8; 16] = expression_id(0xf8a7_7a0b);
const EXPRESSION_NEGATE_TYPE: [u8; 16] = expression_id(0xf8a7_7a0c);
const EXPRESSION_POWER_IDENTITY_TYPE: [u8; 16] = expression_id(0xf8a7_7a0d);
const UNIT_TYPE: [u8; 16] = expression_id(0xf8a7_79fd);

/// Builds an expression or unit type identifier from the `time_low` field of
/// its GUID.
///
/// The remaining 12 identifier bytes are `d2118f09c0005a9a2378d04f`, where
/// [`inventor_id`] builds `d011f8d10008cabc0663dc09`.
const fn expression_id(time_low: u32) -> [u8; 16] {
    let first = time_low.to_le_bytes();
    [
        first[0], first[1], first[2], first[3], 0xd2, 0x11, 0x8f, 0x09, 0xc0, 0x00, 0x5a, 0x9a,
        0x23, 0x78, 0xd0, 0x4f,
    ]
}

const PARAMETER_FULL_TYPE: [u8; 16] = inventor_id(0x9087_4d26);
const MILLIMETRE_TYPE: [u8; 16] = [
    0xbc, 0x20, 0x41, 0x62, 0xd2, 0x11, 0x9b, 0x0b, 0x60, 0x00, 0x6a, 0xb7, 0x60, 0xfe, 0xc3, 0xb0,
];
const METRE_TYPE: [u8; 16] = expression_id(0xf8a7_79f5);
const INCH_TYPE: [u8; 16] = expression_id(0xf8a7_79f6);
const FOOT_TYPE: [u8; 16] = expression_id(0xf8a7_79f7);
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
    pub(crate) issues: Vec<RecordIssue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcParameterPayload {
    pub(crate) save_version_major: u8,
    #[serde(flatten)]
    pub(crate) header: PmDcContentHeader,
    pub(crate) name: String,
    pub(crate) name_value: u32,
    pub(crate) unit: PmDcReference,
    pub(crate) formula: PmDcReference,
    pub(crate) nominal_value: cadmpeg_ir::scalar::FiniteReal,
    pub(crate) model_value: cadmpeg_ir::scalar::FiniteReal,
    pub(crate) tolerance: u16,
    pub(crate) terminal_value: i16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcExpressionPayload {
    save_version_major: u8,
    header_value: u32,
    header_id: u16,
    pub(crate) unit: PmDcReference,
    pub(crate) kind: PmDcExpressionKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum PmDcExpressionKind {
    Value {
        value: FiniteReal,
        value_type: u16,
        state: u32,
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
pub(crate) struct PmDcUnitPayload {
    save_version_major: u8,
    header_value: u32,
    header_id: u16,
    pub(crate) kind: PmDcUnitKind,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "PmDcUnitKindWire")]
pub(crate) enum PmDcUnitKind {
    Definition {
        numerators: PmDcPairedReferenceList<[u16; 2]>,
        denominators: PmDcPairedReferenceList<[u16; 2]>,
        visible: bool,
        derived: PmDcReference,
    },
    Base {
        dimension: PmDcUnitDimension,
        symbol: String,
        scale_to_internal: FiniteReal,
        magnitude: FiniteReal,
        factor: FiniteReal,
    },
}

#[derive(Serialize)]
#[serde(tag = "form", rename_all = "snake_case")]
enum PmDcUnitKindRef<'a> {
    Definition {
        numerators: &'a [PmDcReference],
        numerator_metadata: Option<[u16; 2]>,
        denominators: &'a [PmDcReference],
        denominator_metadata: Option<[u16; 2]>,
        visible: bool,
        derived: PmDcReference,
    },
    Base {
        dimension: &'a PmDcUnitDimension,
        symbol: &'a str,
        scale_to_internal: &'a FiniteReal,
        magnitude: &'a FiniteReal,
        factor: &'a FiniteReal,
    },
}

impl Serialize for PmDcUnitKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let wire = match self {
            Self::Definition {
                numerators,
                denominators,
                visible,
                derived,
            } => PmDcUnitKindRef::Definition {
                numerators: numerators.references(),
                numerator_metadata: numerators.metadata().copied(),
                denominators: denominators.references(),
                denominator_metadata: denominators.metadata().copied(),
                visible: *visible,
                derived: *derived,
            },
            Self::Base {
                dimension,
                symbol,
                scale_to_internal,
                magnitude,
                factor,
            } => PmDcUnitKindRef::Base {
                dimension,
                symbol,
                scale_to_internal,
                magnitude,
                factor,
            },
        };
        wire.serialize(serializer)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
enum PmDcUnitKindWire {
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
        scale_to_internal: FiniteReal,
        magnitude: FiniteReal,
        factor: FiniteReal,
    },
}

impl From<PmDcUnitKind> for PmDcUnitKindWire {
    fn from(value: PmDcUnitKind) -> Self {
        match value {
            PmDcUnitKind::Definition {
                numerators,
                denominators,
                visible,
                derived,
            } => Self::Definition {
                numerator_metadata: numerators.metadata().copied(),
                denominator_metadata: denominators.metadata().copied(),
                numerators: numerators.into_references(),
                denominators: denominators.into_references(),
                visible,
                derived,
            },
            PmDcUnitKind::Base {
                dimension,
                symbol,
                scale_to_internal,
                magnitude,
                factor,
            } => Self::Base {
                dimension,
                symbol,
                scale_to_internal,
                magnitude,
                factor,
            },
        }
    }
}
impl TryFrom<PmDcUnitKindWire> for PmDcUnitKind {
    type Error = String;
    fn try_from(value: PmDcUnitKindWire) -> Result<Self, Self::Error> {
        Ok(match value {
            PmDcUnitKindWire::Definition {
                numerators,
                numerator_metadata,
                denominators,
                denominator_metadata,
                visible,
                derived,
            } => Self::Definition {
                numerators: PmDcPairedReferenceList::new(numerator_metadata, numerators)
                    .ok_or("unit numerator metadata disagrees with length")?,
                denominators: PmDcPairedReferenceList::new(denominator_metadata, denominators)
                    .ok_or("unit denominator metadata disagrees with length")?,
                visible,
                derived,
            },
            PmDcUnitKindWire::Base {
                dimension,
                symbol,
                scale_to_internal,
                magnitude,
                factor,
            } => Self::Base {
                dimension,
                symbol,
                scale_to_internal,
                magnitude,
                factor,
            },
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PmDcUnitDimension {
    Length,
    Angle,
    Dimensionless,
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
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        for record in &table.records {
            let result = if record.type_id == PARAMETER_FULL_TYPE {
                parse_parameter(ctx, record.payload, version).and_then(|value| {
                    push_record(
                        ctx,
                        &mut inventory.parameters,
                        value,
                        record.type_id,
                        segment.pair.token.key(),
                        record.ordinal,
                        "admit Inventor PmDc parameter record",
                    )
                })
            } else if let Some(operation) = binary_operation(record.type_id) {
                parse_binary_expression(record.payload, version, operation).and_then(|value| {
                    push_record(
                        ctx,
                        &mut inventory.expressions,
                        value,
                        record.type_id,
                        segment.pair.token.key(),
                        record.ordinal,
                        "admit Inventor PmDc expression record",
                    )
                })
            } else if let Some(operation) = unary_operation(record.type_id) {
                parse_unary_expression(record.payload, version, operation).and_then(|value| {
                    push_record(
                        ctx,
                        &mut inventory.expressions,
                        value,
                        record.type_id,
                        segment.pair.token.key(),
                        record.ordinal,
                        "admit Inventor PmDc expression record",
                    )
                })
            } else if record.type_id == EXPRESSION_VALUE_TYPE {
                parse_value_expression(record.payload, version).and_then(|value| {
                    push_record(
                        ctx,
                        &mut inventory.expressions,
                        value,
                        record.type_id,
                        segment.pair.token.key(),
                        record.ordinal,
                        "admit Inventor PmDc expression record",
                    )
                })
            } else if record.type_id == EXPRESSION_REFERENCE_TYPE {
                parse_reference_expression(record.payload, version).and_then(|value| {
                    push_record(
                        ctx,
                        &mut inventory.expressions,
                        value,
                        record.type_id,
                        segment.pair.token.key(),
                        record.ordinal,
                        "admit Inventor PmDc expression record",
                    )
                })
            } else if record.type_id == UNIT_TYPE {
                parse_unit_definition(record.payload, version).and_then(|value| {
                    push_record(
                        ctx,
                        &mut inventory.units,
                        value,
                        record.type_id,
                        segment.pair.token.key(),
                        record.ordinal,
                        "admit Inventor PmDc unit record",
                    )
                })
            } else if let Some((dimension, symbol, scale)) = base_unit(record.type_id) {
                parse_base_unit(record.payload, version, dimension, symbol, scale).and_then(
                    |value| {
                        push_record(
                            ctx,
                            &mut inventory.units,
                            value,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc unit record",
                        )
                    },
                )
            } else {
                continue;
            };
            if let Err(error) = result {
                if matches!(error, CodecError::ResourceLimit(_)) {
                    return Err(error);
                }
                ctx.charge_entities(1, "admit Inventor PmDc design issue")?;
                inventory.issues.push(RecordIssue {
                    family: RecordIssueFamily::Design {
                        type_id: crate::record_identity::RecordTypeId::from_bytes(record.type_id),
                    },
                    segment_token: segment.pair.token.key().clone(),
                    record_ordinal: record.ordinal,
                    detail: error.to_string(),
                });
            }
        }
    }
    Ok(inventory)
}

/// Parameters projected from one design inventory and the records left out.
pub(crate) struct ParameterProjection {
    pub(crate) parameters: Vec<DesignParameter>,
    /// Records whose unit, value or expression graph is not closed.
    pub(crate) unresolved: usize,
    /// Records whose expression renders past [`MAX_EXPRESSION_TEXT_BYTES`].
    pub(crate) bounded: usize,
}

/// The loss for parameter records whose expression renders past
/// [`MAX_EXPRESSION_TEXT_BYTES`], when there are any.
pub(crate) fn bounded_expression_loss(bounded: usize) -> Option<LossNote> {
    (bounded != 0).then(|| {
        InventorLossCode::ParameterExpressionBounded.note(format!(
            "Retained {bounded} Inventor parameter record(s) whose expression text would pass the {MAX_EXPRESSION_TEXT_BYTES}-byte expression text cap."
        ))
    })
}

pub(crate) fn project_parameters(
    ctx: &DecodeContext<'_>,
    inventory: &DesignInventory,
    admitted_entities: &mut u64,
) -> Result<ParameterProjection, CodecError> {
    let expressions = unique_index(inventory.expressions.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    let units = unique_index(inventory.units.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    let parameters = unique_index(inventory.parameters.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    let mut renderer = ExpressionRenderer::new(ctx, &expressions, &units, &parameters);
    let mut projected = Vec::new();
    let mut unresolved = 0usize;
    let mut bounded = 0usize;
    for parameter in &inventory.parameters {
        let token = parameter.identity.segment_token.as_str();
        if !parameters.contains_key(&(token, parameter.identity.record_ordinal)) {
            unresolved += 1;
            continue;
        }
        let Some(unit) = resolve_unit(token, parameter.unit.index(), &units) else {
            unresolved += 1;
            continue;
        };
        let expression_length = match renderer.shape(token, parameter.formula.index())? {
            ExpressionShape::Text(length) => length,
            ExpressionShape::Open => {
                unresolved += 1;
                continue;
            }
            ExpressionShape::Bounded => {
                bounded += 1;
                continue;
            }
        };
        let value = match unit.dimension {
            PmDcUnitDimension::Length => {
                Length::new(parameter.model_value.get() * 10.0).map(ParameterValue::Length)
            }
            PmDcUnitDimension::Angle => Some(ParameterValue::Angle(Angle::from_assigned_real(
                parameter.model_value,
            ))),
            PmDcUnitDimension::Dimensionless => Some(ParameterValue::Real(parameter.model_value)),
        };
        let Some(value) = value else {
            unresolved += 1;
            continue;
        };
        ctx.admit_entities(
            cadmpeg_core::decode::u64_from_index(projected.len()) + 1,
            admitted_entities,
            "project Inventor parameter",
        )?;
        let id = parameter_id(parameter)?;
        let (expression, dependencies) =
            renderer.emit(token, parameter.formula.index(), expression_length)?;
        let mut dependency_members = cadmpeg_ir::features::DistinctMembers::default();
        dependency_members.extend(dependencies);
        projected.push(DesignParameter {
            id,
            owner: None,
            ordinal: parameter.header.source_index,
            name: parameter.name.clone(),
            expression,
            display: None,
            value: Some(value),
            dependencies: dependency_members,
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: Some(parameter.id()),
        });
    }
    let (projected, graph_rejections) = close_parameter_graph(ctx, projected)?;
    *admitted_entities = cadmpeg_core::decode::u64_from_index(projected.len());
    Ok(ParameterProjection {
        parameters: projected,
        unresolved: unresolved + graph_rejections,
        bounded,
    })
}

fn close_parameter_graph(
    ctx: &DecodeContext<'_>,
    parameters: Vec<DesignParameter>,
) -> Result<(Vec<DesignParameter>, usize), CodecError> {
    let count = parameters.len();
    let indices = parameters
        .iter()
        .enumerate()
        .map(|(index, parameter)| (&parameter.id, index))
        .collect::<HashMap<_, _>>();
    let mut remaining = ctx.alloc_filled(count, 0usize, "admit Inventor parameter indegrees")?;
    let mut dependents = ctx.alloc_filled(
        count,
        Vec::<usize>::new(),
        "admit Inventor parameter adjacency",
    )?;
    let mut ready = VecDeque::new();
    for (index, parameter) in parameters.iter().enumerate() {
        for dependency in &parameter.dependencies {
            if let Some(&source) = indices.get(dependency) {
                remaining[index] += 1;
                dependents[source].push(index);
            } else {
                remaining[index] += 1;
            }
        }
        if remaining[index] == 0 {
            ready.push_back(index);
        }
    }
    let mut closed = ctx.alloc_filled(count, false, "admit Inventor parameter closure")?;
    while let Some(source) = ready.pop_front() {
        closed[source] = true;
        for &dependent in &dependents[source] {
            remaining[dependent] -= 1;
            if remaining[dependent] == 0 {
                ready.push_back(dependent);
            }
        }
    }
    let accepted = closed.iter().filter(|&&value| value).count();
    Ok((
        parameters
            .into_iter()
            .zip(closed)
            .filter_map(|(parameter, is_closed)| is_closed.then_some(parameter))
            .collect(),
        count - accepted,
    ))
}

fn parameter_id(parameter: &PmDcParameter) -> Result<ParameterId, CodecError> {
    Ok(ParameterId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "design", "parameter"),
        parameter.identity.key()?,
    ))
}

struct ResolvedUnit<'a> {
    dimension: PmDcUnitDimension,
    symbol: &'a str,
    scale_to_internal: FiniteReal,
}

fn resolve_unit<'a>(
    token: &str,
    reference: u32,
    units: &HashMap<(&str, u32), Option<&'a PmDcUnit>>,
) -> Option<ResolvedUnit<'a>> {
    let ordinal = reference.checked_sub(1)?;
    let definition = units.get(&(token, ordinal)).and_then(Option::as_ref)?;
    let PmDcUnitKind::Definition {
        numerators,
        denominators,
        derived,
        ..
    } = &definition.kind
    else {
        return None;
    };
    if numerators.references().len() != 1
        || !denominators.references().is_empty()
        || derived.index() != 0
    {
        return None;
    }
    let base_ordinal = numerators.references()[0].index().checked_sub(1)?;
    let base = units.get(&(token, base_ordinal)).and_then(Option::as_ref)?;
    let PmDcUnitKind::Base {
        dimension,
        symbol,
        scale_to_internal,
        magnitude: _,
        factor: _,
    } = &base.kind
    else {
        return None;
    };
    Some(ResolvedUnit {
        dimension: *dimension,
        symbol,
        scale_to_internal: *scale_to_internal,
    })
}

/// Bytes one rendered parameter expression may span. Each use of a shared
/// operand is rendered in full, so text can double with every graph level.
pub(crate) const MAX_EXPRESSION_TEXT_BYTES: usize = 1 << 16;

/// What an expression renders to, decided before any text is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpressionShape {
    /// The expression renders to this many bytes.
    Text(usize),
    /// A node it reaches does not resolve.
    Open,
    /// Its text would pass [`MAX_EXPRESSION_TEXT_BYTES`].
    Bounded,
}

impl ExpressionShape {
    fn of_length(length: Option<usize>) -> Self {
        length
            .filter(|length| *length <= MAX_EXPRESSION_TEXT_BYTES)
            .map_or(Self::Bounded, Self::Text)
    }
}

/// One measured node and the nesting depth its walk reached, itself included.
#[derive(Clone, Copy)]
struct MeasuredExpression {
    height: usize,
    shape: ExpressionShape,
}

impl MeasuredExpression {
    const UNRESOLVED_LEAF: Self = Self {
        height: 1,
        shape: ExpressionShape::Open,
    };

    fn leaf(length: Option<usize>) -> Self {
        length.map_or(Self::UNRESOLVED_LEAF, |length| Self {
            height: 1,
            shape: ExpressionShape::of_length(Some(length)),
        })
    }
}

enum ExpressionPiece {
    Node(u32),
    Literal(&'static str),
}

/// Renders parameter formulas over one inventory's expression graph.
///
/// Every node is measured once for all parameters. Text is built only for a
/// formula whose measured length fits [`MAX_EXPRESSION_TEXT_BYTES`].
struct ExpressionRenderer<'a, 'b> {
    ctx: &'b DecodeContext<'b>,
    expressions: &'b HashMap<(&'a str, u32), Option<&'a PmDcExpression>>,
    units: &'b HashMap<(&'a str, u32), Option<&'a PmDcUnit>>,
    parameters: &'b HashMap<(&'a str, u32), Option<&'a PmDcParameter>>,
    measured: HashMap<(&'a str, u32), MeasuredExpression>,
    visiting: HashSet<(&'a str, u32)>,
}

impl<'a, 'b> ExpressionRenderer<'a, 'b> {
    fn new(
        ctx: &'b DecodeContext<'b>,
        expressions: &'b HashMap<(&'a str, u32), Option<&'a PmDcExpression>>,
        units: &'b HashMap<(&'a str, u32), Option<&'a PmDcUnit>>,
        parameters: &'b HashMap<(&'a str, u32), Option<&'a PmDcParameter>>,
    ) -> Self {
        Self {
            ctx,
            expressions,
            units,
            parameters,
            measured: HashMap::new(),
            visiting: HashSet::new(),
        }
    }

    /// The shape of the expression `reference` names in segment `token`.
    fn shape(&mut self, token: &'a str, reference: u32) -> Result<ExpressionShape, CodecError> {
        Ok(self.measure(token, reference)?.shape)
    }

    fn measure(
        &mut self,
        token: &'a str,
        reference: u32,
    ) -> Result<MeasuredExpression, CodecError> {
        let _depth = self.ctx.enter_nested("walk Inventor expression graph")?;
        let Some(ordinal) = reference.checked_sub(1) else {
            return Ok(MeasuredExpression::UNRESOLVED_LEAF);
        };
        let key = (token, ordinal);
        if self.visiting.contains(&key) {
            return Err(CodecError::Malformed(
                "Inventor expression graph contains a cycle".into(),
            ));
        }
        if let Some(measured) = self.measured.get(&key).copied() {
            admit_cached_expression_depth(self.ctx, measured.height - 1)?;
            return Ok(measured);
        }
        let Some(expression) = self.expressions.get(&key).copied().flatten() else {
            return Ok(MeasuredExpression::UNRESOLVED_LEAF);
        };
        self.visiting.insert(key);
        let measured = self.measure_node(token, expression)?;
        self.visiting.remove(&key);
        self.measured.insert(key, measured);
        Ok(measured)
    }

    fn measure_node(
        &mut self,
        token: &'a str,
        expression: &'a PmDcExpression,
    ) -> Result<MeasuredExpression, CodecError> {
        Ok(match &expression.kind {
            PmDcExpressionKind::Value { .. } => MeasuredExpression::leaf(
                value_text(token, expression, self.units).map(|text| text.len()),
            ),
            PmDcExpressionKind::ParameterReference { operand } => MeasuredExpression::leaf(
                self.reference_target(token, operand.index())
                    .map(|target| target.name.len()),
            ),
            PmDcExpressionKind::Unary { operation, operand } => {
                let operand = self.measure(token, operand.index())?;
                let shape = match operand.shape {
                    ExpressionShape::Text(_) if *operation == PmDcUnaryOperation::PowerIdentity => {
                        ExpressionShape::Open
                    }
                    ExpressionShape::Text(length) => {
                        ExpressionShape::of_length(length.checked_add("-()".len()))
                    }
                    unrendered => unrendered,
                };
                MeasuredExpression {
                    height: operand.height + 1,
                    shape,
                }
            }
            PmDcExpressionKind::Binary {
                operation,
                left,
                right,
            } => {
                let left = self.measure(token, left.index())?;
                let ExpressionShape::Text(left_length) = left.shape else {
                    return Ok(MeasuredExpression {
                        height: left.height + 1,
                        shape: left.shape,
                    });
                };
                let right = self.measure(token, right.index())?;
                let shape = match right.shape {
                    ExpressionShape::Text(right_length) => ExpressionShape::of_length(
                        left_length
                            .checked_add(right_length)
                            .and_then(|length| length.checked_add("() ".len()))
                            .and_then(|length| length.checked_add(binary_symbol(*operation).len()))
                            .and_then(|length| length.checked_add(" ()".len())),
                    ),
                    unrendered => unrendered,
                };
                MeasuredExpression {
                    height: left.height.max(right.height) + 1,
                    shape,
                }
            }
        })
    }

    /// The text of a formula measured at `length` bytes, charged as retained
    /// output, and the parameters it references in first-use order.
    fn emit(
        &self,
        token: &'a str,
        reference: u32,
        length: usize,
    ) -> Result<(String, Vec<ParameterId>), CodecError> {
        self.ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(length),
            "retain Inventor parameter expression text",
        )?;
        let mut text = String::with_capacity(length);
        let mut dependencies = Vec::new();
        let mut referenced = HashSet::new();
        let mut pending = vec![ExpressionPiece::Node(reference)];
        while let Some(piece) = pending.pop() {
            let reference = match piece {
                ExpressionPiece::Literal(literal) => {
                    text.push_str(literal);
                    continue;
                }
                ExpressionPiece::Node(reference) => reference,
            };
            let expression = reference
                .checked_sub(1)
                .and_then(|ordinal| self.expressions.get(&(token, ordinal)).copied().flatten())
                .ok_or_else(expression_changed_after_measure)?;
            match &expression.kind {
                PmDcExpressionKind::Value { .. } => text.push_str(
                    &value_text(token, expression, self.units)
                        .ok_or_else(expression_changed_after_measure)?,
                ),
                PmDcExpressionKind::ParameterReference { operand } => {
                    let target = self
                        .reference_target(token, operand.index())
                        .ok_or_else(expression_changed_after_measure)?;
                    if referenced.insert(operand.index()) {
                        dependencies.push(parameter_id(target)?);
                    }
                    text.push_str(&target.name);
                }
                PmDcExpressionKind::Unary { operand, .. } => pending.extend([
                    ExpressionPiece::Literal(")"),
                    ExpressionPiece::Node(operand.index()),
                    ExpressionPiece::Literal("-("),
                ]),
                PmDcExpressionKind::Binary {
                    operation,
                    left,
                    right,
                } => pending.extend([
                    ExpressionPiece::Literal(")"),
                    ExpressionPiece::Node(right.index()),
                    ExpressionPiece::Literal(" ("),
                    ExpressionPiece::Literal(binary_symbol(*operation)),
                    ExpressionPiece::Literal(") "),
                    ExpressionPiece::Node(left.index()),
                    ExpressionPiece::Literal("("),
                ]),
            }
        }
        Ok((text, dependencies))
    }

    /// The parameter a reference expression names, when it resolves uniquely.
    fn reference_target(&self, token: &'a str, reference: u32) -> Option<&'a PmDcParameter> {
        let ordinal = reference.checked_sub(1)?;
        self.parameters.get(&(token, ordinal)).copied().flatten()
    }
}

/// The text of a value expression, or `None` when its unit or its value in
/// that unit does not resolve.
fn value_text(
    token: &str,
    expression: &PmDcExpression,
    units: &HashMap<(&str, u32), Option<&PmDcUnit>>,
) -> Option<String> {
    let PmDcExpressionKind::Value { value, .. } = &expression.kind else {
        return None;
    };
    let unit = resolve_unit(token, expression.unit.index(), units)?;
    if unit.scale_to_internal.get() == 0.0 {
        return None;
    }
    let scalar = FiniteReal::new(value.get() / unit.scale_to_internal.get())?;
    let mut text = if scalar.get() == 0.0 {
        "0".to_owned()
    } else {
        scalar.get().to_string()
    };
    if !unit.symbol.is_empty() {
        text.push(' ');
        text.push_str(unit.symbol);
    }
    Some(text)
}

const fn binary_symbol(operation: PmDcBinaryOperation) -> &'static str {
    match operation {
        PmDcBinaryOperation::Add => "+",
        PmDcBinaryOperation::Subtract => "-",
        PmDcBinaryOperation::Multiply => "*",
        PmDcBinaryOperation::Divide => "/",
        PmDcBinaryOperation::Modulo => "%",
        PmDcBinaryOperation::Power => "^",
    }
}

fn expression_changed_after_measure() -> CodecError {
    CodecError::Malformed("Inventor expression changed between measure and render".into())
}

fn admit_cached_expression_depth(
    ctx: &DecodeContext<'_>,
    remaining: usize,
) -> Result<(), CodecError> {
    if remaining == 0 {
        return Ok(());
    }
    let _depth = ctx.enter_nested("walk cached Inventor expression depth")?;
    admit_cached_expression_depth(ctx, remaining - 1)
}

fn parse_parameter(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcParameterPayload, CodecError> {
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
    Ok(PmDcParameterPayload {
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

fn parse_value_expression(
    source: View<'_>,
    version: u8,
) -> Result<PmDcExpressionPayload, CodecError> {
    let (mut cursor, header_value, header_id, unit) = expression_header(source)?;
    let value = cursor.f64("literal expression value")?;
    let value_type = cursor.u16("literal expression value type")?;
    let state = cursor.u32("literal expression state")?;
    cursor.finish("literal expression")?;
    Ok(PmDcExpressionPayload {
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

fn parse_reference_expression(
    source: View<'_>,
    version: u8,
) -> Result<PmDcExpressionPayload, CodecError> {
    let (mut cursor, header_value, header_id, unit) = expression_header(source)?;
    let operand = cursor.reference("parameter-reference expression operand")?;
    cursor.finish("parameter-reference expression")?;
    Ok(PmDcExpressionPayload {
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
) -> Result<PmDcExpressionPayload, CodecError> {
    let (mut cursor, header_value, header_id, unit) = expression_header(source)?;
    let operand = cursor.reference("unary expression operand")?;
    cursor.finish("unary expression")?;
    Ok(PmDcExpressionPayload {
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
) -> Result<PmDcExpressionPayload, CodecError> {
    let (mut cursor, header_value, header_id, unit) = expression_header(source)?;
    let left = cursor.reference("binary expression left operand")?;
    let right = cursor.reference("binary expression right operand")?;
    cursor.finish("binary expression")?;
    Ok(PmDcExpressionPayload {
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

fn parse_unit_definition(source: View<'_>, version: u8) -> Result<PmDcUnitPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header_value = cursor.u32("unit header value")?;
    let header_id = cursor.u16("unit header id")?;
    let numerators = cursor.reference_array("unit numerators")?;
    let denominators = cursor.reference_array("unit denominators")?;
    let visible = cursor.u8("unit visibility")? != 0;
    let derived = cursor.reference("unit derived reference")?;
    cursor.finish("unit")?;
    Ok(PmDcUnitPayload {
        save_version_major: version,
        header_value,
        header_id,
        kind: PmDcUnitKind::Definition {
            numerators,
            denominators,
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
) -> Result<PmDcUnitPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header_value = cursor.u32("base-unit header value")?;
    let header_id = cursor.u16("base-unit header id")?;
    let magnitude = cursor.f64("base-unit magnitude")?;
    let factor = cursor.f64("base-unit factor")?;
    cursor.finish("base unit")?;
    Ok(PmDcUnitPayload {
        save_version_major: version,
        header_value,
        header_id,
        kind: PmDcUnitKind::Base {
            dimension,
            symbol: symbol.into(),
            scale_to_internal: FiniteReal::new(scale_to_internal).ok_or_else(|| {
                CodecError::Malformed("Inventor PmDc base-unit scale is not finite".into())
            })?,
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
        DEGREE_TYPE => Some((
            PmDcUnitDimension::Angle,
            "deg",
            std::f64::consts::PI / 180.0,
        )),
        GRAD_TYPE => Some((
            PmDcUnitDimension::Angle,
            "grad",
            std::f64::consts::PI / 200.0,
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

impl Cursor<'_> {
    fn reference_array(
        &mut self,
        field: &str,
    ) -> Result<PmDcPairedReferenceList<[u16; 2]>, CodecError> {
        let marker = [
            self.u16("reference-array marker 0")?,
            self.u16("reference-array marker 1")?,
        ];
        if marker != [3, 0x3000] {
            return Err(CodecError::malformed(format_args!(
                "Inventor PmDc {field} marker is {marker:?}"
            )));
        }
        let count = usize::try_from(self.u32("reference-array count")?).map_err(|_| {
            CodecError::Malformed("Inventor numeric value exceeds target range".into())
        })?;
        let metadata = if count == 0 {
            None
        } else {
            Some([
                self.u16("reference-array metadata 0")?,
                self.u16("reference-array metadata 1")?,
            ])
        };
        let mut references = Vec::new();
        for _ in 0..count {
            references.push(self.reference("reference-array entry")?);
        }
        PmDcPairedReferenceList::new(metadata, references).ok_or_else(|| {
            CodecError::Malformed(
                "Inventor PmDc unit reference list metadata disagrees with length".into(),
            )
        })
    }
}

pub(crate) type PmDcParameter = Located<PmDcParameterPayload>;

impl RecordPayload for PmDcParameterPayload {
    const KIND: &'static str = "parameter";
}

pub(crate) type PmDcExpression = Located<PmDcExpressionPayload>;

impl RecordPayload for PmDcExpressionPayload {
    const KIND: &'static str = "expression";
}

pub(crate) type PmDcUnit = Located<PmDcUnitPayload>;

impl RecordPayload for PmDcUnitPayload {
    const KIND: &'static str = "unit";
}

#[cfg(test)]
mod tests {
    use super::{
        base_unit, bounded_expression_loss, close_parameter_graph, inventory,
        parse_binary_expression, parse_parameter, parse_unary_expression, parse_unit_definition,
        parse_value_expression, project_parameters, DesignInventory, ExpressionRenderer,
        ExpressionShape, PmDcBinaryOperation, PmDcExpressionKind, PmDcExpressionPayload,
        PmDcParameterPayload, PmDcUnaryOperation, PmDcUnitDimension, PmDcUnitKind, PmDcUnitPayload,
        DIMENSIONLESS_TYPE, EXPRESSION_ADD_TYPE, EXPRESSION_NEGATE_TYPE, EXPRESSION_REFERENCE_TYPE,
        EXPRESSION_VALUE_TYPE, GRAD_TYPE, MAX_EXPRESSION_TEXT_BYTES, PARAMETER_FULL_TYPE,
        UNIT_TYPE,
    };
    use crate::container::InventorContainer;
    use crate::loss::InventorLossCode;
    use crate::pmdc::{PmDcContentHeader, PmDcPairedReferenceList, PmDcReference};
    use crate::record_identity::Located;
    use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};
    use crate::test_support::test_fixtures::primary_envelope_fixture;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
    use cadmpeg_core::decode::{DecodeContext, View};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::features::{DesignParameter, ParameterId, ParameterValue};
    use cadmpeg_ir::scalar::{FiniteReal, Length};
    use std::collections::HashMap;

    fn reference(index: u32, qualified: bool) -> PmDcReference {
        PmDcReference::new(index, qualified).expect("test reference index fits 31 bits")
    }

    fn real(value: f64) -> FiniteReal {
        FiniteReal::new(value).expect("finite test scalar")
    }

    #[test]
    fn pmdc_expression_record_is_admitted() {
        let payload = [0_u8; 14];
        let admitted =
            inventory_with_record(EXPRESSION_REFERENCE_TYPE, &payload, DecodePolicy::service())
                .expect("expression is admitted");
        assert_eq!(admitted.expressions.len(), 1);
        assert!(admitted.issues.is_empty());
    }

    fn inventory_with_record(
        type_id: [u8; 16],
        payload: &[u8],
        policy: DecodePolicy,
    ) -> Result<DesignInventory, CodecError> {
        let bytes = primary_envelope_fixture();
        let payload = payload.to_vec();
        let arena = DecodeArena::new();
        let (setup_ctx, source) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("envelope view");
        let mut container = InventorContainer::open(&setup_ctx, source).expect("framed envelope");
        let segment = &mut container.rse.segments[0];
        segment.kind = SegmentKind::PmDc;
        let SegmentBulkState::Framed(bulk) = &mut segment.bulk else {
            panic!("framed bulk fixture");
        };
        let RecordFrameState::Framed(table) = &mut bulk.records else {
            panic!("framed record fixture");
        };
        table.records[0].type_id = type_id;
        table.records[0].payload = View::over_retained(&payload);
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("input view");
        inventory(&ctx, &container.rse)
    }

    #[test]
    fn pmdc_parameter_record_is_admitted() {
        let payload = [0_u8; 58];
        assert_eq!(
            inventory_with_record(PARAMETER_FULL_TYPE, &payload, DecodePolicy::service())
                .expect("parameter is admitted")
                .parameters
                .len(),
            1
        );
    }

    #[test]
    fn pmdc_expression_forms_are_admitted() {
        for (type_id, len) in [
            (EXPRESSION_REFERENCE_TYPE, 14),
            (EXPRESSION_NEGATE_TYPE, 14),
            (EXPRESSION_ADD_TYPE, 18),
            (EXPRESSION_VALUE_TYPE, 24),
        ] {
            let payload = vec![0_u8; len];
            assert_eq!(
                inventory_with_record(type_id, &payload, DecodePolicy::service())
                    .expect("expression is admitted")
                    .expressions
                    .len(),
                1
            );
        }
    }

    #[test]
    fn pmdc_unit_forms_are_admitted() {
        let mut definition = [0_u8; 27];
        definition[6..10].copy_from_slice(&[3, 0, 0, 0x30]);
        definition[14..18].copy_from_slice(&[3, 0, 0, 0x30]);
        for (type_id, payload) in [
            (UNIT_TYPE, definition.as_slice()),
            (DIMENSIONLESS_TYPE, &[0_u8; 22]),
        ] {
            assert_eq!(
                inventory_with_record(type_id, payload, DecodePolicy::service())
                    .expect("unit is admitted")
                    .units
                    .len(),
                1
            );
        }
    }

    #[test]
    fn pmdc_issue_is_recorded() {
        let payload = [];
        let admitted =
            inventory_with_record(EXPRESSION_REFERENCE_TYPE, &payload, DecodePolicy::service())
                .expect("invalid expression becomes an issue");
        assert_eq!(admitted.issues.len(), 1);
    }

    #[test]
    fn pmdc_design_issue_refuses_entity_limit_before_push() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        assert!(matches!(
            inventory_with_record(EXPRESSION_REFERENCE_TYPE, &[], policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "admit Inventor PmDc design issue"
        ));
        assert_eq!(
            inventory_with_record(EXPRESSION_REFERENCE_TYPE, &[], DecodePolicy::service())
                .expect("build the record inventory")
                .issues
                .len(),
            1
        );
    }

    #[test]
    fn pmdc_named_parameter_is_admitted() {
        let mut payload = [0_u8; 60];
        payload[22..26].copy_from_slice(&1_u32.to_le_bytes());
        payload[26..28].copy_from_slice(&97_u16.to_le_bytes());
        assert_eq!(
            inventory_with_record(PARAMETER_FULL_TYPE, &payload, DecodePolicy::service())
                .expect("named parameter is admitted")
                .parameters
                .len(),
            1
        );
    }

    #[test]
    fn pmdc_unit_reference_arrays_are_admitted() {
        let mut numerator = [0_u8; 35];
        numerator[6..10].copy_from_slice(&[3, 0, 0, 0x30]);
        numerator[10..14].copy_from_slice(&1_u32.to_le_bytes());
        numerator[22..26].copy_from_slice(&[3, 0, 0, 0x30]);
        let mut denominator = [0_u8; 35];
        denominator[6..10].copy_from_slice(&[3, 0, 0, 0x30]);
        denominator[14..18].copy_from_slice(&[3, 0, 0, 0x30]);
        denominator[18..22].copy_from_slice(&1_u32.to_le_bytes());
        for payload in [&numerator, &denominator] {
            assert_eq!(
                inventory_with_record(UNIT_TYPE, payload, DecodePolicy::service())
                    .expect("unit reference list is admitted")
                    .units
                    .len(),
                1
            );
        }
    }

    #[test]
    fn grad_units_use_a_four_hundredth_turn_and_the_grad_symbol() {
        let (dimension, symbol, scale) = base_unit(GRAD_TYPE).expect("grad unit is supported");
        assert_eq!(dimension, PmDcUnitDimension::Angle);
        assert_eq!(symbol, "grad");
        assert_eq!(scale, std::f64::consts::PI / 200.0);
    }

    #[test]
    fn overflowing_unit_quotient_is_not_rendered_as_an_expression() {
        let token = cadmpeg_ir::identity_key!("segment");
        let unit = Located::new(
            PmDcUnitPayload {
                save_version_major: 22,
                header_value: 0,
                header_id: 0,
                kind: PmDcUnitKind::Base {
                    dimension: PmDcUnitDimension::Length,
                    symbol: "mm".into(),
                    scale_to_internal: real(1.0e-308),
                    magnitude: real(1.0),
                    factor: real(1.0),
                },
            },
            crate::record_identity::RecordTypeId::from_bytes([0; 16]),
            token.clone(),
            0,
        );
        let expression = Located::new(
            PmDcExpressionPayload {
                save_version_major: 22,
                header_value: 0,
                header_id: 0,
                unit: reference(1, false),
                kind: PmDcExpressionKind::Value {
                    value: real(1.0e308),
                    value_type: 0,
                    state: 0,
                },
            },
            crate::record_identity::RecordTypeId::from_bytes([0; 16]),
            token.clone(),
            0,
        );
        let expressions = HashMap::from([((token.as_str(), 0), Some(&expression))]);
        let units = HashMap::from([((token.as_str(), 0), Some(&unit))]);
        let parameters = HashMap::new();
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("empty fixture view");
        let mut renderer = ExpressionRenderer::new(&ctx, &expressions, &units, &parameters);
        assert_eq!(
            renderer
                .shape(token.as_str(), 1)
                .expect("invalid scalar remains unresolved"),
            ExpressionShape::Open
        );
    }

    #[test]
    fn unit_definition_rejects_detached_reference_metadata() {
        let unit = PmDcUnitKind::Definition {
            numerators: PmDcPairedReferenceList::new(Some([3, 7]), vec![reference(1, false)])
                .expect("valid test fixture"),
            denominators: PmDcPairedReferenceList::new(None, Vec::new())
                .expect("valid test fixture"),
            visible: true,
            derived: reference(0, false),
        };
        let wire = serde_json::to_value(&unit).expect("valid test fixture");
        assert_eq!(wire["numerator_metadata"], serde_json::json!([3, 7]));
        assert_eq!(
            serde_json::from_value::<PmDcUnitKind>(wire.clone()).expect("valid test fixture"),
            unit
        );
        let mut missing_metadata = wire.clone();
        missing_metadata["numerator_metadata"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<PmDcUnitKind>(missing_metadata).is_err());
        let mut orphan_metadata = wire;
        orphan_metadata["denominator_metadata"] = serde_json::json!([3, 7]);
        assert!(serde_json::from_value::<PmDcUnitKind>(orphan_metadata).is_err());
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
        assert_eq!(parameter.model_value.get(), 60.96);
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
                value,
                state: 0,
                ..
            } if value.get() == 25.4
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
                    left,
                    right,
                    ..
                } if left.index() == 4 && left.qualified() && right.index() == 5 && !right.qualified()
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
                    operand,
                    ..
                } if operand.index() == 4 && operand.qualified()
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
        let (_, source) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("unit view");
        let unit = parse_unit_definition(source, 22).expect("unit definition parses");
        assert!(
            matches!(unit.kind, PmDcUnitKind::Definition { ref numerators, visible: true, .. } if numerators.references() == [reference(7, true)])
        );
    }

    #[test]
    fn parameter_dependencies_render_and_resolve() {
        let token = cadmpeg_ir::identity_key!("segment");
        let base = Located::new(
            PmDcUnitPayload {
                save_version_major: 22,
                header_value: 0,
                header_id: 0,
                kind: PmDcUnitKind::Base {
                    dimension: PmDcUnitDimension::Length,
                    symbol: "in".into(),
                    scale_to_internal: real(2.54),
                    magnitude: real(1.0),
                    factor: real(1.0),
                },
            },
            crate::record_identity::RecordTypeId::from_bytes([0; 16]),
            token.clone(),
            0,
        );
        let unit = Located::new(
            PmDcUnitPayload {
                save_version_major: 22,
                header_value: 0,
                header_id: 0,
                kind: PmDcUnitKind::Definition {
                    numerators: PmDcPairedReferenceList::new(
                        Some([0, 0]),
                        vec![reference(1, false)],
                    )
                    .expect("valid test fixture"),
                    denominators: PmDcPairedReferenceList::new(None, Vec::new())
                        .expect("valid test fixture"),
                    visible: true,
                    derived: reference(0, false),
                },
            },
            crate::record_identity::RecordTypeId::from_bytes([0; 16]),
            token.clone(),
            1,
        );
        let literal = Located::new(
            PmDcExpressionPayload {
                save_version_major: 22,
                header_value: 0,
                header_id: 0,
                unit: reference(2, false),
                kind: PmDcExpressionKind::Value {
                    value: real(60.96),
                    value_type: 0,
                    state: 0,
                },
            },
            crate::record_identity::RecordTypeId::from_bytes([0; 16]),
            token.clone(),
            2,
        );
        let first = Located::new(
            PmDcParameterPayload {
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
                nominal_value: real(60.96),
                model_value: real(60.96),
                tolerance: 0,
                terminal_value: -1,
            },
            crate::record_identity::RecordTypeId::from_bytes([0; 16]),
            token.clone(),
            3,
        );
        let reference_expression = Located::new(
            PmDcExpressionPayload {
                save_version_major: 22,
                header_value: 0,
                header_id: 0,
                unit: reference(2, false),
                kind: PmDcExpressionKind::ParameterReference {
                    operand: reference(4, true),
                },
            },
            crate::record_identity::RecordTypeId::from_bytes([0; 16]),
            token.clone(),
            4,
        );
        let second = Located::new(
            PmDcParameterPayload {
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
                nominal_value: real(60.96),
                model_value: real(60.96),
                tolerance: 0,
                terminal_value: -1,
            },
            crate::record_identity::RecordTypeId::from_bytes([0; 16]),
            token.clone(),
            5,
        );
        let inventory = DesignInventory {
            parameters: vec![first, second],
            expressions: vec![literal, reference_expression],
            units: vec![base, unit],
            issues: Vec::new(),
        };
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("empty fixture view");
        let mut admitted_entities = 0;
        let projection = project_parameters(&ctx, &inventory, &mut admitted_entities)
            .expect("parameter projection");
        assert_eq!((projection.unresolved, projection.bounded), (0, 0));
        let parameters = projection.parameters;
        assert_eq!(parameters[0].expression, "24 in");
        assert_eq!(parameters[1].expression, "width");
        assert_eq!(
            parameters[1].dependencies.as_slice(),
            vec![parameters[0].id.clone()]
        );
        assert_eq!(
            parameters[0].value,
            Some(ParameterValue::Length(
                Length::new(609.6).expect("finite length fixture")
            ))
        );
    }

    fn single_parameter_inventory() -> DesignInventory {
        let token = cadmpeg_ir::identity_key!("segment");
        let base = Located::new(
            PmDcUnitPayload {
                save_version_major: 22,
                header_value: 0,
                header_id: 0,
                kind: PmDcUnitKind::Base {
                    dimension: PmDcUnitDimension::Dimensionless,
                    symbol: String::new(),
                    scale_to_internal: real(1.0),
                    magnitude: real(1.0),
                    factor: real(1.0),
                },
            },
            crate::record_identity::RecordTypeId::from_bytes([0; 16]),
            token.clone(),
            0,
        );
        let unit = Located::new(
            PmDcUnitPayload {
                save_version_major: 22,
                header_value: 0,
                header_id: 0,
                kind: PmDcUnitKind::Definition {
                    numerators: PmDcPairedReferenceList::new(
                        Some([0, 0]),
                        vec![reference(1, false)],
                    )
                    .expect("valid test fixture"),
                    denominators: PmDcPairedReferenceList::new(None, Vec::new())
                        .expect("valid test fixture"),
                    visible: true,
                    derived: reference(0, false),
                },
            },
            crate::record_identity::RecordTypeId::from_bytes([0; 16]),
            token.clone(),
            1,
        );
        let literal = Located::new(
            PmDcExpressionPayload {
                save_version_major: 22,
                header_value: 0,
                header_id: 0,
                unit: reference(2, false),
                kind: PmDcExpressionKind::Value {
                    value: real(1.0),
                    value_type: 0,
                    state: 0,
                },
            },
            crate::record_identity::RecordTypeId::from_bytes([0; 16]),
            token.clone(),
            2,
        );
        let parameter = Located::new(
            PmDcParameterPayload {
                save_version_major: 22,
                header: PmDcContentHeader {
                    header_value: 0,
                    header_id: 0,
                    next: reference(0, false),
                    flags: 0,
                    context: reference(0, false),
                    source_index: 0,
                },
                name: "ratio".into(),
                name_value: 0,
                unit: reference(2, false),
                formula: reference(3, false),
                nominal_value: real(1.0),
                model_value: real(1.0),
                tolerance: 0,
                terminal_value: -1,
            },
            crate::record_identity::RecordTypeId::from_bytes([0; 16]),
            token.clone(),
            3,
        );
        DesignInventory {
            parameters: vec![parameter],
            expressions: vec![literal],
            units: vec![base, unit],
            issues: Vec::new(),
        }
    }

    fn project_single_parameter(policy: DecodePolicy) -> Result<Vec<DesignParameter>, CodecError> {
        let inventory = single_parameter_inventory();
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty fixture view");
        let mut admitted_entities = 0;
        project_parameters(&ctx, &inventory, &mut admitted_entities)
            .map(|projection| projection.parameters)
    }

    #[test]
    fn parameter_projection_refuses_entity_limit_before_output_creation() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        assert!(matches!(
            project_single_parameter(policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "project Inventor parameter"
                    && limit.used == 0
        ));
    }

    #[test]
    fn parameter_projection_identity_strings() {
        let admitted = project_single_parameter(DecodePolicy::service())
            .expect("parameter is admitted under service limits");
        assert_eq!(admitted.len(), 1);
        let parameter = &admitted[0];
        assert_eq!(parameter.id.as_str(), "inventor:design:parameter#segment-3");
        assert_eq!(
            parameter.native_ref.as_deref(),
            Some("inventor:pmdc:parameter#segment-3")
        );
    }

    #[test]
    fn rejects_parameter_cycles_and_their_dependents() {
        let make = |name: &str, dependencies: Vec<ParameterId>| DesignParameter {
            id: ParameterId::mint(format!("synthetic:test:id#{name}")).expect("identity grammar"),
            owner: None,
            ordinal: 0,
            name: name.into(),
            expression: name.into(),
            display: None,
            value: Some(ParameterValue::Real(
                cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite scalar fixture"),
            )),
            dependencies: cadmpeg_ir::features::DistinctMembers::try_from(dependencies)
                .expect("valid test fixture"),
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: None,
        };
        let parameters = vec![
            make(
                "a",
                vec![ParameterId::mint("synthetic:test:id#b").expect("identity grammar")],
            ),
            make(
                "b",
                vec![ParameterId::mint("synthetic:test:id#a").expect("identity grammar")],
            ),
            make(
                "c",
                vec![ParameterId::mint("synthetic:test:id#a").expect("identity grammar")],
            ),
            make("d", Vec::new()),
        ];
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("empty fixture view");
        let (closed, rejected) = close_parameter_graph(&ctx, parameters).expect("closure");
        assert_eq!(rejected, 3);
        assert_eq!(
            closed
                .into_iter()
                .map(|parameter| cadmpeg_ir::ids::Identity::from(parameter.id).into_string())
                .collect::<Vec<_>>(),
            ["synthetic:test:id#d"]
        );
    }

    #[test]
    fn reverse_ordered_parameter_chain_keeps_source_order_after_topological_closure() {
        let id = |name: &str| {
            ParameterId::mint(format!("synthetic:test:id#{name}")).expect("identity grammar")
        };
        let make = |name: &str, dependency: Option<&str>| DesignParameter {
            id: id(name),
            owner: None,
            ordinal: 0,
            name: name.into(),
            expression: name.into(),
            display: None,
            value: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::try_from(
                dependency.into_iter().map(id).collect::<Vec<_>>(),
            )
            .expect("valid dependency fixture"),
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: None,
        };
        let parameters = vec![make("c", Some("b")), make("b", Some("a")), make("a", None)];
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("empty fixture view");
        let (closed, rejected) = close_parameter_graph(&ctx, parameters).expect("closure");
        assert_eq!(rejected, 0);
        assert_eq!(
            closed
                .iter()
                .map(|parameter| parameter.name.as_str())
                .collect::<Vec<_>>(),
            ["c", "b", "a"]
        );
    }

    fn render_graph(
        policy: &DecodePolicy,
        kinds: Vec<PmDcExpressionKind>,
        root: u32,
    ) -> Result<Option<(String, Vec<ParameterId>)>, CodecError> {
        let token = cadmpeg_ir::identity_key!("segment");
        let parameter = Located::new(
            PmDcParameterPayload {
                save_version_major: 22,
                header: PmDcContentHeader {
                    header_value: 0,
                    header_id: 0,
                    next: reference(0, false),
                    flags: 0,
                    context: reference(0, false),
                    source_index: 0,
                },
                name: "x".into(),
                name_value: 0,
                unit: reference(0, false),
                formula: reference(0, false),
                nominal_value: real(0.0),
                model_value: real(0.0),
                tolerance: 0,
                terminal_value: 0,
            },
            crate::record_identity::RecordTypeId::from_bytes([0; 16]),
            token.clone(),
            0,
        );
        let nodes = kinds
            .into_iter()
            .enumerate()
            .map(|(ordinal, kind)| {
                Located::new(
                    PmDcExpressionPayload {
                        save_version_major: 22,
                        header_value: 0,
                        header_id: 0,
                        unit: reference(0, false),
                        kind,
                    },
                    crate::record_identity::RecordTypeId::from_bytes([0; 16]),
                    token.clone(),
                    u32::try_from(ordinal).expect("small fixture"),
                )
            })
            .collect::<Vec<_>>();
        let expressions = nodes
            .iter()
            .map(|node| ((token.as_str(), node.identity.record_ordinal), Some(node)))
            .collect::<HashMap<_, _>>();
        let parameters = HashMap::from([((token.as_str(), 0), Some(&parameter))]);
        let units = HashMap::new();
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
        let mut renderer = ExpressionRenderer::new(&ctx, &expressions, &units, &parameters);
        match renderer.shape(token.as_str(), root)? {
            ExpressionShape::Text(length) => renderer.emit(token.as_str(), root, length).map(Some),
            ExpressionShape::Open | ExpressionShape::Bounded => Ok(None),
        }
    }

    fn reference_leaf() -> PmDcExpressionKind {
        PmDcExpressionKind::ParameterReference {
            operand: reference(1, false),
        }
    }

    fn shared_add(previous: u32) -> PmDcExpressionKind {
        PmDcExpressionKind::Binary {
            operation: PmDcBinaryOperation::Add,
            left: reference(previous, false),
            right: reference(previous, false),
        }
    }

    #[test]
    fn shared_expression_dag_renders_once_per_node_with_original_text_and_dependencies() {
        let result = render_graph(
            &DecodePolicy::service(),
            vec![reference_leaf(), shared_add(1)],
            2,
        )
        .expect("admitted graph")
        .expect("closed graph");
        assert_eq!(result.0, "(x) + (x)");
        assert_eq!(result.1.len(), 1);
    }

    #[test]
    fn expression_cycle_is_malformed_at_the_expression_aggregate() {
        let kinds = vec![
            PmDcExpressionKind::Unary {
                operation: PmDcUnaryOperation::Negate,
                operand: reference(2, false),
            },
            PmDcExpressionKind::Unary {
                operation: PmDcUnaryOperation::Negate,
                operand: reference(1, false),
            },
        ];
        assert!(matches!(
            render_graph(&DecodePolicy::service(), kinds, 1),
            Err(CodecError::Malformed(message)) if message.contains("expression graph contains a cycle")
        ));
    }

    #[test]
    fn expression_chain_refuses_recursion_depth_before_render() {
        let kinds = vec![
            reference_leaf(),
            PmDcExpressionKind::Unary {
                operation: PmDcUnaryOperation::Negate,
                operand: reference(1, false),
            },
            PmDcExpressionKind::Unary {
                operation: PmDcUnaryOperation::Negate,
                operand: reference(2, false),
            },
        ];
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 2;
        assert!(matches!(
            render_graph(&policy, kinds, 3),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RecursionDepth
                    && limit.operation == "walk Inventor expression graph"
        ));
    }

    #[test]
    fn cached_expression_subtree_refuses_deeper_reuse() {
        let kinds = vec![
            reference_leaf(),
            PmDcExpressionKind::Unary {
                operation: PmDcUnaryOperation::Negate,
                operand: reference(1, false),
            },
            PmDcExpressionKind::Unary {
                operation: PmDcUnaryOperation::Negate,
                operand: reference(2, false),
            },
            PmDcExpressionKind::Binary {
                operation: PmDcBinaryOperation::Add,
                left: reference(2, false),
                right: reference(3, false),
            },
        ];
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 3;
        assert!(matches!(
            render_graph(&policy, kinds, 4),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RecursionDepth
                    && limit.operation == "walk cached Inventor expression depth"
        ));
    }

    #[test]
    fn expression_dependency_id_is_collected() {
        let admitted = render_graph(&DecodePolicy::service(), vec![reference_leaf()], 1)
            .expect("graph is admitted")
            .expect("graph is closed");
        assert_eq!(admitted.0, "x");
        assert_eq!(admitted.1.len(), 1);
    }

    fn value_one() -> PmDcExpressionKind {
        PmDcExpressionKind::Value {
            value: real(1.0),
            value_type: 0,
            state: 0,
        }
    }

    /// The reference to the `index`th expression of a [`formula_inventory`].
    fn formula_node(index: u32) -> PmDcReference {
        reference(3 + index, false)
    }

    /// One segment with a symbol-free dimensionless unit, `kinds` as its
    /// expressions, and one parameter per formula reference.
    fn formula_inventory(
        kinds: Vec<PmDcExpressionKind>,
        formulas: &[PmDcReference],
    ) -> DesignInventory {
        let token = cadmpeg_ir::identity_key!("segment");
        let type_id = || crate::record_identity::RecordTypeId::from_bytes([0; 16]);
        let base = Located::new(
            PmDcUnitPayload {
                save_version_major: 22,
                header_value: 0,
                header_id: 0,
                kind: PmDcUnitKind::Base {
                    dimension: PmDcUnitDimension::Dimensionless,
                    symbol: String::new(),
                    scale_to_internal: real(1.0),
                    magnitude: real(1.0),
                    factor: real(1.0),
                },
            },
            type_id(),
            token.clone(),
            0,
        );
        let unit = Located::new(
            PmDcUnitPayload {
                save_version_major: 22,
                header_value: 0,
                header_id: 0,
                kind: PmDcUnitKind::Definition {
                    numerators: PmDcPairedReferenceList::new(
                        Some([0, 0]),
                        vec![reference(1, false)],
                    )
                    .expect("valid test fixture"),
                    denominators: PmDcPairedReferenceList::new(None, Vec::new())
                        .expect("valid test fixture"),
                    visible: true,
                    derived: reference(0, false),
                },
            },
            type_id(),
            token.clone(),
            1,
        );
        let first_parameter = 2 + u32::try_from(kinds.len()).expect("small fixture");
        let expressions = kinds
            .into_iter()
            .zip(2..)
            .map(|(kind, ordinal)| {
                Located::new(
                    PmDcExpressionPayload {
                        save_version_major: 22,
                        header_value: 0,
                        header_id: 0,
                        unit: reference(2, false),
                        kind,
                    },
                    type_id(),
                    token.clone(),
                    ordinal,
                )
            })
            .collect();
        let parameters = formulas
            .iter()
            .zip(0..)
            .map(|(formula, index)| {
                Located::new(
                    PmDcParameterPayload {
                        save_version_major: 22,
                        header: PmDcContentHeader {
                            header_value: 0,
                            header_id: 0,
                            next: reference(0, false),
                            flags: 0,
                            context: reference(0, false),
                            source_index: index,
                        },
                        name: format!("p{index}"),
                        name_value: 0,
                        unit: reference(2, false),
                        formula: *formula,
                        nominal_value: real(1.0),
                        model_value: real(1.0),
                        tolerance: 0,
                        terminal_value: -1,
                    },
                    type_id(),
                    token.clone(),
                    first_parameter + index,
                )
            })
            .collect();
        DesignInventory {
            parameters,
            expressions,
            units: vec![base, unit],
            issues: Vec::new(),
        }
    }

    /// A literal `1` and `levels` additions of the previous node to itself.
    /// Level `n` renders to `8 * 2^n - 7` bytes.
    fn doubling_chain(levels: u32) -> Vec<PmDcExpressionKind> {
        std::iter::once(value_one())
            .chain((0..levels).map(|level| PmDcExpressionKind::Binary {
                operation: PmDcBinaryOperation::Add,
                left: formula_node(level),
                right: formula_node(level),
            }))
            .collect()
    }

    fn project(
        policy: &DecodePolicy,
        inventory: &DesignInventory,
    ) -> Result<super::ParameterProjection, CodecError> {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
        project_parameters(&ctx, inventory, &mut 0)
    }

    #[test]
    fn expression_past_the_text_cap_is_skipped_and_reported() {
        let inventory =
            formula_inventory(doubling_chain(14), &[formula_node(13), formula_node(14)]);

        let projection =
            project(&DecodePolicy::service(), &inventory).expect("one parameter is projected");

        assert_eq!((projection.unresolved, projection.bounded), (0, 1));
        assert_eq!(projection.parameters.len(), 1);
        assert_eq!(projection.parameters[0].name, "p0");
        let widest = (0..13).fold("1".to_owned(), |text, _| format!("({text}) + ({text})"));
        assert_eq!(projection.parameters[0].expression, widest);
        assert!(widest.len() <= MAX_EXPRESSION_TEXT_BYTES);
        assert!(2 * widest.len() + "() + ()".len() > MAX_EXPRESSION_TEXT_BYTES);
        assert!(bounded_expression_loss(0).is_none());
        let loss =
            bounded_expression_loss(projection.bounded).expect("a bounded record is reported");
        assert_eq!(
            loss.code,
            InventorLossCode::ParameterExpressionBounded.kind()
        );
        assert!(loss
            .message
            .starts_with("Retained 1 Inventor parameter record(s)"));
        assert!(loss.message.contains("65536-byte expression text cap"));
    }

    #[test]
    fn parameters_sharing_a_formula_measure_it_once_and_render_the_same_text() {
        const LEAVES: u32 = 1 << 11;
        const OPEN_SHARERS: usize = 1 << 12;
        const CLOSED_SHARERS: usize = 8;
        let mut kinds = doubling_chain(12);
        let closed_root = formula_node(12);
        let first_leaf = u32::try_from(kinds.len()).expect("small fixture");
        kinds.extend((1..LEAVES).map(|_| value_one()));
        kinds.push(PmDcExpressionKind::ParameterReference {
            operand: reference(0, false),
        });
        let mut level = (first_leaf..first_leaf + LEAVES).collect::<Vec<_>>();
        while level.len() > 1 {
            level = level
                .chunks(2)
                .map(|pair| {
                    kinds.push(PmDcExpressionKind::Binary {
                        operation: PmDcBinaryOperation::Add,
                        left: formula_node(pair[0]),
                        right: formula_node(pair[1]),
                    });
                    u32::try_from(kinds.len() - 1).expect("small fixture")
                })
                .collect();
        }
        let open_root = formula_node(level[0]);
        let formulas = std::iter::repeat_n(open_root, OPEN_SHARERS)
            .chain(std::iter::repeat_n(closed_root, CLOSED_SHARERS))
            .collect::<Vec<_>>();
        let inventory = formula_inventory(kinds, &formulas);

        let projection =
            project(&DecodePolicy::service(), &inventory).expect("shared formulas are projected");

        assert_eq!(
            (projection.unresolved, projection.bounded),
            (OPEN_SHARERS, 0)
        );
        assert_eq!(projection.parameters.len(), CLOSED_SHARERS);
        let expected = (0..12).fold("1".to_owned(), |text, _| format!("({text}) + ({text})"));
        assert_eq!(expected.len(), (8 << 12) - 7);
        assert!(projection.parameters.iter().all(|parameter| {
            parameter.expression == expected && parameter.dependencies.as_slice().is_empty()
        }));
    }

    #[test]
    fn expression_text_is_charged_as_retained_bytes() {
        let inventory = formula_inventory(doubling_chain(13), &[formula_node(13)]);
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 1 << 12;

        assert!(matches!(
            project(&policy, &inventory),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor parameter expression text"
        ));
    }

    mod serialization;
}
