// SPDX-License-Identifier: Apache-2.0
//! Structural grammar for legacy ASCII persistence records.

use cadmpeg_core::{decode::DecodeContext, CodecError};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::num::NonZeroUsize;
use std::ops::Range;

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

mod numeric_array;
pub(crate) mod type_code;
use type_code::LegacyTypeCode;

pub(crate) fn value_index<'a, K: LegacyCode>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &'a [ValueRecord<K>],
    index: &mut BTreeMap<(usize, &'a str), Vec<&'a ValueRecord<K>>>,
) -> Result<(), CodecError> {
    for record in ctx.admit_iter(records, "creo legacy value index traversal")? {
        if let Some(parent) = record.parent {
            let key = (parent, record.name.as_str());
            match ctx.entry_btree_map(index, key, "creo legacy value index nodes")? {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let mut values = Vec::new();
                    ctx.reserve_vec(&mut values, 1, "creo legacy value index rows")?;
                    values.push(record);
                    entry.insert(values);
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let values = entry.get_mut();
                    ctx.reserve_vec(values, 1, "creo legacy value index rows")?;
                    values.push(record);
                }
            }
        }
    }
    Ok(())
}

const PRINCIPAL_UNIT_NAME: &str = "principal_sys_units";
const MILLIMETER_NEWTON_SECOND: &str = "millimeter Newton Second (mmNs)";
const INCH_POUND_MASS_SECOND: &str = "Inch lbm Second (Pro/E Default)";
const LEGACY_INCH_TO_MM: f64 = 25.4;
const LEGACY_LENGTH_UNIT_TYPE: i32 = 0;

/// Active coordinate-unit system selected by a model-level persistence field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PrincipalUnitSystem {
    /// Millimeter, Newton, second.
    MillimeterNewtonSecond,
    /// Millimeter, kilogram, second.
    MillimeterKilogramSecond,
    /// Inch, pound mass, second.
    InchPoundMassSecond,
    /// A complete legacy `unit_arr` length record with a source-specific scale.
    LegacyLengthScale(cadmpeg_ir::scalar::PositiveReal),
}

impl PrincipalUnitSystem {
    /// Scale from stored coordinate lengths to canonical millimeters.
    pub(crate) fn length_scale_mm(self) -> Option<cadmpeg_ir::scalar::PositiveReal> {
        match self {
            Self::MillimeterNewtonSecond | Self::MillimeterKilogramSecond => {
                Some(cadmpeg_ir::scalar::PositiveReal::ONE)
            }
            Self::InchPoundMassSecond => cadmpeg_ir::scalar::PositiveReal::new(LEGACY_INCH_TO_MM),
            Self::LegacyLengthScale(scale) => Some(scale),
        }
    }
}

impl std::fmt::Display for PrincipalUnitSystem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MillimeterNewtonSecond => formatter.write_str("mmNs"),
            Self::MillimeterKilogramSecond => formatter.write_str("mmKs"),
            Self::InchPoundMassSecond => formatter.write_str("inLbmS"),
            Self::LegacyLengthScale(scale) => {
                write!(formatter, "legacy_length_scale_mm:{:.17}", scale.get())
            }
        }
    }
}

/// One finite legacy type-2 real, stored by its exact IEEE-754 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Real(u64);

impl Real {
    /// Construct a real from its exact stored IEEE-754 bits.
    #[cfg(test)]
    pub(crate) const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    /// Numeric value represented by the stored bits.
    pub(crate) fn value(self) -> f64 {
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
pub(crate) struct NumericRun<T> {
    /// Number of consecutive array elements carrying `value`.
    pub(crate) count: u32,
    /// Element value.
    pub(crate) value: T,
}

/// Complete semantic payload of one numeric legacy value row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum NumericPayload<T> {
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
    pub(crate) fn array(
        ctx: &DecodeContext<'_>,
        dimensions: Vec<u32>,
        runs: Vec<NumericRun<T>>,
    ) -> Result<Option<Self>, CodecError> {
        Ok(numeric_array::NumericArray::try_new(ctx, dimensions, runs)?.map(Self::Array))
    }

    /// Number of logical scalar elements represented by this payload.
    pub(crate) fn element_count(&self) -> usize {
        match self {
            Self::Scalar { .. } => 1,
            Self::Array(array) => array.element_count(),
        }
    }
}

/// One typed legacy attribute value in the scoped object tree.
pub(crate) struct ValueRecord<K: LegacyCode> {
    /// Declared attribute name.
    pub(crate) name: String,
    /// Scope-local declaration identifier.
    pub(crate) attribute_id: u32,
    /// Byte offset of the owning attribute-ID scope.
    pub(crate) scope_offset: usize,
    /// Owning type-0 object node, when the depth tree supplies one.
    pub(crate) parent: Option<usize>,
    /// Object-tree nesting depth of the scalar or array header.
    pub(crate) depth: u32,
    /// Typed value payload.
    pub(crate) payload: K::Payload,
    /// Byte offset of the scalar row or array header.
    pub(crate) offset: usize,
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
pub(crate) type RealRun = NumericRun<Real>;
/// Complete semantic payload of one legacy type-2 value row.
#[cfg(test)]
pub(crate) type RealPayload = NumericPayload<Real>;
/// One completely decoded legacy type-2 attribute value.
pub(crate) type RealRecord = ValueRecord<RealCode>;
/// One run in a type-1 integer array.
#[cfg(test)]
pub(crate) type IntegerRun = NumericRun<i32>;
/// Complete semantic payload of one legacy type-1 value row.
#[cfg(test)]
pub(crate) type IntegerPayload = NumericPayload<i32>;
/// One completely decoded legacy type-1 attribute value.
pub(crate) type IntegerRecord = ValueRecord<IntegerCode>;
/// Complete semantic payload of one unsigned-decimal legacy value.
#[cfg(test)]
type UnsignedPayload = NumericPayload<u32>;
/// One completely decoded legacy type-5 attribute value.
type Type5Record = ValueRecord<Type5Code>;
/// One completely decoded legacy type-6 attribute value.
type Type6Record = ValueRecord<Type6Code>;
/// One completely decoded legacy type-7 attribute value.
type Type7Record = ValueRecord<Type7Code>;
/// One completely decoded legacy type-9 attribute value.
type Type9Record = ValueRecord<Type9Code>;
/// One completely decoded legacy type-11 attribute value.
type Type11Record = ValueRecord<Type11Code>;

/// Structural payload of one legacy type-0 object node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ObjectPayload {
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
        /// Completeness derived from the admitted dimensions and elements.
        complete: bool,
    },
    /// A type-0 payload outside the defined object forms.
    Opaque {
        /// Uninterpreted payload bytes after the attribute identifier.
        bytes: Vec<u8>,
    },
}

fn object_array_is_complete(
    ctx: &DecodeContext<'_>,
    dimensions: &[u32],
    elements: &[String],
) -> Result<bool, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut count = 1u64;
    let mut dimensions = dimensions.iter();
    while dimensions.len() != 0 {
        let Some(dimension) =
            ctx.next_charged(&mut dimensions, "creo object array extent traversal")?
        else {
            break;
        };
        let Some(product) = count.checked_mul(u64::from(*dimension)) else {
            return Ok(false);
        };
        count = product;
    }
    Ok(usize::try_from(count).ok() == Some(elements.len()))
}

impl ObjectPayload {
    /// Whether an array has exactly its declared extent product of elements.
    pub(crate) fn is_complete(&self, ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let Self::Array {
            dimensions,
            elements,
            ..
        } = self
        else {
            return Ok(false);
        };
        object_array_is_complete(ctx, dimensions, elements)
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
                complete,
            } => {
                wire.serialize_field("dimensions", dimensions)?;
                wire.serialize_field("elements", elements)?;
                wire.serialize_field("complete", complete)?;
            }
            Self::Opaque { bytes } => wire.serialize_field("bytes", bytes)?,
            _ => {}
        }
        wire.end()
    }
}

