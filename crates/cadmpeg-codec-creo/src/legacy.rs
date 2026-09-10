// SPDX-License-Identifier: Apache-2.0
//! Structural grammar for legacy ASCII persistence records.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::ops::Range;

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

mod numeric_array;
pub(crate) mod type_code;
use type_code::LegacyTypeCode;

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
    Array(numeric_array::NumericArray<T>),
}

impl<T> NumericPayload<T> {
    /// Admits source runs whose count sum equals the extent product.
    pub fn array(dimensions: Vec<u32>, runs: Vec<NumericRun<T>>) -> Option<Self> {
        numeric_array::NumericArray::try_new(dimensions, runs).map(Self::Array)
    }

    /// Number of logical scalar elements represented by this payload.
    pub fn element_count(&self) -> u64 {
        match self {
            Self::Scalar { .. } => 1,
            Self::Array(array) => array.element_count(),
        }
    }
}

/// One typed legacy attribute value in the scoped object tree.
pub struct ValueRecord<K: LegacyCode> {
    /// Declared attribute name.
    pub name: String,
    /// Scope-local declaration identifier.
    pub attribute_id: u32,
    /// Byte offset of the owning attribute-ID scope.
    pub scope_offset: usize,
    /// Owning type-0 object node, when the depth tree supplies one.
    pub parent: Option<usize>,
    /// Object-tree nesting depth of the scalar or array header.
    pub depth: u32,
    /// Typed value payload.
    pub payload: K::Payload,
    /// Byte offset of the scalar row or array header.
    pub offset: usize,
}

impl<K: LegacyCode> std::fmt::Debug for ValueRecord<K>
where
    K::Payload: std::fmt::Debug,
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ValueRecord")
            .field("name", &self.name)
            .field("attribute_id", &self.attribute_id)
            .field("scope_offset", &self.scope_offset)
            .field("parent", &self.parent)
            .field("depth", &self.depth)
            .field("payload", &self.payload)
            .field("offset", &self.offset)
            .finish()
    }
}

impl<K: LegacyCode> Clone for ValueRecord<K>
where
    K::Payload: Clone,
{
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            attribute_id: self.attribute_id,
            scope_offset: self.scope_offset,
            parent: self.parent,
            depth: self.depth,
            payload: self.payload.clone(),
            offset: self.offset,
        }
    }
}

impl<K: LegacyCode> PartialEq for ValueRecord<K>
where
    K::Payload: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.attribute_id == other.attribute_id
            && self.scope_offset == other.scope_offset
            && self.parent == other.parent
            && self.depth == other.depth
            && self.payload == other.payload
            && self.offset == other.offset
    }
}

impl<K: LegacyCode> Eq for ValueRecord<K> where K::Payload: Eq {}

/// One run in a type-2 real array.
#[cfg(test)]
pub type RealRun = NumericRun<Real>;
/// Complete semantic payload of one legacy type-2 value row.
#[cfg(test)]
pub type RealPayload = NumericPayload<Real>;
/// One completely decoded legacy type-2 attribute value.
pub type RealRecord = ValueRecord<RealCode>;
/// One run in a type-1 integer array.
#[cfg(test)]
pub type IntegerRun = NumericRun<i32>;
/// Complete semantic payload of one legacy type-1 value row.
#[cfg(test)]
pub type IntegerPayload = NumericPayload<i32>;
/// One completely decoded legacy type-1 attribute value.
pub type IntegerRecord = ValueRecord<IntegerCode>;
/// Complete semantic payload of one unsigned-decimal legacy value.
#[cfg(test)]
pub type UnsignedPayload = NumericPayload<u32>;
/// One completely decoded legacy type-5 attribute value.
pub type Type5Record = ValueRecord<Type5Code>;
/// One completely decoded legacy type-6 attribute value.
pub type Type6Record = ValueRecord<Type6Code>;
/// One completely decoded legacy type-7 attribute value.
pub type Type7Record = ValueRecord<Type7Code>;
/// One completely decoded legacy type-9 attribute value.
pub type Type9Record = ValueRecord<Type9Code>;
/// One completely decoded legacy type-11 attribute value.
pub type Type11Record = ValueRecord<Type11Code>;

