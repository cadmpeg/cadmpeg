// SPDX-License-Identifier: Apache-2.0
//! Structural grammar for legacy ASCII persistence records.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use serde::{Serialize, Serializer};

const PRINCIPAL_UNIT_NAME: &str = "principal_sys_units";
const MILLIMETER_NEWTON_SECOND: &str = "millimeter Newton Second (mmNs)";
const INCH_POUND_MASS_SECOND: &str = "Inch lbm Second (Pro/E Default)";
const LEGACY_INCH_TO_MM: f64 = 25.4;
const LEGACY_LENGTH_UNIT_TYPE: i32 = 0;

/// Active coordinate-unit system selected by a model-level persistence field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrincipalUnitSystem {
    /// Millimeter, Newton, second.
    MillimeterNewtonSecond,
    /// Millimeter, kilogram, second.
    MillimeterKilogramSecond,
    /// Inch, pound mass, second.
    InchPoundMassSecond,
    /// A complete legacy `unit_arr` length record with a source-specific scale.
    LegacyLengthScale(u64),
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
            Self::LegacyLengthScale(bits) => {
                format!("legacy_length_scale_mm:{:.17}", f64::from_bits(bits))
            }
            Self::UnknownBinarySelector(value) => format!("unknown:{value}"),
        }
    }

    /// Scale from stored coordinate lengths to canonical millimeters.
    pub const fn length_scale_mm(self) -> Option<f64> {
        match self {
            Self::MillimeterNewtonSecond | Self::MillimeterKilogramSecond => Some(1.0),
            Self::InchPoundMassSecond => Some(LEGACY_INCH_TO_MM),
            Self::LegacyLengthScale(bits) => Some(f64::from_bits(bits)),
            Self::UnknownBinarySelector(_) => None,
        }
    }
}

/// One finite legacy type-2 real, stored by its exact IEEE-754 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Real(u64);

impl Real {
    /// Construct a real from its exact stored IEEE-754 bits.
    #[cfg(test)]
    pub(crate) const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

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

/// One typed legacy attribute value in the scoped object tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValueRecord<T> {
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
    /// Typed value payload.
    pub payload: T,
    /// Byte offset of the scalar row or array header.
    pub offset: usize,
}

/// One completely decoded numeric legacy attribute value.
pub type NumericRecord<T> = ValueRecord<NumericPayload<T>>;

/// One run in a type-2 real array.
#[cfg(test)]
pub type RealRun = NumericRun<Real>;
/// Complete semantic payload of one legacy type-2 value row.
#[cfg(test)]
pub type RealPayload = NumericPayload<Real>;
/// One completely decoded legacy type-2 attribute value.
pub type RealRecord = NumericRecord<Real>;
/// One run in a type-1 integer array.
#[cfg(test)]
pub type IntegerRun = NumericRun<i32>;
/// Complete semantic payload of one legacy type-1 value row.
#[cfg(test)]
pub type IntegerPayload = NumericPayload<i32>;
/// One completely decoded legacy type-1 attribute value.
pub type IntegerRecord = NumericRecord<i32>;
/// Complete semantic payload of one unsigned-decimal legacy value.
#[cfg(test)]
pub type UnsignedPayload = NumericPayload<u32>;
/// One completely decoded unsigned-decimal legacy attribute value.
pub type UnsignedRecord = NumericRecord<u32>;

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

/// One legacy byte string or null element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub enum StringValue {
    /// The `NULL` token.
    Null,
    /// A byte string that is valid UTF-8.
    Utf8 {
        /// Decoded text. An empty string is a stored empty value.
        text: String,
    },
    /// A byte string whose character encoding is not UTF-8.
    Bytes {
        /// Exact uninterpreted source bytes.
        bytes: Vec<u8>,
    },
}

impl StringValue {
    /// Whether the exact bytes could not be decoded as UTF-8.
    pub fn undecoded_encoding_count(&self) -> usize {
        usize::from(matches!(self, Self::Bytes { .. }))
    }
}

/// Semantic payload of one legacy byte-string value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub enum StringPayload {
    /// One string or null value.
    Scalar {
        /// Stored value.
        value: StringValue,
    },
    /// A dimensioned string array and its direct elements.
    Array {
        /// Declared array dimensions.
        dimensions: Vec<u32>,
        /// Direct string elements in source order.
        values: Vec<StringValue>,
        /// Whether the element count equals the first extent.
        complete: bool,
    },
}

impl StringPayload {
    /// Number of logical string elements represented by this payload.
    pub fn element_count(&self) -> usize {
        match self {
            Self::Scalar { .. } => 1,
            Self::Array { values, .. } => values.len(),
        }
    }