/// One legacy type-0 object node in the depth-defined ownership tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObjectRecord {
    /// Declared attribute name.
    pub(crate) name: String,
    /// Scope-local declaration identifier.
    pub(crate) attribute_id: u32,
    /// Byte offset of the owning attribute-ID scope.
    pub(crate) scope_offset: usize,
    /// Owning type-0 object node, when the depth tree supplies one.
    pub(crate) parent: Option<usize>,
    /// Object-tree nesting depth.
    pub(crate) depth: u32,
    /// Stored object form.
    pub(crate) payload: ObjectPayload,
    /// Byte offset of the value row.
    pub(crate) offset: usize,
}

/// One legacy byte string or null element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum StringValue {
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
    pub(crate) fn undecoded_encoding_count(&self) -> usize {
        usize::from(matches!(self, Self::Bytes { .. }))
    }
}

/// Semantic payload of one legacy byte-string value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StringPayload {
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
        /// Header continuation retained for the test-only completeness traversal.
        #[cfg(test)]
        continuation: Option<Continuation>,
        /// Completeness derived while the source rows were admitted.
        complete: bool,
        /// Source indices of supported values, in source order.
        accepted_value_indices: Vec<usize>,
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
                dimensions,
                values,
                complete,
                accepted_value_indices,
                ..
            } => {
                wire.serialize_field("form", "array")?;
                wire.serialize_field("dimensions", dimensions)?;
                wire.serialize_field(
                    "values",
                    &StringValues {
                        values,
                        accepted_indices: accepted_value_indices,
                    },
                )?;
                wire.serialize_field("complete", complete)?;
            }
        }
        wire.end()
    }
}

/// Supported values selected by their admitted source indices.
struct StringValues<'a> {
    values: &'a [Result<StringValue, Continuation>],
    accepted_indices: &'a [usize],
}

impl Serialize for StringValues<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.accepted_indices.iter().filter_map(|index| {
            self.values
                .get(*index)
                .and_then(|value| value.as_ref().ok())
        }))
    }
}

impl StringPayload {
    /// Whether every declared string row has a supported, complete value.
    #[cfg(test)]
    fn is_complete(&self, ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let Self::Array {
            dimensions,
            values,
            continuation,
            ..
        } = self
        else {
            return Ok(false);
        };
        Ok(continuation.is_none()
            && dimensions
                .first()
                .and_then(|dimension| usize::try_from(*dimension).ok())
                .is_some_and(|count| count == values.len())
            && ctx.all_by(
                values,
                |value| Ok(value.is_ok()),
                "creo string array completeness traversal",
            )?)
    }

    /// Number of logical string elements represented by this payload.
    #[cfg(test)]
    pub(crate) fn element_count(&self, ctx: &DecodeContext<'_>) -> Result<usize, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        Ok(match self {
            Self::Scalar { .. } => 1,
            Self::Array { values, .. } => ctx
                .admit_iter(values, "creo legacy string element count")?
                .filter(|value| value.is_ok())
                .count(),
        })
    }

    /// Count elements whose character encoding remains uninterpreted.
    #[cfg(test)]
    pub(crate) fn undecoded_encoding_count(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<usize, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        Ok(match self {
            Self::Scalar { value } => value.undecoded_encoding_count(),
            Self::Array { values, .. } => ctx
                .admit_iter(values, "creo legacy string encoding count")?
                .filter_map(|value| value.as_ref().ok())
                .map(StringValue::undecoded_encoding_count)
                .sum(),
        })
    }
}

/// One decoded legacy byte-string value.
pub(crate) type StringRecord = ValueRecord<StringCode>;
/// One decoded legacy type-3 scalar byte-string value.
type Type3Record = ValueRecord<Type3Code>;
/// One decoded legacy type-4 scalar byte-string value.
type Type4Record = ValueRecord<Type4Code>;

/// One unique `@<name> <id> <type-code>` declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttributeDeclaration {
    /// Attribute identifier referenced by value rows in the same scope.
    pub(crate) id: u32,
    /// Attribute name without the leading `@`.
    pub(crate) name: String,
    /// Stored numeric type code.
    pub(crate) type_code: LegacyTypeCode,
    /// Byte offset of the declaration line.
    offset: usize,
}

/// One `<depth> <attribute-id> <payload>` value row.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AttributeValue {
    /// Object-tree nesting depth.
    depth: u32,
    /// Identifier of the owning attribute declaration in the same scope.
    attribute_id: u32,
    /// Byte offset of the value row.
    offset: usize,
    /// Byte range of the payload after the second field separator.
    payload: Range<usize>,
    /// Immediately following `$` rows, when present.
    continuation: Option<Continuation>,
}

impl cadmpeg_core::decode::cost::DecodeCost for AttributeValue {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.depth,
                &self.attribute_id,
                &self.offset,
                &self.payload.start,
                &self.payload.end,
                &self.continuation,
            ),
            ctx,
            operation,
        )
    }
}

/// A nonempty sequence of continuation rows following one value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Continuation {
    /// Contiguous source range containing the rows.
    rows: Range<usize>,
    /// Number of rows in the source range.
    count: NonZeroUsize,
}

impl cadmpeg_core::decode::cost::DecodeCost for Continuation {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(&self.rows.start, &self.rows.end, &self.count),
            ctx,
            operation,
        )
    }
}

/// Declarations and values owned by one outer object or named ASCII section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Scope {
    /// Complete byte extent scanned for this scope.
    range: Range<usize>,
    /// First declaration for each identifier, in source order.
    declarations: Vec<AttributeDeclaration>,
    /// Fixed-key positions in the source-ordered declaration table.
    declaration_positions: HashMap<u32, usize>,
    /// Value rows whose declaration resolves uniquely in this scope.
    values: Vec<AttributeValue>,
    /// Numeric value rows whose identifier has no unique local declaration.
    unresolved_value_count: usize,
    /// Repeated local identifiers whose name or type code conflicts.
    conflicting_declaration_count: usize,
}

/// Parsed rows and unresolved-row count for one legacy declaration type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypedValues<T> {
    /// Complete typed rows in source order.
    pub(crate) rows: Vec<T>,
    /// Source rows not represented by a complete typed value.
    pub(crate) unresolved_count: usize,
}

impl<T> Default for TypedValues<T> {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            unresolved_count: 0,
        }
    }
}

/// Source-ordered typed values and structural counts from legacy ASCII persistence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Persistence {
    /// Counts of the outer persistence scope and named ASCII section scopes.
    pub(crate) counts: PersistenceCounts,
    /// Complete finite type-2 scalar and array values in source order.
    pub(crate) real_values: TypedValues<RealRecord>,
    /// Complete type-1 signed-integer scalars and arrays in source order.
    pub(crate) integer_values: TypedValues<IntegerRecord>,
    /// Type-0 object nodes in source order.
    pub(crate) objects: Vec<ObjectRecord>,
    /// Type-0 arrays whose direct element count differs from their extents.
    pub(crate) incomplete_object_array_count: usize,
    /// Type-0 value rows outside the defined object forms.
    pub(crate) unresolved_object_value_count: usize,
    /// Type-10 byte-string scalars and arrays in source order.
    pub(crate) string_values: Vec<StringRecord>,
    /// Type-10 arrays whose direct element count differs from the first extent.
    pub(crate) incomplete_string_array_count: usize,
    /// Type-10 rows that use an undefined continuation form.
    pub(crate) unresolved_string_value_count: usize,
    /// Type-3 nullable byte-string scalars in source order.
    pub(crate) type_3_values: TypedValues<Type3Record>,
    /// Type-4 byte-string scalars in source order.
    pub(crate) type_4_values: TypedValues<Type4Record>,
    /// Type-5 unsigned-decimal scalars and arrays in source order.
    pub(crate) type_5_values: TypedValues<Type5Record>,
    /// Type-6 compact-real scalars and arrays in source order.
    pub(crate) type_6_values: TypedValues<Type6Record>,
    /// Type-7 unsigned-decimal scalars and arrays in source order.
    pub(crate) type_7_values: TypedValues<Type7Record>,
    /// Type-9 unsigned-decimal scalars and arrays in source order.
    pub(crate) type_9_values: TypedValues<Type9Record>,
    /// Type-11 unsigned-decimal scalars and arrays in source order.
    pub(crate) type_11_values: TypedValues<Type11Record>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PersistenceCounts {
    pub(crate) scopes: usize,
    pub(crate) declarations: usize,
    pub(crate) values: usize,
    pub(crate) continuations: usize,
    pub(crate) unresolved_values: usize,
    pub(crate) conflicting_declarations: usize,
}

