// SPDX-License-Identifier: Apache-2.0
//! Structural grammar for legacy ASCII persistence records.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use serde::{Serialize, Serializer};

const PRINCIPAL_UNIT_NAME: &str = "principal_sys_units";
const MILLIMETER_NEWTON_SECOND: &[u8] = b"millimeter Newton Second (mmNs)";
const INCH_POUND_MASS_SECOND: &[u8] = b"Inch lbm Second (Pro/E Default)";

/// Active coordinate-unit system selected by a model-level persistence field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrincipalUnitSystem {
    /// Millimeter, Newton, second.
    MillimeterNewtonSecond,
    /// Millimeter, kilogram, second.
    MillimeterKilogramSecond,
    /// Inch, pound mass, second.
    InchPoundMassSecond,
    /// A binary selector whose unit definition is not known.
    UnknownBinarySelector(u8),
}

impl PrincipalUnitSystem {
    /// Stable source-metadata token.
    pub fn token(self) -> String {
        match self {
            Self::MillimeterNewtonSecond => "mmNs".to_string(),
            Self::MillimeterKilogramSecond => "mmKs".to_string(),
            Self::InchPoundMassSecond => "inLbmS".to_string(),
            Self::UnknownBinarySelector(value) => format!("unknown:{value}"),
        }
    }

    /// Scale from stored coordinate lengths to canonical millimeters.
    pub const fn length_scale_mm(self) -> Option<f64> {
        match self {
            Self::MillimeterNewtonSecond | Self::MillimeterKilogramSecond => Some(1.0),
            Self::InchPoundMassSecond => Some(25.4),
            Self::UnknownBinarySelector(_) => None,
        }
    }
}

/// One finite legacy type-2 real, stored by its exact IEEE-754 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Real(u64);

impl Real {
    /// Numeric value represented by the stored bits.
    pub fn value(self) -> f64 {
        f64::from_bits(self.0)
    }
}

impl Serialize for Real {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_f64(self.value())
    }
}

/// One run in a numeric legacy array.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NumericRun<T> {
    /// Number of consecutive array elements carrying `value`.
    pub count: u32,
    /// Element value.
    pub value: T,
}

/// Complete semantic payload of one numeric legacy value row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub enum NumericPayload<T> {
    /// One scalar value.
    Scalar {
        /// Scalar value.
        value: T,
    },
    /// A complete multidimensional array, retained as source runs.
    Array {
        /// Array extents from outermost to innermost dimension.
        dimensions: Vec<u32>,
        /// Ordered source runs whose count sum equals the extent product.
        runs: Vec<NumericRun<T>>,
    },
}

impl<T> NumericPayload<T> {
    /// Number of logical scalar elements represented by this payload.
    pub fn element_count(&self) -> u64 {
        match self {
            Self::Scalar { .. } => 1,
            Self::Array { runs, .. } => runs.iter().map(|run| u64::from(run.count)).sum(),
        }
    }
}

/// One completely decoded numeric legacy attribute value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NumericRecord<T> {
    /// Globally unique native record identity.
    pub id: String,
    /// Declared attribute name.
    pub name: String,
    /// Scope-local declaration identifier.
    pub attribute_id: u32,
    /// Byte offset of the owning attribute-ID scope.
    pub scope_offset: usize,
    /// Owning type-0 object node, when the depth tree supplies one.
    pub parent: Option<String>,
    /// Object-tree nesting depth of the scalar or array header.
    pub depth: u32,
    /// Complete typed value.
    pub payload: NumericPayload<T>,
    /// Byte offset of the scalar row or array header.
    pub offset: usize,
}

/// One run in a type-2 real array.
pub type RealRun = NumericRun<Real>;
/// Complete semantic payload of one legacy type-2 value row.
pub type RealPayload = NumericPayload<Real>;
/// One completely decoded legacy type-2 attribute value.
pub type RealRecord = NumericRecord<Real>;
/// One run in a type-1 integer array.
pub type IntegerRun = NumericRun<i32>;
/// Complete semantic payload of one legacy type-1 value row.
pub type IntegerPayload = NumericPayload<i32>;
/// One completely decoded legacy type-1 attribute value.
pub type IntegerRecord = NumericRecord<i32>;