/// Structural payload of one legacy type-0 object node.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    },
    /// A type-0 payload outside the defined object forms.
    Opaque {
        /// Uninterpreted payload bytes after the attribute identifier.
        bytes: Vec<u8>,
    },
}

impl ObjectPayload {
    /// Whether an array has exactly its declared extent product of elements.
    pub fn is_complete(&self) -> bool {
        let Self::Array {
            dimensions,
            elements,
        } = self
        else {
            return false;
        };
        dimensions
            .iter()
            .try_fold(1u64, |count, dimension| {
                count.checked_mul(u64::from(*dimension))
            })
            .and_then(|count| usize::try_from(count).ok())
            .is_some_and(|count| count == elements.len())
    }
}

impl Serialize for ObjectPayload {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_struct(
            "ObjectPayload",
            match self {
                Self::Array { .. } => 4,
                Self::Opaque { .. } => 2,
                _ => 1,
            },
        )?;
        wire.serialize_field(
            "form",
            match self {
                Self::Arrow => "arrow",
                Self::Inline => "inline",
                Self::Null => "null",
                Self::Array { .. } => "array",
                Self::Opaque { .. } => "opaque",
            },
        )?;
        match self {
            Self::Array {
                dimensions,
                elements,
            } => {
                wire.serialize_field("dimensions", dimensions)?;
                wire.serialize_field("elements", elements)?;
                wire.serialize_field("complete", &self.is_complete())?;
            }
            Self::Opaque { bytes } => wire.serialize_field("bytes", bytes)?,
            _ => {}
        }
        wire.end()
    }
}

/// One legacy type-0 object node in the depth-defined ownership tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectRecord {
    /// Declared attribute name.
    pub name: String,
    /// Scope-local declaration identifier.
    pub attribute_id: u32,
    /// Byte offset of the owning attribute-ID scope.
    pub scope_offset: usize,
    /// Owning type-0 object node, when the depth tree supplies one.
    pub parent: Option<usize>,
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
        /// Direct source rows, retaining unsupported continuation evidence.
        values: Vec<Result<StringValue, Continuation>>,
        /// Continuation rows attached to the array header.
        continuation: Option<Continuation>,
    },
}

impl Serialize for StringPayload {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_struct(
            "StringPayload",
            match self {
                Self::Scalar { .. } => 2,
                Self::Array { .. } => 4,
            },
        )?;
        match self {
            Self::Scalar { value } => {
                wire.serialize_field("form", "scalar")?;
                wire.serialize_field("value", value)?;
            }
            Self::Array {
                dimensions, values, ..
            } => {
                wire.serialize_field("form", "array")?;
                wire.serialize_field("dimensions", dimensions)?;
                wire.serialize_field(
                    "values",
                    &values
                        .iter()
                        .filter_map(|value| value.as_ref().ok())
                        .collect::<Vec<_>>(),
                )?;
                wire.serialize_field("complete", &self.is_complete())?;
            }
        }
        wire.end()
    }
}

impl StringPayload {
    /// Whether every declared string row has a supported, complete value.
    pub fn is_complete(&self) -> bool {
        let Self::Array {
            dimensions,
            values,
            continuation,
        } = self
        else {
            return false;
        };
        continuation.is_none()
            && dimensions
                .first()
                .and_then(|dimension| usize::try_from(*dimension).ok())
                .is_some_and(|count| count == values.len())
            && values.iter().all(Result::is_ok)
    }

    /// Number of logical string elements represented by this payload.
    pub fn element_count(&self) -> usize {
        match self {
            Self::Scalar { .. } => 1,
            Self::Array { values, .. } => values.iter().filter(|value| value.is_ok()).count(),
        }
    }