impl Persistence {
    /// Resolve one unambiguous native model identity from legacy string rows.
    ///
    /// A legacy persistence tree can carry `model_name` in more than one
    /// object, including null placeholders on view records. Prefer a
    /// non-empty value owned by a root `Solid` object. If that role is absent,
    /// accept one distinct non-empty value across the remaining rows; distinct
    /// identities remain unresolved.
    pub(crate) fn model_name(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<(String, usize)>, CodecError> {
        let mut object_storage = ctx.reserve_scoped(0, "creo legacy model name object storage")?;
        let mut objects = std::collections::HashMap::new();
        for object in ctx.admit_iter(&self.objects, "creo legacy model name object traversal")? {
            object_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut objects,
                    object.offset,
                    object,
                    "creo legacy model name object nodes",
                )
            })?;
        }
        let mut all = None::<(&str, usize)>;
        let mut all_conflict = false;
        let mut preferred = None::<(&str, usize)>;
        let mut preferred_conflict = false;
        for record in ctx
            .admit_iter(
                &self.string_values,
                "creo legacy model name value traversal",
            )?
            .filter(|record| record.name == "model_name")
        {
            let StringPayload::Scalar {
                value: StringValue::Utf8 { text },
            } = &record.payload
            else {
                continue;
            };
            let text = ctx.trim_text(text, "creo legacy model name trim")?;
            if text.is_empty() {
                continue;
            }
            match all {
                None => all = Some((text, record.offset)),
                Some((known, _))
                    if !ctx.equal(known, text, "creo legacy model name equality")? =>
                {
                    all_conflict = true;
                }
                Some(_) => {}
            }
            let is_root_solid = match record
                .parent
                .as_ref()
                .and_then(|parent| objects.get(parent))
            {
                Some(object) => {
                    object.parent.is_none()
                        && ctx.eq_ignore_ascii_case(
                            &object.name,
                            "solid",
                            "creo legacy root object name",
                        )?
                }
                None => false,
            };
            if is_root_solid {
                match preferred {
                    None => preferred = Some((text, record.offset)),
                    Some((known, _))
                        if !ctx.equal(known, text, "creo legacy model name equality")? =>
                    {
                        preferred_conflict = true;
                    }
                    Some(_) => {}
                }
            }
        }
        let selected = if preferred.is_some() {
            if preferred_conflict {
                None
            } else {
                preferred
            }
        } else if all_conflict {
            None
        } else {
            all
        };
        selected
            .map(|(text, offset)| {
                ctx.copy_retained_text(text, "creo legacy model name")
                    .map(|name| (name, offset))
            })
            .transpose()
    }

    /// Return the first non-null source-order `model_name` row.
    ///
    /// This is a source-identity fallback for legacy sections that contain
    /// several scoped model names. [`Self::model_name`] remains the resolver
    /// for relation evaluation and withholds conflicting identities.
    pub(crate) fn first_source_model_name(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<(String, usize)>, CodecError> {
        let mut selected = None::<(&str, usize)>;
        for record in ctx.admit_iter(&self.string_values, "creo legacy source model rows")? {
            if record.name != "model_name" {
                continue;
            }
            let StringPayload::Scalar {
                value: StringValue::Utf8 { text },
            } = &record.payload
            else {
                continue;
            };
            let text = ctx.trim_text(text, "creo legacy source model name trim")?;
            if text.is_empty()
                || ctx.eq_ignore_ascii_case(text, "NULL", "creo legacy null source model name")?
            {
                continue;
            }
            if selected.is_none_or(|(_, offset)| record.offset < offset) {
                selected = Some((text, record.offset));
            }
        }
        selected
            .map(|(text, offset)| {
                ctx.copy_retained_text(text, "creo legacy first source model name")
                    .map(|name| (name, offset))
            })
            .transpose()
    }

    /// Resolve one unambiguous legacy principal-unit string.
    pub(crate) fn principal_unit_system(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<PrincipalUnitSystem>, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let mut candidate = None;
        let mut found = false;
        let mut records = self.string_values.iter();
        while records.len() != 0 {
            let Some(record) =
                ctx.next_charged(&mut records, "creo legacy principal unit selection")?
            else {
                break;
            };
            if record.name != PRINCIPAL_UNIT_NAME {
                continue;
            }
            found = true;
            if candidate.is_some() {
                return Ok(None);
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
                _ => return Ok(None),
            };
        }
        if found {
            Ok(candidate)
        } else {
            self.legacy_unit_array_system(ctx)
        }
    }

    fn legacy_unit_array_system(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<PrincipalUnitSystem>, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        if self.objects.is_empty() {
            return Ok(None);
        }
        let Some(array) = crate::decode::uniqueness::exactly_one_by(
            ctx,
            &self.objects,
            |object| {
                if object.name != "unit_arr" {
                    return Ok(false);
                }
                object.payload.is_complete(ctx)
            },
            "creo legacy unit array selection",
        )?
        else {
            return Ok(None);
        };
        let ObjectPayload::Array { elements, .. } = &array.payload else {
            return Ok(None);
        };
        if elements.is_empty() {
            return Ok(None);
        }
        let mut identity_storage = ctx.reserve_scoped(0, "creo legacy unit identity storage")?;
        let mut element_ids = BTreeSet::new();
        let mut ids = elements.iter();
        while ids.len() != 0 {
            let Some(element_id) =
                ctx.next_charged(&mut ids, "creo legacy unit identity traversal")?
            else {
                break;
            };
            if ctx.contains_btree_set(
                &element_ids,
                element_id,
                "creo legacy unit identity membership",
            )? {
                return Ok(None);
            }
            identity_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut element_ids,
                    element_id,
                    "creo legacy unit array element identities",
                )
            })?;
        }
        let mut object_storage = ctx.reserve_scoped(0, "creo legacy unit object storage")?;
        let mut objects = HashMap::<usize, Option<&ObjectRecord>>::new();
        for object in ctx.admit_iter(&self.objects, "creo legacy unit object index traversal")? {
            if object.parent != Some(array.offset) || object.name != "unit_arr" {
                continue;
            }
            match object_storage.with_storage(|| {
                ctx.entry_hash_map(
                    &mut objects,
                    object.offset,
                    "creo legacy unit object index nodes",
                )
            })? {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(Some(object));
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
            }
        }
        let mut first = None;
        let mut ids = elements.iter();
        while ids.len() != 0 {
            let Some(element_id) =
                ctx.next_charged(&mut ids, "creo legacy unit element traversal")?
            else {
                break;
            };
            let offset = match ctx.strip_prefix(
                element_id,
                "creo:legacy_ascii:object#",
                "creo legacy unit object prefix",
            )? {
                Some(digits) => ctx
                    .parse_text::<usize>(digits, "creo scalar text parsing")?
                    .ok(),
                None => None,
            };
            let Some(element) = offset
                .and_then(|offset| objects.get(&offset))
                .copied()
                .flatten()
            else {
                return Ok(None);
            };
            first.get_or_insert(element);
        }
        let Some(first) = first else {
            return Ok(None);
        };
        let Some(unit_type) = self.unique_integer_scalar(ctx, first.offset, "unit_type")? else {
            return Ok(None);
        };
        if unit_type != LEGACY_LENGTH_UNIT_TYPE
            || self
                .unique_utf8_scalar(ctx, first.offset, "name")?
                .is_none_or(str::is_empty)
        {
            return Ok(None);
        }
        let Some(factor) = self.unique_real_scalar(ctx, first.offset, "factor")? else {
            return Ok(None);
        };
        let scale_mm = factor * LEGACY_INCH_TO_MM;
        Ok(cadmpeg_ir::scalar::PositiveReal::new(scale_mm)
            .map(PrincipalUnitSystem::LegacyLengthScale))
    }

    fn unique_integer_scalar(
        &self,
        ctx: &DecodeContext<'_>,
        parent: usize,
        name: &str,
    ) -> Result<Option<i32>, CodecError> {
        let Some(record) = crate::decode::uniqueness::exactly_one_by(
            ctx,
            &self.integer_values.rows,
            |record| {
                Ok(record.parent == Some(parent)
                    && ctx.equal(
                        record.name.as_str(),
                        name,
                        "creo legacy unit scalar name equality",
                    )?)
            },
            "creo legacy unit scalar selection",
        )?
        else {
            return Ok(None);
        };
        Ok(match &record.payload {
            NumericPayload::Scalar { value } => Some(*value),
            NumericPayload::Array(_) => None,
        })
    }

    fn unique_real_scalar(
        &self,
        ctx: &DecodeContext<'_>,
        parent: usize,
        name: &str,
    ) -> Result<Option<f64>, CodecError> {
        let Some(record) = crate::decode::uniqueness::exactly_one_by(
            ctx,
            &self.real_values.rows,
            |record| {
                Ok(record.parent == Some(parent)
                    && ctx.equal(
                        record.name.as_str(),
                        name,
                        "creo legacy unit scalar name equality",
                    )?)
            },
            "creo legacy unit scalar selection",
        )?
        else {
            return Ok(None);
        };
        Ok(match &record.payload {
            NumericPayload::Scalar { value } => Some(value.value()),
            NumericPayload::Array(_) => None,
        })
    }

    fn unique_utf8_scalar<'a>(
        &'a self,
        ctx: &DecodeContext<'_>,
        parent: usize,
        name: &str,
    ) -> Result<Option<&'a str>, CodecError> {
        let Some(record) = crate::decode::uniqueness::exactly_one_by(
            ctx,
            &self.string_values,
            |record| {
                Ok(record.parent == Some(parent)
                    && ctx.equal(
                        record.name.as_str(),
                        name,
                        "creo legacy unit scalar name equality",
                    )?)
            },
            "creo legacy unit scalar selection",
        )?
        else {
            return Ok(None);
        };
        Ok(match &record.payload {
            StringPayload::Scalar {
                value: StringValue::Utf8 { text },
            } => Some(text),
            _ => None,
        })
    }
}