/// Structural payload of one legacy type-0 object node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub enum ObjectPayload {
    /// The `->` object token.
    Arrow,
    /// The empty inline-object payload.
    Inline,
    /// The `NULL` object token.
    Null,
    /// A dimensioned object array and its direct element nodes.
    Array {
        /// Array extents from outermost to innermost dimension.
        dimensions: Vec<u32>,
        /// Direct child object identities in source order.
        elements: Vec<String>,
        /// Whether element cardinality equals the extent product.
        complete: bool,
    },
    /// A type-0 payload outside the defined object forms.
    Opaque {
        /// Uninterpreted payload bytes after the attribute identifier.
        bytes: Vec<u8>,
    },
}

/// One legacy type-0 object node in the depth-defined ownership tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObjectRecord {
    /// Globally unique native object-node identity.
    pub id: String,
    /// Declared attribute name.
    pub name: String,
    /// Scope-local declaration identifier.
    pub attribute_id: u32,
    /// Byte offset of the owning attribute-ID scope.
    pub scope_offset: usize,
    /// Owning type-0 object node, when the depth tree supplies one.
    pub parent: Option<String>,
    /// Object-tree nesting depth.
    pub depth: u32,
    /// Stored object form.
    pub payload: ObjectPayload,
    /// Byte offset of the value row.
    pub offset: usize,
}

/// One unique `@<name> <id> <type-code>` declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeDeclaration {
    /// Attribute identifier referenced by value rows in the same scope.
    pub id: u32,
    /// Attribute name without the leading `@`.
    pub name: String,
    /// Stored numeric type code.
    pub type_code: u8,
    /// Byte offset of the declaration line.
    pub offset: usize,
}

/// One `<depth> <attribute-id> <payload>` value row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeValue {
    /// Object-tree nesting depth.
    pub depth: u32,
    /// Identifier of the owning attribute declaration in the same scope.
    pub attribute_id: u32,
    /// Byte offset of the value row.
    pub offset: usize,
    /// Byte range of the payload after the second field separator.
    pub payload: Range<usize>,
    /// Contiguous source range containing immediately following `$` rows.
    pub continuation_rows: Option<Range<usize>>,
    /// Number of immediately following `$` rows.
    pub continuation_count: usize,
}

/// Declarations and values owned by one outer object or named ASCII section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    /// Complete byte extent scanned for this scope.
    pub range: Range<usize>,
    /// First declaration for each identifier, in source order.
    pub declarations: Vec<AttributeDeclaration>,
    /// Value rows whose declaration resolves uniquely in this scope.
    pub values: Vec<AttributeValue>,
    /// Numeric value rows whose identifier has no unique local declaration.
    pub unresolved_value_count: usize,
    /// Repeated local identifiers whose name or type code conflicts.
    pub conflicting_declaration_count: usize,
}

/// Structurally resolved legacy ASCII persistence scopes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Persistence {
    /// Outer persistence scope followed by each named ASCII section scope.
    pub scopes: Vec<Scope>,
    /// Complete finite type-2 scalar and array values in source order.
    pub real_values: Vec<RealRecord>,
    /// Type-2 value rows not represented by a complete scalar or owning array.
    pub unresolved_real_value_count: usize,
    /// Complete type-1 signed-integer scalars and arrays in source order.
    pub integer_values: Vec<IntegerRecord>,
    /// Type-1 value rows not represented by a complete scalar or owning array.
    pub unresolved_integer_value_count: usize,
    /// Type-0 object nodes in source order.
    pub objects: Vec<ObjectRecord>,
    /// Type-0 arrays whose direct element count differs from their extents.
    pub incomplete_object_array_count: usize,
    /// Type-0 value rows outside the defined object forms.
    pub unresolved_object_value_count: usize,
}

impl Persistence {
    /// Number of unique local attribute declarations across all scopes.
    pub fn declaration_count(&self) -> usize {
        self.scopes
            .iter()
            .map(|scope| scope.declarations.len())
            .sum()
    }

    /// Number of structurally resolved value rows across all scopes.
    pub fn value_count(&self) -> usize {
        self.scopes.iter().map(|scope| scope.values.len()).sum()
    }

    /// Number of `$` continuation rows across all resolved values.
    pub fn continuation_count(&self) -> usize {
        self.scopes
            .iter()
            .flat_map(|scope| &scope.values)
            .map(|value| value.continuation_count)
            .sum()
    }

    /// Number of numeric rows without a unique declaration in their scope.
    pub fn unresolved_value_count(&self) -> usize {
        self.scopes
            .iter()
            .map(|scope| scope.unresolved_value_count)
            .sum()
    }

    /// Number of conflicting declaration identifiers across all scopes.
    pub fn conflicting_declaration_count(&self) -> usize {
        self.scopes
            .iter()
            .map(|scope| scope.conflicting_declaration_count)
            .sum()
    }