    /// Number of elements whose character encoding remains uninterpreted.
    pub fn undecoded_encoding_count(&self) -> usize {
        match self {
            Self::Scalar { value } => value.undecoded_encoding_count(),
            Self::Array { values, .. } => values
                .iter()
                .map(StringValue::undecoded_encoding_count)
                .sum(),
        }
    }
}

/// One decoded legacy byte-string value.
pub type StringRecord = ValueRecord<StringPayload>;
/// One decoded legacy scalar byte-string value.
pub type ScalarStringRecord = ValueRecord<StringValue>;

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
    /// Type-10 byte-string scalars and arrays in source order.
    pub string_values: Vec<StringRecord>,
    /// Type-10 arrays whose direct element count differs from the first extent.
    pub incomplete_string_array_count: usize,
    /// Type-10 rows that use an undefined continuation form.
    pub unresolved_string_value_count: usize,
    /// Type-3 nullable byte-string scalars in source order.
    pub type_3_values: Vec<ScalarStringRecord>,
    /// Type-3 rows that use an undefined continuation form.
    pub unresolved_type_3_value_count: usize,
    /// Type-4 byte-string scalars in source order.
    pub type_4_values: Vec<ScalarStringRecord>,
    /// Type-4 rows that use an undefined continuation form.
    pub unresolved_type_4_value_count: usize,
    /// Type-5 unsigned-decimal scalars and arrays in source order.
    pub type_5_values: Vec<UnsignedRecord>,
    /// Type-5 rows not represented by a complete scalar or array.
    pub unresolved_type_5_value_count: usize,
    /// Type-6 compact-real scalars and arrays in source order.
    pub type_6_values: Vec<RealRecord>,
    /// Type-6 rows not represented by a complete finite scalar or array.
    pub unresolved_type_6_value_count: usize,
    /// Type-7 unsigned-decimal scalars and arrays in source order.
    pub type_7_values: Vec<UnsignedRecord>,
    /// Type-7 rows not represented by a complete scalar or array.
    pub unresolved_type_7_value_count: usize,
    /// Type-9 unsigned-decimal scalars and arrays in source order.
    pub type_9_values: Vec<UnsignedRecord>,
    /// Type-9 rows not represented by a complete scalar or array.
    pub unresolved_type_9_value_count: usize,
    /// Type-11 unsigned-decimal scalars and arrays in source order.
    pub type_11_values: Vec<UnsignedRecord>,
    /// Type-11 rows not represented by a complete scalar or array.
    pub unresolved_type_11_value_count: usize,
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
    pub fn principal_unit_system(&self) -> Option<PrincipalUnitSystem> {
        let mut candidate = None;
        let mut found = false;
        for record in self
            .string_values
            .iter()
            .filter(|record| record.name == PRINCIPAL_UNIT_NAME)
        {
            found = true;
            if candidate.is_some() {
                return None;
            }
            candidate = match &record.payload {
                StringPayload::Scalar {
                    value: StringValue::Utf8 { text },
                } if text == MILLIMETER_NEWTON_SECOND => {
                    Some(PrincipalUnitSystem::MillimeterNewtonSecond)
                }
                StringPayload::Scalar {
                    value: StringValue::Utf8 { text },
                } if text == INCH_POUND_MASS_SECOND => {
                    Some(PrincipalUnitSystem::InchPoundMassSecond)
                }
                _ => return None,
            };
        }
        if found {
            candidate
        } else {
            self.legacy_unit_array_system()
        }
    }

    fn legacy_unit_array_system(&self) -> Option<PrincipalUnitSystem> {
        let mut arrays = self.objects.iter().filter(|object| {
            object.name == "unit_arr"
                && matches!(object.payload, ObjectPayload::Array { complete: true, .. })
        });
        let array = arrays.next()?;
        arrays.next().is_none().then_some(())?;
        let ObjectPayload::Array { elements, .. } = &array.payload else {
            unreachable!("the unit array was filtered above");
        };
        if elements.is_empty() {
            return None;
        }
        let mut element_ids = BTreeSet::new();
        if !elements
            .iter()
            .all(|element_id| element_ids.insert(element_id))
        {
            return None;
        }
        let element_records = elements
            .iter()
            .map(|element_id| {
                let mut matches = self.objects.iter().filter(|object| {
                    object.id == *element_id
                        && object.parent.as_deref() == Some(array.id.as_str())
                        && object.name == "unit_arr"
                });
                let element = matches.next()?;
                matches.next().is_none().then_some(element)
            })
            .collect::<Option<Vec<_>>>()?;
        let first = element_records.first()?;
        let unit_type = self.unique_integer_scalar(&first.id, "unit_type")?;
        if unit_type != LEGACY_LENGTH_UNIT_TYPE
            || self.unique_utf8_scalar(&first.id, "name")?.is_empty()
        {
            return None;
        }
        let factor = self.unique_real_scalar(&first.id, "factor")?;
        let scale_mm = factor * LEGACY_INCH_TO_MM;
        (scale_mm.is_finite() && scale_mm > 0.0)
            .then_some(PrincipalUnitSystem::LegacyLengthScale(scale_mm.to_bits()))
    }

    fn unique_integer_scalar(&self, parent: &str, name: &str) -> Option<i32> {
        let mut matches = self
            .integer_values
            .iter()
            .filter(|record| record.parent.as_deref() == Some(parent) && record.name == name);
        let record = matches.next()?;
        matches.next().is_none().then_some(())?;
        match &record.payload {
            NumericPayload::Scalar { value } => Some(*value),
            NumericPayload::Array { .. } => None,
        }
    }

    fn unique_real_scalar(&self, parent: &str, name: &str) -> Option<f64> {
        let mut matches = self
            .real_values
            .iter()
            .filter(|record| record.parent.as_deref() == Some(parent) && record.name == name);
        let record = matches.next()?;
        matches.next().is_none().then_some(())?;
        match &record.payload {
            NumericPayload::Scalar { value } => Some(value.value()),
            NumericPayload::Array { .. } => None,
        }
    }

    fn unique_utf8_scalar<'a>(&'a self, parent: &str, name: &str) -> Option<&'a str> {
        let mut matches = self
            .string_values
            .iter()
            .filter(|record| record.parent.as_deref() == Some(parent) && record.name == name);
        let record = matches.next()?;
        matches.next().is_none().then_some(())?;
        match &record.payload {
            StringPayload::Scalar {
                value: StringValue::Utf8 { text },
            } => Some(text),
            _ => None,
        }
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