pub(crate) fn line<'a>(
    ctx: &DecodeContext<'_>,
    data: &'a [u8],
    start: usize,
) -> Result<Option<(&'a [u8], usize)>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(bytes) = data.get(start..) else {
        return Ok(None);
    };
    let mut relative_end = None;
    let mut positions = bytes.iter().enumerate();
    while positions.len() != 0 {
        let Some((offset, byte)) = ctx.next_charged(&mut positions, "creo legacy line scan")?
        else {
            break;
        };
        if *byte == b'\n' {
            relative_end = Some(offset);
            break;
        }
    }
    let end = relative_end.map_or(data.len(), |end| start + end);
    let next = relative_end.map_or(end, |_| end + 1);
    Ok(Some((
        data[start..end]
            .strip_suffix(b"\r")
            .unwrap_or(&data[start..end]),
        next,
    )))
}

/// Parse the next borrowed persistence or TOC field.
pub(crate) fn text_field<'a>(
    ctx: &DecodeContext<'_>,
    text: &mut &'a str,
    ascii: bool,
) -> Result<Option<&'a str>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let whitespace = |c: char| {
        if ascii {
            c.is_ascii_whitespace()
        } else {
            c.is_whitespace()
        }
    };
    let mut first = None;
    let mut characters = text.char_indices();
    while !characters.as_str().is_empty() {
        let Some((offset, c)) =
            ctx.next_charged(&mut characters, "creo text field whitespace")?
        else {
            break;
        };
        if !whitespace(c) {
            first = Some((offset, c.len_utf8()));
            break;
        }
    }
    let Some((start, first_width)) = first else {
        *text = "";
        return Ok(None);
    };
    let remaining = &text[start..];
    let mut boundary = None;
    let mut characters = remaining[first_width..].char_indices();
    while !characters.as_str().is_empty() {
        let Some((offset, c)) =
            ctx.next_charged(&mut characters, "creo text field boundary")?
        else {
            break;
        };
        if whitespace(c) {
            boundary = Some((first_width + offset, c.len_utf8()));
            break;
        }
    }
    let end = boundary.map_or(remaining.len(), |(end, _)| end);
    *text = boundary.map_or("", |(end, width)| &remaining[end + width..]);
    Ok(Some(&remaining[..end]))
}

pub(crate) fn parse_declaration<'a>(
    ctx: &DecodeContext<'_>,
    line: &'a [u8],
) -> Result<Option<(u32, &'a str, LegacyTypeCode)>, CodecError> {
    let Some(line) = ctx
        .validate_utf8(line, "creo legacy declaration UTF-8 validation")?
        .ok()
    else {
        return Ok(None);
    };
    let mut fields = line;
    let Some(name) = text_field(ctx, &mut fields, true)?.and_then(|name| name.strip_prefix('@'))
    else {
        return Ok(None);
    };
    if name.is_empty()
        || !ctx.all_by(
            name.bytes(),
            |byte| Ok(byte.is_ascii_graphic()),
            "creo legacy declaration name validation",
        )?
    {
        return Ok(None);
    }
    let Some(id_text) = text_field(ctx, &mut fields, true)? else {
        return Ok(None);
    };
    let Some(id) = ctx.parse_text(id_text, "creo scalar text parsing")?.ok() else {
        return Ok(None);
    };
    let Some(code_text) = text_field(ctx, &mut fields, true)? else {
        return Ok(None);
    };
    let Some(code) = ctx
        .parse_text::<u8>(code_text, "creo scalar text parsing")?
        .ok()
    else {
        return Ok(None);
    };
    Ok(text_field(ctx, &mut fields, true)?.is_none().then_some((
        id,
        name,
        LegacyTypeCode::from(code),
    )))
}

pub(crate) fn starts_with_declaration(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    start: usize,
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some((source, _)) = line(ctx, data, start)? else {
        return Ok(false);
    };
    Ok(parse_declaration(ctx, source)?.is_some())
}

fn decimal(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    mut offset: usize,
) -> Result<Option<(u32, usize)>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let start = offset;
    let mut value = 0u32;
    let mut digits = bytes.get(start..).unwrap_or_default().iter();
    while digits.len() != 0 {
        let Some(digit) =
            ctx.next_charged(&mut digits, "creo legacy decimal digits")?
        else {
            break;
        };
        if !digit.is_ascii_digit() {
            break;
        }
        let Some(next) = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(u32::from(*digit - b'0')))
        else {
            return Ok(None);
        };
        value = next;
        offset += 1;
    }
    Ok((offset > start).then_some((value, offset)))
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
        bits = (bits << 4) | fill;
    }
    f64::from_bits(bits).is_finite().then_some(Real(bits))
}