    /// Resolve one unambiguous legacy principal-unit string.
    pub fn principal_unit_system(&self, data: &[u8]) -> Option<PrincipalUnitSystem> {
        let mut candidate = None;
        for scope in &self.scopes {
            for declaration in scope
                .declarations
                .iter()
                .filter(|declaration| declaration.name == PRINCIPAL_UNIT_NAME)
            {
                for value in scope
                    .values
                    .iter()
                    .filter(|value| value.attribute_id == declaration.id)
                {
                    if candidate.is_some()
                        || declaration.type_code != 10
                        || value.continuation_count != 0
                    {
                        return None;
                    }
                    candidate = match data.get(value.payload.clone())? {
                        MILLIMETER_NEWTON_SECOND => {
                            Some(PrincipalUnitSystem::MillimeterNewtonSecond)
                        }
                        INCH_POUND_MASS_SECOND => Some(PrincipalUnitSystem::InchPoundMassSecond),
                        _ => return None,
                    };
                }
            }
        }
        candidate
    }
}

pub(crate) fn line(data: &[u8], start: usize) -> Option<(&[u8], usize)> {
    let bytes = data.get(start..)?;
    let relative_end = bytes.iter().position(|byte| *byte == b'\n');
    let end = relative_end.map_or(data.len(), |end| start + end);
    let next = relative_end.map_or(end, |_| end + 1);
    Some((
        data[start..end]
            .strip_suffix(b"\r")
            .unwrap_or(&data[start..end]),
        next,
    ))
}

pub(crate) fn parse_declaration(line: &[u8], offset: usize) -> Option<AttributeDeclaration> {
    let line = std::str::from_utf8(line).ok()?;
    let mut fields = line.split_ascii_whitespace();
    let name = fields.next()?.strip_prefix('@')?;
    if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_graphic()) {
        return None;
    }
    let id = fields.next()?.parse().ok()?;
    let type_code = fields.next()?.parse().ok()?;
    fields.next().is_none().then(|| AttributeDeclaration {
        id,
        name: name.to_string(),
        type_code,
        offset,
    })
}

pub(crate) fn starts_with_declaration(data: &[u8], start: usize) -> bool {
    line(data, start)
        .and_then(|(line, _)| parse_declaration(line, start))
        .is_some()
}

fn decimal(bytes: &[u8], mut offset: usize) -> Option<(u32, usize)> {
    let start = offset;
    let mut value = 0u32;
    while let Some(digit) = bytes.get(offset).filter(|byte| byte.is_ascii_digit()) {
        value = value
            .checked_mul(10)?
            .checked_add(u32::from(*digit - b'0'))?;
        offset += 1;
    }
    (offset > start).then_some((value, offset))
}

fn compact_real(bytes: &[u8]) -> Option<Real> {
    let (digits, repeat_last) = bytes
        .strip_suffix(b"R")
        .map_or((bytes, false), |digits| (digits, true));
    if digits.is_empty()
        || digits.len() > 16
        || !digits
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_lowercase())
    {
        return None;
    }
    let digits = std::str::from_utf8(digits).ok()?;
    let mut bits = u64::from_str_radix(digits, 16).ok()?;
    let fill = if repeat_last { bits & 0x0f } else { 0 };
    for _ in digits.len()..16 {
        bits = bits.checked_shl(4)? | fill;
    }
    f64::from_bits(bits).is_finite().then_some(Real(bits))
}

fn signed_integer(bytes: &[u8]) -> Option<i32> {
    let text = std::str::from_utf8(bytes).ok()?;
    if text.is_empty()
        || !text
            .strip_prefix('-')
            .unwrap_or(text)
            .bytes()
            .all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    text.parse().ok()
}

fn array_dimensions(bytes: &[u8]) -> Option<Vec<u32>> {
    let mut dimensions = Vec::new();
    let mut cursor = 0;
    while bytes.get(cursor) == Some(&b'[') {
        let (dimension, after_dimension) = decimal(bytes, cursor + 1)?;
        if dimension == 0 || bytes.get(after_dimension) != Some(&b']') {
            return None;
        }
        dimensions.push(dimension);
        cursor = after_dimension + 1;
    }
    (!dimensions.is_empty() && cursor == bytes.len()).then_some(dimensions)
}