    /// Number of elements whose character encoding remains uninterpreted.
    pub fn undecoded_encoding_count(&self) -> usize {
        match self {
            Self::Scalar { value } => value.undecoded_encoding_count(),
            Self::Array { values, .. } => values
                .iter()
                .filter_map(|value| value.as_ref().ok())
                .map(StringValue::undecoded_encoding_count)
                .sum(),
        }
    }
}

/// One decoded legacy byte-string value.
pub type StringRecord = ValueRecord<StringCode>;
/// One decoded legacy type-3 scalar byte-string value.
pub type Type3Record = ValueRecord<Type3Code>;
/// One decoded legacy type-4 scalar byte-string value.
pub type Type4Record = ValueRecord<Type4Code>;

/// One unique `@<name> <id> <type-code>` declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeDeclaration {
    /// Attribute identifier referenced by value rows in the same scope.
    pub id: u32,
    /// Attribute name without the leading `@`.
    pub name: String,
    /// Stored numeric type code.
    pub type_code: LegacyTypeCode,
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
    /// Immediately following `$` rows, when present.
    pub continuation: Option<Continuation>,
}

/// A nonempty sequence of continuation rows following one value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Continuation {
    /// Contiguous source range containing the rows.
    pub rows: Range<usize>,
    /// Number of rows in the source range.
    pub count: NonZeroUsize,
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

/// Parsed rows and unresolved-row count for one legacy declaration type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedValues<T> {
    /// Complete typed rows in source order.
    pub rows: Vec<T>,
    /// Source rows not represented by a complete typed value.
    pub unresolved_count: usize,
}

impl<T> Default for TypedValues<T> {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            unresolved_count: 0,
        }
    }
}

/// Structurally resolved legacy ASCII persistence scopes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Persistence {
    /// Outer persistence scope followed by each named ASCII section scope.
    pub scopes: Vec<Scope>,
    /// Complete finite type-2 scalar and array values in source order.
    pub real_values: TypedValues<RealRecord>,
    /// Complete type-1 signed-integer scalars and arrays in source order.
    pub integer_values: TypedValues<IntegerRecord>,
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
    pub type_3_values: TypedValues<Type3Record>,
    /// Type-4 byte-string scalars in source order.
    pub type_4_values: TypedValues<Type4Record>,
    /// Type-5 unsigned-decimal scalars and arrays in source order.
    pub type_5_values: TypedValues<Type5Record>,
    /// Type-6 compact-real scalars and arrays in source order.
    pub type_6_values: TypedValues<Type6Record>,
    /// Type-7 unsigned-decimal scalars and arrays in source order.
    pub type_7_values: TypedValues<Type7Record>,
    /// Type-9 unsigned-decimal scalars and arrays in source order.
    pub type_9_values: TypedValues<Type9Record>,
    /// Type-11 unsigned-decimal scalars and arrays in source order.
    pub type_11_values: TypedValues<Type11Record>,
}

impl Persistence {
    /// Resolve one unambiguous native model identity from legacy string rows.
    ///
    /// A legacy persistence tree can carry `model_name` in more than one
    /// object, including null placeholders on view records. Prefer a
    /// non-empty value owned by a root `Solid` object. If that role is absent,
    /// accept one distinct non-empty value across the remaining rows; distinct
    /// identities remain unresolved.
    pub fn model_name(&self) -> Option<(String, usize)> {
        let objects = self
            .objects
            .iter()
            .map(|object| (object.offset, object))
            .collect::<BTreeMap<_, _>>();
        let mut all = BTreeMap::<String, usize>::new();
        let mut preferred = BTreeMap::<String, usize>::new();
        for record in self
            .string_values
            .iter()
            .filter(|record| record.name == "model_name")
        {
            let StringPayload::Scalar {
                value: StringValue::Utf8 { text },
            } = &record.payload
            else {
                continue;
            };
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            all.entry(text.to_string()).or_insert(record.offset);
            let is_root_solid = record
                .parent
                .as_ref()
                .and_then(|parent| objects.get(parent))
                .is_some_and(|object| {
                    object.parent.is_none() && object.name.eq_ignore_ascii_case("solid")
                });
            if is_root_solid {
                preferred.entry(text.to_string()).or_insert(record.offset);
            }
        }
        let selected = if preferred.is_empty() { all } else { preferred };
        let mut values = selected.into_iter();
        let first = values.next()?;
        values.next().is_none().then_some(first)
    }