fn signed_integer(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<i32>, cadmpeg_core::CodecError> {
    let Some(text) = ctx.validate_utf8(bytes, "creo UTF-8 validation")?.ok() else {
        return Ok(None);
    };
    // FromStr admits ASCII decimal digits and one sign. The persistence grammar excludes '+'.
    if text.starts_with('+') {
        return Ok(None);
    }
    Ok(ctx.parse_text(text, "creo scalar text parsing")?.ok())
}

fn unsigned_integer(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<u32>, cadmpeg_core::CodecError> {
    let Some(text) = ctx.validate_utf8(bytes, "creo UTF-8 validation")?.ok() else {
        return Ok(None);
    };
    if text.starts_with('+') {
        return Ok(None);
    }
    Ok(ctx.parse_text(text, "creo scalar text parsing")?.ok())
}

fn array_dimensions(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<Vec<u32>>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut dimensions = Vec::new();
    let mut cursor = 0;
    while bytes.get(cursor) == Some(&b'[') {
        let Some((dimension, after_dimension)) = decimal(ctx, bytes, cursor + 1)? else {
            return Ok(None);
        };
        if dimension == 0 || bytes.get(after_dimension) != Some(&b']') {
            return Ok(None);
        }
        ctx.reserve_vec(&mut dimensions, 1, "creo legacy array dimensions")?;
        dimensions.push(dimension);
        cursor = after_dimension + 1;
    }
    Ok((!dimensions.is_empty() && cursor == bytes.len()).then_some(dimensions))
}

fn numeric_run<T>(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scalar: fn(&DecodeContext<'_>, &[u8]) -> Result<Option<T>, CodecError>,
) -> Result<Option<NumericRun<T>>, CodecError> {
    let (count, scalar_bytes) = if let Some(star) = ctx.position_by(
        bytes,
        |byte| Ok(*byte == b'*'),
        "creo legacy numeric repeat marker",
    )? {
        let Some((count, after_count)) = decimal(ctx, bytes, 0)? else {
            return Ok(None);
        };
        if count == 0 || after_count != star {
            return Ok(None);
        }
        (count, &bytes[star + 1..])
    } else {
        (1, bytes)
    };
    let Some(value) = scalar(ctx, scalar_bytes)? else {
        return Ok(None);
    };
    Ok(Some(NumericRun { count, value }))
}

fn continuation_numeric_runs<T>(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scalar: fn(&DecodeContext<'_>, &[u8]) -> Result<Option<T>, CodecError>,
) -> Result<Option<Vec<NumericRun<T>>>, CodecError> {
    let mut runs = Vec::new();
    let mut remaining = bytes;
    loop {
        let newline = ctx.position_by(
            remaining,
            |byte| Ok(*byte == b'\n'),
            "creo legacy continuation lines",
        )?;
        let row = &remaining[..newline.unwrap_or(remaining.len())];
        let row = row.strip_suffix(b"\r").unwrap_or(row);
        let Some(mut tokens) = row.strip_prefix(b"$") else {
            return Ok(None);
        };
        loop {
            let comma = ctx.position_by(
                tokens,
                |byte| Ok(*byte == b','),
                "creo legacy continuation tokens",
            )?;
            let token = &tokens[..comma.unwrap_or(tokens.len())];
            if token.is_empty() {
                if comma.is_some() {
                    return Ok(None);
                }
            } else {
                let Some(run) = numeric_run(ctx, token, scalar)? else {
                    return Ok(None);
                };
                ctx.reserve_vec(&mut runs, 1, "creo legacy continuation numeric runs")?;
                runs.push(run);
            }
            let Some(comma) = comma else {
                break;
            };
            tokens = &tokens[comma + 1..];
        }
        let Some(newline) = newline else {
            break;
        };
        remaining = &remaining[newline + 1..];
    }
    Ok(Some(runs))
}

#[cfg(test)]
pub(crate) fn object_node_id(offset: usize) -> String {
    format!("creo:legacy_ascii:object#{offset}")
}

pub(crate) struct SerializedOffsetId {
    pub(crate) namespace: &'static str,
    pub(crate) kind: &'static str,
    pub(crate) offset: usize,
}

impl std::fmt::Display for SerializedOffsetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "creo:{}:{}#{}", self.namespace, self.kind, self.offset)
    }
}

impl Serialize for SerializedOffsetId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

pub(crate) fn serialized_object_node_id(offset: usize) -> SerializedOffsetId {
    SerializedOffsetId {
        namespace: "legacy_ascii",
        kind: "object",
        offset,
    }
}

pub(crate) fn checked_object_node_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    offset: usize,
    operation: &'static str,
) -> Result<String, CodecError> {
    const PREFIX: &str = "creo:legacy_ascii:object#";
    let mut remaining = offset;
    let mut digits = 1;
    while remaining >= 10 {
        remaining /= 10;
        digits += 1;
    }
    let mut id = String::new();
    ctx.try_reserve_retained_text(&mut id, PREFIX.len() + digits, operation)?;
    id.push_str(PREFIX);
    std::fmt::Write::write_fmt(&mut id, format_args!("{offset}"))
        .map_err(|_| CodecError::Malformed(String::new()))?;
    Ok(id)
}

impl Scope {
    fn declaration(&self, id: u32) -> Option<&AttributeDeclaration> {
        self.declaration_positions
            .get(&id)
            .and_then(|position| self.declarations.get(*position))
    }
}

fn parent_object_offsets(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scopes: &[Scope],
) -> Result<HashMap<usize, usize>, CodecError> {
    let mut parents = HashMap::new();
    for scope in ctx.admit_iter(scopes, "creo legacy scope traversal")? {
        let mut active_storage =
            ctx.reserve_scoped(0, "creo legacy active object storage")?;
        let mut active_objects = Vec::<(u32, usize)>::new();
        for value in ctx.admit_iter(&scope.values, "creo legacy value traversal")? {
            while active_objects
                .last()
                .is_some_and(|(depth, _)| *depth >= value.depth)
            {
                let mut expired = std::iter::from_fn(|| active_objects.pop());
                let _ = ctx.next_charged(&mut expired, "creo legacy active object pruning")?;
            }
            let parent = active_objects
                .last()
                .filter(|(depth, _)| value.depth.checked_sub(1) == Some(*depth))
                .map(|(_, offset)| offset);
            if let Some(parent) = parent {
                ctx.insert_hash_map(
                    &mut parents,
                    value.offset,
                    *parent,
                    "creo legacy parent offset nodes",
                )?;
            }
            if scope
                .declaration(value.attribute_id)
                .is_some_and(|declaration| matches!(declaration.type_code, LegacyTypeCode::Object))
            {
                active_storage.with_storage(|| {
                    ctx.reserve_vec(&mut active_objects, 1, "creo legacy active object nodes")
                })?;
                active_objects.push((value.depth, value.offset));
            }
        }
    }
    Ok(parents)
}

fn object_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    scopes: &[Scope],
    parents: &HashMap<usize, usize>,
) -> Result<(Vec<ObjectRecord>, usize, usize), CodecError> {
    let mut records = Vec::new();
    let mut incomplete_arrays = 0usize;
    let mut unresolved = 0usize;
    for scope in ctx.admit_iter(scopes, "creo legacy scope traversal")? {
        let mut projection_storage =
            ctx.reserve_scoped(0, "creo legacy projection index storage")?;
        let mut value_attributes = HashMap::new();
        for value in ctx.admit_iter(&scope.values, "creo legacy value traversal")? {
            projection_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut value_attributes,
                    value.offset,
                    value.attribute_id,
                    "creo legacy object value attribute nodes",
                )
            })?;
        }
        let mut direct_array_elements = HashMap::<usize, Vec<usize>>::new();
        for child in ctx.admit_iter(&scope.values, "creo legacy object child traversal")? {
            let Some(parent_offset) = parents.get(&child.offset).copied() else {
                continue;
            };
            if value_attributes.get(&parent_offset) == Some(&child.attribute_id)
                && scope
                    .declaration(child.attribute_id)
                    .is_some_and(|declaration| {
                        matches!(declaration.type_code, LegacyTypeCode::Object)
                    })
            {
                projection_storage.with_storage(|| {
                    match ctx.entry_hash_map(
                        &mut direct_array_elements,
                        parent_offset,
                        "creo legacy object array index nodes",
                    )? {
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            let mut elements = Vec::new();
                            ctx.reserve_vec(
                                &mut elements,
                                1,
                                "creo legacy object array index rows",
                            )?;
                            elements.push(child.offset);
                            entry.insert(elements);
                        }
                        std::collections::hash_map::Entry::Occupied(mut entry) => {
                            let elements = entry.get_mut();
                            ctx.reserve_vec(elements, 1, "creo legacy object array index rows")?;
                            elements.push(child.offset);
                        }
                    }
                    Ok::<(), CodecError>(())
                })?;
            }
        }
        for value in ctx.admit_iter(&scope.values, "creo legacy value traversal")? {
            let Some(declaration) = scope
                .declaration(value.attribute_id)
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
            } else if let Some(dimensions) = array_dimensions(ctx, bytes)? {
                let offsets = direct_array_elements
                    .get(&value.offset)
                    .map_or(&[][..], Vec::as_slice);
                let mut elements = Vec::new();
                ctx.reserve_vec(
                    &mut elements,
                    offsets.len(),
                    "creo legacy object array elements",
                )?;
                for offset in ctx.admit_iter(offsets, "creo legacy object element traversal")? {
                    elements.push(checked_object_node_id(
                        ctx,
                        *offset,
                        "creo legacy object array element IDs",
                    )?);
                }
                let complete = object_array_is_complete(ctx, &dimensions, &elements)?;
                incomplete_arrays += usize::from(!complete);
                let payload = ObjectPayload::Array {
                    dimensions,
                    elements,
                    complete,
                };
                payload
            } else {
                unresolved += 1;
                ObjectPayload::Opaque {
                    bytes: ctx.copy_retained(bytes, "creo legacy opaque object bytes")?,
                }
            };
            let name =
                ctx.copy_retained_text(&declaration.name, "creo legacy object record names")?;
            ctx.reserve_vec(&mut records, 1, "creo legacy object records")?;
            records.push(ObjectRecord {
                name,
                attribute_id: value.attribute_id,
                scope_offset: scope.range.start,
                parent: parents.get(&value.offset).copied(),
                depth: value.depth,
                payload,
                offset: value.offset,
            });
        }
    }
    Ok((records, incomplete_arrays, unresolved))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NullToken {
    RepresentsNull,
    RepresentsBytes,
}