fn numeric_run<T>(bytes: &[u8], scalar: fn(&[u8]) -> Option<T>) -> Option<NumericRun<T>> {
    if let Some(star) = bytes.iter().position(|byte| *byte == b'*') {
        let (count, after_count) = decimal(bytes, 0)?;
        if count == 0 || after_count != star {
            return None;
        }
        Some(NumericRun {
            count,
            value: scalar(bytes.get(star + 1..)?)?,
        })
    } else {
        Some(NumericRun {
            count: 1,
            value: scalar(bytes)?,
        })
    }
}

fn continuation_numeric_runs<T>(
    bytes: &[u8],
    scalar: fn(&[u8]) -> Option<T>,
) -> Option<Vec<NumericRun<T>>> {
    let mut runs = Vec::new();
    for row in bytes.split(|byte| *byte == b'\n') {
        let row = row.strip_suffix(b"\r").unwrap_or(row);
        let row = row.strip_prefix(b"$")?;
        let mut tokens = row.split(|byte| *byte == b',').peekable();
        while let Some(token) = tokens.next() {
            if token.is_empty() {
                if tokens.peek().is_some() {
                    return None;
                }
                continue;
            }
            runs.push(numeric_run(token, scalar)?);
        }
    }
    Some(runs)
}

fn object_node_id(offset: usize) -> String {
    format!("creo:legacy_ascii:object#{offset}")
}

fn parent_object_offsets(scopes: &[Scope]) -> BTreeMap<usize, usize> {
    let mut parents = BTreeMap::new();
    for scope in scopes {
        let declarations = scope
            .declarations
            .iter()
            .map(|declaration| (declaration.id, declaration))
            .collect::<BTreeMap<_, _>>();
        let mut active_objects = BTreeMap::<u32, usize>::new();
        for value in &scope.values {
            drop(active_objects.split_off(&value.depth));
            if let Some(parent) = value
                .depth
                .checked_sub(1)
                .and_then(|depth| active_objects.get(&depth))
            {
                parents.insert(value.offset, *parent);
            }
            if declarations
                .get(&value.attribute_id)
                .is_some_and(|declaration| declaration.type_code == 0)
            {
                active_objects.insert(value.depth, value.offset);
            }
        }
    }
    parents
}

fn object_records(
    data: &[u8],
    scopes: &[Scope],
    parents: &BTreeMap<usize, usize>,
) -> (Vec<ObjectRecord>, usize, usize) {
    let mut records = Vec::new();
    let mut incomplete_arrays = 0usize;
    let mut unresolved = 0usize;
    for scope in scopes {
        let declarations = scope
            .declarations
            .iter()
            .map(|declaration| (declaration.id, declaration))
            .collect::<BTreeMap<_, _>>();
        let value_attributes = scope
            .values
            .iter()
            .map(|value| (value.offset, value.attribute_id))
            .collect::<BTreeMap<_, _>>();
        let mut direct_array_elements = BTreeMap::<usize, Vec<usize>>::new();
        for child in &scope.values {
            let Some(parent_offset) = parents.get(&child.offset).copied() else {
                continue;
            };
            if value_attributes.get(&parent_offset) == Some(&child.attribute_id)
                && declarations
                    .get(&child.attribute_id)
                    .is_some_and(|declaration| declaration.type_code == 0)
            {
                direct_array_elements
                    .entry(parent_offset)
                    .or_default()
                    .push(child.offset);
            }
        }
        for value in &scope.values {
            let Some(declaration) = declarations
                .get(&value.attribute_id)
                .filter(|declaration| declaration.type_code == 0)
            else {
                continue;
            };
            let bytes = &data[value.payload.clone()];
            let payload = if bytes == b"->" {
                ObjectPayload::Arrow
            } else if bytes.is_empty() {
                ObjectPayload::Inline
            } else if bytes == b"NULL" {
                ObjectPayload::Null
            } else if let Some(dimensions) = array_dimensions(bytes) {
                let elements = direct_array_elements
                    .get(&value.offset)
                    .into_iter()
                    .flatten()
                    .map(|offset| object_node_id(*offset))
                    .collect::<Vec<_>>();
                let expected = dimensions.iter().try_fold(1u64, |count, dimension| {
                    count.checked_mul(u64::from(*dimension))
                });
                let complete = expected
                    .and_then(|count| usize::try_from(count).ok())
                    .is_some_and(|count| count == elements.len());
                incomplete_arrays += usize::from(!complete);
                ObjectPayload::Array {
                    dimensions,
                    elements,
                    complete,
                }
            } else {
                unresolved += 1;
                ObjectPayload::Opaque {
                    bytes: bytes.to_vec(),
                }
            };
            records.push(ObjectRecord {
                id: object_node_id(value.offset),
                name: declaration.name.clone(),
                attribute_id: value.attribute_id,
                scope_offset: scope.range.start,
                parent: parents
                    .get(&value.offset)
                    .map(|offset| object_node_id(*offset)),
                depth: value.depth,
                payload,
                offset: value.offset,
            });
        }
    }
    (records, incomplete_arrays, unresolved)
}

