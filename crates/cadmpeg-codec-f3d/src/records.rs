// SPDX-License-Identifier: Apache-2.0
#![deny(clippy::disallowed_methods)]
//! Fusion parametric-design records and links to the solved B-rep.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::num::NonZeroU32;

/// A source value and the byte offset of its encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Located<T, O = u64> {
    pub value: T,
    pub offset: O,
}

impl<T, O> Located<T, O> {
    fn from_wire(value: Option<T>, offset: Option<O>, field: &str) -> Result<Option<Self>, String> {
        match (value, offset) {
            (None, None) => Ok(None),
            (Some(value), Some(offset)) => Ok(Some(Self { value, offset })),
            _ => Err(format!("{field} and {field}_offset must occur together")),
        }
    }
}

/// An ordered run whose encoding locations are either complete or absent.
#[derive(Debug, Clone)]
pub enum ReferenceRun<T, O = u64> {
    Unlocated(Vec<T>),
    Located(Vec<Located<T, O>>),
}

impl<T: PartialEq, O: PartialEq> PartialEq for ReferenceRun<T, O> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Unlocated(left), Self::Unlocated(right)) => left == right,
            (Self::Located(left), Self::Located(right)) => left == right,
            // An empty run has no locations in either wire form.
            _ => self.is_empty() && other.is_empty(),
        }
    }
}

impl<T: Eq, O: Eq> Eq for ReferenceRun<T, O> {}

impl<T, O> ReferenceRun<T, O> {
    pub fn values(&self) -> impl DoubleEndedIterator<Item = &T> {
        let (unlocated, located): (&[T], &[Located<T, O>]) = match self {
            Self::Unlocated(values) => (values, &[]),
            Self::Located(values) => (&[], values),
        };
        unlocated.iter().chain(located.iter().map(|row| &row.value))
    }

    pub fn offsets(&self) -> impl Iterator<Item = &O> {
        let rows: &[Located<T, O>] = match self {
            Self::Unlocated(_) => &[],
            Self::Located(rows) => rows,
        };
        rows.iter().map(|row| &row.offset)
    }

    #[cfg(test)]
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        let (unlocated, located): (&mut [T], &mut [Located<T, O>]) = match self {
            Self::Unlocated(values) => (values, &mut []),
            Self::Located(values) => (&mut [], values),
        };
        unlocated.iter_mut().chain(located.iter_mut().map(|row| &mut row.value))
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Unlocated(values) => values.len(),
            Self::Located(values) => values.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::Unlocated(values) => values.is_empty(),
            Self::Located(values) => values.is_empty(),
        }
    }

    fn from_wire(values: Vec<T>, offsets: Vec<O>, field: &str) -> Result<Self, String> {
        if offsets.is_empty() {
            return Ok(Self::Unlocated(values));
        }
        if values.len() != offsets.len() {
            return Err(format!("{field} offsets must be absent or match every value"));
        }
        Ok(Self::Located(values.into_iter().zip(offsets).map(|(value, offset)| Located { value, offset }).collect()))
    }

    fn into_wire(self) -> (Vec<T>, Vec<O>) {
        match self {
            Self::Unlocated(values) => (values, Vec::new()),
            Self::Located(values) => values.into_iter().map(|row| (row.value, row.offset)).unzip(),
        }
    }
}

/// A non-empty half-open interval of source bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NonEmptyByteSpan {
    start: u64,
    end: u64,
}

impl NonEmptyByteSpan {
    pub fn new(start: u64, end: u64) -> Option<Self> {
        if start >= end {
            return None;
        }
        Some(Self { start, end })
    }

    pub fn start(&self) -> u64 {
        self.start
    }

    pub fn end(&self) -> u64 {
        self.end
    }

    pub fn byte_len(&self) -> u64 {
        self.end - self.start
    }
}

/// A value with an optional source encoding location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RecordedValue<T> {
    pub value: T,
    pub offset: Option<u64>,
}

impl<T> RecordedValue<T> {
    fn from_wire(value: Option<T>, offset: Option<u64>, field: &str) -> Result<Option<Self>, String> {
        match (value, offset) {
            (Some(value), offset) => Ok(Some(Self { value, offset })),
            (None, None) => Ok(None),
            (None, Some(_)) => Err(format!("{field}_offset requires {field}")),
        }
    }
}

fn serialize_absent_u64_offset<S: Serializer>(
    value: &Option<u64>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_u64(value.unwrap_or(0))
}

fn deserialize_absent_u64_offset<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    let offset = u64::deserialize(deserializer)?;
    Ok((offset != 0).then_some(offset))
}

use cadmpeg_ir::assets::AssetId;
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::ids::{BodyId, EdgeId, FaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::Color;

/// The `sketch_attrib_def` sense value that constrains nothing, written as the
/// unsigned decimal spelling of `0xFFFFFFFF`.
pub(crate) const SKETCH_LINK_SENSE_UNCONSTRAINED: i64 = 0xFFFF_FFFF;

/// Whether a `sketch_attrib_def` sense leaves the sense unconstrained. The
/// tagged-field form spells the value as the unsigned decimal of `0xFFFFFFFF`
/// and the integer forms as the signed `-1` of that same 32-bit pattern, so a
/// reader that accepts one spelling keeps the other as a stored sense.
pub(crate) fn sketch_link_sense_is_unconstrained(sense: i64) -> bool {
    sense == SKETCH_LINK_SENSE_UNCONSTRAINED || sense == -1
}

/// Provenance link from a solved B-rep entity to its source sketch curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchCurveLink {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved B-rep entity this link provenances back to a sketch curve.
    pub target: AttributeTarget,
    /// Numeric design-entity id of the source sketch-curve record.
    pub sketch_curve_id: i64,
    /// Second member of the source tuple, retained in the spelling the source
    /// writes. It is `0` in most links; what a non-zero value names is open as
    /// `DR-30`.
    pub ref_b: u64,
    /// Which of the sketch curve's two senses this link takes, `0` or `1`.
    /// Absent when the source record leaves the sense unconstrained.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sense: Option<i64>,
    /// Source role tag distinguishing how the sketch curve participates in the link
    /// (e.g. profile edge vs. construction reference).
    pub role: i64,
    /// Source closure/continuity tag of the sketch curve at this link.
    pub closure: i64,
}

/// Persistent Fusion design identifier attached to a solved B-rep entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "PersistentDesignLinkWire"))]
#[serde(
    try_from = "PersistentDesignLinkWire",
    into = "PersistentDesignLinkWire"
)]
pub struct PersistentDesignLink {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved B-rep entity this persistent Fusion design id is attached to.
    pub target: AttributeTarget,
    /// Fusion persistent design-entity id string, stable across regeneration.
    pub design_id: String,
    /// Design-stream reference paired with this persistent identifier.
    pub design_reference: i64,
    /// Position of this id in the entity's persistent-id history, in assignment order.
    pub ordinal: u32,
    /// Whether this is the active persistent id for `target`, as opposed to a
    /// superseded historical id retained for provenance.
    pub is_current: bool,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct PersistentDesignLinkWire {
    /// Globally unique deterministic identifier for this native record.
    id: String,
    /// Solved B-rep entity this persistent Fusion design id is attached to.
    target: AttributeTarget,
    /// Fusion persistent design-entity id string, stable across regeneration.
    design_id: String,
    entity_kind: i64,
    /// Design-stream reference paired with this persistent identifier.
    design_reference: i64,
    /// Position of this id in the entity's persistent-id history, in assignment order.
    ordinal: u32,
    /// Whether this is the active persistent id for `target`, as opposed to a
    /// superseded historical id retained for provenance.
    is_current: bool,
}

impl TryFrom<PersistentDesignLinkWire> for PersistentDesignLink {
    type Error = String;

    fn try_from(wire: PersistentDesignLinkWire) -> Result<Self, Self::Error> {
        if wire.entity_kind != 3 {
            return Err("entity_kind must be 3".into());
        }
        Ok(Self {
            id: wire.id,
            target: wire.target,
            design_id: wire.design_id,
            design_reference: wire.design_reference,
            ordinal: wire.ordinal,
            is_current: wire.is_current,
        })
    }
}

impl From<PersistentDesignLink> for PersistentDesignLinkWire {
    fn from(record: PersistentDesignLink) -> Self {
        Self {
            id: record.id,
            target: record.target,
            design_id: record.design_id,
            entity_kind: 3,
            design_reference: record.design_reference,
            ordinal: record.ordinal,
            is_current: record.is_current,
        }
    }
}

/// Native face/edge tag group linking a solved subentity to design records.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PersistentSubentityTag {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved B-rep face or edge carrying this tag group.
    pub target: AttributeTarget,
    /// Native selector stored before the tag token.
    pub selector: i64,
    /// Native UTF-8 tag token. Numeric strings and `-1` retain their spelling.
    pub token: String,
    /// Ordered signed Design-stream references carried by this group.
    pub design_references: Vec<i64>,
    /// Position of this group in the owning attribute record.
    pub ordinal: u32,
}

/// Component-local Design naming space bound to a context UUID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignComponentNamingSpace {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the binding marker in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Component entity id named by the binding.
    pub component_record_index: u64,
    /// UUID used by persistent identities to select this component.
    pub context_uuid: String,
    /// Byte offset of the UUID length prefix.
    pub context_uuid_offset: u64,
}

/// Original authoring time attached to a solved ASM entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CreationTimestamp {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved B-rep entity carrying the timestamp attribute.
    pub target: AttributeTarget,
    /// Source SAB record index of the timestamp attribute.
    pub record_index: u32,
    /// Creation time as microseconds since the Unix epoch.
    pub unix_microseconds: f64,
}

/// Design `BulkStream` regeneration-recipe family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ConstructionRecipeKind {
    /// Recipe regenerates a whole body.
    Body,
    /// Recipe regenerates a single face.
    Face,
    /// Recipe regenerates a face bounded by an explicit region.
    BoundedFace,
    /// Recipe regenerates a single edge.
    Edge,
    /// Recipe regenerates a single vertex.
    Vertex,
}

/// One source-framed parametric regeneration recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "ConstructionRecipeWire", into = "ConstructionRecipeWire")]
pub struct ConstructionRecipe {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of this recipe's family marker in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of `record_index` in the Design `BulkStream`, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_index_offset: Option<u64>,
    /// Topology kind this recipe regenerates on replay.
    pub kind: ConstructionRecipeKind,
    /// Design entity id of the body this recipe is keyed to, if the source record
    /// carried a `generic_tag_attrib_def` construction id; `None` for body-less recipes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_id: Option<RecordedValue<String>>,
    /// Selector following the Design entity id, when the recipe carries that id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_selector: Option<ConstructionRecipeSelector>,
    /// Position of this recipe in the `BulkStream` recipe sequence, in source order.
    pub recipe_index: u32,
    /// Source `BulkStream` record index this recipe was decoded from.
    pub record_index: i32,
}

/// One source-framed parametric regeneration recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ConstructionRecipeWire {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of this recipe's family marker in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of `record_index` in the Design `BulkStream`, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_index_offset: Option<u64>,
    /// Topology kind this recipe regenerates on replay.
    pub kind: ConstructionRecipeKind,
    /// Design entity id of the body this recipe is keyed to, if the source record
    /// carried a `generic_tag_attrib_def` construction id; `None` for body-less recipes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_id: Option<String>,
    /// Byte offset of `design_id` in the Design `BulkStream`, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_id_offset: Option<u64>,
    /// Selector following the Design entity id, when the recipe carries that id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_selector: Option<ConstructionRecipeSelector>,
    /// Position of this recipe in the `BulkStream` recipe sequence, in source order.
    pub recipe_index: u32,
    /// Source `BulkStream` record index this recipe was decoded from.
    pub record_index: i32,
}

impl TryFrom<ConstructionRecipeWire> for ConstructionRecipe {
    type Error = String;
    fn try_from(wire: ConstructionRecipeWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            record_index_offset: wire.record_index_offset,
            kind: wire.kind,
            design_selector: wire.design_selector,
            recipe_index: wire.recipe_index,
            record_index: wire.record_index,
            design_id: RecordedValue::from_wire(wire.design_id, wire.design_id_offset, "design_id")?,
        })
    }
}

impl From<ConstructionRecipe> for ConstructionRecipeWire {
    fn from(value: ConstructionRecipe) -> Self {
        Self {
            id: value.id,
            byte_offset: value.byte_offset,
            record_index_offset: value.record_index_offset,
            kind: value.kind,
            design_selector: value.design_selector,
            recipe_index: value.recipe_index,
            record_index: value.record_index,
            design_id_offset: value.design_id.as_ref().and_then(|field| field.offset),
            design_id: value.design_id.map(|field| field.value),
        }
    }
}

/// Serialized Design selector carried by a construction recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ConstructionRecipeSelector {
    /// Selector value.
    pub value: u32,
    /// Byte offset of `value`.
    pub byte_offset: u64,
}

/// Semantic family of one Design parameter record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DesignParameterKind {
    /// A document-level named user parameter.
    User,
    /// A dimensional constraint parameter.
    Dimension,
    /// A parameter consumed by a construction feature.
    Feature,
}

/// Who owns a Design parameter: a user parameter, a dimension, or a feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignParameterOwnerKind {
    User,
    Dimension { owner_record_index: u32 },
    Feature { owner_record_index: u32 },
}

impl DesignParameterOwnerKind {
    pub(crate) fn kind(self) -> DesignParameterKind {
        match self {
            Self::User => DesignParameterKind::User,
            Self::Dimension { .. } => DesignParameterKind::Dimension,
            Self::Feature { .. } => DesignParameterKind::Feature,
        }
    }

    pub(crate) fn owner_record_index(self) -> Option<u32> {
        match self {
            Self::User => None,
            Self::Dimension { owner_record_index } | Self::Feature { owner_record_index } => {
                Some(owner_record_index)
            }
        }
    }

    pub(crate) fn from_kind(kind: DesignParameterKind, owner_record_index: Option<u32>) -> Self {
        match (kind, owner_record_index) {
            (DesignParameterKind::User, _) => Self::User,
            (DesignParameterKind::Dimension, Some(owner_record_index)) => {
                Self::Dimension { owner_record_index }
            }
            (DesignParameterKind::Feature, Some(owner_record_index)) => {
                Self::Feature { owner_record_index }
            }
            (DesignParameterKind::Dimension, None) => Self::Dimension {
                owner_record_index: 0,
            },
            (DesignParameterKind::Feature, None) => Self::Feature {
                owner_record_index: 0,
            },
        }
    }
}

/// One indexed Design parameter or expression record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "DesignParameterSerde"))]
#[serde(try_from = "DesignParameterSerde", into = "DesignParameterSerde")]
pub struct DesignParameter {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the indexed record header in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: String,
    /// Source indexed-record identity.
    pub record_index: u32,
    /// Parameter-family discriminator when the frame carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_discriminator: Option<Located<u64>>,
    /// Source ordering value stored by the parameter record.
    pub source_ordinal: u32,
    /// Indexed owner: user parameters have none; feature and dimension
    /// parameters name their owning record.
    pub owner: DesignParameterOwnerKind,
    /// Literal or symbolic source expression.
    pub expression: String,
    /// Byte offset of the expression's UTF-16LE code units.
    pub expression_offset: u64,
    /// Source family label such as `User Parameter`, `AlongDistance`, or
    /// `Linear Dimension-2`.
    pub source_kind: String,
    /// Byte offset of the source-family UTF-16LE code units.
    pub source_kind_offset: u64,
    /// Declared unit token; absent for dimensionless and Boolean parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<RecordedValue<String>>,
    /// Source parameter name or dimension identifier.
    pub name: String,
    /// Byte offset of the name's UTF-16LE code units.
    pub name_offset: u64,
    /// Evaluated scalar in the record's native unit convention.
    pub evaluated_value: f64,
    /// Byte offset of `evaluated_value`.
    pub evaluated_value_offset: u64,
}

impl DesignParameter {
    pub(crate) fn kind(&self) -> DesignParameterKind {
        self.owner.kind()
    }

    pub(crate) fn owner_record_index(&self) -> Option<u32> {
        self.owner.owner_record_index()
    }
}

fn design_parameter_kind_from_source(source_kind: &str) -> DesignParameterKind {
    if source_kind == "User Parameter" {
        DesignParameterKind::User
    } else if source_kind.contains("Dimension") {
        DesignParameterKind::Dimension
    } else {
        DesignParameterKind::Feature
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignParameterSerde {
    id: String,
    byte_offset: u64,
    class_tag: String,
    record_index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    family_discriminator: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    family_discriminator_offset: Option<u64>,
    source_ordinal: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner_record_index: Option<u32>,
    expression: String,
    expression_offset: u64,
    source_kind: String,
    source_kind_offset: u64,
    kind: DesignParameterKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unit_offset: Option<u64>,
    name: String,
    name_offset: u64,
    evaluated_value: f64,
    evaluated_value_offset: u64,
}

impl TryFrom<DesignParameterSerde> for DesignParameter {
    type Error = String;

    fn try_from(wire: DesignParameterSerde) -> Result<Self, Self::Error> {
        let derived = design_parameter_kind_from_source(&wire.source_kind);
        if wire.kind != derived {
            return Err("design parameter kind disagrees with source_kind".into());
        }
        let owner = match (wire.kind, wire.owner_record_index) {
            (DesignParameterKind::User, None) => DesignParameterOwnerKind::User,
            (DesignParameterKind::Dimension, Some(owner_record_index)) => {
                DesignParameterOwnerKind::Dimension { owner_record_index }
            }
            (DesignParameterKind::Feature, Some(owner_record_index)) => {
                DesignParameterOwnerKind::Feature { owner_record_index }
            }
            _ => {
                return Err("design parameter owner_record_index disagrees with kind".into());
            }
        };
        Ok(Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag,
            record_index: wire.record_index,
            family_discriminator: Located::from_wire(wire.family_discriminator, wire.family_discriminator_offset, "DesignParameter.family_discriminator")?,
            source_ordinal: wire.source_ordinal,
            owner,
            expression: wire.expression,
            expression_offset: wire.expression_offset,
            source_kind: wire.source_kind,
            source_kind_offset: wire.source_kind_offset,
            unit: RecordedValue::from_wire(wire.unit, wire.unit_offset, "unit")?,
            name: wire.name,
            name_offset: wire.name_offset,
            evaluated_value: wire.evaluated_value,
            evaluated_value_offset: wire.evaluated_value_offset,
        })
    }
}

impl From<DesignParameter> for DesignParameterSerde {
    fn from(parameter: DesignParameter) -> Self {
        let kind = parameter.kind();
        let owner_record_index = parameter.owner_record_index();
        Self {
            id: parameter.id,
            byte_offset: parameter.byte_offset,
            class_tag: parameter.class_tag,
            record_index: parameter.record_index,
            family_discriminator: parameter.family_discriminator.map(|value| value.value),
            family_discriminator_offset: parameter.family_discriminator.map(|value| value.offset),
            source_ordinal: parameter.source_ordinal,
            owner_record_index,
            expression: parameter.expression,
            expression_offset: parameter.expression_offset,
            source_kind: parameter.source_kind,
            source_kind_offset: parameter.source_kind_offset,
            kind,
            unit_offset: parameter.unit.as_ref().and_then(|field| field.offset),
            unit: parameter.unit.map(|field| field.value),
            name: parameter.name,
            name_offset: parameter.name_offset,
            evaluated_value: parameter.evaluated_value,
            evaluated_value_offset: parameter.evaluated_value_offset,
        }
    }
}

/// Indexed record that owns one Design parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignParameterOwner {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the indexed record header in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte length from the primary header to its same-index paired header.
    #[serde(default)]
    pub frame_length: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: String,
    /// Source indexed-record identity.
    pub record_index: u32,
    /// Feature or sketch record that scopes this parameter.
    pub scope_record_index: u32,
    /// Position among parameters in the same scope.
    pub local_ordinal: u32,
    /// Evaluated scalar duplicated from the parameter record.
    pub evaluated_value: f64,
    /// Byte offset of `evaluated_value`.
    pub evaluated_value_offset: u64,
    /// Indexed parameter record owned by this frame.
    pub parameter_record_index: u32,
    /// Native owner ordering value.
    pub owned_ordinal: u32,
    /// Source owner-frame variant flag when the frame carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<u8>,
    /// Paired indexed record following the parameter record.
    pub companion_record_index: u32,
}

/// Fixed prefix of the indexed record paired with a Design parameter owner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignParameterCompanion {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the indexed record header in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: String,
    /// Source indexed-record identity.
    pub record_index: u32,
    /// Indexed parameter-owner record referenced by this prefix.
    pub owner_record_index: u32,
    /// Nonzero Unix-epoch timestamp in microseconds.
    #[serde(alias = "opaque_value")]
    pub timestamp_micros: u64,
    /// Byte offset of `timestamp_micros`.
    #[serde(alias = "opaque_value_offset")]
    pub timestamp_micros_offset: u64,
    /// First byte owned after the fixed companion prefix.
    #[serde(default)]
    pub payload_byte_offset: u64,
    /// Number of bytes owned before the next sibling Design record.
    #[serde(default)]
    pub payload_byte_length: u64,
    /// Construction recipes contained by the owned payload, in byte order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owned_recipe_ids: Vec<String>,
}

/// Indexed record that directly contains one construction recipe owned by a
/// dimensional parameter companion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignDimensionRecipeRecord {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Companion record containing this indexed record.
    pub companion_record_index: u32,
    /// Deterministic decoder ordinal within the companion payload; it is not a
    /// source operation order.
    pub recipe_ordinal: u32,
    /// Construction recipe contained by this indexed record.
    pub recipe_id: String,
    /// Topology kind regenerated by the contained construction recipe.
    pub recipe_kind: ConstructionRecipeKind,
    /// Byte offset of the indexed record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: String,
    /// Source indexed-record identity.
    pub record_index: u32,
    /// Number of bytes from this header to the next indexed header or the end
    /// of the companion-owned payload.
    pub frame_length: u64,
    /// Byte offset of the recipe-specific prefix after the indexed header.
    pub prefix_offset: u64,
    /// Complete recipe-specific prefix before the length-prefixed family name.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub prefix_bytes: Vec<u8>,
    /// Persistent Design selector/reference tails decoded from the prefix.
    pub references: Vec<DesignRecipeReference>,
    /// Byte offset of the first i32 after the recipe-family name.
    pub program_offset: u64,
    /// Complete little-endian i32 program through the indexed-record boundary.
    pub program: Vec<i32>,
    /// Edge operands whose complete post-prologue recipe program occurs as a
    /// contiguous subsequence of this program.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matching_edge_operand_ids: Vec<String>,
}

/// One persistent Design selector/reference tail in a dimension recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignRecipeReference {
    /// Native persistent-subentity selector.
    pub selector: i64,
    /// Byte offset of `selector`.
    pub selector_offset: u64,
    /// ASCII persistent-subentity selector token.
    pub token: String,
    /// Byte offset of the token bytes.
    pub token_offset: u64,
    /// Persistent Design reference paired with `token`.
    pub design_reference: i64,
    /// Byte offset of `design_reference`.
    pub design_reference_offset: u64,
    /// Recipe-state faces carrying the token and Design reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_faces: Vec<FaceId>,
    /// Recipe-state edges carrying the token and Design reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_edges: Vec<EdgeId>,
    /// Active-BREP faces carrying the token and Design reference under a
    /// different native selector, before a historical state supersedes them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternate_selector_faces: Vec<FaceId>,
    /// Active-BREP edges carrying the token and Design reference under a
    /// different native selector, before a historical state supersedes them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternate_selector_edges: Vec<EdgeId>,
}

/// Paired-locus frame nested under a dimensional parameter companion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignDimensionLocusPair {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Companion record containing this frame.
    pub companion_record_index: u32,
    /// Companion record owned by the following dimension parameter governed by
    /// this frame.
    pub governing_companion_record_index: u32,
    /// Byte offset of the primary indexed record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: String,
    /// Shared logical record identity.
    pub record_index: u32,
    /// Byte length from the primary header to the paired header.
    pub frame_length: u64,
    /// Opaque u32 preceding the two locus references.
    pub opaque_index: u32,
    /// Byte offset of `opaque_index`.
    pub opaque_index_offset: u64,
    /// First typed sketch-geometry record.
    pub first_geometry_record_index: u32,
    /// Byte offset of the first geometry record index.
    pub first_geometry_reference_offset: u64,
    /// Source role code following the first geometry reference.
    pub first_role: u32,
    /// Byte offset of `first_role`.
    pub first_role_offset: u64,
    /// Second typed sketch-geometry record.
    pub second_geometry_record_index: u32,
    /// Byte offset of the second geometry record index.
    pub second_geometry_reference_offset: u64,
    /// Source role code following the second geometry reference.
    pub second_role: u32,
    /// Byte offset of `second_role`.
    pub second_role_offset: u64,
    /// Per-file dynamic class tag of the paired header.
    pub paired_class_tag: String,
    /// Byte offset of the paired indexed record header.
    pub paired_byte_offset: u64,
}

/// Dimension frame with one null locus and one typed sketch-geometry locus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignDimensionNullLocusPair {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Companion record containing this frame.
    pub companion_record_index: u32,
    /// Companion record owned by the following dimension parameter governed by
    /// this frame.
    pub governing_companion_record_index: u32,
    /// Byte offset of the primary indexed record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: String,
    /// Shared logical record identity.
    pub record_index: u32,
    /// Byte length from the primary header to the paired header.
    pub frame_length: u64,
    /// Byte offset of the fixed zero record reference.
    pub null_reference_offset: u64,
    /// Role code attached to the null record reference.
    pub null_role: u32,
    /// Byte offset of `null_role`.
    pub null_role_offset: u64,
    /// Typed sketch-geometry record.
    pub geometry_record_index: u32,
    /// Byte offset of `geometry_record_index`.
    pub geometry_reference_offset: u64,
    /// Role code attached to the typed geometry record.
    pub geometry_role: u32,
    /// Byte offset of `geometry_role`.
    pub geometry_role_offset: u64,
    /// Per-file dynamic class tag of the paired header.
    pub paired_class_tag: String,
    /// Byte offset of the paired indexed record header.
    pub paired_byte_offset: u64,
}

/// One nullable typed operand in an annotated dimension frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignDimensionAnnotationOperand {
    /// Indexed sketch geometry record, or zero for the null locus.
    pub geometry_record_index: u32,
    /// Byte offset of `geometry_record_index`.
    pub geometry_reference_offset: u64,
    /// Source dimension-role code.
    pub role: u32,
    /// Byte offset of `role`.
    pub role_offset: u64,
}

/// Paired `EntityGenesis` dimension frame carrying annotation geometry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "DesignDimensionAnnotationFrameWire"))]
#[serde(try_from = "DesignDimensionAnnotationFrameWire", into = "DesignDimensionAnnotationFrameWire")]
pub struct DesignDimensionAnnotationFrame {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Companion record containing this frame, absent before the first companion in a scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub companion_record_index: Option<u32>,
    /// Companion record of the dimension parameter governed by this frame.
    pub governing_companion_record_index: u32,
    /// Byte offset of the primary indexed record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: String,
    /// Source indexed-record identity.
    pub record_index: u32,
    /// Byte length from the primary through the paired header boundary.
    pub frame_length: u64,
    /// Ordered nullable locus operands.
    pub operands: Vec<DesignDimensionAnnotationOperand>,
    /// `EntityGenesis` origin bitfield.
    pub entity_genesis: u64,
    /// Opaque annotation bytes between the genesis block and governing owner.
    pub annotation_bytes: Vec<u8>,
    /// Byte offset of `annotation_bytes`.
    pub annotation_byte_offset: u64,
    /// Indexed parameter-owner record selecting the governed dimension.
    pub governing_owner_record_index: u32,
    /// Byte offset of `governing_owner_record_index`.
    pub governing_owner_reference_offset: u64,
    /// Ordered non-null return geometry records.
    pub return_members: Vec<Located<u32>>,
    /// Dynamic class tag of the paired indexed record.
    pub paired_class_tag: String,
    /// Byte offset of the paired indexed record header.
    pub paired_byte_offset: u64,
    /// Numeric design-entity suffix of the owning sketch.
    pub owner_reference: u32,
    /// Byte offset of `owner_reference`.
    pub owner_reference_offset: u64,
}

/// Paired `EntityGenesis` dimension frame carrying annotation geometry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignDimensionAnnotationFrameWire {
    /// Globally unique deterministic identifier for this native record.
    id: String,
    /// Companion record containing this frame, absent before the first companion in a scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    companion_record_index: Option<u32>,
    /// Companion record of the dimension parameter governed by this frame.
    governing_companion_record_index: u32,
    /// Byte offset of the primary indexed record header.
    byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    class_tag: String,
    /// Source indexed-record identity.
    record_index: u32,
    /// Byte length from the primary through the paired header boundary.
    frame_length: u64,
    /// Ordered nullable locus operands.
    operands: Vec<DesignDimensionAnnotationOperand>,
    /// `EntityGenesis` origin bitfield.
    entity_genesis: u64,
    /// Opaque annotation bytes between the genesis block and governing owner.
    annotation_bytes: Vec<u8>,
    /// Byte offset of `annotation_bytes`.
    annotation_byte_offset: u64,
    /// Indexed parameter-owner record selecting the governed dimension.
    governing_owner_record_index: u32,
    /// Byte offset of `governing_owner_record_index`.
    governing_owner_reference_offset: u64,
    /// Ordered non-null return geometry records.
    return_members: Vec<u32>,
    /// Byte offsets parallel to `return_members`.
    return_member_offsets: Vec<u64>,
    /// Dynamic class tag of the paired indexed record.
    paired_class_tag: String,
    /// Byte offset of the paired indexed record header.
    paired_byte_offset: u64,
    /// Numeric design-entity suffix of the owning sketch.
    owner_reference: u32,
    /// Byte offset of `owner_reference`.
    owner_reference_offset: u64,
}

impl TryFrom<DesignDimensionAnnotationFrameWire> for DesignDimensionAnnotationFrame {
    type Error = String;
    fn try_from(wire: DesignDimensionAnnotationFrameWire) -> Result<Self, Self::Error> {
        if wire.return_members.len() != wire.return_member_offsets.len() {
            return Err("return_members and return_member_offsets must have equal lengths".into());
        }
        Ok(Self {
            return_members: wire.return_members.into_iter().zip(wire.return_member_offsets).map(|(value, offset)| Located { value, offset }).collect(),
            id: wire.id,
            companion_record_index: wire.companion_record_index,
            governing_companion_record_index: wire.governing_companion_record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag,
            record_index: wire.record_index,
            frame_length: wire.frame_length,
            operands: wire.operands,
            entity_genesis: wire.entity_genesis,
            annotation_bytes: wire.annotation_bytes,
            annotation_byte_offset: wire.annotation_byte_offset,
            governing_owner_record_index: wire.governing_owner_record_index,
            governing_owner_reference_offset: wire.governing_owner_reference_offset,
            paired_class_tag: wire.paired_class_tag,
            paired_byte_offset: wire.paired_byte_offset,
            owner_reference: wire.owner_reference,
            owner_reference_offset: wire.owner_reference_offset,
        })
    }
}
impl From<DesignDimensionAnnotationFrame> for DesignDimensionAnnotationFrameWire {
    fn from(value: DesignDimensionAnnotationFrame) -> Self {
        let (return_members, return_member_offsets) = value.return_members.into_iter().map(|member| (member.value, member.offset)).unzip();
        Self { return_members, return_member_offsets,
            id: value.id,
            companion_record_index: value.companion_record_index,
            governing_companion_record_index: value.governing_companion_record_index,
            byte_offset: value.byte_offset,
            class_tag: value.class_tag,
            record_index: value.record_index,
            frame_length: value.frame_length,
            operands: value.operands,
            entity_genesis: value.entity_genesis,
            annotation_bytes: value.annotation_bytes,
            annotation_byte_offset: value.annotation_byte_offset,
            governing_owner_record_index: value.governing_owner_record_index,
            governing_owner_reference_offset: value.governing_owner_reference_offset,
            paired_class_tag: value.paired_class_tag,
            paired_byte_offset: value.paired_byte_offset,
            owner_reference: value.owner_reference,
            owner_reference_offset: value.owner_reference_offset,
        }
    }
}

/// Paired Fusion presentation frame that directly identifies a dimension's
/// measured sketch geometry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignDimensionPresentationFrame {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the primary indexed record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: String,
    /// Source indexed-record identity.
    pub record_index: u32,
    /// Byte length from the primary through the paired-header boundary.
    pub frame_length: u64,
    /// Ordered typed sketch-geometry operands.
    pub operands: Vec<DesignDimensionAnnotationOperand>,
    /// Opaque presentation bytes between the operand run and the paired
    /// `EntityTracking` header.
    pub presentation_bytes: Vec<u8>,
    /// Byte offset of `presentation_bytes`.
    pub presentation_byte_offset: u64,
    /// Dynamic class tag of the paired `EntityTracking` header.
    pub paired_class_tag: String,
    /// Byte offset of the paired indexed record header.
    pub paired_byte_offset: u64,
    /// Numeric suffix of the owning Sketch entity.
    pub owner_reference: u32,
    /// Byte offset of `owner_reference`.
    pub owner_reference_offset: u64,
    /// Indexed parameter-owner record governed by this frame.
    pub governing_owner_record_index: u32,
    /// Indexed Design parameter selected by the governing owner.
    pub governing_parameter_record_index: u32,
    /// Indexed parameter companion selected by the governing owner.
    pub governing_companion_record_index: u32,
}

/// One typed geometry locus and its dimension-role code.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignDimensionLocus {
    /// Return reference at the same position in the return run.
    pub returned: Located<u32>,
    /// Indexed sketch-point or sketch-curve record.
    pub geometry_record_index: u32,
    /// Byte offset of `geometry_record_index`.
    pub geometry_reference_offset: u64,
    /// Source role code following the geometry reference.
    pub role: u32,
    /// Byte offset of `role`.
    pub role_offset: u64,
}

/// Counted-locus frame nested under a dimensional parameter companion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "DesignDimensionLocusGroupWire"))]
#[serde(try_from = "DesignDimensionLocusGroupWire", into = "DesignDimensionLocusGroupWire")]
pub struct DesignDimensionLocusGroup {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Companion record containing this frame.
    pub companion_record_index: u32,
    /// Byte offset of the indexed record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: String,
    /// Source indexed-record identity.
    pub record_index: u32,
    /// Byte length through the zero byte preceding the next indexed header.
    pub frame_length: u64,
    /// Ordered typed geometry loci.
    pub loci: Vec<DesignDimensionLocus>,
    /// Numeric design-entity suffix of the owning sketch.
    pub owner_reference: u32,
    /// Byte offset of `owner_reference`.
    pub owner_reference_offset: u64,
    /// Source role code following the owner reference.
    pub owner_role: u32,
    /// Byte offset of `owner_role`.
    pub owner_role_offset: u64,
    /// Source constraint-state mask.
    pub state: u32,
    /// Byte offset of `state`.
    pub state_offset: u64,
    /// Dynamic class tag of the immediately following indexed record.
    pub next_class_tag: String,
    /// Identity of the immediately following indexed record.
    pub next_record_index: u32,
    /// Byte offset of the immediately following indexed record.
    pub next_byte_offset: u64,
}

/// One typed geometry locus and its dimension-role code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignDimensionLocusWire {
    /// Indexed sketch-point or sketch-curve record.
    geometry_record_index: u32,
    /// Byte offset of `geometry_record_index`.
    geometry_reference_offset: u64,
    /// Source role code following the geometry reference.
    role: u32,
    /// Byte offset of `role`.
    role_offset: u64,
}

/// Counted-locus frame nested under a dimensional parameter companion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignDimensionLocusGroupWire {
    /// Globally unique deterministic identifier for this native record.
    id: String,
    /// Companion record containing this frame.
    companion_record_index: u32,
    /// Byte offset of the indexed record header.
    byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    class_tag: String,
    /// Source indexed-record identity.
    record_index: u32,
    /// Byte length through the zero byte preceding the next indexed header.
    frame_length: u64,
    /// Ordered typed geometry loci.
    loci: Vec<DesignDimensionLocusWire>,
    /// Numeric design-entity suffix of the owning sketch.
    owner_reference: u32,
    /// Byte offset of `owner_reference`.
    owner_reference_offset: u64,
    /// Source role code following the owner reference.
    owner_role: u32,
    /// Byte offset of `owner_role`.
    owner_role_offset: u64,
    /// Source constraint-state mask.
    state: u32,
    /// Byte offset of `state`.
    state_offset: u64,
    /// Constraint kinds selected by `state`.
    constraint_kinds: Vec<SketchConstraintKind>,
    /// Bits in `state` outside the defined constraint mask.
    unknown_constraint_bits: u32,
    /// Ordered return geometry records.
    return_members: Vec<u32>,
    /// Byte offsets parallel to `return_members`.
    return_member_offsets: Vec<u64>,
    /// Dynamic class tag of the immediately following indexed record.
    next_class_tag: String,
    /// Identity of the immediately following indexed record.
    next_record_index: u32,
    /// Byte offset of the immediately following indexed record.
    next_byte_offset: u64,
}

impl DesignDimensionLocusGroup {
    #[must_use]
    pub fn constraint_kinds(&self) -> Vec<SketchConstraintKind> {
        constraint_kinds_from_state(u64::from(self.state)).0
    }

    #[must_use]
    pub fn unknown_constraint_bits(&self) -> u32 {
        self.state & !(SKETCH_CONSTRAINT_MASK as u32)
    }
}

impl TryFrom<DesignDimensionLocusGroupWire> for DesignDimensionLocusGroup {
    type Error = String;
    fn try_from(wire: DesignDimensionLocusGroupWire) -> Result<Self, Self::Error> {
        if wire.return_members.len() != wire.loci.len() {
            return Err("return_members must match loci".into());
        }
        if wire.return_member_offsets.len() != wire.loci.len() {
            return Err("return_member_offsets must match loci".into());
        }
        let (kinds, unknown) = constraint_kinds_from_state(u64::from(wire.state));
        if wire.constraint_kinds != kinds { return Err("constraint_kinds must match state".into()); }
        if u64::from(wire.unknown_constraint_bits) != unknown { return Err("unknown_constraint_bits must match state".into()); }
        let loci = wire.loci.into_iter().zip(wire.return_members.into_iter().zip(wire.return_member_offsets)).map(|(locus, (value, offset))| DesignDimensionLocus {
            geometry_record_index: locus.geometry_record_index,
            geometry_reference_offset: locus.geometry_reference_offset,
            role: locus.role,
            role_offset: locus.role_offset,
            returned: Located { value, offset },
        }).collect();
        Ok(Self { loci,
            id: wire.id,
            companion_record_index: wire.companion_record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag,
            record_index: wire.record_index,
            frame_length: wire.frame_length,
            owner_reference: wire.owner_reference,
            owner_reference_offset: wire.owner_reference_offset,
            owner_role: wire.owner_role,
            owner_role_offset: wire.owner_role_offset,
            state: wire.state,
            state_offset: wire.state_offset,
            next_class_tag: wire.next_class_tag,
            next_record_index: wire.next_record_index,
            next_byte_offset: wire.next_byte_offset,
        })
    }
}
impl From<DesignDimensionLocusGroup> for DesignDimensionLocusGroupWire {
    fn from(value: DesignDimensionLocusGroup) -> Self {
        let constraint_kinds = value.constraint_kinds();
        let unknown_constraint_bits = value.unknown_constraint_bits();
        let mut loci = Vec::with_capacity(value.loci.len());
        let mut return_members = Vec::with_capacity(value.loci.len());
        let mut return_member_offsets = Vec::with_capacity(value.loci.len());
        for locus in value.loci {
            return_members.push(locus.returned.value);
            return_member_offsets.push(locus.returned.offset);
            loci.push(DesignDimensionLocusWire {
                geometry_record_index: locus.geometry_record_index,
                geometry_reference_offset: locus.geometry_reference_offset,
                role: locus.role,
                role_offset: locus.role_offset,
            });
        }
        Self { loci, return_members, return_member_offsets, constraint_kinds, unknown_constraint_bits,
            id: value.id,
            companion_record_index: value.companion_record_index,
            byte_offset: value.byte_offset,
            class_tag: value.class_tag,
            record_index: value.record_index,
            frame_length: value.frame_length,
            owner_reference: value.owner_reference,
            owner_reference_offset: value.owner_reference_offset,
            owner_role: value.owner_role,
            owner_role_offset: value.owner_role_offset,
            state: value.state,
            state_offset: value.state_offset,
            next_class_tag: value.next_class_tag,
            next_record_index: value.next_record_index,
            next_byte_offset: value.next_byte_offset,
        }
    }
}

/// Boolean result operation stored by an Extrude parameter scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DesignExtrudeOperation {
    /// Union the swept volume with the selected bodies.
    Join,
    /// Subtract the swept volume from the selected bodies.
    Cut,
    /// Retain the intersection of the swept volume and selected bodies.
    Intersect,
    /// Create an independent body.
    NewBody,
}

/// Decoded Extrude travel form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DesignExtrudeExtent {
    /// Travel a signed fixed distance on the first side of the profile.
    OneSidedDistance,
    /// Travel on the first side until reaching a selected face or shape.
    OneSidedToFace,
    /// Travel on both sides until each side reaches its selected face.
    TwoSidedToFaces,
    /// Travel independent fixed distances on both sides of the profile.
    TwoSidedDistance,
    /// Travel a fixed distance on the first side and to a selected face on the second side.
    TwoSidedDistanceToFace,
    /// Travel one fixed total distance symmetrically around the profile plane.
    SymmetricDistance,
    /// Travel symmetrically through all material on both sides of the profile plane.
    SymmetricThroughAll,
    /// Travel on the first side until the next material region is exited.
    OneSidedThroughNext,
    /// Travel on the first side through all material.
    OneSidedThroughAll,
}

/// Starting support selected by the fixed Extrude prologue enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DesignExtrudeStart {
    /// Start on the selected sketch's plane.
    ProfilePlane,
    /// Start on a parallel offset from the selected sketch's plane.
    OffsetProfilePlane,
    /// Start on a selected face.
    FromFace,
}

/// Indexed-record prefix preceding a reference-aware Extrude prologue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignExtrudePrologueReferenceWire", into = "DesignExtrudePrologueReferenceWire")]
pub struct DesignExtrudePrologueReference {
    /// Referenced Design record.
    pub record_index: u32,
    /// Byte offset of `record_index`.
    pub record_index_offset: u64,
    /// Number of zero bytes between `record_index` and the operation or its marker.
    pub trailing_zero_count: u8,
    /// Optional marker byte between the zero run and the operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_prefix_marker: Option<Located<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignExtrudePrologueReferenceWire {
    /// Referenced Design record.
    record_index: u32,
    /// Byte offset of `record_index`.
    record_index_offset: u64,
    /// Number of zero bytes between `record_index` and the operation or its marker.
    trailing_zero_count: u8,
    /// Optional marker byte between the zero run and the operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    operation_prefix_marker: Option<u8>,
    /// Byte offset of `operation_prefix_marker` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    operation_prefix_marker_offset: Option<u64>,
}

impl TryFrom<DesignExtrudePrologueReferenceWire> for DesignExtrudePrologueReference {
    type Error = String;
    fn try_from(wire: DesignExtrudePrologueReferenceWire) -> Result<Self, Self::Error> {
        Ok(Self {
            record_index: wire.record_index,
            record_index_offset: wire.record_index_offset,
            trailing_zero_count: wire.trailing_zero_count,
            operation_prefix_marker: Located::from_wire(wire.operation_prefix_marker, wire.operation_prefix_marker_offset, "operation_prefix_marker")?,
        })
    }
}

impl From<DesignExtrudePrologueReference> for DesignExtrudePrologueReferenceWire {
    fn from(record: DesignExtrudePrologueReference) -> Self {
        Self {
            record_index: record.record_index,
            record_index_offset: record.record_index_offset,
            trailing_zero_count: record.trailing_zero_count,
            operation_prefix_marker: record.operation_prefix_marker.map(|marker| marker.value),
            operation_prefix_marker_offset: record.operation_prefix_marker.map(|marker| marker.offset),
        }
    }
}

/// Scope-reference ordinal repeated before a whole-body Extrude target extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignExtrudeTargetOrdinal {
    /// Zero-based ordinal in the enclosing scope reference table.
    pub scope_reference_ordinal: u32,
    /// Byte offset of `scope_reference_ordinal`.
    pub scope_reference_ordinal_offset: u64,
}

/// Fixed fields preceding an Extrude parameter scope's reference table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignExtrudePrologueWire", into = "DesignExtrudePrologueWire")]
pub enum DesignExtrudePrologue {
    /// Early distance-only layout with a nullable prefix field.
    LegacyDistance {
        /// Value of the nullable prefix field when its marker is present.
        prefix_value: Option<Located<u32>>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw extent-kind value (`2 = one-sided distance`).
        #[serde(alias = "extent_discriminator")]
        extent_kind: u32,
        /// Byte offset of `extent_kind`.
        #[serde(alias = "extent_discriminator_offset")]
        extent_kind_offset: u64,
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Raw geometry-kind discriminator (`0 = sheet`, `1 = solid`).
        geometry_kind: u32,
        /// Byte offset of `geometry_kind`.
        geometry_kind_offset: u64,
    },
    /// Reference-aware layout with an optional indexed-reference prefix.
    ReferenceAware {
        /// Indexed-record prefix, when present.
        reference: Option<DesignExtrudePrologueReference>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side extent discriminators stored after the profile normal and reference slots.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        #[serde(default)]
        side_extent_discriminator_offsets: [u64; 2],
        /// Repeated target-group ordinal in the whole-body target form.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_side_target_ordinal: Option<DesignExtrudeTargetOrdinal>,
        /// Decoded extent form.
        extent: DesignExtrudeExtent,
        /// Byte offsets parallel to `direction_face_extend_values`.
        #[serde(alias = "extent_discriminator_offsets")]
        direction_face_extend_offsets: [u64; 2],
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: u64,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: u64,
    },
    /// Shifted reference-aware two-sided face-target layout.
    ShiftedReferenceAware {
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side extent discriminators stored in the fixed legacy tail.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        #[serde(default)]
        side_extent_discriminator_offsets: [u64; 2],
        /// Decoded extent form.
        extent: DesignExtrudeExtent,
        /// Byte offsets parallel to `direction_face_extend_values`.
        #[serde(alias = "extent_discriminator_offsets")]
        direction_face_extend_offsets: [u64; 2],
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: u64,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: u64,
    },
    /// Shifted layout without the reference-aware prefix.
    LegacyShifted {
        /// Optional marker immediately before the operation fields.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operation_prefix_marker: Option<Located<u8>>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side termination discriminators stored after the profile-normal slots.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        #[serde(default)]
        side_extent_discriminator_offsets: [u64; 2],
        /// Decoded extent form.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extent: Option<DesignExtrudeExtent>,
        /// Byte offsets parallel to `direction_face_extend_values`.
        #[serde(alias = "extent_discriminator_offsets")]
        direction_face_extend_offsets: [u64; 2],
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: u64,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", tag = "layout")]
enum DesignExtrudePrologueWire {
    /// Early distance-only layout with a nullable prefix field.
    LegacyDistance {
        /// Value of the nullable prefix field when its marker is present.
        prefix_value: Option<u32>,
        /// Byte offset of `prefix_value` when present.
        prefix_value_offset: Option<u64>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw extent-kind value (`2 = one-sided distance`).
        #[serde(alias = "extent_discriminator")]
        extent_kind: u32,
        /// Byte offset of `extent_kind`.
        #[serde(alias = "extent_discriminator_offset")]
        extent_kind_offset: u64,
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Raw geometry-kind discriminator (`0 = sheet`, `1 = solid`).
        geometry_kind: u32,
        /// Byte offset of `geometry_kind`.
        geometry_kind_offset: u64,
    },
    /// Reference-aware layout with an optional indexed-reference prefix.
    ReferenceAware {
        /// Indexed-record prefix, when present.
        reference: Option<DesignExtrudePrologueReference>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side extent discriminators stored after the profile normal and reference slots.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        #[serde(default)]
        side_extent_discriminator_offsets: [u64; 2],
        /// Repeated target-group ordinal in the whole-body target form.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_side_target_ordinal: Option<DesignExtrudeTargetOrdinal>,
        /// Decoded extent form.
        extent: DesignExtrudeExtent,
        /// Byte offsets parallel to `direction_face_extend_values`.
        #[serde(alias = "extent_discriminator_offsets")]
        direction_face_extend_offsets: [u64; 2],
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: u64,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: u64,
    },
    /// Shifted reference-aware two-sided face-target layout.
    ShiftedReferenceAware {
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side extent discriminators stored in the fixed legacy tail.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        #[serde(default)]
        side_extent_discriminator_offsets: [u64; 2],
        /// Decoded extent form.
        extent: DesignExtrudeExtent,
        /// Byte offsets parallel to `direction_face_extend_values`.
        #[serde(alias = "extent_discriminator_offsets")]
        direction_face_extend_offsets: [u64; 2],
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: u64,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: u64,
    },
    /// Shifted layout without the reference-aware prefix.
    LegacyShifted {
        /// Optional marker immediately before the operation fields.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operation_prefix_marker: Option<u8>,
        /// Byte offset of `operation_prefix_marker` when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operation_prefix_marker_offset: Option<u64>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side termination discriminators stored after the profile-normal slots.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        #[serde(default)]
        side_extent_discriminator_offsets: [u64; 2],
        /// Decoded extent form.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extent: Option<DesignExtrudeExtent>,
        /// Byte offsets parallel to `direction_face_extend_values`.
        #[serde(alias = "extent_discriminator_offsets")]
        direction_face_extend_offsets: [u64; 2],
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: u64,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: u64,
    },
}

impl TryFrom<DesignExtrudePrologueWire> for DesignExtrudePrologue {
    type Error = String;
    fn try_from(wire: DesignExtrudePrologueWire) -> Result<Self, Self::Error> {
        Ok(match wire {
            DesignExtrudePrologueWire::LegacyDistance { prefix_value, prefix_value_offset, operation, operation_offset, extent_kind, extent_kind_offset, direction_reversed, direction_reversed_offset, geometry_kind, geometry_kind_offset } => Self::LegacyDistance { prefix_value: Located::from_wire(prefix_value, prefix_value_offset, "prefix_value")?, operation, operation_offset, extent_kind, extent_kind_offset, direction_reversed, direction_reversed_offset, geometry_kind, geometry_kind_offset },
            DesignExtrudePrologueWire::ReferenceAware { reference, operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, first_side_target_ordinal, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset } => Self::ReferenceAware { reference, operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, first_side_target_ordinal, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset },
            DesignExtrudePrologueWire::ShiftedReferenceAware { operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset } => Self::ShiftedReferenceAware { operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset },
            DesignExtrudePrologueWire::LegacyShifted { operation_prefix_marker, operation_prefix_marker_offset, operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset } => Self::LegacyShifted { operation_prefix_marker: Located::from_wire(operation_prefix_marker, operation_prefix_marker_offset, "operation_prefix_marker")?, operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset },
        })
    }
}

impl From<DesignExtrudePrologue> for DesignExtrudePrologueWire {
    fn from(record: DesignExtrudePrologue) -> Self {
        match record {
            DesignExtrudePrologue::LegacyDistance { prefix_value, operation, operation_offset, extent_kind, extent_kind_offset, direction_reversed, direction_reversed_offset, geometry_kind, geometry_kind_offset } => Self::LegacyDistance { prefix_value: prefix_value.map(|value| value.value), prefix_value_offset: prefix_value.map(|value| value.offset), operation, operation_offset, extent_kind, extent_kind_offset, direction_reversed, direction_reversed_offset, geometry_kind, geometry_kind_offset },
            DesignExtrudePrologue::ReferenceAware { reference, operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, first_side_target_ordinal, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset } => Self::ReferenceAware { reference, operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, first_side_target_ordinal, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset },
            DesignExtrudePrologue::ShiftedReferenceAware { operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset } => Self::ShiftedReferenceAware { operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset },
            DesignExtrudePrologue::LegacyShifted { operation_prefix_marker, operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset } => Self::LegacyShifted { operation_prefix_marker: operation_prefix_marker.map(|value| value.value), operation_prefix_marker_offset: operation_prefix_marker.map(|value| value.offset), operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset },
        }
    }
}

impl DesignExtrudePrologue {
    /// Boolean result operation.
    pub fn operation(self) -> DesignExtrudeOperation {
        match self {
            Self::LegacyDistance { operation, .. }
            | Self::ReferenceAware { operation, .. }
            | Self::ShiftedReferenceAware { operation, .. }
            | Self::LegacyShifted { operation, .. } => operation,
        }
    }

    /// Decoded extent form.
    pub fn extent(self) -> Option<DesignExtrudeExtent> {
        match self {
            Self::LegacyDistance { extent_kind: 2, .. } => {
                Some(DesignExtrudeExtent::OneSidedDistance)
            }
            Self::LegacyDistance { .. } => None,
            Self::ReferenceAware { extent, .. } => Some(extent),
            Self::ShiftedReferenceAware { extent, .. } => Some(extent),
            Self::LegacyShifted { extent, .. } => extent,
        }
    }

    /// Direction-reversal state.
    pub fn direction_reversed(self) -> bool {
        match self {
            Self::LegacyDistance {
                direction_reversed, ..
            }
            | Self::ReferenceAware {
                direction_reversed, ..
            }
            | Self::ShiftedReferenceAware {
                direction_reversed, ..
            }
            | Self::LegacyShifted {
                direction_reversed, ..
            } => direction_reversed,
        }
    }

    /// Whether the operation creates solid rather than sheet geometry.
    pub fn solid_operation(self) -> bool {
        match self {
            Self::LegacyDistance { geometry_kind, .. } => geometry_kind == 1,
            Self::ReferenceAware {
                solid_operation, ..
            }
            | Self::ShiftedReferenceAware {
                solid_operation, ..
            }
            | Self::LegacyShifted {
                solid_operation, ..
            } => solid_operation,
        }
    }

    /// Starting support.
    pub fn start(self) -> DesignExtrudeStart {
        match self {
            Self::LegacyDistance { .. } => DesignExtrudeStart::ProfilePlane,
            Self::ReferenceAware { start, .. }
            | Self::ShiftedReferenceAware { start, .. }
            | Self::LegacyShifted { start, .. } => start,
        }
    }
}

/// Driving-dimension mode stored by a Coil parameter scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DesignCoilExtent {
    /// Revolution count and total height are independent.
    RevolutionsHeight,
    /// Revolution count and pitch are independent.
    RevolutionsPitch,
    /// Total height and pitch are independent.
    HeightPitch,
    /// Revolution count and radial pitch define a planar spiral.
    Spiral,
}

/// Generated section family stored by a Coil parameter scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DesignCoilSection {
    /// Circular section.
    Circular,
    /// Square section.
    Square,
    /// Triangular section pointing away from the axis.
    ExternalTriangle,
    /// Triangular section pointing toward the axis.
    InternalTriangle,
}

/// Radial section placement stored by a Coil parameter scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DesignCoilSectionPlacement {
    /// Section inside the reference trajectory.
    Inside,
    /// Section centered on the reference trajectory.
    Center,
    /// Section outside the reference trajectory.
    Outside,
}

/// Selection carrier used by a compact Coil placement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesignCoilSelection {
    /// Nested entity-selection frame with one or two persistent identities.
    Persistent {
        /// Asset UUID qualifying the persistent selection namespace.
        asset_id: String,
        /// Context UUID qualifying the persistent selection namespace.
        context_id: String,
        /// Indexed nested record carrying the persistent identity pair.
        identity_record_index: u32,
        /// First persistent identity value.
        primary_identity: u64,
        /// Second persistent identity value in the expanded form.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        secondary_identity: Option<u64>,
        /// Curve secondary identity in the expanded curve-selection form.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        curve_secondary_identity: Option<u64>,
    },
    /// Face construction recipe carried by a placement selection frame.
    FaceRecipe {
        /// Asset UUID qualifying the recipe selection namespace.
        asset_id: String,
        /// Context UUID qualifying the recipe selection namespace.
        context_id: String,
        /// Indexed record containing the face recipe.
        recipe_record_index: u32,
        /// Byte offset of the face recipe record header.
        recipe_record_byte_offset: u64,
        /// Native construction-recipe arena identity.
        recipe_id: String,
        /// Exact face-recipe family.
        recipe_kind: ConstructionRecipeKind,
        /// Design entity id carried by the recipe, when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        design_id: Option<String>,
        /// Selector following the recipe's Design entity id, when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        design_selector: Option<ConstructionRecipeSelector>,
    },
}

/// Exact placement construction carried by a compact Coil scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignCoilPlacement {
    /// First ordered placement-construction reference.
    pub selection_record_index: u32,
    /// Byte offset of the support selection frame header.
    pub selection_record_byte_offset: u64,
    /// Dynamic class tag of the support selection frame.
    pub selection_class_tag: String,
    /// Exact selection semantics carried by the first placement reference.
    pub selection: DesignCoilSelection,
    /// Second ordered placement-construction reference: the frame carrier.
    pub transform_record_index: u32,
    /// Byte offset of the frame carrier header.
    pub transform_record_byte_offset: u64,
    /// Dynamic class tag of the frame carrier.
    pub transform_class_tag: String,
    /// Row-major local-to-model rigid transform. Matrix values are in source
    /// centimetres for the translation column.
    pub transform: [[f64; 4]; 4],
    /// Byte offset of the matrix, or absent for the encoded identity form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform_offset: Option<u64>,
}

/// Direct rigid placement carried by the long ten-reference Coil form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignCoilTransform {
    /// Row-major local-to-model rigid transform. Translation is in source
    /// centimetres.
    pub transform: [[f64; 4]; 4],
    /// Byte offset of the first matrix scalar.
    pub transform_offset: u64,
}

/// Exact construction data of a solid primitive scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", tag = "primitive")]
pub enum DesignSolidPrimitive {
    /// Axis-aligned box defined by five owned dimensions and offsets.
    Box(DesignBoxPrimitive),
    /// Circular cylinder defined by height and diameter owners.
    Cylinder(DesignCylinderPrimitive),
    /// Sphere defined by a placement frame and diameter.
    Sphere(DesignSpherePrimitive),
    /// Torus defined by a placement frame and two diameters.
    Torus(DesignTorusPrimitive),
}

/// Exact `Box` primitive construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignBoxPrimitive {
    /// Length along the source x-axis in source centimetres.
    pub length: f64,
    /// Referenced length owner.
    pub length_record_index: u32,
    /// Byte offset of the evaluated length.
    pub length_offset: u64,
    /// Width along the source y-axis in source centimetres.
    pub width: f64,
    /// Referenced width owner.
    pub width_record_index: u32,
    /// Byte offset of the evaluated width.
    pub width_offset: u64,
    /// Height along the source z-axis in source centimetres.
    pub height: f64,
    /// Referenced height owner.
    pub height_record_index: u32,
    /// Byte offset of the evaluated height.
    pub height_offset: u64,
    /// Translation along the source x-axis in source centimetres.
    pub offset_x: f64,
    /// Referenced x-offset owner.
    pub offset_x_record_index: u32,
    /// Byte offset of the evaluated x offset.
    pub offset_x_offset: u64,
    /// Translation along the source y-axis in source centimetres.
    pub offset_y: f64,
    /// Referenced y-offset owner.
    pub offset_y_record_index: u32,
    /// Byte offset of the evaluated y offset.
    pub offset_y_offset: u64,
    /// Result Boolean operation.
    pub operation: DesignExtrudeOperation,
    /// Byte offset of the operation enum.
    pub operation_offset: u64,
}

/// Exact `Cylinder` primitive construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignCylinderPrimitiveWire", into = "DesignCylinderPrimitiveWire")]
pub struct DesignCylinderPrimitive {
    /// Axial height in source centimetres.
    pub height: f64,
    /// Referenced height owner.
    pub height_record_index: u32,
    /// Byte offset of the evaluated height.
    pub height_offset: u64,
    /// Circular diameter in source centimetres.
    pub diameter: f64,
    /// Referenced diameter owner.
    pub diameter_record_index: u32,
    /// Byte offset of the evaluated diameter.
    pub diameter_offset: u64,
    /// Source frame carried by the shifted cylinder form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<Located<[[f64; 4]; 4]>>,
    /// Result Boolean operation.
    pub operation: DesignExtrudeOperation,
    /// Byte offset of the operation enum.
    pub operation_offset: u64,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignCylinderPrimitiveWire {
    /// Axial height in source centimetres.
    height: f64,
    /// Referenced height owner.
    height_record_index: u32,
    /// Byte offset of the evaluated height.
    height_offset: u64,
    /// Circular diameter in source centimetres.
    diameter: f64,
    /// Referenced diameter owner.
    diameter_record_index: u32,
    /// Byte offset of the evaluated diameter.
    diameter_offset: u64,
    /// Source frame carried by the shifted cylinder form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transform: Option<[[f64; 4]; 4]>,
    /// Byte offset of the shifted-form source frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transform_offset: Option<u64>,
    /// Result Boolean operation.
    operation: DesignExtrudeOperation,
    /// Byte offset of the operation enum.
    operation_offset: u64,
}

impl From<DesignCylinderPrimitive> for DesignCylinderPrimitiveWire {
    fn from(value: DesignCylinderPrimitive) -> Self {
        Self {
            height: value.height,
            height_record_index: value.height_record_index,
            height_offset: value.height_offset,
            diameter: value.diameter,
            diameter_record_index: value.diameter_record_index,
            diameter_offset: value.diameter_offset,
            transform: value.transform.map(|located| located.value),
            transform_offset: value.transform.map(|located| located.offset),
            operation: value.operation,
            operation_offset: value.operation_offset,
        }
    }
}

impl TryFrom<DesignCylinderPrimitiveWire> for DesignCylinderPrimitive {
    type Error = String;
    fn try_from(value: DesignCylinderPrimitiveWire) -> Result<Self, Self::Error> {
        Ok(Self {
            height: value.height,
            height_record_index: value.height_record_index,
            height_offset: value.height_offset,
            diameter: value.diameter,
            diameter_record_index: value.diameter_record_index,
            diameter_offset: value.diameter_offset,
            transform: Located::from_wire(value.transform, value.transform_offset, "transform")?,
            operation: value.operation,
            operation_offset: value.operation_offset,
        })
    }
}


/// Exact `Sphere` primitive construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignSpherePrimitive {
    /// Row-major local-to-model placement frame.
    pub transform: [[f64; 4]; 4],
    /// Byte offset of the placement matrix.
    pub transform_offset: u64,
    /// Sphere diameter in source centimetres.
    pub diameter: f64,
    /// Referenced diameter record.
    pub diameter_record_index: u32,
    /// Byte offset of the diameter scalar.
    pub diameter_offset: u64,
    /// Result Boolean operation.
    pub operation: DesignExtrudeOperation,
    /// Byte offset of the operation enum.
    pub operation_offset: u64,
}

/// Exact `Torus` primitive construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignTorusPrimitive {
    /// Row-major local-to-model placement frame.
    pub transform: [[f64; 4]; 4],
    /// Byte offset of the placement matrix.
    pub transform_offset: u64,
    /// Major diameter in source centimetres.
    pub major_diameter: f64,
    /// Referenced major-diameter record.
    pub major_diameter_record_index: u32,
    /// Byte offset of the major-diameter scalar.
    pub major_diameter_offset: u64,
    /// Tube diameter in source centimetres.
    pub minor_diameter: f64,
    /// Referenced minor-diameter record.
    pub minor_diameter_record_index: u32,
    /// Byte offset of the minor-diameter scalar.
    pub minor_diameter_offset: u64,
    /// Result Boolean operation.
    pub operation: DesignExtrudeOperation,
    /// Byte offset of the operation enum.
    pub operation_offset: u64,
}


impl From<DesignSolidPrimitive> for DesignScopePayload {
    fn from(value: DesignSolidPrimitive) -> Self {
        match value {
            DesignSolidPrimitive::Box(value) => Self::BoxPrimitive(Some(value)),
            DesignSolidPrimitive::Cylinder(value) => Self::CylinderPrimitive(Some(value)),
            DesignSolidPrimitive::Sphere(value) => Self::SpherePrimitive(Some(value)),
            DesignSolidPrimitive::Torus(value) => Self::TorusPrimitive(Some(value)),
        }
    }
}

/// Exact fixed-form construction data of a direct-face feature scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", tag = "operation")]
pub enum DesignDirectFaceOperation {
    /// Signed normal offset applied to selected faces.
    OffsetFaces(DesignOffsetFacesOperation),
    /// Thin-wall shell applied after removing selected faces.
    Shell(DesignShellOperation),
    /// Signed normal thickness added from selected faces.
    Thicken(DesignThickenOperation),
}

/// Exact `OffsetFaces` construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignOffsetFacesOperation {
    /// Signed distance in source centimetres.
    pub distance: f64,
    /// Referenced scalar record.
    pub distance_record_index: u32,
    /// Byte offset of the scalar.
    pub distance_offset: u64,
}

/// Exact `Shell` construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignShellOperation {
    /// Positive wall thickness in source centimetres.
    pub thickness: f64,
    /// Referenced scalar record.
    pub thickness_record_index: u32,
    /// Byte offset of the scalar.
    pub thickness_offset: u64,
    /// Whether the wall grows outward from the original boundary.
    pub outward: bool,
    /// Byte offset of the outward Boolean.
    pub outward_offset: u64,
}

/// Exact `Thicken` construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignThickenOperation {
    /// Signed thickness in source centimetres.
    pub signed_thickness: f64,
    /// Referenced scalar record.
    pub thickness_record_index: u32,
    /// Byte offset of the scalar.
    pub thickness_offset: u64,
}


/// Exact rigid transform carried by a Move feature scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignMoveOperation {
    /// Row-major model-space rigid transform in source centimetres.
    pub transform: [[f64; 4]; 4],
    /// Byte offset of the first matrix scalar.
    pub transform_offset: u64,
    /// Indexed class-349 record carrying `transform`.
    pub transform_record_index: u32,
    /// Source transform-form discriminator.
    pub form: u32,
    /// Byte offset of `form`.
    pub form_offset: u64,
}

/// One exact scalar carrier used by an Extrude scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignFixedExtrudeScalar {
    /// Scalar value in source centimetres for a distance or radians for an angle.
    pub value: f64,
    /// Referenced record carrying the scalar.
    pub record_index: u32,
    /// Byte offset of the scalar.
    pub value_offset: u64,
}

/// Exact carrier of an Extrude's one-sided distance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "carrier", content = "scalar", rename_all = "snake_case")]
pub enum DesignFixedExtrudeDistance {
    /// Signed distance in an owner-local scalar lane.
    FixedScalar(DesignFixedExtrudeScalar),
    /// Positive magnitude in an owned distance-construction frame.
    DistanceConstruction(DesignFixedExtrudeScalar),
}

/// Exact fixed scalar lanes carried by an Extrude scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignFixedExtrudeParameters {
    /// One-sided distance carrier in source centimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub along_distance: Option<DesignFixedExtrudeDistance>,
    /// Taper-angle lane in radians.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taper_angle: Option<DesignFixedExtrudeScalar>,
}

/// Exact fixed scalar lanes carried by a Fillet scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignFixedFilletParameters {
    /// Radius laws in scalar-lane order.
    pub groups: Vec<DesignFixedFilletGroup>,
}

/// One Fillet radius law carried by fixed scalar lanes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignFixedFilletGroup {
    /// Optional explicit dimensionless tangency-weight lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tangency_weight: Option<DesignFixedFilletTangencyWeight>,
    /// One constant radius, or endpoint radii followed by intermediate radii,
    /// in source centimetres.
    pub radii: Vec<f64>,
    /// Referenced radius scalar records in semantic radius order.
    pub radius_record_indexes: Vec<u32>,
    /// Byte offsets of the radius scalars in semantic radius order.
    pub radius_offsets: Vec<u64>,
    /// Normalized edge-chain positions paired with the intermediate radii.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intermediate_parameters: Vec<f64>,
    /// Referenced intermediate-position scalar records in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intermediate_parameter_record_indexes: Vec<u32>,
    /// Byte offsets of intermediate-position scalars in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intermediate_parameter_offsets: Vec<u64>,
}

/// One explicit fixed Fillet tangency-weight lane and its source provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignFixedFilletTangencyWeight {
    /// Positive dimensionless tangency weight.
    pub value: f64,
    /// Referenced tangency-weight scalar record.
    pub record_index: u32,
    /// Byte offset of the tangency-weight scalar.
    pub value_offset: u64,
}

/// Exact construction carried by a fixed circular-pattern scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignCircularPatternConstruction {
    /// Positive total instance count, including the seed.
    pub count: u32,
    /// Referenced compact count-parameter owner.
    pub count_record_index: u32,
    /// Byte offset of the evaluated count scalar.
    pub count_offset: u64,
    /// Positive angular span in radians.
    pub angle: f64,
    /// Referenced total-angle scalar.
    pub angle_record_index: u32,
    /// Byte offset of the total-angle scalar.
    pub angle_offset: u64,
    /// Serialized axis construction and its resolved placement.
    pub axis: DesignCircularPatternAxis,
    /// Referenced axis record.
    pub axis_record_index: u32,
    /// Referenced persistent selection operand.
    pub selection_record_index: u32,
}

/// Proven origin and unit direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesignAxis {
    pub origin: Point3,
    pub direction: Vector3,
}

/// Proven origin and unit normal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesignPlane {
    pub origin: Point3,
    pub normal: Vector3,
}

/// Axis construction carried by a fixed circular-pattern scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesignCircularPatternAxis {
    /// Axis coordinates stored directly in the Design record.
    Inline {
        /// Axis origin in source centimetres.
        origin: [f64; 3],
        /// Byte offset of the first origin coordinate.
        origin_offset: u64,
        /// Unit axis direction derived from the serialized displacement.
        direction: [f64; 3],
        /// Byte offset of the first direction coordinate.
        direction_offset: u64,
    },
    /// Axis selected through one or two persistent historical topology identities.
    HistoricalEdge {
        /// Referenced Design wrapper records, in serialized order.
        wrapper_record_indices: Vec<u32>,
        /// Persistent ASM identities carried by the wrappers.
        persistent_identities: Vec<u64>,
        /// Byte offsets parallel to `persistent_identities`.
        identity_offsets: Vec<u64>,
        /// Resolved model-space axis, when exact.
        #[serde(flatten)]
        resolved: Option<HistoricalResolvedAxisWire>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(crate) struct HistoricalResolvedAxisWire {
    pub resolved_origin: Point3,
    pub resolved_direction: Vector3,
}

impl From<DesignAxis> for HistoricalResolvedAxisWire {
    fn from(axis: DesignAxis) -> Self {
        Self {
            resolved_origin: axis.origin,
            resolved_direction: axis.direction,
        }
    }
}

impl From<HistoricalResolvedAxisWire> for DesignAxis {
    fn from(axis: HistoricalResolvedAxisWire) -> Self {
        Self {
            origin: axis.resolved_origin,
            direction: axis.resolved_direction,
        }
    }
}

/// Ordered scalar lanes carried by a rectangular-pattern scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignRectangularPatternConstruction {
    /// Positive U-direction instance count, including the seed.
    pub u_count: u32,
    /// Positive V-direction instance count, including the seed.
    pub v_count: u32,
    /// Signed U-direction seed-to-final-instance span in source centimetres.
    pub u_extent: f64,
    /// Signed V-direction seed-to-final-instance span in source centimetres.
    pub v_extent: f64,
    /// Parameter-owner records for U count, V count, U extent, and V extent.
    pub owner_record_indices: [u32; 4],
    /// Evaluated-value offsets parallel to `owner_record_indices`.
    pub value_offsets: [u64; 4],
    /// Exact serialized instance sequence when one pattern direction is active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instances: Option<DesignRectangularPatternInstances>,
}

/// Serialized placements of one linearized rectangular-pattern instance run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignRectangularPatternInstances {
    /// Seed record followed by the generated-instance records in pattern order.
    pub record_indices: Vec<u32>,
    /// Row-major local-to-model placements parallel to `record_indices`.
    pub transforms: Vec<[[f64; 4]; 4]>,
    /// Byte offsets of the first transform scalar parallel to `record_indices`.
    pub transform_offsets: Vec<u64>,
    /// Component occurrences carried by this run when the pattern repeats a component.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component_occurrences: Option<DesignComponentPatternOccurrences>,
}

/// Component seed and generated occurrences carried by a rectangular pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignComponentPatternOccurrences {
    /// Reusable local component definition shared by every occurrence.
    pub component_guid: String,
    /// Existing seed occurrence.
    pub seed_occurrence_guid: String,
    /// Newly generated occurrences in pattern order after the seed.
    pub generated_occurrence_guids: Vec<String>,
}

/// Domain of the two scalar limits carried by a legacy As-built scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DesignAssemblyLimitKind {
    /// Limits on the joint's angular degree of freedom.
    #[default]
    Angular,
    /// Limits on the joint's linear degree of freedom.
    Linear,
}

/// Ordered lower and upper limits carried by a legacy As-built assembly scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignAssemblyLimits {
    /// Degree-of-freedom domain of the limits.
    #[serde(default)]
    pub kind: DesignAssemblyLimitKind,
    /// Lower bound in the domain's native units.
    pub minimum: f64,
    /// Upper bound in the domain's native units.
    pub maximum: f64,
    /// Parameter-owner records for the lower and upper bounds.
    pub owner_record_indices: [u32; 2],
    /// Evaluated-value offsets parallel to `owner_record_indices`.
    pub value_offsets: [u64; 2],
}

/// Exact solved frame carried by a legacy 421-byte `As-built` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignAssemblySolvedFrame {
    /// Frame-carrier record named by reference-table entry eight.
    pub reference_record_index: u32,
    /// Byte offset of the frame-carrier reference in the scope.
    pub reference_offset: u64,
    /// Byte offset of the frame-carrier indexed header.
    pub record_byte_offset: u64,
    /// Dynamic class of the frame-carrier indexed record.
    pub class_tag: String,
    /// Row-major solved connector frame.
    pub transform: [[f64; 4]; 4],
    /// Byte offset of the first matrix scalar.
    pub transform_offset: u64,
}

/// Exact construction and face-selection pair carried by a legacy 421-byte
/// `As-built` operand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignAssemblyLegacyOperand {
    /// Primary construction record named by the scope reference table.
    pub construction_record_index: u32,
    /// Byte offset of the primary construction record header.
    pub construction_byte_offset: u64,
    /// Dynamic class of the primary construction record.
    pub construction_class_tag: String,
    /// Exact point or point-and-direction construction.
    pub construction: DesignAssemblyLegacyConstruction,
    /// Face-selection record paired with the construction record.
    pub selection: DesignAssemblyLegacySelection,
    /// Connector-local frame derived from the stored solved-frame orientation
    /// and this operand's construction position.
    pub frame: DesignAssemblyOperandFrame,
}

/// Construction carrier family used by a legacy 421-byte `As-built` operand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum DesignAssemblyLegacyConstruction {
    /// Point-only connector construction.
    Point(Box<DesignWorkPointConstruction>),
    /// Point-and-direction connector construction.
    Hole(Box<DesignHoleConstruction>),
}

/// Exact face-recipe selection paired with a legacy 421-byte construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignAssemblyLegacySelection {
    /// Indexed selection record.
    pub record_index: u32,
    /// Byte offset of the selection record header.
    pub byte_offset: u64,
    /// Dynamic class of the selection record.
    pub class_tag: String,
    /// Asset UUID qualifying the selection namespace.
    pub asset_id: String,
    /// Byte offset of the asset UUID's UTF-16LE payload.
    pub asset_id_offset: u64,
    /// Context UUID qualifying the selection.
    pub context_id: String,
    /// Byte offset of the context UUID's UTF-16LE payload.
    pub context_id_offset: u64,
    /// Indexed record containing the face recipe.
    pub recipe_record_index: u32,
    /// Byte offset of the recipe record's indexed header.
    pub recipe_record_byte_offset: u64,
    /// Construction-recipe arena id.
    pub recipe_id: String,
    /// Exact face-recipe family.
    pub recipe_kind: ConstructionRecipeKind,
    /// Persistent selector/reference tails carried by the recipe prefix.
    pub recipe_references: Vec<DesignRecipeReference>,
    /// Byte offset of the indexed record immediately after the recipe.
    pub next_byte_offset: u64,
}

/// Alignment scalars carried by an assembly-operation scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "DesignAssemblyAlignmentSerde"))]
#[serde(
    try_from = "DesignAssemblyAlignmentSerde",
    into = "DesignAssemblyAlignmentSerde"
)]
pub struct DesignAssemblyAlignment {
    /// Signed alignment rotation in radians.
    pub angle: f64,
    /// Signed local-frame translation in source centimetres.
    pub offset: [f64; 3],
    /// Parameter-owner records for the stored alignment scalars.
    pub owner_record_indices: Vec<u32>,
    /// Evaluated-value offsets parallel to `owner_record_indices`.
    pub value_offsets: Vec<u64>,
    /// Form-dependent operand payload: legacy 421-byte As-built, occurrence
    /// paths, or axial targets.
    pub operands: Option<DesignAssemblyOperandForm>,
    /// Optional limits carried by a legacy As-built scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(alias = "angular_limits")]
    pub limits: Option<DesignAssemblyLimits>,
    /// `JointOrigin` scope whose datum frame is carried by this scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joint_origin_scope_record_index: Option<u32>,
}

/// Operand layout selected by an assembly-alignment form.
#[derive(Debug, Clone, PartialEq)]
pub enum DesignAssemblyOperandForm {
    SolvedOnly {
        solved_frame: DesignAssemblySolvedFrame,
    },
    LegacyAsBuilt421 {
        carriers: [DesignAssemblyLegacyOperand; 2],
        solved_frame: DesignAssemblySolvedFrame,
    },
    Frames {
        frames: [DesignAssemblyOperandFrame; 2],
    },
    OccurrencePaths {
        frames: [DesignAssemblyOperandFrame; 2],
        paths: [DesignAssemblyOperandPath; 2],
    },
    Axial {
        frames: [DesignAssemblyOperandFrame; 2],
        targets: [DesignAssemblyAxialOperandTarget; 2],
    },
}

impl DesignAssemblyOperandForm {
    pub(crate) fn from_decoded(
        carriers: Option<[DesignAssemblyLegacyOperand; 2]>,
        solved_frame: Option<DesignAssemblySolvedFrame>,
        frames: Option<[DesignAssemblyOperandFrame; 2]>,
        qualifiers: Option<[DesignAssemblyOperandQualifier; 2]>,
    ) -> Option<Self> {
        DesignAssemblyAlignmentSerde {
            angle: 0.0,
            offset: [0.0; 3],
            owner_record_indices: Vec::new(),
            value_offsets: Vec::new(),
            operand_frames: frames,
            legacy_operand_carriers: carriers,
            solved_frame,
            operand_qualifiers: qualifiers,
            limits: None,
            joint_origin_scope_record_index: None,
        }
        .try_into()
        .ok()
        .and_then(|alignment: DesignAssemblyAlignment| alignment.operands)
    }
}

impl DesignAssemblyAlignment {
    pub(crate) fn operand_frames(&self) -> Option<[DesignAssemblyOperandFrame; 2]> {
        match &self.operands {
            Some(DesignAssemblyOperandForm::Frames { frames })
            | Some(DesignAssemblyOperandForm::OccurrencePaths { frames, .. })
            | Some(DesignAssemblyOperandForm::Axial { frames, .. }) => Some(frames.clone()),
            Some(
                DesignAssemblyOperandForm::LegacyAsBuilt421 { .. }
                | DesignAssemblyOperandForm::SolvedOnly { .. },
            )
            | None => None,
        }
    }

    pub(crate) fn legacy_operand_carriers(&self) -> Option<[DesignAssemblyLegacyOperand; 2]> {
        match &self.operands {
            Some(DesignAssemblyOperandForm::LegacyAsBuilt421 { carriers, .. }) => {
                Some(carriers.clone())
            }
            _ => None,
        }
    }

    pub(crate) fn solved_frame(&self) -> Option<DesignAssemblySolvedFrame> {
        match &self.operands {
            Some(DesignAssemblyOperandForm::SolvedOnly { solved_frame })
            | Some(DesignAssemblyOperandForm::LegacyAsBuilt421 { solved_frame, .. }) => {
                Some(solved_frame.clone())
            }
            _ => None,
        }
    }

    pub(crate) fn set_operand_qualifiers(
        &mut self,
        qualifiers: [DesignAssemblyOperandQualifier; 2],
    ) {
        let Some(frames) = self.operand_frames() else {
            return;
        };
        self.operands = match qualifiers {
            [DesignAssemblyOperandQualifier::OccurrencePath { path: first }, DesignAssemblyOperandQualifier::OccurrencePath { path: second }] => {
                Some(DesignAssemblyOperandForm::OccurrencePaths {
                    frames,
                    paths: [first, second],
                })
            }
            [DesignAssemblyOperandQualifier::AxialTarget { target: first }, DesignAssemblyOperandQualifier::AxialTarget { target: second }] => {
                Some(DesignAssemblyOperandForm::Axial {
                    frames,
                    targets: [first, second],
                })
            }
            _ => self.operands.take(),
        };
    }

    pub(crate) fn set_legacy_operand_carriers(
        &mut self,
        carriers: [DesignAssemblyLegacyOperand; 2],
    ) {
        let Some(solved_frame) = self.solved_frame() else {
            return;
        };
        self.operands = Some(DesignAssemblyOperandForm::LegacyAsBuilt421 {
            carriers,
            solved_frame,
        });
    }

    pub(crate) fn operand_qualifiers(&self) -> Option<[DesignAssemblyOperandQualifier; 2]> {
        match &self.operands {
            Some(DesignAssemblyOperandForm::OccurrencePaths { paths, .. }) => Some(
                paths
                    .clone()
                    .map(|path| DesignAssemblyOperandQualifier::OccurrencePath { path }),
            ),
            Some(DesignAssemblyOperandForm::Axial { targets, .. }) => Some(
                targets
                    .clone()
                    .map(|target| DesignAssemblyOperandQualifier::AxialTarget { target }),
            ),
            _ => None,
        }
    }

    /// Return both occurrence paths when every operand uses that qualifier form.
    pub(crate) fn operand_paths(&self) -> Option<[DesignAssemblyOperandPath; 2]> {
        match &self.operands {
            Some(DesignAssemblyOperandForm::OccurrencePaths { paths, .. }) => Some(paths.clone()),
            _ => None,
        }
    }

    /// Return both axial targets when every operand uses that qualifier form.
    pub(crate) fn axial_operand_targets(&self) -> Option<[DesignAssemblyAxialOperandTarget; 2]> {
        match &self.operands {
            Some(DesignAssemblyOperandForm::Axial { targets, .. }) => Some(targets.clone()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignAssemblyAlignmentSerde {
    angle: f64,
    offset: [f64; 3],
    owner_record_indices: Vec<u32>,
    value_offsets: Vec<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    operand_frames: Option<[DesignAssemblyOperandFrame; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    legacy_operand_carriers: Option<[DesignAssemblyLegacyOperand; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    solved_frame: Option<DesignAssemblySolvedFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    operand_qualifiers: Option<[DesignAssemblyOperandQualifier; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(alias = "angular_limits")]
    limits: Option<DesignAssemblyLimits>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    joint_origin_scope_record_index: Option<u32>,
}

impl TryFrom<DesignAssemblyAlignmentSerde> for DesignAssemblyAlignment {
    type Error = String;

    fn try_from(wire: DesignAssemblyAlignmentSerde) -> Result<Self, Self::Error> {
        let operands = match (
            wire.legacy_operand_carriers,
            wire.solved_frame,
            wire.operand_frames,
            wire.operand_qualifiers,
        ) {
            (Some(carriers), Some(solved_frame), None, None) => {
                Some(DesignAssemblyOperandForm::LegacyAsBuilt421 {
                    carriers,
                    solved_frame,
                })
            }
            (None, Some(solved_frame), None, None) => {
                Some(DesignAssemblyOperandForm::SolvedOnly { solved_frame })
            }
            (None, None, Some(frames), None) => Some(DesignAssemblyOperandForm::Frames { frames }),
            (None, None, Some(frames), Some(qualifiers)) => match qualifiers {
                [DesignAssemblyOperandQualifier::OccurrencePath { path: first }, DesignAssemblyOperandQualifier::OccurrencePath { path: second }] => {
                    Some(DesignAssemblyOperandForm::OccurrencePaths {
                        frames,
                        paths: [first, second],
                    })
                }
                [DesignAssemblyOperandQualifier::AxialTarget { target: first }, DesignAssemblyOperandQualifier::AxialTarget { target: second }] => {
                    Some(DesignAssemblyOperandForm::Axial {
                        frames,
                        targets: [first, second],
                    })
                }
                _ => {
                    return Err(
                        "assembly alignment operand_qualifiers must be a homogeneous path or axial pair"
                            .into(),
                    );
                }
            },
            (None, None, None, None) => None,
            _ => {
                return Err(
                    "assembly alignment operand fields disagree with a single operand form".into(),
                );
            }
        };
        Ok(Self {
            angle: wire.angle,
            offset: wire.offset,
            owner_record_indices: wire.owner_record_indices,
            value_offsets: wire.value_offsets,
            operands,
            limits: wire.limits,
            joint_origin_scope_record_index: wire.joint_origin_scope_record_index,
        })
    }
}

impl From<DesignAssemblyAlignment> for DesignAssemblyAlignmentSerde {
    fn from(alignment: DesignAssemblyAlignment) -> Self {
        let operand_frames = alignment.operand_frames();
        let legacy_operand_carriers = alignment.legacy_operand_carriers();
        let solved_frame = alignment.solved_frame();
        let operand_qualifiers = alignment.operand_qualifiers();
        Self {
            angle: alignment.angle,
            offset: alignment.offset,
            owner_record_indices: alignment.owner_record_indices,
            value_offsets: alignment.value_offsets,
            operand_frames,
            legacy_operand_carriers,
            solved_frame,
            operand_qualifiers,
            limits: alignment.limits,
            joint_origin_scope_record_index: alignment.joint_origin_scope_record_index,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesignAssemblyAxialOperandTarget {
    /// Connector object selected inside a placed `Component Insert` occurrence.
    ComponentInsertOccurrence {
        /// `Component Insert` scope whose placement has the selected occurrence role.
        component_insert_scope_record_index: u32,
        /// Construction carrier referenced by the operand frame.
        construction_record_index: u32,
        /// Dynamic class of the construction carrier's primary record.
        construction_class_tag: String,
        /// Byte offset of the construction carrier's primary indexed header.
        construction_byte_offset: u64,
        /// Byte offset of the construction carrier's transform.
        construction_transform_offset: u64,
        /// Byte offsets of the two axis-record indices in the construction carrier.
        axis_record_index_offsets: [u64; 2],
        /// Dynamic class of the construction carrier's paired record.
        construction_paired_class_tag: String,
        /// Byte offset of the construction carrier's paired indexed header.
        construction_paired_byte_offset: u64,
        /// Two axis selectors that identify the same connector object.
        selectors: Box<[DesignAssemblyAxialSelectorIdentity; 2]>,
    },
    /// Datum connector owned directly by the current document root.
    DocumentRootJointOrigin {
        /// Referenced `JointOrigin` feature scope.
        scope_record_index: u32,
    },
}

/// Persistent connector identity carried by one axial assembly selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignAssemblyAxialSelectorIdentityWire", into = "DesignAssemblyAxialSelectorIdentityWire")]
pub struct DesignAssemblyAxialSelectorIdentity {
    /// Axis record named by the operand construction carrier.
    pub axis_record_index: u32,
    /// Dynamic class of the axis record's primary indexed header.
    pub axis_class_tag: String,
    /// Byte offset of the axis record's primary indexed header.
    pub axis_byte_offset: u64,
    /// Dynamic class of the axis record's paired indexed header.
    pub axis_paired_class_tag: String,
    /// Byte offset of the axis record's paired indexed header.
    pub axis_paired_byte_offset: u64,
    /// Selector record three indices after the axis record.
    pub selector_record_index: u32,
    /// Dynamic class of the selector record's primary indexed header.
    pub selector_class_tag: String,
    /// Byte offset of the selector record's primary indexed header.
    pub selector_byte_offset: u64,
    /// Dynamic class of the selector record's paired indexed header.
    pub selector_paired_class_tag: String,
    /// Byte offset of the selector record's paired indexed header.
    pub selector_paired_byte_offset: u64,
    /// Nested record named by the selector prefix.
    pub nested_record_index: u32,
    /// Byte offset of `nested_record_index`.
    pub nested_record_index_offset: u64,
    /// Asset GUID of the enclosing selector.
    pub selector_asset_id: String,
    /// Byte offset of `selector_asset_id`.
    pub selector_asset_id_offset: u64,
    /// Context GUID of the enclosing selector.
    pub selector_context_id: String,
    /// Byte offset of `selector_context_id`.
    pub selector_context_id_offset: u64,
    /// Axis-specific same-segment occurrence reference.
    pub occurrence_reference: u64,
    /// Byte offset of `occurrence_reference`.
    pub occurrence_reference_offset: u64,
    /// Entity reference of the selected object in the referenced document.
    pub external_object_reference: u64,
    /// Byte offset of `external_object_reference`.
    pub external_object_reference_offset: u64,
    /// Segment carried by the cross-document object reference.
    pub external_segment: u32,
    /// Byte offset of `external_segment`.
    pub external_segment_offset: u64,
    /// Asset GUID carried by the cross-document object reference.
    pub external_asset_id: String,
    /// Byte offset of `external_asset_id`.
    pub external_asset_id_offset: u64,
    /// Link name carried by the cross-document object reference.
    pub external_link_name: String,
    /// Byte offset of `external_link_name`.
    pub external_link_name_offset: u64,
    /// Located property key and referenced-document version identity.
    pub external_version: Option<DesignExternalVersion>,
    /// Embedded record that carries the selected occurrence role.
    pub role_record_index: u32,
    /// Dynamic class of the occurrence-role record.
    pub role_class_tag: String,
    /// Byte offset of the occurrence-role record's indexed header.
    pub role_byte_offset: u64,
    /// Occurrence-role GUID joining this selector to a component insertion.
    pub occurrence_role: String,
    /// Byte offset of `occurrence_role`.
    pub occurrence_role_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignAssemblyAxialSelectorIdentityWire {
    /// Axis record named by the operand construction carrier.
    axis_record_index: u32,
    /// Dynamic class of the axis record's primary indexed header.
    axis_class_tag: String,
    /// Byte offset of the axis record's primary indexed header.
    axis_byte_offset: u64,
    /// Dynamic class of the axis record's paired indexed header.
    axis_paired_class_tag: String,
    /// Byte offset of the axis record's paired indexed header.
    axis_paired_byte_offset: u64,
    /// Selector record three indices after the axis record.
    selector_record_index: u32,
    /// Dynamic class of the selector record's primary indexed header.
    selector_class_tag: String,
    /// Byte offset of the selector record's primary indexed header.
    selector_byte_offset: u64,
    /// Dynamic class of the selector record's paired indexed header.
    selector_paired_class_tag: String,
    /// Byte offset of the selector record's paired indexed header.
    selector_paired_byte_offset: u64,
    /// Nested record named by the selector prefix.
    nested_record_index: u32,
    /// Byte offset of `nested_record_index`.
    nested_record_index_offset: u64,
    /// Asset GUID of the enclosing selector.
    selector_asset_id: String,
    /// Byte offset of `selector_asset_id`.
    selector_asset_id_offset: u64,
    /// Context GUID of the enclosing selector.
    selector_context_id: String,
    /// Byte offset of `selector_context_id`.
    selector_context_id_offset: u64,
    /// Axis-specific same-segment occurrence reference.
    occurrence_reference: u64,
    /// Byte offset of `occurrence_reference`.
    occurrence_reference_offset: u64,
    /// Entity reference of the selected object in the referenced document.
    external_object_reference: u64,
    /// Byte offset of `external_object_reference`.
    external_object_reference_offset: u64,
    /// Segment carried by the cross-document object reference.
    external_segment: u32,
    /// Byte offset of `external_segment`.
    external_segment_offset: u64,
    /// Asset GUID carried by the cross-document object reference.
    external_asset_id: String,
    /// Byte offset of `external_asset_id`.
    external_asset_id_offset: u64,
    /// Link name carried by the cross-document object reference.
    external_link_name: String,
    /// Byte offset of `external_link_name`.
    external_link_name_offset: u64,
    /// Optional property key preceding the version identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    external_property_key: Option<String>,
    /// Byte offset of `external_property_key` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    external_property_key_offset: Option<u64>,
    /// Optional referenced-document version identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    external_version_urn: Option<String>,
    /// Byte offset of `external_version_urn` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    external_version_urn_offset: Option<u64>,
    /// Embedded record that carries the selected occurrence role.
    role_record_index: u32,
    /// Dynamic class of the occurrence-role record.
    role_class_tag: String,
    /// Byte offset of the occurrence-role record's indexed header.
    role_byte_offset: u64,
    /// Occurrence-role GUID joining this selector to a component insertion.
    occurrence_role: String,
    /// Byte offset of `occurrence_role`.
    occurrence_role_offset: u64,
}

impl TryFrom<DesignAssemblyAxialSelectorIdentityWire> for DesignAssemblyAxialSelectorIdentity {
    type Error = String;
    fn try_from(wire: DesignAssemblyAxialSelectorIdentityWire) -> Result<Self, Self::Error> {
        Ok(Self {
            axis_record_index: wire.axis_record_index,
            axis_class_tag: wire.axis_class_tag,
            axis_byte_offset: wire.axis_byte_offset,
            axis_paired_class_tag: wire.axis_paired_class_tag,
            axis_paired_byte_offset: wire.axis_paired_byte_offset,
            selector_record_index: wire.selector_record_index,
            selector_class_tag: wire.selector_class_tag,
            selector_byte_offset: wire.selector_byte_offset,
            selector_paired_class_tag: wire.selector_paired_class_tag,
            selector_paired_byte_offset: wire.selector_paired_byte_offset,
            nested_record_index: wire.nested_record_index,
            nested_record_index_offset: wire.nested_record_index_offset,
            selector_asset_id: wire.selector_asset_id,
            selector_asset_id_offset: wire.selector_asset_id_offset,
            selector_context_id: wire.selector_context_id,
            selector_context_id_offset: wire.selector_context_id_offset,
            occurrence_reference: wire.occurrence_reference,
            occurrence_reference_offset: wire.occurrence_reference_offset,
            external_object_reference: wire.external_object_reference,
            external_object_reference_offset: wire.external_object_reference_offset,
            external_segment: wire.external_segment,
            external_segment_offset: wire.external_segment_offset,
            external_asset_id: wire.external_asset_id,
            external_asset_id_offset: wire.external_asset_id_offset,
            external_link_name: wire.external_link_name,
            external_link_name_offset: wire.external_link_name_offset,
            external_version: match (wire.external_property_key, wire.external_property_key_offset, wire.external_version_urn, wire.external_version_urn_offset) {
                (None, None, None, None) => None,
                (Some(key), Some(key_offset), Some(urn), Some(urn_offset)) => Some(DesignExternalVersion { property_key: Located { value: key, offset: key_offset }, version_urn: Located { value: urn, offset: urn_offset } }),
                _ => return Err("external_property_key, external_property_key_offset, external_version_urn and external_version_urn_offset must occur together".into()),
            },
            role_record_index: wire.role_record_index,
            role_class_tag: wire.role_class_tag,
            role_byte_offset: wire.role_byte_offset,
            occurrence_role: wire.occurrence_role,
            occurrence_role_offset: wire.occurrence_role_offset,
        })
    }
}

impl From<DesignAssemblyAxialSelectorIdentity> for DesignAssemblyAxialSelectorIdentityWire {
    fn from(record: DesignAssemblyAxialSelectorIdentity) -> Self {
        Self {
            axis_record_index: record.axis_record_index,
            axis_class_tag: record.axis_class_tag,
            axis_byte_offset: record.axis_byte_offset,
            axis_paired_class_tag: record.axis_paired_class_tag,
            axis_paired_byte_offset: record.axis_paired_byte_offset,
            selector_record_index: record.selector_record_index,
            selector_class_tag: record.selector_class_tag,
            selector_byte_offset: record.selector_byte_offset,
            selector_paired_class_tag: record.selector_paired_class_tag,
            selector_paired_byte_offset: record.selector_paired_byte_offset,
            nested_record_index: record.nested_record_index,
            nested_record_index_offset: record.nested_record_index_offset,
            selector_asset_id: record.selector_asset_id,
            selector_asset_id_offset: record.selector_asset_id_offset,
            selector_context_id: record.selector_context_id,
            selector_context_id_offset: record.selector_context_id_offset,
            occurrence_reference: record.occurrence_reference,
            occurrence_reference_offset: record.occurrence_reference_offset,
            external_object_reference: record.external_object_reference,
            external_object_reference_offset: record.external_object_reference_offset,
            external_segment: record.external_segment,
            external_segment_offset: record.external_segment_offset,
            external_asset_id: record.external_asset_id,
            external_asset_id_offset: record.external_asset_id_offset,
            external_link_name: record.external_link_name,
            external_link_name_offset: record.external_link_name_offset,
            external_property_key: record.external_version.as_ref().map(|version| version.property_key.value.clone()),
            external_property_key_offset: record.external_version.as_ref().map(|version| version.property_key.offset),
            external_version_urn: record.external_version.as_ref().map(|version| version.version_urn.value.clone()),
            external_version_urn_offset: record.external_version.as_ref().map(|version| version.version_urn.offset),
            role_record_index: record.role_record_index,
            role_class_tag: record.role_class_tag,
            role_byte_offset: record.role_byte_offset,
            occurrence_role: record.occurrence_role,
            occurrence_role_offset: record.occurrence_role_offset,
        }
    }
}

impl DesignAssemblyAxialSelectorIdentity {
    /// Report whether two axis selectors carry the same persistent connector identity.
    pub(crate) fn selects_same_object(&self, other: &Self) -> bool {

        self.selector_asset_id
            .eq_ignore_ascii_case(&other.selector_asset_id)
            && self
                .selector_context_id
                .eq_ignore_ascii_case(&other.selector_context_id)
            && self.external_object_reference == other.external_object_reference
            && self.external_segment == other.external_segment
            && self
                .external_asset_id
                .eq_ignore_ascii_case(&other.external_asset_id)
            && self.external_link_name == other.external_link_name
            && match (&self.external_version, &other.external_version) {
                (None, None) => true,
                (Some(first), Some(second)) => {
                    first.property_key.value.eq_ignore_ascii_case(&second.property_key.value)
                        && first.version_urn.value == second.version_urn.value
                }
                _ => false,
            }
    }
}

/// Exact reference chain from an assembly scope to one occurrence-path record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignAssemblyOperandPathLink {
    /// Byte offset of the locator-record index in the assembly scope.
    pub locator_reference_offset: u64,
    /// Locator-record index named by the assembly scope.
    pub locator_record_index: u32,
    /// Dynamic indexed-record class carrying the locator.
    pub locator_class_tag: String,
    /// Byte offset of the locator's indexed header.
    pub locator_byte_offset: u64,
    /// Byte offset of the assembly-scope backlink in the locator.
    pub locator_scope_reference_offset: u64,
    /// Wrapper-record index named by the locator.
    pub wrapper_record_index: u32,
    /// Byte offset of the wrapper-record index in the locator.
    pub wrapper_reference_offset: u64,
    /// Dynamic indexed-record class carrying the wrapper.
    pub wrapper_class_tag: String,
    /// Byte offset of the wrapper's indexed header.
    pub wrapper_byte_offset: u64,
    /// Byte offset of the path-record index in the wrapper.
    pub path_reference_offset: u64,
}

/// Counted occurrence path qualifying one assembly operand construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignAssemblyOperandPath {
    /// Exact ordered scope-to-locator-to-wrapper reference chain.
    pub link: DesignAssemblyOperandPathLink,
    /// Path-record index.
    pub record_index: u32,
    /// Indexed-record class carrying this path.
    pub class_tag: String,
    /// Byte offset of the indexed header.
    pub byte_offset: u64,
    /// Ordered occurrence GUIDs from the outermost occurrence to the selected occurrence.
    pub occurrence_guids: Vec<String>,
    /// Byte offsets of the UTF-16 GUID code units parallel to `occurrence_guids`.
    pub occurrence_guid_offsets: Vec<u64>,
    /// Four ordered identity GUIDs following a class-390 occurrence path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub identity_guids: Vec<String>,
    /// Byte offsets parallel to `identity_guids`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub identity_guid_offsets: Vec<u64>,
}

/// One exact native qualifier for an assembly operand construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesignAssemblyOperandQualifier {
    /// Ordered occurrence path carried by a locator graph.
    OccurrencePath {
        /// Exact path and its native reference chain.
        path: DesignAssemblyOperandPath,
    },
    /// Pathless target carried by an axial selector graph.
    AxialTarget {
        /// Exact pathless target.
        target: DesignAssemblyAxialOperandTarget,
    },
    /// Datum connector owned directly by the current document root.
    JointOrigin {
        /// Referenced `JointOrigin` feature scope.
        scope_record_index: u32,
        /// Dynamic class of the scope's primary indexed header.
        class_tag: String,
        /// Byte offset of the primary indexed header.
        byte_offset: u64,
        /// Dynamic class of the paired indexed header.
        paired_class_tag: String,
        /// Byte offset of the paired indexed header.
        paired_byte_offset: u64,
    },
}

impl DesignAssemblyOperandQualifier {
    /// Return the occurrence path when this qualifier carries one.
    pub(crate) fn occurrence_path(&self) -> Option<&DesignAssemblyOperandPath> {
        match self {
            Self::OccurrencePath { path } => Some(path),
            Self::AxialTarget { .. } | Self::JointOrigin { .. } => None,
        }
    }

    /// Return the axial target when this qualifier carries one.
    pub(crate) fn axial_target(&self) -> Option<&DesignAssemblyAxialOperandTarget> {
        match self {
            Self::AxialTarget { target } => Some(target),
            Self::OccurrencePath { .. } | Self::JointOrigin { .. } => None,
        }
    }
}

/// One operand frame embedded by an assembly-operation scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignAssemblyOperandFrame {
    /// Construction record referenced by the operand.
    pub reference_record_index: u32,
    /// Byte offset of `reference_record_index`.
    pub reference_offset: u64,
    /// Row-major operand-local-to-model transform.
    pub transform: [[f64; 4]; 4],
    /// Byte offset of the first transform scalar.
    pub transform_offset: u64,
}

/// External occurrence and placement joined through a `Component Insert` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignComponentInsertConstruction {
    /// Scope-owned relation record.
    pub relation_record_index: u32,
    /// Grouped occurrence carrier named by the relation record.
    pub carrier_record_index: u32,
    /// Eight-byte occurrence identity carried by the scope prologue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurrence_identity: Option<u64>,
    /// Occurrence-role GUID joining the carrier to the external-reference table.
    pub neutron_role: String,
    /// Byte offset of the occurrence-role string payload.
    pub neutron_role_offset: u64,
    /// Row-major local occurrence transform in centimetres.
    pub transform: [[f64; 4]; 4],
    /// Byte offset of the first scope-local transform scalar. `None` is the
    /// stored identity form, which has no scalar block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform_offset: Option<u64>,
    /// Byte offset of the equal transform's first scalar in the grouped
    /// carrier. `None` is the stored identity form, which has no scalar block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carrier_transform_offset: Option<u64>,
}

/// Local component occurrence joined through a `DerivedInstance` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignDerivedInstanceConstruction {
    /// Scope prologue record referenced by the fixed field at scope offset 22.
    pub reference_record_index: u32,
    /// Scope-owned class-310 relation record.
    pub relation_record_index: u32,
    /// Class-380 component-occurrence carrier named by the relation.
    pub carrier_record_index: u32,
    /// Component definition GUID carried by the joined occurrence.
    pub component_guid: String,
    /// Placed occurrence GUID carried by the joined occurrence.
    pub occurrence_guid: String,
    /// Row-major local-to-model placement in centimetres.
    pub transform: [[f64; 4]; 4],
    /// Byte offset of the first scope-local transform scalar.
    pub transform_offset: u64,
}

/// One exact local component-occurrence carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignComponentOccurrenceWire", into = "DesignComponentOccurrenceWire")]
pub struct DesignComponentOccurrence {
    /// Stable native record identity.
    pub id: String,
    /// Indexed-record class carrying this occurrence.
    pub class_tag: String,
    /// Indexed carrier record.
    pub record_index: u32,
    /// Byte offset of the indexed header.
    pub byte_offset: u64,
    /// Referenced component-definition record.
    pub component_record_index: u64,
    /// Stable component-definition GUID.
    pub component_guid: String,
    /// Byte offset of the component GUID payload.
    pub component_guid_offset: u64,
    /// Stable placed-occurrence GUID.
    pub occurrence_guid: String,
    /// Byte offset of the occurrence GUID payload.
    pub occurrence_guid_offset: u64,
    /// One-based occurrence ordinal within the component definition.
    pub occurrence_ordinal: u32,
    /// Explicit local-to-model placement for placed occurrences.
    pub transform: Option<Located<[[f64; 4]; 4]>>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignComponentOccurrenceWire {
    /// Stable native record identity.
    id: String,
    /// Indexed-record class carrying this occurrence.
    class_tag: String,
    /// Indexed carrier record.
    record_index: u32,
    /// Byte offset of the indexed header.
    byte_offset: u64,
    /// Referenced component-definition record.
    component_record_index: u64,
    /// Stable component-definition GUID.
    component_guid: String,
    /// Byte offset of the component GUID payload.
    component_guid_offset: u64,
    /// Stable placed-occurrence GUID.
    occurrence_guid: String,
    /// Byte offset of the occurrence GUID payload.
    occurrence_guid_offset: u64,
    /// One-based occurrence ordinal within the component definition.
    occurrence_ordinal: u32,
    /// Explicit local-to-model placement for placed occurrences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transform: Option<[[f64; 4]; 4]>,
    /// Byte offset of the explicit placement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transform_offset: Option<u64>,
}

impl From<DesignComponentOccurrence> for DesignComponentOccurrenceWire {
    fn from(value: DesignComponentOccurrence) -> Self {
        Self {
            id: value.id,
            class_tag: value.class_tag,
            record_index: value.record_index,
            byte_offset: value.byte_offset,
            component_record_index: value.component_record_index,
            component_guid: value.component_guid,
            component_guid_offset: value.component_guid_offset,
            occurrence_guid: value.occurrence_guid,
            occurrence_guid_offset: value.occurrence_guid_offset,
            occurrence_ordinal: value.occurrence_ordinal,
            transform: value.transform.map(|frame| frame.value),
            transform_offset: value.transform.map(|frame| frame.offset),
        }
    }
}

impl TryFrom<DesignComponentOccurrenceWire> for DesignComponentOccurrence {
    type Error = String;
    fn try_from(value: DesignComponentOccurrenceWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            class_tag: value.class_tag,
            record_index: value.record_index,
            byte_offset: value.byte_offset,
            component_record_index: value.component_record_index,
            component_guid: value.component_guid,
            component_guid_offset: value.component_guid_offset,
            occurrence_guid: value.occurrence_guid,
            occurrence_guid_offset: value.occurrence_guid_offset,
            occurrence_ordinal: value.occurrence_ordinal,
            transform: Located::from_wire(value.transform, value.transform_offset, "transform")?,
        })
    }
}

/// Legacy component copy/paste construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignCopyPasteComponentOperation {
    /// Scope-owned relation record.
    pub relation_record_index: u32,
    /// Existing source occurrence carrier.
    pub source_occurrence_record_index: u32,
    /// Newly copied occurrence carrier.
    pub copied_occurrence_record_index: u32,
    /// Reusable component definition shared by source and copy.
    pub component_guid: String,
    /// Existing source occurrence identity.
    pub source_occurrence_guid: String,
    /// Newly copied occurrence identity.
    pub copied_occurrence_guid: String,
    /// Source placement embedded by the scope.
    pub source_transform: [[f64; 4]; 4],
    /// Byte offset of the source placement.
    pub source_transform_offset: u64,
    /// Copied placement embedded by both scope and occurrence carrier.
    pub copied_transform: [[f64; 4]; 4],
    /// Byte offset of the scope-local copied placement.
    pub copied_transform_offset: u64,
}

/// Exact construction carried by a Mirror scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignMirrorConstructionWire", into = "DesignMirrorConstructionWire")]
pub struct DesignMirrorConstruction {
    /// Fixed instance count, including the seed.
    pub count: u32,
    /// Parameter-owner record carrying `count`.
    pub count_record_index: u32,
    /// Byte offset of the evaluated count scalar.
    pub count_offset: u64,
    /// Positive model-space stitch tolerance in source centimetres.
    pub stitch_tolerance: f64,
    /// Parameter-owner record carrying `stitch_tolerance`, when the source
    /// stores the scalar in a separate owner record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stitch_tolerance_record_index: Option<u32>,
    /// Byte offset of the evaluated stitch-tolerance scalar.
    pub stitch_tolerance_offset: u64,
    /// Inline scope-frame carrier used by the legacy Mirror envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stitch_tolerance_scope: Option<DesignMirrorScopeTolerance>,
    /// Seed group selected by the source operation.
    pub seed_group_record_index: u32,
    /// Role-`0x5` mirror-plane group.
    pub plane_group_record_index: u32,
    /// Referenced seed feature scope when the seed is a complete feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed_feature_scope_record_index: Option<Located<u32>>,
    /// Referenced `WorkPlane` scope, when the plane operand names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane_scope_record_index: Option<Located<u32>>,
    /// Persistent entity-selection record used as the mirror plane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane_selection_record_index: Option<u32>,
    /// Proven selected-face mirror plane, when exact.
    #[serde(flatten)]
    pub plane: Option<MirrorPlaneWire>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignMirrorConstructionWire {
    /// Fixed instance count, including the seed.
    count: u32,
    /// Parameter-owner record carrying `count`.
    count_record_index: u32,
    /// Byte offset of the evaluated count scalar.
    count_offset: u64,
    /// Positive model-space stitch tolerance in source centimetres.
    stitch_tolerance: f64,
    /// Parameter-owner record carrying `stitch_tolerance`, when the source
    /// stores the scalar in a separate owner record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stitch_tolerance_record_index: Option<u32>,
    /// Byte offset of the evaluated stitch-tolerance scalar.
    stitch_tolerance_offset: u64,
    /// Inline scope-frame carrier used by the legacy Mirror envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stitch_tolerance_scope: Option<DesignMirrorScopeTolerance>,
    /// Seed group selected by the source operation.
    seed_group_record_index: u32,
    /// Role-`0x5` mirror-plane group.
    plane_group_record_index: u32,
    /// Referenced seed feature scope when the seed is a complete feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    seed_feature_scope_record_index: Option<u32>,
    /// Byte offset of the optional seed-feature reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    seed_feature_reference_offset: Option<u64>,
    /// Referenced `WorkPlane` scope, when the plane operand names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plane_scope_record_index: Option<u32>,
    /// Byte offset of the optional `WorkPlane` reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plane_reference_offset: Option<u64>,
    /// Persistent entity-selection record used as the mirror plane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plane_selection_record_index: Option<u32>,
    /// Proven selected-face mirror plane, when exact.
    #[serde(flatten)]
    plane: Option<MirrorPlaneWire>,
}

impl TryFrom<DesignMirrorConstructionWire> for DesignMirrorConstruction {
    type Error = String;
    fn try_from(wire: DesignMirrorConstructionWire) -> Result<Self, Self::Error> {
        Ok(Self {
            count: wire.count,
            count_record_index: wire.count_record_index,
            count_offset: wire.count_offset,
            stitch_tolerance: wire.stitch_tolerance,
            stitch_tolerance_record_index: wire.stitch_tolerance_record_index,
            stitch_tolerance_offset: wire.stitch_tolerance_offset,
            stitch_tolerance_scope: wire.stitch_tolerance_scope,
            seed_group_record_index: wire.seed_group_record_index,
            plane_group_record_index: wire.plane_group_record_index,
            seed_feature_scope_record_index: Located::from_wire(wire.seed_feature_scope_record_index, wire.seed_feature_reference_offset, "seed_feature_scope_record_index").map_err(|_| "seed_feature_scope_record_index and seed_feature_reference_offset must occur together")?,
            plane_scope_record_index: Located::from_wire(wire.plane_scope_record_index, wire.plane_reference_offset, "plane_scope_record_index").map_err(|_| "plane_scope_record_index and plane_reference_offset must occur together")?,
            plane_selection_record_index: wire.plane_selection_record_index,
            plane: wire.plane,
        })
    }
}

impl From<DesignMirrorConstruction> for DesignMirrorConstructionWire {
    fn from(record: DesignMirrorConstruction) -> Self {
        Self {
            count: record.count,
            count_record_index: record.count_record_index,
            count_offset: record.count_offset,
            stitch_tolerance: record.stitch_tolerance,
            stitch_tolerance_record_index: record.stitch_tolerance_record_index,
            stitch_tolerance_offset: record.stitch_tolerance_offset,
            stitch_tolerance_scope: record.stitch_tolerance_scope,
            seed_group_record_index: record.seed_group_record_index,
            plane_group_record_index: record.plane_group_record_index,
            seed_feature_scope_record_index: record.seed_feature_scope_record_index.map(|reference| reference.value),
            seed_feature_reference_offset: record.seed_feature_scope_record_index.map(|reference| reference.offset),
            plane_scope_record_index: record.plane_scope_record_index.map(|reference| reference.value),
            plane_reference_offset: record.plane_scope_record_index.map(|reference| reference.offset),
            plane_selection_record_index: record.plane_selection_record_index,
            plane: record.plane,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MirrorPlaneWire {
    pub plane_origin: Point3,
    pub plane_normal: Vector3,
}

impl From<DesignPlane> for MirrorPlaneWire {
    fn from(plane: DesignPlane) -> Self {
        Self {
            plane_origin: plane.origin,
            plane_normal: plane.normal,
        }
    }
}

impl From<MirrorPlaneWire> for DesignPlane {
    fn from(plane: MirrorPlaneWire) -> Self {
        Self {
            origin: plane.plane_origin,
            normal: plane.plane_normal,
        }
    }
}

/// Exact inline carrier for a legacy Mirror stitch tolerance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignMirrorScopeTolerance {
    /// Fixed scalar-lane marker preceding the tolerance value.
    pub marker: u32,
    /// Byte offset of the first scalar-lane marker.
    pub marker_offset: u64,
    /// Byte offset of the repeated scalar-lane marker, when this generation
    /// carries the marker twice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeated_marker_offset: Option<u64>,
    /// First marked reference in the scalar lane.
    pub first_reference: u32,
    /// Byte offset of the first marked reference.
    pub first_reference_offset: u64,
    /// Second marked reference in the scalar lane.
    pub second_reference: u32,
    /// Byte offset of the second marked reference.
    pub second_reference_offset: u64,
}

/// Exact fixed scalar lanes carried by a Chamfer scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesignFixedChamferParameters {
    /// One equal setback distance applies to both incident faces.
    EqualDistance {
        /// Equal setback distance.
        distance: DesignFixedChamferDistance,
    },
    /// The two incident faces have independently oriented setback distances.
    TwoDistances {
        /// Setback on the first incident face.
        first: DesignFixedChamferDistance,
        /// Setback on the second incident face.
        second: DesignFixedChamferDistance,
    },
}

/// One fixed Chamfer distance lane and its source provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignFixedChamferDistance {
    /// Positive distance in source centimetres.
    pub value: f64,
    /// Referenced scalar record.
    pub record_index: u32,
    /// Byte offset of the scalar.
    pub value_offset: u64,
}

/// Exact construction carried by a Revolve, Loft, or Sweep scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesignPathFeatureConstruction {
    /// One-sided fixed-angle revolution result operation.
    Revolve(DesignRevolveConstruction),
    /// Loft result operation.
    Loft(DesignLoftConstruction),
    /// Sweep result operation and fixed dimension lanes.
    Sweep(DesignSweepConstruction),
    /// Generated-section Pipe result and fixed dimension lanes.
    Pipe(DesignPipeConstruction),
}

/// Fixed construction of a `Revolve` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignRevolveConstructionWire", into = "DesignRevolveConstructionWire")]
pub struct DesignRevolveConstruction {
    /// Boolean result operation.
    pub operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    pub operation_offset: u64,
    /// Positive angular travel in radians.
    pub angle: f64,
    /// Referenced angular-travel scalar record.
    pub angle_record_index: u32,
    /// Byte offset of the angular-travel scalar.
    pub angle_offset: u64,
    /// Zero-valued opposite-side angle scalar record, when serialized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opposite_angle: Option<Located<u32>>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignRevolveConstructionWire {
    /// Boolean result operation.
    operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    operation_offset: u64,
    /// Positive angular travel in radians.
    angle: f64,
    /// Referenced angular-travel scalar record.
    angle_record_index: u32,
    /// Byte offset of the angular-travel scalar.
    angle_offset: u64,
    /// Zero-valued opposite-side angle scalar record, when serialized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    opposite_angle_record_index: Option<u32>,
    /// Byte offset of the opposite-side angle scalar, when serialized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    opposite_angle_offset: Option<u64>,
}

impl From<DesignRevolveConstruction> for DesignRevolveConstructionWire {
    fn from(value: DesignRevolveConstruction) -> Self {
        Self {
            operation: value.operation,
            operation_offset: value.operation_offset,
            angle: value.angle,
            angle_record_index: value.angle_record_index,
            angle_offset: value.angle_offset,
            opposite_angle_record_index: value.opposite_angle.map(|located| located.value),
            opposite_angle_offset: value.opposite_angle.map(|located| located.offset),
        }
    }
}

impl TryFrom<DesignRevolveConstructionWire> for DesignRevolveConstruction {
    type Error = String;
    fn try_from(value: DesignRevolveConstructionWire) -> Result<Self, Self::Error> {
        Ok(Self {
            operation: value.operation,
            operation_offset: value.operation_offset,
            angle: value.angle,
            angle_record_index: value.angle_record_index,
            angle_offset: value.angle_offset,
            opposite_angle: match (value.opposite_angle_record_index, value.opposite_angle_offset) {
                (None, None) => None,
                (Some(value), Some(offset)) => Some(Located { value, offset }),
                _ => return Err("opposite_angle_record_index and opposite_angle_offset must occur together".into()),
            },
        })
    }
}


/// Fixed construction of a `Loft` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignLoftConstruction {
    /// Boolean result operation.
    pub operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    pub operation_offset: u64,
}

/// Fixed construction of a `Sweep` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignSweepConstruction {
    /// Boolean result operation.
    pub operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    pub operation_offset: u64,
    /// Six scalar values in `AlongDistance`, `AgainstDistance`,
    /// `AlongRailDistance`, `AgainstRailDistance`, `TwistAngle`, and `TaperAngle` order.
    pub values: [f64; 6],
    /// Referenced scalar records in lane order.
    pub record_indexes: [u32; 6],
    /// Byte offsets of the scalar values in lane order.
    pub value_offsets: [u64; 6],
}

/// Fixed construction of a `Pipe` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignPipeConstruction {
    /// Boolean result operation.
    pub operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    pub operation_offset: u64,
    /// Section-shape selector byte.
    pub section_shape: DesignPipeSectionShape,
    /// Byte offset of the section-shape selector.
    pub section_shape_offset: u64,
    /// Whether the generated section is filled.
    pub filled: bool,
    /// Byte offset of the filled-section flag.
    pub filled_offset: u64,
    /// Four scalar values in path-fraction, reverse-path-fraction,
    /// section-size, and section-thickness order.
    pub values: [f64; 4],
    /// Referenced scalar records in lane order.
    pub record_indexes: [u32; 4],
    /// Byte offsets of the scalar values in lane order.
    pub value_offsets: [u64; 4],
}


/// Serialized prologue form of a `Combine` scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DesignCombineForm {
    /// Nine zero bytes followed by the operation at offset 20.
    Standard,
    /// Class-387 form with the operation at offset 21.
    Compact,
    /// Eighteen-zero reference form with the operation at offset 31.
    ExtendedReference,
}

/// Version identity carried by a cross-document reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignExternalVersion {
    pub property_key: Located<String>,
    pub version_urn: Located<String>,
}

/// Cross-document persistent body identity carried by a `Combine` tool selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignCombineExternalBodyIdentityWire", into = "DesignCombineExternalBodyIdentityWire")]
pub struct DesignCombineExternalBodyIdentity {
    /// Asset GUID of the enclosing body selector.
    pub selector_asset_id: String,
    /// Byte offset of `selector_asset_id`.
    pub selector_asset_id_offset: u64,
    /// Context GUID of the enclosing body selector.
    pub selector_context_id: String,
    /// Byte offset of `selector_context_id`.
    pub selector_context_id_offset: u64,
    /// Same-segment occurrence reference preceding the external body reference.
    pub occurrence_reference: u64,
    /// Byte offset of `occurrence_reference`.
    pub occurrence_reference_offset: u64,
    /// Entity reference of the body in the referenced document.
    pub external_body_reference: u64,
    /// Byte offset of `external_body_reference`.
    pub external_body_reference_offset: u64,
    /// Segment carried by the cross-document body reference.
    pub external_segment: u32,
    /// Byte offset of `external_segment`.
    pub external_segment_offset: u64,
    /// Asset GUID carried by the cross-document body reference.
    pub external_asset_id: String,
    /// Byte offset of `external_asset_id`.
    pub external_asset_id_offset: u64,
    /// Link name carried by the cross-document body reference.
    pub external_link_name: String,
    /// Byte offset of `external_link_name`.
    pub external_link_name_offset: u64,
    /// Located property key and referenced-document version identity.
    pub external_version: Option<DesignExternalVersion>,
    /// Retained u64 values around the fixed `u32 48` member in the selector tail.
    #[serde(default)]
    pub tail_values: [u64; 2],
    /// Byte offsets of `tail_values` in source order.
    #[serde(default)]
    pub tail_value_offsets: [u64; 2],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignCombineExternalBodyIdentityWire {
    /// Asset GUID of the enclosing body selector.
    selector_asset_id: String,
    /// Byte offset of `selector_asset_id`.
    selector_asset_id_offset: u64,
    /// Context GUID of the enclosing body selector.
    selector_context_id: String,
    /// Byte offset of `selector_context_id`.
    selector_context_id_offset: u64,
    /// Same-segment occurrence reference preceding the external body reference.
    occurrence_reference: u64,
    /// Byte offset of `occurrence_reference`.
    occurrence_reference_offset: u64,
    /// Entity reference of the body in the referenced document.
    external_body_reference: u64,
    /// Byte offset of `external_body_reference`.
    external_body_reference_offset: u64,
    /// Segment carried by the cross-document body reference.
    external_segment: u32,
    /// Byte offset of `external_segment`.
    external_segment_offset: u64,
    /// Asset GUID carried by the cross-document body reference.
    external_asset_id: String,
    /// Byte offset of `external_asset_id`.
    external_asset_id_offset: u64,
    /// Link name carried by the cross-document body reference.
    external_link_name: String,
    /// Byte offset of `external_link_name`.
    external_link_name_offset: u64,
    /// Optional property key preceding the version identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    external_property_key: Option<String>,
    /// Byte offset of `external_property_key` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    external_property_key_offset: Option<u64>,
    /// Optional referenced-document version identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    external_version_urn: Option<String>,
    /// Byte offset of `external_version_urn` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    external_version_urn_offset: Option<u64>,
    /// Retained u64 values around the fixed `u32 48` member in the selector tail.
    #[serde(default)]
    tail_values: [u64; 2],
    /// Byte offsets of `tail_values` in source order.
    #[serde(default)]
    tail_value_offsets: [u64; 2],
}

impl TryFrom<DesignCombineExternalBodyIdentityWire> for DesignCombineExternalBodyIdentity {
    type Error = String;
    fn try_from(wire: DesignCombineExternalBodyIdentityWire) -> Result<Self, Self::Error> {
        Ok(Self {
            selector_asset_id: wire.selector_asset_id,
            selector_asset_id_offset: wire.selector_asset_id_offset,
            selector_context_id: wire.selector_context_id,
            selector_context_id_offset: wire.selector_context_id_offset,
            occurrence_reference: wire.occurrence_reference,
            occurrence_reference_offset: wire.occurrence_reference_offset,
            external_body_reference: wire.external_body_reference,
            external_body_reference_offset: wire.external_body_reference_offset,
            external_segment: wire.external_segment,
            external_segment_offset: wire.external_segment_offset,
            external_asset_id: wire.external_asset_id,
            external_asset_id_offset: wire.external_asset_id_offset,
            external_link_name: wire.external_link_name,
            external_link_name_offset: wire.external_link_name_offset,
            external_version: match (wire.external_property_key, wire.external_property_key_offset, wire.external_version_urn, wire.external_version_urn_offset) {
                (None, None, None, None) => None,
                (Some(key), Some(key_offset), Some(urn), Some(urn_offset)) => Some(DesignExternalVersion { property_key: Located { value: key, offset: key_offset }, version_urn: Located { value: urn, offset: urn_offset } }),
                _ => return Err("external_property_key, external_property_key_offset, external_version_urn and external_version_urn_offset must occur together".into()),
            },
            tail_values: wire.tail_values,
            tail_value_offsets: wire.tail_value_offsets,
        })
    }
}

impl From<DesignCombineExternalBodyIdentity> for DesignCombineExternalBodyIdentityWire {
    fn from(record: DesignCombineExternalBodyIdentity) -> Self {
        Self {
            selector_asset_id: record.selector_asset_id,
            selector_asset_id_offset: record.selector_asset_id_offset,
            selector_context_id: record.selector_context_id,
            selector_context_id_offset: record.selector_context_id_offset,
            occurrence_reference: record.occurrence_reference,
            occurrence_reference_offset: record.occurrence_reference_offset,
            external_body_reference: record.external_body_reference,
            external_body_reference_offset: record.external_body_reference_offset,
            external_segment: record.external_segment,
            external_segment_offset: record.external_segment_offset,
            external_asset_id: record.external_asset_id,
            external_asset_id_offset: record.external_asset_id_offset,
            external_link_name: record.external_link_name,
            external_link_name_offset: record.external_link_name_offset,
            external_property_key: record.external_version.as_ref().map(|version| version.property_key.value.clone()),
            external_property_key_offset: record.external_version.as_ref().map(|version| version.property_key.offset),
            external_version_urn: record.external_version.as_ref().map(|version| version.version_urn.value.clone()),
            external_version_urn_offset: record.external_version.as_ref().map(|version| version.version_urn.offset),
            tail_values: record.tail_values,
            tail_value_offsets: record.tail_value_offsets,
        }
    }
}

/// One target or tool body selector owned by a `Combine` operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignCombineBodySelection {
    /// Body-selection record index.
    pub record_index: u32,
    /// Complete external body identity when the selector crosses a document boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_identity: Option<DesignCombineExternalBodyIdentity>,
}

/// Exact Boolean construction carried by a `Combine` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignCombineOperation {
    /// Serialized scope-prologue form.
    pub form: DesignCombineForm,
    /// Join, cut, or intersect operation.
    pub operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    pub operation_offset: u64,
    /// Whether the source operation retains its tool bodies.
    pub keep_tools: bool,
    /// Byte offset of the keep-tools Boolean.
    pub keep_tools_offset: u64,
    /// Boolean target body selector.
    pub target: DesignCombineBodySelection,
    /// Boolean tool body selectors in source order.
    pub tools: Vec<DesignCombineBodySelection>,
}

/// Thread construction form selected by the scope prefix and payload marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignThreadForm {
    /// Standard prefix, construction marker, and trailer layout.
    Standard,
    /// Compact prefix, construction marker, and trailer layout.
    Compact(Option<Located<u32>>),
    /// Direct standard prefix with the legacy compact scalar and trailer lanes.
    StandardLegacy,
    /// Compact prefix with the legacy scalar and no-reference trailer lanes.
    CompactLegacy,
}

/// Exact form and size construction carried by a `Thread` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignThreadConstructionWire", into = "DesignThreadConstructionWire")]
pub struct DesignThreadConstruction {
    /// Standard, compact, or class-specific legacy construction form.
    pub form: DesignThreadForm,
    /// Byte offset of the designation LP-UTF16 field.
    pub designation_offset: u64,
    /// Standard thread designation.
    pub designation: String,
    /// Exact nominal-size text interpreted into `nominal_size`.
    pub nominal_size_text: String,
    /// Numeric nominal size interpreted by `profile`.
    pub nominal_size: f64,
    /// Thread profile name.
    pub profile: String,
    /// Physical major diameter in Design length units.
    pub major_diameter: f64,
    /// Physical minor diameter in Design length units.
    pub minor_diameter: f64,
    /// Thread pitch in Design length units.
    pub pitch: f64,
    /// Pitch diameter in Design length units.
    pub pitch_diameter: f64,
    /// Ordered counted face-selection groups referenced by the scope.
    pub face_group_record_indices: Vec<u32>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
enum DesignThreadFormWire {
    /// Standard prefix, construction marker, and trailer layout.
    Standard,
    /// Compact prefix, construction marker, and trailer layout.
    Compact,
    /// Direct standard prefix with the legacy compact scalar and trailer lanes.
    StandardLegacy,
    /// Compact prefix with the legacy scalar and no-reference trailer lanes.
    CompactLegacy,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignThreadConstructionWire {
    /// Standard, compact, or class-specific legacy construction form.
    form: DesignThreadFormWire,
    /// Byte offset of the designation LP-UTF16 field.
    designation_offset: u64,
    /// Standard thread designation.
    designation: String,
    /// Exact nominal-size text interpreted into `nominal_size`.
    nominal_size_text: String,
    /// Numeric nominal size interpreted by `profile`.
    nominal_size: f64,
    /// Thread profile name.
    profile: String,
    /// Physical major diameter in Design length units.
    major_diameter: f64,
    /// Physical minor diameter in Design length units.
    minor_diameter: f64,
    /// Thread pitch in Design length units.
    pitch: f64,
    /// Pitch diameter in Design length units.
    pitch_diameter: f64,
    /// Record named by the reference-bearing compact trailer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trailing_reference_record_index: Option<u32>,
    /// Byte offset of `trailing_reference_record_index`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trailing_reference_offset: Option<u64>,
    /// Ordered counted face-selection groups referenced by the scope.
    face_group_record_indices: Vec<u32>,
}

impl From<DesignThreadConstruction> for DesignThreadConstructionWire {
    fn from(value: DesignThreadConstruction) -> Self {
        let (form, trailing_reference) = match value.form {
            DesignThreadForm::Standard => (DesignThreadFormWire::Standard, None),
            DesignThreadForm::Compact(reference) => (DesignThreadFormWire::Compact, reference),
            DesignThreadForm::StandardLegacy => (DesignThreadFormWire::StandardLegacy, None),
            DesignThreadForm::CompactLegacy => (DesignThreadFormWire::CompactLegacy, None),
        };
        Self {
            form,
            designation_offset: value.designation_offset,
            designation: value.designation,
            nominal_size_text: value.nominal_size_text,
            nominal_size: value.nominal_size,
            profile: value.profile,
            major_diameter: value.major_diameter,
            minor_diameter: value.minor_diameter,
            pitch: value.pitch,
            pitch_diameter: value.pitch_diameter,
            trailing_reference_record_index: trailing_reference.map(|located| located.value),
            trailing_reference_offset: trailing_reference.map(|located| located.offset),
            face_group_record_indices: value.face_group_record_indices,
        }
    }
}

impl TryFrom<DesignThreadConstructionWire> for DesignThreadConstruction {
    type Error = String;
    fn try_from(value: DesignThreadConstructionWire) -> Result<Self, Self::Error> {
        let reference = match (value.trailing_reference_record_index, value.trailing_reference_offset) {
            (None, None) => None,
            (Some(value), Some(offset)) => Some(Located { value, offset }),
            _ => return Err("trailing_reference_record_index and trailing_reference_offset must occur together".into()),
        };
        let form = match (value.form, reference) {
            (DesignThreadFormWire::Compact, reference) => DesignThreadForm::Compact(reference),
            (DesignThreadFormWire::Standard, None) => DesignThreadForm::Standard,
            (DesignThreadFormWire::StandardLegacy, None) => DesignThreadForm::StandardLegacy,
            (DesignThreadFormWire::CompactLegacy, None) => DesignThreadForm::CompactLegacy,
            _ => return Err("trailing_reference_record_index is only valid for compact form".into()),
        };
        Ok(Self {
            form,
            designation_offset: value.designation_offset,
            designation: value.designation,
            nominal_size_text: value.nominal_size_text,
            nominal_size: value.nominal_size,
            profile: value.profile,
            major_diameter: value.major_diameter,
            minor_diameter: value.minor_diameter,
            pitch: value.pitch,
            pitch_diameter: value.pitch_diameter,
            face_group_record_indices: value.face_group_record_indices,
        })
    }
}


/// Exact signed-angle lanes carried by a `Draft` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignDraftOperation {
    /// Signed draft angle in radians.
    pub angle: f64,
    /// Referenced draft-angle scalar record.
    pub angle_record_index: u32,
    /// Byte offset of the draft-angle scalar.
    pub angle_offset: u64,
    /// Zero-valued opposite-side angle scalar record.
    pub opposite_angle_record_index: u32,
    /// Byte offset of the opposite-side angle scalar.
    pub opposite_angle_offset: u64,
}

/// Source form for an exact solved `WorkAxis` construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum DesignWorkAxisSource {
    /// The axis carrier and two endpoint point carriers are cross-checked.
    TwoPoint {
        /// Ordered endpoint carrier record indices.
        point_record_indices: [u32; 2],
        /// Byte offsets of the first coordinate in each endpoint carrier.
        point_offsets: [u64; 2],
    },
    /// A generation-specific carrier stores the axis directly and has one
    /// additional construction-support record in the enclosing scope.
    DirectCarrier {
        /// Indexed record carrying the origin and displacement values.
        carrier_record_index: u32,
        /// Enclosing scope's second ordered construction record.
        support_record_index: u32,
    },
}

/// Exact solved construction carried by a `WorkAxis` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignWorkAxisConstruction {
    /// First construction point in model centimetres.
    pub origin: [f64; 3],
    /// Displacement from the first construction point to the second, in centimetres.
    pub displacement: [f64; 3],
    /// Byte offset of the first origin coordinate.
    pub origin_offset: u64,
    /// Byte offset of the first displacement component.
    pub displacement_offset: u64,
    /// Native record form that supplied or corroborated the axis geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<DesignWorkAxisSource>,
}

/// One source-record reference used by a `WorkPoint` construction rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignWorkPointInput {
    /// Referenced Design record index.
    pub record_index: u32,
    /// Byte offset of the serialized reference target.
    pub reference_offset: u64,
    /// Exact source carrier selected by this reference, when decoded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carrier: Option<Box<DesignWorkPointInputCarrier>>,
}

/// Exact source carrier selected by one `WorkPoint` construction input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesignWorkPointInputCarrier {
    /// Persistent edge recipe retained in the native edge-operand arena.
    EdgeRecipe {
        /// Native `DesignEdgeOperand` identifier.
        operand_id: String,
    },
    /// Persistent vertex recipe carried directly by this `WorkPoint` input.
    VertexRecipe {
        /// Exact vertex-recipe envelope.
        recipe: DesignVertexRecipe,
    },
    /// Persistent entity selection naming one `WorkPlane` scope.
    WorkPlane {
        /// Exact selection envelope and resolved `WorkPlane` scope.
        selection: DesignWorkPointPlaneSelection,
    },
    /// Direct persistent selection of one sketch point.
    SketchPoint {
        /// Exact selection envelope and resolved native sketch-point record.
        selection: DesignWorkPointSketchPointSelection,
    },
}

/// Exact persistent `vertex_recipe_data` envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignVertexRecipe {
    /// Indexed record that owns the vertex-recipe envelope.
    pub record_index: u32,
    /// Byte offset of the owning indexed-record header.
    pub byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    pub class_tag: String,
    /// Byte offset of the same-index paired header.
    pub paired_byte_offset: u64,
    /// Source per-file dynamic paired class tag.
    pub paired_class_tag: String,
    /// Indexed record containing the vertex recipe.
    pub recipe_record_index: u32,
    /// Byte offset of the vertex-recipe record header.
    pub recipe_record_byte_offset: u64,
    /// Native construction-recipe arena id.
    pub recipe_id: String,
    /// Byte offset of the recipe-specific prefix after the indexed header.
    pub recipe_prefix_offset: u64,
    /// Complete prefix before the length-prefixed recipe-family name.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub recipe_prefix_bytes: Vec<u8>,
    /// Persistent selector/reference entries decoded from the prefix.
    pub recipe_references: Vec<DesignRecipeReference>,
    /// Byte offset of the first post-name i32.
    pub recipe_program_offset: u64,
    /// Complete post-name i32 program.
    pub recipe_program: Vec<i32>,
    /// Historical topology state against which the vertex recipe was evaluated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_state_id: Option<i64>,
    /// Stable vertex slot proven by the persistent face references and solved point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_vertex_slot: Option<i64>,
    /// Identity of the indexed record closing the envelope.
    pub next_record_index: u32,
    /// Byte offset of the indexed record closing the envelope.
    pub next_byte_offset: u64,
}

/// Corner-vertex recipe carried as one member of an edge-treatment group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignEdgeTreatmentVertexOperand {
    /// Globally unique deterministic identifier for this group member.
    pub id: String,
    /// Owning edge-treatment scope record.
    pub scope_record_index: u32,
    /// Zero-based position in the scope reference table.
    pub scope_reference_ordinal: u32,
    /// Owning counted construction group.
    pub group_record_index: u32,
    /// Zero-based position in the group's member run.
    pub group_member_ordinal: u32,
    /// Exact persistent vertex-recipe envelope and resolved historical corner.
    pub recipe: DesignVertexRecipe,
}

/// Exact construction rule carried by a `WorkPlane` scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesignWorkPlaneConstruction {
    /// Plane through three persistent B-rep vertices.
    ThreePoint {
        /// Solved placement-frame record named by the scope.
        placement_record_index: u32,
        /// Persistent vertex inputs in source order.
        inputs: Box<[DesignVertexRecipe; 3]>,
    },
}

/// Exact persistent entity selection naming one `WorkPlane` scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignWorkPointPlaneSelection {
    /// Source per-file dynamic primary class tag.
    pub class_tag: String,
    /// Asset UUID qualifying the selection namespace.
    pub asset_id: String,
    /// Byte offset of the asset identifier's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// UUID of the selection context.
    pub context_id: String,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub context_id_offset: u64,
    /// Nested indexed record carrying the persistent identity.
    pub identity_record_index: u32,
    /// Byte offset of the nested identity record.
    pub identity_record_offset: u64,
    /// Serialized primary identity immediately preceding the `WorkPlane` scope.
    pub primary_identity: u64,
    /// Byte offset of the primary identity.
    pub primary_identity_offset: u64,
    /// Selected `WorkPlane` scope record index.
    pub work_plane_scope_record_index: u32,
    /// Identity of the indexed record closing the selection envelope.
    pub next_record_index: u32,
    /// Byte offset of the indexed record closing the selection envelope.
    pub next_byte_offset: u64,
}

/// Exact persistent entity selection naming one sketch point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignWorkPointSketchPointSelection {
    /// Source per-file dynamic primary class tag.
    pub class_tag: String,
    /// Asset UUID qualifying the selection namespace.
    pub asset_id: String,
    /// Byte offset of the asset identifier's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// UUID of the selection context.
    pub context_id: String,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub context_id_offset: u64,
    /// Nested indexed record carrying the persistent identity.
    pub identity_record_index: u32,
    /// Byte offset of the nested identity record.
    pub identity_record_offset: u64,
    /// Record identity of the owning Sketch entity.
    pub sketch_record_index: u32,
    /// Byte offset of the Sketch entity identity.
    pub sketch_record_index_offset: u64,
    /// Persistent identity of the selected sketch point.
    pub point_persistent_id: u64,
    /// Byte offset of the sketch-point identity.
    pub point_persistent_id_offset: u64,
    /// Native id of the decoded sketch-point record selected by this frame.
    pub point_native_id: String,
    /// Identity of the indexed record closing the selection envelope.
    pub next_record_index: u32,
    /// Byte offset of the indexed record closing the selection envelope.
    pub next_byte_offset: u64,
}

/// Construction rule and exact input arity carried by a `WorkPoint` point-data record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesignWorkPointRule {
    /// Center of one selected circular edge.
    CircleCenter {
        /// Selected circular-edge carrier.
        input: DesignWorkPointInput,
    },
    /// Intersection of two selected edges.
    TwoEdgeIntersection {
        /// Selected edge carriers in source order.
        inputs: [DesignWorkPointInput; 2],
    },
    /// Intersection of three selected planes.
    ThreePlaneIntersection {
        /// Selected plane carriers in source order.
        inputs: [DesignWorkPointInput; 3],
    },
    /// One selected B-rep vertex.
    Vertex {
        /// Selected vertex carrier.
        input: DesignWorkPointInput,
    },
    /// Intersection of one selected edge and one selected plane, in source order.
    EdgePlaneIntersection {
        /// Edge and plane carriers in serialized order.
        inputs: [DesignWorkPointInput; 2],
    },
    /// Point at a specified distance along one selected edge.
    DistanceOnEdge {
        /// Selected edge carrier.
        input: DesignWorkPointInput,
    },
    /// Rule code whose operation semantics or input arity is not assigned.
    Native {
        /// Serialized `refType` value.
        reference_type: u32,
        /// Counted input-reference run in source order.
        inputs: Vec<DesignWorkPointInput>,
    },
}

impl DesignWorkPointRule {
    pub(crate) fn from_serialized(reference_type: u32, inputs: Vec<DesignWorkPointInput>) -> Self {
        match (reference_type, inputs.as_slice()) {
            (5, [input]) => Self::CircleCenter {
                input: input.clone(),
            },
            (7, [first, second]) => Self::TwoEdgeIntersection {
                inputs: [first.clone(), second.clone()],
            },
            (8, [first, second, third]) => Self::ThreePlaneIntersection {
                inputs: [first.clone(), second.clone(), third.clone()],
            },
            (10, [input]) => Self::Vertex {
                input: input.clone(),
            },
            (14, [first, second]) => Self::EdgePlaneIntersection {
                inputs: [first.clone(), second.clone()],
            },
            (20, [input]) => Self::DistanceOnEdge {
                input: input.clone(),
            },
            _ => Self::Native {
                reference_type,
                inputs,
            },
        }
    }

    /// Return the serialized `refType` value.
    pub fn reference_type(&self) -> u32 {
        match self {
            Self::CircleCenter { .. } => 5,
            Self::TwoEdgeIntersection { .. } => 7,
            Self::ThreePlaneIntersection { .. } => 8,
            Self::Vertex { .. } => 10,
            Self::EdgePlaneIntersection { .. } => 14,
            Self::DistanceOnEdge { .. } => 20,
            Self::Native { reference_type, .. } => *reference_type,
        }
    }

    /// Return the source input references in serialized order.
    pub fn inputs(&self) -> &[DesignWorkPointInput] {
        match self {
            Self::CircleCenter { input }
            | Self::Vertex { input }
            | Self::DistanceOnEdge { input } => std::slice::from_ref(input),
            Self::TwoEdgeIntersection { inputs } | Self::EdgePlaneIntersection { inputs } => inputs,
            Self::ThreePlaneIntersection { inputs } => inputs,
            Self::Native { inputs, .. } => inputs,
        }
    }

    pub(crate) fn inputs_mut(&mut self) -> &mut [DesignWorkPointInput] {
        match self {
            Self::CircleCenter { input }
            | Self::Vertex { input }
            | Self::DistanceOnEdge { input } => std::slice::from_mut(input),
            Self::TwoEdgeIntersection { inputs } | Self::EdgePlaneIntersection { inputs } => inputs,
            Self::ThreePlaneIntersection { inputs } => inputs,
            Self::Native { inputs, .. } => inputs,
        }
    }

    pub(crate) fn carriers_are_compatible(&self) -> bool {
        let is_edge = |input: &DesignWorkPointInput| {
            input.carrier.as_deref().is_none_or(|carrier| {
                matches!(carrier, DesignWorkPointInputCarrier::EdgeRecipe { .. })
            })
        };
        let is_vertex = |input: &DesignWorkPointInput| {
            input.carrier.as_deref().is_none_or(|carrier| {
                matches!(
                    carrier,
                    DesignWorkPointInputCarrier::VertexRecipe { .. }
                        | DesignWorkPointInputCarrier::SketchPoint { .. }
                )
            })
        };
        let is_plane = |input: &DesignWorkPointInput| {
            input.carrier.as_deref().is_none_or(|carrier| {
                matches!(carrier, DesignWorkPointInputCarrier::WorkPlane { .. })
            })
        };
        match self {
            Self::CircleCenter { input } | Self::DistanceOnEdge { input } => is_edge(input),
            Self::TwoEdgeIntersection { inputs } => inputs.iter().all(is_edge),
            Self::ThreePlaneIntersection { inputs } => inputs.iter().all(is_plane),
            Self::Vertex { input } => is_vertex(input),
            Self::EdgePlaneIntersection { inputs } => is_edge(&inputs[0]) && is_plane(&inputs[1]),
            Self::Native { .. } => true,
        }
    }
}

/// Exact solved construction carried by a `WorkPoint` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignWorkPointConstruction {
    /// Point-data record selected by the scope.
    pub point_record_index: u32,
    /// Byte offset of the point-data record header.
    pub point_record_byte_offset: u64,
    /// Solved point in source model centimetres.
    pub position: [f64; 3],
    /// Byte offset of the first position coordinate.
    pub position_offset: u64,
    /// Typed construction rule and its source inputs.
    pub rule: DesignWorkPointRule,
    /// Byte offset of the serialized `refType` value.
    pub reference_type_offset: u64,
}

/// Tangent-point payload of a version-four Hole point carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignHoleTangentPoint {
    pub prefix: u8,
    pub data: Located<[f64; 3]>,
}

/// Exact point-and-direction construction carried by a `Hole` scope.
///
/// The native point carrier stores the coordinates in source centimetres and
/// the direction as a unit model-space vector. The remaining fields preserve
/// the carrier's base-level evidence so later Hole forms can bind their input
/// records without reparsing the byte stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignHoleConstructionWire", into = "DesignHoleConstructionWire")]
pub struct DesignHoleConstruction {
    /// Point-data record selected by the Hole scope.
    pub point_record_index: u32,
    /// Byte offset of the point-data record header.
    pub point_record_byte_offset: u64,
    /// Hole entry position in source model centimetres.
    pub position: [f64; 3],
    /// Byte offset of the first position coordinate.
    pub position_offset: u64,
    /// Directed drilling vector in model space.
    pub direction: [f64; 3],
    /// Byte offset of the first direction component.
    pub direction_offset: u64,
    /// Two point-construction parameters carried by the point-data base level.
    pub point_parameters: [f64; 2],
    /// Byte offsets of the two point-construction parameters.
    pub point_parameter_offsets: [u64; 2],
    /// `refType` construction rule carried by the point-data record.
    pub reference_type: u32,
    /// Byte offset of `reference_type`.
    pub reference_type_offset: u64,
    /// Version-four tangent-point data with its prefix and source location.
    pub tangent_point_data: Option<DesignHoleTangentPoint>,
    /// Located targets of the counted input-reference run.
    pub input_records: Vec<Located<u32>>,
    /// Direct persistent face selection carried by the Hole scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_selection: Option<DesignHoleFaceSelection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignHoleConstructionWire {
    /// Point-data record selected by the Hole scope.
    point_record_index: u32,
    /// Byte offset of the point-data record header.
    point_record_byte_offset: u64,
    /// Hole entry position in source model centimetres.
    position: [f64; 3],
    /// Byte offset of the first position coordinate.
    position_offset: u64,
    /// Directed drilling vector in model space.
    direction: [f64; 3],
    /// Byte offset of the first direction component.
    direction_offset: u64,
    /// Two point-construction parameters carried by the point-data base level.
    point_parameters: [f64; 2],
    /// Byte offsets of the two point-construction parameters.
    point_parameter_offsets: [u64; 2],
    /// `refType` construction rule carried by the point-data record.
    reference_type: u32,
    /// Byte offset of `reference_type`.
    reference_type_offset: u64,
    /// Tangent-point data carried by the version-four point-data base level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tangent_point_data: Option<[f64; 3]>,
    /// Serialized byte immediately before the version-four tangent-point data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tangent_point_data_prefix: Option<u8>,
    /// Byte offset of the first version-four tangent-point component.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tangent_point_data_offset: Option<u64>,
    /// Record indices of the counted input-reference run.
    input_record_indices: Vec<u32>,
    /// Byte offsets of the input-reference targets.
    input_record_offsets: Vec<u64>,
    /// Direct persistent face selection carried by the Hole scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    face_selection: Option<DesignHoleFaceSelection>,
}

impl TryFrom<DesignHoleConstructionWire> for DesignHoleConstruction {
    type Error = String;
    fn try_from(wire: DesignHoleConstructionWire) -> Result<Self, Self::Error> {
        if wire.input_record_indices.len() != wire.input_record_offsets.len() {
            return Err("input_record_indices and input_record_offsets must have equal lengths".into());
        }
        Ok(Self {
            point_record_index: wire.point_record_index,
            point_record_byte_offset: wire.point_record_byte_offset,
            position: wire.position,
            position_offset: wire.position_offset,
            direction: wire.direction,
            direction_offset: wire.direction_offset,
            point_parameters: wire.point_parameters,
            point_parameter_offsets: wire.point_parameter_offsets,
            reference_type: wire.reference_type,
            reference_type_offset: wire.reference_type_offset,
            tangent_point_data: match (wire.tangent_point_data, wire.tangent_point_data_prefix, wire.tangent_point_data_offset) {
                (None, None, None) => None,
                (Some(value), Some(prefix), Some(offset)) => Some(DesignHoleTangentPoint { prefix, data: Located { value, offset } }),
                _ => return Err("tangent_point_data, tangent_point_data_prefix and tangent_point_data_offset must occur together".into()),
            },
            input_records: wire.input_record_indices.into_iter().zip(wire.input_record_offsets).map(|(value, offset)| Located { value, offset }).collect(),
            face_selection: wire.face_selection,
        })
    }
}

impl From<DesignHoleConstruction> for DesignHoleConstructionWire {
    fn from(record: DesignHoleConstruction) -> Self {
        Self {
            point_record_index: record.point_record_index,
            point_record_byte_offset: record.point_record_byte_offset,
            position: record.position,
            position_offset: record.position_offset,
            direction: record.direction,
            direction_offset: record.direction_offset,
            point_parameters: record.point_parameters,
            point_parameter_offsets: record.point_parameter_offsets,
            reference_type: record.reference_type,
            reference_type_offset: record.reference_type_offset,
            tangent_point_data: record.tangent_point_data.as_ref().map(|tangent| tangent.data.value),
            tangent_point_data_prefix: record.tangent_point_data.as_ref().map(|tangent| tangent.prefix),
            tangent_point_data_offset: record.tangent_point_data.as_ref().map(|tangent| tangent.data.offset),
            input_record_indices: record.input_records.iter().map(|reference| reference.value).collect(),
            input_record_offsets: record.input_records.iter().map(|reference| reference.offset).collect(),
            face_selection: record.face_selection,
        }
    }
}

/// Direct persistent face selection carried by a `Hole` scope.
///
/// Hole selections are scope references rather than construction-group
/// members. Their envelope is the same persistent entity-selection grammar
/// used by grouped operands, but the scope owns the selection directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignHoleFaceSelectionWire", into = "DesignHoleFaceSelectionWire")]
pub struct DesignHoleFaceSelection {
    /// Indexed record carrying the persistent selection envelope.
    pub record_index: u32,
    /// Byte offset of the selection envelope header.
    pub byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    pub class_tag: String,
    /// Asset UUID qualifying the selection namespace.
    pub asset_id: String,
    /// Byte offset of the asset identifier's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// UUID of the selection context.
    pub context_id: String,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub context_id_offset: u64,
    /// Nested indexed record carrying the persistent identity.
    pub identity_record_index: u32,
    /// Byte offset of the nested identity record.
    pub identity_record_offset: u64,
    /// Primary persistent identity of the selected face.
    pub primary_identity: u64,
    /// Byte offset of the primary persistent identity.
    pub primary_identity_offset: u64,
    /// Optional secondary persistent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_identity: Option<Located<u64>>,
    /// Optional secondary identity of a selected Sketch curve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve_secondary_identity: Option<Located<u64>>,
    /// History-qualified face proofs for the primary identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub historical_face_candidates: Vec<DesignEntitySelectionFaceCandidate>,
    /// Indexed record immediately following the selection envelope.
    pub next_record_index: u32,
    /// Byte offset of the following indexed record.
    pub next_byte_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignHoleFaceSelectionWire {
    /// Indexed record carrying the persistent selection envelope.
    record_index: u32,
    /// Byte offset of the selection envelope header.
    byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    class_tag: String,
    /// Asset UUID qualifying the selection namespace.
    asset_id: String,
    /// Byte offset of the asset identifier's UTF-16LE code units.
    asset_id_offset: u64,
    /// UUID of the selection context.
    context_id: String,
    /// Byte offset of the context UUID's UTF-16LE code units.
    context_id_offset: u64,
    /// Nested indexed record carrying the persistent identity.
    identity_record_index: u32,
    /// Byte offset of the nested identity record.
    identity_record_offset: u64,
    /// Primary persistent identity of the selected face.
    primary_identity: u64,
    /// Byte offset of the primary persistent identity.
    primary_identity_offset: u64,
    /// Optional secondary persistent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secondary_identity: Option<u64>,
    /// Byte offset of the optional secondary persistent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secondary_identity_offset: Option<u64>,
    /// Optional secondary identity of a selected Sketch curve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curve_secondary_identity: Option<u64>,
    /// Byte offset of the optional Sketch-curve secondary identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curve_secondary_identity_offset: Option<u64>,
    /// History-qualified face proofs for the primary identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    historical_face_candidates: Vec<DesignEntitySelectionFaceCandidate>,
    /// Indexed record immediately following the selection envelope.
    next_record_index: u32,
    /// Byte offset of the following indexed record.
    next_byte_offset: u64,
}

impl TryFrom<DesignHoleFaceSelectionWire> for DesignHoleFaceSelection {
    type Error = String;
    fn try_from(wire: DesignHoleFaceSelectionWire) -> Result<Self, Self::Error> {
        Ok(Self {
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag,
            asset_id: wire.asset_id,
            asset_id_offset: wire.asset_id_offset,
            context_id: wire.context_id,
            context_id_offset: wire.context_id_offset,
            identity_record_index: wire.identity_record_index,
            identity_record_offset: wire.identity_record_offset,
            primary_identity: wire.primary_identity,
            primary_identity_offset: wire.primary_identity_offset,
            secondary_identity: Located::from_wire(wire.secondary_identity, wire.secondary_identity_offset, "secondary_identity")?,
            curve_secondary_identity: Located::from_wire(wire.curve_secondary_identity, wire.curve_secondary_identity_offset, "curve_secondary_identity")?,
            historical_face_candidates: wire.historical_face_candidates,
            next_record_index: wire.next_record_index,
            next_byte_offset: wire.next_byte_offset,
        })
    }
}

impl From<DesignHoleFaceSelection> for DesignHoleFaceSelectionWire {
    fn from(record: DesignHoleFaceSelection) -> Self {
        Self {
            record_index: record.record_index,
            byte_offset: record.byte_offset,
            class_tag: record.class_tag,
            asset_id: record.asset_id,
            asset_id_offset: record.asset_id_offset,
            context_id: record.context_id,
            context_id_offset: record.context_id_offset,
            identity_record_index: record.identity_record_index,
            identity_record_offset: record.identity_record_offset,
            primary_identity: record.primary_identity,
            primary_identity_offset: record.primary_identity_offset,
            secondary_identity: record.secondary_identity.map(|identity| identity.value),
            secondary_identity_offset: record.secondary_identity.map(|identity| identity.offset),
            curve_secondary_identity: record.curve_secondary_identity.map(|identity| identity.value),
            curve_secondary_identity_offset: record.curve_secondary_identity.map(|identity| identity.offset),
            historical_face_candidates: record.historical_face_candidates,
            next_record_index: record.next_record_index,
            next_byte_offset: record.next_byte_offset,
        }
    }
}

macro_rules! design_feature_kinds {
    (data { $($variant:ident => $lit:literal : $payload:ty),+ $(,)? }
     names { $($unit:ident => $unit_lit:literal),+ $(,)? }) => {
        /// Source feature-family name stored on a parameter scope.
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        #[cfg_attr(feature = "schema", schemars(with = "String"))]
        #[serde(from = "String", into = "String")]
        pub enum DesignFeatureKind {
            $($variant,)+
            $($unit,)+
            /// Source name without a specialized construction grammar.
            Native(std::sync::Arc<str>),
        }

        impl DesignFeatureKind {
            /// Source spelling written on the wire.
            pub fn as_str(&self) -> &str {
                match self {
                    $(Self::$variant => $lit,)+
                    $(Self::$unit => $unit_lit,)+
                    Self::Native(name) => name,
                }
            }

            /// Whether the source spelling is empty.
            pub fn is_empty(&self) -> bool { self.as_str().is_empty() }
        }

        impl From<String> for DesignFeatureKind {
            fn from(name: String) -> Self {
                match name.as_str() {
                    $($lit => Self::$variant,)+
                    $($unit_lit => Self::$unit,)+
                    _ => Self::Native(name.into()),
                }
            }
        }

        impl From<DesignFeatureKind> for String {
            fn from(kind: DesignFeatureKind) -> Self { kind.as_str().to_owned() }
        }

        impl std::fmt::Display for DesignFeatureKind {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        /// Source family and its construction carrier. An independently decoded
        /// scope envelope can lack specialized construction data.
        #[derive(Debug, Clone, PartialEq)]
        pub enum DesignScopePayload {
            $($variant($payload),)+
            $($unit,)+
            /// Source name without a specialized construction grammar.
            Native(std::sync::Arc<str>),
        }

        impl From<DesignFeatureKind> for DesignScopePayload {
            fn from(kind: DesignFeatureKind) -> Self {
                match kind {
                    $(DesignFeatureKind::$variant => Self::$variant(Default::default()),)+
                    $(DesignFeatureKind::$unit => Self::$unit,)+
                    DesignFeatureKind::Native(name) => Self::Native(name),
                }
            }
        }

        impl DesignScopePayload {
            fn kind(&self) -> DesignFeatureKind {
                match self {
                    $(Self::$variant(_) => DesignFeatureKind::$variant,)+
                    $(Self::$unit => DesignFeatureKind::$unit,)+
                    Self::Native(name) => DesignFeatureKind::Native(name.clone()),
                }
            }

            fn kind_name(&self) -> &str {
                match self {
                    $(Self::$variant(_) => $lit,)+
                    $(Self::$unit => $unit_lit,)+
                    Self::Native(name) => name,
                }
            }
        }
    };
}

design_feature_kinds! {
    data {
        Sketch => "Sketch": Option<DesignSketchEntityBinding>,
        Esquisse => "Esquisse": Option<DesignSketchEntityBinding>,
        Skizze => "Skizze": Option<DesignSketchEntityBinding>,
        Esboco => "Esboço": Option<DesignSketchEntityBinding>,
        Assemble => "Assemble": Option<DesignAssemblyAlignment>,
        AsBuilt => "As-built": Option<DesignAssemblyAlignment>,
        Extrude => "Extrude": Option<DesignExtrudeScope>,
        Extrusion => "Extrusion": Option<DesignExtrudeScope>,
        Extrusao => "Extrusão": Option<DesignExtrudeScope>,
        Fillet => "Fillet": Option<DesignFixedFilletParameters>,
        Conge => "Congé": Option<DesignFixedFilletParameters>,
        Abrundung => "Abrundung": Option<DesignFixedFilletParameters>,
        Arredondamento => "Arredondamento": Option<DesignFixedFilletParameters>,
        Chamfer => "Chamfer": Option<DesignFixedChamferParameters>,
        Chanfrein => "Chanfrein": Option<DesignFixedChamferParameters>,
        Combine => "Combine": Option<DesignCombineOperation>,
        Draft => "Draft": Option<DesignDraftOperation>,
        CPattern => "C-Pattern": Option<DesignCircularPatternConstruction>,
        CircularPattern => "Circular Pattern": Option<DesignCircularPatternConstruction>,
        ReseauC => "Réseau C": Option<DesignCircularPatternConstruction>,
        RPattern => "R-Pattern": Option<DesignRectangularPatternConstruction>,
        RectangularPattern => "Rectangular Pattern": Option<DesignRectangularPatternConstruction>,
        Mirror => "Mirror": Option<DesignMirrorConstruction>,
        SymetrieMiroir => "Symétrie miroir": Option<DesignMirrorConstruction>,
        Move => "Move": Option<DesignMoveOperation>,
        OffsetFaces => "OffsetFaces": Option<DesignOffsetFacesOperation>,
        DecalerLesFaces => "DécalerLesFaces": Option<DesignOffsetFacesOperation>,
        Revolve => "Revolve": Option<DesignRevolveConstruction>,
        Shell => "Shell": Option<DesignShellOperation>,
        Schale => "Schale": Option<DesignShellOperation>,
        Thicken => "Thicken": Option<DesignThickenOperation>,
        SpirePrimitive => "SpirePrimitive": Option<DesignCoilScope>,
        CoilPrimitive => "CoilPrimitive": Option<DesignCoilScope>,
        Loft => "Loft": Option<DesignLoftConstruction>,
        Sweep => "Sweep": Option<DesignSweepScope>,
        Pipe => "Pipe": Option<DesignPipeConstruction>,
        SurfacePatch => "SurfacePatch": Vec<DesignSurfacePatchBoundary>,
        SurfaceExtend => "SurfaceExtend": Option<DesignSurfaceExtendOperation>,
        SurfaceOffset => "SurfaceOffset": Option<DesignSurfaceOffsetOperation>,
        SurfaceRuled => "SurfaceRuled": Option<DesignRuledSurfaceOperation>,
        Hole => "Hole": Option<DesignHoleConstruction>,
        Scale => "Scale": Option<DesignScaleOperation>,
        Massstab => "Maßstab": Option<DesignScaleOperation>,
        Thread => "Thread": Option<DesignThreadConstruction>,
        EdgeFlange => "EdgeFlange": Option<DesignEdgeFlangeOperation>,
        Hem => "Hem": Option<DesignHemOperation>,
        BaseFlange => "BaseFlange": Option<DesignBaseFlangeScope>,
        ComponentInsert => "Component Insert": Option<DesignComponentInsertConstruction>,
        CopyPaste => "CopyPaste": Option<DesignCopyPasteComponentOperation>,
        JointOrigin => "JointOrigin": Option<DesignJointOriginTransform>,
        WorkPlane => "WorkPlane": Option<DesignWorkPlaneTransform>,
        WorkAxis => "WorkAxis": Option<DesignWorkAxisConstruction>,
        WorkPoint => "WorkPoint": Option<DesignWorkPointConstruction>,
        DerivedInstance => "DerivedInstance": Option<DesignDerivedInstanceConstruction>,
        SurfaceStitch => "SurfaceStitch": Option<DesignSurfaceStitchOperation>,
        BaseFeature => "Base Feature": Option<DesignBaseFeatureConstruction>,
        CopyPasteBodies => "CopyPasteBodies": Option<DesignCopyPasteBodiesOperation>,
        SpherePrimitive => "SpherePrimitive": Option<DesignSpherePrimitive>,
        TorusPrimitive => "TorusPrimitive": Option<DesignTorusPrimitive>,
        BoxPrimitive => "BoxPrimitive": Option<DesignBoxPrimitive>,
        CylinderPrimitive => "CylinderPrimitive": Option<DesignCylinderPrimitive>,
    }
    names {
        ReplaceFace => "ReplaceFace",
        SurfaceTrim => "SurfaceTrim",
        BoundaryFill => "BoundaryFill",
        Split => "Split",
        Canvas => "Canvas",
        Decal => "Decal",
        BaseMeshFeature => "Base Mesh Feature",
        CustomFeature => "CustomFeature",
        Form => "Form",
        SplitFace => "SplitFace",
        DeleteFace => "DeleteFace",
        SurfaceDeleteFace => "SurfaceDeleteFace",
        RemoveBody => "RemoveBody",
        Face => "Face",
    }
}

/// Rejected CADIR payload that names more than one family or disagrees with `kind`.
#[derive(Debug)]
pub(crate) struct DesignParameterScopePayloadError(String);

impl std::fmt::Display for DesignParameterScopePayloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DesignParameterScopePayloadError {}

/// Indexed sketch or construction-operation record that scopes parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "DesignParameterScopeSerde"))]
#[serde(
    try_from = "DesignParameterScopeSerde",
    into = "DesignParameterScopeSerde"
)]
pub struct DesignParameterScope {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the primary indexed record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: String,
    /// Shared logical record identity.
    pub record_index: u32,
    /// Byte length from the primary header to the paired header.
    pub frame_length: u64,
    /// Byte offset of the kind's UTF-16LE code units.
    pub kind_offset: u64,
    /// One-based ordinal among scopes of the same feature family.
    pub feature_ordinal: u32,
    /// Byte offset of `feature_ordinal`.
    pub feature_ordinal_offset: u64,
    /// ASM delta-state identity produced by this scope, when active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_state_id: Option<i64>,
    /// Byte offset of the encoded history-state identity or null sentinel.
    pub history_state_id_offset: u64,
    /// ASM delta-state identity immediately preceding this scope, when active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_history_state_id: Option<i64>,
    /// Byte offset of the encoded preceding-state identity, when present.
    pub previous_history_state_id_offset: Option<u64>,
    /// Byte offset of the ordered reference-table count.
    pub reference_count_offset: u64,
    /// Ordered indexed-record references carried by the scope.
    pub reference_members: Vec<u32>,
    /// Byte offsets parallel to `reference_members`.
    pub reference_member_offsets: Vec<u64>,
    /// Family-specific construction records.
    pub payload: DesignScopePayload,
    /// Reference members whose records open a construction-operand group the
    /// group grammar does not close.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unclosed_construction_operand_groups: Vec<u32>,
    /// Per-file dynamic class tag of the paired header.
    pub paired_class_tag: String,
    /// Byte offset of the paired indexed record header.
    pub paired_byte_offset: u64,
}

/// Wire form of [`DesignParameterScope`] with the historical flat field set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignParameterScopeSerde {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the primary indexed record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: String,
    /// Shared logical record identity.
    pub record_index: u32,
    /// Byte length from the primary header to the paired header.
    pub frame_length: u64,
    /// Source feature-family name.
    pub kind: DesignFeatureKind,
    /// Byte offset of the kind's UTF-16LE code units.
    pub kind_offset: u64,
    /// Extrude prologue, fixed parameters, and profile.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "extrude_scope_is_absent")]
    #[serde(deserialize_with = "deserialize_flattened_scope")]
    pub extrude: Option<DesignExtrudeScope>,
    /// Coil discriminators, placement, and transform.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "coil_scope_is_absent")]
    #[serde(deserialize_with = "deserialize_flattened_scope")]
    pub coil: Option<DesignCoilScope>,
    /// One-based ordinal among scopes of the same feature family.
    pub feature_ordinal: u32,
    /// Byte offset of `feature_ordinal`.
    pub feature_ordinal_offset: u64,
    /// ASM delta-state identity produced by this scope, when active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_state_id: Option<i64>,
    /// Byte offset of the encoded history-state identity or null sentinel.
    pub history_state_id_offset: u64,
    /// ASM delta-state identity immediately preceding this scope, when active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_history_state_id: Option<i64>,
    /// Byte offset of the encoded preceding-state identity, when present.
    #[serde(
        default,
        serialize_with = "serialize_absent_u64_offset",
        deserialize_with = "deserialize_absent_u64_offset"
    )]
    pub previous_history_state_id_offset: Option<u64>,
    /// Byte offset of the ordered reference-table count.
    pub reference_count_offset: u64,
    /// Ordered indexed-record references carried by the scope.
    pub reference_members: Vec<u32>,
    /// Byte offsets parallel to `reference_members`.
    pub reference_member_offsets: Vec<u64>,
    /// Exact solid-primitive construction carried by this scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solid_primitive: Option<DesignSolidPrimitive>,
    /// Exact fixed-form construction carried by a direct-face scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct_face_operation: Option<DesignDirectFaceOperation>,
    /// Exact rigid transform carried by a Move scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub move_operation: Option<DesignMoveOperation>,
    /// Exact uniform body-scale construction carried by a Scale scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_operation: Option<DesignScaleOperation>,
    /// Exact tolerance and setting-record references carried by a `SurfaceStitch` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_stitch_operation: Option<DesignSurfaceStitchOperation>,
    /// Exact distance, method, and boundary records carried by a `SurfaceExtend` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_extend_operation: Option<DesignSurfaceExtendOperation>,
    /// Exact distance and boundary records carried by a `SurfaceOffset` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_offset_operation: Option<DesignSurfaceOffsetOperation>,
    /// Exact mode, parameter, and selection records carried by a `SurfaceRuled` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ruled_surface_operation: Option<DesignRuledSurfaceOperation>,
    /// BaseFlange operation and sketch profile.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "base_flange_scope_is_absent")]
    #[serde(deserialize_with = "deserialize_flattened_scope")]
    pub base_flange: Option<DesignBaseFlangeScope>,
    /// Per-boundary-component settings carried by a `SurfacePatch` scope, in
    /// scope reference order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surface_patch_boundaries: Vec<DesignSurfacePatchBoundary>,
    /// Exact edge, parameter, and settings records carried by an `EdgeFlange` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_flange_operation: Option<DesignEdgeFlangeOperation>,
    /// Exact edge, parameter, and settings records carried by a `Hem` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hem_operation: Option<DesignHemOperation>,

    /// Exact fixed scalar lanes carried by a Fillet scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_fillet_parameters: Option<DesignFixedFilletParameters>,
    /// Exact fixed scalar lane carried by an equal-distance Chamfer scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_chamfer_parameters: Option<DesignFixedChamferParameters>,
    /// Path-feature construction and Sweep sketch profile.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "path_feature_scope_is_absent")]
    #[serde(deserialize_with = "deserialize_flattened_scope")]
    pub path_feature: Option<DesignPathFeatureWire>,
    /// Exact Boolean construction carried by a `Combine` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combine_operation: Option<DesignCombineOperation>,
    /// Exact form and size construction carried by a `Thread` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_construction: Option<DesignThreadConstruction>,
    /// Exact signed-angle construction carried by a `Draft` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_operation: Option<DesignDraftOperation>,
    /// Exact construction carried by a circular-pattern scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub circular_pattern_construction: Option<DesignCircularPatternConstruction>,
    /// Exact scalar lanes carried by a rectangular-pattern scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rectangular_pattern_construction: Option<DesignRectangularPatternConstruction>,
    /// Exact alignment scalars carried by an `Assemble` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly_alignment: Option<DesignAssemblyAlignment>,
    /// Exact external-occurrence construction carried by a `Component Insert` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component_insert_construction: Option<DesignComponentInsertConstruction>,
    /// Exact local-occurrence construction carried by a `DerivedInstance` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_instance_construction: Option<DesignDerivedInstanceConstruction>,
    /// Exact local-component construction carried by a legacy `CopyPaste` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_paste_component_operation: Option<DesignCopyPasteComponentOperation>,
    /// Exact construction carried by a Mirror scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror_construction: Option<DesignMirrorConstruction>,
    /// Exact source-to-copy body mapping carried by a `CopyPasteBodies` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_paste_bodies_operation: Option<DesignCopyPasteBodiesOperation>,
    /// Exact result-body references carried by a `Base Feature` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_feature_construction: Option<DesignBaseFeatureConstruction>,
    /// Exact row-major local-to-model frame carried by a `WorkPlane` scope.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_work_plane_frame")]
    pub work_plane_frame: Option<DesignWorkPlaneTransform>,
    /// Exact two-point construction carried by a `WorkAxis` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_axis_construction: Option<DesignWorkAxisConstruction>,
    /// Exact row-major local-to-model frame owned by a `JointOrigin` scope.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_joint_origin_frame")]
    pub joint_origin_frame: Option<DesignJointOriginTransform>,

    /// Exact solved construction carried by a `WorkPoint` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_point_construction: Option<DesignWorkPointConstruction>,
    /// Reference members whose records open a construction-operand group the
    /// group grammar does not close.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unclosed_construction_operand_groups: Vec<u32>,
    /// Exact point-and-direction construction carried by a `Hole` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hole_construction: Option<DesignHoleConstruction>,

    /// Sketch-module entity bound to this sketch scope.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_sketch_entity")]
    pub sketch_entity: Option<DesignSketchEntityBinding>,
    /// Per-file dynamic class tag of the paired header.
    pub paired_class_tag: String,
    /// Byte offset of the paired indexed record header.
    pub paired_byte_offset: u64,
}

// Deserialize the payload itself: flattened Option<T> suppresses T's errors.
fn deserialize_flattened_scope<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
struct WorkPlaneFrameWire {
    work_plane_transform: Option<[[f64; 4]; 4]>,
    work_plane_transform_offset: Option<u64>,
    work_plane_reference: Option<u32>,
    work_plane_reference_offset: Option<u64>,
    work_plane_construction: Option<DesignWorkPlaneConstruction>,
}

fn deserialize_work_plane_frame<'de, D>(deserializer: D) -> Result<Option<DesignWorkPlaneTransform>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let wire = WorkPlaneFrameWire::deserialize(deserializer)?;
    let reference = match (wire.work_plane_reference, wire.work_plane_reference_offset) {
        (None, None) => None,
        (Some(work_plane_reference), Some(work_plane_reference_offset)) => Some(DesignWorkPlaneReference { work_plane_reference, work_plane_reference_offset }),
        _ => return Err(serde::de::Error::custom("work_plane_reference and work_plane_reference_offset must occur together")),
    };
    match (wire.work_plane_transform, wire.work_plane_transform_offset) {
        (None, None) if reference.is_none() && wire.work_plane_construction.is_none() => Ok(None),
        (Some(work_plane_transform), Some(work_plane_transform_offset)) => Ok(Some(DesignWorkPlaneTransform {
            work_plane_transform,
            work_plane_transform_offset,
            reference,
            work_plane_construction: wire.work_plane_construction,
        })),
        _ => Err(serde::de::Error::custom("work_plane_transform and work_plane_transform_offset are required for work_plane frame data")),
    }
}

impl<'de> Deserialize<'de> for DesignWorkPlaneTransform {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_work_plane_frame(deserializer)?.ok_or_else(|| serde::de::Error::missing_field("work_plane_transform"))
    }
}

#[derive(Deserialize)]
struct JointOriginFrameWire {
    joint_origin_transform: Option<[[f64; 4]; 4]>,
    joint_origin_transform_offset: Option<u64>,
    joint_origin_reference: Option<u32>,
    joint_origin_reference_offset: Option<u64>,
}

fn deserialize_joint_origin_frame<'de, D>(deserializer: D) -> Result<Option<DesignJointOriginTransform>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let wire = JointOriginFrameWire::deserialize(deserializer)?;
    let reference = match (wire.joint_origin_reference, wire.joint_origin_reference_offset) {
        (None, None) => None,
        (Some(joint_origin_reference), Some(joint_origin_reference_offset)) => Some(DesignJointOriginReference { joint_origin_reference, joint_origin_reference_offset }),
        _ => return Err(serde::de::Error::custom("joint_origin_reference and joint_origin_reference_offset must occur together")),
    };
    match (wire.joint_origin_transform, wire.joint_origin_transform_offset) {
        (None, None) if reference.is_none() => Ok(None),
        (Some(joint_origin_transform), Some(joint_origin_transform_offset)) => Ok(Some(DesignJointOriginTransform {
            joint_origin_transform,
            joint_origin_transform_offset,
            reference,
        })),
        _ => Err(serde::de::Error::custom("joint_origin_transform and joint_origin_transform_offset are required for joint_origin frame data")),
    }
}

impl<'de> Deserialize<'de> for DesignJointOriginTransform {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_joint_origin_frame(deserializer)?.ok_or_else(|| serde::de::Error::missing_field("joint_origin_transform"))
    }
}

#[derive(Deserialize)]
struct SketchEntityWire {
    entity_id: Option<String>,
    entity_suffix: Option<u64>,
    entity_reference_offset: Option<u64>,
}

fn deserialize_sketch_entity<'de, D>(deserializer: D) -> Result<Option<DesignSketchEntityBinding>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let wire = SketchEntityWire::deserialize(deserializer)?;
    match (wire.entity_id, wire.entity_suffix, wire.entity_reference_offset) {
        (None, None, None) => Ok(None),
        (Some(entity_id), Some(entity_suffix), Some(entity_reference_offset)) => Ok(Some(DesignSketchEntityBinding { entity_id, entity_suffix, entity_reference_offset })),
        _ => Err(serde::de::Error::custom("entity_id, entity_suffix, and entity_reference_offset must occur together")),
    }
}

fn base_flange_scope_is_absent(base_flange: &Option<DesignBaseFlangeScope>) -> bool {
    match base_flange {
        None => true,
        Some(base_flange) => {
            base_flange.base_flange_operation.is_none() && base_flange.base_flange_profile.is_none()
        }
    }
}

fn coil_scope_is_absent(coil: &Option<DesignCoilScope>) -> bool {
    match coil {
        None => true,
        Some(coil) => {
            coil.coil_operation.is_none()
                && coil.coil_extent.is_none()
                && coil.coil_section.is_none()
                && coil.coil_section_placement.is_none()
                && coil.coil_clockwise.is_none()
                && coil.coil_placement.is_none()
                && coil.coil_transform.is_none()
        }
    }
}

fn extrude_scope_is_absent(extrude: &Option<DesignExtrudeScope>) -> bool {
    match extrude {
        None => true,
        Some(extrude) => {
            extrude.extrude_prologue.is_none()
                && extrude.fixed_extrude_parameters.is_none()
                && extrude.extrude_profile.is_none()
        }
    }
}

fn path_feature_scope_is_absent(path_feature: &Option<DesignPathFeatureWire>) -> bool {
    match path_feature {
        None => true,
        Some(path_feature) => {
            path_feature.path_feature_construction.is_none() && path_feature.sweep_profile.is_none()
        }
    }
}

/// BaseFlange-specific records carried by a BaseFlange parameter scope.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignBaseFlangeScope {
    /// Exact profile and thickness records carried by a `BaseFlange` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_flange_operation: Option<DesignBaseFlangeOperation>,
    /// Sketch-profile operand carried by a `BaseFlange` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_flange_profile: Option<DesignSketchProfileOperand>,
}

/// Extrude-specific records carried by an Extrude parameter scope.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignExtrudeScope {
    /// Extrude fixed prologue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extrude_prologue: Option<DesignExtrudePrologue>,
    /// Exact fixed scalar lanes carried by an Extrude scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_extrude_parameters: Option<DesignFixedExtrudeParameters>,
    /// Profile operand carried by an Extrude scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extrude_profile: Option<DesignSketchProfileOperand>,
}

/// Sweep construction and its independently decoded profile operand.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignSweepScope {
    pub construction: Option<DesignSweepConstruction>,
    pub sweep_profile: Option<DesignSketchProfileOperand>,
}

impl From<DesignPathFeatureConstruction> for DesignScopePayload {
    fn from(value: DesignPathFeatureConstruction) -> Self {
        match value {
            DesignPathFeatureConstruction::Revolve(value) => Self::Revolve(Some(value)),
            DesignPathFeatureConstruction::Loft(value) => Self::Loft(Some(value)),
            DesignPathFeatureConstruction::Pipe(value) => Self::Pipe(Some(value)),
            DesignPathFeatureConstruction::Sweep(value) => Self::Sweep(Some(DesignSweepScope { construction: Some(value), sweep_profile: None })),
        }
    }
}

/// Flat wire fields for path construction and the Sweep profile.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignPathFeatureWire {
    /// Exact fixed construction carried by a Loft, Sweep, Revolve, or Pipe scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_feature_construction: Option<DesignPathFeatureConstruction>,
    /// Sketch-profile operand carried by a `Sweep` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sweep_profile: Option<DesignSketchProfileOperand>,
}

/// Coil-specific records carried by a Coil parameter scope.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignCoilScopeWire", into = "DesignCoilScopeWire")]
pub struct DesignCoilScope {
    pub coil_operation: Option<RecordedValue<DesignExtrudeOperation>>,
    pub coil_extent: Option<RecordedValue<DesignCoilExtent>>,
    pub coil_section: Option<RecordedValue<DesignCoilSection>>,
    pub coil_section_placement: Option<RecordedValue<DesignCoilSectionPlacement>>,
    pub coil_clockwise: Option<RecordedValue<bool>>,
    pub coil_placement: Option<DesignCoilPlacement>,
    pub coil_transform: Option<DesignCoilTransform>,
}

/// Coil-specific records carried by a Coil parameter scope.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignCoilScopeWire {
    /// Coil result operation from the fixed scope prologue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_operation: Option<DesignExtrudeOperation>,
    /// Byte offset of the Coil operation enum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_operation_offset: Option<u64>,
    /// Coil driving-dimension mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_extent: Option<DesignCoilExtent>,
    /// Byte offset of the Coil mode enum, when the form stores one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_extent_offset: Option<u64>,
    /// Generated Coil section family.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_section: Option<DesignCoilSection>,
    /// Byte offset of the Coil section enum, when the form stores one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_section_offset: Option<u64>,
    /// Radial placement of the generated Coil section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_section_placement: Option<DesignCoilSectionPlacement>,
    /// Byte offset of the Coil section-placement enum, when the form stores one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_section_placement_offset: Option<u64>,
    /// Whether Coil angular travel is clockwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_clockwise: Option<bool>,
    /// Byte offset of the Coil direction enum, when the form stores one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_clockwise_offset: Option<u64>,
    /// Exact placement construction carried by a compact Coil scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_placement: Option<DesignCoilPlacement>,
    /// Direct rigid placement carried by the long ten-reference Coil form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_transform: Option<DesignCoilTransform>,
}

impl TryFrom<DesignCoilScopeWire> for DesignCoilScope {
    type Error = String;
    fn try_from(wire: DesignCoilScopeWire) -> Result<Self, Self::Error> {
        Ok(Self {
            coil_operation: RecordedValue::from_wire(wire.coil_operation, wire.coil_operation_offset, "coil_operation")?,
            coil_extent: RecordedValue::from_wire(wire.coil_extent, wire.coil_extent_offset, "coil_extent")?,
            coil_section: RecordedValue::from_wire(wire.coil_section, wire.coil_section_offset, "coil_section")?,
            coil_section_placement: RecordedValue::from_wire(wire.coil_section_placement, wire.coil_section_placement_offset, "coil_section_placement")?,
            coil_clockwise: RecordedValue::from_wire(wire.coil_clockwise, wire.coil_clockwise_offset, "coil_clockwise")?,
            coil_placement: wire.coil_placement,
            coil_transform: wire.coil_transform,
        })
    }
}

impl From<DesignCoilScope> for DesignCoilScopeWire {
    fn from(value: DesignCoilScope) -> Self {
        Self {
            coil_operation: value.coil_operation.map(|field| field.value),
            coil_operation_offset: value.coil_operation.and_then(|field| field.offset),
            coil_extent: value.coil_extent.map(|field| field.value),
            coil_extent_offset: value.coil_extent.and_then(|field| field.offset),
            coil_section: value.coil_section.map(|field| field.value),
            coil_section_offset: value.coil_section.and_then(|field| field.offset),
            coil_section_placement: value.coil_section_placement.map(|field| field.value),
            coil_section_placement_offset: value.coil_section_placement.and_then(|field| field.offset),
            coil_clockwise: value.coil_clockwise.map(|field| field.value),
            coil_clockwise_offset: value.coil_clockwise.and_then(|field| field.offset),
            coil_placement: value.coil_placement,
            coil_transform: value.coil_transform,
        }
    }
}

/// Sketch-module entity named by a sketch parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignSketchEntityBinding {
    /// Full Design entity id of a sketch scope.
    pub entity_id: String,
    /// Numeric suffix of `entity_id`.
    pub entity_suffix: u64,
    /// Byte offset of the sketch entity suffix.
    pub entity_reference_offset: u64,
}

/// Explicit 16-f64 frame carried by a `WorkPlane` scope.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignWorkPlaneTransform {
    /// Exact row-major local-to-model frame.
    pub work_plane_transform: [[f64; 4]; 4],
    /// Byte offset of the explicit 16-f64 matrix.
    pub work_plane_transform_offset: u64,
    /// Construction record referenced by the frame, when present.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<DesignWorkPlaneReference>,
    /// Exact construction rule carried by this WorkPlane frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_plane_construction: Option<DesignWorkPlaneConstruction>,
}

/// Construction record named by a `WorkPlane` frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignWorkPlaneReference {
    /// Construction record referenced by the `WorkPlane` frame.
    pub work_plane_reference: u32,
    /// Byte offset of the `WorkPlane` construction reference.
    pub work_plane_reference_offset: u64,
}

/// Explicit 16-f64 frame carried by a `JointOrigin` scope.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignJointOriginTransform {
    /// Exact row-major local-to-model frame.
    pub joint_origin_transform: [[f64; 4]; 4],
    /// Byte offset of the explicit 16-f64 matrix.
    pub joint_origin_transform_offset: u64,
    /// Construction record referenced by the frame, when present.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<DesignJointOriginReference>,
}

/// Construction record named by a `JointOrigin` frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignJointOriginReference {
    /// Construction record referenced by the `JointOrigin` frame.
    pub joint_origin_reference: u32,
    /// Byte offset of the `JointOrigin` construction reference.
    pub joint_origin_reference_offset: u64,
}

/// Fixed operation records named by a `SurfaceStitch` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignSurfaceStitchOperation {
    /// Positive maximum stitched-boundary gap in centimetres.
    pub gap_tolerance: f64,
    /// Byte offset of `gap_tolerance`.
    pub gap_tolerance_offset: u64,
    /// Indexed tolerance-record identity.
    pub tolerance_record_index: u32,
    /// Indexed operation-settings record identity.
    pub settings_record_index: u32,
}

/// Geometric continuation law encoded by a `SurfaceExtend` operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DesignSurfaceExtendMethod {
    /// Continue the source surface parameterization.
    Natural,
    /// Create faces tangent to the source faces.
    Tangent,
    /// Create faces perpendicular to the source faces.
    Perpendicular,
}

/// Fixed construction records named by a `SurfaceExtend` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignSurfaceExtendOperation {
    /// Extension distance in source centimetres.
    pub distance: f64,
    /// Byte offset of `distance`.
    pub distance_offset: u64,
    /// Indexed scalar record carrying `distance`.
    pub distance_record_index: u32,
    /// Geometric continuation law.
    pub method: DesignSurfaceExtendMethod,
    /// Byte offset of the method enum.
    pub method_offset: u64,
    /// Indexed boundary-carrier record.
    pub boundary_record_index: u32,
    /// Additional indexed reference carried by the boundary tail.
    pub boundary_reference_record_index: u32,
    /// Byte offset of `boundary_reference_record_index`'s marked reference.
    pub boundary_reference_offset: u64,
    /// Ordered edge-recipe records contained by the boundary carrier.
    pub edge_record_indices: Vec<u32>,
    /// Positive modelling tolerance in source centimetres.
    pub tolerance: f64,
    /// Byte offset of `tolerance`.
    pub tolerance_offset: u64,
}

/// Source selection form named by a `SurfaceOffset` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum DesignSurfaceOffsetSupport {
    /// A boundary carrier followed by edge recipes.
    BoundaryCarrier {
        /// Source boundary-mode enum.
        boundary_mode: u32,
        /// Byte offset of `boundary_mode`.
        boundary_mode_offset: u64,
        /// Indexed boundary-carrier record.
        boundary_record_index: u32,
        /// Additional indexed reference carried by the boundary tail.
        boundary_reference_record_index: u32,
        /// Byte offset of `boundary_reference_record_index`'s marked reference.
        boundary_reference_offset: u64,
        /// Ordered edge-recipe records contained by the boundary carrier.
        edge_record_indices: Vec<u32>,
        /// Positive modelling tolerance in source centimetres.
        tolerance: f64,
        /// Byte offset of `tolerance`.
        tolerance_offset: u64,
    },
    /// Counted role-0x41 groups containing bounded-face recipes.
    FaceGroups {
        /// Ordered construction-group records named by the scope.
        group_record_indices: Vec<u32>,
    },
}

/// Fixed construction records named by a `SurfaceOffset` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignSurfaceOffsetOperation {
    /// Signed offset distance in source centimetres.
    pub distance: f64,
    /// Byte offset of `distance`.
    pub distance_offset: u64,
    /// Indexed scalar record carrying `distance`.
    pub distance_record_index: u32,
    /// Exact source selection form.
    pub support: DesignSurfaceOffsetSupport,
}

/// One indexed record in the auxiliary chain preceding a `SurfaceTrim` cell table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignSurfaceTrimChainRecord {
    /// Indexed record identity.
    pub record_index: u32,
    /// Primary indexed-header byte offset.
    pub byte_offset: u64,
    /// Source per-file dynamic class tag.
    pub class_tag: String,
    /// Bytes from the primary header to the following indexed header.
    pub frame_length: u64,
}

/// One source `BRep` cell entry in a `SurfaceTrim` cell table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignSurfaceTrimCellEntry {
    /// Indexed cell-record identity.
    pub record_index: u32,
    /// Byte offset of the marked cell-record reference.
    pub record_reference_offset: u64,
    /// One-based partition ordinal of a cell selected for removal.
    pub ordinal: u64,
    /// Byte offset of the serialized entry ordinal.
    pub ordinal_offset: u64,
}

/// Exact auxiliary carrier of a `SurfaceTrim` operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignSurfaceTrimOperation {
    /// Globally unique deterministic identifier for this native carrier.
    pub id: String,
    /// Owning `SurfaceTrim` parameter-scope record index.
    pub scope_record_index: u32,
    /// Indexed entity-selection record that starts the trimming tool chain.
    pub selection_record_index: u32,
    /// Byte offset of the entity-selection record.
    pub selection_byte_offset: u64,
    /// Indexed record immediately following the entity-selection frame.
    pub selection_next_record_index: u32,
    /// Byte offset of the record immediately following the entity-selection frame.
    pub selection_next_byte_offset: u64,
    /// Two indexed records between the entity selection and the cell table.
    pub chain_records: Vec<DesignSurfaceTrimChainRecord>,
    /// Indexed record carrying the counted BRep-cell table.
    pub cell_table_record_index: u32,
    /// Byte offset of the cell-table primary header.
    pub cell_table_byte_offset: u64,
    /// Dynamic class tag of the cell-table primary frame.
    pub cell_table_class_tag: String,
    /// Bytes from the cell-table primary header to its paired header.
    pub cell_table_frame_length: u64,
    /// Dynamic class tag of the cell-table paired frame.
    pub cell_table_paired_class_tag: String,
    /// Byte offset of the cell-table paired header.
    pub cell_table_paired_byte_offset: u64,
    /// Count of entries in the cell table.
    pub cell_count: u32,
    /// Byte offset of the cell-table count.
    pub cell_count_offset: u64,
    /// Ordered cell-table entries.
    pub cell_entries: Vec<DesignSurfaceTrimCellEntry>,
    /// Total number of cells in the operation's partition.
    pub trailing_value: u32,
    /// Byte offset of `trailing_value`.
    pub trailing_value_offset: u64,
    /// Byte offset of the zero value after `trailing_value`.
    pub trailing_zero_offset: u64,
}

/// Direction law encoded by a `SurfaceRuled` operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DesignRuledSurfaceMethod {
    /// Generate ruled strips tangent to the support faces.
    Tangent,
    /// Generate ruled strips normal to the support faces.
    Normal,
    /// Generate ruled strips along an explicitly selected direction.
    Direction,
}

/// Corner law encoded by a `SurfaceRuled` operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DesignRuledSurfaceCorner {
    /// Round adjacent ruled strips through a common corner.
    Rounded,
    /// Intersect adjacent ruled strips at a miter.
    Mitered,
}

/// Fixed construction carried by a `SurfaceRuled` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignRuledSurfaceOperation {
    /// Direction law.
    pub method: DesignRuledSurfaceMethod,
    /// Byte offset of the direction-law enum.
    pub method_offset: u64,
    /// Corner construction law.
    pub corner: DesignRuledSurfaceCorner,
    /// Byte offset of the corner-law enum.
    pub corner_offset: u64,
    /// Whether the opposite incident face supplies the angle reference.
    pub alternate_face: bool,
    /// Byte offset of the alternate-face Boolean.
    pub alternate_face_offset: u64,
    /// Referenced ruled-angle parameter owner.
    pub angle_owner_record_index: u32,
    /// Referenced ruled-distance parameter owner.
    pub distance_owner_record_index: u32,
    /// Ordered role-`0x08` edge-group records.
    pub edge_group_record_indices: Vec<u32>,
    /// Ordered auxiliary selection records between the edge-group runs.
    pub auxiliary_record_indices: Vec<u32>,
    /// Serialized direction entity identity; the all-zero UUID means absent.
    pub direction_entity_id: Option<String>,
}

/// Boundary condition a `SurfacePatch` component imposes against its adjacent
/// face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DesignPatchContinuity {
    /// Positional continuity only.
    Connected,
    /// First-derivative continuity.
    Tangent,
    /// Second-derivative continuity.
    Curvature,
    /// A serialized value whose continuity meaning is not settled.
    Unknown(u32),
}

impl DesignPatchContinuity {
    /// Decode the serialized continuity ordinal without discarding unknown values.
    #[must_use]
    pub fn from_code(code: u32) -> Self {
        match code {
            0 => Self::Connected,
            1 => Self::Tangent,
            2 => Self::Curvature,
            code => Self::Unknown(code),
        }
    }

    /// Return the serialized ordinal.
    #[must_use]
    pub fn code(self) -> u32 {
        match self {
            Self::Connected => 0,
            Self::Tangent => 1,
            Self::Curvature => 2,
            Self::Unknown(code) => code,
        }
    }
}

/// Native member-kind code of a sketch profile-region member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "u32", into = "u32")]
pub enum DesignSketchProfileRegionMemberKind {
    /// Profile-region curve member (serialized value 3).
    Curve,
}

impl DesignSketchProfileRegionMemberKind {
    #[must_use]
    pub fn from_code(code: u32) -> Option<Self> {
        (code == 3).then_some(Self::Curve)
    }

    #[must_use]
    pub fn code(self) -> u32 {
        3
    }
}

impl TryFrom<u32> for DesignSketchProfileRegionMemberKind {
    type Error = String;

    fn try_from(code: u32) -> Result<Self, Self::Error> {
        Self::from_code(code)
            .ok_or_else(|| format!("sketch profile-region member kind must be 3, not {code}"))
    }
}

impl From<DesignSketchProfileRegionMemberKind> for u32 {
    fn from(kind: DesignSketchProfileRegionMemberKind) -> Self {
        kind.code()
    }
}

/// ACT root-component registry flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "u32", into = "u32")]
pub enum ActRegistryFlag {
    Off,
    On,
}

impl ActRegistryFlag {
    #[must_use]
    pub fn from_code(code: u32) -> Option<Self> {
        match code {
            0 => Some(Self::Off),
            1 => Some(Self::On),
            _ => None,
        }
    }

    #[must_use]
    pub fn code(self) -> u32 {
        match self {
            Self::Off => 0,
            Self::On => 1,
        }
    }
}

impl TryFrom<u32> for ActRegistryFlag {
    type Error = String;

    fn try_from(code: u32) -> Result<Self, Self::Error> {
        Self::from_code(code).ok_or_else(|| format!("act registry flag must be 0 or 1, not {code}"))
    }
}

impl From<ActRegistryFlag> for u32 {
    fn from(flag: ActRegistryFlag) -> Self {
        flag.code()
    }
}

/// Decal image mapping-mode byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(from = "u8", into = "u8")]
pub enum DesignDecalMappingMode {
    FitToFaces,
    Unknown(u8),
}

impl DesignDecalMappingMode {
    #[must_use]
    pub fn from_code(code: u8) -> Self {
        match code {
            0x60 => Self::FitToFaces,
            code => Self::Unknown(code),
        }
    }

    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::FitToFaces => 0x60,
            Self::Unknown(code) => code,
        }
    }
}

impl From<u8> for DesignDecalMappingMode {
    fn from(code: u8) -> Self {
        Self::from_code(code)
    }
}

impl From<DesignDecalMappingMode> for u8 {
    fn from(mode: DesignDecalMappingMode) -> Self {
        mode.code()
    }
}

/// Pipe generated-section shape selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(from = "u8", into = "u8")]
pub enum DesignPipeSectionShape {
    Circular,
    Unknown(u8),
}

impl DesignPipeSectionShape {
    #[must_use]
    pub fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Circular,
            code => Self::Unknown(code),
        }
    }

    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::Circular => 1,
            Self::Unknown(code) => code,
        }
    }
}

impl From<u8> for DesignPipeSectionShape {
    fn from(code: u8) -> Self {
        Self::from_code(code)
    }
}

impl From<DesignPipeSectionShape> for u8 {
    fn from(shape: DesignPipeSectionShape) -> Self {
        shape.code()
    }
}

/// Settings a `SurfacePatch` scope carries for one boundary component.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignSurfacePatchBoundary {
    /// Position of the settings record in the scope's ordered reference table.
    pub scope_reference_ordinal: u32,
    /// Indexed settings-record identity.
    pub record_index: u32,
    /// Source `IsSeedSel` flag.
    pub is_seed_selection: bool,
    /// Boundary condition this component imposes against its adjacent face.
    pub continuity: DesignPatchContinuity,
    /// Source `PatchFlip` ordinal. Retained without a neutral meaning.
    pub flip: u32,
    /// Source `PatchScale` value.
    pub scale: f64,
    /// Indexed record the `rPatchModelRef` reference names: this boundary
    /// component's model reference.
    pub model_reference: u32,
}

/// Fixed construction carried by a planar sheet-metal `BaseFlange` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignBaseFlangeOperation {
    /// Positive sheet thickness in centimetres.
    pub thickness: f64,
    /// Byte offset of `thickness`.
    pub thickness_offset: u64,
    /// Counted sketch-profile operand group.
    pub profile_group_record_index: u32,
    /// Sketch-profile record contained by the profile group.
    pub profile_record_index: u32,
    /// Indexed thickness-construction record.
    pub thickness_record_index: u32,
    /// Indexed operation-settings record.
    pub settings_record_index: u32,
}

/// Bend position used by sheet-metal edge operations.
///
/// The position places the bend region against the selected edge: `Outside` and
/// `Inside` put the bend beyond and within the source face boundary, `Adjacent`
/// starts it at the boundary, and `TangentToSide` makes it tangent to the side
/// reference plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DesignBendPosition {
    /// The bend lies outside the selected edge.
    Outside,
    /// The bend lies inside the selected edge.
    Inside,
    /// The bend starts at the selected edge.
    Adjacent,
    /// The bend is tangent to the side reference plane.
    TangentToSide,
    /// A serialized value whose bend-position meaning is not settled.
    Unknown(u32),
}

impl DesignBendPosition {
    /// Decode the serialized bend-position discriminator without discarding unknown values.
    #[must_use]
    pub fn from_code(code: u32) -> Self {
        match code {
            1 => Self::Outside,
            2 => Self::Inside,
            3 => Self::Adjacent,
            4 => Self::TangentToSide,
            code => Self::Unknown(code),
        }
    }

    /// Return the serialized discriminator.
    #[must_use]
    pub fn code(self) -> u32 {
        match self {
            Self::Outside => 1,
            Self::Inside => 2,
            Self::Adjacent => 3,
            Self::TangentToSide => 4,
            Self::Unknown(code) => code,
        }
    }
}

/// Face pair an `EdgeFlange` height is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DesignSheetMetalHeightDatum {
    /// The height is measured from the inner faces of the sheet.
    InnerFaces,
    /// The height is measured from the outer faces of the sheet.
    OuterFaces,
    /// A serialized value whose height-datum meaning is not settled.
    Unknown(u32),
}

impl DesignSheetMetalHeightDatum {
    /// Decode the serialized height-datum discriminator without discarding unknown values.
    #[must_use]
    pub fn from_code(code: u32) -> Self {
        match code {
            1 => Self::InnerFaces,
            2 => Self::OuterFaces,
            code => Self::Unknown(code),
        }
    }

    /// Return the serialized discriminator.
    #[must_use]
    pub fn code(self) -> u32 {
        match self {
            Self::InnerFaces => 1,
            Self::OuterFaces => 2,
            Self::Unknown(code) => code,
        }
    }
}

/// Extent of an `EdgeFlange` along its selected edge.
///
/// Ordinary forms derive the mode from the count of width-distance parameter
/// owners in the ordered reference table. Classed forms can carry a distinct
/// explicit mode when that count has per-edge meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DesignEdgeWidthMode {
    /// The flange spans the complete selected edge and adds no width owner.
    FullEdge,
    /// The flange is centred on the edge and adds one width owner.
    Symmetric,
    /// The flange is measured from each end and adds two width owners.
    TwoSides,
    /// The fixed section carries one symmetric-width owner per selected edge.
    ///
    /// Neutral projection can collapse these owners to one symmetric width only
    /// when their stored values agree. Distinct values remain source-native.
    SymmetricPerEdge,
    /// The fixed section carries one `EdgeWidth_1`/`EdgeWidth_2` pair per
    /// selected edge. The edge-local orientation is not part of the neutral law.
    TwoSidesPerEdge,
}

/// Width law of an `EdgeFlange` operation, including the owner records it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesignEdgeWidth {
    FullEdge,
    Symmetric(u32),
    TwoSides([u32; 2]),
    SymmetricPerEdge(Vec<u32>),
    TwoSidesPerEdge(Vec<[u32; 2]>),
}

impl DesignEdgeWidth {
    pub(crate) fn mode(&self) -> DesignEdgeWidthMode {
        match self {
            Self::FullEdge => DesignEdgeWidthMode::FullEdge,
            Self::Symmetric(_) => DesignEdgeWidthMode::Symmetric,
            Self::TwoSides(_) => DesignEdgeWidthMode::TwoSides,
            Self::SymmetricPerEdge(_) => DesignEdgeWidthMode::SymmetricPerEdge,
            Self::TwoSidesPerEdge(_) => DesignEdgeWidthMode::TwoSidesPerEdge,
        }
    }

    pub(crate) fn owner_indices(&self) -> Vec<u32> {
        match self {
            Self::FullEdge => Vec::new(),
            Self::Symmetric(owner) => vec![*owner],
            Self::TwoSides(owners) => owners.to_vec(),
            Self::SymmetricPerEdge(owners) => owners.clone(),
            Self::TwoSidesPerEdge(pairs) => pairs.iter().flat_map(|pair| *pair).collect(),
        }
    }

    pub(crate) fn owner_indices_by_edge(&self) -> Vec<[u32; 2]> {
        match self {
            Self::TwoSidesPerEdge(pairs) => pairs.clone(),
            _ => Vec::new(),
        }
    }

    pub(crate) fn from_wire(
        width_mode: Option<DesignEdgeWidthMode>,
        owners: Vec<u32>,
        owners_by_edge: Vec<[u32; 2]>,
    ) -> Result<Self, String> {
        let mode = width_mode.unwrap_or(match owners.len() {
            0 => DesignEdgeWidthMode::FullEdge,
            1 => DesignEdgeWidthMode::Symmetric,
            _ => DesignEdgeWidthMode::TwoSides,
        });
        match mode {
            DesignEdgeWidthMode::FullEdge if owners.is_empty() && owners_by_edge.is_empty() => {
                Ok(Self::FullEdge)
            }
            DesignEdgeWidthMode::Symmetric if owners.len() == 1 && owners_by_edge.is_empty() => {
                Ok(Self::Symmetric(owners[0]))
            }
            DesignEdgeWidthMode::TwoSides if owners.len() == 2 && owners_by_edge.is_empty() => {
                Ok(Self::TwoSides([owners[0], owners[1]]))
            }
            DesignEdgeWidthMode::SymmetricPerEdge
                if !owners.is_empty() && owners_by_edge.is_empty() =>
            {
                Ok(Self::SymmetricPerEdge(owners))
            }
            DesignEdgeWidthMode::TwoSidesPerEdge
                if !owners_by_edge.is_empty()
                    && owners
                        == owners_by_edge
                            .iter()
                            .flat_map(|pair| *pair)
                            .collect::<Vec<_>>() =>
            {
                Ok(Self::TwoSidesPerEdge(owners_by_edge))
            }
            _ => Err("edge flange width mode disagrees with owner records".into()),
        }
    }
}

/// Parameter source used by a typed `EdgeFlange` width law.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DesignEdgeFlangeWidthParameterSource {
    /// Width parameters use the ordinary positive `EdgeWidth` source kinds.
    #[default]
    EdgeWidth,
    /// Legacy edge-end parameters use signed `EdgeOffset` source kinds.
    EdgeOffset,
}

impl TryFrom<DesignParameterScopeSerde> for DesignParameterScope {
    type Error = DesignParameterScopePayloadError;

    fn try_from(mut wire: DesignParameterScopeSerde) -> Result<Self, Self::Error> {
        if extrude_scope_is_absent(&wire.extrude) {
            wire.extrude = None;
        }
        if coil_scope_is_absent(&wire.coil) {
            wire.coil = None;
        }
        if base_flange_scope_is_absent(&wire.base_flange) {
            wire.base_flange = None;
        }
        if path_feature_scope_is_absent(&wire.path_feature) {
            wire.path_feature = None;
        }
        let mut present = Vec::new();
        if wire.extrude.is_some() {
            present.push("extrude");
        }
        if wire.coil.is_some() {
            present.push("coil");
        }
        if wire.base_flange.is_some() {
            present.push("base_flange");
        }
        if wire.path_feature.is_some() {
            present.push("path_feature");
        }
        if wire.work_plane_frame.is_some() {
            present.push("work_plane_frame");
        }
        if wire.joint_origin_frame.is_some() {
            present.push("joint_origin_frame");
        }
        if wire.sketch_entity.is_some() {
            present.push("sketch_entity");
        }
        if wire.solid_primitive.is_some() {
            present.push("solid_primitive");
        }
        if wire.direct_face_operation.is_some() {
            present.push("direct_face_operation");
        }
        if wire.move_operation.is_some() {
            present.push("move_operation");
        }
        if wire.scale_operation.is_some() {
            present.push("scale_operation");
        }
        if wire.surface_stitch_operation.is_some() {
            present.push("surface_stitch_operation");
        }
        if wire.surface_extend_operation.is_some() {
            present.push("surface_extend_operation");
        }
        if wire.surface_offset_operation.is_some() {
            present.push("surface_offset_operation");
        }
        if wire.ruled_surface_operation.is_some() {
            present.push("ruled_surface_operation");
        }
        if !wire.surface_patch_boundaries.is_empty() {
            present.push("surface_patch_boundaries");
        }
        if wire.edge_flange_operation.is_some() {
            present.push("edge_flange_operation");
        }
        if wire.hem_operation.is_some() {
            present.push("hem_operation");
        }
        if wire.fixed_fillet_parameters.is_some() {
            present.push("fixed_fillet_parameters");
        }
        if wire.fixed_chamfer_parameters.is_some() {
            present.push("fixed_chamfer_parameters");
        }
        if wire.combine_operation.is_some() {
            present.push("combine_operation");
        }
        if wire.thread_construction.is_some() {
            present.push("thread_construction");
        }
        if wire.draft_operation.is_some() {
            present.push("draft_operation");
        }
        if wire.circular_pattern_construction.is_some() {
            present.push("circular_pattern_construction");
        }
        if wire.rectangular_pattern_construction.is_some() {
            present.push("rectangular_pattern_construction");
        }
        if wire.assembly_alignment.is_some() {
            present.push("assembly_alignment");
        }
        if wire.component_insert_construction.is_some() {
            present.push("component_insert_construction");
        }
        if wire.derived_instance_construction.is_some() {
            present.push("derived_instance_construction");
        }
        if wire.copy_paste_component_operation.is_some() {
            present.push("copy_paste_component_operation");
        }
        if wire.copy_paste_bodies_operation.is_some() {
            present.push("copy_paste_bodies_operation");
        }
        if wire.mirror_construction.is_some() {
            present.push("mirror_construction");
        }
        if wire.base_feature_construction.is_some() {
            present.push("base_feature_construction");
        }
        if wire.work_axis_construction.is_some() {
            present.push("work_axis_construction");
        }
        if wire.work_point_construction.is_some() {
            present.push("work_point_construction");
        }
        if wire.hole_construction.is_some() {
            present.push("hole_construction");
        }
        if present.len() > 1 {
            return Err(DesignParameterScopePayloadError(format!(
                "design parameter scope carries more than one payload family: {}",
                present.join(", ")
            )));
        }
        let payload = match &wire.kind {
            DesignFeatureKind::Sketch => DesignScopePayload::Sketch(wire.sketch_entity.take()),
            DesignFeatureKind::Esquisse => DesignScopePayload::Esquisse(wire.sketch_entity.take()),
            DesignFeatureKind::Skizze => DesignScopePayload::Skizze(wire.sketch_entity.take()),
            DesignFeatureKind::Esboco => DesignScopePayload::Esboco(wire.sketch_entity.take()),
            DesignFeatureKind::Assemble => {
                DesignScopePayload::Assemble(wire.assembly_alignment.take())
            }
            DesignFeatureKind::AsBuilt => {
                DesignScopePayload::AsBuilt(wire.assembly_alignment.take())
            }
            DesignFeatureKind::Extrude => DesignScopePayload::Extrude(wire.extrude.take()),
            DesignFeatureKind::Extrusion => DesignScopePayload::Extrusion(wire.extrude.take()),
            DesignFeatureKind::Extrusao => DesignScopePayload::Extrusao(wire.extrude.take()),
            DesignFeatureKind::Fillet => {
                DesignScopePayload::Fillet(wire.fixed_fillet_parameters.take())
            }
            DesignFeatureKind::Conge => {
                DesignScopePayload::Conge(wire.fixed_fillet_parameters.take())
            }
            DesignFeatureKind::Abrundung => {
                DesignScopePayload::Abrundung(wire.fixed_fillet_parameters.take())
            }
            DesignFeatureKind::Arredondamento => {
                DesignScopePayload::Arredondamento(wire.fixed_fillet_parameters.take())
            }
            DesignFeatureKind::Chamfer => {
                DesignScopePayload::Chamfer(wire.fixed_chamfer_parameters.take())
            }
            DesignFeatureKind::Chanfrein => {
                DesignScopePayload::Chanfrein(wire.fixed_chamfer_parameters.take())
            }
            DesignFeatureKind::Combine => {
                DesignScopePayload::Combine(wire.combine_operation.take())
            }
            DesignFeatureKind::Draft => DesignScopePayload::Draft(wire.draft_operation.take()),
            DesignFeatureKind::ReplaceFace => {
                DesignScopePayload::ReplaceFace
            }
            DesignFeatureKind::CPattern => {
                DesignScopePayload::CPattern(wire.circular_pattern_construction.take())
            }
            DesignFeatureKind::CircularPattern => {
                DesignScopePayload::CircularPattern(wire.circular_pattern_construction.take())
            }
            DesignFeatureKind::ReseauC => {
                DesignScopePayload::ReseauC(wire.circular_pattern_construction.take())
            }
            DesignFeatureKind::RPattern => {
                DesignScopePayload::RPattern(wire.rectangular_pattern_construction.take())
            }
            DesignFeatureKind::RectangularPattern => {
                DesignScopePayload::RectangularPattern(wire.rectangular_pattern_construction.take())
            }
            DesignFeatureKind::Mirror => {
                DesignScopePayload::Mirror(wire.mirror_construction.take())
            }
            DesignFeatureKind::SymetrieMiroir => {
                DesignScopePayload::SymetrieMiroir(wire.mirror_construction.take())
            }
            DesignFeatureKind::Move => DesignScopePayload::Move(wire.move_operation.take()),
            DesignFeatureKind::OffsetFaces => {
                DesignScopePayload::OffsetFaces(match wire.direct_face_operation.take() {
                    None => None,
                    Some(DesignDirectFaceOperation::OffsetFaces(value)) => Some(value),
                    Some(_) => return Err(DesignParameterScopePayloadError("direct_face_operation.operation does not match OffsetFaces".into())),
                })
            }
            DesignFeatureKind::DecalerLesFaces => {
                DesignScopePayload::DecalerLesFaces(match wire.direct_face_operation.take() {
                    None => None,
                    Some(DesignDirectFaceOperation::OffsetFaces(value)) => Some(value),
                    Some(_) => return Err(DesignParameterScopePayloadError("direct_face_operation.operation does not match DecalerLesFaces".into())),
                })
            }
            DesignFeatureKind::Revolve => DesignScopePayload::Revolve(match wire.path_feature.take() {
                None => None,
                Some(DesignPathFeatureWire { path_feature_construction: Some(DesignPathFeatureConstruction::Revolve(value)), sweep_profile: None }) => Some(value),
                Some(_) => return Err(DesignParameterScopePayloadError("path_feature_construction or sweep_profile does not match Revolve".into())),
            }),
            DesignFeatureKind::Shell => {
                DesignScopePayload::Shell(match wire.direct_face_operation.take() {
                    None => None,
                    Some(DesignDirectFaceOperation::Shell(value)) => Some(value),
                    Some(_) => return Err(DesignParameterScopePayloadError("direct_face_operation.operation does not match Shell".into())),
                })
            }
            DesignFeatureKind::Schale => {
                DesignScopePayload::Schale(match wire.direct_face_operation.take() {
                    None => None,
                    Some(DesignDirectFaceOperation::Shell(value)) => Some(value),
                    Some(_) => return Err(DesignParameterScopePayloadError("direct_face_operation.operation does not match Schale".into())),
                })
            }
            DesignFeatureKind::Thicken => {
                DesignScopePayload::Thicken(match wire.direct_face_operation.take() {
                    None => None,
                    Some(DesignDirectFaceOperation::Thicken(value)) => Some(value),
                    Some(_) => return Err(DesignParameterScopePayloadError("direct_face_operation.operation does not match Thicken".into())),
                })
            }
            DesignFeatureKind::SpirePrimitive => {
                DesignScopePayload::SpirePrimitive(wire.coil.take())
            }
            DesignFeatureKind::CoilPrimitive => DesignScopePayload::CoilPrimitive(wire.coil.take()),
            DesignFeatureKind::Loft => DesignScopePayload::Loft(match wire.path_feature.take() {
                None => None,
                Some(DesignPathFeatureWire { path_feature_construction: Some(DesignPathFeatureConstruction::Loft(value)), sweep_profile: None }) => Some(value),
                Some(_) => return Err(DesignParameterScopePayloadError("path_feature_construction or sweep_profile does not match Loft".into())),
            }),
            DesignFeatureKind::Sweep => DesignScopePayload::Sweep(match wire.path_feature.take() {
                None => None,
                Some(DesignPathFeatureWire { path_feature_construction, sweep_profile }) => {
                    let construction = match path_feature_construction {
                        None => None,
                        Some(DesignPathFeatureConstruction::Sweep(value)) => Some(value),
                        Some(_) => return Err(DesignParameterScopePayloadError("path_feature_construction.kind does not match Sweep".into())),
                    };
                    Some(DesignSweepScope { construction, sweep_profile })
                }
            }),
            DesignFeatureKind::Pipe => DesignScopePayload::Pipe(match wire.path_feature.take() {
                None => None,
                Some(DesignPathFeatureWire { path_feature_construction: Some(DesignPathFeatureConstruction::Pipe(value)), sweep_profile: None }) => Some(value),
                Some(_) => return Err(DesignParameterScopePayloadError("path_feature_construction or sweep_profile does not match Pipe".into())),
            }),
            DesignFeatureKind::SurfacePatch => {
                DesignScopePayload::SurfacePatch(std::mem::take(&mut wire.surface_patch_boundaries))
            }
            DesignFeatureKind::SurfaceExtend => {
                DesignScopePayload::SurfaceExtend(wire.surface_extend_operation.take())
            }
            DesignFeatureKind::SurfaceOffset => {
                DesignScopePayload::SurfaceOffset(wire.surface_offset_operation.take())
            }
            DesignFeatureKind::SurfaceRuled => {
                DesignScopePayload::SurfaceRuled(wire.ruled_surface_operation.take())
            }
            DesignFeatureKind::SurfaceTrim => DesignScopePayload::SurfaceTrim,
            DesignFeatureKind::BoundaryFill => DesignScopePayload::BoundaryFill,
            DesignFeatureKind::Hole => DesignScopePayload::Hole(wire.hole_construction.take()),
            DesignFeatureKind::Split => DesignScopePayload::Split,
            DesignFeatureKind::Scale => DesignScopePayload::Scale(wire.scale_operation.take()),
            DesignFeatureKind::Massstab => {
                DesignScopePayload::Massstab(wire.scale_operation.take())
            }
            DesignFeatureKind::Thread => {
                DesignScopePayload::Thread(wire.thread_construction.take())
            }
            DesignFeatureKind::EdgeFlange => {
                DesignScopePayload::EdgeFlange(wire.edge_flange_operation.take())
            }
            DesignFeatureKind::Hem => DesignScopePayload::Hem(wire.hem_operation.take()),
            DesignFeatureKind::BaseFlange => {
                DesignScopePayload::BaseFlange(wire.base_flange.take())
            }
            DesignFeatureKind::ComponentInsert => {
                DesignScopePayload::ComponentInsert(wire.component_insert_construction.take())
            }
            DesignFeatureKind::CopyPaste => {
                DesignScopePayload::CopyPaste(wire.copy_paste_component_operation.take())
            }
            DesignFeatureKind::JointOrigin => {
                DesignScopePayload::JointOrigin(wire.joint_origin_frame.take())
            }
            DesignFeatureKind::Canvas => DesignScopePayload::Canvas,
            DesignFeatureKind::Decal => DesignScopePayload::Decal,
            DesignFeatureKind::BaseMeshFeature => DesignScopePayload::BaseMeshFeature,
            DesignFeatureKind::WorkPlane => {
                DesignScopePayload::WorkPlane(wire.work_plane_frame.take())
            }
            DesignFeatureKind::WorkAxis => {
                DesignScopePayload::WorkAxis(wire.work_axis_construction.take())
            }
            DesignFeatureKind::WorkPoint => {
                DesignScopePayload::WorkPoint(wire.work_point_construction.take())
            }
            DesignFeatureKind::DerivedInstance => {
                DesignScopePayload::DerivedInstance(wire.derived_instance_construction.take())
            }
            DesignFeatureKind::CustomFeature => DesignScopePayload::CustomFeature,
            DesignFeatureKind::Form => DesignScopePayload::Form,
            DesignFeatureKind::SurfaceStitch => {
                DesignScopePayload::SurfaceStitch(wire.surface_stitch_operation.take())
            }
            DesignFeatureKind::BaseFeature => {
                DesignScopePayload::BaseFeature(wire.base_feature_construction.take())
            }
            DesignFeatureKind::CopyPasteBodies => {
                DesignScopePayload::CopyPasteBodies(wire.copy_paste_bodies_operation.take())
            }
            DesignFeatureKind::SplitFace => DesignScopePayload::SplitFace,
            DesignFeatureKind::DeleteFace => DesignScopePayload::DeleteFace,
            DesignFeatureKind::SurfaceDeleteFace => DesignScopePayload::SurfaceDeleteFace,
            DesignFeatureKind::RemoveBody => DesignScopePayload::RemoveBody,
            DesignFeatureKind::Face => DesignScopePayload::Face,
            DesignFeatureKind::SpherePrimitive => {
                DesignScopePayload::SpherePrimitive(match wire.solid_primitive.take() {
                    None => None,
                    Some(DesignSolidPrimitive::Sphere(value)) => Some(value),
                    Some(_) => return Err(DesignParameterScopePayloadError("solid_primitive.primitive does not match SpherePrimitive".into())),
                })
            }
            DesignFeatureKind::TorusPrimitive => {
                DesignScopePayload::TorusPrimitive(match wire.solid_primitive.take() {
                    None => None,
                    Some(DesignSolidPrimitive::Torus(value)) => Some(value),
                    Some(_) => return Err(DesignParameterScopePayloadError("solid_primitive.primitive does not match TorusPrimitive".into())),
                })
            }
            DesignFeatureKind::BoxPrimitive => {
                DesignScopePayload::BoxPrimitive(match wire.solid_primitive.take() {
                    None => None,
                    Some(DesignSolidPrimitive::Box(value)) => Some(value),
                    Some(_) => return Err(DesignParameterScopePayloadError("solid_primitive.primitive does not match BoxPrimitive".into())),
                })
            }
            DesignFeatureKind::CylinderPrimitive => {
                DesignScopePayload::CylinderPrimitive(match wire.solid_primitive.take() {
                    None => None,
                    Some(DesignSolidPrimitive::Cylinder(value)) => Some(value),
                    Some(_) => return Err(DesignParameterScopePayloadError("solid_primitive.primitive does not match CylinderPrimitive".into())),
                })
            }
            DesignFeatureKind::Native(name) => DesignScopePayload::Native(name.clone()),
        };
        if wire.extrude.is_some()
            || wire.coil.is_some()
            || wire.base_flange.is_some()
            || wire.path_feature.is_some()
            || wire.work_plane_frame.is_some()
            || wire.joint_origin_frame.is_some()
            || wire.sketch_entity.is_some()
            || wire.solid_primitive.is_some()
            || wire.direct_face_operation.is_some()
            || wire.move_operation.is_some()
            || wire.scale_operation.is_some()
            || wire.surface_stitch_operation.is_some()
            || wire.surface_extend_operation.is_some()
            || wire.surface_offset_operation.is_some()
            || wire.ruled_surface_operation.is_some()
            || !wire.surface_patch_boundaries.is_empty()
            || wire.edge_flange_operation.is_some()
            || wire.hem_operation.is_some()
            || wire.fixed_fillet_parameters.is_some()
            || wire.fixed_chamfer_parameters.is_some()
            || wire.combine_operation.is_some()
            || wire.thread_construction.is_some()
            || wire.draft_operation.is_some()
            || wire.circular_pattern_construction.is_some()
            || wire.rectangular_pattern_construction.is_some()
            || wire.assembly_alignment.is_some()
            || wire.component_insert_construction.is_some()
            || wire.derived_instance_construction.is_some()
            || wire.copy_paste_component_operation.is_some()
            || wire.copy_paste_bodies_operation.is_some()
            || wire.mirror_construction.is_some()
            || wire.base_feature_construction.is_some()
            || wire.work_axis_construction.is_some()
            || wire.work_point_construction.is_some()
            || wire.hole_construction.is_some()
        {
            return Err(DesignParameterScopePayloadError(format!(
                "design parameter scope payload disagrees with kind {}",
                wire.kind
            )));
        }
        Ok(Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag,
            record_index: wire.record_index,
            frame_length: wire.frame_length,
            kind_offset: wire.kind_offset,
            feature_ordinal: wire.feature_ordinal,
            feature_ordinal_offset: wire.feature_ordinal_offset,
            history_state_id: wire.history_state_id,
            history_state_id_offset: wire.history_state_id_offset,
            previous_history_state_id: wire.previous_history_state_id,
            previous_history_state_id_offset: wire.previous_history_state_id_offset,
            reference_count_offset: wire.reference_count_offset,
            reference_members: wire.reference_members,
            reference_member_offsets: wire.reference_member_offsets,
            payload,
            unclosed_construction_operand_groups: wire.unclosed_construction_operand_groups,
            paired_class_tag: wire.paired_class_tag,
            paired_byte_offset: wire.paired_byte_offset,
        })
    }
}

impl From<DesignParameterScope> for DesignParameterScopeSerde {
    fn from(scope: DesignParameterScope) -> Self {
        let kind = scope.kind();
        let mut wire = DesignParameterScopeSerde {
            id: scope.id,
            byte_offset: scope.byte_offset,
            class_tag: scope.class_tag,
            record_index: scope.record_index,
            frame_length: scope.frame_length,
            kind,
            kind_offset: scope.kind_offset,
            extrude: None,
            coil: None,
            feature_ordinal: scope.feature_ordinal,
            feature_ordinal_offset: scope.feature_ordinal_offset,
            history_state_id: scope.history_state_id,
            history_state_id_offset: scope.history_state_id_offset,
            previous_history_state_id: scope.previous_history_state_id,
            previous_history_state_id_offset: scope.previous_history_state_id_offset,
            reference_count_offset: scope.reference_count_offset,
            reference_members: scope.reference_members,
            reference_member_offsets: scope.reference_member_offsets,
            solid_primitive: None,
            direct_face_operation: None,
            move_operation: None,
            scale_operation: None,
            surface_stitch_operation: None,
            surface_extend_operation: None,
            surface_offset_operation: None,
            ruled_surface_operation: None,
            base_flange: None,
            surface_patch_boundaries: Vec::new(),
            edge_flange_operation: None,
            hem_operation: None,
            fixed_fillet_parameters: None,
            fixed_chamfer_parameters: None,
            path_feature: None,
            combine_operation: None,
            thread_construction: None,
            draft_operation: None,
            circular_pattern_construction: None,
            rectangular_pattern_construction: None,
            assembly_alignment: None,
            component_insert_construction: None,
            derived_instance_construction: None,
            copy_paste_component_operation: None,
            copy_paste_bodies_operation: None,
            mirror_construction: None,
            base_feature_construction: None,
            work_plane_frame: None,
            work_axis_construction: None,
            joint_origin_frame: None,
            work_point_construction: None,
            unclosed_construction_operand_groups: scope.unclosed_construction_operand_groups,
            hole_construction: None,
            sketch_entity: None,
            paired_class_tag: scope.paired_class_tag,
            paired_byte_offset: scope.paired_byte_offset,
        };
        match scope.payload {
            DesignScopePayload::Extrude(value)
            | DesignScopePayload::Extrusion(value)
            | DesignScopePayload::Extrusao(value) => wire.extrude = value,
            DesignScopePayload::SpirePrimitive(value)
            | DesignScopePayload::CoilPrimitive(value) => wire.coil = value,
            DesignScopePayload::BaseFlange(value) => wire.base_flange = value,
            DesignScopePayload::Revolve(value) => wire.path_feature = value.map(|value| DesignPathFeatureWire { path_feature_construction: Some(DesignPathFeatureConstruction::Revolve(value)), sweep_profile: None }),
            DesignScopePayload::Loft(value) => wire.path_feature = value.map(|value| DesignPathFeatureWire { path_feature_construction: Some(DesignPathFeatureConstruction::Loft(value)), sweep_profile: None }),
            DesignScopePayload::Sweep(value) => wire.path_feature = value.map(|sweep| DesignPathFeatureWire { path_feature_construction: sweep.construction.map(DesignPathFeatureConstruction::Sweep), sweep_profile: sweep.sweep_profile }),
            DesignScopePayload::Pipe(value) => wire.path_feature = value.map(|value| DesignPathFeatureWire { path_feature_construction: Some(DesignPathFeatureConstruction::Pipe(value)), sweep_profile: None }),
            DesignScopePayload::WorkPlane(value) => wire.work_plane_frame = value,
            DesignScopePayload::JointOrigin(value) => wire.joint_origin_frame = value,
            DesignScopePayload::Sketch(value)
            | DesignScopePayload::Esquisse(value)
            | DesignScopePayload::Skizze(value)
            | DesignScopePayload::Esboco(value) => wire.sketch_entity = value,
            DesignScopePayload::SpherePrimitive(value) => wire.solid_primitive = value.map(DesignSolidPrimitive::Sphere),
            DesignScopePayload::TorusPrimitive(value) => wire.solid_primitive = value.map(DesignSolidPrimitive::Torus),
            DesignScopePayload::BoxPrimitive(value) => wire.solid_primitive = value.map(DesignSolidPrimitive::Box),
            DesignScopePayload::CylinderPrimitive(value) => wire.solid_primitive = value.map(DesignSolidPrimitive::Cylinder),
            DesignScopePayload::ReplaceFace => {},
            DesignScopePayload::OffsetFaces(value) | DesignScopePayload::DecalerLesFaces(value) => wire.direct_face_operation = value.map(DesignDirectFaceOperation::OffsetFaces),
            DesignScopePayload::Shell(value) | DesignScopePayload::Schale(value) => wire.direct_face_operation = value.map(DesignDirectFaceOperation::Shell),
            DesignScopePayload::Thicken(value) => wire.direct_face_operation = value.map(DesignDirectFaceOperation::Thicken),
            DesignScopePayload::Move(value) => wire.move_operation = value,
            DesignScopePayload::Scale(value) | DesignScopePayload::Massstab(value) => {
                wire.scale_operation = value
            }
            DesignScopePayload::SurfaceStitch(value) => wire.surface_stitch_operation = value,
            DesignScopePayload::SurfaceExtend(value) => wire.surface_extend_operation = value,
            DesignScopePayload::SurfaceOffset(value) => wire.surface_offset_operation = value,
            DesignScopePayload::SurfaceRuled(value) => wire.ruled_surface_operation = value,
            DesignScopePayload::SurfacePatch(value) => wire.surface_patch_boundaries = value,
            DesignScopePayload::EdgeFlange(value) => wire.edge_flange_operation = value,
            DesignScopePayload::Hem(value) => wire.hem_operation = value,
            DesignScopePayload::Fillet(value)
            | DesignScopePayload::Conge(value)
            | DesignScopePayload::Abrundung(value)
            | DesignScopePayload::Arredondamento(value) => wire.fixed_fillet_parameters = value,
            DesignScopePayload::Chamfer(value) | DesignScopePayload::Chanfrein(value) => {
                wire.fixed_chamfer_parameters = value
            }
            DesignScopePayload::Combine(value) => wire.combine_operation = value,
            DesignScopePayload::Thread(value) => wire.thread_construction = value,
            DesignScopePayload::Draft(value) => wire.draft_operation = value,
            DesignScopePayload::CPattern(value)
            | DesignScopePayload::CircularPattern(value)
            | DesignScopePayload::ReseauC(value) => wire.circular_pattern_construction = value,
            DesignScopePayload::RPattern(value) | DesignScopePayload::RectangularPattern(value) => {
                wire.rectangular_pattern_construction = value
            }
            DesignScopePayload::Assemble(value) | DesignScopePayload::AsBuilt(value) => {
                wire.assembly_alignment = value
            }
            DesignScopePayload::ComponentInsert(value) => {
                wire.component_insert_construction = value
            }
            DesignScopePayload::DerivedInstance(value) => {
                wire.derived_instance_construction = value
            }
            DesignScopePayload::CopyPaste(value) => wire.copy_paste_component_operation = value,
            DesignScopePayload::CopyPasteBodies(value) => wire.copy_paste_bodies_operation = value,
            DesignScopePayload::Mirror(value) | DesignScopePayload::SymetrieMiroir(value) => {
                wire.mirror_construction = value
            }
            DesignScopePayload::BaseFeature(value) => wire.base_feature_construction = value,
            DesignScopePayload::WorkAxis(value) => wire.work_axis_construction = value,
            DesignScopePayload::WorkPoint(value) => wire.work_point_construction = value,
            DesignScopePayload::Hole(value) => wire.hole_construction = value,
            DesignScopePayload::SurfaceTrim
            | DesignScopePayload::BoundaryFill
            | DesignScopePayload::Split
            | DesignScopePayload::Canvas
            | DesignScopePayload::Decal
            | DesignScopePayload::BaseMeshFeature
            | DesignScopePayload::CustomFeature
            | DesignScopePayload::Form
            | DesignScopePayload::SplitFace
            | DesignScopePayload::DeleteFace
            | DesignScopePayload::SurfaceDeleteFace
            | DesignScopePayload::RemoveBody
            | DesignScopePayload::Face
            | DesignScopePayload::Native(_) => {}
        }
        wire
    }
}

impl DesignParameterScope {
    /// Source feature-family name, derived from its construction variant.
    pub(crate) fn kind(&self) -> DesignFeatureKind {
        self.payload.kind()
    }

    /// Source spelling without allocating a kind tag.
    pub(crate) fn kind_name(&self) -> &str {
        self.payload.kind_name()
    }

    pub(crate) fn extrude(&self) -> Option<&DesignExtrudeScope> {
        match &self.payload {
            DesignScopePayload::Extrude(value)
            | DesignScopePayload::Extrusion(value)
            | DesignScopePayload::Extrusao(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn extrude_mut(&mut self) -> Option<&mut DesignExtrudeScope> {
        match &mut self.payload {
            DesignScopePayload::Extrude(value)
            | DesignScopePayload::Extrusion(value)
            | DesignScopePayload::Extrusao(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn coil(&self) -> Option<&DesignCoilScope> {
        match &self.payload {
            DesignScopePayload::SpirePrimitive(value)
            | DesignScopePayload::CoilPrimitive(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn coil_mut(&mut self) -> Option<&mut DesignCoilScope> {
        match &mut self.payload {
            DesignScopePayload::SpirePrimitive(value)
            | DesignScopePayload::CoilPrimitive(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn base_flange(&self) -> Option<&DesignBaseFlangeScope> {
        match &self.payload {
            DesignScopePayload::BaseFlange(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn base_flange_mut(&mut self) -> Option<&mut DesignBaseFlangeScope> {
        match &mut self.payload {
            DesignScopePayload::BaseFlange(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn work_plane_frame(&self) -> Option<&DesignWorkPlaneTransform> {
        match &self.payload {
            DesignScopePayload::WorkPlane(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn work_plane_frame_mut(&mut self) -> Option<&mut DesignWorkPlaneTransform> {
        match &mut self.payload {
            DesignScopePayload::WorkPlane(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn joint_origin_frame(&self) -> Option<&DesignJointOriginTransform> {
        match &self.payload {
            DesignScopePayload::JointOrigin(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn joint_origin_frame_mut(&mut self) -> Option<&mut DesignJointOriginTransform> {
        match &mut self.payload {
            DesignScopePayload::JointOrigin(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn sketch_entity(&self) -> Option<&DesignSketchEntityBinding> {
        match &self.payload {
            DesignScopePayload::Sketch(value)
            | DesignScopePayload::Esquisse(value)
            | DesignScopePayload::Skizze(value)
            | DesignScopePayload::Esboco(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn sketch_entity_mut(&mut self) -> Option<&mut DesignSketchEntityBinding> {
        match &mut self.payload {
            DesignScopePayload::Sketch(value)
            | DesignScopePayload::Esquisse(value)
            | DesignScopePayload::Skizze(value)
            | DesignScopePayload::Esboco(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn move_operation(&self) -> Option<&DesignMoveOperation> {
        match &self.payload {
            DesignScopePayload::Move(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn move_operation_mut(&mut self) -> Option<&mut DesignMoveOperation> {
        match &mut self.payload {
            DesignScopePayload::Move(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn scale_operation(&self) -> Option<&DesignScaleOperation> {
        match &self.payload {
            DesignScopePayload::Scale(value) | DesignScopePayload::Massstab(value) => {
                value.as_ref()
            }
            _ => None,
        }
    }

    pub(crate) fn scale_operation_mut(&mut self) -> Option<&mut DesignScaleOperation> {
        match &mut self.payload {
            DesignScopePayload::Scale(value) | DesignScopePayload::Massstab(value) => {
                value.as_mut()
            }
            _ => None,
        }
    }

    pub(crate) fn surface_stitch_operation(&self) -> Option<&DesignSurfaceStitchOperation> {
        match &self.payload {
            DesignScopePayload::SurfaceStitch(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn surface_stitch_operation_mut(
        &mut self,
    ) -> Option<&mut DesignSurfaceStitchOperation> {
        match &mut self.payload {
            DesignScopePayload::SurfaceStitch(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn surface_extend_operation(&self) -> Option<&DesignSurfaceExtendOperation> {
        match &self.payload {
            DesignScopePayload::SurfaceExtend(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn surface_extend_operation_mut(
        &mut self,
    ) -> Option<&mut DesignSurfaceExtendOperation> {
        match &mut self.payload {
            DesignScopePayload::SurfaceExtend(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn surface_offset_operation(&self) -> Option<&DesignSurfaceOffsetOperation> {
        match &self.payload {
            DesignScopePayload::SurfaceOffset(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn surface_offset_operation_mut(
        &mut self,
    ) -> Option<&mut DesignSurfaceOffsetOperation> {
        match &mut self.payload {
            DesignScopePayload::SurfaceOffset(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn ruled_surface_operation(&self) -> Option<&DesignRuledSurfaceOperation> {
        match &self.payload {
            DesignScopePayload::SurfaceRuled(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn ruled_surface_operation_mut(
        &mut self,
    ) -> Option<&mut DesignRuledSurfaceOperation> {
        match &mut self.payload {
            DesignScopePayload::SurfaceRuled(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn edge_flange_operation(&self) -> Option<&DesignEdgeFlangeOperation> {
        match &self.payload {
            DesignScopePayload::EdgeFlange(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn edge_flange_operation_mut(&mut self) -> Option<&mut DesignEdgeFlangeOperation> {
        match &mut self.payload {
            DesignScopePayload::EdgeFlange(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn hem_operation(&self) -> Option<&DesignHemOperation> {
        match &self.payload {
            DesignScopePayload::Hem(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn hem_operation_mut(&mut self) -> Option<&mut DesignHemOperation> {
        match &mut self.payload {
            DesignScopePayload::Hem(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn fixed_fillet_parameters(&self) -> Option<&DesignFixedFilletParameters> {
        match &self.payload {
            DesignScopePayload::Fillet(value)
            | DesignScopePayload::Conge(value)
            | DesignScopePayload::Abrundung(value)
            | DesignScopePayload::Arredondamento(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn fixed_fillet_parameters_mut(
        &mut self,
    ) -> Option<&mut DesignFixedFilletParameters> {
        match &mut self.payload {
            DesignScopePayload::Fillet(value)
            | DesignScopePayload::Conge(value)
            | DesignScopePayload::Abrundung(value)
            | DesignScopePayload::Arredondamento(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn fixed_chamfer_parameters(&self) -> Option<&DesignFixedChamferParameters> {
        match &self.payload {
            DesignScopePayload::Chamfer(value) | DesignScopePayload::Chanfrein(value) => {
                value.as_ref()
            }
            _ => None,
        }
    }

    pub(crate) fn fixed_chamfer_parameters_mut(
        &mut self,
    ) -> Option<&mut DesignFixedChamferParameters> {
        match &mut self.payload {
            DesignScopePayload::Chamfer(value) | DesignScopePayload::Chanfrein(value) => {
                value.as_mut()
            }
            _ => None,
        }
    }

    pub(crate) fn combine_operation(&self) -> Option<&DesignCombineOperation> {
        match &self.payload {
            DesignScopePayload::Combine(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn combine_operation_mut(&mut self) -> Option<&mut DesignCombineOperation> {
        match &mut self.payload {
            DesignScopePayload::Combine(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn thread_construction(&self) -> Option<&DesignThreadConstruction> {
        match &self.payload {
            DesignScopePayload::Thread(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn thread_construction_mut(&mut self) -> Option<&mut DesignThreadConstruction> {
        match &mut self.payload {
            DesignScopePayload::Thread(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn draft_operation(&self) -> Option<&DesignDraftOperation> {
        match &self.payload {
            DesignScopePayload::Draft(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn draft_operation_mut(&mut self) -> Option<&mut DesignDraftOperation> {
        match &mut self.payload {
            DesignScopePayload::Draft(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn circular_pattern_construction(
        &self,
    ) -> Option<&DesignCircularPatternConstruction> {
        match &self.payload {
            DesignScopePayload::CPattern(value)
            | DesignScopePayload::CircularPattern(value)
            | DesignScopePayload::ReseauC(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn circular_pattern_construction_mut(
        &mut self,
    ) -> Option<&mut DesignCircularPatternConstruction> {
        match &mut self.payload {
            DesignScopePayload::CPattern(value)
            | DesignScopePayload::CircularPattern(value)
            | DesignScopePayload::ReseauC(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn rectangular_pattern_construction(
        &self,
    ) -> Option<&DesignRectangularPatternConstruction> {
        match &self.payload {
            DesignScopePayload::RPattern(value) | DesignScopePayload::RectangularPattern(value) => {
                value.as_ref()
            }
            _ => None,
        }
    }

    pub(crate) fn rectangular_pattern_construction_mut(
        &mut self,
    ) -> Option<&mut DesignRectangularPatternConstruction> {
        match &mut self.payload {
            DesignScopePayload::RPattern(value) | DesignScopePayload::RectangularPattern(value) => {
                value.as_mut()
            }
            _ => None,
        }
    }

    pub(crate) fn assembly_alignment(&self) -> Option<&DesignAssemblyAlignment> {
        match &self.payload {
            DesignScopePayload::Assemble(value) | DesignScopePayload::AsBuilt(value) => {
                value.as_ref()
            }
            _ => None,
        }
    }

    pub(crate) fn assembly_alignment_mut(&mut self) -> Option<&mut DesignAssemblyAlignment> {
        match &mut self.payload {
            DesignScopePayload::Assemble(value) | DesignScopePayload::AsBuilt(value) => {
                value.as_mut()
            }
            _ => None,
        }
    }

    pub(crate) fn component_insert_construction(
        &self,
    ) -> Option<&DesignComponentInsertConstruction> {
        match &self.payload {
            DesignScopePayload::ComponentInsert(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn component_insert_construction_mut(
        &mut self,
    ) -> Option<&mut DesignComponentInsertConstruction> {
        match &mut self.payload {
            DesignScopePayload::ComponentInsert(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn derived_instance_construction(
        &self,
    ) -> Option<&DesignDerivedInstanceConstruction> {
        match &self.payload {
            DesignScopePayload::DerivedInstance(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn derived_instance_construction_mut(
        &mut self,
    ) -> Option<&mut DesignDerivedInstanceConstruction> {
        match &mut self.payload {
            DesignScopePayload::DerivedInstance(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn copy_paste_component_operation(
        &self,
    ) -> Option<&DesignCopyPasteComponentOperation> {
        match &self.payload {
            DesignScopePayload::CopyPaste(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn copy_paste_component_operation_mut(
        &mut self,
    ) -> Option<&mut DesignCopyPasteComponentOperation> {
        match &mut self.payload {
            DesignScopePayload::CopyPaste(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn copy_paste_bodies_operation(&self) -> Option<&DesignCopyPasteBodiesOperation> {
        match &self.payload {
            DesignScopePayload::CopyPasteBodies(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn copy_paste_bodies_operation_mut(
        &mut self,
    ) -> Option<&mut DesignCopyPasteBodiesOperation> {
        match &mut self.payload {
            DesignScopePayload::CopyPasteBodies(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn mirror_construction(&self) -> Option<&DesignMirrorConstruction> {
        match &self.payload {
            DesignScopePayload::Mirror(value) | DesignScopePayload::SymetrieMiroir(value) => {
                value.as_ref()
            }
            _ => None,
        }
    }

    pub(crate) fn mirror_construction_mut(&mut self) -> Option<&mut DesignMirrorConstruction> {
        match &mut self.payload {
            DesignScopePayload::Mirror(value) | DesignScopePayload::SymetrieMiroir(value) => {
                value.as_mut()
            }
            _ => None,
        }
    }

    pub(crate) fn base_feature_construction(&self) -> Option<&DesignBaseFeatureConstruction> {
        match &self.payload {
            DesignScopePayload::BaseFeature(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn base_feature_construction_mut(
        &mut self,
    ) -> Option<&mut DesignBaseFeatureConstruction> {
        match &mut self.payload {
            DesignScopePayload::BaseFeature(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn work_axis_construction(&self) -> Option<&DesignWorkAxisConstruction> {
        match &self.payload {
            DesignScopePayload::WorkAxis(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn work_axis_construction_mut(&mut self) -> Option<&mut DesignWorkAxisConstruction> {
        match &mut self.payload {
            DesignScopePayload::WorkAxis(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn work_point_construction(&self) -> Option<&DesignWorkPointConstruction> {
        match &self.payload {
            DesignScopePayload::WorkPoint(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn work_point_construction_mut(
        &mut self,
    ) -> Option<&mut DesignWorkPointConstruction> {
        match &mut self.payload {
            DesignScopePayload::WorkPoint(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn hole_construction(&self) -> Option<&DesignHoleConstruction> {
        match &self.payload {
            DesignScopePayload::Hole(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn hole_construction_mut(&mut self) -> Option<&mut DesignHoleConstruction> {
        match &mut self.payload {
            DesignScopePayload::Hole(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn surface_patch_boundaries(&self) -> &[DesignSurfacePatchBoundary] {
        match &self.payload {
            DesignScopePayload::SurfacePatch(value) => value,
            _ => &[],
        }
    }

    pub(crate) fn extrude_prologue(&self) -> Option<DesignExtrudePrologue> {
        self.extrude().and_then(|extrude| extrude.extrude_prologue)
    }

    pub(crate) fn extrude_prologue_mut(&mut self) -> Option<&mut DesignExtrudePrologue> {
        self.extrude_mut()
            .and_then(|extrude| extrude.extrude_prologue.as_mut())
    }

    pub(crate) fn extrude_profile(&self) -> Option<&DesignSketchProfileOperand> {
        self.extrude()
            .and_then(|extrude| extrude.extrude_profile.as_ref())
    }

    pub(crate) fn extrude_profile_mut(&mut self) -> Option<&mut DesignSketchProfileOperand> {
        self.extrude_mut()
            .and_then(|extrude| extrude.extrude_profile.as_mut())
    }

    pub(crate) fn fixed_extrude_parameters(&self) -> Option<&DesignFixedExtrudeParameters> {
        self.extrude()
            .and_then(|extrude| extrude.fixed_extrude_parameters.as_ref())
    }

    pub(crate) fn base_flange_operation(&self) -> Option<&DesignBaseFlangeOperation> {
        self.base_flange()
            .and_then(|base_flange| base_flange.base_flange_operation.as_ref())
    }

    pub(crate) fn base_flange_profile(&self) -> Option<&DesignSketchProfileOperand> {
        self.base_flange()
            .and_then(|base_flange| base_flange.base_flange_profile.as_ref())
    }

    pub(crate) fn coil_operation(&self) -> Option<DesignExtrudeOperation> {
        self.coil().and_then(|coil| coil.coil_operation.map(|field| field.value))
    }

    pub(crate) fn coil_operation_offset(&self) -> Option<u64> {
        self.coil().and_then(|coil| coil.coil_operation.and_then(|field| field.offset))
    }

    pub(crate) fn coil_extent(&self) -> Option<DesignCoilExtent> {
        self.coil().and_then(|coil| coil.coil_extent.map(|field| field.value))
    }

    pub(crate) fn coil_section(&self) -> Option<DesignCoilSection> {
        self.coil().and_then(|coil| coil.coil_section.map(|field| field.value))
    }

    pub(crate) fn coil_section_placement(&self) -> Option<DesignCoilSectionPlacement> {
        self.coil().and_then(|coil| coil.coil_section_placement.map(|field| field.value))
    }

    pub(crate) fn coil_clockwise(&self) -> Option<bool> {
        self.coil().and_then(|coil| coil.coil_clockwise.map(|field| field.value))
    }

    pub(crate) fn coil_extent_offset(&self) -> Option<u64> {
        self.coil().and_then(|coil| coil.coil_extent.and_then(|field| field.offset))
    }

    pub(crate) fn coil_section_offset(&self) -> Option<u64> {
        self.coil().and_then(|coil| coil.coil_section.and_then(|field| field.offset))
    }

    pub(crate) fn coil_section_placement_offset(&self) -> Option<u64> {
        self.coil()
            .and_then(|coil| coil.coil_section_placement.and_then(|field| field.offset))
    }

    pub(crate) fn coil_clockwise_offset(&self) -> Option<u64> {
        self.coil().and_then(|coil| coil.coil_clockwise.and_then(|field| field.offset))
    }

    pub(crate) fn coil_placement(&self) -> Option<&DesignCoilPlacement> {
        self.coil().and_then(|coil| coil.coil_placement.as_ref())
    }

    pub(crate) fn coil_transform(&self) -> Option<&DesignCoilTransform> {
        self.coil().and_then(|coil| coil.coil_transform.as_ref())
    }

    pub(crate) fn has_path_construction(&self) -> bool {
        match &self.payload {
            DesignScopePayload::Revolve(value) => value.is_some(),
            DesignScopePayload::Loft(value) => value.is_some(),
            DesignScopePayload::Pipe(value) => value.is_some(),
            DesignScopePayload::Sweep(value) => value.as_ref().is_some_and(|sweep| sweep.construction.is_some()),
            _ => false,
        }
    }

    pub(crate) fn sweep_profile(&self) -> Option<&DesignSketchProfileOperand> {
        match &self.payload {
            DesignScopePayload::Sweep(value) => value.as_ref().and_then(|sweep| sweep.sweep_profile.as_ref()),
            _ => None,
        }
    }

    pub(crate) fn work_plane_transform(&self) -> Option<[[f64; 4]; 4]> {
        self.work_plane_frame()
            .map(|frame| frame.work_plane_transform)
    }

    pub(crate) fn work_plane_reference(&self) -> Option<u32> {
        self.work_plane_frame()
            .and_then(|frame| frame.reference.as_ref().map(|r| r.work_plane_reference))
    }

    pub(crate) fn work_plane_construction(&self) -> Option<&DesignWorkPlaneConstruction> {
        self.work_plane_frame()
            .and_then(|frame| frame.work_plane_construction.as_ref())
    }

    pub(crate) fn work_plane_construction_mut(
        &mut self,
    ) -> Option<&mut DesignWorkPlaneConstruction> {
        self.work_plane_frame_mut()
            .and_then(|frame| frame.work_plane_construction.as_mut())
    }

    pub(crate) fn joint_origin_transform(&self) -> Option<[[f64; 4]; 4]> {
        self.joint_origin_frame()
            .map(|frame| frame.joint_origin_transform)
    }

    pub(crate) fn joint_origin_transform_offset(&self) -> Option<u64> {
        self.joint_origin_frame()
            .map(|frame| frame.joint_origin_transform_offset)
    }

    pub(crate) fn joint_origin_reference(&self) -> Option<u32> {
        self.joint_origin_frame().and_then(|frame| {
            frame
                .reference
                .as_ref()
                .map(|reference| reference.joint_origin_reference)
        })
    }

    pub(crate) fn joint_origin_reference_offset(&self) -> Option<u64> {
        self.joint_origin_frame().and_then(|frame| {
            frame
                .reference
                .as_ref()
                .map(|reference| reference.joint_origin_reference_offset)
        })
    }
}

#[cfg(test)]
impl DesignParameterScope {
    /// Build a scope carrying only its identity, kind, and record index.
    pub(crate) fn with_work_plane_transform(&mut self, transform: [[f64; 4]; 4]) {
        self.payload = DesignScopePayload::WorkPlane(Some(DesignWorkPlaneTransform {
            work_plane_transform: transform,
            work_plane_transform_offset: 0,
            reference: None,
            work_plane_construction: None,
        }));
    }

    pub(crate) fn with_work_plane_reference(&mut self, record_index: u32) {
        if let Some(frame) = self.work_plane_frame_mut() {
            frame.reference = Some(DesignWorkPlaneReference {
                work_plane_reference: record_index,
                work_plane_reference_offset: 0,
            });
        }
    }

    pub(crate) fn with_joint_origin_transform(&mut self, transform: [[f64; 4]; 4]) {
        self.payload = DesignScopePayload::JointOrigin(Some(DesignJointOriginTransform {
            joint_origin_transform: transform,
            joint_origin_transform_offset: 0,
            reference: None,
        }));
    }

    pub(crate) fn empty(id: &str, kind: DesignFeatureKind, record_index: u32) -> Self {
        Self {
            id: id.to_string(),
            byte_offset: 0,
            class_tag: String::new(),
            record_index,
            frame_length: 0,
            kind_offset: 0,
            feature_ordinal: 0,
            feature_ordinal_offset: 0,
            history_state_id: None,
            history_state_id_offset: 0,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 0,
            reference_members: Vec::new(),
            reference_member_offsets: Vec::new(),
            payload: kind.into(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: String::new(),
            paired_byte_offset: 0,
        }
    }
}

/// Height extent law carried by a sheet-metal `EdgeFlange` scope.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum DesignEdgeFlangeHeightExtent {
    /// The flange height is a direct distance from the selected sheet datum.
    #[default]
    Distance,
    /// The flange height is measured from a selected construction entity.
    ToObject {
        /// Role-`0x21` construction-operand group containing the target.
        target_group_record_index: u32,
        /// Entity-selection operand carried by the target group.
        target_operand_record_index: u32,
        /// Parameter owner carrying the signed target offset.
        offset_owner_record_index: u32,
        /// Two marked references inserted in the fixed operation section.
        reference_record_indices: [u32; 2],
    },
}

/// Fixed construction carried by a sheet-metal `EdgeFlange` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "DesignEdgeFlangeOperationSerde"))]
#[serde(
    try_from = "DesignEdgeFlangeOperationSerde",
    into = "DesignEdgeFlangeOperationSerde"
)]
pub struct DesignEdgeFlangeOperation {
    /// Per-edge selection-wrapper records in source order.
    pub edge_wrapper_record_indices: Vec<u32>,
    /// Per-edge role-`0x08` operand-group records parallel to the wrappers.
    pub edge_group_record_indices: Vec<u32>,
    /// Per-edge recipe-backed operand records parallel to the wrappers.
    pub edge_operand_record_indices: Vec<u32>,
    /// Role-`0x43` aggregate operand-group record.
    pub aggregate_group_record_index: u32,
    /// Recipe-backed aggregate operands in source order.
    pub aggregate_operand_record_indices: Vec<u32>,
    /// Height parameter-owner record.
    pub height_owner_record_index: u32,
    /// Height extent law and, for a to-object form, its target records.
    #[serde(default)]
    pub height_extent: DesignEdgeFlangeHeightExtent,
    /// Angle parameter-owner record.
    pub angle_owner_record_index: u32,
    /// Width law and the owner records it names.
    pub width: DesignEdgeWidth,
    /// Scope references retained by a classed layout after typed roles and
    /// width owners have been claimed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auxiliary_reference_record_indices: Vec<u32>,
    /// Source-kind convention used by the width owners.
    #[serde(default)]
    pub width_parameter_source: DesignEdgeFlangeWidthParameterSource,
    /// Indexed operation-settings record.
    pub settings_record_index: u32,
    /// Positive rule-derived inside bend radius in centimetres.
    pub bend_radius: f64,
    /// Byte offset of `bend_radius`.
    pub bend_radius_offset: u64,
    /// Face pair the flange height is measured from.
    pub height_datum: DesignSheetMetalHeightDatum,
    /// Bend position relative to the selected edge.
    pub bend_position: DesignBendPosition,
}

impl DesignEdgeFlangeOperation {
    #[must_use]
    pub fn edge_width_mode(&self) -> DesignEdgeWidthMode {
        self.width.mode()
    }

    pub(crate) fn width_distance_owner_record_indices(&self) -> Vec<u32> {
        self.width.owner_indices()
    }

    pub(crate) fn width_distance_owner_record_indices_by_edge(&self) -> Vec<[u32; 2]> {
        self.width.owner_indices_by_edge()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignEdgeFlangeOperationSerde {
    edge_wrapper_record_indices: Vec<u32>,
    edge_group_record_indices: Vec<u32>,
    edge_operand_record_indices: Vec<u32>,
    aggregate_group_record_index: u32,
    aggregate_operand_record_indices: Vec<u32>,
    height_owner_record_index: u32,
    #[serde(default)]
    height_extent: DesignEdgeFlangeHeightExtent,
    angle_owner_record_index: u32,
    #[serde(default)]
    width_mode: Option<DesignEdgeWidthMode>,
    width_distance_owner_record_indices: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    width_distance_owner_record_indices_by_edge: Vec<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    auxiliary_reference_record_indices: Vec<u32>,
    #[serde(default)]
    width_parameter_source: DesignEdgeFlangeWidthParameterSource,
    settings_record_index: u32,
    bend_radius: f64,
    bend_radius_offset: u64,
    reference_side_code: u32,
    height_datum: DesignSheetMetalHeightDatum,
    bend_position: DesignBendPosition,
}

impl TryFrom<DesignEdgeFlangeOperationSerde> for DesignEdgeFlangeOperation {
    type Error = String;

    fn try_from(wire: DesignEdgeFlangeOperationSerde) -> Result<Self, Self::Error> {
        if wire.reference_side_code != 4 {
            return Err("reference_side_code must be 4".into());
        }
        let width = DesignEdgeWidth::from_wire(
            wire.width_mode,
            wire.width_distance_owner_record_indices,
            wire.width_distance_owner_record_indices_by_edge,
        )?;
        Ok(Self {
            edge_wrapper_record_indices: wire.edge_wrapper_record_indices,
            edge_group_record_indices: wire.edge_group_record_indices,
            edge_operand_record_indices: wire.edge_operand_record_indices,
            aggregate_group_record_index: wire.aggregate_group_record_index,
            aggregate_operand_record_indices: wire.aggregate_operand_record_indices,
            height_owner_record_index: wire.height_owner_record_index,
            height_extent: wire.height_extent,
            angle_owner_record_index: wire.angle_owner_record_index,
            width,
            auxiliary_reference_record_indices: wire.auxiliary_reference_record_indices,
            width_parameter_source: wire.width_parameter_source,
            settings_record_index: wire.settings_record_index,
            bend_radius: wire.bend_radius,
            bend_radius_offset: wire.bend_radius_offset,
            height_datum: wire.height_datum,
            bend_position: wire.bend_position,
        })
    }
}

impl From<DesignEdgeFlangeOperation> for DesignEdgeFlangeOperationSerde {
    fn from(operation: DesignEdgeFlangeOperation) -> Self {
        let width_mode = Some(operation.width.mode());
        let width_distance_owner_record_indices = operation.width.owner_indices();
        let width_distance_owner_record_indices_by_edge = operation.width.owner_indices_by_edge();
        Self {
            edge_wrapper_record_indices: operation.edge_wrapper_record_indices,
            edge_group_record_indices: operation.edge_group_record_indices,
            edge_operand_record_indices: operation.edge_operand_record_indices,
            aggregate_group_record_index: operation.aggregate_group_record_index,
            aggregate_operand_record_indices: operation.aggregate_operand_record_indices,
            height_owner_record_index: operation.height_owner_record_index,
            height_extent: operation.height_extent,
            angle_owner_record_index: operation.angle_owner_record_index,
            width_mode,
            width_distance_owner_record_indices,
            width_distance_owner_record_indices_by_edge,
            auxiliary_reference_record_indices: operation.auxiliary_reference_record_indices,
            width_parameter_source: operation.width_parameter_source,
            settings_record_index: operation.settings_record_index,
            bend_radius: operation.bend_radius,
            bend_radius_offset: operation.bend_radius_offset,
            reference_side_code: 4,
            height_datum: operation.height_datum,
            bend_position: operation.bend_position,
        }
    }
}

/// Parameter-owner layout carried by a sheet-metal `Hem` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesignHemParameterOwners {
    /// Flat and open forms own a gap and a length.
    GapLength {
        /// Gap parameter-owner record.
        gap_owner_record_index: u32,
        /// Length parameter-owner record.
        length_owner_record_index: u32,
    },
    /// Rolled form owns a radius and an angle.
    RadiusAngle {
        /// Radius parameter-owner record.
        radius_owner_record_index: u32,
        /// Angle parameter-owner record.
        angle_owner_record_index: u32,
    },
    /// Teardrop form owns a gap, a length, and a radius.
    GapLengthRadius {
        /// Gap parameter-owner record.
        gap_owner_record_index: u32,
        /// Length parameter-owner record.
        length_owner_record_index: u32,
        /// Radius parameter-owner record.
        radius_owner_record_index: u32,
    },
}

/// Fixed operation section and parameter-owner layout carried by a sheet-metal
/// `Hem` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "DesignHemOperationWire"))]
#[serde(try_from = "DesignHemOperationWire", into = "DesignHemOperationWire")]
pub struct DesignHemOperation {
    /// Selection-wrapper record for the hem edge.
    pub edge_wrapper_record_index: u32,
    /// Role-`0x08` operand-group record.
    pub edge_group_record_index: u32,
    /// Recipe-backed role-`0x08` operand record.
    pub edge_operand_record_index: u32,
    /// Role-`0x43` aggregate operand-group record.
    pub aggregate_group_record_index: u32,
    /// Recipe-backed role-`0x43` operand record.
    pub aggregate_operand_record_index: u32,
    /// Parameter-owner layout selected by the owned source kinds.
    pub parameter_owners: DesignHemParameterOwners,
    /// Indexed operation-settings record.
    pub settings_record_index: u32,
    /// Positive rule-derived inside bend radius in centimetres.
    pub bend_radius: f64,
    /// Byte offset of `bend_radius`.
    pub bend_radius_offset: u64,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignHemOperationWire {
    /// Selection-wrapper record for the hem edge.
    edge_wrapper_record_index: u32,
    /// Role-`0x08` operand-group record.
    edge_group_record_index: u32,
    /// Recipe-backed role-`0x08` operand record.
    edge_operand_record_index: u32,
    /// Role-`0x43` aggregate operand-group record.
    aggregate_group_record_index: u32,
    /// Recipe-backed role-`0x43` operand record.
    aggregate_operand_record_index: u32,
    /// Parameter-owner layout selected by the owned source kinds.
    parameter_owners: DesignHemParameterOwners,
    /// Indexed operation-settings record.
    settings_record_index: u32,
    /// Positive rule-derived inside bend radius in centimetres.
    bend_radius: f64,
    /// Byte offset of `bend_radius`.
    bend_radius_offset: u64,
    form_code: u32,
    direction_code: u32,
    direction_reversal_byte: u8,
    reference_side_code: u32,
}

impl TryFrom<DesignHemOperationWire> for DesignHemOperation {
    type Error = String;

    fn try_from(wire: DesignHemOperationWire) -> Result<Self, Self::Error> {
        if wire.form_code != 3 {
            return Err("form_code must be 3".into());
        }
        if wire.direction_code != 1 {
            return Err("direction_code must be 1".into());
        }
        if wire.direction_reversal_byte != 0 {
            return Err("direction_reversal_byte must be 0".into());
        }
        if wire.reference_side_code != 4 {
            return Err("reference_side_code must be 4".into());
        }
        Ok(Self {
            edge_wrapper_record_index: wire.edge_wrapper_record_index,
            edge_group_record_index: wire.edge_group_record_index,
            edge_operand_record_index: wire.edge_operand_record_index,
            aggregate_group_record_index: wire.aggregate_group_record_index,
            aggregate_operand_record_index: wire.aggregate_operand_record_index,
            parameter_owners: wire.parameter_owners,
            settings_record_index: wire.settings_record_index,
            bend_radius: wire.bend_radius,
            bend_radius_offset: wire.bend_radius_offset,
        })
    }
}

impl From<DesignHemOperation> for DesignHemOperationWire {
    fn from(record: DesignHemOperation) -> Self {
        Self {
            edge_wrapper_record_index: record.edge_wrapper_record_index,
            edge_group_record_index: record.edge_group_record_index,
            edge_operand_record_index: record.edge_operand_record_index,
            aggregate_group_record_index: record.aggregate_group_record_index,
            aggregate_operand_record_index: record.aggregate_operand_record_index,
            parameter_owners: record.parameter_owners,
            settings_record_index: record.settings_record_index,
            bend_radius: record.bend_radius,
            bend_radius_offset: record.bend_radius_offset,
            form_code: 3,
            direction_code: 1,
            direction_reversal_byte: 0,
            reference_side_code: 4,
        }
    }
}

/// Fixed construction carried by a uniform body-scale scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignScaleOperationWire", into = "DesignScaleOperationWire")]
pub struct DesignScaleOperation {
    /// Counted construction group selecting the transformed bodies.
    pub body_group_record_index: u32,
    /// Native reference selecting the fixed scale center.
    pub center_record_index: u32,
    /// Explicit center position carried by legacy point-data centers, in source
    /// model centimetres.
    pub center_position: Option<Located<[f64; 3]>>,
    /// Positive uniform scale factor.
    pub uniform_factor: f64,
    /// Byte offset of `uniform_factor`.
    pub uniform_factor_offset: u64,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignScaleOperationWire {
    /// Counted construction group selecting the transformed bodies.
    body_group_record_index: u32,
    /// Native reference selecting the fixed scale center.
    center_record_index: u32,
    /// Explicit center position carried by legacy point-data centers, in source
    /// model centimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    center_position: Option<[f64; 3]>,
    /// Byte offset of the explicit center position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    center_position_offset: Option<u64>,
    /// Positive uniform scale factor.
    uniform_factor: f64,
    /// Byte offset of `uniform_factor`.
    uniform_factor_offset: u64,
}

impl From<DesignScaleOperation> for DesignScaleOperationWire {
    fn from(value: DesignScaleOperation) -> Self {
        Self {
            body_group_record_index: value.body_group_record_index,
            center_record_index: value.center_record_index,
            center_position: value.center_position.map(|center| center.value),
            center_position_offset: value.center_position.map(|center| center.offset),
            uniform_factor: value.uniform_factor,
            uniform_factor_offset: value.uniform_factor_offset,
        }
    }
}

impl TryFrom<DesignScaleOperationWire> for DesignScaleOperation {
    type Error = String;
    fn try_from(value: DesignScaleOperationWire) -> Result<Self, Self::Error> {
        Ok(Self {
            body_group_record_index: value.body_group_record_index,
            center_record_index: value.center_record_index,
            center_position: Located::from_wire(value.center_position, value.center_position_offset, "center_position")?,
            uniform_factor: value.uniform_factor,
            uniform_factor_offset: value.uniform_factor_offset,
        })
    }
}

/// Source and copied Design body identities carried by `CopyPasteBodies`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "DesignCopyPasteBodiesOperationWire"))]
#[serde(try_from = "DesignCopyPasteBodiesOperationWire", into = "DesignCopyPasteBodiesOperationWire")]
pub struct DesignCopyPasteBodiesOperation {
    pub bodies: Vec<DesignCopiedBody>,
    /// Counted body-selection group named by the scope prefix and reference table.
    pub body_group_record_index: u32,
    /// Dynamic class tag of the body group's primary header.
    pub body_group_class_tag: String,
    /// Byte offset of the body group's primary header.
    pub body_group_byte_offset: u64,
    /// Indexed source-to-copy relation record named by the scope prefix.
    pub relation_record_index: u32,
    /// Dynamic class tag of the relation record's primary header.
    pub relation_class_tag: String,
    /// Byte offset of the relation record's primary header.
    pub relation_byte_offset: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignCopiedBody {
    pub operand: Located<u32>,
    pub source: Located<u32>,
    pub copied: Located<u32>,
}

/// Source and copied Design body identities carried by `CopyPasteBodies`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignCopyPasteBodiesOperationWire {
    /// Counted body-selection group named by the scope prefix and reference table.
    body_group_record_index: u32,
    /// Dynamic class tag of the body group's primary header.
    body_group_class_tag: String,
    /// Byte offset of the body group's primary header.
    body_group_byte_offset: u64,
    /// Ordered body-operand records carried by the counted group.
    body_operand_record_indices: Vec<u32>,
    /// Byte offsets parallel to `body_operand_record_indices`.
    body_operand_record_offsets: Vec<u64>,
    /// Indexed source-to-copy relation record named by the scope prefix.
    relation_record_index: u32,
    /// Dynamic class tag of the relation record's primary header.
    relation_class_tag: String,
    /// Byte offset of the relation record's primary header.
    relation_byte_offset: u64,
    /// Source Design body entity suffixes in copy order.
    source_body_entity_suffixes: Vec<u32>,
    /// Byte offsets parallel to `source_body_entity_suffixes`.
    source_body_entity_suffix_offsets: Vec<u64>,
    /// Newly copied Design body entity suffixes parallel to the sources.
    copied_body_entity_suffixes: Vec<u32>,
    /// Byte offsets parallel to `copied_body_entity_suffixes`.
    copied_body_entity_suffix_offsets: Vec<u64>,
}

impl TryFrom<DesignCopyPasteBodiesOperationWire> for DesignCopyPasteBodiesOperation {
    type Error = String;
    fn try_from(wire: DesignCopyPasteBodiesOperationWire) -> Result<Self, Self::Error> {
        let count = wire.body_operand_record_indices.len();
        if wire.body_operand_record_offsets.len() != count { return Err("body_operand_record_offsets must match body_operand_record_indices".into()); }
        if wire.source_body_entity_suffixes.len() != count { return Err("source_body_entity_suffixes must match body_operand_record_indices".into()); }
        if wire.source_body_entity_suffix_offsets.len() != count { return Err("source_body_entity_suffix_offsets must match body_operand_record_indices".into()); }
        if wire.copied_body_entity_suffixes.len() != count { return Err("copied_body_entity_suffixes must match body_operand_record_indices".into()); }
        if wire.copied_body_entity_suffix_offsets.len() != count { return Err("copied_body_entity_suffix_offsets must match body_operand_record_indices".into()); }
        let bodies = wire.body_operand_record_indices.into_iter()
            .zip(wire.body_operand_record_offsets)
            .zip(wire.source_body_entity_suffixes.into_iter().zip(wire.source_body_entity_suffix_offsets))
            .zip(wire.copied_body_entity_suffixes.into_iter().zip(wire.copied_body_entity_suffix_offsets))
            .map(|(((value, offset), (source, source_offset)), (copied, copied_offset))| DesignCopiedBody {
                operand: Located { value, offset },
                source: Located { value: source, offset: source_offset },
                copied: Located { value: copied, offset: copied_offset },
            }).collect();
        Ok(Self { bodies,
            body_group_record_index: wire.body_group_record_index,
            body_group_class_tag: wire.body_group_class_tag,
            body_group_byte_offset: wire.body_group_byte_offset,
            relation_record_index: wire.relation_record_index,
            relation_class_tag: wire.relation_class_tag,
            relation_byte_offset: wire.relation_byte_offset,
        })
    }
}
impl From<DesignCopyPasteBodiesOperation> for DesignCopyPasteBodiesOperationWire {
    fn from(value: DesignCopyPasteBodiesOperation) -> Self {
        Self {
            body_group_record_index: value.body_group_record_index,
            body_group_class_tag: value.body_group_class_tag,
            body_group_byte_offset: value.body_group_byte_offset,
            relation_record_index: value.relation_record_index,
            relation_class_tag: value.relation_class_tag,
            relation_byte_offset: value.relation_byte_offset,
            body_operand_record_indices: value.bodies.iter().map(|body| body.operand.value).collect(),
            body_operand_record_offsets: value.bodies.iter().map(|body| body.operand.offset).collect(),
            source_body_entity_suffixes: value.bodies.iter().map(|body| body.source.value).collect(),
            source_body_entity_suffix_offsets: value.bodies.iter().map(|body| body.source.offset).collect(),
            copied_body_entity_suffixes: value.bodies.iter().map(|body| body.copied.value).collect(),
            copied_body_entity_suffix_offsets: value.bodies.iter().map(|body| body.copied.offset).collect(),
        }
    }
}

/// Layout of the legacy class-452/class-262 Base Feature envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum DesignBaseFeatureBodyReferenceForm {
    /// One output body with an encoded compact mode.
    CompactOneBody { mode: Located<u8>, body: DesignLegacyBaseFeatureBody },
    /// Two output bodies with no mode slot.
    ExpandedTwoBody { bodies: [DesignLegacyBaseFeatureBody; 2] },
}

/// One body in a legacy Base Feature envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignLegacyBaseFeatureBody {
    pub entity: DesignBaseFeatureEntry<u32>,
    pub parameter_body: Located<u64>,
    pub auxiliary: Located<u64>,
}

impl DesignBaseFeatureBodyReferenceForm {
    fn bodies(&self) -> &[DesignLegacyBaseFeatureBody] {
        match self {
            Self::CompactOneBody { body, .. } => std::slice::from_ref(body),
            Self::ExpandedTwoBody { bodies } => bodies,
        }
    }
}

/// Typed construction data carried by a Fusion direct-modeling Base Feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignBaseFeatureConstructionWire", into = "DesignBaseFeatureConstructionWire")]
pub enum DesignBaseFeatureConstruction {
    /// Counted body, passive-reference, metadata, and result runs.
    ResultBodies {
        /// Ordered body, passive-reference, result, and optional repeated-field rows.
        bodies: DesignBaseFeatureResults,
        /// Shared passive-reference metadata record.
        metadata_record: u32,
        /// Byte offset of the shared metadata record.
        metadata_record_offset: u64,
        /// Variant-width source field following the metadata record.
        metadata_field: Vec<u8>,
    },
    /// Direct-modeling body-reference envelope used by the class-365/class-262 and
    /// class-377/class-259 forms.
    BodyBasedOnFaces {
        /// The single body suffix and the location shared by its reference views.
        body: Located<u32>,
        /// PM body-reference record named by the fixed envelope lane.
        parameter_body_record: u32,
        /// Byte offset of `parameter_body_record`.
        parameter_body_record_offset: u64,
        /// Auxiliary record named by the fixed envelope lane.
        auxiliary_record: u32,
        /// Byte offset of `auxiliary_record`.
        auxiliary_record_offset: u64,
        /// LP-UTF-16 GUID carried by the envelope.
        envelope_guid: String,
        /// Byte offset of the first code unit of `envelope_guid`.
        envelope_guid_offset: u64,
        /// Byte offset of `tag_body_based_on_faces`.
        tag_body_based_on_faces_offset: u64,
    },
    /// Legacy body-reference envelope used by the class-452/class-262 forms.
    LegacyBodyBasedOnFaces {
        /// Compact one-body or expanded two-body source envelope form.
        form: DesignBaseFeatureBodyReferenceForm,
        /// Scope record repeated by the envelope's explicit scope-reference lane.
        scope_reference: u64,
        /// Byte offset of `scope_reference`.
        scope_reference_offset: u64,
        /// LP-UTF-16 GUID carried by the envelope.
        envelope_guid: String,
        /// Byte offset of the first code unit of `envelope_guid`.
        envelope_guid_offset: u64,
        /// Byte offset of `tag_body_based_on_faces`.
        tag_body_based_on_faces_offset: u64,
    },
    /// Body snapshot form used by the class-314/class-259 scope pair.
    BodySnapshot {
        /// Ordered snapshot bodies with their source fields.
        bodies: Vec<DesignBaseFeatureEntry<u64>>,
        /// Three LP-UTF-16 source GUIDs carried by the snapshot envelope.
        related_guids: [String; 3],
        /// Byte offsets of the first code unit of each related GUID.
        related_guid_offsets: [u64; 3],
        /// Indexed record carried by the snapshot linkage tail.
        linkage_record: u32,
        /// Byte offset of `linkage_record`.
        linkage_record_offset: u64,
        /// Auxiliary indexed record carried by the snapshot linkage tail.
        auxiliary_record: u32,
        /// Byte offset of `auxiliary_record`.
        auxiliary_record_offset: u64,
    },
}

/// One aligned body, passive reference, and result record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignBaseFeatureResultBody {
    pub entity: DesignBaseFeatureEntry<u64>,
    pub reference: DesignBaseFeatureEntry<u32>,
    pub result: DesignBaseFeatureEntry<u32>,
}

/// Result-body runs with either no repeated fields or one field per body.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum DesignBaseFeatureResults {
    WithoutRepeatedFields(Vec<DesignBaseFeatureResultBody>),
    WithRepeatedFields {
        first: (DesignBaseFeatureResultBody, [u8; 6]),
        rest: Vec<(DesignBaseFeatureResultBody, [u8; 6])>,
    },
}

impl DesignBaseFeatureResults {
    pub(crate) fn iter(&self) -> impl ExactSizeIterator<Item = &DesignBaseFeatureResultBody> {
        let count = match self {
            Self::WithoutRepeatedFields(bodies) => bodies.len(),
            Self::WithRepeatedFields { rest, .. } => 1 + rest.len(),
        };
        (0..count).map(move |index| match self {
            Self::WithoutRepeatedFields(bodies) => &bodies[index],
            Self::WithRepeatedFields { first, .. } if index == 0 => &first.0,
            Self::WithRepeatedFields { rest, .. } => &rest[index - 1].0,
        })
    }
}

/// One Base Feature reference value, its location, and its six-byte field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignBaseFeatureEntry<T> {
    pub value: T,
    pub offset: u64,
    pub field: [u8; 6],
}

/// Wire form of the legacy class-452/class-262 Base Feature body-reference
/// envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
enum DesignBaseFeatureBodyReferenceFormWire {
    /// One output body with 64-bit references in the legacy compact lanes.
    CompactOneBody,
    /// Two output bodies with counted 32-bit reference runs.
    ExpandedTwoBody,
}

/// Typed construction data carried by a Fusion direct-modeling Base Feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
// Untagged: required field sets are disjoint across variants.
#[serde(untagged)]
enum DesignBaseFeatureConstructionWire {
    /// Counted body, passive-reference, metadata, and result runs.
    ResultBodies {
        /// Ordered Design body entity suffixes exposed by the Base Feature.
        body_entity_suffixes: Vec<u64>,
        /// Byte offsets parallel to `body_entity_suffixes`.
        body_entity_suffix_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `body_entity_suffixes`.
        body_entity_fields: Vec<[u8; 6]>,
        /// Ordered passive body-reference records parallel to the body suffixes.
        body_reference_records: Vec<u32>,
        /// Byte offsets parallel to `body_reference_records`.
        body_reference_record_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `body_reference_records`.
        body_reference_fields: Vec<[u8; 6]>,
        /// Six-byte source fields in the repeated passive-reference run.
        repeated_reference_fields: Vec<[u8; 6]>,
        /// Shared passive-reference metadata record.
        metadata_record: u32,
        /// Byte offset of `metadata_record`.
        metadata_record_offset: u64,
        /// Variant-width source field following `metadata_record`.
        metadata_field: Vec<u8>,
        /// Ordered result-body join records parallel to the body suffixes.
        result_records: Vec<u32>,
        /// Byte offsets parallel to `result_records`.
        result_record_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `result_records`.
        result_fields: Vec<[u8; 6]>,
    },
    /// Direct-modeling body-reference envelope used by the class-365/class-262 and
    /// class-377/class-259 forms.
    BodyBasedOnFaces {
        /// The Design body entity suffix exposed by the envelope.
        body_entity_suffixes: Vec<u64>,
        /// Byte offsets parallel to `body_entity_suffixes`.
        body_entity_suffix_offsets: Vec<u64>,
        /// Body suffixes used by history-to-BREP resolution for this form.
        body_reference_records: Vec<u32>,
        /// Byte offsets parallel to `body_reference_records`.
        body_reference_record_offsets: Vec<u64>,
        /// PM body-reference record named by the fixed envelope lane.
        parameter_body_record: u32,
        /// Byte offset of `parameter_body_record`.
        parameter_body_record_offset: u64,
        /// Auxiliary record named by the fixed envelope lane.
        auxiliary_record: u32,
        /// Byte offset of `auxiliary_record`.
        auxiliary_record_offset: u64,
        /// LP-UTF-16 GUID carried by the envelope.
        envelope_guid: String,
        /// Byte offset of the first code unit of `envelope_guid`.
        envelope_guid_offset: u64,
        /// Stored body-source property value.
        tag_body_based_on_faces: bool,
        /// Byte offset of `tag_body_based_on_faces`.
        tag_body_based_on_faces_offset: u64,
    },
    /// Legacy body-reference envelope used by the class-452/class-262 forms.
    LegacyBodyBasedOnFaces {
        /// Compact one-body or expanded two-body source envelope form.
        form: DesignBaseFeatureBodyReferenceFormWire,
        /// Compact-form mode byte. The expanded form has no mode byte.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<u8>,
        /// Byte offset of the compact-form mode byte.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode_offset: Option<u64>,
        /// Ordered Design body entity suffixes exposed by the envelope.
        body_entity_suffixes: Vec<u64>,
        /// Byte offsets parallel to `body_entity_suffixes`.
        body_entity_suffix_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `body_entity_suffixes`.
        body_entity_fields: Vec<[u8; 6]>,
        /// Body suffixes used by history-to-BREP resolution for this form.
        body_reference_records: Vec<u32>,
        /// Byte offsets parallel to `body_reference_records`.
        body_reference_record_offsets: Vec<u64>,
        /// Ordered PM body-reference records carried by the envelope.
        parameter_body_records: Vec<u64>,
        /// Byte offsets parallel to `parameter_body_records`.
        parameter_body_record_offsets: Vec<u64>,
        /// Ordered DM body-reference records carried by the envelope.
        auxiliary_records: Vec<u64>,
        /// Byte offsets parallel to `auxiliary_records`.
        auxiliary_record_offsets: Vec<u64>,
        /// Scope record repeated by the envelope's explicit scope-reference lane.
        scope_reference: u64,
        /// Byte offset of `scope_reference`.
        scope_reference_offset: u64,
        /// LP-UTF-16 GUID carried by the envelope.
        envelope_guid: String,
        /// Byte offset of the first code unit of `envelope_guid`.
        envelope_guid_offset: u64,
        /// Stored body-source property value.
        tag_body_based_on_faces: bool,
        /// Byte offset of `tag_body_based_on_faces`.
        tag_body_based_on_faces_offset: u64,
    },
    /// Body snapshot form used by the class-314/class-259 scope pair.
    BodySnapshot {
        /// Ordered Design body entity suffixes exposed by the snapshot.
        body_entity_suffixes: Vec<u64>,
        /// Byte offsets parallel to `body_entity_suffixes`.
        body_entity_suffix_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `body_entity_suffixes`.
        body_entity_fields: Vec<[u8; 6]>,
        /// Three LP-UTF-16 source GUIDs carried by the snapshot envelope.
        related_guids: [String; 3],
        /// Byte offsets of the first code unit of each related GUID.
        related_guid_offsets: [u64; 3],
        /// Indexed record carried by the snapshot linkage tail.
        linkage_record: u32,
        /// Byte offset of `linkage_record`.
        linkage_record_offset: u64,
        /// Auxiliary indexed record carried by the snapshot linkage tail.
        auxiliary_record: u32,
        /// Byte offset of `auxiliary_record`.
        auxiliary_record_offset: u64,
    },
}

impl TryFrom<DesignBaseFeatureConstructionWire> for DesignBaseFeatureConstruction {
    type Error = String;
    fn try_from(wire: DesignBaseFeatureConstructionWire) -> Result<Self, Self::Error> {
        Ok(match wire {
            DesignBaseFeatureConstructionWire::ResultBodies { body_entity_suffixes, body_entity_suffix_offsets, body_entity_fields, body_reference_records, body_reference_record_offsets, body_reference_fields, repeated_reference_fields, metadata_record, metadata_record_offset, metadata_field, result_records, result_record_offsets, result_fields } => {
                let count = body_entity_suffixes.len();
                for (field, len) in [
                    ("body_entity_suffix_offsets", body_entity_suffix_offsets.len()),
                    ("body_entity_fields", body_entity_fields.len()),
                    ("body_reference_records", body_reference_records.len()),
                    ("body_reference_record_offsets", body_reference_record_offsets.len()),
                    ("body_reference_fields", body_reference_fields.len()),
                    ("result_records", result_records.len()),
                    ("result_record_offsets", result_record_offsets.len()),
                    ("result_fields", result_fields.len()),
                ] {
                    if len != count {
                        return Err(format!("{field} must have the same length as body_entity_suffixes"));
                    }
                }
                if !repeated_reference_fields.is_empty() && repeated_reference_fields.len() != count {
                    return Err("repeated_reference_fields must be empty or have the same length as body_entity_suffixes".into());
                }
                let bodies = (0..count).map(|index| DesignBaseFeatureResultBody {
                    entity: DesignBaseFeatureEntry { value: body_entity_suffixes[index], offset: body_entity_suffix_offsets[index], field: body_entity_fields[index] },
                    reference: DesignBaseFeatureEntry { value: body_reference_records[index], offset: body_reference_record_offsets[index], field: body_reference_fields[index] },
                    result: DesignBaseFeatureEntry { value: result_records[index], offset: result_record_offsets[index], field: result_fields[index] },
                });
                let bodies = if repeated_reference_fields.is_empty() {
                    DesignBaseFeatureResults::WithoutRepeatedFields(bodies.collect())
                } else {
                    let mut repeated = bodies.zip(repeated_reference_fields);
                    match repeated.next() {
                        Some(first) => DesignBaseFeatureResults::WithRepeatedFields { first, rest: repeated.collect() },
                        None => return Err("repeated_reference_fields require body_entity_suffixes".into()),
                    }
                };
                Self::ResultBodies { bodies, metadata_record, metadata_record_offset, metadata_field }
            },
            DesignBaseFeatureConstructionWire::BodyBasedOnFaces { body_entity_suffixes, body_entity_suffix_offsets, body_reference_records, body_reference_record_offsets, parameter_body_record, parameter_body_record_offset, auxiliary_record, auxiliary_record_offset, envelope_guid, envelope_guid_offset, tag_body_based_on_faces, tag_body_based_on_faces_offset } => {
                let ([suffix], [offset], [reference], [reference_offset]) = (body_entity_suffixes.as_slice(), body_entity_suffix_offsets.as_slice(), body_reference_records.as_slice(), body_reference_record_offsets.as_slice()) else {
                    return Err("body_entity_suffixes, body_entity_suffix_offsets, body_reference_records, and body_reference_record_offsets require one body".into());
                };
                if *suffix != u64::from(*reference) || offset != reference_offset {
                    return Err("body_reference_records and body_reference_record_offsets must match body_entity_suffixes and body_entity_suffix_offsets".into());
                }
                if !tag_body_based_on_faces {
                    return Err("tag_body_based_on_faces must be true for BodyBasedOnFaces".into());
                }
                Self::BodyBasedOnFaces { body: Located { value: *reference, offset: *offset }, parameter_body_record, parameter_body_record_offset, auxiliary_record, auxiliary_record_offset, envelope_guid, envelope_guid_offset, tag_body_based_on_faces_offset }
            },
            DesignBaseFeatureConstructionWire::LegacyBodyBasedOnFaces { form, mode, mode_offset, body_entity_suffixes, body_entity_suffix_offsets, body_entity_fields, body_reference_records, body_reference_record_offsets, parameter_body_records, parameter_body_record_offsets, auxiliary_records, auxiliary_record_offsets, scope_reference, scope_reference_offset, envelope_guid, envelope_guid_offset, tag_body_based_on_faces, tag_body_based_on_faces_offset } => {
                let count = body_entity_suffixes.len();
                for (field, len) in [
                    ("body_entity_suffix_offsets", body_entity_suffix_offsets.len()),
                    ("body_entity_fields", body_entity_fields.len()),
                    ("body_reference_records", body_reference_records.len()),
                    ("body_reference_record_offsets", body_reference_record_offsets.len()),
                    ("parameter_body_records", parameter_body_records.len()),
                    ("parameter_body_record_offsets", parameter_body_record_offsets.len()),
                    ("auxiliary_records", auxiliary_records.len()),
                    ("auxiliary_record_offsets", auxiliary_record_offsets.len()),
                ] {
                    if len != count {
                        return Err(format!("{field} must have the same length as body_entity_suffixes"));
                    }
                }
                if !tag_body_based_on_faces {
                    return Err("tag_body_based_on_faces must be true for LegacyBodyBasedOnFaces".into());
                }
                let mut bodies = Vec::with_capacity(count);
                for index in 0..count {
                    if body_entity_suffixes[index] != u64::from(body_reference_records[index]) || body_entity_suffix_offsets[index] != body_reference_record_offsets[index] {
                        return Err("body_reference_records and body_reference_record_offsets must match body_entity_suffixes and body_entity_suffix_offsets".into());
                    }
                    bodies.push(DesignLegacyBaseFeatureBody {
                        entity: DesignBaseFeatureEntry { value: body_reference_records[index], offset: body_entity_suffix_offsets[index], field: body_entity_fields[index] },
                        parameter_body: Located { value: parameter_body_records[index], offset: parameter_body_record_offsets[index] },
                        auxiliary: Located { value: auxiliary_records[index], offset: auxiliary_record_offsets[index] },
                    });
                }
                let form = match (form, mode, mode_offset, bodies.as_slice()) {
                    (DesignBaseFeatureBodyReferenceFormWire::CompactOneBody, Some(value), Some(offset), [body]) => DesignBaseFeatureBodyReferenceForm::CompactOneBody { mode: Located { value, offset }, body: *body },
                    (DesignBaseFeatureBodyReferenceFormWire::ExpandedTwoBody, None, None, [first, second]) => DesignBaseFeatureBodyReferenceForm::ExpandedTwoBody { bodies: [*first, *second] },
                    _ => return Err("form requires one body with mode and mode_offset for compact_one_body, or two bodies without mode for expanded_two_body".into()),
                };
                Self::LegacyBodyBasedOnFaces { form, scope_reference, scope_reference_offset, envelope_guid, envelope_guid_offset, tag_body_based_on_faces_offset }
            },
            DesignBaseFeatureConstructionWire::BodySnapshot { body_entity_suffixes, body_entity_suffix_offsets, body_entity_fields, related_guids, related_guid_offsets, linkage_record, linkage_record_offset, auxiliary_record, auxiliary_record_offset } => {
                if body_entity_suffixes.len() != body_entity_suffix_offsets.len() || body_entity_suffixes.len() != body_entity_fields.len() {
                    return Err("body_entity_suffixes, body_entity_suffix_offsets, and body_entity_fields must have equal lengths".into());
                }
                let bodies = body_entity_suffixes.into_iter().zip(body_entity_suffix_offsets).zip(body_entity_fields)
                    .map(|((value, offset), field)| DesignBaseFeatureEntry { value, offset, field }).collect();
                Self::BodySnapshot { bodies, related_guids, related_guid_offsets, linkage_record, linkage_record_offset, auxiliary_record, auxiliary_record_offset }
            },
        })
    }
}

impl From<DesignBaseFeatureConstruction> for DesignBaseFeatureConstructionWire {
    fn from(value: DesignBaseFeatureConstruction) -> Self {
        match value {
            DesignBaseFeatureConstruction::ResultBodies { bodies, metadata_record, metadata_record_offset, metadata_field } => {
                let body_entity_suffixes = bodies.iter().map(|body| body.entity.value).collect();
                let body_entity_suffix_offsets = bodies.iter().map(|body| body.entity.offset).collect();
                let body_entity_fields = bodies.iter().map(|body| body.entity.field).collect();
                let body_reference_records = bodies.iter().map(|body| body.reference.value).collect();
                let body_reference_record_offsets = bodies.iter().map(|body| body.reference.offset).collect();
                let body_reference_fields = bodies.iter().map(|body| body.reference.field).collect();
                let result_records = bodies.iter().map(|body| body.result.value).collect();
                let result_record_offsets = bodies.iter().map(|body| body.result.offset).collect();
                let result_fields = bodies.iter().map(|body| body.result.field).collect();
                let repeated_reference_fields = match bodies {
                    DesignBaseFeatureResults::WithoutRepeatedFields(_) => Vec::new(),
                    DesignBaseFeatureResults::WithRepeatedFields { first, rest } => std::iter::once(first.1).chain(rest.into_iter().map(|(_, field)| field)).collect(),
                };
                Self::ResultBodies { body_entity_suffixes, body_entity_suffix_offsets, body_entity_fields, body_reference_records, body_reference_record_offsets, body_reference_fields, repeated_reference_fields, metadata_record, metadata_record_offset, metadata_field, result_records, result_record_offsets, result_fields }
            },
            DesignBaseFeatureConstruction::BodyBasedOnFaces { body, parameter_body_record, parameter_body_record_offset, auxiliary_record, auxiliary_record_offset, envelope_guid, envelope_guid_offset, tag_body_based_on_faces_offset } => {
                Self::BodyBasedOnFaces { body_entity_suffixes: vec![u64::from(body.value)], body_entity_suffix_offsets: vec![body.offset], body_reference_records: vec![body.value], body_reference_record_offsets: vec![body.offset], tag_body_based_on_faces: true, parameter_body_record, parameter_body_record_offset, auxiliary_record, auxiliary_record_offset, envelope_guid, envelope_guid_offset, tag_body_based_on_faces_offset }
            },
            DesignBaseFeatureConstruction::LegacyBodyBasedOnFaces { form, scope_reference, scope_reference_offset, envelope_guid, envelope_guid_offset, tag_body_based_on_faces_offset } => {
                let bodies = form.bodies();
                let body_entity_suffixes = bodies.iter().map(|body| u64::from(body.entity.value)).collect();
                let body_entity_suffix_offsets = bodies.iter().map(|body| body.entity.offset).collect();
                let body_entity_fields = bodies.iter().map(|body| body.entity.field).collect();
                let body_reference_records = bodies.iter().map(|body| body.entity.value).collect();
                let body_reference_record_offsets = bodies.iter().map(|body| body.entity.offset).collect();
                let parameter_body_records = bodies.iter().map(|body| body.parameter_body.value).collect();
                let parameter_body_record_offsets = bodies.iter().map(|body| body.parameter_body.offset).collect();
                let auxiliary_records = bodies.iter().map(|body| body.auxiliary.value).collect();
                let auxiliary_record_offsets = bodies.iter().map(|body| body.auxiliary.offset).collect();
                let (form, mode, mode_offset) = match form {
                    DesignBaseFeatureBodyReferenceForm::CompactOneBody { mode, .. } => (DesignBaseFeatureBodyReferenceFormWire::CompactOneBody, Some(mode.value), Some(mode.offset)),
                    DesignBaseFeatureBodyReferenceForm::ExpandedTwoBody { .. } => (DesignBaseFeatureBodyReferenceFormWire::ExpandedTwoBody, None, None),
                };
                Self::LegacyBodyBasedOnFaces { form, mode, mode_offset, body_entity_suffixes, body_entity_suffix_offsets, body_entity_fields, body_reference_records, body_reference_record_offsets, parameter_body_records, parameter_body_record_offsets, auxiliary_records, auxiliary_record_offsets, scope_reference, scope_reference_offset, envelope_guid, envelope_guid_offset, tag_body_based_on_faces: true, tag_body_based_on_faces_offset }
            },
            DesignBaseFeatureConstruction::BodySnapshot { bodies, related_guids, related_guid_offsets, linkage_record, linkage_record_offset, auxiliary_record, auxiliary_record_offset } => {
                let mut body_entity_suffixes = Vec::with_capacity(bodies.len());
                let mut body_entity_suffix_offsets = Vec::with_capacity(bodies.len());
                let mut body_entity_fields = Vec::with_capacity(bodies.len());
                for body in bodies {
                    body_entity_suffixes.push(body.value);
                    body_entity_suffix_offsets.push(body.offset);
                    body_entity_fields.push(body.field);
                }
                Self::BodySnapshot { body_entity_suffixes, body_entity_suffix_offsets, body_entity_fields, related_guids, related_guid_offsets, linkage_record, linkage_record_offset, auxiliary_record, auxiliary_record_offset }
            },
        }
    }
}

impl DesignBaseFeatureConstruction {
    /// Return the body suffixes in source order for any Base Feature form.
    pub(crate) fn body_entity_suffixes(&self) -> impl ExactSizeIterator<Item = u64> + '_ {
        let count = match self {
            Self::BodySnapshot { bodies, .. } => bodies.len(),
            Self::BodyBasedOnFaces { .. } => 1,
            Self::ResultBodies { bodies, .. } => bodies.iter().len(),
            Self::LegacyBodyBasedOnFaces { form, .. } => form.bodies().len(),
        };
        (0..count).map(move |index| match self {
            Self::BodySnapshot { bodies, .. } => bodies[index].value,
            Self::BodyBasedOnFaces { body, .. } => u64::from(body.value),
            Self::ResultBodies { bodies, .. } => match bodies {
                DesignBaseFeatureResults::WithoutRepeatedFields(bodies) => bodies[index].entity.value,
                DesignBaseFeatureResults::WithRepeatedFields { first, .. } if index == 0 => first.0.entity.value,
                DesignBaseFeatureResults::WithRepeatedFields { rest, .. } => rest[index - 1].0.entity.value,
            },
            Self::LegacyBodyBasedOnFaces { form, .. } => u64::from(form.bodies()[index].entity.value),
        })
    }

    /// Return passive body-reference records for forms that carry them.
    pub(crate) fn body_reference_records(&self) -> impl Iterator<Item = u32> + '_ {
        let (results, legacy, single) = match self {
            Self::ResultBodies { bodies, .. } => (Some(bodies), &[][..], None),
            Self::LegacyBodyBasedOnFaces { form, .. } => (None, form.bodies(), None),
            Self::BodyBasedOnFaces { body, .. } => (None, &[][..], Some(body.value)),
            Self::BodySnapshot { .. } => (None, &[][..], None),
        };
        results.into_iter().flat_map(DesignBaseFeatureResults::iter).map(|body| body.reference.value)
            .chain(legacy.iter().map(|body| body.entity.value)).chain(single)
    }

}

/// Sketch-profile selection frame named by a profile-based feature scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignSketchProfileOperand {
    /// Zero-based position in the scope's ordered reference table.
    pub scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    pub record_index: u32,
    /// Byte offset of the primary indexed-record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: String,
    /// Asset UUID qualifying the selected Sketch reference.
    pub asset_id: String,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// Full Design entity id of the selected Sketch.
    pub entity_id: String,
    /// Numeric suffix stored by the profile frame.
    pub entity_suffix: u64,
    /// Byte offset of the suffix's UTF-16LE code units.
    pub entity_reference_offset: u64,
    /// Exact nested profile-region selection, when its complete frame closes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region_selection: Option<DesignSketchProfileRegionSelection>,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub paired_class_tag: String,
    /// Byte offset of the same-index paired header.
    pub paired_byte_offset: u64,
}

/// Nested ordered region selection carried by a sketch-profile operand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignSketchProfileRegionSelection {
    /// Indexed identity of the region-selection record.
    pub record_index: u32,
    /// Byte offset of the region-selection indexed header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII region-selection class tag.
    pub class_tag: String,
    /// Byte offset of the selected-region count.
    pub region_count_offset: u64,
    /// Selected regions in source order.
    pub regions: Vec<DesignSketchProfileRegion>,
    /// Source per-file dynamic three-digit ASCII companion class tag.
    pub companion_class_tag: String,
    /// Byte offset of the same-index companion header.
    pub companion_byte_offset: u64,
}

/// One selected region in a nested sketch-profile selection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignSketchProfileRegion {
    /// Byte offset of this region's member count.
    pub member_count_offset: u64,
    /// Persistent curve members in source order.
    pub members: Vec<DesignSketchProfileRegionMember>,
}

/// One fixed-width persistent curve member of a selected sketch region.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignSketchProfileRegionMember {
    /// Native member-kind code. Profile-region curve members use value three.
    pub kind: DesignSketchProfileRegionMemberKind,
    /// Byte offset of the member-kind code.
    pub kind_offset: u64,
    /// Primary persistent identity of the selected Sketch curve.
    pub curve_primary_id: u64,
    /// Byte offset of the persistent curve identity.
    pub curve_primary_id_offset: u64,
    /// Structural region-incidence words retained in source order.
    pub incidence_words: [u32; 8],
    /// Byte offset of the first incidence word.
    pub incidence_words_offset: u64,
}

/// Counted selection group owned by an Extrude parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignExtrudeSelectionGroupWire", into = "DesignExtrudeSelectionGroupWire")]
pub struct DesignExtrudeSelectionGroup {
    /// Globally unique deterministic identifier for this native group.
    pub id: String,
    /// Owning Extrude parameter-scope record.
    pub scope_record_index: u32,
    /// Zero-based position in the scope's ordered reference table.
    pub scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    pub record_index: u32,
    /// Byte offset of the primary indexed-record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: String,
    /// Byte offset of the counted member-run length.
    pub member_count_offset: u64,
    /// Ordered indexed selection-member records.
    pub members: Vec<Located<u32>>,
    /// Opaque nonzero u32 repeated around the f64 scalar.
    pub opaque_index: u32,
    /// Byte offset of the first `opaque_index` copy.
    pub opaque_index_offset: u64,
    /// Opaque finite f64 between the repeated u32 copies.
    pub opaque_scalar: f64,
    /// Byte offset of `opaque_scalar`.
    pub opaque_scalar_offset: u64,
    /// Boolean byte between the two nested-record references.
    pub variant: bool,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub paired_class_tag: String,
    /// Byte offset of the same-index paired header.
    pub paired_byte_offset: u64,
}

/// Counted selection group owned by an Extrude parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignExtrudeSelectionGroupWire {
    /// Globally unique deterministic identifier for this native group.
    id: String,
    /// Owning Extrude parameter-scope record.
    scope_record_index: u32,
    /// Zero-based position in the scope's ordered reference table.
    scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    record_index: u32,
    /// Byte offset of the primary indexed-record header.
    byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    class_tag: String,
    /// Byte offset of the counted member-run length.
    member_count_offset: u64,
    /// Ordered indexed selection-member records.
    members: Vec<u32>,
    /// Byte offsets parallel to `members`.
    member_offsets: Vec<u64>,
    /// Opaque nonzero u32 repeated around the f64 scalar.
    opaque_index: u32,
    /// Byte offset of the first `opaque_index` copy.
    opaque_index_offset: u64,
    /// Opaque finite f64 between the repeated u32 copies.
    opaque_scalar: f64,
    /// Byte offset of `opaque_scalar`.
    opaque_scalar_offset: u64,
    /// Boolean byte between the two nested-record references.
    variant: bool,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    paired_class_tag: String,
    /// Byte offset of the same-index paired header.
    paired_byte_offset: u64,
}

impl TryFrom<DesignExtrudeSelectionGroupWire> for DesignExtrudeSelectionGroup {
    type Error = String;
    fn try_from(wire: DesignExtrudeSelectionGroupWire) -> Result<Self, Self::Error> {
        if wire.members.len() != wire.member_offsets.len() {
            return Err("members and member_offsets must have equal lengths".into());
        }
        Ok(Self {
            members: wire.members.into_iter().zip(wire.member_offsets).map(|(value, offset)| Located { value, offset }).collect(),
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            scope_reference_ordinal: wire.scope_reference_ordinal,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag,
            member_count_offset: wire.member_count_offset,
            opaque_index: wire.opaque_index,
            opaque_index_offset: wire.opaque_index_offset,
            opaque_scalar: wire.opaque_scalar,
            opaque_scalar_offset: wire.opaque_scalar_offset,
            variant: wire.variant,
            paired_class_tag: wire.paired_class_tag,
            paired_byte_offset: wire.paired_byte_offset,
        })
    }
}

impl From<DesignExtrudeSelectionGroup> for DesignExtrudeSelectionGroupWire {
    fn from(group: DesignExtrudeSelectionGroup) -> Self {
        let (members, member_offsets) = group.members.into_iter().map(|member| (member.value, member.offset)).unzip();
        Self {
            members,
            member_offsets,
            id: group.id,
            scope_record_index: group.scope_record_index,
            scope_reference_ordinal: group.scope_reference_ordinal,
            record_index: group.record_index,
            byte_offset: group.byte_offset,
            class_tag: group.class_tag,
            member_count_offset: group.member_count_offset,
            opaque_index: group.opaque_index,
            opaque_index_offset: group.opaque_index_offset,
            opaque_scalar: group.opaque_scalar,
            opaque_scalar_offset: group.opaque_scalar_offset,
            variant: group.variant,
            paired_class_tag: group.paired_class_tag,
            paired_byte_offset: group.paired_byte_offset,
        }
    }
}

/// Semantic role of a counted Extrude operand group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignExtrudeOperandRole {
    /// Existing bodies consumed by the Boolean operation.
    Bodies,
    /// Sketch profile swept by the Extrude.
    Profile,
    /// Faces used by profile-start or termination construction.
    Faces(Option<DesignExtrudeFaceRole>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
enum DesignExtrudeOperandRoleTag {
    Bodies,
    Profile,
    Faces,
}

/// Semantic use of an ordered Extrude face-operand group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DesignExtrudeFaceRole {
    /// Face supporting a selected-face start.
    Start,
    /// Face terminating a one-sided to-face extent.
    Termination,
}

/// Construction-operand group owned by a feature scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(with = "DesignConstructionOperandGroupSerde")
)]
#[serde(
    try_from = "DesignConstructionOperandGroupSerde",
    into = "DesignConstructionOperandGroupSerde"
)]
pub struct DesignConstructionOperandGroup {
    /// Globally unique deterministic identifier.
    pub id: String,
    /// Owning feature scope record.
    pub scope_record_index: u32,
    /// Position in the scope reference table.
    pub scope_reference_ordinal: u32,
    /// Primary indexed-record identity.
    pub record_index: u32,
    /// Primary indexed-header byte offset.
    pub byte_offset: u64,
    /// Per-file dynamic primary class tag.
    pub class_tag: String,
    /// Ordered operand-record references.
    pub members: Vec<Located<u32>>,
    /// Ordered unresolved-edge records whose run terminates at this group's identity.
    pub lost_edge_references: Vec<String>,
    /// Exact framing of the operand-member run and its auxiliary fields.
    pub frame: DesignConstructionOperandGroupFrame,
    /// Source u64 role code.
    pub role: u64,
    /// Extrude-specific semantic role of `role`. Face start/termination lives
    /// on `Faces`.
    pub extrude_role: Option<DesignExtrudeOperandRole>,
    /// Byte offset of `role`.
    pub role_offset: u64,
    /// Per-file dynamic paired class tag.
    pub paired_class_tag: String,
    /// Same-index paired-header byte offset.
    pub paired_byte_offset: u64,
}

impl DesignConstructionOperandGroup {
    pub(crate) fn extrude_face_role(&self) -> Option<DesignExtrudeFaceRole> {
        match self.extrude_role {
            Some(DesignExtrudeOperandRole::Faces(role)) => role,
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignConstructionOperandGroupSerde {
    id: String,
    scope_record_index: u32,
    scope_reference_ordinal: u32,
    record_index: u32,
    byte_offset: u64,
    class_tag: String,
    members: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    lost_edge_references: Vec<String>,
    member_offsets: Vec<u64>,
    frame: DesignConstructionOperandGroupFrame,
    role: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    extrude_role: Option<DesignExtrudeOperandRoleTag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    extrude_face_role: Option<DesignExtrudeFaceRole>,
    role_offset: u64,
    paired_class_tag: String,
    paired_byte_offset: u64,
}

impl TryFrom<DesignConstructionOperandGroupSerde> for DesignConstructionOperandGroup {
    type Error = String;

    fn try_from(wire: DesignConstructionOperandGroupSerde) -> Result<Self, Self::Error> {
        if wire.members.len() != wire.member_offsets.len() {
 return Err("members and member_offsets must have equal lengths".into());
}
let extrude_role = match (wire.extrude_role, wire.extrude_face_role) {
            (Some(DesignExtrudeOperandRoleTag::Bodies), None) => {
                Some(DesignExtrudeOperandRole::Bodies)
            }
            (Some(DesignExtrudeOperandRoleTag::Profile), None) => {
                Some(DesignExtrudeOperandRole::Profile)
            }
            (Some(DesignExtrudeOperandRoleTag::Faces), face_role) => {
                Some(DesignExtrudeOperandRole::Faces(face_role))
            }
            (None, None) => None,
            _ => {
                return Err("extrude_face_role is only valid when extrude_role is faces".into());
            }
        };
        Ok(Self {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            scope_reference_ordinal: wire.scope_reference_ordinal,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag,
            members: wire.members.into_iter().zip(wire.member_offsets).map(|(value, offset)| Located { value, offset }).collect(),
            lost_edge_references: wire.lost_edge_references,
            frame: wire.frame,
            role: wire.role,
            extrude_role,
            role_offset: wire.role_offset,
            paired_class_tag: wire.paired_class_tag,
            paired_byte_offset: wire.paired_byte_offset,
        })
    }
}

impl From<DesignConstructionOperandGroup> for DesignConstructionOperandGroupSerde {
    fn from(group: DesignConstructionOperandGroup) -> Self {
        let (members, member_offsets) = group.members.into_iter().map(|member| (member.value, member.offset)).unzip();
let (extrude_role, extrude_face_role) = match group.extrude_role {
            Some(DesignExtrudeOperandRole::Bodies) => {
                (Some(DesignExtrudeOperandRoleTag::Bodies), None)
            }
            Some(DesignExtrudeOperandRole::Profile) => {
                (Some(DesignExtrudeOperandRoleTag::Profile), None)
            }
            Some(DesignExtrudeOperandRole::Faces(face_role)) => {
                (Some(DesignExtrudeOperandRoleTag::Faces), face_role)
            }
            None => (None, None),
        };
        Self {
            id: group.id,
            scope_record_index: group.scope_record_index,
            scope_reference_ordinal: group.scope_reference_ordinal,
            record_index: group.record_index,
            byte_offset: group.byte_offset,
            class_tag: group.class_tag,
            members,
            lost_edge_references: group.lost_edge_references,
            member_offsets,
            frame: group.frame,
            role: group.role,
            extrude_role,
            extrude_face_role,
            role_offset: group.role_offset,
            paired_class_tag: group.paired_class_tag,
            paired_byte_offset: group.paired_byte_offset,
        }
    }
}

/// Serialized framing of a construction-operand group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignConstructionOperandGroupFrameWire", into = "DesignConstructionOperandGroupFrameWire")]
pub struct DesignConstructionOperandGroupFrame {
    /// Byte offset of the member count.
    pub member_count_offset: u64,
    /// Auxiliary records named by the two optional references that follow the
    /// member run; an absent reference contributes no entry.
    pub auxiliary_records: Vec<Located<u32>>,
    /// Exact selection-path records selected by the optional references.
    pub auxiliary_paths: Vec<DesignConstructionOperandPath>,
    /// Indexed records named by the counted trailing-reference run. The target
    /// grammar is selected by the owning operand family: persistent-selection
    /// groups name identity wrappers and placed-selection groups name affine
    /// transforms.
    pub trailing_records: Vec<Located<u32>>,
    /// Exact affine-transform records selected from the trailing-reference
    /// run. Other trailing records remain represented by their indices and
    /// offsets and can select another typed grammar.
    pub trailing_transforms: Vec<DesignConstructionOperandTransform>,
    /// Exact dual-transform records selected from the trailing-reference run.
    pub trailing_dual_transforms: Vec<DesignConstructionOperandDualTransform>,
    /// Exact compact flag records selected from the trailing-reference run.
    pub trailing_flags: Vec<DesignConstructionOperandFlag>,
    /// Opaque ordinal: nonzero and below 256, repeated after `opaque_scalar` in
    /// every container generation but one.
    pub opaque_index: u32,
    /// Byte offset of the first `opaque_index` copy.
    pub opaque_index_offset: u64,
    /// Opaque nonnegative finite f64.
    pub opaque_scalar: f64,
    /// Byte offset of `opaque_scalar`.
    pub opaque_scalar_offset: u64,
    /// Boolean tail variant.
    pub variant: bool,
}

/// Serialized framing of a construction-operand group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignConstructionOperandGroupFrameWire {
    member_count_offset: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    auxiliary_record_indices: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    auxiliary_record_offsets: Vec<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    auxiliary_paths: Vec<DesignConstructionOperandPath>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    trailing_record_indices: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    trailing_record_offsets: Vec<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    trailing_transforms: Vec<DesignConstructionOperandTransform>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    trailing_dual_transforms: Vec<DesignConstructionOperandDualTransform>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    trailing_flags: Vec<DesignConstructionOperandFlag>,
    opaque_index: u32,
    opaque_index_offset: u64,
    opaque_scalar: f64,
    opaque_scalar_offset: u64,
    variant: bool,
}

impl TryFrom<DesignConstructionOperandGroupFrameWire> for DesignConstructionOperandGroupFrame {
    type Error = String;
    fn try_from(wire: DesignConstructionOperandGroupFrameWire) -> Result<Self, Self::Error> {
        if wire.auxiliary_record_indices.len() != wire.auxiliary_record_offsets.len() {
            return Err("auxiliary_record_offsets must match auxiliary_record_indices".into());
        }
        if wire.trailing_record_indices.len() != wire.trailing_record_offsets.len() {
            return Err("trailing_record_offsets must match trailing_record_indices".into());
        }
        Ok(Self {
            member_count_offset: wire.member_count_offset,
            auxiliary_records: wire.auxiliary_record_indices.into_iter().zip(wire.auxiliary_record_offsets).map(|(value, offset)| Located { value, offset }).collect(),
            auxiliary_paths: wire.auxiliary_paths,
            trailing_records: wire.trailing_record_indices.into_iter().zip(wire.trailing_record_offsets).map(|(value, offset)| Located { value, offset }).collect(),
            trailing_transforms: wire.trailing_transforms,
            trailing_dual_transforms: wire.trailing_dual_transforms,
            trailing_flags: wire.trailing_flags,
            opaque_index: wire.opaque_index,
            opaque_index_offset: wire.opaque_index_offset,
            opaque_scalar: wire.opaque_scalar,
            opaque_scalar_offset: wire.opaque_scalar_offset,
            variant: wire.variant,
        })
    }
}

impl From<DesignConstructionOperandGroupFrame> for DesignConstructionOperandGroupFrameWire {
    fn from(frame: DesignConstructionOperandGroupFrame) -> Self {
        Self {
            member_count_offset: frame.member_count_offset,
            auxiliary_record_indices: frame.auxiliary_records.iter().map(|record| record.value).collect(),
            auxiliary_record_offsets: frame.auxiliary_records.iter().map(|record| record.offset).collect(),
            auxiliary_paths: frame.auxiliary_paths,
            trailing_record_indices: frame.trailing_records.iter().map(|record| record.value).collect(),
            trailing_record_offsets: frame.trailing_records.iter().map(|record| record.offset).collect(),
            trailing_transforms: frame.trailing_transforms,
            trailing_dual_transforms: frame.trailing_dual_transforms,
            trailing_flags: frame.trailing_flags,
            opaque_index: frame.opaque_index,
            opaque_index_offset: frame.opaque_index_offset,
            opaque_scalar: frame.opaque_scalar,
            opaque_scalar_offset: frame.opaque_scalar_offset,
            variant: frame.variant,
        }
    }
}

/// Compact boolean record named by a construction-operand group's trailing run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignConstructionOperandFlag {
    /// Indexed flag-record identity.
    pub record_index: u32,
    /// Flag-record header byte offset.
    pub byte_offset: u64,
    /// Per-file dynamic flag-record class tag.
    pub class_tag: String,
    /// Stored boolean value.
    pub value: bool,
    /// Byte offset of the stored boolean.
    pub value_offset: u64,
}

/// Affine placement named by a construction-operand group's trailing run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignConstructionOperandTransform {
    /// Indexed transform-record identity.
    pub record_index: u32,
    /// Transform-record header byte offset.
    pub byte_offset: u64,
    /// Per-file dynamic transform-record class tag.
    pub class_tag: String,
    /// Row-major local-to-model affine transform.
    pub transform: [[f64; 4]; 4],
    /// Byte offset of the first matrix scalar.
    pub transform_offset: u64,
    /// Indexed record immediately following the transform.
    pub following_record_index: u32,
    /// Following-record header byte offset.
    pub following_byte_offset: u64,
    /// Per-file dynamic following-record class tag.
    pub following_class_tag: String,
}

/// Two ordered affine placements named by an operand group's trailing run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignConstructionOperandDualTransform {
    /// Indexed transform-record identity.
    pub record_index: u32,
    /// Transform-record header byte offset.
    pub byte_offset: u64,
    /// Per-file dynamic transform-record class tag.
    pub class_tag: String,
    /// First row-major affine transform.
    pub first_transform: [[f64; 4]; 4],
    /// Byte offset of the first matrix scalar.
    pub first_transform_offset: u64,
    /// Second row-major affine transform.
    pub second_transform: [[f64; 4]; 4],
    /// Byte offset of the second matrix scalar.
    pub second_transform_offset: u64,
}

/// One persistent-entity step in a construction operand's selection path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignConstructionOperandPathWire", into = "DesignConstructionOperandPathWire")]
pub struct DesignConstructionOperandPath {
    /// Indexed path-record identity.
    pub record_index: u32,
    /// Path-record header byte offset.
    pub byte_offset: u64,
    /// Per-file dynamic path-record class tag.
    pub class_tag: String,
    /// Persistent entity identity carried by this path step.
    pub entity_ref: u64,
    /// Byte offset of `entity_ref`.
    pub entity_ref_offset: u64,
    /// Transform or compact selection-path layout.
    pub placement: DesignConstructionPathPlacement,
    /// Owning feature-scope record.
    pub scope_record_index: u32,
    /// Byte offset of the owning-scope reference.
    pub scope_record_index_offset: u64,
    /// Nested record selected after the owning scope.
    pub nested_record_index: u32,
    /// Byte offset of the nested-record reference.
    pub nested_record_index_offset: u64,
    /// Indexed record immediately following this path frame.
    pub following_record_index: u32,
    /// Following-record header byte offset.
    pub following_byte_offset: u64,
    /// Per-file dynamic following-record class tag.
    pub following_class_tag: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignConstructionOperandPathWire {
    /// Indexed path-record identity.
    record_index: u32,
    /// Path-record header byte offset.
    byte_offset: u64,
    /// Per-file dynamic path-record class tag.
    class_tag: String,
    /// Persistent entity identity carried by this path step.
    entity_ref: u64,
    /// Byte offset of `entity_ref`.
    entity_ref_offset: u64,
    /// Optional row-major selection-path placement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transform: Option<[[f64; 4]; 4]>,
    /// Byte offset of the first transform scalar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transform_offset: Option<u64>,
    /// Compact-frame boolean; absent from the transform frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    compact_variant: Option<bool>,
    /// Owning feature-scope record.
    scope_record_index: u32,
    /// Byte offset of the owning-scope reference.
    scope_record_index_offset: u64,
    /// Nested record selected after the owning scope.
    nested_record_index: u32,
    /// Byte offset of the nested-record reference.
    nested_record_index_offset: u64,
    /// Indexed record immediately following this path frame.
    following_record_index: u32,
    /// Following-record header byte offset.
    following_byte_offset: u64,
    /// Per-file dynamic following-record class tag.
    following_class_tag: String,
}

impl TryFrom<DesignConstructionOperandPathWire> for DesignConstructionOperandPath {
    type Error = String;
    fn try_from(wire: DesignConstructionOperandPathWire) -> Result<Self, Self::Error> {
        Ok(Self {
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag,
            entity_ref: wire.entity_ref,
            entity_ref_offset: wire.entity_ref_offset,
            placement: match (wire.transform, wire.transform_offset, wire.compact_variant) {
                (Some(value), Some(offset), None) => DesignConstructionPathPlacement::Transform(Located { value, offset }),
                (None, None, Some(variant)) => DesignConstructionPathPlacement::Compact(variant),
                _ => return Err("transform and transform_offset must occur together and exclude compact_variant; compact_variant is required without transform".into()),
            },
            scope_record_index: wire.scope_record_index,
            scope_record_index_offset: wire.scope_record_index_offset,
            nested_record_index: wire.nested_record_index,
            nested_record_index_offset: wire.nested_record_index_offset,
            following_record_index: wire.following_record_index,
            following_byte_offset: wire.following_byte_offset,
            following_class_tag: wire.following_class_tag,
        })
    }
}

impl From<DesignConstructionOperandPath> for DesignConstructionOperandPathWire {
    fn from(record: DesignConstructionOperandPath) -> Self {
        let (transform, transform_offset, compact_variant) = match record.placement {
            DesignConstructionPathPlacement::Transform(transform) => (Some(transform.value), Some(transform.offset), None),
            DesignConstructionPathPlacement::Compact(variant) => (None, None, Some(variant)),
        };
        Self {
            record_index: record.record_index,
            byte_offset: record.byte_offset,
            class_tag: record.class_tag,
            entity_ref: record.entity_ref,
            entity_ref_offset: record.entity_ref_offset,
            transform,
            transform_offset,
            compact_variant,
            scope_record_index: record.scope_record_index,
            scope_record_index_offset: record.scope_record_index_offset,
            nested_record_index: record.nested_record_index,
            nested_record_index_offset: record.nested_record_index_offset,
            following_record_index: record.following_record_index,
            following_byte_offset: record.following_byte_offset,
            following_class_tag: record.following_class_tag,
        }
    }
}

/// Placement layout carried by a persistent-entity selection path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum DesignConstructionPathPlacement {
    Transform(Located<[[f64; 4]; 4]>),
    Compact(bool),
}


/// Nested identity chain named by a construction-operand group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignConstructionOperandIdentityWire", into = "DesignConstructionOperandIdentityWire")]
pub struct DesignConstructionOperandIdentity {
    /// Globally unique deterministic identifier.
    pub id: String,
    /// Owning operand-group record.
    pub group_record_index: u32,
    /// Ordered identity-wrapper indexed records.
    pub wrappers: Vec<DesignIdentityWrapper>,
    /// Indexed identity of the record physically following the wrappers.
    pub following_record_index: u32,
    /// Indexed-header byte offset of the record following the wrappers.
    pub following_byte_offset: u64,
    /// Per-file dynamic class tag of the record following the wrappers.
    pub following_class_tag: String,
    /// Entity-tracking path between the outer wrappers and persistent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracking_path: Option<DesignConstructionTrackingPath>,
    /// Fixed-width persistent identity, when the following record has that grammar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistent_identity: Option<DesignConstructionPersistentIdentity>,
}

/// Identity and location of one indexed construction wrapper.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignIdentityWrapper {
    pub record_index: u32,
    pub byte_offset: u64,
    pub class_tag: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignConstructionOperandIdentityWire {
    /// Globally unique deterministic identifier.
    pub id: String,
    /// Owning operand-group record.
    pub group_record_index: u32,
    /// Ordered identity-wrapper indexed-record identities.
    pub wrapper_record_indices: Vec<u32>,
    /// Indexed-header byte offsets parallel to `wrapper_record_indices`.
    pub wrapper_byte_offsets: Vec<u64>,
    /// Per-file dynamic class tags parallel to `wrapper_record_indices`.
    pub wrapper_class_tags: Vec<String>,
    /// Indexed identity of the record physically following the wrappers.
    pub following_record_index: u32,
    /// Indexed-header byte offset of the record following the wrappers.
    pub following_byte_offset: u64,
    /// Per-file dynamic class tag of the record following the wrappers.
    pub following_class_tag: String,
    /// Entity-tracking path between the outer wrappers and persistent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracking_path: Option<DesignConstructionTrackingPath>,
    /// Fixed-width persistent identity, when the following record has that grammar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistent_identity: Option<DesignConstructionPersistentIdentity>,
}

impl TryFrom<DesignConstructionOperandIdentityWire> for DesignConstructionOperandIdentity {
    type Error = String;
    fn try_from(wire: DesignConstructionOperandIdentityWire) -> Result<Self, Self::Error> {
        if wire.wrapper_record_indices.len() != wire.wrapper_byte_offsets.len()
            || wire.wrapper_record_indices.len() != wire.wrapper_class_tags.len()
        {
            return Err("wrapper_record_indices, wrapper_byte_offsets, and wrapper_class_tags must have equal lengths".into());
        }
        Ok(Self {
            id: wire.id,
            group_record_index: wire.group_record_index,
            following_record_index: wire.following_record_index,
            following_byte_offset: wire.following_byte_offset,
            following_class_tag: wire.following_class_tag,
            tracking_path: wire.tracking_path,
            persistent_identity: wire.persistent_identity,
            wrappers: wire.wrapper_record_indices.into_iter().zip(wire.wrapper_byte_offsets).zip(wire.wrapper_class_tags)
                .map(|((record_index, byte_offset), class_tag)| DesignIdentityWrapper { record_index, byte_offset, class_tag }).collect(),
        })
    }
}

impl From<DesignConstructionOperandIdentity> for DesignConstructionOperandIdentityWire {
    fn from(identity: DesignConstructionOperandIdentity) -> Self {
        let mut wrapper_record_indices = Vec::with_capacity(identity.wrappers.len());
        let mut wrapper_byte_offsets = Vec::with_capacity(identity.wrappers.len());
        let mut wrapper_class_tags = Vec::with_capacity(identity.wrappers.len());
        for wrapper in identity.wrappers {
            wrapper_record_indices.push(wrapper.record_index);
            wrapper_byte_offsets.push(wrapper.byte_offset);
            wrapper_class_tags.push(wrapper.class_tag);
        }
        Self {
            id: identity.id,
            group_record_index: identity.group_record_index,
            following_record_index: identity.following_record_index,
            following_byte_offset: identity.following_byte_offset,
            following_class_tag: identity.following_class_tag,
            tracking_path: identity.tracking_path,
            persistent_identity: identity.persistent_identity,
            wrapper_record_indices,
            wrapper_byte_offsets,
            wrapper_class_tags,
        }
    }
}

/// Entity-tracking path embedded in a construction-operand identity chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "DesignConstructionTrackingPathWire"))]
#[serde(try_from = "DesignConstructionTrackingPathWire", into = "DesignConstructionTrackingPathWire")]
pub struct DesignConstructionTrackingPath {
    /// Outer tracking-wrapper record identity.
    pub wrapper_record_index: u32,
    /// Outer tracking-wrapper header byte offset.
    pub wrapper_byte_offset: u64,
    /// Outer tracking-wrapper dynamic class tag.
    pub wrapper_class_tag: String,
    /// Nested tracking-carrier record identity.
    pub carrier_record_index: u32,
    /// Nested tracking-carrier header byte offset.
    pub carrier_byte_offset: u64,
    /// Nested tracking-carrier dynamic class tag.
    pub carrier_class_tag: String,
    /// Primary persistent identity stored by the carrier.
    pub primary_identity: u64,
    /// Byte offset of `primary_identity`.
    pub primary_identity_offset: u64,
    /// Signed carrier selector.
    pub selector: i32,
    /// Byte offset of `selector`.
    pub selector_offset: u64,
    /// Carrier-kind discriminator.
    pub kind: u32,
    /// Byte offset of `kind`.
    pub kind_offset: u64,
    /// First optional related persistent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_related_identity: Option<Located<u64>>,
    /// Second optional related persistent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_related_identity: Option<Located<u64>>,
    /// Indexed record immediately following the carrier.
    pub following_record_index: u32,
    /// Following-record header byte offset.
    pub following_byte_offset: u64,
    /// Following-record dynamic class tag.
    pub following_class_tag: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignConstructionTrackingPathWire {
    wrapper_record_index: u32,
    wrapper_byte_offset: u64,
    wrapper_class_tag: String,
    carrier_record_index: u32,
    carrier_byte_offset: u64,
    carrier_class_tag: String,
    primary_identity: u64,
    primary_identity_offset: u64,
    selector: i32,
    selector_offset: u64,
    kind: u32,
    kind_offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    first_related_identity: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    first_related_identity_offset: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    second_related_identity: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    second_related_identity_offset: Option<u64>,
    following_record_index: u32,
    following_byte_offset: u64,
    following_class_tag: String,
}

impl TryFrom<DesignConstructionTrackingPathWire> for DesignConstructionTrackingPath {
    type Error = String;
    fn try_from(wire: DesignConstructionTrackingPathWire) -> Result<Self, Self::Error> {
        Ok(Self {
            wrapper_record_index: wire.wrapper_record_index,
            wrapper_byte_offset: wire.wrapper_byte_offset,
            wrapper_class_tag: wire.wrapper_class_tag,
            carrier_record_index: wire.carrier_record_index,
            carrier_byte_offset: wire.carrier_byte_offset,
            carrier_class_tag: wire.carrier_class_tag,
            primary_identity: wire.primary_identity,
            primary_identity_offset: wire.primary_identity_offset,
            selector: wire.selector,
            selector_offset: wire.selector_offset,
            kind: wire.kind,
            kind_offset: wire.kind_offset,
            first_related_identity: Located::from_wire(wire.first_related_identity, wire.first_related_identity_offset, "first_related_identity")?,
            second_related_identity: Located::from_wire(wire.second_related_identity, wire.second_related_identity_offset, "second_related_identity")?,
            following_record_index: wire.following_record_index,
            following_byte_offset: wire.following_byte_offset,
            following_class_tag: wire.following_class_tag,
        })
    }
}

impl From<DesignConstructionTrackingPath> for DesignConstructionTrackingPathWire {
    fn from(value: DesignConstructionTrackingPath) -> Self {
        Self {
            wrapper_record_index: value.wrapper_record_index,
            wrapper_byte_offset: value.wrapper_byte_offset,
            wrapper_class_tag: value.wrapper_class_tag,
            carrier_record_index: value.carrier_record_index,
            carrier_byte_offset: value.carrier_byte_offset,
            carrier_class_tag: value.carrier_class_tag,
            primary_identity: value.primary_identity,
            primary_identity_offset: value.primary_identity_offset,
            selector: value.selector,
            selector_offset: value.selector_offset,
            kind: value.kind,
            kind_offset: value.kind_offset,
            first_related_identity: value.first_related_identity.map(|located| located.value),
            first_related_identity_offset: value.first_related_identity.map(|located| located.offset),
            second_related_identity: value.second_related_identity.map(|located| located.value),
            second_related_identity_offset: value.second_related_identity.map(|located| located.offset),
            following_record_index: value.following_record_index,
            following_byte_offset: value.following_byte_offset,
            following_class_tag: value.following_class_tag,
        }
    }
}


/// Fixed-width persistent identity following a construction-operand identity chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignConstructionPersistentIdentity {
    /// Local persistent identity preceding the two UUID fields.
    pub local_id: u64,
    /// Byte offset of `local_id`.
    pub local_id_offset: u64,
    /// Asset UUID qualifying the local identity.
    pub asset_id: String,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// UUID of the local identity context.
    pub context_id: String,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub context_id_offset: u64,
    /// Whether the fixed tail's optional slot is present.
    #[serde(default)]
    pub tail_slot_present: bool,
    /// Byte offset of the optional-slot marker.
    #[serde(default)]
    pub tail_slot_offset: u64,
    /// Identity of the indexed record immediately following this identity.
    pub next_record_index: u32,
    /// Byte offset of the indexed record immediately following this identity.
    pub next_byte_offset: u64,
}

/// One radius assignment and its ordered edge group in a Fillet scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignFilletRadiusGroup {
    /// Globally unique deterministic identifier.
    pub id: String,
    /// Owning Fillet scope record.
    pub scope_record_index: u32,
    /// Position among construction-operand groups in scope-reference order.
    pub group_ordinal: u32,
    /// Counted construction-operand group carrying the edges.
    pub group_record_index: u32,
    /// Ordered edge-operand records assigned this radius.
    pub edge_operand_record_indices: Vec<u32>,
    /// Radius law paired with this edge group.
    pub law: DesignFilletRadiusLaw,
    /// Tangency-weight parameter record paired with this edge group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tangency_weight_parameter_record_index: Option<u32>,
}

/// Parameter records defining one Fillet group's radius law.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesignFilletRadiusLaw {
    /// One radius applies along the complete edge group.
    Constant {
        /// Radius parameter record.
        radius_parameter_record_index: u32,
    },
    /// Constant transverse chord length across the fillet surface.
    Chordal {
        /// Chord-length parameter record.
        chord_length_parameter_record_index: u32,
    },
    /// Distinct support-face offsets along the complete edge group.
    Asymmetric {
        /// First support-face offset parameter record.
        offset_one_parameter_record_index: u32,
        /// Second support-face offset parameter record.
        offset_two_parameter_record_index: u32,
    },
    /// Explicit endpoint and optional midpoint radius controls.
    Variable {
        /// Radius at normalized parameter zero.
        start_radius_parameter_record_index: u32,
        /// Radius at normalized parameter one.
        end_radius_parameter_record_index: u32,
        /// Midpoint radius records in owner-local order.
        middle_radius_parameter_record_indices: Vec<u32>,
        /// Midpoint normalized-parameter records parallel to the radii.
        middle_parameter_record_indices: Vec<u32>,
    },
}

/// ASM history family, entity slot, and states for one selected identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct HistoricalBinding {
    /// Stable ASM family containing the selected identity.
    #[serde(rename = "historical_entity_kind")]
    pub kind: AsmHistoricalEntityKind,
    /// Stable ASM entity slot after record-revision normalization.
    #[serde(rename = "historical_entity_ref")]
    pub entity_ref: i64,
    /// ASM history states containing the identity, in history arena order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[serde(rename = "historical_state_ids")]
    pub state_ids: Vec<i64>,
}

/// One fixed-width member named by an Extrude selection group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignExtrudeSelectionMember {
    /// Globally unique deterministic identifier for this native member.
    pub id: String,
    /// Owning selection-group record.
    pub group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    pub group_member_ordinal: u32,
    /// Indexed-record identity named by the selection group.
    pub record_index: u32,
    /// Byte offset of the indexed-record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: String,
    /// Local persistent selection identity preceding the two UUID fields.
    pub local_id: u64,
    /// Byte offset of `local_id`.
    pub local_id_offset: u64,
    /// Asset UUID qualifying the local selection identity.
    pub asset_id: String,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// UUID of the local selection-identity context.
    pub context_id: String,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub context_id_offset: u64,
    /// Whether the fixed tail's optional slot is present.
    #[serde(default)]
    pub tail_slot_present: bool,
    /// Byte offset of the optional-slot marker.
    #[serde(default)]
    pub tail_slot_offset: u64,
    /// Sketch geometry carrying `local_id`, when it resolves uniquely in
    /// the selected Sketch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_geometry: Option<SketchRelationOperand>,
    /// Construction-operand identity chains that terminate at this member.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operand_identity_ids: Vec<String>,
    /// Stable ASM history family, entity slot, and states carrying `local_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(flatten)]
    pub historical: Option<HistoricalBinding>,
    /// Identity of the indexed record immediately following this member.
    pub next_record_index: u32,
    /// Byte offset of the indexed record immediately following this member.
    pub next_byte_offset: u64,
}

impl DesignExtrudeSelectionMember {
    pub(crate) fn historical_entity_kind(&self) -> Option<AsmHistoricalEntityKind> {
        self.historical.as_ref().map(|binding| binding.kind)
    }

    pub(crate) fn historical_entity_ref(&self) -> Option<i64> {
        self.historical.as_ref().map(|binding| binding.entity_ref)
    }

    pub(crate) fn historical_state_ids(&self) -> &[i64] {
        self.historical
            .as_ref()
            .map(|binding| binding.state_ids.as_slice())
            .unwrap_or(&[])
    }
}

/// Persistent Design entity selected through a nested indexed-record frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignEntitySelectionOperandWire", into = "DesignEntitySelectionOperandWire")]
pub struct DesignEntitySelectionOperand {
    /// Globally unique deterministic identifier for this native operand.
    pub id: String,
    /// Owning feature scope record.
    pub scope_record_index: u32,
    /// Owning construction-operand group record.
    pub group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    pub group_member_ordinal: u32,
    /// Primary indexed-record identity.
    pub record_index: u32,
    /// Primary indexed-header byte offset.
    pub byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    pub class_tag: String,
    /// Asset UUID qualifying the selection namespace.
    pub asset_id: String,
    /// Byte offset of the asset identifier's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// UUID of the selection context.
    pub context_id: String,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub context_id_offset: u64,
    /// Nested indexed record that carries the persistent entity identity.
    pub identity_record_index: u32,
    /// Byte offset of the nested identity record.
    pub identity_record_offset: u64,
    /// Primary entity identity in the nested identity pair; for a Sketch
    /// curve selection, this is the owning Sketch entity suffix.
    pub primary_identity: u64,
    /// Byte offset of `primary_identity`.
    pub primary_identity_offset: u64,
    /// Secondary identity in the nested pair; for a Sketch curve selection,
    /// this is the curve's primary persistent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_identity: Option<Located<u64>>,
    /// Optional secondary identity of the selected Sketch curve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve_secondary_identity: Option<Located<u64>>,
    /// Input-state edge proofs derived from the two serialized identities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub historical_edge_candidates: Vec<DesignEntitySelectionEdgeCandidate>,
    /// History-qualified face proofs derived from the primary identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub historical_face_candidates: Vec<DesignEntitySelectionFaceCandidate>,
    /// Unique input-state edge selected by every available identity proof.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_edge_slot: Option<i64>,
    /// Identity of the indexed record immediately following the identity record.
    pub next_record_index: u32,
    /// Byte offset of the indexed record immediately following the identity record.
    pub next_byte_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignEntitySelectionOperandWire {
    /// Globally unique deterministic identifier for this native operand.
    id: String,
    /// Owning feature scope record.
    scope_record_index: u32,
    /// Owning construction-operand group record.
    group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    group_member_ordinal: u32,
    /// Primary indexed-record identity.
    record_index: u32,
    /// Primary indexed-header byte offset.
    byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    class_tag: String,
    /// Asset UUID qualifying the selection namespace.
    asset_id: String,
    /// Byte offset of the asset identifier's UTF-16LE code units.
    asset_id_offset: u64,
    /// UUID of the selection context.
    context_id: String,
    /// Byte offset of the context UUID's UTF-16LE code units.
    context_id_offset: u64,
    /// Nested indexed record that carries the persistent entity identity.
    identity_record_index: u32,
    /// Byte offset of the nested identity record.
    identity_record_offset: u64,
    /// Primary entity identity in the nested identity pair; for a Sketch
    /// curve selection, this is the owning Sketch entity suffix.
    primary_identity: u64,
    /// Byte offset of `primary_identity`.
    primary_identity_offset: u64,
    /// Secondary identity in the nested pair; for a Sketch curve selection,
    /// this is the curve's primary persistent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secondary_identity: Option<u64>,
    /// Byte offset of `secondary_identity`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secondary_identity_offset: Option<u64>,
    /// Optional secondary identity of the selected Sketch curve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curve_secondary_identity: Option<u64>,
    /// Byte offset of `curve_secondary_identity`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curve_secondary_identity_offset: Option<u64>,
    /// Input-state edge proofs derived from the two serialized identities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    historical_edge_candidates: Vec<DesignEntitySelectionEdgeCandidate>,
    /// History-qualified face proofs derived from the primary identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    historical_face_candidates: Vec<DesignEntitySelectionFaceCandidate>,
    /// Unique input-state edge selected by every available identity proof.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolved_edge_slot: Option<i64>,
    /// Identity of the indexed record immediately following the identity record.
    next_record_index: u32,
    /// Byte offset of the indexed record immediately following the identity record.
    next_byte_offset: u64,
}

impl TryFrom<DesignEntitySelectionOperandWire> for DesignEntitySelectionOperand {
    type Error = String;
    fn try_from(wire: DesignEntitySelectionOperandWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            group_record_index: wire.group_record_index,
            group_member_ordinal: wire.group_member_ordinal,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag,
            asset_id: wire.asset_id,
            asset_id_offset: wire.asset_id_offset,
            context_id: wire.context_id,
            context_id_offset: wire.context_id_offset,
            identity_record_index: wire.identity_record_index,
            identity_record_offset: wire.identity_record_offset,
            primary_identity: wire.primary_identity,
            primary_identity_offset: wire.primary_identity_offset,
            secondary_identity: Located::from_wire(wire.secondary_identity, wire.secondary_identity_offset, "secondary_identity")?,
            curve_secondary_identity: Located::from_wire(wire.curve_secondary_identity, wire.curve_secondary_identity_offset, "curve_secondary_identity")?,
            historical_edge_candidates: wire.historical_edge_candidates,
            historical_face_candidates: wire.historical_face_candidates,
            resolved_edge_slot: wire.resolved_edge_slot,
            next_record_index: wire.next_record_index,
            next_byte_offset: wire.next_byte_offset,
        })
    }
}

impl From<DesignEntitySelectionOperand> for DesignEntitySelectionOperandWire {
    fn from(record: DesignEntitySelectionOperand) -> Self {
        Self {
            id: record.id,
            scope_record_index: record.scope_record_index,
            group_record_index: record.group_record_index,
            group_member_ordinal: record.group_member_ordinal,
            record_index: record.record_index,
            byte_offset: record.byte_offset,
            class_tag: record.class_tag,
            asset_id: record.asset_id,
            asset_id_offset: record.asset_id_offset,
            context_id: record.context_id,
            context_id_offset: record.context_id_offset,
            identity_record_index: record.identity_record_index,
            identity_record_offset: record.identity_record_offset,
            primary_identity: record.primary_identity,
            primary_identity_offset: record.primary_identity_offset,
            secondary_identity: record.secondary_identity.map(|identity| identity.value),
            secondary_identity_offset: record.secondary_identity.map(|identity| identity.offset),
            curve_secondary_identity: record.curve_secondary_identity.map(|identity| identity.value),
            curve_secondary_identity_offset: record.curve_secondary_identity.map(|identity| identity.offset),
            historical_edge_candidates: record.historical_edge_candidates,
            historical_face_candidates: record.historical_face_candidates,
            resolved_edge_slot: record.resolved_edge_slot,
            next_record_index: record.next_record_index,
            next_byte_offset: record.next_byte_offset,
        }
    }
}

/// Face proof for one persistent identity in one ASM history namespace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignEntitySelectionFaceCandidate {
    /// Native ASM history containing the selected identity.
    pub history_id: String,
    /// Stable ASM family, entity slot, and states containing the selected identity.
    #[serde(flatten)]
    pub historical: HistoricalBinding,
    /// Face incident to the identity in every listed state.
    pub face_slot: i64,
}

/// Legacy Boolean-Loft body carrier paired with a role-`0x8` body group.
///
/// The carrier is a scope-owned, role-less frame. It is retained separately
/// from the ordinary construction-operand group because its member and
/// scalar lanes do not use the counted-group grammar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(with = "DesignLoftLegacyBodyCarrierSerde")
)]
#[serde(
    try_from = "DesignLoftLegacyBodyCarrierSerde",
    into = "DesignLoftLegacyBodyCarrierSerde"
)]
pub struct DesignLoftLegacyBodyCarrier {
    /// Globally unique deterministic identifier.
    pub id: String,
    /// Owning Loft feature scope record.
    pub scope_record_index: u32,
    /// Primary indexed-record identity.
    pub record_index: u32,
    /// Primary indexed-header byte offset.
    pub byte_offset: u64,
    /// Per-file dynamic primary class tag (`322` or `411`).
    pub class_tag: String,
    /// Raw scope reference stored at the fixed owner lane.
    pub owner_scope_record_index: u32,
    /// Byte offset of `owner_scope_record_index`.
    pub owner_scope_record_index_offset: u64,
    /// The one member reference carried by this fixed legacy frame.
    pub member: u32,
    /// Byte offset of `member`.
    pub member_offset: u64,
    /// Byte offset of the on-wire member count (always 1).
    pub member_count_offset: u64,
    /// Opaque nonzero ordinal in the legacy scalar lane.
    pub opaque_index: u32,
    /// Byte offset of the first `opaque_index` copy.
    pub opaque_index_offset: u64,
    /// Opaque finite scalar in the legacy scalar lane.
    pub opaque_scalar: f64,
    /// Byte offset of `opaque_scalar`.
    pub opaque_scalar_offset: u64,
    /// Repeated scalar-lane ordinal.
    pub repeated_opaque_index: u32,
    /// Byte offset of the repeated `opaque_index` copy.
    pub repeated_opaque_index_offset: u64,
    /// Record named by the marked `N+2` reference.
    pub next_next_record_index: u32,
    /// Byte offset of the marked `N+2` reference.
    pub next_next_reference_offset: u64,
    /// Two bytes between the `N+2` and `N+1` references.
    pub flags: [u8; 2],
    /// Byte offset of `flags`.
    pub flags_offset: u64,
    /// Record named by the marked `N+1` reference.
    pub next_record_index: u32,
    /// Byte offset of the marked `N+1` reference.
    pub next_reference_offset: u64,
    /// Additional owning-scope reference and its source location, when present.
    pub trailing_scope_record_index: Option<Located<u32>>,
    /// Per-file dynamic paired class tag (`262` or `266`).
    pub paired_class_tag: String,
    /// Same-index paired-header byte offset.
    pub paired_byte_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignLoftLegacyBodyCarrierSerde {
    id: String,
    scope_record_index: u32,
    scope_reference_ordinal: u32,
    record_index: u32,
    byte_offset: u64,
    class_tag: String,
    owner_scope_record_index: u32,
    owner_scope_record_index_offset: u64,
    members: Vec<u32>,
    member_offsets: Vec<u64>,
    member_count: u32,
    member_count_offset: u64,
    opaque_index: u32,
    opaque_index_offset: u64,
    opaque_scalar: f64,
    opaque_scalar_offset: u64,
    repeated_opaque_index: u32,
    repeated_opaque_index_offset: u64,
    next_next_record_index: u32,
    next_next_reference_offset: u64,
    flags: [u8; 2],
    flags_offset: u64,
    next_record_index: u32,
    next_reference_offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trailing_scope_record_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trailing_scope_reference_offset: Option<u64>,
    paired_class_tag: String,
    paired_byte_offset: u64,
}

impl TryFrom<DesignLoftLegacyBodyCarrierSerde> for DesignLoftLegacyBodyCarrier {
    type Error = String;

    fn try_from(wire: DesignLoftLegacyBodyCarrierSerde) -> Result<Self, Self::Error> {
        if wire.scope_reference_ordinal != 0
            || wire.member_count != 1
            || wire.members.len() != 1
            || wire.member_offsets.len() != 1
        {
            return Err(
                "legacy loft body carrier must have one member at scope-reference ordinal zero"
                    .into(),
            );
        }
        Ok(Self {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag,
            owner_scope_record_index: wire.owner_scope_record_index,
            owner_scope_record_index_offset: wire.owner_scope_record_index_offset,
            member: wire.members[0],
            member_offset: wire.member_offsets[0],
            member_count_offset: wire.member_count_offset,
            opaque_index: wire.opaque_index,
            opaque_index_offset: wire.opaque_index_offset,
            opaque_scalar: wire.opaque_scalar,
            opaque_scalar_offset: wire.opaque_scalar_offset,
            repeated_opaque_index: wire.repeated_opaque_index,
            repeated_opaque_index_offset: wire.repeated_opaque_index_offset,
            next_next_record_index: wire.next_next_record_index,
            next_next_reference_offset: wire.next_next_reference_offset,
            flags: wire.flags,
            flags_offset: wire.flags_offset,
            next_record_index: wire.next_record_index,
            next_reference_offset: wire.next_reference_offset,
            trailing_scope_record_index: Located::from_wire(wire.trailing_scope_record_index, wire.trailing_scope_reference_offset, "trailing_scope_record_index")
                .map_err(|_| "trailing_scope_record_index and trailing_scope_reference_offset must occur together")?,
            paired_class_tag: wire.paired_class_tag,
            paired_byte_offset: wire.paired_byte_offset,
        })
    }
}

impl From<DesignLoftLegacyBodyCarrier> for DesignLoftLegacyBodyCarrierSerde {
    fn from(carrier: DesignLoftLegacyBodyCarrier) -> Self {
        Self {
            id: carrier.id,
            scope_record_index: carrier.scope_record_index,
            scope_reference_ordinal: 0,
            record_index: carrier.record_index,
            byte_offset: carrier.byte_offset,
            class_tag: carrier.class_tag,
            owner_scope_record_index: carrier.owner_scope_record_index,
            owner_scope_record_index_offset: carrier.owner_scope_record_index_offset,
            members: vec![carrier.member],
            member_offsets: vec![carrier.member_offset],
            member_count: 1,
            member_count_offset: carrier.member_count_offset,
            opaque_index: carrier.opaque_index,
            opaque_index_offset: carrier.opaque_index_offset,
            opaque_scalar: carrier.opaque_scalar,
            opaque_scalar_offset: carrier.opaque_scalar_offset,
            repeated_opaque_index: carrier.repeated_opaque_index,
            repeated_opaque_index_offset: carrier.repeated_opaque_index_offset,
            next_next_record_index: carrier.next_next_record_index,
            next_next_reference_offset: carrier.next_next_reference_offset,
            flags: carrier.flags,
            flags_offset: carrier.flags_offset,
            next_record_index: carrier.next_record_index,
            next_reference_offset: carrier.next_reference_offset,
            trailing_scope_record_index: carrier.trailing_scope_record_index.map(|reference| reference.value),
            trailing_scope_reference_offset: carrier.trailing_scope_record_index.map(|reference| reference.offset),
            paired_class_tag: carrier.paired_class_tag,
            paired_byte_offset: carrier.paired_byte_offset,
        }
    }
}

/// Historical edge proof carried by one nested entity-selection identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignEntitySelectionEdgeCandidate {
    /// Zero for the first identity and one for the second identity.
    pub identity_ordinal: u32,
    /// Serialized persistent identity.
    pub local_id: u64,
    /// Stable ASM history family containing the identity.
    pub historical_entity_kind: AsmHistoricalEntityKind,
    /// Stable ASM entity slot after record-revision normalization.
    pub historical_entity_ref: i64,
    /// Edges incident to the stable entity in the feature-input topology.
    pub edge_slots: Vec<i64>,
}

/// Whole-body construction operand carrying a persistent body-recipe reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignBodyRecipeOperandWire", into = "DesignBodyRecipeOperandWire")]
pub struct DesignBodyRecipeOperand {
    /// Globally unique deterministic identifier for this native operand.
    pub id: String,
    /// Owning feature scope record.
    pub scope_record_index: u32,
    /// Exact feature-scope ownership form.
    #[serde(flatten)]
    pub owner: DesignOperandOwner,
    /// Primary indexed-record identity.
    pub record_index: u32,
    /// Primary indexed-header byte offset.
    pub byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    pub class_tag: String,
    /// Asset UUID qualifying the persistent selection namespace.
    pub asset_id: String,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// UUID of the selection context.
    pub context_id: String,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub context_id_offset: u64,
    /// Raw four-byte selector-tail member after the fixed `u32 2`.
    ///
    /// Class `365` varies this member without a settled neutral meaning;
    /// class `367` stores `01 00 00 00`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector_tail: Option<Located<[u8; 4]>>,
    /// Counted persistent Design references carried by this operand.
    pub references: Vec<DesignBodyRecipeReference>,
    /// Tagged nested record reference following the Design reference.
    pub nested_record_index: u64,
    /// Byte offset of `nested_record_index`.
    pub nested_record_index_offset: u64,
    /// Body construction recipe contained by this operand record.
    pub recipe_id: String,
    /// Unique input-state face selected by this operand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_face_slot: Option<i64>,
    /// Exact ASM input state containing the resolved body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_body_state_id: Option<i64>,
    /// Unique input-state body containing every reference's candidate faces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_body_slot: Option<i64>,
    /// Complete boundary-face set of the resolved body in its input state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resolved_body_face_slots: Vec<i64>,
    /// Identity of the indexed record immediately following this operand.
    pub next_record_index: u32,
    /// Byte offset of the indexed record immediately following this operand.
    pub next_byte_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignBodyRecipeOperandWire {
    /// Globally unique deterministic identifier for this native operand.
    id: String,
    /// Owning feature scope record.
    scope_record_index: u32,
    /// Exact feature-scope ownership form.
    #[serde(flatten)]
    owner: DesignOperandOwner,
    /// Primary indexed-record identity.
    record_index: u32,
    /// Primary indexed-header byte offset.
    byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    class_tag: String,
    /// Asset UUID qualifying the persistent selection namespace.
    asset_id: String,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    asset_id_offset: u64,
    /// UUID of the selection context.
    context_id: String,
    /// Byte offset of the context UUID's UTF-16LE code units.
    context_id_offset: u64,
    /// Raw four-byte selector-tail member after the fixed `u32 2`.
    ///
    /// Class `365` varies this member without a settled neutral meaning;
    /// class `367` stores `01 00 00 00`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selector_tail: Option<[u8; 4]>,
    /// Byte offset of the raw selector-tail member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selector_tail_offset: Option<u64>,
    /// Counted persistent Design references carried by this operand.
    references: Vec<DesignBodyRecipeReference>,
    /// Tagged nested record reference following the Design reference.
    nested_record_index: u64,
    /// Byte offset of `nested_record_index`.
    nested_record_index_offset: u64,
    /// Body construction recipe contained by this operand record.
    recipe_id: String,
    /// Unique input-state face selected by this operand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolved_face_slot: Option<i64>,
    /// Exact ASM input state containing the resolved body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolved_body_state_id: Option<i64>,
    /// Unique input-state body containing every reference's candidate faces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolved_body_slot: Option<i64>,
    /// Complete boundary-face set of the resolved body in its input state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    resolved_body_face_slots: Vec<i64>,
    /// Identity of the indexed record immediately following this operand.
    next_record_index: u32,
    /// Byte offset of the indexed record immediately following this operand.
    next_byte_offset: u64,
}

impl TryFrom<DesignBodyRecipeOperandWire> for DesignBodyRecipeOperand {
    type Error = String;
    fn try_from(wire: DesignBodyRecipeOperandWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            owner: wire.owner,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag,
            asset_id: wire.asset_id,
            asset_id_offset: wire.asset_id_offset,
            context_id: wire.context_id,
            context_id_offset: wire.context_id_offset,
            selector_tail: Located::from_wire(wire.selector_tail, wire.selector_tail_offset, "selector_tail")?,
            references: wire.references,
            nested_record_index: wire.nested_record_index,
            nested_record_index_offset: wire.nested_record_index_offset,
            recipe_id: wire.recipe_id,
            resolved_face_slot: wire.resolved_face_slot,
            resolved_body_state_id: wire.resolved_body_state_id,
            resolved_body_slot: wire.resolved_body_slot,
            resolved_body_face_slots: wire.resolved_body_face_slots,
            next_record_index: wire.next_record_index,
            next_byte_offset: wire.next_byte_offset,
        })
    }
}

impl From<DesignBodyRecipeOperand> for DesignBodyRecipeOperandWire {
    fn from(record: DesignBodyRecipeOperand) -> Self {
        Self {
            id: record.id,
            scope_record_index: record.scope_record_index,
            owner: record.owner,
            record_index: record.record_index,
            byte_offset: record.byte_offset,
            class_tag: record.class_tag,
            asset_id: record.asset_id,
            asset_id_offset: record.asset_id_offset,
            context_id: record.context_id,
            context_id_offset: record.context_id_offset,
            selector_tail: record.selector_tail.map(|tail| tail.value),
            selector_tail_offset: record.selector_tail.map(|tail| tail.offset),
            references: record.references,
            nested_record_index: record.nested_record_index,
            nested_record_index_offset: record.nested_record_index_offset,
            recipe_id: record.recipe_id,
            resolved_face_slot: record.resolved_face_slot,
            resolved_body_state_id: record.resolved_body_state_id,
            resolved_body_slot: record.resolved_body_slot,
            resolved_body_face_slots: record.resolved_body_face_slots,
            next_record_index: record.next_record_index,
            next_byte_offset: record.next_byte_offset,
        }
    }
}

/// Construction-operand group record and member position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignOperandGroup {
    /// Owning construction-operand group record.
    pub group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    pub group_member_ordinal: u32,
}

/// Exact owner of a construction operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(untagged)]
pub enum DesignOperandOwner {
    /// Operand named by a counted construction-operand group.
    Group {
        /// Owning construction-operand group record.
        group_record_index: u32,
        /// Zero-based position in the group's ordered member run.
        group_member_ordinal: u32,
    },
    /// Standalone operand named directly by the feature scope reference table.
    ScopeReference {
        /// Zero-based position in the scope's ordered reference table.
        scope_reference_ordinal: u32,
    },
}

impl DesignOperandOwner {
    /// Return the construction-group record and member position, when grouped.
    pub const fn group(self) -> Option<(u32, u32)> {
        match self {
            Self::Group {
                group_record_index,
                group_member_ordinal,
            } => Some((group_record_index, group_member_ordinal)),
            Self::ScopeReference { .. } => None,
        }
    }
}

/// One counted persistent reference inside a whole-body recipe operand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignBodyRecipeReference {
    /// Persistent Design reference.
    pub design_reference: u64,
    /// Byte offset of `design_reference`.
    pub design_reference_offset: u64,
    /// Reference-local serialized form discriminator.
    pub form: u32,
    /// Byte offset of `form`.
    pub form_offset: u64,
    /// Solved faces carrying this reference, ordered by face identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_faces: Vec<cadmpeg_ir::ids::FaceId>,
    /// Candidate faces present in the owning feature's input topology.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_candidate_faces: Vec<cadmpeg_ir::ids::FaceId>,
    /// Input-state bodies containing at least one candidate face.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_body_slots: Vec<i64>,
}

/// Stable ASM entity family named by a Design persistent identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AsmHistoricalEntityKind {
    /// Body topology slot.
    Body,
    /// Region topology slot.
    Region,
    /// Shell topology slot.
    Shell,
    /// Face topology slot.
    Face,
    /// Loop topology slot.
    Loop,
    /// Coedge topology slot.
    Coedge,
    /// Edge topology slot.
    Edge,
    /// Vertex topology slot.
    Vertex,
    /// Point carrier slot.
    Point,
    /// Surface carrier slot.
    Surface,
    /// Curve carrier slot.
    Curve,
    /// Parametric-curve carrier slot.
    Pcurve,
}

/// Persistent selection identity owned by a Fillet or Chamfer operand group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignEdgeIdentityOperand {
    /// Globally unique deterministic identifier for this native operand.
    pub id: String,
    /// Owning parameter-scope record.
    pub scope_record_index: u32,
    /// Owning construction-operand group record.
    pub group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    pub group_member_ordinal: u32,
    /// Indexed-record identity named by the construction group.
    pub record_index: u32,
    /// Byte offset of the indexed-record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: String,
    /// Whether the identity uses the compact eleven-zero prologue.
    #[serde(default)]
    pub compact_layout: bool,
    /// Local persistent selection identity preceding the two UUID fields.
    pub local_id: u64,
    /// Byte offset of `local_id`.
    pub local_id_offset: u64,
    /// Asset UUID qualifying the local selection identity.
    pub asset_id: String,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// UUID of the local selection-identity context.
    pub context_id: String,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub context_id_offset: u64,
    /// Stable ASM history family, entity slot, and states carrying `local_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(flatten)]
    pub historical: Option<HistoricalBinding>,
    /// Complete radius-qualified deleted source-edge set proved by the owning
    /// feature transition. The transition-scoped set repeats on each operand.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub treatment_radius_candidates: Vec<DesignEdgeTreatmentRadiusCandidate>,
    /// Complete deleted source-edge chain proved by the owning feature
    /// transition. The transition-scoped chain repeats on each operand.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transition_edge_candidates: Vec<i64>,
    /// Ordered deleted treatment edges selected by an embedded bounded-face
    /// rule owned by this operand.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resolved_edge_slots: Vec<i64>,
    /// Unique edge slot selected in the owning feature's preceding state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_edge_slot: Option<i64>,
    /// Native identity or embedded bounded-face operand proving the resolved
    /// edge selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_identity_id: Option<String>,
}

impl DesignEdgeIdentityOperand {
    pub(crate) fn historical_entity_kind(&self) -> Option<AsmHistoricalEntityKind> {
        self.historical.as_ref().map(|binding| binding.kind)
    }

    pub(crate) fn historical_entity_ref(&self) -> Option<i64> {
        self.historical.as_ref().map(|binding| binding.entity_ref)
    }

    pub(crate) fn historical_state_ids(&self) -> &[i64] {
        self.historical
            .as_ref()
            .map(|binding| binding.state_ids.as_slice())
            .unwrap_or(&[])
    }
}

/// Edge-selection operand owned by an edge-selecting parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignEdgeOperand {
    /// Globally unique deterministic identifier for this native operand.
    pub id: String,
    /// Owning parameter-scope record.
    pub scope_record_index: u32,
    /// Zero-based position in the scope's ordered reference table.
    pub scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    pub record_index: u32,
    /// Byte offset of the primary indexed-record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: String,
    /// Byte offset of the same-index paired header.
    pub paired_byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub paired_class_tag: String,
    /// Indexed record containing the edge regeneration recipe.
    pub recipe_record_index: u32,
    /// Byte offset of the recipe record's indexed header.
    pub recipe_record_byte_offset: u64,
    /// Native construction-recipe arena id.
    pub recipe_id: String,
    /// Byte offset of the recipe-specific prefix after the indexed header.
    pub recipe_prefix_offset: u64,
    /// Complete recipe-specific prefix before the length-prefixed family name.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub recipe_prefix_bytes: Vec<u8>,
    /// Persistent Design selector/reference entries decoded from the prefix.
    pub recipe_references: Vec<DesignRecipeReference>,
    /// Byte offset of the first i32 after the framed recipe-family name.
    pub recipe_program_offset: u64,
    /// Complete post-name i32 program ending at the next indexed record.
    pub recipe_program: Vec<i32>,
    /// Standard two-side structure decoded from the recipe program.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_structure: Option<DesignEdgeRecipeStructure>,
    /// Alternate two-clause structure decoded from a `SurfacePatch` edge
    /// recipe program.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_patch_recipe_structure: Option<DesignSurfacePatchRecipeStructure>,
    /// Ordered local topology references when every nonzero root and side scalar
    /// is a valid prefix-reference ordinal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_topology_references: Option<Vec<NonZeroU32>>,
    /// Active solved faces carrying the recipe's persistent Design reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_faces: Vec<FaceId>,
    /// Candidate faces present in the ASM topology produced by the owning
    /// edge-treatment feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub result_candidate_faces: Vec<FaceId>,
    /// Stable edge slots on the result candidate-face boundaries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub result_boundary_edge_slots: Vec<i64>,
    /// Candidate faces present in the ASM topology immediately preceding the
    /// owning edge-treatment feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_candidate_faces: Vec<FaceId>,
    /// Candidate and effective prefix-reference faces in the terminal topology
    /// used by a suppressed feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terminal_candidate_faces: Vec<FaceId>,
    /// Preceding candidate faces deleted or updated by the owning feature's
    /// exact ASM state transition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_candidate_faces: Vec<FaceId>,
    /// Stable edge slots on the preceding candidate-face boundaries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_boundary_edge_slots: Vec<i64>,
    /// Stable edge slots on terminal candidate-face boundaries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terminal_boundary_edge_slots: Vec<i64>,
    /// Preceding boundary-edge slots deleted or updated by the owning feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_boundary_edge_slots: Vec<i64>,
    /// Preceding boundary-edge slots deleted by the owning feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted_boundary_edge_slots: Vec<i64>,
    /// Preceding boundary-edge slots assigned a different record revision by
    /// the owning feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub updated_boundary_edge_slots: Vec<i64>,
    /// Deleted predecessor edges associated with inserted treatment-carrier radii.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub treatment_radius_candidates: Vec<DesignEdgeTreatmentRadiusCandidate>,
    /// Ordered incident-loop topology for every changed boundary edge.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_boundary_edge_contexts: Vec<DesignHistoricalEdgeContext>,
    /// Ordered incident-loop topology for terminal candidate-face boundaries
    /// used by a suppressed feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terminal_boundary_edge_contexts: Vec<DesignHistoricalEdgeContext>,
    /// Boundary-edge sets of the prefix-reference faces in the terminal
    /// topology, indexed by zero-based prefix-reference ordinal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terminal_reference_edge_slots: Vec<Vec<i64>>,
    /// Ordered historical topology context for each prefix reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recipe_reference_contexts: Vec<DesignEdgeRecipeReferenceContext>,
    /// Topology entries grouped by source selector with evaluation-state edge
    /// context matching the selector's incident-loop boundary counts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recipe_selectors: Vec<DesignEdgeRecipeSelectorContext>,
    /// Historical topology state against which the edge recipe was evaluated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_state_id: Option<i64>,
    /// Stable historical edge slot proven by the selector/reference candidate
    /// intersection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_edge_slot: Option<i64>,
    /// Selected historical carrier axis, when exact.
    #[serde(flatten)]
    pub resolved_axis: Option<EdgeResolvedAxisWire>,
    /// Identity of the indexed record following the operand frame.
    pub next_record_index: u32,
    /// Byte offset of the indexed record following the operand frame.
    pub next_byte_offset: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct EdgeResolvedAxisWire {
    #[serde(rename = "resolved_axis_origin")]
    pub origin: Point3,
    #[serde(rename = "resolved_axis_direction")]
    pub direction: Vector3,
}

impl From<DesignAxis> for EdgeResolvedAxisWire {
    fn from(axis: DesignAxis) -> Self {
        Self {
            origin: axis.origin,
            direction: axis.direction,
        }
    }
}

impl From<EdgeResolvedAxisWire> for DesignAxis {
    fn from(axis: EdgeResolvedAxisWire) -> Self {
        Self {
            origin: axis.origin,
            direction: axis.direction,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
/// One radius-qualified historical edge candidate recovered from an inserted
/// treatment face and its carrier-stable adjacent supports.
pub struct DesignEdgeTreatmentRadiusCandidate {
    /// Deleted stable edge slot shared by the preceding support faces.
    pub edge_slot: i64,
    /// Positive characteristic radius of the inserted treatment carrier.
    pub radius: f64,
}

/// Stable surface-support relation from an active face candidate to the
/// topology preceding its owning feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignHistoricalFaceSupportContext {
    /// Stable slot of the active face candidate.
    pub active_face_slot: i64,
    /// Invariant stable surface-carrier slot.
    pub surface_slot: i64,
    /// Preceding face slots owning the surface carrier.
    pub preceding_face_slots: Vec<i64>,
    /// Ordered loop boundaries of the preceding carrier owners.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_face_boundaries: Vec<DesignHistoricalFaceBoundaryContext>,
    /// Preceding owners deleted or updated by the feature transition.
    pub changed_preceding_face_slots: Vec<i64>,
}

/// Historical edge-boundary context for one ordered edge-recipe prefix reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignEdgeRecipeReferenceContext {
    /// Zero-based position in the edge recipe's prefix reference sequence.
    pub reference_ordinal: u32,
    /// Referenced faces present in the owning feature's result topology.
    pub result_faces: Vec<FaceId>,
    /// Ordered loop boundaries of each referenced result face.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub result_face_boundaries: Vec<DesignHistoricalFaceBoundaryContext>,
    /// Stable result edge slots shared by the referenced-face boundaries and
    /// the primary candidate-face boundaries.
    pub result_shared_edge_slots: Vec<i64>,
    /// Referenced faces present in the immediately preceding ASM topology.
    pub preceding_faces: Vec<FaceId>,
    /// Ordered loop boundaries of each referenced preceding face.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_face_boundaries: Vec<DesignHistoricalFaceBoundaryContext>,
    /// Preceding faces uniquely owning the surface carriers of the referenced
    /// result faces.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_support_face_slots: Vec<i64>,
    /// Ordered loop boundaries of the uniquely matched preceding support
    /// faces.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_support_face_boundaries: Vec<DesignHistoricalFaceBoundaryContext>,
    /// Stable edge slots shared by the referenced-face boundaries and the
    /// primary candidate-face boundaries.
    pub shared_edge_slots: Vec<i64>,
    /// Shared edge slots deleted or updated by the owning feature transition.
    pub changed_shared_edge_slots: Vec<i64>,
    /// Changed primary-boundary edges belonging to either a directly
    /// persistent referenced face or its unique preceding surface support.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_reference_edge_slots: Vec<i64>,
}

/// Ordered loop topology retained for one historical face.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignHistoricalFaceBoundaryContext {
    /// Stable ASM face slot.
    pub face_slot: i64,
    /// Face loops in their serialized membership order.
    pub loops: Vec<DesignHistoricalFaceLoopContext>,
}

/// Ordered coedge and edge membership of one historical face loop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignHistoricalFaceLoopContext {
    /// Stable ASM loop slot.
    pub loop_slot: i64,
    /// Stable coedge slots in cyclic loop order.
    pub coedge_slots: Vec<i64>,
    /// Stable edge slots aligned one-to-one with `coedge_slots`.
    pub edge_slots: Vec<i64>,
    /// Stable boundary-vertex slots preceding the aligned coedges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vertex_slots: Vec<i64>,
    /// Stable point-carrier slots aligned one-to-one with `vertex_slots`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub point_slots: Vec<i64>,
    /// Model-space positions aligned one-to-one with `point_slots`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub positions: Vec<cadmpeg_ir::math::Point3>,
}

/// Historical topology surrounding one candidate edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignHistoricalEdgeContext {
    /// Stable ASM edge slot.
    pub edge_slot: i64,
    /// Incident coedge uses in stable coedge-slot order.
    pub incident_loops: Vec<DesignHistoricalEdgeLoopContext>,
}

/// One historical coedge use of a candidate edge and its ordered loop neighbors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignHistoricalEdgeLoopContext {
    /// Stable ASM coedge slot using the candidate edge.
    pub coedge_slot: i64,
    /// Stable ASM owner-loop slot.
    pub loop_slot: i64,
    /// Stable ASM owner-face slot.
    pub face_slot: i64,
    /// Number of coedges in the owner loop.
    pub boundary_edge_count: u32,
    /// Zero-based position of this coedge in the owner loop's ordered membership.
    pub coedge_ordinal: u32,
    /// Stable edge slot used by the preceding coedge.
    pub previous_edge_slot: i64,
    /// Stable edge slot used by the following coedge.
    pub next_edge_slot: i64,
}

/// Edge-recipe topology entries sharing one selector value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignEdgeRecipeSelectorContext {
    /// Selector value stored in each grouped entry.
    pub selector: i32,
    /// Entry from each ordered recipe clause; a selector occurs at most once in
    /// one clause.
    pub clause_entries: Vec<Option<DesignTopologyRecipeEntry>>,
    /// Changed historical edge slots at the loop position named by each of the
    /// two triplets in each present clause entry.
    pub clause_triplet_edge_slots: Vec<Option<[Vec<i64>; 2]>>,
    /// Changed historical edges satisfying both triplets of every present
    /// clause entry.
    pub incidence_matching_edge_slots: Vec<i64>,
    /// The sole incidence-compatible historical edge when the matching set is
    /// a singleton.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unique_incidence_edge_slot: Option<i64>,
    /// Changed historical edges whose incident loop counts satisfy every
    /// present clause entry.
    pub boundary_count_matching_edge_slots: Vec<i64>,
}

/// Standard delimiter structure following an edge recipe's common prologue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignEdgeRecipeStructure {
    /// Number of ordered side clauses.
    pub root: i32,
    /// Ordered side clauses.
    pub sides: Vec<DesignTopologyRecipeSide>,
}

/// The alternate two-clause structure used by a fixed-path `SurfacePatch`
/// edge recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignSurfacePatchRecipeStructure {
    /// Root discriminator. Value `2` identifies the two-clause form.
    pub root: i32,
    /// Ordered clauses in the recipe program.
    pub clauses: Vec<DesignSurfacePatchRecipeClause>,
}

/// One clause in a `SurfacePatch` edge recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignSurfacePatchRecipeClause {
    /// Six delimiter-bounded fields before the counted topology payload.
    pub fields: Vec<Vec<i32>>,
    /// Zero-based face-reference ordinals named by the first two fields.
    pub face_reference_ordinals: [u32; 2],
    /// Zero-based edge-reference ordinals named by the third and fifth fields.
    pub edge_reference_ordinals: [u32; 2],
    /// Number of eight-word topology entries in the payload.
    pub payload_entry_count: u32,
    /// Ordered topology entries in the payload.
    pub entries: Vec<DesignTopologyRecipeEntry>,
}

/// One delimiter-bounded side clause in a standard edge recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignTopologyRecipeSide {
    /// Encoded number of fields after the header count: scalar fields plus the payload.
    pub field_count: NonZeroU32,
    /// Second word of the side header.
    pub header_value: i32,
    /// Ordered scalar fields following the side header.
    pub scalars: Vec<i32>,
    /// Exact field program preceding the topology-entry count.
    pub payload_prefix: Vec<i32>,
    /// Encoded number of eight-word topology entries following the field program.
    pub payload_entry_count: u32,
    /// Ordered eight-word payload entries.
    pub entries: Vec<DesignTopologyRecipeEntry>,
}

/// One eight-word topology entry in an edge-recipe side clause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignTopologyRecipeEntry {
    /// Nonnegative clause-local selector, strictly increasing within one clause.
    pub selector: i32,
    /// Number of boundary edges on the referenced face loop.
    pub boundary_edge_count: NonZeroU32,
    /// Two ordered topology triplets.
    pub topology_triplets: [DesignTopologyRecipeTriplet; 2],
    /// Zero-based boundary-edge ordinal named by both triplets when equal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub common_incident_edge_ordinal: Option<u32>,
}

/// One three-word invariant in an edge-recipe entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignTopologyRecipeTriplet {
    /// Equal positive first and third words, not exceeding the containing
    /// entry's boundary-edge count.
    pub outer: NonZeroU32,
    /// Signed middle word retained from the source triplet.
    pub middle: i32,
    /// Zero-based loop vertex ordinal encoded by `outer`.
    pub vertex_ordinal: u32,
    /// Zero-based boundary-edge ordinal incident to `vertex_ordinal`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incident_edge_ordinal: Option<u32>,
    /// Whether the incident edge precedes or follows the vertex in loop order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incident_side: Option<DesignTopologyIncidentSide>,
}

/// Which loop edge incident to a recipe vertex is named by a topology triplet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DesignTopologyIncidentSide {
    /// Edge immediately preceding the vertex in cyclic loop order.
    Preceding,
    /// Edge immediately following the vertex in cyclic loop order.
    Following,
}

/// Face-selection operand owned by a parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignFaceOperand {
    /// Globally unique deterministic identifier for this native operand.
    pub id: String,
    /// Owning parameter-scope record.
    pub scope_record_index: u32,
    /// Zero-based position in the scope's ordered reference table.
    pub scope_reference_ordinal: u32,
    /// Owning construction-operand group, absent for a direct scope operand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(flatten)]
    pub group: Option<DesignOperandGroup>,
    /// Primary indexed-record identity named by a face operand group.
    pub record_index: u32,
    /// Byte offset of the primary indexed-record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: String,
    /// Byte offset of the same-index paired header.
    pub paired_byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub paired_class_tag: String,
    /// Indexed record containing the face regeneration recipe.
    pub recipe_record_index: u32,
    /// Byte offset of the recipe record's indexed header.
    pub recipe_record_byte_offset: u64,
    /// Native construction-recipe arena id.
    pub recipe_id: String,
    /// Byte offset of the recipe-specific prefix after the indexed header.
    pub recipe_prefix_offset: u64,
    /// Complete recipe-specific prefix before the length-prefixed family name.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub recipe_prefix_bytes: Vec<u8>,
    /// Persistent Design selector/reference entries decoded from the prefix.
    pub recipe_references: Vec<DesignRecipeReference>,
    /// Exact face-recipe family.
    pub recipe_kind: ConstructionRecipeKind,
    /// Byte offset of the first i32 after the framed recipe-family name.
    pub recipe_program_offset: u64,
    /// Complete post-name i32 program ending at the next indexed record.
    pub recipe_program: Vec<i32>,
    /// Byte offsets of the `[-1, -1, 2]` node openers declared by the program.
    pub recipe_node_offsets: Vec<u64>,
    /// Ordered nodes partitioning the program after its three-word header.
    pub recipe_nodes: Vec<DesignFaceRecipeNode>,
    /// Active solved faces carrying the recipe's persistent Design reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_faces: Vec<FaceId>,
    /// Candidate faces not explicitly named as topology context by a prefix
    /// selector carrying the recipe's own Design reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unreferenced_candidate_faces: Vec<FaceId>,
    /// Faces named by a prefix operand carrying the recipe's own token and
    /// Design reference under a different native selector.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternate_selector_candidate_faces: Vec<FaceId>,
    /// Candidate faces present in the ASM topology immediately preceding the
    /// owning feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_candidate_faces: Vec<FaceId>,
    /// Preceding candidate faces deleted or updated by the owning feature's
    /// exact ASM state transition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_candidate_faces: Vec<FaceId>,
    /// Active candidates mapped through an invariant surface carrier to face
    /// owners in the immediately preceding historical topology.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub historical_support_contexts: Vec<DesignHistoricalFaceSupportContext>,
    /// Ordered stable historical face slots proven by the preceding topology
    /// or exact feature transition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resolved_face_slots: Vec<i64>,
    /// Current active-BREP face identity proven by a legacy Extrude recipe
    /// when no preceding historical slot exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_active_face: Option<FaceId>,
    /// Identity of the indexed record following the operand frame.
    pub next_record_index: u32,
    /// Byte offset of the indexed record following the operand frame.
    pub next_byte_offset: u64,
}

impl DesignFaceOperand {
    pub(crate) fn group_record_index(&self) -> Option<u32> {
        self.group.map(|group| group.group_record_index)
    }

    pub(crate) fn group_member_ordinal(&self) -> Option<u32> {
        self.group.map(|group| group.group_member_ordinal)
    }
}

/// Native source-shape carrier owned by a `Face` parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "DesignFaceSourceGroupWire"))]
#[serde(try_from = "DesignFaceSourceGroupWire", into = "DesignFaceSourceGroupWire")]
pub struct DesignFaceSourceGroup {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Owning `Face` parameter-scope record.
    pub scope_record_index: u32,
    /// Zero-based position of the source carrier in the scope reference table.
    pub carrier_reference_ordinal: u32,
    /// Indexed record carrying the ordered source-shape references.
    pub carrier_record_index: u32,
    /// Source interval from the carrier header to its paired header.
    pub carrier_span: NonEmptyByteSpan,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub carrier_class_tag: String,
    /// Indexed record paired with the source carrier.
    pub paired_record_index: u32,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub paired_class_tag: String,
    /// Ordered persistent source-shape identities.
    pub source_members: Vec<Located<DesignFaceSourceMember>>,
}

/// Native source-shape carrier owned by a `Face` parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignFaceSourceGroupWire {
    /// Globally unique deterministic identifier for this native record.
    id: String,
    /// Owning `Face` parameter-scope record.
    scope_record_index: u32,
    /// Zero-based position of the source carrier in the scope reference table.
    carrier_reference_ordinal: u32,
    /// Indexed record carrying the ordered source-shape references.
    carrier_record_index: u32,
    /// Byte offset of the source carrier's indexed header.
    carrier_byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    carrier_class_tag: String,
    /// Bytes from the carrier header to its paired carrier header.
    carrier_frame_length: u64,
    /// Indexed record paired with the source carrier.
    paired_record_index: u32,
    /// Byte offset of the paired carrier's indexed header.
    paired_byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    paired_class_tag: String,
    /// Absolute byte offsets of the marked source-reference slots.
    source_reference_offsets: Vec<u64>,
    /// Ordered persistent source-shape identities.
    source_members: Vec<DesignFaceSourceMember>,
}

impl TryFrom<DesignFaceSourceGroupWire> for DesignFaceSourceGroup {
    type Error = String;
    fn try_from(wire: DesignFaceSourceGroupWire) -> Result<Self, Self::Error> {
        if wire.source_members.len() != wire.source_reference_offsets.len() {
            return Err("source_members and source_reference_offsets must have equal lengths".into());
        }
        let carrier_span = NonEmptyByteSpan::new(wire.carrier_byte_offset, wire.paired_byte_offset)
            .ok_or("paired_byte_offset must follow carrier_byte_offset")?;
        if wire.carrier_frame_length != carrier_span.byte_len() {
            return Err("carrier_frame_length must match the carrier byte span".into());
        }
        Ok(Self {
            carrier_span,
            source_members: wire.source_members.into_iter().zip(wire.source_reference_offsets).map(|(value, offset)| Located { value, offset }).collect(),
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            carrier_reference_ordinal: wire.carrier_reference_ordinal,
            carrier_record_index: wire.carrier_record_index,
            carrier_class_tag: wire.carrier_class_tag,
            paired_record_index: wire.paired_record_index,
            paired_class_tag: wire.paired_class_tag,
        })
    }
}

impl From<DesignFaceSourceGroup> for DesignFaceSourceGroupWire {
    fn from(group: DesignFaceSourceGroup) -> Self {
        let (source_members, source_reference_offsets) = group.source_members.into_iter().map(|member| (member.value, member.offset)).unzip();
        Self {
            source_members,
            source_reference_offsets,
            id: group.id,
            scope_record_index: group.scope_record_index,
            carrier_reference_ordinal: group.carrier_reference_ordinal,
            carrier_record_index: group.carrier_record_index,
            carrier_byte_offset: group.carrier_span.start(),
            carrier_class_tag: group.carrier_class_tag,
            carrier_frame_length: group.carrier_span.byte_len(),
            paired_record_index: group.paired_record_index,
            paired_byte_offset: group.carrier_span.end(),
            paired_class_tag: group.paired_class_tag,
        }
    }
}

/// Persistent source-shape identity named by a `Face` source carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignFaceSourceMember {
    /// Indexed record named by the carrier's source-reference slot.
    pub record_index: u32,
    /// Byte offset of the persistent-identity record's indexed header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII identity class tag.
    pub class_tag: String,
    /// Fixed persistent identity carried by the source record.
    pub persistent_identity: DesignConstructionPersistentIdentity,
}

/// One length-delimited node in a face regeneration recipe program.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignFaceRecipeNode {
    /// Byte offset of the node's `[-1, -1, 2]` opener.
    pub byte_offset: u64,
    /// Exclusive byte offset of the next node or the operand's following record.
    pub end_byte_offset: u64,
    /// Complete node words, including the three-word opener.
    pub program: Vec<i32>,
    /// Shared two-side topology recipe structure following the node opener.
    pub recipe_structure: Option<DesignFaceRecipeStructure>,
}

/// Structured topology program following a face-recipe node opener.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignFaceRecipeStructure {
    /// Scalar before the prelude delimiters.
    pub root: i32,
    /// Two scalar prelude runs before the first side clause.
    pub prelude: [i32; 2],
    /// Two ordered topology side clauses.
    pub sides: [DesignTopologyRecipeSide; 2],
    /// Optional six-word postlude following the two side clauses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub postlude: Vec<i32>,
}

/// Typed sketch-container visibility bound to a Design sketch entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignSketchVisibility {
    /// One-based ordinal among sketch Geometry members in the Design stream.
    pub stream_ordinal: u32,
    /// Byte offset of `stream_ordinal`.
    pub stream_ordinal_offset: u64,
    /// Byte offset of the native visibility flag.
    pub visible_offset: u64,
    /// Direct display visibility.
    pub visible: bool,
}

/// Local-to-model placement frame referenced by a Design sketch scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignSketchPlacement {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Owning parameter-scope record; absent when the sketch has no parameter
    /// scope. A localized Sketch scope can own a member-run head placement
    /// through record interval order without directly referencing it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_record_index: Option<u32>,
    /// Full Design entity id of the placed sketch.
    pub entity_id: String,
    /// Numeric suffix of `entity_id`.
    pub entity_suffix: u64,
    /// Typed sketch-container visibility for the placed sketch entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<DesignSketchVisibility>,
    /// Byte offset of the primary indexed record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: String,
    /// Shared logical record identity.
    pub record_index: u32,
    /// Byte length from the primary header to the paired header.
    pub frame_length: u64,
    /// Row-major local-to-model affine transform.
    pub transform: [[f64; 4]; 4],
    /// Byte offset of the explicit 16-f64 matrix; absent for the compact identity form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform_offset: Option<u64>,
    /// Per-file dynamic class tag of the paired header.
    pub paired_class_tag: String,
    /// Byte offset of the paired indexed record header.
    pub paired_byte_offset: u64,
    /// Whether this placement is the transform-carrying member-run head
    /// record named by the sketch entity's paired record rather than a
    /// parameter-scope placement frame.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub member_run_head: bool,
}

/// Persistent-reference channel in the Design construction stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PersistentReferenceKind {
    /// Reference identifies a persistent point.
    Point,
    /// Reference identifies the primary id of a persistent curve.
    CurvePrimary,
    /// Reference identifies the secondary id of a persistent curve.
    CurveSecondary,
}

/// One byte-stored persistent point or curve identifier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PersistentReference {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the persistent-reference field name in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of the u64 value relative to `byte_offset`.
    pub value_offset: u32,
    /// Whether this reference identifies a persistent point or one end of a curve.
    pub kind: PersistentReferenceKind,
    /// Raw persistent point/curve identifier as stored in the `Design` construction stream.
    pub value: u64,
}

/// A construction-history edge selection that Fusion could not re-resolve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct LostEdgeReference {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the unresolved record's indexed header.
    pub record_byte_offset: u64,
    /// Byte offset of the unresolved record's three-byte class tag.
    pub class_tag_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag of the unresolved record.
    pub class_tag: String,
    /// Source `BulkStream` record index of the unresolved edge selection.
    pub record_index: u32,
    /// Byte offset of `record_index`.
    pub record_index_offset: u64,
    /// Byte offset of the `EDGE_REFERENCE_LOST` marker in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of the indexed header immediately following this record.
    pub next_byte_offset: u64,
    /// Per-file dynamic class tag of the following indexed record.
    pub next_class_tag: String,
    /// Record index of the following indexed record.
    pub next_record_index: u32,
}

/// One Design `BulkStream` material assignment joining a design entity to visual assets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DesignMaterialAssignmentWire", into = "DesignMaterialAssignmentWire")]
pub struct DesignMaterialAssignment {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// ASM body key resolved through the Design body map.
    pub asm_body_key: u64,
    /// Byte offset of the body-map ASM key.
    pub asm_body_key_offset: u64,
    /// Numeric suffix of `entity_id`.
    pub entity_suffix: u64,
    /// Byte offset of the body-map entity suffix.
    pub entity_suffix_offset: u64,
    /// UTF-16 design-entity id.
    pub entity_id: String,
    /// Byte offset of the UTF-16 entity-id code units.
    pub entity_id_offset: u64,
    /// Complete serialized visual token.
    pub visual_guid: String,
    /// Byte offset of the UTF-16 visual-token code units.
    pub visual_guid_offset: u64,
    /// Physical-material token, when present.
    pub physical_token: Option<RecordedValue<String>>,
    /// Visual preset name, when present.
    pub visual_preset: Option<RecordedValue<String>>,
}

/// One Design `BulkStream` material assignment joining a design entity to visual assets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignMaterialAssignmentWire {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// ASM body key resolved through the Design body map.
    pub asm_body_key: u64,
    /// Byte offset of the body-map ASM key.
    pub asm_body_key_offset: u64,
    /// Numeric suffix of `entity_id`.
    pub entity_suffix: u64,
    /// Byte offset of the body-map entity suffix.
    pub entity_suffix_offset: u64,
    /// UTF-16 design-entity id.
    pub entity_id: String,
    /// Byte offset of the UTF-16 entity-id code units.
    pub entity_id_offset: u64,
    /// Complete serialized visual token.
    pub visual_guid: String,
    /// Byte offset of the UTF-16 visual-token code units.
    pub visual_guid_offset: u64,
    /// Physical-material token, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical_token: Option<String>,
    /// Byte offset of the UTF-16 physical token, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical_token_offset: Option<u64>,
    /// Visual preset name, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_preset: Option<String>,
    /// Byte offset of the UTF-16 preset name, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_preset_offset: Option<u64>,
}

impl TryFrom<DesignMaterialAssignmentWire> for DesignMaterialAssignment {
    type Error = String;
    fn try_from(wire: DesignMaterialAssignmentWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            asm_body_key: wire.asm_body_key,
            asm_body_key_offset: wire.asm_body_key_offset,
            entity_suffix: wire.entity_suffix,
            entity_suffix_offset: wire.entity_suffix_offset,
            entity_id: wire.entity_id,
            entity_id_offset: wire.entity_id_offset,
            visual_guid: wire.visual_guid,
            visual_guid_offset: wire.visual_guid_offset,
            physical_token: RecordedValue::from_wire(wire.physical_token, wire.physical_token_offset, "physical_token")?,
            visual_preset: RecordedValue::from_wire(wire.visual_preset, wire.visual_preset_offset, "visual_preset")?,
        })
    }
}

impl From<DesignMaterialAssignment> for DesignMaterialAssignmentWire {
    fn from(value: DesignMaterialAssignment) -> Self {
        Self {
            id: value.id,
            asm_body_key: value.asm_body_key,
            asm_body_key_offset: value.asm_body_key_offset,
            entity_suffix: value.entity_suffix,
            entity_suffix_offset: value.entity_suffix_offset,
            entity_id: value.entity_id,
            entity_id_offset: value.entity_id_offset,
            visual_guid: value.visual_guid,
            visual_guid_offset: value.visual_guid_offset,
            physical_token_offset: value.physical_token.as_ref().and_then(|field| field.offset),
            physical_token: value.physical_token.map(|field| field.value),
            visual_preset_offset: value.visual_preset.as_ref().and_then(|field| field.offset),
            visual_preset: value.visual_preset.map(|field| field.value),
        }
    }
}

/// Add-in module that registers the Design sketch types.
pub const DESIGN_MODULE_SKETCH: &str = "MSketch";
/// Add-in module that registers the Design body types.
pub const DESIGN_MODULE_BODY: &str = "Body";
/// Add-in module that registers the Design geometry types.
pub const DESIGN_MODULE_GEOMETRY: &str = "Geometry";
/// Add-in module that registers the Design component types.
pub const DESIGN_MODULE_COMPONENT: &str = "Component";
/// Add-in module that registers the root Fusion document types.
pub const DESIGN_MODULE_FUSION: &str = "Fusion";

/// JSON configuration payload stored in a Fusion design-configuration entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignConfiguration {
    /// Stable identity derived from the ZIP entry name.
    pub id: String,
    /// Complete ZIP entry name used for native regeneration.
    pub entry_name: String,
    /// Native configuration entry family.
    pub kind: DesignConfigurationKind,
    /// Variant names in serialized object-member order.
    #[serde(default)]
    pub variant_order: Vec<String>,
    /// Complete decoded JSON payload, including unrecognized fields.
    pub payload: serde_json::Value,
}

/// Native Fusion design-configuration entry family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DesignConfigurationKind {
    /// A `.dsgcfg` configuration table.
    Table,
    /// A `.dsgcfgrule` configuration rule.
    Rule,
}

/// One type-table entry from a `MetaStream` segment header. The entry registers
/// a record type and lists the entities whose sibling `BulkStream` records
/// carry it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "SegmentTypeWire"))]
#[serde(try_from = "SegmentTypeWire", into = "SegmentTypeWire")]
pub struct SegmentType {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of this type-table entry in its `MetaStream`.
    pub byte_offset: u64,
    /// GUID naming this entry's record type. Class tags are segment-local, so
    /// this GUID is the only discriminator that is stable across files.
    pub type_guid: String,
    /// Byte offset of the type-GUID bytes in the `MetaStream`.
    pub type_guid_offset: u64,
    /// GUID of this type's base type; `None` for a root type, whose stored base
    /// GUID is the empty string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_type_guid: Option<RecordedValue<String>>,
    /// Record version of this type.
    pub version: u32,
    /// Byte offset of `version` in the Design `MetaStream`.
    pub version_offset: u64,
    /// Add-in module that registers this type, e.g. `Fusion`, `MSketch`, or
    /// `Body`. Every type a module registers repeats the module name, so it
    /// classifies a type but does not identify one. Some types record no module.
    pub module: String,
    /// Entity ids whose records carry this type, in source `MetaStream` order;
    /// a count rather than a fixed-arity list, so length varies per entry.
    pub entities: ReferenceRun<u64>,
}

/// One type-table entry from a `MetaStream` segment header. The entry registers
/// a record type and lists the entities whose sibling `BulkStream` records
/// carry it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SegmentTypeWire {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of this type-table entry in its `MetaStream`.
    pub byte_offset: u64,
    /// GUID naming this entry's record type. Class tags are segment-local, so
    /// this GUID is the only discriminator that is stable across files.
    pub type_guid: String,
    /// Byte offset of the type-GUID bytes in the `MetaStream`.
    pub type_guid_offset: u64,
    /// GUID of this type's base type; `None` for a root type, whose stored base
    /// GUID is the empty string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_type_guid: Option<String>,
    /// Byte offset of the base-type-GUID bytes, when the entry names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_type_guid_offset: Option<u64>,
    /// Record version of this type.
    pub version: u32,
    /// Byte offset of `version` in the Design `MetaStream`.
    pub version_offset: u64,
    /// Add-in module that registers this type, e.g. `Fusion`, `MSketch`, or
    /// `Body`. Every type a module registers repeats the module name, so it
    /// classifies a type but does not identify one. Some types record no module.
    pub module: String,
    /// Entity ids whose records carry this type, in source `MetaStream` order;
    /// a count rather than a fixed-arity list, so length varies per entry.
    pub entity_ids: Vec<u64>,
    /// Byte offsets parallel to `entity_ids`.
    pub entity_id_offsets: Vec<u64>,
}

impl TryFrom<SegmentTypeWire> for SegmentType {
    type Error = String;
    fn try_from(wire: SegmentTypeWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            type_guid: wire.type_guid,
            type_guid_offset: wire.type_guid_offset,
            version: wire.version,
            version_offset: wire.version_offset,
            module: wire.module,
            entities: ReferenceRun::from_wire(wire.entity_ids, wire.entity_id_offsets, "entity_ids/entity_id_offsets")?,
            base_type_guid: RecordedValue::from_wire(wire.base_type_guid, wire.base_type_guid_offset, "base_type_guid")?,
        })
    }
}

impl From<SegmentType> for SegmentTypeWire {
    fn from(value: SegmentType) -> Self {
        let (entity_ids, entity_id_offsets) = value.entities.into_wire();
        Self {
            id: value.id,
            byte_offset: value.byte_offset,
            type_guid: value.type_guid,
            type_guid_offset: value.type_guid_offset,
            version: value.version,
            version_offset: value.version_offset,
            module: value.module,
            entity_ids,
            entity_id_offsets,
            base_type_guid_offset: value.base_type_guid.as_ref().and_then(|field| field.offset),
            base_type_guid: value.base_type_guid.map(|field| field.value),
        }
    }
}

/// Counted Design timeline-item list that carries authored feature order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "DesignFeatureTimelineWire"))]
#[serde(try_from = "DesignFeatureTimelineWire", into = "DesignFeatureTimelineWire")]
pub struct DesignFeatureTimeline {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of this record in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: String,
    /// Design entity identity of the timeline record.
    pub record_index: u64,
    /// Zero-based position in the `MetaStream` timeline-record list.
    pub source_ordinal: u32,
    /// Complete top-level record length.
    pub frame_length: u64,
    /// Same-segment context record referenced before the scope list.
    pub context_record_index: u64,
    /// Byte offset of `context_record_index`.
    pub context_record_index_offset: u64,
    /// Byte offset of the timeline-item count.
    pub item_count_offset: u64,
    /// Design record indices in authored timeline order.
    pub items: Vec<Located<u64>>,
}

/// Counted Design timeline-item list that carries authored feature order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignFeatureTimelineWire {
    /// Globally unique deterministic identifier for this native record.
    id: String,
    /// Byte offset of this record in its Design `BulkStream`.
    byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    class_tag: String,
    /// Design entity identity of the timeline record.
    record_index: u64,
    /// Zero-based position in the `MetaStream` timeline-record list.
    source_ordinal: u32,
    /// Complete top-level record length.
    frame_length: u64,
    /// Same-segment context record referenced before the scope list.
    context_record_index: u64,
    /// Byte offset of `context_record_index`.
    context_record_index_offset: u64,
    /// Byte offset of the timeline-item count.
    item_count_offset: u64,
    /// Design record indices in authored timeline order.
    item_record_indices: Vec<u64>,
    /// Byte offsets parallel to `item_record_indices`.
    item_record_index_offsets: Vec<u64>,
}

impl TryFrom<DesignFeatureTimelineWire> for DesignFeatureTimeline {
    type Error = String;
    fn try_from(wire: DesignFeatureTimelineWire) -> Result<Self, Self::Error> {
        if wire.item_record_indices.len() != wire.item_record_index_offsets.len() {
            return Err("item_record_indices and item_record_index_offsets must have equal lengths".into());
        }
        Ok(Self {
            items: wire.item_record_indices.into_iter().zip(wire.item_record_index_offsets).map(|(value, offset)| Located { value, offset }).collect(),
            id: wire.id,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag,
            record_index: wire.record_index,
            source_ordinal: wire.source_ordinal,
            frame_length: wire.frame_length,
            context_record_index: wire.context_record_index,
            context_record_index_offset: wire.context_record_index_offset,
            item_count_offset: wire.item_count_offset,
        })
    }
}
impl From<DesignFeatureTimeline> for DesignFeatureTimelineWire {
    fn from(value: DesignFeatureTimeline) -> Self {
        let (item_record_indices, item_record_index_offsets) = value.items.into_iter().map(|item| (item.value, item.offset)).unzip();
        Self { item_record_indices, item_record_index_offsets,
            id: value.id,
            byte_offset: value.byte_offset,
            class_tag: value.class_tag,
            record_index: value.record_index,
            source_ordinal: value.source_ordinal,
            frame_length: value.frame_length,
            context_record_index: value.context_record_index,
            context_record_index_offset: value.context_record_index_offset,
            item_count_offset: value.item_count_offset,
        }
    }
}

/// Self-validating entity-bound header in the Design `BulkStream`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "DesignEntityHeaderWire"))]
#[serde(try_from = "DesignEntityHeaderWire", into = "DesignEntityHeaderWire")]
pub struct DesignEntityHeader {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of this entity header in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Numeric suffix of the owning design-entity id (e.g. the `N` in `Body:N`).
    pub entity_suffix: u64,
    /// Full UTF-16LE-decoded design-entity id string for this header.
    pub entity_id: String,
    /// Source per-file dynamic three-digit ASCII class tag naming this header's record type.
    pub class_tag: String,
    /// Whether the flag-selected four-byte optional slot is present.
    pub optional_slot_present: bool,
    /// Add-in module of the `MetaStream` type whose entity-id list contains this
    /// header's entity, when the `MetaStream` registers that entity.
    pub module: Option<String>,
    /// Index of an associated `BulkStream` record, when the header carries one.
    pub record_reference: Option<u32>,
    /// Byte offset of the base-record slot, including its no-base-record sentinel.
    pub record_reference_offset: Option<u64>,
    /// Whether the wire includes the reference count; its value is derived from the run.
    pub reference_count_present: bool,
    /// Padded record-reference run owned by a sketch entity container.
    pub references: ReferenceRun<u32>,
    /// Counted member-record run from the paired same-index container record.
    pub members: ReferenceRun<u32>,
}


impl DesignEntityHeader {
    pub fn declared_reference_count(&self) -> Option<usize> {
        self.reference_count_present.then(|| self.references.len())
    }

    /// Whether the `MetaStream` registers this entity under the sketch module.
    pub fn in_sketch_module(&self) -> bool {
        self.module.as_deref() == Some(DESIGN_MODULE_SKETCH)
    }
}

/// Self-validating entity-bound header in the Design `BulkStream`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignEntityHeaderWire {
    /// Globally unique deterministic identifier for this native record.
    id: String,
    /// Byte offset of this entity header in its Design `BulkStream`.
    byte_offset: u64,
    /// Numeric suffix of the owning design-entity id (e.g. the `N` in `Body:N`).
    entity_suffix: u64,
    /// Full UTF-16LE-decoded design-entity id string for this header.
    entity_id: String,
    /// Source per-file dynamic three-digit ASCII class tag naming this header's record type.
    class_tag: String,
    /// Whether the flag-selected four-byte optional slot is present.
    optional_slot_present: bool,
    /// Add-in module of the `MetaStream` type whose entity-id list contains this
    /// header's entity, when the `MetaStream` registers that entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    module: Option<String>,
    /// Index of an associated `BulkStream` record, when the header carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    record_reference: Option<u32>,
    /// Byte offset of `record_reference` in the Design `BulkStream`, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    record_reference_offset: Option<u64>,
    /// Declared count of reference entries the header claims to own, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    declared_reference_count: Option<usize>,
    /// Padded record-reference run owned by a sketch entity container.
    #[serde(default)]
    reference_indices: Vec<u32>,
    /// Byte offsets parallel to `reference_indices`.
    #[serde(default)]
    reference_offsets: Vec<u64>,
    /// Counted member-record run from the paired same-index container record
    /// of an `EntityGenesis`-form sketch entity header.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    member_indices: Vec<u32>,
    /// Byte offsets parallel to `member_indices`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    member_offsets: Vec<u64>,
}
impl TryFrom<DesignEntityHeaderWire> for DesignEntityHeader {
    type Error = String;
    fn try_from(wire: DesignEntityHeaderWire) -> Result<Self, Self::Error> {
        if wire.declared_reference_count.is_some_and(|count| count != wire.reference_indices.len()) {
            return Err("declared_reference_count must match reference_indices".into());
        }
        Ok(Self {
            reference_count_present: wire.declared_reference_count.is_some(),
            references: ReferenceRun::from_wire(wire.reference_indices, wire.reference_offsets, "reference_indices/reference_offsets")?,
            members: ReferenceRun::from_wire(wire.member_indices, wire.member_offsets, "member_indices/member_offsets")?,
            id: wire.id,
            byte_offset: wire.byte_offset,
            entity_suffix: wire.entity_suffix,
            entity_id: wire.entity_id,
            class_tag: wire.class_tag,
            optional_slot_present: wire.optional_slot_present,
            module: wire.module,
            record_reference: wire.record_reference,
            record_reference_offset: wire.record_reference_offset,
        })
    }
}

impl From<DesignEntityHeader> for DesignEntityHeaderWire {
    fn from(header: DesignEntityHeader) -> Self {
        let declared_reference_count = header.declared_reference_count();
        let (reference_indices, reference_offsets) = header.references.into_wire();
        let (member_indices, member_offsets) = header.members.into_wire();
        Self {
            declared_reference_count,
            reference_indices,
            reference_offsets,
            member_indices,
            member_offsets,
            id: header.id,
            byte_offset: header.byte_offset,
            entity_suffix: header.entity_suffix,
            entity_id: header.entity_id,
            class_tag: header.class_tag,
            optional_slot_present: header.optional_slot_present,
            module: header.module,
            record_reference: header.record_reference,
            record_reference_offset: header.record_reference_offset,
        }
    }
}

/// Exact identity and source extent of one indexed Design mesh record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignMeshRecordIdentity {
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: String,
    /// Stream-local indexed-record identity.
    pub record_index: u32,
    /// Byte offset of the indexed header in the Design `BulkStream`.
    pub byte_offset: u64,
    /// Complete primary or nested record length in bytes.
    pub frame_length: u64,
}

/// One texture resource owned by a Design mesh feature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignMeshTextureResource {
    /// Zero-based position in the serialized flags map.
    pub ordinal: u32,
    /// Stable resource GUID used as the key in both texture maps.
    pub resource_guid: String,
    /// Byte offset of the flags-map GUID payload.
    pub flags_guid_offset: u64,
    /// Opaque resource flags retained without reinterpretation.
    pub flags: u32,
    /// Byte offset of `flags`.
    pub flags_offset: u64,
    /// Zero-based position of the same GUID in the serialized filename map.
    pub filename_ordinal: u32,
    /// Byte offset of the filename-map GUID payload.
    pub filename_guid_offset: u64,
    /// Record storing the archive-entry basename.
    pub filename_record: DesignMeshRecordIdentity,
    /// Byte offset of the filename-record reference.
    pub filename_record_reference_offset: u64,
    /// Archive-entry basename stored by `filename_record`.
    pub filename: String,
    /// Byte offset of the UTF-16LE filename code units.
    pub filename_offset: u64,
    /// Complete matching archive-entry name.
    pub archive_entry_name: String,
    /// Neutral embedded asset projected from the matching archive entry.
    pub asset: AssetId,
}

/// One finite axis-aligned bound stored by a mesh Scene record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignMeshSceneBounds {
    /// Component-wise upper corner, serialized first.
    pub maximum: [f64; 3],
    /// Component-wise lower corner, serialized second.
    pub minimum: [f64; 3],
    /// Byte offsets of the serialized upper and lower corners.
    pub offsets: [u64; 2],
}

/// One mesh body and its complete Design identity graph.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignMeshBody {
    /// Byte offset of the scope's reference to this body.
    pub scope_body_reference_offset: u64,
    /// Byte offset of the collection's reference to this body.
    pub collection_body_reference_offset: u64,
    /// Mesh-body record carrying placement and graph references.
    pub body_record: DesignMeshRecordIdentity,
    /// Entry-name record joining the body to one `.paramesh` archive entry.
    pub entry_name_record: DesignMeshRecordIdentity,
    /// GUID record joining the body to the container's `fusion_uuid`.
    pub guid_record: DesignMeshRecordIdentity,
    /// One-to-one `ParaMesh` wrapper around `body_record`.
    pub wrapper_record: DesignMeshRecordIdentity,
    /// Fixed Scene-state record owned by this mesh body.
    pub scene_state_record: DesignMeshRecordIdentity,
    /// Finite bound carried by the Scene-state footer; absent for its unset sentinel.
    pub scene_state_bounds: Option<DesignMeshSceneBounds>,
    /// Scene node connecting `body_record` to its state and auxiliary cache.
    pub scene_node_record: DesignMeshRecordIdentity,
    /// Finite bound carried by the Scene-node footer; absent for its unset sentinel.
    pub scene_node_bounds: Option<DesignMeshSceneBounds>,
    /// Optional row-major affine transform carried by the placed Scene-node form.
    pub scene_node_transform: Option<Located<[[f64; 4]; 4]>>,
    /// Separately typed Scene auxiliary cache reached through the Scene node.
    pub scene_auxiliary_record: DesignMeshRecordIdentity,
    /// Typed Design body-owner record referenced by `body_record`.
    /// Multiple mesh bodies can reference the same owner.
    pub owner_record: DesignMeshRecordIdentity,
    /// Stored `.paramesh` archive-entry basename.
    pub entry_name: String,
    /// Byte offset of the UTF-16LE entry-name code units.
    pub entry_name_offset: u64,
    /// Container identity stored by both Design and `.paramesh` payloads.
    pub fusion_uuid: String,
    /// Container-local version-4 mesh UUID from protobuf registry field 12,
    /// when the geometry container joined this Design body.
    pub container_mesh_uuid: Option<String>,
    /// Byte offset of the ASCII `fusion_uuid` payload.
    pub fusion_uuid_offset: u64,
    /// Equal row-major container-to-model-centimetre affine transform.
    pub transform: [[f64; 4]; 4],
    /// Byte offsets of the two equal serialized transform blocks.
    pub transform_offsets: [u64; 2],
    /// Byte offset of the body-to-feature-scope reference.
    pub scope_reference_offset: u64,
    /// Byte offset of the body-to-wrapper reference.
    pub wrapper_reference_offset: u64,
    /// Byte offset of the body-to-owner reference.
    pub owner_reference_offset: u64,
    /// Byte offset of the body-to-GUID reference.
    pub guid_reference_offset: u64,
    /// Byte offset of the body-to-Scene-node reference.
    pub scene_node_reference_offset: u64,
    /// Byte offset of the body's final collection backlink.
    pub collection_reference_offset: u64,
    /// Byte offset of the wrapper's reciprocal body reference.
    pub wrapper_body_reference_offset: u64,
    /// Byte offset of the entry-name record's GUID reference.
    pub entry_guid_reference_offset: u64,
    /// Byte offset of the GUID record's entry-name backlink.
    pub guid_entry_reference_offset: u64,
    /// Byte offset of the Scene node's state-record reference.
    pub scene_state_reference_offset: u64,
    /// Byte offset of the Scene node's auxiliary-record reference.
    pub scene_auxiliary_reference_offset: u64,
    /// Neutral tessellation projected from the joined container, when present.
    pub tessellation_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignMeshBodyWire {
    /// Mesh-body record carrying placement and graph references.
    body_record: DesignMeshRecordIdentity,
    /// Entry-name record joining the body to one `.paramesh` archive entry.
    entry_name_record: DesignMeshRecordIdentity,
    /// GUID record joining the body to the container's `fusion_uuid`.
    guid_record: DesignMeshRecordIdentity,
    /// One-to-one `ParaMesh` wrapper around `body_record`.
    wrapper_record: DesignMeshRecordIdentity,
    /// Fixed Scene-state record owned by this mesh body.
    scene_state_record: DesignMeshRecordIdentity,
    /// Finite bound carried by the Scene-state footer; absent for its unset sentinel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scene_state_bounds: Option<DesignMeshSceneBounds>,
    /// Scene node connecting `body_record` to its state and auxiliary cache.
    scene_node_record: DesignMeshRecordIdentity,
    /// Finite bound carried by the Scene-node footer; absent for its unset sentinel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scene_node_bounds: Option<DesignMeshSceneBounds>,
    /// Optional row-major affine transform carried by the placed Scene-node form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scene_node_transform: Option<[[f64; 4]; 4]>,
    /// Byte offset of `scene_node_transform` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scene_node_transform_offset: Option<u64>,
    /// Separately typed Scene auxiliary cache reached through the Scene node.
    scene_auxiliary_record: DesignMeshRecordIdentity,
    /// Typed Design body-owner record referenced by `body_record`.
    /// Multiple mesh bodies can reference the same owner.
    owner_record: DesignMeshRecordIdentity,
    /// Stored `.paramesh` archive-entry basename.
    entry_name: String,
    /// Byte offset of the UTF-16LE entry-name code units.
    entry_name_offset: u64,
    /// Container identity stored by both Design and `.paramesh` payloads.
    fusion_uuid: String,
    /// Container-local version-4 mesh UUID from protobuf registry field 12,
    /// when the geometry container joined this Design body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    container_mesh_uuid: Option<String>,
    /// Byte offset of the ASCII `fusion_uuid` payload.
    fusion_uuid_offset: u64,
    /// Equal row-major container-to-model-centimetre affine transform.
    transform: [[f64; 4]; 4],
    /// Byte offsets of the two equal serialized transform blocks.
    transform_offsets: [u64; 2],
    /// Byte offset of the body-to-feature-scope reference.
    scope_reference_offset: u64,
    /// Byte offset of the body-to-wrapper reference.
    wrapper_reference_offset: u64,
    /// Byte offset of the body-to-owner reference.
    owner_reference_offset: u64,
    /// Byte offset of the body-to-GUID reference.
    guid_reference_offset: u64,
    /// Byte offset of the body-to-Scene-node reference.
    scene_node_reference_offset: u64,
    /// Byte offset of the body's final collection backlink.
    collection_reference_offset: u64,
    /// Byte offset of the wrapper's reciprocal body reference.
    wrapper_body_reference_offset: u64,
    /// Byte offset of the entry-name record's GUID reference.
    entry_guid_reference_offset: u64,
    /// Byte offset of the GUID record's entry-name backlink.
    guid_entry_reference_offset: u64,
    /// Byte offset of the Scene node's state-record reference.
    scene_state_reference_offset: u64,
    /// Byte offset of the Scene node's auxiliary-record reference.
    scene_auxiliary_reference_offset: u64,
    /// Neutral tessellation projected from the joined container, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tessellation_id: Option<String>,
}

impl From<DesignMeshBody> for DesignMeshBodyWire {
    fn from(value: DesignMeshBody) -> Self {
        Self {
            body_record: value.body_record,
            entry_name_record: value.entry_name_record,
            guid_record: value.guid_record,
            wrapper_record: value.wrapper_record,
            scene_state_record: value.scene_state_record,
            scene_state_bounds: value.scene_state_bounds,
            scene_node_record: value.scene_node_record,
            scene_node_bounds: value.scene_node_bounds,
            scene_node_transform: value.scene_node_transform.map(|located| located.value),
            scene_node_transform_offset: value.scene_node_transform.map(|located| located.offset),
            scene_auxiliary_record: value.scene_auxiliary_record,
            owner_record: value.owner_record,
            entry_name: value.entry_name,
            entry_name_offset: value.entry_name_offset,
            fusion_uuid: value.fusion_uuid,
            container_mesh_uuid: value.container_mesh_uuid,
            fusion_uuid_offset: value.fusion_uuid_offset,
            transform: value.transform,
            transform_offsets: value.transform_offsets,
            scope_reference_offset: value.scope_reference_offset,
            wrapper_reference_offset: value.wrapper_reference_offset,
            owner_reference_offset: value.owner_reference_offset,
            guid_reference_offset: value.guid_reference_offset,
            scene_node_reference_offset: value.scene_node_reference_offset,
            collection_reference_offset: value.collection_reference_offset,
            wrapper_body_reference_offset: value.wrapper_body_reference_offset,
            entry_guid_reference_offset: value.entry_guid_reference_offset,
            guid_entry_reference_offset: value.guid_entry_reference_offset,
            scene_state_reference_offset: value.scene_state_reference_offset,
            scene_auxiliary_reference_offset: value.scene_auxiliary_reference_offset,
            tessellation_id: value.tessellation_id,
        }
    }
}

impl DesignMeshBody {
    fn from_wire(value: DesignMeshBodyWire, scope_body_reference_offset: u64, collection_body_reference_offset: u64) -> Result<Self, String> {
        Ok(Self {
            scope_body_reference_offset,
            collection_body_reference_offset,
            body_record: value.body_record,
            entry_name_record: value.entry_name_record,
            guid_record: value.guid_record,
            wrapper_record: value.wrapper_record,
            scene_state_record: value.scene_state_record,
            scene_state_bounds: value.scene_state_bounds,
            scene_node_record: value.scene_node_record,
            scene_node_bounds: value.scene_node_bounds,
            scene_node_transform: Located::from_wire(value.scene_node_transform, value.scene_node_transform_offset, "scene_node_transform")?,
            scene_auxiliary_record: value.scene_auxiliary_record,
            owner_record: value.owner_record,
            entry_name: value.entry_name,
            entry_name_offset: value.entry_name_offset,
            fusion_uuid: value.fusion_uuid,
            container_mesh_uuid: value.container_mesh_uuid,
            fusion_uuid_offset: value.fusion_uuid_offset,
            transform: value.transform,
            transform_offsets: value.transform_offsets,
            scope_reference_offset: value.scope_reference_offset,
            wrapper_reference_offset: value.wrapper_reference_offset,
            owner_reference_offset: value.owner_reference_offset,
            guid_reference_offset: value.guid_reference_offset,
            scene_node_reference_offset: value.scene_node_reference_offset,
            collection_reference_offset: value.collection_reference_offset,
            wrapper_body_reference_offset: value.wrapper_body_reference_offset,
            entry_guid_reference_offset: value.entry_guid_reference_offset,
            guid_entry_reference_offset: value.guid_entry_reference_offset,
            scene_state_reference_offset: value.scene_state_reference_offset,
            scene_auxiliary_reference_offset: value.scene_auxiliary_reference_offset,
            tessellation_id: value.tessellation_id,
        })
    }
}


/// One complete `Base Mesh Feature` Design graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "DesignMeshFeatureWire"))]
#[serde(try_from = "DesignMeshFeatureWire", into = "DesignMeshFeatureWire")]
pub struct DesignMeshFeature {
    /// Globally unique deterministic identity keyed by the feature-scope record.
    pub id: String,
    /// Typed `Base Mesh Feature` scope record.
    pub scope_record: DesignMeshRecordIdentity,
    /// Paired same-index base record closing the feature scope.
    pub scope_base_record: DesignMeshRecordIdentity,
    /// Typed `ParaMesh` body-collection record.
    pub collection_record: DesignMeshRecordIdentity,
    /// Paired same-index base record inside the collection.
    pub collection_base_record: DesignMeshRecordIdentity,
    /// Typed `ParaMesh` texture-table record owned by the collection.
    pub texture_table_record: DesignMeshRecordIdentity,
    /// Three equal body counts: scope, collection prefix, collection base.
    pub body_count_offsets: [u64; 3],
    /// Byte offset of the collection's texture-table reference.
    pub texture_table_reference_offset: u64,
    /// Typed Design owner of the mesh-body collection.
    pub collection_owner_record: DesignMeshRecordIdentity,
    /// Byte offset of the collection's owner reference.
    pub collection_owner_reference_offset: u64,
    /// Byte offset of the owner's reciprocal collection reference.
    pub collection_owner_backlink_offset: u64,
    /// Design owner of the feature scope.
    pub scope_owner_record_index: u32,
    /// Byte offset of the paired scope record's owner reference.
    pub scope_owner_reference_offset: u64,
    /// Byte offset of the texture flags-map count.
    pub texture_flags_count_offset: u64,
    /// Byte offset of the texture filename-map count.
    pub texture_filename_count_offset: u64,
    /// Mesh bodies in the source collection order.
    pub bodies: Vec<DesignMeshBody>,
    /// Texture resources in flags-map order.
    pub textures: Vec<DesignMeshTextureResource>,
}

/// One complete `Base Mesh Feature` Design graph.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DesignMeshFeatureWire {
    /// Globally unique deterministic identity keyed by the feature-scope record.
    id: String,
    /// Typed `Base Mesh Feature` scope record.
    scope_record: DesignMeshRecordIdentity,
    /// Paired same-index base record closing the feature scope.
    scope_base_record: DesignMeshRecordIdentity,
    /// Typed `ParaMesh` body-collection record.
    collection_record: DesignMeshRecordIdentity,
    /// Paired same-index base record inside the collection.
    collection_base_record: DesignMeshRecordIdentity,
    /// Typed `ParaMesh` texture-table record owned by the collection.
    texture_table_record: DesignMeshRecordIdentity,
    /// Three equal body counts: scope, collection prefix, collection base.
    body_count_offsets: [u64; 3],
    /// Ordered mesh-body record identities owned by the feature.
    body_record_indices: Vec<u32>,
    /// Scope body-reference offsets parallel to `body_record_indices`.
    scope_body_reference_offsets: Vec<u64>,
    /// Collection body-reference offsets parallel to `body_record_indices`.
    collection_body_reference_offsets: Vec<u64>,
    /// Byte offset of the collection's texture-table reference.
    texture_table_reference_offset: u64,
    /// Typed Design owner of the mesh-body collection.
    collection_owner_record: DesignMeshRecordIdentity,
    /// Byte offset of the collection's owner reference.
    collection_owner_reference_offset: u64,
    /// Byte offset of the owner's reciprocal collection reference.
    collection_owner_backlink_offset: u64,
    /// Design owner of the feature scope.
    scope_owner_record_index: u32,
    /// Byte offset of the paired scope record's owner reference.
    scope_owner_reference_offset: u64,
    /// Byte offset of the texture flags-map count.
    texture_flags_count_offset: u64,
    /// Byte offset of the texture filename-map count.
    texture_filename_count_offset: u64,
    /// Mesh bodies in the source collection order.
    bodies: Vec<DesignMeshBodyWire>,
    /// Texture resources in flags-map order.
    textures: Vec<DesignMeshTextureResource>,
}

impl TryFrom<DesignMeshFeatureWire> for DesignMeshFeature {
    type Error = String;
    fn try_from(wire: DesignMeshFeatureWire) -> Result<Self, Self::Error> {
        if wire.scope_body_reference_offsets.len() != wire.bodies.len() {
            return Err("scope_body_reference_offsets must match bodies".into());
        }
        if wire.collection_body_reference_offsets.len() != wire.bodies.len() {
            return Err("collection_body_reference_offsets must match bodies".into());
        }
        if !wire.body_record_indices.iter().copied().eq(wire.bodies.iter().map(|body| body.body_record.record_index)) {
            return Err("body_record_indices must repeat bodies.body_record.record_index in order".into());
        }
        let bodies = wire.bodies.into_iter().zip(wire.scope_body_reference_offsets).zip(wire.collection_body_reference_offsets)
            .map(|((body, scope_offset), collection_offset)| DesignMeshBody::from_wire(body, scope_offset, collection_offset))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            bodies,
            id: wire.id,
            scope_record: wire.scope_record,
            scope_base_record: wire.scope_base_record,
            collection_record: wire.collection_record,
            collection_base_record: wire.collection_base_record,
            texture_table_record: wire.texture_table_record,
            body_count_offsets: wire.body_count_offsets,
            texture_table_reference_offset: wire.texture_table_reference_offset,
            collection_owner_record: wire.collection_owner_record,
            collection_owner_reference_offset: wire.collection_owner_reference_offset,
            collection_owner_backlink_offset: wire.collection_owner_backlink_offset,
            scope_owner_record_index: wire.scope_owner_record_index,
            scope_owner_reference_offset: wire.scope_owner_reference_offset,
            texture_flags_count_offset: wire.texture_flags_count_offset,
            texture_filename_count_offset: wire.texture_filename_count_offset,
            textures: wire.textures,
        })
    }
}

impl From<DesignMeshFeature> for DesignMeshFeatureWire {
    fn from(value: DesignMeshFeature) -> Self {
        let mut body_record_indices = Vec::with_capacity(value.bodies.len());
        let mut scope_body_reference_offsets = Vec::with_capacity(value.bodies.len());
        let mut collection_body_reference_offsets = Vec::with_capacity(value.bodies.len());
        let mut bodies = Vec::with_capacity(value.bodies.len());
        for body in value.bodies {
            body_record_indices.push(body.body_record.record_index);
            scope_body_reference_offsets.push(body.scope_body_reference_offset);
            collection_body_reference_offsets.push(body.collection_body_reference_offset);
            bodies.push(body.into());
        }
        Self {
            body_record_indices,
            scope_body_reference_offsets,
            collection_body_reference_offsets,
            bodies,
            id: value.id,
            scope_record: value.scope_record,
            scope_base_record: value.scope_base_record,
            collection_record: value.collection_record,
            collection_base_record: value.collection_base_record,
            texture_table_record: value.texture_table_record,
            body_count_offsets: value.body_count_offsets,
            texture_table_reference_offset: value.texture_table_reference_offset,
            collection_owner_record: value.collection_owner_record,
            collection_owner_reference_offset: value.collection_owner_reference_offset,
            collection_owner_backlink_offset: value.collection_owner_backlink_offset,
            scope_owner_record_index: value.scope_owner_record_index,
            scope_owner_reference_offset: value.scope_owner_reference_offset,
            texture_flags_count_offset: value.texture_flags_count_offset,
            texture_filename_count_offset: value.texture_filename_count_offset,
            textures: value.textures,
        }
    }
}

/// Exact image-plane binding owned by one Design `Canvas` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignCanvasImage {
    /// Globally unique deterministic identifier for this native binding.
    pub id: String,
    /// Canvas scope record index.
    pub scope_record_index: u32,
    /// Byte offset of the marked scope reference in the geometry record.
    pub scope_reference_offset: u64,
    /// Dynamic class tag of the primary geometry record.
    pub geometry_class_tag: String,
    /// Geometry record index.
    pub geometry_record_index: u32,
    /// Byte offset of the scope's marked geometry-record reference.
    pub geometry_reference_offset: u64,
    /// Byte offset of the primary geometry record.
    pub geometry_byte_offset: u64,
    /// Fixed geometry prologue immediately following the primary record header.
    pub geometry_prologue: [u8; 15],
    /// Whether the Canvas raster is visible.
    pub visible: bool,
    /// Byte offset of the visibility byte in the geometry prologue.
    pub visibility_offset: u64,
    /// Byte length from the primary geometry header to its paired header.
    pub geometry_frame_length: u64,
    /// Dynamic class tag of the paired geometry record.
    pub paired_geometry_class_tag: String,
    /// Byte offset of the paired geometry record.
    pub paired_geometry_byte_offset: u64,
    /// Byte offset of the paired record's marked component reference.
    pub paired_component_reference_offset: u64,
    /// Two opposite boundary segments in plane-local coordinates.
    pub boundary_segments: [[Point2; 2]; 2],
    /// Byte offsets of the eight boundary-coordinate f64 values.
    pub boundary_coordinate_offsets: [u64; 8],
    /// Byte offset of the presence marker preceding the second boundary segment.
    pub second_boundary_present_offset: u64,
    /// Design entity suffix of the supporting construction plane.
    pub plane_entity_suffix: u32,
    /// Byte offset of the marked construction-plane reference.
    pub plane_reference_offset: u64,
    /// Design entity suffix of the component owning the Canvas.
    pub component_entity_suffix: u32,
    /// Byte offset of the marked component reference.
    pub component_reference_offset: u64,
    /// Dynamic class tag of the standalone image-asset record.
    pub asset_class_tag: String,
    /// Image-asset record index.
    pub asset_record_index: u32,
    /// Byte offset of the marked image-asset reference.
    pub asset_reference_offset: u64,
    /// Byte offset of the image-asset record.
    pub asset_byte_offset: u64,
    /// Archive entry basename stored by the image-asset record.
    pub asset_name: String,
    /// Byte offset of the asset name's UTF-16LE code units.
    pub asset_name_offset: u64,
    /// Persistent Canvas label stored after the boundary segments.
    pub label: String,
    /// Byte offset of the label's UTF-16LE code units.
    pub label_offset: u64,
    /// Normalized raster opacity.
    pub opacity: f32,
    /// Image-plane origin in model-space millimeters.
    pub origin: Point3,
    /// Unit direction of increasing image u coordinate.
    pub u_axis: Vector3,
    /// Unit direction of increasing image v coordinate.
    pub v_axis: Vector3,
    /// Uninterpreted fixed geometry payload between the plane reference and scope link.
    pub geometry_payload: Vec<u8>,
}

/// Exact image and target binding owned by one Design `Decal` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignDecalImage {
    /// Globally unique deterministic identifier for this native binding.
    pub id: String,
    /// Decal scope record index.
    pub scope_record_index: u32,
    /// Byte offset of the scope's marked image-asset reference.
    pub asset_reference_offset: u64,
    /// Source mapping-mode byte.
    pub mapping_mode: DesignDecalMappingMode,
    /// Byte offset of `mapping_mode`.
    pub mapping_mode_offset: u64,
    /// Target construction-group record index.
    pub target_group_record_index: u32,
    /// Byte offset of the scope's marked target-group reference.
    pub target_group_reference_offset: u64,
    /// Dynamic class tag of the primary image-asset record.
    pub asset_class_tag: String,
    /// Primary image-asset record index.
    pub asset_record_index: u32,
    /// Byte offset of the primary image-asset record.
    pub asset_byte_offset: u64,
    /// Byte length from the primary image header to the name-record header.
    pub asset_frame_length: u64,
    /// Design entity suffix carried by the primary image record.
    pub asset_entity_suffix: u32,
    /// Byte offset of the marked Design entity-suffix reference.
    pub asset_entity_reference_offset: u64,
    /// Dynamic class tag of the image-name record.
    pub name_class_tag: String,
    /// Image-name record index.
    pub name_record_index: u32,
    /// Byte offset of the image-name record.
    pub name_byte_offset: u64,
    /// Byte length of the complete image-name record.
    pub name_frame_length: u64,
    /// Archive entry basename stored by the image-name record.
    pub asset_name: String,
    /// Byte offset of the asset name's UTF-16LE code units.
    pub asset_name_offset: u64,
}

/// One indexed record header in the recursive Design `BulkStream` tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignRecordHeader {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Index of this record within the recursive `BulkStream` tree.
    pub record_index: u32,
    /// Source per-file dynamic three-digit ASCII class tag naming this record's type.
    pub class_tag: String,
    /// Byte offset of this header within its Design `BulkStream`.
    pub byte_offset: u64,
}

pub(crate) const SKETCH_CONSTRAINT_MASK: u64 = 0x0320_b000_3fff;

/// Decode the constraint kinds and unknown bits selected by a sketch-relation mask.
#[must_use]
pub(crate) fn constraint_kinds_from_state(state: u64) -> (Vec<SketchConstraintKind>, u64) {
    let definitions = [
        (0x0000_0001, SketchConstraintKind::Coincident),
        (0x0000_0002, SketchConstraintKind::Colinear),
        (0x0000_0004, SketchConstraintKind::Concentric),
        (0x0000_0008, SketchConstraintKind::EqualLength),
        (0x0000_0010, SketchConstraintKind::Parallel),
        (0x0000_0020, SketchConstraintKind::Perpendicular),
        (0x0000_0040, SketchConstraintKind::Horizontal),
        (0x0000_0080, SketchConstraintKind::Vertical),
        (0x0000_0100, SketchConstraintKind::Tangent),
        (0x0000_0200, SketchConstraintKind::Curvature),
        (0x0000_0400, SketchConstraintKind::Symmetry),
        (0x0000_0800, SketchConstraintKind::Equal),
        (0x0000_1000, SketchConstraintKind::Midpoint),
        (0x0000_2000, SketchConstraintKind::Polygon),
        (0x1000_0000, SketchConstraintKind::CircularPattern),
        (0x2000_0000, SketchConstraintKind::RectangularPattern),
        (0x8000_0000, SketchConstraintKind::SplineGroup),
        (0x20_0000_0000, SketchConstraintKind::Offset),
        (0x100_0000_0000, SketchConstraintKind::TextFrame),
        (0x200_0000_0000, SketchConstraintKind::TextPath),
    ];
    let mut kinds = if state == 0 {
        vec![SketchConstraintKind::Coincident]
    } else {
        Vec::new()
    };
    let mut recognized = 0u64;
    for (bit, kind) in definitions {
        if state & bit != 0 {
            kinds.push(kind);
            recognized |= bit;
        }
    }
    debug_assert_eq!(recognized, state & SKETCH_CONSTRAINT_MASK);
    (kinds, state & !SKETCH_CONSTRAINT_MASK)
}

/// One first-run sketch-relation member with its offset, ordinal, and resolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchRelationMember {
    /// Indexed Design record referenced by the relation.
    pub record_index: u32,
    /// Payload offset of the member, relative to the record.
    pub offset: u32,
    /// Count of relations already recorded on this member.
    pub relation_ordinal: u32,
    /// Typed sketch identity, when the member has been resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<SketchRelationOperand>,
}

impl SketchRelationMember {
    /// A member named only by record index, with zero offset and ordinal.
    #[must_use]
    pub fn from_index(record_index: u32) -> Self {
        Self {
            record_index,
            offset: 0,
            relation_ordinal: 0,
            resolved: None,
        }
    }
}

impl From<u32> for SketchRelationMember {
    fn from(record_index: u32) -> Self {
        Self::from_index(record_index)
    }
}

/// One return-run sketch-relation member with its offset and resolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchRelationReturnMember {
    /// Indexed Design record referenced by the relation.
    pub record_index: u32,
    /// Payload offset of the return member, relative to the record.
    pub offset: u32,
    /// Typed sketch identity, when the member has been resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<SketchRelationOperand>,
}

impl SketchRelationReturnMember {
    /// A return member named only by record index, with zero offset.
    #[must_use]
    pub fn from_index(record_index: u32) -> Self {
        Self {
            record_index,
            offset: 0,
            resolved: None,
        }
    }
}

impl From<u32> for SketchRelationReturnMember {
    fn from(record_index: u32) -> Self {
        Self::from_index(record_index)
    }
}

/// Pattern or text payload a sketch relation carries, when the mask names one.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum SketchRelationKind {
    /// No class-specific pattern or text payload.
    #[default]
    Unpatterned,
    /// A circular-pattern relation's auxiliary operands.
    Circular {
        /// Record index of the total-angle parameter value record.
        angle_parameter: u32,
        /// Record index of the instance-count parameter value record.
        count_parameter: u32,
        /// Evaluated total pattern angle in radians.
        evaluated_angle: f64,
        /// Evaluated instance count.
        evaluated_count: u32,
    },
    /// A rectangular-pattern relation's two direction clauses.
    Rectangular {
        /// The two pattern direction clauses in record order.
        directions: [SketchPatternDirection; 2],
    },
    /// A text-frame relation's auxiliary operand.
    TextFrame {
        /// Record index of the sketch-text entity the frame curves bind to.
        text_reference: u32,
    },
    /// A text-path relation's auxiliary operands.
    TextPath {
        /// Record index of the sketch-text entity placed along the path curve.
        text_reference: u32,
        /// Row-major 4×4 character placement transforms in character order,
        /// in centimetres.
        glyph_transforms: Vec<[[f64; 4]; 4]>,
    },
}

impl SketchRelationKind {
    pub(crate) fn from_pattern(pattern: Option<SketchPatternDefinition>) -> Self {
        match pattern {
            None => Self::Unpatterned,
            Some(SketchPatternDefinition::Circular {
                angle_parameter,
                count_parameter,
                evaluated_angle,
                evaluated_count,
            }) => Self::Circular {
                angle_parameter,
                count_parameter,
                evaluated_angle,
                evaluated_count,
            },
            Some(SketchPatternDefinition::Rectangular { directions }) => {
                Self::Rectangular { directions }
            }
            Some(SketchPatternDefinition::TextFrame { text_reference }) => {
                Self::TextFrame { text_reference }
            }
            Some(SketchPatternDefinition::TextPath {
                text_reference,
                glyph_transforms,
            }) => Self::TextPath {
                text_reference,
                glyph_transforms,
            },
        }
    }

    pub(crate) fn to_pattern(&self) -> Option<SketchPatternDefinition> {
        match self {
            Self::Unpatterned => None,
            Self::Circular {
                angle_parameter,
                count_parameter,
                evaluated_angle,
                evaluated_count,
            } => Some(SketchPatternDefinition::Circular {
                angle_parameter: *angle_parameter,
                count_parameter: *count_parameter,
                evaluated_angle: *evaluated_angle,
                evaluated_count: *evaluated_count,
            }),
            Self::Rectangular { directions } => Some(SketchPatternDefinition::Rectangular {
                directions: directions.clone(),
            }),
            Self::TextFrame { text_reference } => Some(SketchPatternDefinition::TextFrame {
                text_reference: *text_reference,
            }),
            Self::TextPath {
                text_reference,
                glyph_transforms,
            } => Some(SketchPatternDefinition::TextPath {
                text_reference: *text_reference,
                glyph_transforms: glyph_transforms.clone(),
            }),
        }
    }

    fn expected_constraint_kind(&self) -> Option<SketchConstraintKind> {
        match self {
            Self::Unpatterned => None,
            Self::Circular { .. } => Some(SketchConstraintKind::CircularPattern),
            Self::Rectangular { .. } => Some(SketchConstraintKind::RectangularPattern),
            Self::TextFrame { .. } => Some(SketchConstraintKind::TextFrame),
            Self::TextPath { .. } => Some(SketchConstraintKind::TextPath),
        }
    }

    pub(crate) fn agrees_with_state(&self, state: u64) -> bool {
        let (kinds, _) = constraint_kinds_from_state(state);
        let first = kinds.first().copied();
        match self.expected_constraint_kind() {
            None => !matches!(
                first,
                Some(
                    SketchConstraintKind::CircularPattern
                        | SketchConstraintKind::RectangularPattern
                        | SketchConstraintKind::TextFrame
                        | SketchConstraintKind::TextPath
                )
            ),
            Some(expected) => first == Some(expected),
        }
    }
}

/// Rejected CADIR sketch relation whose derived fields disagree with `state`.
#[derive(Debug)]
pub(crate) struct SketchRelationPayloadError(String);

impl std::fmt::Display for SketchRelationPayloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SketchRelationPayloadError {}

/// Counted constraint relation owned by a sketch container.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "SketchRelationSerde"))]
#[serde(try_from = "SketchRelationSerde", into = "SketchRelationSerde")]
pub struct SketchRelation {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Index of this relation record within the `BulkStream` tree.
    pub record_index: u32,
    /// Source per-file dynamic three-digit ASCII class tag naming this relation's type.
    pub class_tag: String,
    /// Byte offset of this record within its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of the constraint mask relative to the record start.
    pub state_offset: u32,
    /// Numeric design-entity suffix of the sketch container that owns this relation.
    pub owner_reference: u32,
    /// Full Design entity id resolved from `owner_reference`.
    #[serde(default)]
    pub owner_entity_id: String,
    /// Nullable or role-specific references stored before the owner reference.
    pub auxiliary_references: ReferenceRun<u32, u32>,
    /// Serialized count of the rectangular class's reference run. Zero selects
    /// seed-to-final spans; a nonzero count selects adjacent spacing. `None`
    /// for other relation classes and native data that did not retain it.
    pub rectangular_counted_reference_count: Option<u32>,
    /// First reference run, interleaved with per-member relation ordinals.
    /// Its order does not define relation operand order.
    pub members: Vec<SketchRelationMember>,
    /// Payload offset of `owner_reference`, relative to the record.
    pub owner_reference_offset: u32,
    /// Source sketch-constraint bitmask.
    pub state: u64,
    /// `EntityGenesis` origin bitfield stored by the relation record, when present.
    pub entity_genesis: Option<u64>,
    /// Pattern or text payload named by the constraint mask.
    pub kind: SketchRelationKind,
    /// Second reference run in semantic member order.
    pub return_members: Vec<SketchRelationReturnMember>,
    /// Complete variable-width source record for native replay/write.
    pub raw_bytes: Vec<u8>,
}

impl SketchRelation {
    /// Constraint kinds selected by `state`.
    #[must_use]
    pub fn constraint_kinds(&self) -> Vec<SketchConstraintKind> {
        constraint_kinds_from_state(self.state).0
    }

    /// Bits in `state` outside the defined constraint mask.
    #[must_use]
    pub fn unknown_constraint_bits(&self) -> u64 {
        constraint_kinds_from_state(self.state).1
    }

    /// Class-specific pattern or text payload, when the kind carries one.
    #[must_use]
    pub fn pattern(&self) -> Option<SketchPatternDefinition> {
        self.kind.to_pattern()
    }

    /// Record indices of the first reference run.
    #[must_use]
    pub fn member_indices(&self) -> Vec<u32> {
        self.members
            .iter()
            .map(|member| member.record_index)
            .collect()
    }

    /// Record indices of the return reference run.
    #[must_use]
    pub fn return_member_indices(&self) -> Vec<u32> {
        self.return_members
            .iter()
            .map(|member| member.record_index)
            .collect()
    }

    /// First-run then return-run record indices.
    pub fn all_member_indices(&self) -> impl Iterator<Item = u32> + '_ {
        self.members
            .iter()
            .map(|member| member.record_index)
            .chain(self.return_members.iter().map(|member| member.record_index))
    }

    /// Payload offsets parallel to the first reference run.
    #[must_use]
    pub fn member_offsets(&self) -> Vec<u32> {
        self.members.iter().map(|member| member.offset).collect()
    }

    /// Payload offsets parallel to the return reference run.
    #[must_use]
    pub fn return_member_offsets(&self) -> Vec<u32> {
        self.return_members
            .iter()
            .map(|member| member.offset)
            .collect()
    }

    /// Relation ordinals parallel to the first reference run.
    #[must_use]
    pub fn member_relation_ordinals(&self) -> Vec<u32> {
        self.members
            .iter()
            .map(|member| member.relation_ordinal)
            .collect()
    }

    /// Resolved first-run members, empty when none have been bound.
    #[must_use]
    pub fn resolved_members(&self) -> Vec<SketchRelationOperand> {
        if self.members.iter().any(|member| member.resolved.is_none()) {
            Vec::new()
        } else {
            self.members
                .iter()
                .filter_map(|member| member.resolved.clone())
                .collect()
        }
    }

    /// Resolved return-run members, empty when none have been bound.
    #[must_use]
    pub fn resolved_return_members(&self) -> Vec<SketchRelationOperand> {
        if self
            .return_members
            .iter()
            .any(|member| member.resolved.is_none())
        {
            Vec::new()
        } else {
            self.return_members
                .iter()
                .filter_map(|member| member.resolved.clone())
                .collect()
        }
    }
}

fn zip_relation_members(
    members: Vec<u32>,
    offsets: Vec<u32>,
    ordinals: Vec<u32>,
    resolved: Vec<SketchRelationOperand>,
) -> Result<Vec<SketchRelationMember>, SketchRelationPayloadError> {
    let len = members.len();
    let offsets = pad_or_check("member_offsets", offsets, len)?;
    let ordinals = pad_or_check("member_relation_ordinals", ordinals, len)?;
    let resolved = pad_resolved("resolved_members", resolved, len)?;
    Ok(members
        .into_iter()
        .zip(offsets)
        .zip(ordinals)
        .zip(resolved)
        .map(
            |(((record_index, offset), relation_ordinal), resolved)| SketchRelationMember {
                record_index,
                offset,
                relation_ordinal,
                resolved,
            },
        )
        .collect())
}

fn zip_return_members(
    members: Vec<u32>,
    offsets: Vec<u32>,
    resolved: Vec<SketchRelationOperand>,
) -> Result<Vec<SketchRelationReturnMember>, SketchRelationPayloadError> {
    let len = members.len();
    let offsets = pad_or_check("return_member_offsets", offsets, len)?;
    let resolved = pad_resolved("resolved_return_members", resolved, len)?;
    Ok(members
        .into_iter()
        .zip(offsets)
        .zip(resolved)
        .map(
            |((record_index, offset), resolved)| SketchRelationReturnMember {
                record_index,
                offset,
                resolved,
            },
        )
        .collect())
}

fn pad_or_check(
    name: &str,
    values: Vec<u32>,
    len: usize,
) -> Result<Vec<u32>, SketchRelationPayloadError> {
    if values.is_empty() {
        Ok(vec![0; len])
    } else if values.len() == len {
        Ok(values)
    } else {
        Err(SketchRelationPayloadError(format!(
            "sketch relation {name} length {} does not match members {len}",
            values.len()
        )))
    }
}

fn pad_resolved(
    name: &str,
    values: Vec<SketchRelationOperand>,
    len: usize,
) -> Result<Vec<Option<SketchRelationOperand>>, SketchRelationPayloadError> {
    if values.is_empty() {
        Ok(vec![None; len])
    } else if values.len() == len {
        Ok(values.into_iter().map(Some).collect())
    } else {
        Err(SketchRelationPayloadError(format!(
            "sketch relation {name} length {} does not match members {len}",
            values.len()
        )))
    }
}

/// Wire form of [`SketchRelation`] with the historical flat field set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SketchRelationSerde {
    pub id: String,
    pub record_index: u32,
    pub class_tag: String,
    pub byte_offset: u64,
    pub state_offset: u32,
    pub owner_reference: u32,
    #[serde(default)]
    pub owner_entity_id: String,
    #[serde(default)]
    pub auxiliary_references: Vec<u32>,
    #[serde(default)]
    pub auxiliary_reference_offsets: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rectangular_counted_reference_count: Option<u32>,
    pub members: Vec<u32>,
    #[serde(default)]
    pub resolved_members: Vec<SketchRelationOperand>,
    #[serde(default)]
    pub member_offsets: Vec<u32>,
    #[serde(default)]
    pub owner_reference_offset: u32,
    pub state: u64,
    #[serde(default)]
    pub constraint_kinds: Vec<SketchConstraintKind>,
    #[serde(default)]
    pub unknown_constraint_bits: u64,
    #[serde(default)]
    pub member_relation_ordinals: Vec<u32>,
    #[serde(default)]
    pub entity_genesis: Option<u64>,
    #[serde(default)]
    pub pattern: Option<SketchPatternDefinition>,
    pub return_members: Vec<u32>,
    #[serde(default)]
    pub resolved_return_members: Vec<SketchRelationOperand>,
    #[serde(default)]
    pub return_member_offsets: Vec<u32>,
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub raw_bytes: Vec<u8>,
}

impl TryFrom<SketchRelationSerde> for SketchRelation {
    type Error = SketchRelationPayloadError;

    fn try_from(wire: SketchRelationSerde) -> Result<Self, Self::Error> {
        let (derived_kinds, derived_unknown) = constraint_kinds_from_state(wire.state);
        if !wire.constraint_kinds.is_empty() && wire.constraint_kinds != derived_kinds {
            return Err(SketchRelationPayloadError(
                "sketch relation constraint_kinds disagrees with state".into(),
            ));
        }
        if wire.unknown_constraint_bits != 0 && wire.unknown_constraint_bits != derived_unknown {
            return Err(SketchRelationPayloadError(
                "sketch relation unknown_constraint_bits disagrees with state".into(),
            ));
        }
        let kind = SketchRelationKind::from_pattern(wire.pattern);
        if !kind.agrees_with_state(wire.state) {
            return Err(SketchRelationPayloadError(
                "sketch relation pattern disagrees with the first constraint kind".into(),
            ));
        }
        Ok(Self {
            id: wire.id,
            record_index: wire.record_index,
            class_tag: wire.class_tag,
            byte_offset: wire.byte_offset,
            state_offset: wire.state_offset,
            owner_reference: wire.owner_reference,
            owner_entity_id: wire.owner_entity_id,
            auxiliary_references: ReferenceRun::from_wire(wire.auxiliary_references, wire.auxiliary_reference_offsets, "auxiliary_reference").map_err(SketchRelationPayloadError)?,
            rectangular_counted_reference_count: wire.rectangular_counted_reference_count,
            members: zip_relation_members(
                wire.members,
                wire.member_offsets,
                wire.member_relation_ordinals,
                wire.resolved_members,
            )?,
            owner_reference_offset: wire.owner_reference_offset,
            state: wire.state,
            entity_genesis: wire.entity_genesis,
            kind,
            return_members: zip_return_members(
                wire.return_members,
                wire.return_member_offsets,
                wire.resolved_return_members,
            )?,
            raw_bytes: wire.raw_bytes,
        })
    }
}

impl From<SketchRelation> for SketchRelationSerde {
    fn from(relation: SketchRelation) -> Self {
        let (constraint_kinds, unknown_constraint_bits) =
            constraint_kinds_from_state(relation.state);
        let emit_resolved = relation
            .members
            .iter()
            .all(|member| member.resolved.is_some());
        let emit_return_resolved = relation
            .return_members
            .iter()
            .all(|member| member.resolved.is_some());
        let emit_ordinals = relation
            .members
            .iter()
            .any(|member| member.relation_ordinal != 0);
        let (auxiliary_references, auxiliary_reference_offsets) = relation.auxiliary_references.into_wire();
        Self {
            id: relation.id,
            record_index: relation.record_index,
            class_tag: relation.class_tag,
            byte_offset: relation.byte_offset,
            state_offset: relation.state_offset,
            owner_reference: relation.owner_reference,
            owner_entity_id: relation.owner_entity_id,
            auxiliary_references,
            auxiliary_reference_offsets,
            rectangular_counted_reference_count: relation.rectangular_counted_reference_count,
            members: relation
                .members
                .iter()
                .map(|member| member.record_index)
                .collect(),
            resolved_members: if emit_resolved {
                relation
                    .members
                    .iter()
                    .filter_map(|member| member.resolved.clone())
                    .collect()
            } else {
                Vec::new()
            },
            member_offsets: relation
                .members
                .iter()
                .map(|member| member.offset)
                .collect(),
            owner_reference_offset: relation.owner_reference_offset,
            state: relation.state,
            constraint_kinds,
            unknown_constraint_bits,
            member_relation_ordinals: if emit_ordinals {
                relation
                    .members
                    .iter()
                    .map(|member| member.relation_ordinal)
                    .collect()
            } else {
                Vec::new()
            },
            entity_genesis: relation.entity_genesis,
            pattern: relation.kind.to_pattern(),
            return_members: relation
                .return_members
                .iter()
                .map(|member| member.record_index)
                .collect(),
            resolved_return_members: if emit_return_resolved {
                relation
                    .return_members
                    .iter()
                    .filter_map(|member| member.resolved.clone())
                    .collect()
            } else {
                Vec::new()
            },
            return_member_offsets: relation
                .return_members
                .iter()
                .map(|member| member.offset)
                .collect(),
            raw_bytes: relation.raw_bytes,
        }
    }
}

/// One sketch-relation reference resolved against the indexed Design record graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchRelationOperand {
    /// A sketch point.
    Point {
        /// Indexed Design record referenced by the relation.
        record_index: u32,
        /// Persistent point identity stored by that record, when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        persistent_id: Option<u64>,
    },
    /// A persistent sketch curve.
    Curve {
        /// Indexed Design record referenced by the relation.
        record_index: u32,
        /// Primary persistent curve identity.
        primary_id: u64,
        /// Nullable secondary persistent curve identity.
        secondary_id: u64,
    },
    /// A persistent sketch surface.
    Surface {
        /// Indexed Design record referenced by the relation.
        record_index: u32,
        /// Persistent surface identity stored by that record.
        persistent_id: u64,
    },
    /// A referenced indexed record without point, curve, or surface identity fields.
    Record {
        /// Indexed Design record referenced by the relation.
        record_index: u32,
    },
}

/// One bit in a Fusion sketch-constraint state mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SketchConstraintKind {
    /// Points or endpoints occupy the same position.
    Coincident,
    /// Two line-bearing entities lie on one infinite line.
    Colinear,
    /// Circular entities share a center.
    Concentric,
    /// Line-bearing entities have equal length.
    EqualLength,
    /// Line-bearing entities have parallel directions.
    Parallel,
    /// Line-bearing entities meet at a right angle.
    Perpendicular,
    /// An entity is horizontal in sketch coordinates.
    Horizontal,
    /// An entity is vertical in sketch coordinates.
    Vertical,
    /// Two entities share a tangent direction at contact.
    Tangent,
    /// Two entities share curvature at contact.
    Curvature,
    /// Entities are symmetric about an axis.
    Symmetry,
    /// Entities have equal size.
    Equal,
    /// A point lies at an entity midpoint.
    Midpoint,
    /// Entities participate in a polygon relation.
    Polygon,
    /// Result entities are offset from oriented source entities by one magnitude.
    Offset,
    /// A spline's defining entities grouped under the owning sketch.
    SplineGroup,
    /// Entities participate in a circular pattern.
    CircularPattern,
    /// Entities participate in a rectangular pattern.
    RectangularPattern,
    /// Frame curves bound to a sketch-text entity.
    TextFrame,
    /// A sketch-text entity bound to a path curve.
    TextPath,
}

/// Class-specific auxiliary payload of a pattern or text sketch relation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchPatternDefinition {
    /// A circular-pattern relation's auxiliary operands.
    Circular {
        /// Record index of the total-angle parameter value record.
        angle_parameter: u32,
        /// Record index of the instance-count parameter value record.
        count_parameter: u32,
        /// Evaluated total pattern angle in radians.
        evaluated_angle: f64,
        /// Evaluated instance count.
        evaluated_count: u32,
    },
    /// A rectangular-pattern relation's two direction clauses.
    Rectangular {
        /// The two pattern direction clauses in record order.
        directions: [SketchPatternDirection; 2],
    },
    /// A text-frame relation's auxiliary operand.
    TextFrame {
        /// Record index of the sketch-text entity the frame curves bind to.
        text_reference: u32,
    },
    /// A text-path relation's auxiliary operands.
    TextPath {
        /// Record index of the sketch-text entity placed along the path curve.
        text_reference: u32,
        /// Row-major 4×4 character placement transforms in character order,
        /// in centimetres.
        glyph_transforms: Vec<[[f64; 4]; 4]>,
    },
}

/// One direction clause of a rectangular-pattern sketch relation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchPatternDirection {
    /// Evaluated instance count along this direction.
    pub evaluated_count: u32,
    /// Record index of the count parameter value record.
    pub count_parameter: u32,
    /// Unit direction vector in sketch coordinates.
    pub direction: [f64; 3],
    /// Evaluated source distance along this direction, in source units. The
    /// owning relation's [`SketchRelation::rectangular_counted_reference_count`]
    /// gives its meaning.
    pub evaluated_distance: f64,
    /// Record index of the distance parameter value record.
    pub distance_parameter: u32,
}

/// One text entity in a Fusion sketch coordinate system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "SketchTextSerde"))]
#[serde(try_from = "SketchTextSerde", into = "SketchTextSerde")]
pub struct SketchText {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Index of this text record within the `BulkStream` tree.
    pub record_index: u32,
    /// Owning sketch record index.
    pub owner_reference: u32,
    /// Source per-file dynamic ASCII class tag naming this record's type.
    pub class_tag: String,
    /// Record version of this record's class, from its Design `MetaStream` type
    /// table. It selects the member sequence the record was written under.
    pub class_version: u32,
    /// Byte offset of this record within its Design `BulkStream`.
    pub byte_offset: u64,
    /// Optional `EntityGenesis` origin bitfield.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_genesis: Option<u64>,
    /// Persistent identity of the text entity. A `txt_tag` record below class
    /// version 4 writes no identity key and stores none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistent_id: Option<u64>,
    /// Persistent base identity, a property key absent from some records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_id: Option<u64>,
    /// Unicode text content.
    pub text: String,
    /// Font-family name.
    pub font_family: String,
    /// Numeric font weight stored by the sketch-text class.
    pub font_weight: i32,
    /// Nominal text height in millimetres.
    pub height: f64,
    /// Display colour of the glyphs. Both identity forms store it, so it is
    /// never absent. `SketchGeometry` carries no display attribute on any
    /// variant, so the colour stays on the native record.
    pub color: Color,
    /// Identity-form layout: `txt_tag` placement or `textex_tag` width, alignment,
    /// and parameter references.
    pub layout: SketchTextLayout,
    /// Complete source record bytes for native replay and rewrite.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub raw_bytes: Vec<u8>,
}

/// `txt_tag` versus `textex_tag` member layout of one sketch-text record.
#[derive(Debug, Clone, PartialEq)]
pub enum SketchTextLayout {
    TxtTag {
        anchor: Point2,
        rotation: f64,
    },
    TextexTag {
        width_factor: f64,
        horizontal_alignment: Option<u32>,
        vertical_alignment: Option<u32>,
        first_reference: Option<u32>,
        second_reference: Option<u32>,
        anchor: Option<Point2>,
        rotation: Option<f64>,
    },
}

impl SketchText {
    pub(crate) fn width_factor(&self) -> Option<f64> {
        match self.layout {
            SketchTextLayout::TxtTag { .. } => None,
            SketchTextLayout::TextexTag { width_factor, .. } => Some(width_factor),
        }
    }

    pub(crate) fn anchor(&self) -> Option<Point2> {
        match self.layout {
            SketchTextLayout::TxtTag { anchor, .. } => Some(anchor),
            SketchTextLayout::TextexTag { anchor, .. } => anchor,
        }
    }

    pub(crate) fn rotation(&self) -> Option<f64> {
        match self.layout {
            SketchTextLayout::TxtTag { rotation, .. } => Some(rotation),
            SketchTextLayout::TextexTag { rotation, .. } => rotation,
        }
    }

    pub(crate) fn horizontal_alignment(&self) -> Option<u32> {
        match self.layout {
            SketchTextLayout::TxtTag { .. } => None,
            SketchTextLayout::TextexTag {
                horizontal_alignment,
                ..
            } => horizontal_alignment,
        }
    }

    pub(crate) fn vertical_alignment(&self) -> Option<u32> {
        match self.layout {
            SketchTextLayout::TxtTag { .. } => None,
            SketchTextLayout::TextexTag {
                vertical_alignment, ..
            } => vertical_alignment,
        }
    }

    pub(crate) fn first_reference(&self) -> Option<u32> {
        match self.layout {
            SketchTextLayout::TxtTag { .. } => None,
            SketchTextLayout::TextexTag {
                first_reference, ..
            } => first_reference,
        }
    }

    pub(crate) fn second_reference(&self) -> Option<u32> {
        match self.layout {
            SketchTextLayout::TxtTag { .. } => None,
            SketchTextLayout::TextexTag {
                second_reference, ..
            } => second_reference,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SketchTextSerde {
    id: String,
    record_index: u32,
    owner_reference: u32,
    class_tag: String,
    class_version: u32,
    byte_offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    entity_genesis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    persistent_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base_id: Option<u64>,
    text: String,
    font_family: String,
    font_weight: i32,
    height: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    width_factor: Option<f64>,
    color: Color,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    anchor: Option<Point2>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rotation: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    horizontal_alignment: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vertical_alignment: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    first_reference: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    second_reference: Option<u32>,
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    raw_bytes: Vec<u8>,
}

impl TryFrom<SketchTextSerde> for SketchText {
    type Error = String;

    fn try_from(wire: SketchTextSerde) -> Result<Self, Self::Error> {
        let layout = match (
            wire.width_factor,
            wire.horizontal_alignment,
            wire.vertical_alignment,
            wire.first_reference,
            wire.second_reference,
            wire.anchor,
            wire.rotation,
        ) {
            (None, None, None, None, None, Some(anchor), Some(rotation)) => {
                SketchTextLayout::TxtTag { anchor, rotation }
            }
            (
                Some(width_factor),
                horizontal_alignment,
                vertical_alignment,
                first_reference,
                second_reference,
                anchor,
                rotation,
            ) => SketchTextLayout::TextexTag {
                width_factor,
                horizontal_alignment,
                vertical_alignment,
                first_reference,
                second_reference,
                anchor,
                rotation,
            },
            _ => {
                return Err(
                    "sketch text layout disagrees with width_factor, alignment, and placement"
                        .into(),
                );
            }
        };
        Ok(Self {
            id: wire.id,
            record_index: wire.record_index,
            owner_reference: wire.owner_reference,
            class_tag: wire.class_tag,
            class_version: wire.class_version,
            byte_offset: wire.byte_offset,
            entity_genesis: wire.entity_genesis,
            persistent_id: wire.persistent_id,
            base_id: wire.base_id,
            text: wire.text,
            font_family: wire.font_family,
            font_weight: wire.font_weight,
            height: wire.height,
            color: wire.color,
            layout,
            raw_bytes: wire.raw_bytes,
        })
    }
}

impl From<SketchText> for SketchTextSerde {
    fn from(text: SketchText) -> Self {
        let width_factor = text.width_factor();
        let anchor = text.anchor();
        let rotation = text.rotation();
        let horizontal_alignment = text.horizontal_alignment();
        let vertical_alignment = text.vertical_alignment();
        let first_reference = text.first_reference();
        let second_reference = text.second_reference();
        Self {
            id: text.id,
            record_index: text.record_index,
            owner_reference: text.owner_reference,
            class_tag: text.class_tag,
            class_version: text.class_version,
            byte_offset: text.byte_offset,
            entity_genesis: text.entity_genesis,
            persistent_id: text.persistent_id,
            base_id: text.base_id,
            text: text.text,
            font_family: text.font_family,
            font_weight: text.font_weight,
            height: text.height,
            width_factor,
            color: text.color,
            anchor,
            rotation,
            horizontal_alignment,
            vertical_alignment,
            first_reference,
            second_reference,
            raw_bytes: text.raw_bytes,
        }
    }
}

/// Selector and state following a three-coordinate sketch point payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SketchPointClosure {
    Selector0State0,
    Selector0State1,
    Selector1State0,
    Selector2State1,
    Selector4State0,
}

impl SketchPointClosure {
    pub(crate) fn selector(self) -> u64 {
        match self {
            Self::Selector0State0 | Self::Selector0State1 => 0,
            Self::Selector1State0 => 1,
            Self::Selector2State1 => 2,
            Self::Selector4State0 => 4,
        }
    }

    pub(crate) fn state(self) -> u8 {
        match self {
            Self::Selector0State0 | Self::Selector1State0 | Self::Selector4State0 => 0,
            Self::Selector0State1 | Self::Selector2State1 => 1,
        }
    }

    pub(crate) fn from_pair(selector: u64, state: u8) -> Option<Self> {
        match (selector, state) {
            (0, 0) => Some(Self::Selector0State0),
            (0, 1) => Some(Self::Selector0State1),
            (1, 0) => Some(Self::Selector1State0),
            (2, 1) => Some(Self::Selector2State1),
            (4, 0) => Some(Self::Selector4State0),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SketchPointClosureSerde {
    selector: u64,
    state: u8,
}

impl TryFrom<SketchPointClosureSerde> for SketchPointClosure {
    type Error = String;

    fn try_from(wire: SketchPointClosureSerde) -> Result<Self, Self::Error> {
        Self::from_pair(wire.selector, wire.state).ok_or_else(|| {
            format!(
                "sketch point closure selector {} state {} is not an admitted pair",
                wire.selector, wire.state
            )
        })
    }
}

impl From<SketchPointClosure> for SketchPointClosureSerde {
    fn from(closure: SketchPointClosure) -> Self {
        Self {
            selector: closure.selector(),
            state: closure.state(),
        }
    }
}

/// Version-10 same-segment closure: selector `0` and state `0` or `1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SketchPointClosure10 {
    State0,
    State1,
}

impl SketchPointClosure10 {
    pub(crate) fn from_closure(closure: SketchPointClosure) -> Option<Self> {
        match closure {
            SketchPointClosure::Selector0State0 => Some(Self::State0),
            SketchPointClosure::Selector0State1 => Some(Self::State1),
            _ => None,
        }
    }

    fn to_closure(self) -> SketchPointClosure {
        match self {
            Self::State0 => SketchPointClosure::Selector0State0,
            Self::State1 => SketchPointClosure::Selector0State1,
        }
    }
}

/// Version-10 inline-typed closure: `(0, 0)`, `(0, 1)`, or `(2, 1)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SketchPointClosure10Inline {
    Selector0State0,
    Selector0State1,
    Selector2State1,
}

impl SketchPointClosure10Inline {
    pub(crate) fn from_closure(closure: SketchPointClosure) -> Option<Self> {
        match closure {
            SketchPointClosure::Selector0State0 => Some(Self::Selector0State0),
            SketchPointClosure::Selector0State1 => Some(Self::Selector0State1),
            SketchPointClosure::Selector2State1 => Some(Self::Selector2State1),
            _ => None,
        }
    }

    fn to_closure(self) -> SketchPointClosure {
        match self {
            Self::Selector0State0 => SketchPointClosure::Selector0State0,
            Self::Selector0State1 => SketchPointClosure::Selector0State1,
            Self::Selector2State1 => SketchPointClosure::Selector2State1,
        }
    }
}

/// Serialized member sequence of one sketch-point record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SketchPointRecordForm {
    /// Class version 0: one flag, two coordinates, and no persistent identity.
    Version0 { flag: u8 },
    /// Class version 8: seven flags and an eight-zero closure lane.
    Version8 { persistent_id: u64, flags: [u8; 7] },
    /// Class version 10 with same-segment references and seven flags.
    Version10 {
        persistent_id: u64,
        flags: [u8; 7],
        closure: SketchPointClosure10,
    },
    /// Class version 10 with inline target-type GUIDs on its references.
    Version10InlineTyped {
        /// Final inline-typed reference following the repeated companion reference.
        trailing_reference: u32,
        persistent_id: u64,
        flags: [u8; 7],
        closure: SketchPointClosure10Inline,
    },
    /// Class version 11 with same-segment references and eight flags.
    Version11 {
        /// Whether four fixed zero bytes follow the repeated companion reference.
        padded_paired_reference: bool,
        persistent_id: u64,
        flags: [u8; 8],
        closure: SketchPointClosure,
    },
    /// Class version 11 with inline target-type GUIDs on its references and eight flags.
    Version11InlineTyped {
        /// Final inline-typed reference following the repeated companion reference.
        trailing_reference: u32,
        persistent_id: u64,
        flags: [u8; 8],
        closure: SketchPointClosure,
    },
}

impl Default for SketchPointRecordForm {
    fn default() -> Self {
        Self::version11(0, SketchPointClosure::Selector0State0)
    }
}

impl SketchPointRecordForm {
    pub(crate) fn version11(persistent_id: u64, closure: SketchPointClosure) -> Self {
        Self::Version11 {
            padded_paired_reference: false,
            persistent_id,
            flags: [0; 8],
            closure,
        }
    }
}

impl SketchPointRecordForm {
    pub(crate) fn class_version(&self) -> u32 {
        match self {
            Self::Version0 { .. } => 0,
            Self::Version8 { .. } => 8,
            Self::Version10 { .. } | Self::Version10InlineTyped { .. } => 10,
            Self::Version11 { .. } | Self::Version11InlineTyped { .. } => 11,
        }
    }

    pub(crate) fn uses_inline_typed_references(&self) -> bool {
        matches!(
            self,
            Self::Version10InlineTyped { .. } | Self::Version11InlineTyped { .. }
        )
    }

    pub(crate) fn persistent_id(&self) -> Option<u64> {
        match *self {
            Self::Version0 { .. } => None,
            Self::Version8 { persistent_id, .. }
            | Self::Version10 { persistent_id, .. }
            | Self::Version10InlineTyped { persistent_id, .. }
            | Self::Version11 { persistent_id, .. }
            | Self::Version11InlineTyped { persistent_id, .. } => Some(persistent_id),
        }
    }

    pub(crate) fn set_persistent_id(&mut self, id: u64) {
        match self {
            Self::Version0 { .. } => {}
            Self::Version8 { persistent_id, .. }
            | Self::Version10 { persistent_id, .. }
            | Self::Version10InlineTyped { persistent_id, .. }
            | Self::Version11 { persistent_id, .. }
            | Self::Version11InlineTyped { persistent_id, .. } => *persistent_id = id,
        }
    }

    pub(crate) fn flags(&self) -> [u8; 8] {
        let mut flags = [0; 8];
        match self {
            Self::Version0 { flag } => flags[0] = *flag,
            Self::Version8 { flags: source, .. }
            | Self::Version10 { flags: source, .. }
            | Self::Version10InlineTyped { flags: source, .. } => {
                flags[..7].copy_from_slice(source);
            }
            Self::Version11 { flags: source, .. }
            | Self::Version11InlineTyped { flags: source, .. } => flags = *source,
        }
        flags
    }

    pub(crate) fn set_flags(&mut self, flags: [u8; 8]) {
        match self {
            Self::Version0 { flag } => *flag = flags[0],
            Self::Version8 { flags: dest, .. }
            | Self::Version10 { flags: dest, .. }
            | Self::Version10InlineTyped { flags: dest, .. } => {
                dest.copy_from_slice(&flags[..7]);
            }
            Self::Version11 { flags: dest, .. }
            | Self::Version11InlineTyped { flags: dest, .. } => *dest = flags,
        }
    }

    pub(crate) fn closure(&self) -> Option<SketchPointClosure> {
        match *self {
            Self::Version0 { .. } => None,
            Self::Version8 { .. } => Some(SketchPointClosure::Selector0State0),
            Self::Version10 { closure, .. } => Some(closure.to_closure()),
            Self::Version10InlineTyped { closure, .. } => Some(closure.to_closure()),
            Self::Version11 { closure, .. } | Self::Version11InlineTyped { closure, .. } => {
                Some(closure)
            }
        }
    }
}

/// Encoding of every reference owned by a point companion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SketchPointCompanionReferenceEncoding {
    /// Target entity ID followed directly by the same-segment flags.
    #[default]
    SameSegment,
    /// Target entity ID followed by the target type GUID and same-segment flags.
    InlineTyped,
}

/// Reverse curve-incidence record paired with one sketch point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchPointCompanion {
    /// Whether the companion prefix carries the fixed present-zero member.
    pub prefix_present_zero: bool,
    /// Encoding shared by every incident reference and the inverse point reference.
    #[serde(default)]
    pub reference_encoding: SketchPointCompanionReferenceEncoding,
    /// Incident sketch-curve record indexes in serialized order.
    #[serde(default)]
    pub incident_curves: Vec<u32>,
}

// Serde requires `skip_serializing_if` predicates to borrow the field.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn sketch_point_flags_are_zero(flags: &[u8; 8]) -> bool {
    flags.iter().all(|flag| *flag == 0)
}

/// One point in a Fusion sketch coordinate system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "SketchPointSerde"))]
#[serde(try_from = "SketchPointSerde", into = "SketchPointSerde")]
pub struct SketchPoint {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Index of this point record within the `BulkStream` tree.
    pub record_index: u32,
    /// Resolved owning-sketch reference from a direct backlink, typed relation,
    /// or sketch-container member run.
    pub owner_reference: Option<u32>,
    /// Source per-file dynamic three-digit ASCII class tag naming this point's record type.
    pub class_tag: String,
    /// Byte offset of this record within its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of the first coordinate relative to the record start.
    pub coordinate_offset: u32,
    /// Optional `EntityGenesis` origin bitfield carried ahead of the point identity.
    pub entity_genesis: Option<u64>,
    /// Serialized point-record member sequence, identity, flags, and closure.
    pub record_form: SketchPointRecordForm,
    /// Record index of the paired reverse curve-incidence companion.
    pub paired_reference: u32,
    /// First two sketch coordinates in millimetres.
    pub coordinates: Point2,
    /// Third sketch coordinate in millimetres.
    pub depth: f64,
    /// Typed reverse curve-incidence record named by `paired_reference`.
    pub companion: Option<SketchPointCompanion>,
}

impl SketchPoint {
    pub(crate) fn persistent_id(&self) -> Option<u64> {
        self.record_form.persistent_id()
    }

    pub(crate) fn set_persistent_id(&mut self, id: Option<u64>) {
        if let Some(id) = id {
            self.record_form.set_persistent_id(id);
        }
    }

    pub(crate) fn flags(&self) -> [u8; 8] {
        self.record_form.flags()
    }

    pub(crate) fn set_flags(&mut self, flags: [u8; 8]) {
        self.record_form.set_flags(flags);
    }

    pub(crate) fn closure(&self) -> Option<SketchPointClosure> {
        self.record_form.closure()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SketchPointRecordFormSerde {
    Version0,
    Version8,
    Version10,
    Version10InlineTyped { trailing_reference: u32 },
    Version11 { padded_paired_reference: bool },
    Version11InlineTyped { trailing_reference: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SketchPointSerde {
    id: String,
    record_index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner_reference: Option<u32>,
    class_tag: String,
    byte_offset: u64,
    coordinate_offset: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    entity_genesis: Option<u64>,
    #[serde(default)]
    record_form: SketchPointRecordFormSerde,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    persistent_id: Option<u64>,
    paired_reference: u32,
    #[serde(default, skip_serializing_if = "sketch_point_flags_are_zero")]
    flags: [u8; 8],
    coordinates: Point2,
    #[serde(default)]
    depth: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    closure: Option<SketchPointClosureSerde>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    companion: Option<SketchPointCompanion>,
}

impl Default for SketchPointRecordFormSerde {
    fn default() -> Self {
        Self::Version11 {
            padded_paired_reference: false,
        }
    }
}

fn seven_flags(flags: [u8; 8]) -> Result<[u8; 7], String> {
    if flags[7] != 0 {
        return Err("sketch point flags beyond the form width must be zero".into());
    }
    let mut dest = [0; 7];
    dest.copy_from_slice(&flags[..7]);
    Ok(dest)
}

impl TryFrom<SketchPointSerde> for SketchPoint {
    type Error = String;

    fn try_from(wire: SketchPointSerde) -> Result<Self, Self::Error> {
        if wire.flags.iter().any(|flag| *flag > 1) {
            return Err("sketch point flags must be zero or one".into());
        }
        let closure = wire.closure.map(SketchPointClosure::try_from).transpose()?;
        let record_form = match (wire.record_form, wire.persistent_id, closure) {
            (SketchPointRecordFormSerde::Version0, None, None) => SketchPointRecordForm::Version0 {
                flag: wire.flags[0],
            },
            (
                SketchPointRecordFormSerde::Version8,
                Some(persistent_id),
                Some(SketchPointClosure::Selector0State0),
            ) => SketchPointRecordForm::Version8 {
                persistent_id,
                flags: seven_flags(wire.flags)?,
            },
            (SketchPointRecordFormSerde::Version10, Some(persistent_id), Some(closure)) => {
                SketchPointRecordForm::Version10 {
                    persistent_id,
                    flags: seven_flags(wire.flags)?,
                    closure: SketchPointClosure10::from_closure(closure).ok_or_else(|| {
                        "sketch point version-10 closure must be selector 0 with state 0 or 1"
                            .to_string()
                    })?,
                }
            }
            (
                SketchPointRecordFormSerde::Version10InlineTyped { trailing_reference },
                Some(persistent_id),
                Some(closure),
            ) => SketchPointRecordForm::Version10InlineTyped {
                trailing_reference,
                persistent_id,
                flags: seven_flags(wire.flags)?,
                closure: SketchPointClosure10Inline::from_closure(closure).ok_or_else(|| {
                    "sketch point version-10 inline closure must be (0,0), (0,1), or (2,1)"
                        .to_string()
                })?,
            },
            (
                SketchPointRecordFormSerde::Version11 {
                    padded_paired_reference,
                },
                Some(persistent_id),
                Some(closure),
            ) => SketchPointRecordForm::Version11 {
                padded_paired_reference,
                persistent_id,
                flags: wire.flags,
                closure,
            },
            (
                SketchPointRecordFormSerde::Version11InlineTyped { trailing_reference },
                Some(persistent_id),
                Some(closure),
            ) => SketchPointRecordForm::Version11InlineTyped {
                trailing_reference,
                persistent_id,
                flags: wire.flags,
                closure,
            },
            _ => {
                return Err(
                    "sketch point record_form disagrees with persistent_id or closure".into(),
                );
            }
        };
        if matches!(record_form, SketchPointRecordForm::Version0 { .. })
            && wire.flags[1..].iter().any(|flag| *flag != 0)
        {
            return Err("sketch point flags beyond the form width must be zero".into());
        }
        Ok(Self {
            id: wire.id,
            record_index: wire.record_index,
            owner_reference: wire.owner_reference,
            class_tag: wire.class_tag,
            byte_offset: wire.byte_offset,
            coordinate_offset: wire.coordinate_offset,
            entity_genesis: wire.entity_genesis,
            record_form,
            paired_reference: wire.paired_reference,
            coordinates: wire.coordinates,
            depth: wire.depth,
            companion: wire.companion,
        })
    }
}

impl From<SketchPoint> for SketchPointSerde {
    fn from(point: SketchPoint) -> Self {
        let persistent_id = point.persistent_id();
        let flags = point.flags();
        let closure = point.closure().map(SketchPointClosureSerde::from);
        let record_form = match point.record_form {
            SketchPointRecordForm::Version0 { .. } => SketchPointRecordFormSerde::Version0,
            SketchPointRecordForm::Version8 { .. } => SketchPointRecordFormSerde::Version8,
            SketchPointRecordForm::Version10 { .. } => SketchPointRecordFormSerde::Version10,
            SketchPointRecordForm::Version10InlineTyped {
                trailing_reference, ..
            } => SketchPointRecordFormSerde::Version10InlineTyped { trailing_reference },
            SketchPointRecordForm::Version11 {
                padded_paired_reference,
                ..
            } => SketchPointRecordFormSerde::Version11 {
                padded_paired_reference,
            },
            SketchPointRecordForm::Version11InlineTyped {
                trailing_reference, ..
            } => SketchPointRecordFormSerde::Version11InlineTyped { trailing_reference },
        };
        Self {
            id: point.id,
            record_index: point.record_index,
            owner_reference: point.owner_reference,
            class_tag: point.class_tag,
            byte_offset: point.byte_offset,
            coordinate_offset: point.coordinate_offset,
            entity_genesis: point.entity_genesis,
            record_form,
            persistent_id,
            paired_reference: point.paired_reference,
            flags,
            coordinates: point.coordinates,
            depth: point.depth,
            closure,
            companion: point.companion,
        }
    }
}

/// Persistent identity pair attached to one source sketch-curve record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchCurveIdentity {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Index of this identity record within the `BulkStream` tree.
    pub record_index: u32,
    /// Direct owning-sketch backlink when the curve record form carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_reference: Option<u32>,
    /// Source per-file dynamic three-digit ASCII class tag naming this record's type.
    pub class_tag: String,
    /// Byte offset of this record within its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of the fixed analytic geometry payload relative to the record start.
    pub geometry_offset: u32,
    /// Optional `EntityGenesis` origin bitfield carried ahead of the curve identities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_genesis: Option<u64>,
    /// Primary persistent identifier of the source sketch curve.
    pub primary_id: u64,
    /// Secondary persistent identifier of the source sketch curve (e.g. its
    /// complementary endpoint or paired-curve identity).
    pub secondary_id: u64,
    /// Exact analytic geometry carried by this sketch-curve record, when the
    /// decoder recovered one; `None` when the geometry subtype was not decoded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<SketchCurveGeometry>,
}

/// One persistent tensor-product surface owned by a spatial Fusion sketch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchSurface {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Index of this surface record within the `BulkStream` tree.
    pub record_index: u32,
    /// Owning sketch entity derived from relations using this surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_reference: Option<u32>,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: String,
    /// Byte offset of this record within its Design `BulkStream`.
    pub byte_offset: u64,
    /// Optional `EntityGenesis` origin bitfield carried ahead of the surface identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_genesis: Option<u64>,
    /// Persistent Fusion identifier for the sketch surface.
    pub persistent_id: u64,
    /// Degree in the first surface parameter.
    pub u_degree: u32,
    /// Degree in the second surface parameter.
    pub v_degree: u32,
    /// Full knot vector in the first parameter.
    pub u_knots: Vec<f64>,
    /// Full knot vector in the second parameter.
    pub v_knots: Vec<f64>,
    /// Rectangular control grid in first-parameter-major order, in millimetres.
    pub control_points: Vec<Vec<Point3>>,
}

/// Exact analytic geometry carried by a source sketch-curve record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchCurveGeometry {
    /// A straight line segment.
    Line {
        /// Start point in sketch space, millimetres.
        start: Point3,
        /// End point in sketch space, millimetres.
        end: Point3,
        /// Unit direction vector from `start` to `end`.
        direction: Vector3,
        /// Unit normal of the sketch plane the line lies in.
        normal: Vector3,
    },
    /// A circular arc.
    Arc {
        /// Arc center in sketch space, millimetres.
        center: Point3,
        /// Unit normal of the sketch plane the arc lies in.
        normal: Vector3,
        /// Unit vector marking the zero-angle direction for `start_angle`/`end_angle`.
        reference_direction: Vector3,
        /// Arc radius in millimetres.
        radius: f64,
        /// Start angle in radians, measured from `reference_direction`.
        start_angle: f64,
        /// End angle in radians, measured from `reference_direction`.
        end_angle: f64,
    },
    /// A NURBS (procedural spline) curve.
    Nurbs {
        /// Record index of the underlying carrier geometry, when the NURBS record
        /// references one; `None` when the control data is self-contained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        carrier_reference: Option<u64>,
        /// Source per-file dynamic three-digit ASCII class tag naming the NURBS subtype.
        subtype_class_tag: String,
        /// Record index of the NURBS subtype record.
        subtype_record_index: u32,
        /// Polynomial degree of the curve.
        degree: u32,
        /// Source fit tolerance used when the curve was fitted, in millimetres.
        fit_tolerance: f64,
        /// Width in scalars of each control-point record as stored in the source
        /// (control point components plus weight, before decoding into `control_points`/`weights`).
        scalar_width: u32,
        /// Knot vector, non-decreasing, length `control_points.len() + degree + 1`.
        knots: Vec<f64>,
        /// Per-control-point rational weights, parallel to `control_points`.
        weights: Vec<f64>,
        /// Control points in sketch space, millimetres, parallel to `weights`.
        control_points: Vec<Point3>,
    },
}

/// One member of the Design `BulkStream` `BodiesRoot` list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignBodyMember {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of this member's leading presence byte in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Numeric suffix of this body's design-entity id.
    pub entity_suffix: u64,
    /// Source per-member flag word from the `BodiesRoot` list entry.
    pub flags: u16,
}

/// Triplicated axis-aligned body bounds cached in the Design stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignBodyBounds {
    /// Globally unique deterministic identifier for this native record set.
    pub id: String,
    /// Numeric suffix of the owning Design body entity.
    pub entity_suffix: u64,
    /// Byte offset of the owning Design entity header.
    pub entity_byte_offset: u64,
    /// Three consecutive indexed record identities carrying the cache.
    pub record_indices: [u32; 3],
    /// Indexed-header byte offsets parallel to `record_indices`.
    pub record_byte_offsets: [u64; 3],
    /// First f64 byte of each repeated sextuple.
    pub value_byte_offsets: [u64; 3],
    /// Design BREP body-map pairs carrying this entity suffix, in stream order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body_binding_ids: Vec<String>,
    /// Maximum model-space corner in millimetres.
    pub maximum: Point3,
    /// Minimum model-space corner in millimetres.
    pub minimum: Point3,
}

/// One ordered pair in a Design `BulkStream` BREP body-map record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignBodyBinding {
    /// Globally unique deterministic identifier for this native map entry.
    pub id: String,
    /// Design `BulkStream` ZIP entry containing the map.
    pub stream: String,
    /// Number of pairs in the enclosing body map.
    pub pair_count: u32,
    /// Zero-based position in the enclosing body map.
    pub pair_ordinal: u32,
    /// BREP body selector stored by this pair.
    pub asm_body_key: u64,
    /// Byte offset of `asm_body_key` within `stream`.
    pub asm_body_key_offset: u64,
    /// Numeric Design entity suffix stored by this pair.
    pub entity_suffix: u64,
    /// Byte offset of `entity_suffix` within `stream`.
    pub entity_suffix_offset: u64,
    /// Basename of the BREP blob whose body namespace contains the key.
    pub blob_name: String,
    /// Byte offset of the UTF-16LE `blob_name` code units within `stream`.
    pub blob_name_offset: u64,
    /// Solved body in the BREP blob named by this pair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<BodyId>,
}

/// Design browser-node visibility joined to one solved ASM body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BodyVisibility {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved B-rep body controlled by the browser node.
    pub body: BodyId,
    /// Design `BulkStream` ZIP entry containing the browser node.
    pub stream: String,
    /// Byte offset of the browser node's hidden flag within `stream`.
    pub byte_offset: u64,
    /// Byte offset of the joined body-map ASM key within `stream`.
    pub asm_body_key_offset: u64,
    /// ASM body key used by the BREP body-map join.
    pub asm_body_key: u64,
    /// Numeric Design entity suffix stored by both joined records.
    pub entity_suffix: u64,
    /// Display visibility after inverting the native hidden flag.
    pub visible: bool,
}

/// Inline `ACTTable` row attached to one change group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActTableRow {
    pub record_index_offset: u64,
    pub entity_id_offset: u64,
}

/// Channel-group payload owned by one ACT entity.
#[derive(Debug, Clone, PartialEq)]
pub struct ActChannelGroup {
    pub record_index_offset: u64,
    pub entity_id_offset: Option<u64>,
    pub class_tag: String,
    pub channels: BTreeMap<String, String>,
    pub guid_offsets: BTreeMap<String, u64>,
    pub class_tail: Vec<u8>,
    pub class_tail_offset: Option<u64>,
}

/// Whether an ACT entity is keyed in `ACTTable`, has a channel group, or both.
#[derive(Debug, Clone, PartialEq)]
pub enum ActEntityMembership {
    TableOnly(ActTableRow),
    GroupOnly(ActChannelGroup),
    Both(ActTableRow, ActChannelGroup),
}

/// One Fusion ACT change-version channel group and its optional inline table row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "ActEntitySerde"))]
#[serde(try_from = "ActEntitySerde", into = "ActEntitySerde")]
pub struct ActEntity {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Record index of this entity's change group. Its inline `ACTTable` row,
    /// when present, contains the same index.
    pub record_index: u32,
    /// UTF-16LE-decoded design-entity key this change group tracks.
    pub entity_id: String,
    /// Table-row and/or channel-group membership for this entity.
    pub membership: ActEntityMembership,
}

impl ActEntity {
    pub(crate) fn in_table(&self) -> bool {
        !matches!(self.membership, ActEntityMembership::GroupOnly(_))
    }

    pub(crate) fn table_row(&self) -> Option<&ActTableRow> {
        match &self.membership {
            ActEntityMembership::TableOnly(row) | ActEntityMembership::Both(row, _) => Some(row),
            ActEntityMembership::GroupOnly(_) => None,
        }
    }

    pub(crate) fn table_row_mut(&mut self) -> Option<&mut ActTableRow> {
        match &mut self.membership {
            ActEntityMembership::TableOnly(row) | ActEntityMembership::Both(row, _) => Some(row),
            ActEntityMembership::GroupOnly(_) => None,
        }
    }

    pub(crate) fn channel_group(&self) -> Option<&ActChannelGroup> {
        match &self.membership {
            ActEntityMembership::GroupOnly(group) | ActEntityMembership::Both(_, group) => {
                Some(group)
            }
            ActEntityMembership::TableOnly(_) => None,
        }
    }

    pub(crate) fn channel_group_mut(&mut self) -> Option<&mut ActChannelGroup> {
        match &mut self.membership {
            ActEntityMembership::GroupOnly(group) | ActEntityMembership::Both(_, group) => {
                Some(group)
            }
            ActEntityMembership::TableOnly(_) => None,
        }
    }

    pub(crate) fn table_record_index_offset(&self) -> Option<u64> {
        self.table_row().map(|row| row.record_index_offset)
    }

    pub(crate) fn table_entity_id_offset(&self) -> Option<u64> {
        self.table_row().map(|row| row.entity_id_offset)
    }

    pub(crate) fn channel_record_index_offset(&self) -> Option<u64> {
        self.channel_group().map(|group| group.record_index_offset)
    }

    pub(crate) fn channel_entity_id_offset(&self) -> Option<u64> {
        self.channel_group()
            .and_then(|group| group.entity_id_offset)
    }

    pub(crate) fn channel_class_tag(&self) -> Option<&str> {
        self.channel_group().map(|group| group.class_tag.as_str())
    }

    pub(crate) fn channels(&self) -> &BTreeMap<String, String> {
        self.channel_group()
            .map(|group| &group.channels)
            .unwrap_or(&EMPTY_ACT_CHANNELS)
    }

    pub(crate) fn channel_guid_offsets(&self) -> &BTreeMap<String, u64> {
        self.channel_group()
            .map(|group| &group.guid_offsets)
            .unwrap_or(&EMPTY_ACT_GUID_OFFSETS)
    }

    pub(crate) fn channel_class_tail(&self) -> &[u8] {
        self.channel_group()
            .map(|group| group.class_tail.as_slice())
            .unwrap_or(&[])
    }

    pub(crate) fn channel_class_tail_offset(&self) -> Option<u64> {
        self.channel_group()
            .and_then(|group| group.class_tail_offset)
    }

    pub(crate) fn attach_channel_group(&mut self, group: ActChannelGroup) -> bool {
        match &self.membership {
            ActEntityMembership::TableOnly(row) => {
                self.membership = ActEntityMembership::Both(row.clone(), group);
                true
            }
            ActEntityMembership::GroupOnly(_) | ActEntityMembership::Both(_, _) => false,
        }
    }

    pub(crate) fn strip_channel_group(&mut self) {
        if let ActEntityMembership::Both(row, _) = &self.membership {
            self.membership = ActEntityMembership::TableOnly(row.clone());
        }
    }
}

static EMPTY_ACT_CHANNELS: BTreeMap<String, String> = BTreeMap::new();
static EMPTY_ACT_GUID_OFFSETS: BTreeMap<String, u64> = BTreeMap::new();

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ActEntitySerde {
    id: String,
    record_index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    table_record_index_offset: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    channel_record_index_offset: Option<u64>,
    entity_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    table_entity_id_offset: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    channel_entity_id_offset: Option<u64>,
    in_table: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    channel_class_tag: Option<String>,
    #[serde(default)]
    channels: BTreeMap<String, String>,
    #[serde(default)]
    channel_guid_offsets: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    channel_class_tail: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    channel_class_tail_offset: Option<u64>,
}

impl TryFrom<ActEntitySerde> for ActEntity {
    type Error = String;

    fn try_from(wire: ActEntitySerde) -> Result<Self, Self::Error> {
        let table = match (
            wire.in_table,
            wire.table_record_index_offset,
            wire.table_entity_id_offset,
        ) {
            (true, Some(record_index_offset), Some(entity_id_offset)) => Some(ActTableRow {
                record_index_offset,
                entity_id_offset,
            }),
            (false, None, None) => None,
            _ => {
                return Err(
                    "act entity in_table disagrees with table_record_index_offset/table_entity_id_offset"
                        .into(),
                );
            }
        };
        let group = match (wire.channel_class_tag, wire.channel_record_index_offset) {
            (None, None)
                if wire.channels.is_empty()
                    && wire.channel_guid_offsets.is_empty()
                    && wire.channel_class_tail.is_empty()
                    && wire.channel_entity_id_offset.is_none()
                    && wire.channel_class_tail_offset.is_none() =>
            {
                None
            }
            (Some(class_tag), Some(record_index_offset)) => Some(ActChannelGroup {
                record_index_offset,
                entity_id_offset: wire.channel_entity_id_offset,
                class_tag,
                channels: wire.channels,
                guid_offsets: wire.channel_guid_offsets,
                class_tail: wire.channel_class_tail,
                class_tail_offset: wire.channel_class_tail_offset,
            }),
            _ => {
                return Err(
                    "act entity channel_class_tag disagrees with channel_record_index_offset"
                        .into(),
                );
            }
        };
        let membership = match (table, group) {
            (Some(row), None) => ActEntityMembership::TableOnly(row),
            (None, Some(group)) => ActEntityMembership::GroupOnly(group),
            (Some(row), Some(group)) => ActEntityMembership::Both(row, group),
            (None, None) => {
                return Err("act entity has neither an ACTTable row nor a channel group".into());
            }
        };
        Ok(Self {
            id: wire.id,
            record_index: wire.record_index,
            entity_id: wire.entity_id,
            membership,
        })
    }
}

impl From<ActEntity> for ActEntitySerde {
    fn from(entity: ActEntity) -> Self {
        let in_table = entity.in_table();
        let table_record_index_offset = entity.table_record_index_offset();
        let table_entity_id_offset = entity.table_entity_id_offset();
        let channel_record_index_offset = entity.channel_record_index_offset();
        let channel_entity_id_offset = entity.channel_entity_id_offset();
        let channel_class_tag = entity.channel_class_tag().map(str::to_owned);
        let channel_class_tail_offset = entity.channel_class_tail_offset();
        let (channels, channel_guid_offsets, channel_class_tail) = match entity.membership {
            ActEntityMembership::TableOnly(_) => (BTreeMap::new(), BTreeMap::new(), Vec::new()),
            ActEntityMembership::GroupOnly(group) | ActEntityMembership::Both(_, group) => {
                (group.channels, group.guid_offsets, group.class_tail)
            }
        };
        Self {
            id: entity.id,
            record_index: entity.record_index,
            table_record_index_offset,
            channel_record_index_offset,
            entity_id: entity.entity_id,
            table_entity_id_offset,
            channel_entity_id_offset,
            in_table,
            channel_class_tag,
            channels,
            channel_guid_offsets,
            channel_class_tail,
            channel_class_tail_offset,
        }
    }
}

/// One GUID in the ordered ACT stream-wide asset/change-version pool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ActGuid {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of this GUID's UTF-16 length prefix in the ACT `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of the UTF-16 GUID code units in the ACT `BulkStream`.
    pub guid_offset: u64,
    /// Position of this GUID in the pool, in source stream order; pool position does
    /// not assign one GUID to a single `ACTTable` entry.
    pub ordinal: u32,
    /// The pooled GUID string.
    pub guid: String,
}

/// One reference in the ACT table run between the GUID pool and channel registry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ActTableReference {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Position in the counted reference run, in source order.
    pub ordinal: u32,
    /// Byte offset of the reference-presence marker in the ACT `BulkStream`.
    pub byte_offset: u64,
    /// Target ACT record index.
    pub target_record: u32,
    /// Byte offset of `target_record`.
    pub target_record_offset: u64,
}

/// One named entry in the ACT table's stream-wide channel registry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ActRegistryChannel {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Position in the counted registry, in source order.
    pub ordinal: u32,
    /// Byte offset of the channel-name length prefix in the ACT `BulkStream`.
    pub byte_offset: u64,
    /// Stored channel name.
    pub name: String,
    /// Byte offset of the ASCII channel-name bytes.
    pub name_offset: u64,
    /// Stored registry GUID.
    pub guid: String,
    /// Byte offset of the UTF-16 GUID code units.
    pub guid_offset: u64,
}

/// ACT link from the document root entity to the instance/component registries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ActRootComponent {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of this record in the ACT `BulkStream`.
    pub byte_offset: u64,
    /// Index of this record within the ACT `BulkStream`.
    pub record_index: u32,
    /// Byte offset of `record_index`.
    pub record_index_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag naming this record's type.
    pub class_tag: String,
    /// Record index of the instance registry root.
    pub instance_root_record: u32,
    /// Byte offset of `instance_root_record`.
    pub instance_root_record_offset: u64,
    /// Record index of the Design entity tracked by this link. Value `3`
    /// identifies the document root.
    #[serde(default)]
    pub tracked_entity_record: u32,
    /// Byte offset of `tracked_entity_record`.
    #[serde(default)]
    pub tracked_entity_record_offset: u64,
    /// Record index of the components registry root.
    pub components_root_record: u32,
    /// Byte offset of `components_root_record`.
    pub components_root_record_offset: u64,
    /// Source counter/registry flag; 0 and 1 are both valid.
    pub registry_flag: ActRegistryFlag,
    /// Byte offset of `registry_flag`.
    pub registry_flag_offset: u64,
    /// UTF-16LE-decoded design-entity id of the document root entity.
    pub entity_id: String,
    /// Byte offset of the UTF-16 `entity_id` code units.
    pub entity_id_offset: u64,
    /// Document display name as stored alongside this root-component link.
    pub display_name: String,
    /// Byte offset of the UTF-16 `display_name` code units.
    pub display_name_offset: u64,
}

/// One design entry of the top-level `RedirectionsStream.dat` table
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
/// The first source entry describes the document itself; each further entry
/// describes one referenced document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct XrefDesign {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Position of this entry in the source `designs` array; entry 0 is the
    /// document itself.
    pub ordinal: u32,
    /// Source `file-version` integer.
    pub file_version: i64,
    /// The document's `.f3d` file name.
    pub target_file_name: String,
    /// The document's display name.
    pub display_name: String,
    /// `urn:adsk.wipprod:dm.lineage:<key>` lineage identity.
    pub lineage_urn: String,
    /// `urn:adsk.wipprod:fs.file:vf.<key>?version=N` version identity.
    pub version_urn: String,
}

/// One outgoing XREF placement of the top-level `RedirectionsStream.dat` table
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct XrefReference {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Position of this reference in the source `references` array.
    pub ordinal: u32,
    /// Zero-based occurrence position among Design records carrying this
    /// container reference's occurrence role.
    #[serde(default)]
    pub occurrence_ordinal: u32,
    /// The referencing document's own file name.
    pub from: String,
    /// The target design entry's `target_file_name`.
    pub relative_path: String,
    /// Occurrence-role GUID joining this reference to the Design-segment
    /// `DcXRefPCIFeature` record and the ACT GUID pool.
    pub neutron_role: String,
    /// The independent `neutronData` property value. It is retained exactly
    /// and is never inferred from or aliased to `neutron_role`.
    pub neutron_data: String,
    /// Source Design occurrence transform in centimetres. `None` is the
    /// serialized identity-placement form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<[[f64; 4]; 4]>,
}


#[cfg(test)]
mod tests;