fn byte_string_value(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    null_token: NullToken,
) -> Result<StringValue, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if null_token == NullToken::RepresentsNull && bytes == b"NULL" {
        Ok(StringValue::Null)
    } else if let Ok(text) = ctx.validate_utf8(bytes, "creo UTF-8 validation")? {
        Ok(StringValue::Utf8 {
            text: ctx.copy_retained_text(text, "creo legacy string UTF-8 payload")?,
        })
    } else {
        Ok(StringValue::Bytes {
            bytes: ctx.copy_retained(bytes, "creo legacy string byte payload")?,
        })
    }
}

fn string_value(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
) -> Result<StringValue, CodecError> {
    byte_string_value(ctx, bytes, NullToken::RepresentsNull)
}

fn scalar_string_records<K: LegacyCode<Payload = StringValue>>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    scopes: &[Scope],
    identity_kind: ValueKind<K>,
    null_token: NullToken,
    parents: &HashMap<usize, usize>,
) -> Result<TypedValues<ValueRecord<K>>, CodecError> {
    let mut records = Vec::new();
    let mut unresolved = 0usize;
    for scope in ctx.admit_iter(scopes, "creo legacy scope traversal")? {
        for value in ctx.admit_iter(&scope.values, "creo legacy value traversal")? {
            let Some(declaration) = scope
                .declaration(value.attribute_id)
                .filter(|declaration| declaration.type_code == declaration_code(identity_kind))
            else {
                continue;
            };
            if value.continuation.is_some() {
                unresolved += 1;
                continue;
            }
            let Some(bytes) = data.get(value.payload.clone()) else {
                unresolved += 1;
                continue;
            };
            let payload = byte_string_value(ctx, bytes, null_token)?;
            let name =
                ctx.copy_retained_text(&declaration.name, "creo legacy scalar string names")?;
            ctx.reserve_vec(&mut records, 1, "creo legacy scalar string records")?;
            records.push(ValueRecord {
                name,
                attribute_id: value.attribute_id,
                scope_offset: scope.range.start,
                parent: parents.get(&value.offset).copied(),
                depth: value.depth,
                payload,
                offset: value.offset,
            });
        }
    }
    Ok(TypedValues {
        rows: records,
        unresolved_count: unresolved,
    })
}

fn string_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    scopes: &[Scope],
    parents: &HashMap<usize, usize>,
) -> Result<(Vec<StringRecord>, usize, usize), CodecError> {
    let mut records = Vec::new();
    let mut incomplete_arrays = 0usize;
    let mut unresolved = 0usize;
    for scope in ctx.admit_iter(scopes, "creo legacy scope traversal")? {
        let mut active_storage =
            ctx.reserve_scoped(0, "creo legacy active string array storage")?;
        let mut active_arrays = Vec::<(u32, usize, u32)>::new();
        let mut projection_storage =
            ctx.reserve_scoped(0, "creo legacy projection index storage")?;
        let mut array_children = HashMap::<usize, Vec<&AttributeValue>>::new();
        let mut array_element_offsets = HashSet::new();
        for value in ctx.admit_iter(&scope.values, "creo legacy value traversal")? {
            while active_arrays
                .last()
                .is_some_and(|(depth, _, _)| *depth >= value.depth)
            {
                let mut expired = std::iter::from_fn(|| active_arrays.pop());
                let _ = ctx.next_charged(&mut expired, "creo legacy active string array pruning")?;
            }
            let array_parent = active_arrays
                .last()
                .filter(|(depth, _, attribute_id)| {
                    value.depth.checked_sub(1) == Some(*depth)
                        && *attribute_id == value.attribute_id
                })
                .map(|(_, offset, _)| *offset);
            if let Some(parent_offset) = array_parent {
                projection_storage.with_storage(|| {
                    match ctx.entry_hash_map(
                        &mut array_children,
                        parent_offset,
                        "creo legacy string array child nodes",
                    )? {
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            let mut children = Vec::new();
                            ctx.reserve_vec(
                                &mut children,
                                1,
                                "creo legacy string array child rows",
                            )?;
                            children.push(value);
                            entry.insert(children);
                        }
                        std::collections::hash_map::Entry::Occupied(mut entry) => {
                            let children = entry.get_mut();
                            ctx.reserve_vec(children, 1, "creo legacy string array child rows")?;
                            children.push(value);
                        }
                    }
                    Ok::<(), CodecError>(())
                })?;
                projection_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut array_element_offsets,
                        value.offset,
                        "creo legacy string array element offsets",
                    )
                })?;
                continue;
            }
            if scope
                .declaration(value.attribute_id)
                .is_some_and(|declaration| matches!(declaration.type_code, LegacyTypeCode::String))
            {
                let (dimensions, dimension_storage) = ctx
                    .with_scoped_storage("creo legacy string array recognition storage", || {
                        array_dimensions(ctx, &data[value.payload.clone()])
                    })?;
                let is_array = dimensions.is_some();
                drop((dimensions, dimension_storage));
                if is_array {
                    active_storage.with_storage(|| {
                        ctx.reserve_vec(&mut active_arrays, 1, "creo legacy active string arrays")
                    })?;
                    active_arrays.push((value.depth, value.offset, value.attribute_id));
                }
            }
        }

        for value in ctx.admit_iter(&scope.values, "creo legacy value traversal")? {
            if array_element_offsets.contains(&value.offset) {
                continue;
            }
            let Some(declaration) = scope
                .declaration(value.attribute_id)
                .filter(|declaration| matches!(declaration.type_code, LegacyTypeCode::String))
            else {
                continue;
            };
            let bytes = &data[value.payload.clone()];
            let payload = if let Some(dimensions) = array_dimensions(ctx, bytes)? {
                let children = array_children
                    .get(&value.offset)
                    .map_or(&[][..], Vec::as_slice);
                let mut values = Vec::new();
                ctx.reserve_vec(
                    &mut values,
                    children.len(),
                    "creo legacy string array values",
                )?;
                let mut accepted_value_indices = Vec::new();
                let mut all_supported = true;
                for child in ctx.admit_iter(children, "creo legacy string child traversal")? {
                    if let Some(continuation) = &child.continuation {
                        all_supported = false;
                        unresolved += 1;
                        values.push(Err(continuation.clone()));
                    } else {
                        let value = string_value(ctx, &data[child.payload.clone()])?;
                        ctx.reserve_vec(
                            &mut accepted_value_indices,
                            1,
                            "creo legacy string array serializer indices",
                        )?;
                        accepted_value_indices.push(values.len());
                        values.push(Ok(value));
                    }
                }
                let complete = value.continuation.is_none()
                    && dimensions
                        .first()
                        .and_then(|dimension| usize::try_from(*dimension).ok())
                        .is_some_and(|count| count == values.len())
                    && all_supported;
                unresolved += usize::from(value.continuation.is_some());
                let payload = StringPayload::Array {
                    dimensions,
                    values,
                    #[cfg(test)]
                    continuation: value.continuation.clone(),
                    complete,
                    accepted_value_indices,
                };
                incomplete_arrays += usize::from(!complete);
                payload
            } else {
                if value.continuation.is_some() {
                    unresolved += 1;
                    continue;
                }
                StringPayload::Scalar {
                    value: string_value(ctx, bytes)?,
                }
            };
            let name =
                ctx.copy_retained_text(&declaration.name, "creo legacy string record names")?;
            ctx.reserve_vec(&mut records, 1, "creo legacy string records")?;
            records.push(ValueRecord {
                name,
                attribute_id: value.attribute_id,
                scope_offset: scope.range.start,
                parent: parents.get(&value.offset).copied(),
                depth: value.depth,
                payload,
                offset: value.offset,
            });
        }
    }
    Ok((records, incomplete_arrays, unresolved))
}