    /// Return the first non-null source-order `model_name` row.
    ///
    /// This is a source-identity fallback for legacy sections that contain
    /// several scoped model names. [`Self::model_name`] remains the resolver
    /// for relation evaluation and withholds conflicting identities.
    pub fn first_source_model_name(&self) -> Option<(String, usize)> {
        self.string_values
            .iter()
            .filter(|record| record.name == "model_name")
            .filter_map(|record| {
                let StringPayload::Scalar {
                    value: StringValue::Utf8 { text },
                } = &record.payload
                else {
                    return None;
                };
                let text = text.trim();
                (!text.is_empty() && !text.eq_ignore_ascii_case("NULL"))
                    .then(|| (text.to_owned(), record.offset))
            })
            .min_by_key(|(_, offset)| *offset)
    }

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
            .map(|value| {
                value
                    .continuation
                    .as_ref()
                    .map_or(0, |continuation| continuation.count.get())
            })
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
        let mut arrays = self.objects.iter().filter_map(|object| {
            let ObjectPayload::Array { elements, .. } = &object.payload else {
                return None;
            };
            (object.name == "unit_arr" && object.payload.is_complete())
                .then_some((object, elements))
        });
        let (array, elements) = arrays.next()?;
        arrays.next().is_none().then_some(())?;
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
                    object.id() == *element_id
                        && object.parent == Some(array.offset)
                        && object.name == "unit_arr"
                });
                let element = matches.next()?;
                matches.next().is_none().then_some(element)
            })
            .collect::<Option<Vec<_>>>()?;
        let first = element_records.first()?;
        let unit_type = self.unique_integer_scalar(first.offset, "unit_type")?;
        if unit_type != LEGACY_LENGTH_UNIT_TYPE
            || self.unique_utf8_scalar(first.offset, "name")?.is_empty()
        {
            return None;
        }
        let factor = self.unique_real_scalar(first.offset, "factor")?;
        let scale_mm = factor * LEGACY_INCH_TO_MM;
        (scale_mm.is_finite() && scale_mm > 0.0)
            .then_some(PrincipalUnitSystem::LegacyLengthScale(scale_mm.to_bits()))
    }

    fn unique_integer_scalar(&self, parent: usize, name: &str) -> Option<i32> {
        let mut matches = self
            .integer_values
            .rows
            .iter()
            .filter(|record| record.parent == Some(parent) && record.name == name);
        let record = matches.next()?;
        matches.next().is_none().then_some(())?;
        match &record.payload {
            NumericPayload::Scalar { value } => Some(*value),
            NumericPayload::Array(_) => None,
        }
    }

    fn unique_real_scalar(&self, parent: usize, name: &str) -> Option<f64> {
        let mut matches = self
            .real_values
            .rows
            .iter()
            .filter(|record| record.parent == Some(parent) && record.name == name);
        let record = matches.next()?;
        matches.next().is_none().then_some(())?;
        match &record.payload {
            NumericPayload::Scalar { value } => Some(value.value()),
            NumericPayload::Array(_) => None,
        }
    }

    fn unique_utf8_scalar<'a>(&'a self, parent: usize, name: &str) -> Option<&'a str> {
        let mut matches = self
            .string_values
            .iter()
            .filter(|record| record.parent == Some(parent) && record.name == name);
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
    let type_code = LegacyTypeCode::from(fields.next()?.parse::<u8>().ok()?);
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