fn numeric_records<T>(
    data: &[u8],
    scopes: &[Scope],
    type_code: u8,
    identity_kind: &str,
    scalar: fn(&[u8]) -> Option<T>,
    parents: &BTreeMap<usize, usize>,
) -> (Vec<NumericRecord<T>>, usize) {
    let mut records = Vec::new();
    let mut unresolved = 0usize;
    for scope in scopes {
        let declarations = scope
            .declarations
            .iter()
            .map(|declaration| (declaration.id, declaration))
            .collect::<BTreeMap<_, _>>();
        let mut index = 0;
        while let Some(value) = scope.values.get(index) {
            let Some(declaration) = declarations
                .get(&value.attribute_id)
                .filter(|declaration| declaration.type_code == type_code)
            else {
                index += 1;
                continue;
            };
            let Some(payload_bytes) = data.get(value.payload.clone()) else {
                unresolved += 1;
                index += 1;
                continue;
            };
            let (payload, next_index) = if let Some(dimensions) = array_dimensions(payload_bytes) {
                let mut next_index = index + 1;
                let runs = if let Some(range) = &value.continuation_rows {
                    let Some(bytes) = data.get(range.clone()) else {
                        unresolved += 1;
                        index += 1;
                        continue;
                    };
                    let Some(runs) = continuation_numeric_runs(bytes, scalar) else {
                        unresolved += 1;
                        index += 1;
                        continue;
                    };
                    runs
                } else if dimensions.as_slice() == [1] {
                    let mut runs = Vec::new();
                    while let Some(child) = scope.values.get(next_index).filter(|child| {
                        child.attribute_id == value.attribute_id
                            && child.depth == value.depth.saturating_add(1)
                    }) {
                        let Some(bytes) = data.get(child.payload.clone()) else {
                            break;
                        };
                        let Some(run) = (child.continuation_count == 0)
                            .then(|| numeric_run(bytes, scalar))
                            .flatten()
                        else {
                            break;
                        };
                        runs.push(run);
                        next_index += 1;
                    }
                    runs
                } else {
                    Vec::new()
                };
                let expected = dimensions.iter().try_fold(1u64, |count, dimension| {
                    count.checked_mul(u64::from(*dimension))
                });
                let actual = runs
                    .iter()
                    .try_fold(0u64, |count, run| count.checked_add(u64::from(run.count)));
                if expected.is_none() || expected != actual {
                    unresolved += next_index - index;
                    index = next_index;
                    continue;
                }
                (NumericPayload::Array { dimensions, runs }, next_index)
            } else {
                let Some(scalar_value) = (value.continuation_count == 0)
                    .then(|| scalar(payload_bytes))
                    .flatten()
                else {
                    unresolved += 1;
                    index += 1;
                    continue;
                };
                (
                    NumericPayload::Scalar {
                        value: scalar_value,
                    },
                    index + 1,
                )
            };
            records.push(NumericRecord {
                id: format!("creo:legacy_ascii:{identity_kind}#{}", value.offset),
                name: declaration.name.clone(),
                attribute_id: value.attribute_id,
                scope_offset: scope.range.start,
                parent: parents
                    .get(&value.offset)
                    .map(|offset| object_node_id(*offset)),
                depth: value.depth,
                payload,
                offset: value.offset,
            });
            index = next_index;
        }
    }
    (records, unresolved)
}

fn value(line: &[u8], line_offset: usize) -> Option<AttributeValue> {
    let (depth, after_depth) = decimal(line, 0)?;
    if line.get(after_depth) != Some(&b' ') {
        return None;
    }
    let (attribute_id, after_attribute) = decimal(line, after_depth + 1)?;
    let payload_start = if after_attribute == line.len() {
        after_attribute
    } else {
        if line.get(after_attribute) != Some(&b' ') {
            return None;
        }
        after_attribute + 1
    };
    Some(AttributeValue {
        depth,
        attribute_id,
        offset: line_offset,
        payload: line_offset + payload_start..line_offset + line.len(),
        continuation_rows: None,
        continuation_count: 0,
    })
}