fn numeric_records<K, T>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    scopes: &[Scope],
    identity_kind: ValueKind<K>,
    scalar: fn(&cadmpeg_core::decode::DecodeContext<'_>, &[u8]) -> Result<Option<T>, CodecError>,
    parents: &HashMap<usize, usize>,
) -> Result<TypedValues<ValueRecord<K>>, CodecError>
where
    K: LegacyCode<Payload = NumericPayload<T>>,
{
    let mut records = Vec::new();
    let mut unresolved = 0usize;
    for scope in ctx.admit_iter(scopes, "creo legacy scope traversal")? {
        let mut pending = scope.values.iter().enumerate();
        while pending.len() != 0 {
            let Some((index, value)) =
                ctx.next_charged(&mut pending, "creo legacy numeric value traversal")?
            else {
                break;
            };
            let Some(declaration) = scope
                .declaration(value.attribute_id)
                .filter(|declaration| declaration.type_code == declaration_code(identity_kind))
            else {
                continue;
            };
            let Some(payload_bytes) = data.get(value.payload.clone()) else {
                unresolved += 1;
                continue;
            };
            let payload = if let Some(dimensions) = array_dimensions(ctx, payload_bytes)? {
                let mut next_index = index + 1;
                let runs = if let Some(continuation) = &value.continuation {
                    let Some(bytes) = data.get(continuation.rows.clone()) else {
                        unresolved += 1;
                        continue;
                    };
                    let Some(runs) = continuation_numeric_runs(ctx, bytes, scalar)? else {
                        unresolved += 1;
                        continue;
                    };
                    runs
                } else if dimensions.as_slice() == [1] {
                    let mut runs = Vec::new();
                    while pending.len() != 0 {
                        let mut probe = pending.clone();
                        let Some((position, child)) =
                            ctx.next_charged(&mut probe, "creo legacy numeric child traversal")?
                        else {
                            break;
                        };
                        if child.attribute_id != value.attribute_id
                            || value.depth.checked_add(1) != Some(child.depth)
                        {
                            break;
                        }
                        let Some(bytes) = data.get(child.payload.clone()) else {
                            break;
                        };
                        let run = if child.continuation.is_none() {
                            numeric_run(ctx, bytes, scalar)?
                        } else {
                            None
                        };
                        let Some(run) = run else {
                            break;
                        };
                        ctx.reserve_vec(&mut runs, 1, "creo legacy numeric child runs")?;
                        runs.push(run);
                        next_index = position + 1;
                        pending = probe;
                    }
                    runs
                } else {
                    Vec::new()
                };
                let Some(payload) = NumericPayload::array(ctx, dimensions, runs)? else {
                    unresolved += next_index - index;
                    continue;
                };
                payload
            } else {
                let scalar_value = if value.continuation.is_none() {
                    scalar(ctx, payload_bytes)?
                } else {
                    None
                };
                let Some(scalar_value) = scalar_value else {
                    unresolved += 1;
                    continue;
                };
                NumericPayload::Scalar {
                    value: scalar_value,
                }
            };
            let name =
                ctx.copy_retained_text(&declaration.name, "creo legacy numeric record names")?;
            ctx.reserve_vec(&mut records, 1, "creo legacy numeric records")?;
            records.push(ValueRecord {
                name,
                attribute_id: value.attribute_id,
                scope_offset: scope.range.start,
                parent: parents.get(&value.offset).copied(),
                depth: value.depth,
                payload,
                offset: value.offset,
            });
        }
    }
    Ok(TypedValues {
        rows: records,
        unresolved_count: unresolved,
    })
}

fn value(
    ctx: &DecodeContext<'_>,
    line: &[u8],
    line_offset: usize,
) -> Result<Option<AttributeValue>, CodecError> {
    let Some((depth, after_depth)) = decimal(ctx, line, 0)? else {
        return Ok(None);
    };
    if line.get(after_depth) != Some(&b' ') {
        return Ok(None);
    }
    let Some((attribute_id, after_attribute)) = decimal(ctx, line, after_depth + 1)? else {
        return Ok(None);
    };
    let payload_start = if after_attribute == line.len() {
        after_attribute
    } else {
        if line.get(after_attribute) != Some(&b' ') {
            return Ok(None);
        }
        after_attribute + 1
    };
    Ok(Some(AttributeValue {
        depth,
        attribute_id,
        offset: line_offset,
        payload: line_offset + payload_start..line_offset + line.len(),
        continuation: None,
    }))
}