pub(crate) fn object_node_id(offset: usize) -> String {
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
                .is_some_and(|declaration| matches!(declaration.type_code, LegacyTypeCode::Object))
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
                    .is_some_and(|declaration| {
                        matches!(declaration.type_code, LegacyTypeCode::Object)
                    })
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
                .filter(|declaration| matches!(declaration.type_code, LegacyTypeCode::Object))
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
                let payload = ObjectPayload::Array {
                    dimensions,
                    elements,
                };
                incomplete_arrays += usize::from(!payload.is_complete());
                payload
            } else {
                unresolved += 1;
                ObjectPayload::Opaque {
                    bytes: bytes.to_vec(),
                }
            };
            records.push(ObjectRecord {
                name: declaration.name.clone(),
                attribute_id: value.attribute_id,
                scope_offset: scope.range.start,
                parent: parents.get(&value.offset).copied(),
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

fn scalar_string_records<K: LegacyCode<Payload = StringValue>>(
    data: &[u8],
    scopes: &[Scope],
    identity_kind: ValueKind<K>,
    null_token: NullToken,
    parents: &BTreeMap<usize, usize>,
) -> TypedValues<ValueRecord<K>> {
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
                .filter(|declaration| declaration.type_code == identity_kind.type_code())
            else {
                continue;
            };
            let Some(bytes) = value
                .continuation
                .is_none()
                .then(|| data.get(value.payload.clone()))
                .flatten()
            else {
                unresolved += 1;
                continue;
            };
            records.push(ValueRecord {
                name: declaration.name.clone(),
                attribute_id: value.attribute_id,
                scope_offset: scope.range.start,
                parent: parents.get(&value.offset).copied(),
                depth: value.depth,
                payload: byte_string_value(bytes, null_token),
                offset: value.offset,
            });
        }
    }
    TypedValues {
        rows: records,
        unresolved_count: unresolved,
    }
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
        let mut active_arrays = BTreeMap::<u32, (usize, u32)>::new();
        let mut array_children = BTreeMap::<usize, Vec<&AttributeValue>>::new();
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
                array_children.entry(parent_offset).or_default().push(value);
                array_element_offsets.insert(value.offset);
                continue;
            }
            if declarations
                .get(&value.attribute_id)
                .is_some_and(|declaration| matches!(declaration.type_code, LegacyTypeCode::String))
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
                .filter(|declaration| matches!(declaration.type_code, LegacyTypeCode::String))
            else {
                continue;
            };
            let bytes = &data[value.payload.clone()];
            let payload = if let Some(dimensions) = array_dimensions(bytes) {
                let children = array_children
                    .get(&value.offset)
                    .map_or(&[][..], Vec::as_slice);
                let values = children
                    .iter()
                    .map(|child| {
                        if let Some(continuation) = &child.continuation {
                            unresolved += 1;
                            Err(continuation.clone())
                        } else {
                            Ok(string_value(&data[child.payload.clone()]))
                        }
                    })
                    .collect();
                unresolved += usize::from(value.continuation.is_some());
                let payload = StringPayload::Array {
                    dimensions,
                    values,
                    continuation: value.continuation.clone(),
                };
                incomplete_arrays += usize::from(!payload.is_complete());
                payload
            } else {
                if value.continuation.is_some() {
                    unresolved += 1;
                    continue;
                }
                StringPayload::Scalar {
                    value: string_value(bytes),
                }
            };
            records.push(ValueRecord {
                name: declaration.name.clone(),
                attribute_id: value.attribute_id,
                scope_offset: scope.range.start,
                parent: parents.get(&value.offset).copied(),
                depth: value.depth,
                payload,
                offset: value.offset,
            });
        }
    }
    (records, incomplete_arrays, unresolved)
}