fn unsigned_integer(bytes: &[u8]) -> Option<u32> {
    let text = std::str::from_utf8(bytes).ok()?;
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NullToken {
    RepresentsNull,
    RepresentsBytes,
}

fn byte_string_value(bytes: &[u8], null_token: NullToken) -> StringValue {
    if null_token == NullToken::RepresentsNull && bytes == b"NULL" {
        StringValue::Null
    } else if let Ok(text) = std::str::from_utf8(bytes) {
        StringValue::Utf8 {
            text: text.to_string(),
        }
    } else {
        StringValue::Bytes {
            bytes: bytes.to_vec(),
        }
    }
}

fn string_value(bytes: &[u8]) -> StringValue {
    byte_string_value(bytes, NullToken::RepresentsNull)
}

fn scalar_string_records(
    data: &[u8],
    scopes: &[Scope],
    type_code: u8,
    identity_kind: &str,
    null_token: NullToken,
    parents: &BTreeMap<usize, usize>,
) -> (Vec<ScalarStringRecord>, usize) {
    let mut records = Vec::new();
    let mut unresolved = 0usize;
    for scope in scopes {
        let declarations = scope
            .declarations
            .iter()
            .map(|declaration| (declaration.id, declaration))
            .collect::<BTreeMap<_, _>>();
        for value in &scope.values {
            let Some(declaration) = declarations
                .get(&value.attribute_id)
                .filter(|declaration| declaration.type_code == type_code)
            else {
                continue;
            };
            let Some(bytes) = (value.continuation_count == 0)
                .then(|| data.get(value.payload.clone()))
                .flatten()
            else {
                unresolved += 1;
                continue;
            };
            records.push(ValueRecord {
                id: format!("creo:legacy_ascii:{identity_kind}#{}", value.offset),
                name: declaration.name.clone(),
                attribute_id: value.attribute_id,
                scope_offset: scope.range.start,
                parent: parents
                    .get(&value.offset)
                    .map(|offset| object_node_id(*offset)),
                depth: value.depth,
                payload: byte_string_value(bytes, null_token),
                offset: value.offset,
            });
        }
    }
    (records, unresolved)
}

fn string_records(
    data: &[u8],
    scopes: &[Scope],
    parents: &BTreeMap<usize, usize>,
) -> (Vec<StringRecord>, usize, usize) {
    let mut records = Vec::new();
    let mut incomplete_arrays = 0usize;
    let mut unresolved = 0usize;
    for scope in scopes {
        let declarations = scope
            .declarations
            .iter()
            .map(|declaration| (declaration.id, declaration))
            .collect::<BTreeMap<_, _>>();
        let values_by_offset = scope
            .values
            .iter()
            .map(|value| (value.offset, value))
            .collect::<BTreeMap<_, _>>();
        let mut active_arrays = BTreeMap::<u32, (usize, u32)>::new();
        let mut array_children = BTreeMap::<usize, Vec<usize>>::new();
        let mut array_element_offsets = BTreeSet::new();
        for value in &scope.values {
            drop(active_arrays.split_off(&value.depth));
            let array_parent = value.depth.checked_sub(1).and_then(|depth| {
                active_arrays
                    .get(&depth)
                    .filter(|(_, attribute_id)| *attribute_id == value.attribute_id)
                    .map(|(offset, _)| *offset)
            });
            if let Some(parent_offset) = array_parent {
                array_children
                    .entry(parent_offset)
                    .or_default()
                    .push(value.offset);
                array_element_offsets.insert(value.offset);
                continue;
            }
            if declarations
                .get(&value.attribute_id)
                .is_some_and(|declaration| declaration.type_code == 10)
                && array_dimensions(&data[value.payload.clone()]).is_some()
            {
                active_arrays.insert(value.depth, (value.offset, value.attribute_id));
            }
        }

        for value in &scope.values {
            if array_element_offsets.contains(&value.offset) {
                continue;
            }
            let Some(declaration) = declarations
                .get(&value.attribute_id)
                .filter(|declaration| declaration.type_code == 10)
            else {
                continue;
            };
            let bytes = &data[value.payload.clone()];
            let payload = if let Some(dimensions) = array_dimensions(bytes) {
                let children = array_children
                    .get(&value.offset)
                    .map_or(&[][..], Vec::as_slice);
                let mut values = Vec::new();
                for child in children
                    .iter()
                    .filter_map(|offset| values_by_offset.get(offset).copied())
                {
                    if child.continuation_count == 0 {
                        values.push(string_value(&data[child.payload.clone()]));
                    } else {
                        unresolved += 1;
                    }
                }
                let expected = dimensions
                    .first()
                    .and_then(|dimension| usize::try_from(*dimension).ok());
                let complete = value.continuation_count == 0
                    && expected.is_some_and(|count| count == children.len())
                    && values.len() == children.len();
                unresolved += usize::from(value.continuation_count != 0);
                incomplete_arrays += usize::from(!complete);
                StringPayload::Array {
                    dimensions,
                    values,
                    complete,
                }
            } else {
                if value.continuation_count != 0 {
                    unresolved += 1;
                    continue;
                }
                StringPayload::Scalar {
                    value: string_value(bytes),
                }
            };
            records.push(ValueRecord {
                id: format!("creo:legacy_ascii:string#{}", value.offset),
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
            records.push(ValueRecord {
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
    let (string_values, incomplete_string_array_count, unresolved_string_value_count) =
        string_records(data, &scopes, &parents);
    let (type_3_values, unresolved_type_3_value_count) = scalar_string_records(
        data,
        &scopes,
        3,
        "type_3",
        NullToken::RepresentsNull,
        &parents,
    );
    let (type_4_values, unresolved_type_4_value_count) = scalar_string_records(
        data,
        &scopes,
        4,
        "type_4",
        NullToken::RepresentsBytes,
        &parents,
    );
    let (real_values, unresolved_real_value_count) =
        numeric_records(data, &scopes, 2, "real", compact_real, &parents);
    let (integer_values, unresolved_integer_value_count) =
        numeric_records(data, &scopes, 1, "integer", signed_integer, &parents);
    let (type_5_values, unresolved_type_5_value_count) =
        numeric_records(data, &scopes, 5, "type_5", unsigned_integer, &parents);
    let (type_6_values, unresolved_type_6_value_count) =
        numeric_records(data, &scopes, 6, "type_6", compact_real, &parents);
    let (type_7_values, unresolved_type_7_value_count) =
        numeric_records(data, &scopes, 7, "type_7", unsigned_integer, &parents);
    let (type_9_values, unresolved_type_9_value_count) =
        numeric_records(data, &scopes, 9, "type_9", unsigned_integer, &parents);
    let (type_11_values, unresolved_type_11_value_count) =
        numeric_records(data, &scopes, 11, "type_11", unsigned_integer, &parents);
    Persistence {
        scopes,
        real_values,
        unresolved_real_value_count,
        integer_values,
        unresolved_integer_value_count,
        objects,
        incomplete_object_array_count,
        unresolved_object_value_count,
        string_values,
        incomplete_string_array_count,
        unresolved_string_value_count,
        type_3_values,
        unresolved_type_3_value_count,
        type_4_values,
        unresolved_type_4_value_count,
        type_5_values,
        unresolved_type_5_value_count,
        type_6_values,
        unresolved_type_6_value_count,
        type_7_values,
        unresolved_type_7_value_count,
        type_9_values,
        unresolved_type_9_value_count,
        type_11_values,
        unresolved_type_11_value_count,
    }
}

#[cfg(test)]
mod tests;