fn scan_scope(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    range: Range<usize>,
) -> Result<Scope, CodecError> {
    let mut declarations = Vec::<AttributeDeclaration>::new();
    let mut declaration_indices = HashMap::<u32, usize>::new();
    let mut conflict_storage = ctx.reserve_scoped(0, "creo legacy declaration conflicts")?;
    let mut conflicting_ids = HashSet::<u32>::new();
    let mut candidates = Vec::<AttributeValue>::new();
    let mut continuation_owner = None::<usize>;
    let mut next = range.start;

    while next < range.end {
        let line_offset = next;
        let Some((current, after_line)) = line(ctx, &data[..range.end], next)? else {
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
                        continuation.count =
                            continuation.count.checked_add(1).ok_or_else(|| {
                                CodecError::malformed(
                                    "creo legacy continuation count exceeds usize",
                                )
                            })?;
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

        if let Some((id, name, type_code)) = parse_declaration(ctx, current)? {
            if let Some(index) = declaration_indices.get(&id).copied() {
                let previous = &declarations[index];
                if !conflicting_ids.contains(&id)
                    && (previous.type_code != type_code
                        || !ctx.equal(
                            previous.name.as_str(),
                            name,
                            "creo legacy declaration name equality",
                        )?)
                {
                    conflict_storage.with_storage(|| {
                        ctx.insert_hash_set(
                            &mut conflicting_ids,
                            id,
                            "creo legacy conflicting declaration IDs",
                        )
                    })?;
                }
            } else {
                let name = ctx.copy_retained_text(name, "creo legacy declaration names")?;
                ctx.reserve_vec(&mut declarations, 1, "creo legacy declarations")?;
                ctx.insert_hash_map(
                    &mut declaration_indices,
                    id,
                    declarations.len(),
                    "creo legacy declaration index nodes",
                )?;
                declarations.push(AttributeDeclaration {
                    id,
                    name,
                    type_code,
                    offset: line_offset,
                });
            }
            continue;
        }
        if let Some(value) = value(ctx, current, line_offset)? {
            continuation_owner = Some(candidates.len());
            ctx.reserve_vec(&mut candidates, 1, "creo legacy scope value candidates")?;
            candidates.push(value);
        }
    }

    let candidate_count = candidates.len();
    ctx.retain_vec(
        &mut candidates,
        |value| {
            Ok({
                declaration_indices.contains_key(&value.attribute_id)
                    && !conflicting_ids.contains(&value.attribute_id)
            })
        },
        "creo legacy candidate retain",
    )?;
    Ok(Scope {
        range,
        declarations,
        declaration_positions: declaration_indices,
        unresolved_value_count: candidate_count - candidates.len(),
        values: candidates,
        conflicting_declaration_count: conflicting_ids.len(),
    })
}

/// Scan independently scoped legacy ASCII record extents.
pub(crate) fn scan<I>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    ranges: I,
) -> Result<Persistence, CodecError>
where
    I: IntoIterator<Item = Range<usize>>,
    I::IntoIter: ExactSizeIterator,
{
    let mut scope_storage = ctx.reserve_scoped(0, "creo legacy scope storage")?;
    let mut scopes = Vec::new();
    let mut ranges = ranges.into_iter();
    while ranges.len() != 0 {
        let Some(range) = ctx.next_charged(&mut ranges, "creo legacy scope extent traversal")?
        else {
            break;
        };
        if range.start < range.end && range.start < data.len() {
            if range.end > data.len() {
                return Err(CodecError::malformed(ctx.format_retained(
                    format_args!(
                        "creo legacy persistence scope at offset {} declares end {}, past the file length {}",
                        range.start,
                        range.end,
                        data.len(),
                    ),
                    "creo legacy scope bounds error",
                )?));
            }
            scope_storage.with_storage(|| {
                ctx.reserve_vec(&mut scopes, 1, "creo legacy parsed scopes")?;
                scopes.push(scan_scope(ctx, data, range)?);
                Ok::<(), CodecError>(())
            })?;
        }
    }
    let mut counts = PersistenceCounts {
        scopes: scopes.len(),
        ..PersistenceCounts::default()
    };
    for scope in ctx.admit_iter(&scopes, "creo legacy persistence scope counts")? {
        counts.declarations += scope.declarations.len();
        counts.values += scope.values.len();
        counts.unresolved_values += scope.unresolved_value_count;
        counts.conflicting_declarations += scope.conflicting_declaration_count;
        for value in ctx.admit_iter(&scope.values, "creo legacy persistence continuation counts")? {
            counts.continuations = counts
                .continuations
                .checked_add(
                    value
                        .continuation
                        .as_ref()
                        .map_or(0, |continuation| continuation.count.get()),
                )
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("creo legacy continuation count", u64::MAX, u64::MAX)
                })?;
        }
    }
    let mut parent_storage = ctx.reserve_scoped(0, "creo legacy parent offset storage")?;
    let parents = parent_storage.with_storage(|| parent_object_offsets(ctx, &scopes))?;
    let (objects, incomplete_object_array_count, unresolved_object_value_count) =
        object_records(ctx, data, &scopes, &parents)?;
    let (string_values, incomplete_string_array_count, unresolved_string_value_count) =
        string_records(ctx, data, &scopes, &parents)?;
    let type_3_values = scalar_string_records(
        ctx,
        data,
        &scopes,
        ValueKind::TYPE3,
        NullToken::RepresentsNull,
        &parents,
    )?;
    let type_4_values = scalar_string_records(
        ctx,
        data,
        &scopes,
        ValueKind::TYPE4,
        NullToken::RepresentsBytes,
        &parents,
    )?;
    let real_values = numeric_records(
        ctx,
        data,
        &scopes,
        ValueKind::REAL,
        |_, bytes| Ok(compact_real(bytes)),
        &parents,
    )?;
    let integer_values = numeric_records(
        ctx,
        data,
        &scopes,
        ValueKind::INTEGER,
        signed_integer,
        &parents,
    )?;
    let type_5_values = numeric_records(
        ctx,
        data,
        &scopes,
        ValueKind::TYPE5,
        unsigned_integer,
        &parents,
    )?;
    let type_6_values = numeric_records(
        ctx,
        data,
        &scopes,
        ValueKind::TYPE6,
        |_, bytes| Ok(compact_real(bytes)),
        &parents,
    )?;
    let type_7_values = numeric_records(
        ctx,
        data,
        &scopes,
        ValueKind::TYPE7,
        unsigned_integer,
        &parents,
    )?;
    let type_9_values = numeric_records(
        ctx,
        data,
        &scopes,
        ValueKind::TYPE9,
        unsigned_integer,
        &parents,
    )?;
    let type_11_values = numeric_records(
        ctx,
        data,
        &scopes,
        ValueKind::TYPE11,
        unsigned_integer,
        &parents,
    )?;
    Ok(Persistence {
        counts,
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
    })
}

impl<K: LegacyCode> ValueRecord<K> {
    /// Native identity derived from the source offset.
    pub(crate) fn id(&self) -> SerializedOffsetId {
        SerializedOffsetId {
            namespace: "legacy_ascii",
            kind: K::CODE.identity_token(),
            offset: self.offset,
        }
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
        wire.serialize_field("parent", &self.parent.map(serialized_object_node_id))?;
        wire.serialize_field("depth", &self.depth)?;
        wire.serialize_field("payload", &self.payload)?;
        wire.serialize_field("offset", &self.offset)?;
        wire.end()
    }
}

impl ObjectRecord {
    /// Native identity derived from the source offset.
    pub(crate) fn id(&self) -> SerializedOffsetId {
        serialized_object_node_id(self.offset)
    }
}

impl Serialize for ObjectRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_struct("ObjectRecord", 8)?;
        wire.serialize_field("id", &self.id())?;
        wire.serialize_field("name", &self.name)?;
        wire.serialize_field("attribute_id", &self.attribute_id)?;
        wire.serialize_field("scope_offset", &self.scope_offset)?;
        wire.serialize_field("parent", &self.parent.map(serialized_object_node_id))?;
        wire.serialize_field("depth", &self.depth)?;
        wire.serialize_field("payload", &self.payload)?;
        wire.serialize_field("offset", &self.offset)?;
        wire.end()
    }
}

/// A legacy declaration code carried at the type level.
pub(crate) trait LegacyCode: Copy + Eq + std::fmt::Debug {
    /// The declaration code whose value rows carry this identity.
    const CODE: LegacyTypeCode;
    /// The payload shape stored by a value row of this code.
    type Payload;
}

macro_rules! legacy_code {
    ($(#[$doc:meta])* $name:ident, $code:ident, $payload:ty) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(crate) struct $name;

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
struct ValueKind<K>(std::marker::PhantomData<fn() -> K>);

impl<K> Copy for ValueKind<K> {}

impl<K> Clone for ValueKind<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K: LegacyCode> ValueKind<K> {
    const KIND: Self = Self(std::marker::PhantomData);
}

/// The declaration code named by an identity marker.
fn declaration_code<K: LegacyCode>(_kind: ValueKind<K>) -> LegacyTypeCode {
    K::CODE
}

impl ValueKind<IntegerCode> {
    /// Identity token for integer values.
    const INTEGER: Self = Self::KIND;
}

impl ValueKind<RealCode> {
    /// Identity token for real values.
    const REAL: Self = Self::KIND;
}

impl ValueKind<Type6Code> {
    /// Identity token for `type_6` values.
    const TYPE6: Self = Self::KIND;
}

impl ValueKind<Type5Code> {
    /// Identity token for `type_5` values.
    const TYPE5: Self = Self::KIND;
}

impl ValueKind<Type7Code> {
    /// Identity token for `type_7` values.
    const TYPE7: Self = Self::KIND;
}

impl ValueKind<Type9Code> {
    /// Identity token for `type_9` values.
    const TYPE9: Self = Self::KIND;
}

impl ValueKind<Type11Code> {
    /// Identity token for `type_11` values.
    const TYPE11: Self = Self::KIND;
}

impl ValueKind<Type3Code> {
    /// Identity token for `type_3` values.
    const TYPE3: Self = Self::KIND;
}

impl ValueKind<Type4Code> {
    /// Identity token for `type_4` values.
    const TYPE4: Self = Self::KIND;
}

#[cfg(test)]
mod tests;