fn numeric_records<K, T>(
    data: &[u8],
    scopes: &[Scope],
    identity_kind: ValueKind<K>,
    scalar: fn(&[u8]) -> Option<T>,
    parents: &BTreeMap<usize, usize>,
) -> TypedValues<ValueRecord<K>>
where
    K: LegacyCode<Payload = NumericPayload<T>>,
{
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
                .filter(|declaration| declaration.type_code == identity_kind.type_code())
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
                let runs = if let Some(continuation) = &value.continuation {
                    let Some(bytes) = data.get(continuation.rows.clone()) else {
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
                        let Some(run) = child
                            .continuation
                            .is_none()
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
                let Some(payload) = NumericPayload::array(dimensions, runs) else {
                    unresolved += next_index - index;
                    index = next_index;
                    continue;
                };
                (payload, next_index)
            } else {
                let Some(scalar_value) = value
                    .continuation
                    .is_none()
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
                name: declaration.name.clone(),
                attribute_id: value.attribute_id,
                scope_offset: scope.range.start,
                parent: parents.get(&value.offset).copied(),
                depth: value.depth,
                payload,
                offset: value.offset,
            });
            index = next_index;
        }
    }
    TypedValues {
        rows: records,
        unresolved_count: unresolved,
    }
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
        continuation: None,
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
                match &mut value.continuation {
                    Some(continuation) => {
                        continuation.rows.end = line_offset + current.len();
                        continuation.count = continuation.count.saturating_add(1);
                    }
                    None => {
                        value.continuation = Some(Continuation {
                            rows: line_offset..line_offset + current.len(),
                            count: NonZeroUsize::MIN,
                        });
                    }
                }
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
    let type_3_values = scalar_string_records(
        data,
        &scopes,
        ValueKind::TYPE3,
        NullToken::RepresentsNull,
        &parents,
    );
    let type_4_values = scalar_string_records(
        data,
        &scopes,
        ValueKind::TYPE4,
        NullToken::RepresentsBytes,
        &parents,
    );
    let real_values = numeric_records(data, &scopes, ValueKind::REAL, compact_real, &parents);
    let integer_values =
        numeric_records(data, &scopes, ValueKind::INTEGER, signed_integer, &parents);
    let type_5_values =
        numeric_records(data, &scopes, ValueKind::TYPE5, unsigned_integer, &parents);
    let type_6_values = numeric_records(data, &scopes, ValueKind::TYPE6, compact_real, &parents);
    let type_7_values =
        numeric_records(data, &scopes, ValueKind::TYPE7, unsigned_integer, &parents);
    let type_9_values =
        numeric_records(data, &scopes, ValueKind::TYPE9, unsigned_integer, &parents);
    let type_11_values =
        numeric_records(data, &scopes, ValueKind::TYPE11, unsigned_integer, &parents);
    Persistence {
        scopes,
        real_values,
        integer_values,
        objects,
        incomplete_object_array_count,
        unresolved_object_value_count,
        string_values,
        incomplete_string_array_count,
        unresolved_string_value_count,
        type_3_values,
        type_4_values,
        type_5_values,
        type_6_values,
        type_7_values,
        type_9_values,
        type_11_values,
    }
}

impl<K: LegacyCode> ValueRecord<K> {
    /// Native identity derived from the source offset.
    pub fn id(&self) -> String {
        format!(
            "creo:legacy_ascii:{}#{}",
            K::CODE.identity_token(),
            self.offset
        )
    }
}

impl<K: LegacyCode> Serialize for ValueRecord<K>
where
    K::Payload: Serialize,
{
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_struct("ValueRecord", 8)?;
        wire.serialize_field("id", &self.id())?;
        wire.serialize_field("name", &self.name)?;
        wire.serialize_field("attribute_id", &self.attribute_id)?;
        wire.serialize_field("scope_offset", &self.scope_offset)?;
        wire.serialize_field("parent", &self.parent.map(object_node_id))?;
        wire.serialize_field("depth", &self.depth)?;
        wire.serialize_field("payload", &self.payload)?;
        wire.serialize_field("offset", &self.offset)?;
        wire.end()
    }
}

impl ObjectRecord {
    /// Native identity derived from the source offset.
    pub fn id(&self) -> String {
        object_node_id(self.offset)
    }
}