fn scan_scope(data: &[u8], range: Range<usize>) -> Scope {
    let end = range.end.min(data.len());
    let range = range.start.min(end)..end;
    let mut declarations = Vec::<AttributeDeclaration>::new();
    let mut declaration_indices = BTreeMap::<u32, usize>::new();
    let mut conflicting_ids = BTreeSet::<u32>::new();
    let mut candidates = Vec::<AttributeValue>::new();
    let mut continuation_owner = None::<usize>;
    let mut next = range.start;

    while next < range.end {
        let line_offset = next;
        let Some((current, after_line)) = line(&data[..range.end], next) else {
            break;
        };
        next = after_line;
        if current == b"#END_OF_UGC" {
            break;
        }
        if current.starts_with(b"$") {
            if let Some(owner) = continuation_owner {
                let value = &mut candidates[owner];
                value
                    .continuation_rows
                    .get_or_insert(line_offset..line_offset + current.len())
                    .end = line_offset + current.len();
                value.continuation_count += 1;
            }
            continue;
        }
        continuation_owner = None;

        if let Some(declaration) = parse_declaration(current, line_offset) {
            if let Some(index) = declaration_indices.get(&declaration.id).copied() {
                let previous = &declarations[index];
                if previous.name != declaration.name || previous.type_code != declaration.type_code
                {
                    conflicting_ids.insert(declaration.id);
                }
            } else {
                declaration_indices.insert(declaration.id, declarations.len());
                declarations.push(declaration);
            }
            continue;
        }
        if let Some(value) = value(current, line_offset) {
            continuation_owner = Some(candidates.len());
            candidates.push(value);
        }
    }

    let candidate_count = candidates.len();
    candidates.retain(|value| {
        declaration_indices.contains_key(&value.attribute_id)
            && !conflicting_ids.contains(&value.attribute_id)
    });
    Scope {
        range,
        declarations,
        unresolved_value_count: candidate_count - candidates.len(),
        values: candidates,
        conflicting_declaration_count: conflicting_ids.len(),
    }
}