impl Serialize for ObjectRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_struct("ObjectRecord", 8)?;
        wire.serialize_field("id", &self.id())?;
        wire.serialize_field("name", &self.name)?;
        wire.serialize_field("attribute_id", &self.attribute_id)?;
        wire.serialize_field("scope_offset", &self.scope_offset)?;
        wire.serialize_field("parent", &self.parent.map(object_node_id))?;
        wire.serialize_field("depth", &self.depth)?;
        wire.serialize_field("payload", &self.payload)?;
        wire.serialize_field("offset", &self.offset)?;
        wire.end()
    }
}

/// A legacy declaration code carried at the type level.
pub trait LegacyCode: Copy + Eq + std::fmt::Debug {
    /// The declaration code whose value rows carry this identity.
    const CODE: LegacyTypeCode;
    /// The payload shape stored by a value row of this code.
    type Payload;
}

macro_rules! legacy_code {
    ($(#[$doc:meta])* $name:ident, $code:ident, $payload:ty) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name;

        impl LegacyCode for $name {
            const CODE: LegacyTypeCode = LegacyTypeCode::$code;
            type Payload = $payload;
        }
    };
}

legacy_code!(
    /// The type-1 signed-integer declaration code.
    IntegerCode, Integer, NumericPayload<i32>
);
legacy_code!(
    /// The type-2 compact-real declaration code.
    RealCode, Real, NumericPayload<Real>
);
legacy_code!(
    /// The type-3 nullable byte-string declaration code.
    Type3Code, NullableString, StringValue
);
legacy_code!(
    /// The type-4 byte-string declaration code.
    Type4Code, ByteString, StringValue
);
legacy_code!(
    /// The type-5 unsigned-decimal declaration code.
    Type5Code, Unsigned5, NumericPayload<u32>
);
legacy_code!(
    /// The type-6 compact-real declaration code.
    Type6Code, Real6, NumericPayload<Real>
);
legacy_code!(
    /// The type-7 unsigned-decimal declaration code.
    Type7Code, Unsigned7, NumericPayload<u32>
);
legacy_code!(
    /// The type-9 unsigned-decimal declaration code.
    Type9Code, Unsigned9, NumericPayload<u32>
);
legacy_code!(
    /// The type-10 byte-string declaration code.
    StringCode, String, StringPayload
);
legacy_code!(
    /// The type-11 unsigned-decimal declaration code.
    Type11Code, Unsigned11, NumericPayload<u32>
);

/// Identity marker naming one legacy declaration code.
#[derive(Debug, PartialEq, Eq)]
pub struct ValueKind<K>(std::marker::PhantomData<fn() -> K>);

impl<K> Copy for ValueKind<K> {}

impl<K> Clone for ValueKind<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K: LegacyCode> ValueKind<K> {
    const KIND: Self = Self(std::marker::PhantomData);

    /// The declaration code whose values carry this identity.
    fn type_code(self) -> LegacyTypeCode {
        K::CODE
    }
}

impl ValueKind<IntegerCode> {
    /// Identity token for integer values.
    pub const INTEGER: Self = Self::KIND;
}

impl ValueKind<RealCode> {
    /// Identity token for real values.
    pub const REAL: Self = Self::KIND;
}

impl ValueKind<Type6Code> {
    /// Identity token for `type_6` values.
    pub const TYPE6: Self = Self::KIND;
}

impl ValueKind<Type5Code> {
    /// Identity token for `type_5` values.
    pub const TYPE5: Self = Self::KIND;
}

impl ValueKind<Type7Code> {
    /// Identity token for `type_7` values.
    pub const TYPE7: Self = Self::KIND;
}

impl ValueKind<Type9Code> {
    /// Identity token for `type_9` values.
    pub const TYPE9: Self = Self::KIND;
}

impl ValueKind<Type11Code> {
    /// Identity token for `type_11` values.
    pub const TYPE11: Self = Self::KIND;
}

impl ValueKind<Type3Code> {
    /// Identity token for `type_3` values.
    pub const TYPE3: Self = Self::KIND;
}

impl ValueKind<Type4Code> {
    /// Identity token for `type_4` values.
    pub const TYPE4: Self = Self::KIND;
}

#[cfg(test)]
mod tests;