/// Scan independently scoped legacy ASCII record extents.
pub(crate) fn scan(data: &[u8], ranges: impl IntoIterator<Item = Range<usize>>) -> Persistence {
    let scopes = ranges
        .into_iter()
        .filter(|range| range.start < range.end && range.start < data.len())
        .map(|range| scan_scope(data, range))
        .collect::<Vec<_>>();
    let parents = parent_object_offsets(&scopes);
    let (objects, incomplete_object_array_count, unresolved_object_value_count) =
        object_records(data, &scopes, &parents);
    let (real_values, unresolved_real_value_count) =
        numeric_records(data, &scopes, 2, "real", compact_real, &parents);
    let (integer_values, unresolved_integer_value_count) =
        numeric_records(data, &scopes, 1, "integer", signed_integer, &parents);
    Persistence {
        scopes,
        real_values,
        unresolved_real_value_count,
        integer_values,
        unresolved_integer_value_count,
        objects,
        incomplete_object_array_count,
        unresolved_object_value_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_resolves_declarations_values_and_continuations() {
        let data = b"#P_OBJECT 6\n@root 1 0\n0 1 ->\n@matrix 2 2\n1 2 [2][2]\n\
                     $3FF,0\n$0,3FF\n#END_OF_UGC\n";

        let persistence = scan(data, std::iter::once(0..data.len()));
        let scope = &persistence.scopes[0];

        assert_eq!(scope.declarations.len(), 2);
        assert_eq!(scope.declarations[1].name, "matrix");
        assert_eq!(scope.declarations[1].type_code, 2);
        assert_eq!(scope.values.len(), 2);
        assert_eq!(&data[scope.values[0].payload.clone()], b"->");
        assert_eq!(&data[scope.values[1].payload.clone()], b"[2][2]");
        assert_eq!(scope.values[1].continuation_count, 2);
        let continuation = scope.values[1]
            .continuation_rows
            .clone()
            .expect("continuations");
        assert_eq!(&data[continuation], b"$3FF,0\n$0,3FF");
        assert_eq!(persistence.unresolved_value_count(), 0);
        assert_eq!(persistence.conflicting_declaration_count(), 0);
    }

    #[test]
    fn scan_resolves_identifiers_within_independent_scopes() {
        let data = b"@field 7 1\n1 7 4\n@other 7 2\n2 7 5\n";
        let second = data
            .windows(b"@other".len())
            .position(|window| window == b"@other")
            .expect("second scope");

        let persistence = scan(data, [0..second, second..data.len()]);

        assert_eq!(persistence.scopes.len(), 2);
        assert_eq!(persistence.declaration_count(), 2);
        assert_eq!(persistence.value_count(), 2);
        assert_eq!(persistence.conflicting_declaration_count(), 0);
        assert_eq!(persistence.real_values.len(), 1);
        assert_eq!(persistence.real_values[0].scope_offset, second);
    }

    #[test]
    fn principal_unit_requires_one_complete_known_type_10_scalar() {
        let millimeter = b"@principal_sys_units 25 10\n2 25 millimeter Newton Second (mmNs)\n";
        let persistence = scan(millimeter, std::iter::once(0..millimeter.len()));
        assert_eq!(
            persistence.principal_unit_system(millimeter),
            Some(PrincipalUnitSystem::MillimeterNewtonSecond)
        );
        assert_eq!(
            persistence
                .principal_unit_system(millimeter)
                .and_then(PrincipalUnitSystem::length_scale_mm),
            Some(1.0)
        );

        let inch = b"@principal_sys_units 25 10\n2 25 Inch lbm Second (Pro/E Default)\n";
        let persistence = scan(inch, std::iter::once(0..inch.len()));
        assert_eq!(
            persistence.principal_unit_system(inch),
            Some(PrincipalUnitSystem::InchPoundMassSecond)
        );
        assert_eq!(
            persistence
                .principal_unit_system(inch)
                .and_then(PrincipalUnitSystem::length_scale_mm),
            Some(25.4)
        );

        let mut repeated = millimeter.to_vec();
        repeated.extend_from_slice(millimeter);
        let persistence = scan(&repeated, std::iter::once(0..repeated.len()));
        assert_eq!(persistence.principal_unit_system(&repeated), None);
    }

    #[test]
    fn type_2_reals_decode_compact_bits_runs_and_child_rows() {
        let data = b"@scalar 1 2\n0 1 3FF\n\
            @scale 2 2\n0 2 40396R\n\
            @matrix 3 2\n0 3 [2][2]\n$3FF,2*0,\n$3FF\n\
            @single 4 2\n0 4 [1]\n1 4 400\n";
        let persistence = scan(data, std::iter::once(0..data.len()));

        assert_eq!(persistence.real_values.len(), 4);
        assert_eq!(persistence.unresolved_real_value_count, 0);
        assert_eq!(
            persistence.real_values[0].payload,
            RealPayload::Scalar {
                value: Real(1.0f64.to_bits())
            }
        );
        assert_eq!(
            persistence.real_values[1].payload,
            RealPayload::Scalar {
                value: Real(25.4f64.to_bits())
            }
        );
        assert_eq!(
            persistence.real_values[2].payload,
            RealPayload::Array {
                dimensions: vec![2, 2],
                runs: vec![
                    RealRun {
                        count: 1,
                        value: Real(1.0f64.to_bits()),
                    },
                    RealRun {
                        count: 2,
                        value: Real(0.0f64.to_bits()),
                    },
                    RealRun {
                        count: 1,
                        value: Real(1.0f64.to_bits()),
                    },
                ],
            }
        );
        assert_eq!(persistence.real_values[2].payload.element_count(), 4);
        assert_eq!(
            persistence.real_values[3].payload,
            RealPayload::Array {
                dimensions: vec![1],
                runs: vec![RealRun {
                    count: 1,
                    value: Real(2.0f64.to_bits()),
                }],
            }
        );
    }

    #[test]
    fn type_2_reals_withhold_incomplete_or_nonfinite_values() {
        let data = b"@short 1 2\n0 1 [3]\n$2*0\n\
            @lower 2 2\n0 2 3ff\n\
            @infinite 3 2\n0 3 7FF\n";
        let persistence = scan(data, std::iter::once(0..data.len()));

        assert!(persistence.real_values.is_empty());
        assert_eq!(persistence.unresolved_real_value_count, 3);
    }

    #[test]
    fn type_1_integers_decode_signed_scalars_runs_and_child_rows() {
        let data = b"@minimum 1 1\n0 1 -2147483648\n\
            @array 2 1\n0 2 [4]\n$1,2*-1,0\n\
            @single 3 1\n0 3 [1]\n1 3 42\n";
        let persistence = scan(data, std::iter::once(0..data.len()));

        assert_eq!(persistence.integer_values.len(), 3);
        assert_eq!(persistence.unresolved_integer_value_count, 0);
        assert_eq!(
            persistence.integer_values[0].payload,
            IntegerPayload::Scalar { value: i32::MIN }
        );
        assert_eq!(
            persistence.integer_values[1].payload,
            IntegerPayload::Array {
                dimensions: vec![4],
                runs: vec![
                    IntegerRun { count: 1, value: 1 },
                    IntegerRun {
                        count: 2,
                        value: -1,
                    },
                    IntegerRun { count: 1, value: 0 },
                ],
            }
        );
        assert_eq!(
            persistence.integer_values[2].payload,
            IntegerPayload::Array {
                dimensions: vec![1],
                runs: vec![IntegerRun {
                    count: 1,
                    value: 42,
                }],
            }
        );
    }

    #[test]
    fn type_1_integers_withhold_incomplete_arrays_and_overflow() {
        let data = b"@short 1 1\n0 1 [2]\n$0\n\
            @overflow 2 1\n0 2 2147483648\n";
        let persistence = scan(data, std::iter::once(0..data.len()));

        assert!(persistence.integer_values.is_empty());
        assert_eq!(persistence.unresolved_integer_value_count, 2);
    }

    #[test]
    fn type_0_objects_define_scoped_ownership_and_array_elements() {
        let data = b"@root 1 0\n@number 2 1\n@children 3 0\n@weight 4 2\n\
            0 1 ->\n1 2 7\n1 3 [2]\n2 3 ->\n3 4 3FF\n2 3 NULL\n";
        let root_offset = data
            .windows(b"0 1 ->".len())
            .position(|window| window == b"0 1 ->")
            .expect("root offset");
        let array_offset = data
            .windows(b"1 3 [2]".len())
            .position(|window| window == b"1 3 [2]")
            .expect("array offset");
        let first_child_offset = data
            .windows(b"2 3 ->".len())
            .position(|window| window == b"2 3 ->")
            .expect("first child offset");
        let second_child_offset = data
            .windows(b"2 3 NULL".len())
            .position(|window| window == b"2 3 NULL")
            .expect("second child offset");
        let persistence = scan(data, std::iter::once(0..data.len()));

        assert_eq!(persistence.objects.len(), 4);
        assert_eq!(persistence.incomplete_object_array_count, 0);
        assert_eq!(persistence.unresolved_object_value_count, 0);
        assert_eq!(
            persistence.objects[1].parent.as_deref(),
            Some(object_node_id(root_offset).as_str())
        );
        assert_eq!(
            persistence.objects[1].payload,
            ObjectPayload::Array {
                dimensions: vec![2],
                elements: vec![
                    object_node_id(first_child_offset),
                    object_node_id(second_child_offset),
                ],
                complete: true,
            }
        );
        assert_eq!(persistence.integer_values.len(), 1);
        assert_eq!(
            persistence.integer_values[0].parent.as_deref(),
            Some(object_node_id(root_offset).as_str())
        );
        assert_eq!(persistence.real_values.len(), 1);
        assert_eq!(
            persistence.real_values[0].parent.as_deref(),
            Some(object_node_id(first_child_offset).as_str())
        );
        assert_eq!(persistence.objects[1].offset, array_offset);
    }

    #[test]
    fn type_0_objects_retain_incomplete_and_opaque_forms() {
        let data = b"@array 1 0\n0 1 [2]\n@future 2 0\n0 2 token\n";
        let persistence = scan(data, std::iter::once(0..data.len()));

        assert_eq!(persistence.objects.len(), 2);
        assert_eq!(persistence.incomplete_object_array_count, 1);
        assert_eq!(persistence.unresolved_object_value_count, 1);
        assert!(matches!(
            persistence.objects[0].payload,
            ObjectPayload::Array {
                complete: false,
                ..
            }
        ));
        assert_eq!(
            persistence.objects[1].payload,
            ObjectPayload::Opaque {
                bytes: b"token".to_vec()
            }
        );
    }

    #[test]
    fn scan_withholds_ambiguous_and_undeclared_values() {
        let data = b"#P_OBJECT 6\n$orphan\n@field 7 1\n@other 7 2\n1 7 4\n2 99 5\n";

        let persistence = scan(data, std::iter::once(0..data.len()));
        let scope = &persistence.scopes[0];

        assert!(scope.values.is_empty());
        assert_eq!(scope.declarations.len(), 1);
        assert_eq!(persistence.conflicting_declaration_count(), 1);
        assert_eq!(persistence.unresolved_value_count(), 2);
    }
}
