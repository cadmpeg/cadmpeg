// SPDX-License-Identifier: Apache-2.0
#![deny(clippy::disallowed_methods)]
//! Fusion parametric-design records and links to the solved B-rep.

use cadmpeg_ir::NonEmptyString;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::num::{NonZeroU32, NonZeroU64};

pub(crate) mod feature;
pub(crate) mod topology;

const IDENTITY_MATRIX: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// A secondary selection identity and its optional curve identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DesignSecondaryIdentity<Id> {
    #[serde(rename = "secondary_identity")]
    pub identity: Id,
    #[serde(
        rename = "curve_secondary_identity",
        skip_serializing_if = "Option::is_none"
    )]
    pub curve_identity: Option<Id>,
}

impl<Id> DesignSecondaryIdentity<Id> {
    fn from_wire(identity: Option<Id>, curve_identity: Option<Id>) -> Result<Option<Self>, String> {
        match (identity, curve_identity) {
            (Some(identity), curve_identity) => Ok(Some(Self {
                identity,
                curve_identity,
            })),
            (None, None) => Ok(None),
            (None, Some(_)) => Err("curve_secondary_identity requires secondary_identity".into()),
        }
    }
}

/// Design entity identity with a decimal u64 suffix after its final underscore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignEntityId(String);

impl TryFrom<String> for DesignEntityId {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let (_, suffix) = value
            .rsplit_once('_')
            .ok_or("entity_id requires a decimal suffix")?;
        suffix
            .parse::<u64>()
            .map_err(|_| "entity_id requires a decimal u64 suffix")?;
        Ok(Self(value))
    }
}

impl DesignEntityId {
    pub fn from_parts(prefix: &str, suffix: u64) -> Self {
        Self(format!("{prefix}_{suffix}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn suffix(&self) -> u64 {
        self.0
            .rsplit('_')
            .take(1)
            .flat_map(str::bytes)
            .filter(|byte| *byte != b'+')
            .fold(0, |value, digit| value * 10 + u64::from(digit - b'0'))
    }
}

/// ACT root-component registry flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

const DESIGN_DECAL_FIT_TO_FACES_CODE: u8 = 0x60;

/// A Decal mapping byte other than the fit-to-faces code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnrecognizedDecalMappingMode(u8);
impl TryFrom<u8> for UnrecognizedDecalMappingMode {
    type Error = String;
    fn try_from(code: u8) -> Result<Self, Self::Error> {
        if code == DESIGN_DECAL_FIT_TO_FACES_CODE {
            Err("mapping_mode unknown payload must exclude the fit-to-faces code".into())
        } else {
            Ok(Self(code))
        }
    }
}

/// Decal image mapping-mode byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "u8", into = "u8")]
pub enum DesignDecalMappingMode {
    FitToFaces,
    Unknown(UnrecognizedDecalMappingMode),
}

impl DesignDecalMappingMode {
    #[must_use]
    pub fn from_code(code: u8) -> Self {
        match code {
            DESIGN_DECAL_FIT_TO_FACES_CODE => Self::FitToFaces,
            code => Self::Unknown(UnrecognizedDecalMappingMode(code)),
        }
    }

    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::FitToFaces => DESIGN_DECAL_FIT_TO_FACES_CODE,
            Self::Unknown(code) => code.0,
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

/// A source value and the byte offset of its encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReferenceRunData<T, O> {
    Unlocated(Vec<T>),
    Located(Vec<Located<T, O>>),
}

/// An ordered run whose encoding locations are either complete or absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceRun<T, O = u64>(ReferenceRunData<T, O>);

impl<T, O> ReferenceRun<T, O> {
    /// A run whose values carry no encoding locations. The empty run has no
    /// locations in either wire form, so it is always `Located(vec![])`.
    pub fn unlocated(values: Vec<T>) -> Self {
        if values.is_empty() {
            Self(ReferenceRunData::Located(Vec::new()))
        } else {
            Self(ReferenceRunData::Unlocated(values))
        }
    }

    /// A run whose values each carry an encoding location.
    pub fn located(rows: Vec<Located<T, O>>) -> Self {
        Self(ReferenceRunData::Located(rows))
    }

    pub(crate) fn located_rows(&self) -> Option<&[Located<T, O>]> {
        match &self.0 {
            ReferenceRunData::Located(rows) => Some(rows),
            ReferenceRunData::Unlocated(_) => None,
        }
    }

    pub fn values(&self) -> impl ExactSizeIterator<Item = &T> + DoubleEndedIterator + Clone {
        (0..self.len()).map(|index| match &self.0 {
            ReferenceRunData::Unlocated(values) => &values[index],
            ReferenceRunData::Located(values) => &values[index].value,
        })
    }

    pub fn values_in(
        &self,
        range: std::ops::Range<usize>,
    ) -> Option<impl ExactSizeIterator<Item = &T> + DoubleEndedIterator + Clone> {
        if range.start > range.end || range.end > self.len() {
            return None;
        }
        Some(
            self.values()
                .skip(range.start)
                .take(range.end - range.start),
        )
    }

    pub fn values_array<const N: usize>(&self) -> Option<[&T; N]> {
        match &self.0 {
            ReferenceRunData::Unlocated(values) => {
                let values: &[T; N] = values.as_slice().try_into().ok()?;
                Some(values.each_ref())
            }
            ReferenceRunData::Located(values) => {
                let values: &[Located<T, O>; N] = values.as_slice().try_into().ok()?;
                Some(values.each_ref().map(|row| &row.value))
            }
        }
    }

    pub fn offsets(&self) -> impl ExactSizeIterator<Item = &O> + DoubleEndedIterator + Clone {
        let rows: &[Located<T, O>] = match &self.0 {
            ReferenceRunData::Unlocated(_) => &[],
            ReferenceRunData::Located(rows) => rows,
        };
        rows.iter().map(|row| &row.offset)
    }

    #[cfg(test)]
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        let (unlocated, located): (&mut [T], &mut [Located<T, O>]) = match &mut self.0 {
            ReferenceRunData::Unlocated(values) => (values, &mut []),
            ReferenceRunData::Located(values) => (&mut [], values),
        };
        unlocated
            .iter_mut()
            .chain(located.iter_mut().map(|row| &mut row.value))
    }

    pub fn len(&self) -> usize {
        match &self.0 {
            ReferenceRunData::Unlocated(values) => values.len(),
            ReferenceRunData::Located(values) => values.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.0 {
            ReferenceRunData::Unlocated(values) => values.is_empty(),
            ReferenceRunData::Located(values) => values.is_empty(),
        }
    }

    pub(crate) fn from_columns(
        values: Vec<T>,
        offsets: Vec<O>,
        field: &str,
    ) -> Result<Self, String> {
        if offsets.is_empty() {
            return Ok(Self::unlocated(values));
        }
        if values.len() != offsets.len() {
            return Err(format!(
                "{field} offsets must be absent or match every value"
            ));
        }
        Ok(Self::located(
            values
                .into_iter()
                .zip(offsets)
                .map(|(value, offset)| Located { value, offset })
                .collect(),
        ))
    }

    fn into_wire(self) -> (Vec<T>, Vec<O>) {
        match self.0 {
            ReferenceRunData::Unlocated(values) => (values, Vec::new()),
            ReferenceRunData::Located(values) => values
                .into_iter()
                .map(|row| (row.value, row.offset))
                .unzip(),
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
pub struct RecordedValue<T> {
    pub value: T,
    pub offset: Option<u64>,
}

impl<T> RecordedValue<T> {
    fn from_wire(
        value: Option<T>,
        offset: Option<u64>,
        field: &str,
    ) -> Result<Option<Self>, String> {
        match (value, offset) {
            (Some(value), offset) => Ok(Some(Self { value, offset })),
            (None, None) => Ok(None),
            (None, Some(_)) => Err(format!("{field}_offset requires {field}")),
        }
    }
}

// The wire adapter receives the optional field by reference, including its absence.
#[allow(clippy::ref_option)]
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
use cadmpeg_ir::features::Angle;
use cadmpeg_ir::ids::{BodyId, EdgeId, FaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::TextPlacement;
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

/// A stored sketch-link sense excluding the unconstrained sentinels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "i64", into = "i64")]
pub struct SketchLinkSense(i64);
impl TryFrom<i64> for SketchLinkSense {
    type Error = &'static str;
    fn try_from(value: i64) -> Result<Self, Self::Error> {
        if sketch_link_sense_is_unconstrained(value) {
            return Err("sense must omit the unconstrained sentinel");
        }
        Ok(Self(value))
    }
}
impl From<SketchLinkSense> for i64 {
    fn from(value: SketchLinkSense) -> Self {
        value.0
    }
}

/// Provenance link from a solved B-rep entity to its source sketch curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub sense: Option<SketchLinkSense>,
    /// Source role tag distinguishing how the sketch curve participates in the link
    /// (e.g. profile edge vs. construction reference).
    pub role: i64,
    /// Source closure/continuity tag of the sketch curve at this link.
    pub closure: i64,
}

/// Persistent Fusion design identifier attached to a solved B-rep entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
pub struct DesignComponentNamingSpace {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the binding marker in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Component entity id named by the binding.
    pub component_record_index: u64,
    /// UUID used by persistent identities to select this component.
    pub context_uuid: DesignRelaxedGuidText,
    /// Byte offset of the UUID length prefix.
    pub context_uuid_offset: u64,
}

/// Original authoring time attached to a solved ASM entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// A recipe Design id and its optional following selector.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConstructionRecipeDesign<Id> {
    #[serde(rename = "design_id")]
    pub id: Id,
    #[serde(rename = "design_selector", skip_serializing_if = "Option::is_none")]
    pub selector: Option<ConstructionRecipeSelector>,
}

/// One source-framed parametric regeneration recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Design identity carried by the recipe; absent for recipes without a body key.
    pub design: Option<ConstructionRecipeDesign<RecordedValue<String>>>,
    /// Position of this recipe in the `BulkStream` recipe sequence, in source order.
    pub recipe_index: u32,
    /// Source `BulkStream` record index this recipe was decoded from.
    pub record_index: i32,
}

/// One source-framed parametric regeneration recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
        let id = RecordedValue::from_wire(wire.design_id, wire.design_id_offset, "design_id")?;
        let design = match (id, wire.design_selector) {
            (Some(id), selector) => Some(ConstructionRecipeDesign { id, selector }),
            (None, None) => None,
            (None, Some(_)) => {
                return Err("construction recipe design_selector requires design_id".into())
            }
        };
        Ok(Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            record_index_offset: wire.record_index_offset,
            kind: wire.kind,
            design,
            recipe_index: wire.recipe_index,
            record_index: wire.record_index,
        })
    }
}

impl From<ConstructionRecipe> for ConstructionRecipeWire {
    fn from(value: ConstructionRecipe) -> Self {
        let (design_id, design_id_offset, design_selector) = match value.design {
            Some(design) => (Some(design.id.value), design.id.offset, design.selector),
            None => (None, None, None),
        };
        Self {
            id: value.id,
            byte_offset: value.byte_offset,
            record_index_offset: value.record_index_offset,
            kind: value.kind,
            design_id,
            design_id_offset,
            design_selector,
            recipe_index: value.recipe_index,
            record_index: value.record_index,
        }
    }
}

/// Serialized Design selector carried by a construction recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConstructionRecipeSelector {
    /// Selector value.
    pub value: u32,
    /// Byte offset of `value`.
    pub byte_offset: u64,
}

/// Semantic family of one Design parameter record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignParameterKind {
    /// A document-level named user parameter.
    User,
    /// A dimensional constraint parameter.
    Dimension,
    /// A parameter consumed by a construction feature.
    Feature,
}

/// Admitted Design parameter family discriminator values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum DesignParameterDiscriminator {
    Code0 = 0,
    Code3 = 3,
    Code4 = 4,
    Code5 = 5,
    Code6 = 6,
}

impl DesignParameterDiscriminator {
    pub(crate) fn code(self) -> u64 {
        self as u64
    }
}

impl TryFrom<u64> for DesignParameterDiscriminator {
    type Error = String;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Code0),
            3 => Ok(Self::Code3),
            4 => Ok(Self::Code4),
            5 => Ok(Self::Code5),
            6 => Ok(Self::Code6),
            _ => Err(format!(
                "invalid design parameter family_discriminator {value}"
            )),
        }
    }
}

/// Source family and ownership of one Design parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesignParameterSource {
    User {
        family_discriminator: Located<DesignParameterDiscriminator>,
    },
    Owned(OwnedDesignParameter),
}

/// An owned source family. Its nonempty name cannot identify a user parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedDesignParameter {
    source_kind: String,
    owner_record_index: u32,
    family_discriminator: Option<Located<DesignParameterDiscriminator>>,
}

impl DesignParameterSource {
    pub(crate) fn new(
        source_kind: String,
        owner_record_index: Option<u32>,
        family_discriminator: Option<Located<DesignParameterDiscriminator>>,
    ) -> Result<Self, String> {
        match (source_kind.as_str(), owner_record_index) {
            ("", _) => Err("design parameter source_kind is empty".into()),
            ("User Parameter", None) => family_discriminator
                .map(|family_discriminator| Self::User {
                    family_discriminator,
                })
                .ok_or_else(|| {
                    "design parameter family_discriminator is missing for user source_kind".into()
                }),
            ("User Parameter", Some(_)) => {
                Err("design parameter owner_record_index is present for user source_kind".into())
            }
            (_, Some(owner_record_index)) => Ok(Self::Owned(OwnedDesignParameter {
                source_kind,
                owner_record_index,
                family_discriminator,
            })),
            (_, None) => {
                Err("design parameter owner_record_index is missing for owned source_kind".into())
            }
        }
    }

    fn translate_discriminator_offset(&mut self, offset: u64) {
        let discriminator = match self {
            Self::User {
                family_discriminator,
            } => Some(family_discriminator),
            Self::Owned(source) => source.family_discriminator.as_mut(),
        };
        if let Some(discriminator) = discriminator {
            discriminator.offset += offset;
        }
    }
}

/// One indexed Design parameter or expression record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignParameterSerde", into = "DesignParameterSerde")]
pub struct DesignParameter {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the indexed record header in its Design `BulkStream`.
    byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: DesignClassTag,
    /// Source indexed-record identity.
    pub record_index: u32,
    /// Source ordering value stored by the parameter record.
    pub source_ordinal: u32,
    /// Indexed owner: user parameters have none; feature and dimension
    /// parameters name their owning record.
    source: DesignParameterSource,
    /// Literal or symbolic source expression.
    expression: NonEmptyString,
    /// Byte offset of the expression's UTF-16LE code units.
    expression_offset: u64,
    /// Byte offset of the source-family UTF-16LE code units.
    source_kind_offset: u64,
    /// Declared unit token; absent for dimensionless and Boolean parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unit: Option<Located<NonEmptyString>>,
    /// Source parameter name or dimension identifier.
    name: NonEmptyString,
    /// Byte offset of the name's UTF-16LE code units.
    name_offset: u64,
    /// Evaluated scalar in the record's native unit convention.
    evaluated_value: f64,
    /// Byte offset of `evaluated_value`.
    evaluated_value_offset: u64,
}

/// Unchecked Design parameter input.
pub(crate) struct DesignParameterDraft {
    pub id: String,
    pub byte_offset: u64,
    pub class_tag: DesignClassTag,
    pub record_index: u32,
    pub source_ordinal: u32,
    pub source: DesignParameterSource,
    pub expression: String,
    pub expression_offset: u64,
    pub source_kind_offset: u64,
    pub unit: Option<RecordedValue<String>>,
    pub name: String,
    pub name_offset: u64,
    pub evaluated_value: f64,
    pub evaluated_value_offset: u64,
}

impl TryFrom<DesignParameterDraft> for DesignParameter {
    type Error = String;
    fn try_from(draft: DesignParameterDraft) -> Result<Self, Self::Error> {
        if !draft.evaluated_value.is_finite() {
            return Err("evaluated_value must be finite".into());
        }
        let expression =
            NonEmptyString::new(draft.expression).ok_or("expression must not be empty")?;
        let name = NonEmptyString::new(draft.name).ok_or("name must not be empty")?;
        let unit = draft
            .unit
            .map(|unit| {
                Ok::<_, String>(Located {
                    value: NonEmptyString::new(unit.value).ok_or("unit must not be empty")?,
                    offset: unit.offset.ok_or("unit_offset is required with unit")?,
                })
            })
            .transpose()?;
        check_parameter_discriminator_offset(
            &draft.source,
            draft.byte_offset,
            draft.expression_offset,
        )?;
        if !(draft.byte_offset < draft.expression_offset
            && draft.expression_offset < draft.source_kind_offset
            && unit
                .as_ref()
                .map_or(draft.source_kind_offset < draft.name_offset, |unit| {
                    draft.source_kind_offset < unit.offset && unit.offset < draft.name_offset
                })
            && draft.name_offset < draft.evaluated_value_offset)
        {
            return Err("byte_offset, expression_offset, source_kind_offset, unit_offset, name_offset, and evaluated_value_offset must be strictly ordered".into());
        }
        Ok(Self {
            id: draft.id,
            byte_offset: draft.byte_offset,
            class_tag: draft.class_tag,
            record_index: draft.record_index,
            source_ordinal: draft.source_ordinal,
            source: draft.source,
            expression,
            expression_offset: draft.expression_offset,
            source_kind_offset: draft.source_kind_offset,
            unit,
            name,
            name_offset: draft.name_offset,
            evaluated_value: draft.evaluated_value,
            evaluated_value_offset: draft.evaluated_value_offset,
        })
    }
}

fn check_parameter_discriminator_offset(
    source: &DesignParameterSource,
    byte_offset: u64,
    expression_offset: u64,
) -> Result<(), String> {
    let family_discriminator = match source {
        DesignParameterSource::User {
            family_discriminator,
        } => Some(*family_discriminator),
        DesignParameterSource::Owned(source) => source.family_discriminator,
    };
    if family_discriminator.is_some_and(|field| {
        byte_offset.checked_add(22) != Some(field.offset) || field.offset >= expression_offset
    }) {
        return Err("family_discriminator_offset must follow byte_offset by 22 bytes and precede expression_offset".into());
    }
    Ok(())
}

impl DesignParameter {
    /// Source family and ownership.
    pub(crate) fn source(&self) -> &DesignParameterSource {
        &self.source
    }

    #[cfg(test)]
    /// Checked replacement of a present unit token.
    pub(crate) fn try_set_unit_value(&mut self, value: String) -> Result<(), String> {
        let value = NonEmptyString::new(value).ok_or("unit must not be empty")?;
        let unit = self.unit.as_mut().ok_or("unit is absent")?;
        unit.value = value;
        Ok(())
    }

    /// Indexed record header offset.
    pub fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    /// Expression code-unit offset.
    pub fn expression_offset(&self) -> u64 {
        self.expression_offset
    }
    /// Source-family code-unit offset.
    pub fn source_kind_offset(&self) -> u64 {
        self.source_kind_offset
    }
    /// Name code-unit offset.
    pub fn name_offset(&self) -> u64 {
        self.name_offset
    }
    /// Evaluated scalar offset.
    pub fn evaluated_value_offset(&self) -> u64 {
        self.evaluated_value_offset
    }
    /// Finite evaluated scalar.
    pub fn evaluated_value(&self) -> f64 {
        self.evaluated_value
    }
    /// Nonempty parameter name.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }
    /// Nonempty source expression.
    pub fn expression(&self) -> &str {
        self.expression.as_str()
    }
    /// Located nonempty unit token, when present.
    pub fn unit(&self) -> Option<&Located<NonEmptyString>> {
        self.unit.as_ref()
    }

    /// Checked translation of all parameter locations.
    pub(crate) fn try_translate_offsets(&mut self, delta: u64) -> Result<(), String> {
        let evaluated_value_offset = self
            .evaluated_value_offset
            .checked_add(delta)
            .ok_or("evaluated_value_offset translation overflows")?;
        self.byte_offset += delta;
        self.source.translate_discriminator_offset(delta);
        self.expression_offset += delta;
        self.source_kind_offset += delta;
        if let Some(unit) = &mut self.unit {
            unit.offset += delta;
        }
        self.name_offset += delta;
        self.evaluated_value_offset = evaluated_value_offset;
        Ok(())
    }

    #[cfg(test)]
    /// Checked replacement of the evaluated scalar.
    pub(crate) fn try_set_evaluated_value(&mut self, value: f64) -> Result<(), String> {
        if !value.is_finite() {
            return Err("evaluated_value must be finite".into());
        }
        self.evaluated_value = value;
        Ok(())
    }
    #[cfg(test)]
    /// Checked replacement of source family and ownership.
    pub(crate) fn try_set_source(&mut self, source: DesignParameterSource) -> Result<(), String> {
        check_parameter_discriminator_offset(&source, self.byte_offset, self.expression_offset)?;
        self.source = source;
        Ok(())
    }

    pub(crate) fn family_discriminator(&self) -> Option<Located<DesignParameterDiscriminator>> {
        match &self.source {
            DesignParameterSource::User {
                family_discriminator,
            } => Some(*family_discriminator),
            DesignParameterSource::Owned(source) => source.family_discriminator,
        }
    }

    pub(crate) fn source_kind(&self) -> &str {
        match &self.source {
            DesignParameterSource::User { .. } => "User Parameter",
            DesignParameterSource::Owned(source) => &source.source_kind,
        }
    }

    pub(crate) fn kind(&self) -> DesignParameterKind {
        design_parameter_kind_from_source(self.source_kind())
    }

    pub(crate) fn owner_record_index(&self) -> Option<u32> {
        match &self.source {
            DesignParameterSource::User { .. } => None,
            DesignParameterSource::Owned(source) => Some(source.owner_record_index),
        }
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
        let family_discriminator = Located::from_wire(
            wire.family_discriminator,
            wire.family_discriminator_offset,
            "DesignParameter.family_discriminator",
        )?
        .map(|field| {
            Ok::<_, String>(Located {
                value: DesignParameterDiscriminator::try_from(field.value)?,
                offset: field.offset,
            })
        })
        .transpose()?;
        let source = DesignParameterSource::new(
            wire.source_kind,
            wire.owner_record_index,
            family_discriminator,
        )?;
        Self::try_from(DesignParameterDraft {
            id: wire.id,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            record_index: wire.record_index,
            source_ordinal: wire.source_ordinal,
            source,
            expression: wire.expression,
            expression_offset: wire.expression_offset,
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
        let byte_offset = parameter.byte_offset();
        let expression_offset = parameter.expression_offset();
        let source_kind_offset = parameter.source_kind_offset();
        let name_offset = parameter.name_offset();
        let evaluated_value_offset = parameter.evaluated_value_offset();
        let evaluated_value = parameter.evaluated_value();
        let kind = parameter.kind();
        let owner_record_index = parameter.owner_record_index();
        let family_discriminator = parameter.family_discriminator();
        let source_kind = match parameter.source {
            DesignParameterSource::User { .. } => "User Parameter".to_owned(),
            DesignParameterSource::Owned(source) => source.source_kind,
        };
        Self {
            id: parameter.id,
            byte_offset,
            class_tag: parameter.class_tag.into(),
            record_index: parameter.record_index,
            family_discriminator: family_discriminator.map(|value| value.value.code()),
            family_discriminator_offset: family_discriminator.map(|value| value.offset),
            source_ordinal: parameter.source_ordinal,
            owner_record_index,
            expression: parameter.expression.to_string(),
            expression_offset,
            source_kind,
            source_kind_offset,
            kind,
            unit_offset: parameter.unit.as_ref().map(|field| field.offset),
            unit: parameter.unit.map(|field| field.value.to_string()),
            name: parameter.name.to_string(),
            name_offset,
            evaluated_value,
            evaluated_value_offset,
        }
    }
}

/// Indexed record that owns one Design parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignParameterOwnerWire",
    into = "DesignParameterOwnerWire"
)]
pub struct DesignParameterOwner {
    id: String,
    byte_offset: u64,
    frame_length: u64,
    class_tag: DesignClassTag,
    scope_record_index: u32,
    local_ordinal: u32,
    evaluated_value: f64,
    evaluated_value_offset: u64,
    owned_ordinal: u32,
    variant: Option<u8>,
    base_index: u32,
    order: ParameterFrameOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParameterFrameOrder {
    OwnerFirst,
    ParameterFirst,
    CompanionFirst,
}

impl DesignParameterOwner {
    /// The id value.
    pub fn id(&self) -> &String {
        &self.id
    }
    /// The byte offset value.
    pub fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    /// The frame length value.
    pub fn frame_length(&self) -> u64 {
        self.frame_length
    }
    /// The class tag value.
    pub fn class_tag(&self) -> &DesignClassTag {
        &self.class_tag
    }
    /// The record index value.
    pub fn record_index(&self) -> u32 {
        self.base_index
            + match self.order {
                ParameterFrameOrder::OwnerFirst => 0,
                ParameterFrameOrder::ParameterFirst => 1,
                ParameterFrameOrder::CompanionFirst => 0,
            }
    }
    /// The scope record index value.
    pub fn scope_record_index(&self) -> u32 {
        self.scope_record_index
    }
    /// The local ordinal value.
    pub fn local_ordinal(&self) -> u32 {
        self.local_ordinal
    }
    /// The evaluated value value.
    pub fn evaluated_value(&self) -> f64 {
        self.evaluated_value
    }
    /// The evaluated value offset value.
    pub fn evaluated_value_offset(&self) -> u64 {
        self.evaluated_value_offset
    }
    /// The parameter record index value.
    pub fn parameter_record_index(&self) -> u32 {
        self.base_index
            + match self.order {
                ParameterFrameOrder::OwnerFirst => 1,
                ParameterFrameOrder::ParameterFirst => 0,
                ParameterFrameOrder::CompanionFirst => 2,
            }
    }
    /// The owned ordinal value.
    pub fn owned_ordinal(&self) -> u32 {
        self.owned_ordinal
    }
    /// The companion record index value.
    pub fn companion_record_index(&self) -> u32 {
        self.base_index
            + match self.order {
                ParameterFrameOrder::OwnerFirst => 2,
                ParameterFrameOrder::ParameterFirst => 2,
                ParameterFrameOrder::CompanionFirst => 1,
            }
    }
}

impl TryFrom<DesignParameterOwnerWire> for DesignParameterOwner {
    type Error = String;
    fn try_from(wire: DesignParameterOwnerWire) -> Result<Self, Self::Error> {
        if !wire.evaluated_value.is_finite() {
            return Err("evaluated_value must be finite".into());
        }
        let (base_index, order) = if wire.record_index.checked_add(1)
            == Some(wire.parameter_record_index)
            && wire.record_index.checked_add(2) == Some(wire.companion_record_index)
        {
            (wire.record_index, ParameterFrameOrder::OwnerFirst)
        } else if wire.parameter_record_index.checked_add(1) == Some(wire.record_index)
            && wire.parameter_record_index.checked_add(2) == Some(wire.companion_record_index)
        {
            (
                wire.parameter_record_index,
                ParameterFrameOrder::ParameterFirst,
            )
        } else if wire.record_index.checked_add(1) == Some(wire.companion_record_index)
            && wire.record_index.checked_add(2) == Some(wire.parameter_record_index)
        {
            (wire.record_index, ParameterFrameOrder::CompanionFirst)
        } else {
            return Err("record_index, parameter_record_index, and companion_record_index must follow a parameter frame order".into());
        };
        let modern = matches!(
            (
                wire.frame_length,
                wire.evaluated_value_offset.checked_sub(wire.byte_offset),
                wire.variant
            ),
            (99 | 103, Some(40), None)
                | (100, Some(41), None)
                | (107, Some(44), None)
                | (101, Some(41), Some(0..=1))
                | (104, Some(40), Some(0..=1))
                | (108, Some(44), Some(0..=1))
        );
        let legacy = wire.local_ordinal == 0
            && match wire.frame_length {
                68 => {
                    crate::design::decode::parameters::is_legacy_parameter_owner_68_class(
                        wire.class_tag.as_str(),
                    ) && wire.scope_record_index == 0
                }
                88 => {
                    crate::design::decode::parameters::is_legacy_parameter_owner_88_class(
                        wire.class_tag.as_str(),
                    ) && wire.scope_record_index != 0
                }
                _ => false,
            };
        if !modern && !legacy {
            return Err("frame_length, evaluated_value_offset, and variant must describe a parameter owner frame".into());
        }
        Ok(Self {
            base_index,
            order,
            id: wire.id,
            byte_offset: wire.byte_offset,
            frame_length: wire.frame_length,
            class_tag: wire.class_tag,
            scope_record_index: wire.scope_record_index,
            local_ordinal: wire.local_ordinal,
            evaluated_value: wire.evaluated_value,
            evaluated_value_offset: wire.evaluated_value_offset,
            owned_ordinal: wire.owned_ordinal,
            variant: wire.variant,
        })
    }
}
impl From<DesignParameterOwner> for DesignParameterOwnerWire {
    fn from(owner: DesignParameterOwner) -> Self {
        Self {
            id: owner.id.clone(),
            byte_offset: owner.byte_offset,
            frame_length: owner.frame_length,
            class_tag: owner.class_tag.clone(),
            record_index: owner.record_index(),
            scope_record_index: owner.scope_record_index,
            local_ordinal: owner.local_ordinal,
            evaluated_value: owner.evaluated_value,
            evaluated_value_offset: owner.evaluated_value_offset,
            parameter_record_index: owner.parameter_record_index(),
            owned_ordinal: owner.owned_ordinal,
            variant: owner.variant,
            companion_record_index: owner.companion_record_index(),
        }
    }
}

/// Unchecked parameter owner fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignParameterOwnerWire {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the indexed record header in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte length from the primary header to its same-index paired header.
    #[serde(default)]
    pub frame_length: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: DesignClassTag,
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
pub struct DesignParameterCompanion {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the indexed record header in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: DesignClassTag,
    /// Source indexed-record identity.
    pub record_index: u32,
    /// Indexed parameter-owner record referenced by this prefix.
    pub owner_record_index: u32,
    /// Nonzero Unix-epoch timestamp in microseconds.
    #[serde(
        alias = "opaque_value",
        deserialize_with = "deserialize_companion_timestamp"
    )]
    pub timestamp_micros: NonZeroU64,
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

fn deserialize_companion_timestamp<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<NonZeroU64, D::Error> {
    NonZeroU64::new(u64::deserialize(deserializer)?)
        .ok_or_else(|| serde::de::Error::custom("timestamp_micros must be nonzero"))
}

/// Indexed record that directly contains one construction recipe owned by a
/// dimensional parameter companion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    pub class_tag: DesignClassTag,
    /// Source indexed-record identity.
    pub record_index: u32,
    /// Number of bytes from this header to the next indexed header or the end
    /// of the companion-owned payload.
    pub frame_length: u64,
    /// Byte offset of the recipe-specific prefix after the indexed header.
    pub prefix_offset: u64,
    /// Complete recipe-specific prefix before the length-prefixed family name.
    #[serde(with = "cadmpeg_ir::bytes")]
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
///
/// One frame shape covers both source forms: the two-locus form, which carries
/// the opaque index that precedes its loci, and the null-locus form, whose
/// first locus is the fixed zero reference and which carries no opaque index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignDimensionLocusPairWire",
    into = "DesignDimensionLocusPairWire"
)]
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
    pub class_tag: DesignClassTag,
    /// Shared logical record identity.
    pub record_index: u32,
    /// Byte length from the primary header to the paired header.
    pub frame_length: u64,
    /// Opaque u32 preceding the two locus references. Present exactly when the
    /// first locus names sketch geometry.
    pub opaque_index: Option<Located<u32>>,
    /// The two ordered loci. `loci[0].geometry_record_index` is `None` in the
    /// null-locus form, where the frame stores a fixed zero record reference.
    pub loci: [DesignDimensionAnnotationOperand; 2],
    /// Per-file dynamic class tag of the paired header.
    pub paired_class_tag: DesignClassTag,
    /// Byte offset of the paired indexed record header.
    pub paired_byte_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DesignDimensionLocusPairWire {
    id: String,
    companion_record_index: u32,
    governing_companion_record_index: u32,
    byte_offset: u64,
    class_tag: String,
    record_index: u32,
    frame_length: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    opaque_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    opaque_index_offset: Option<u64>,
    first_geometry_record_index: u32,
    first_geometry_reference_offset: u64,
    first_role: u32,
    first_role_offset: u64,
    second_geometry_record_index: u32,
    second_geometry_reference_offset: u64,
    second_role: u32,
    second_role_offset: u64,
    paired_class_tag: String,
    paired_byte_offset: u64,
}

impl TryFrom<DesignDimensionLocusPairWire> for DesignDimensionLocusPair {
    type Error = String;

    fn try_from(wire: DesignDimensionLocusPairWire) -> Result<Self, Self::Error> {
        let opaque_index = match (wire.opaque_index, wire.opaque_index_offset) {
            (None, None) => None,
            (Some(value), Some(offset)) => Some(Located { value, offset }),
            _ => return Err("opaque_index and opaque_index_offset must occur together".to_owned()),
        };
        let first = NonZeroU32::new(wire.first_geometry_record_index);
        if opaque_index.is_some() != first.is_some() {
            return Err(
                "opaque_index is present exactly when first_geometry_record_index names geometry"
                    .to_owned(),
            );
        }
        let second = NonZeroU32::new(wire.second_geometry_record_index)
            .ok_or("second_geometry_record_index must name an indexed sketch-geometry record")?;
        Ok(Self {
            id: wire.id,
            companion_record_index: wire.companion_record_index,
            governing_companion_record_index: wire.governing_companion_record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            record_index: wire.record_index,
            frame_length: wire.frame_length,
            opaque_index,
            loci: [
                DesignDimensionAnnotationOperand {
                    geometry_record_index: first,
                    geometry_reference_offset: wire.first_geometry_reference_offset,
                    role: wire.first_role,
                    role_offset: wire.first_role_offset,
                },
                DesignDimensionAnnotationOperand {
                    geometry_record_index: Some(second),
                    geometry_reference_offset: wire.second_geometry_reference_offset,
                    role: wire.second_role,
                    role_offset: wire.second_role_offset,
                },
            ],
            paired_class_tag: wire.paired_class_tag.try_into()?,
            paired_byte_offset: wire.paired_byte_offset,
        })
    }
}

impl From<DesignDimensionLocusPair> for DesignDimensionLocusPairWire {
    fn from(pair: DesignDimensionLocusPair) -> Self {
        let [first, second] = pair.loci;
        Self {
            id: pair.id,
            companion_record_index: pair.companion_record_index,
            governing_companion_record_index: pair.governing_companion_record_index,
            byte_offset: pair.byte_offset,
            class_tag: pair.class_tag.into(),
            record_index: pair.record_index,
            frame_length: pair.frame_length,
            opaque_index: pair.opaque_index.as_ref().map(|located| located.value),
            opaque_index_offset: pair.opaque_index.as_ref().map(|located| located.offset),
            first_geometry_record_index: first.geometry_record_index.map_or(0, NonZeroU32::get),
            first_geometry_reference_offset: first.geometry_reference_offset,
            first_role: first.role,
            first_role_offset: first.role_offset,
            second_geometry_record_index: second.geometry_record_index.map_or(0, NonZeroU32::get),
            second_geometry_reference_offset: second.geometry_reference_offset,
            second_role: second.role,
            second_role_offset: second.role_offset,
            paired_class_tag: pair.paired_class_tag.into(),
            paired_byte_offset: pair.paired_byte_offset,
        }
    }
}

/// One nullable typed operand in an annotated dimension frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignDimensionAnnotationOperand {
    /// Indexed sketch geometry record, absent for the null locus.
    #[serde(with = "annotation_geometry_index")]
    pub geometry_record_index: Option<NonZeroU32>,
    /// Byte offset of `geometry_record_index`.
    pub geometry_reference_offset: u64,
    /// Source dimension-role code.
    pub role: u32,
    /// Byte offset of `role`.
    pub role_offset: u64,
}

impl DesignDimensionAnnotationOperand {
    /// The named sketch-geometry record, or the wire's zero for the null locus.
    pub(crate) fn geometry_index(&self) -> u32 {
        self.geometry_record_index.map_or(0, NonZeroU32::get)
    }
}

mod annotation_geometry_index {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::num::NonZeroU32;

    // The wire adapter receives the optional field by reference, including its absence.
    // Serde passes the field by reference to this wire adapter.
    #[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
    pub fn serialize<S: Serializer>(
        index: &Option<NonZeroU32>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match index {
            Some(index) => index.get(),
            None => 0,
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<NonZeroU32>, D::Error> {
        Ok(NonZeroU32::new(u32::deserialize(deserializer)?))
    }
}

/// One required geometry operand in a dimension presentation frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignDimensionPresentationOperand {
    /// Indexed sketch geometry record.
    #[serde(deserialize_with = "deserialize_presentation_geometry_index")]
    pub geometry_record_index: NonZeroU32,
    /// Byte offset of `geometry_record_index`.
    pub geometry_reference_offset: u64,
    /// Source dimension-role code.
    pub role: u32,
    /// Byte offset of `role`.
    pub role_offset: u64,
}

fn deserialize_presentation_geometry_index<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<NonZeroU32, D::Error> {
    NonZeroU32::new(u32::deserialize(deserializer)?)
        .ok_or_else(|| serde::de::Error::custom("geometry_record_index must be nonzero"))
}

/// Paired `EntityGenesis` dimension frame carrying annotation geometry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignDimensionAnnotationFrameWire",
    into = "DesignDimensionAnnotationFrameWire"
)]
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
    pub class_tag: DesignClassTag,
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
    pub return_members: Vec<Located<NonZeroU32>>,
    /// Dynamic class tag of the paired indexed record.
    pub paired_class_tag: DesignClassTag,
    /// Byte offset of the paired indexed record header.
    pub paired_byte_offset: u64,
    /// Numeric design-entity suffix of the owning sketch.
    pub owner_reference: u32,
    /// Byte offset of `owner_reference`.
    pub owner_reference_offset: u64,
}

/// Paired `EntityGenesis` dimension frame carrying annotation geometry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
            return_members: wire
                .return_members
                .into_iter()
                .zip(wire.return_member_offsets)
                .map(|(value, offset)| {
                    let value = NonZeroU32::new(value)
                        .ok_or("return_members must contain nonzero geometry indices")?;
                    Ok(Located { value, offset })
                })
                .collect::<Result<_, Self::Error>>()?,
            id: wire.id,
            companion_record_index: wire.companion_record_index,
            governing_companion_record_index: wire.governing_companion_record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            record_index: wire.record_index,
            frame_length: wire.frame_length,
            operands: wire.operands,
            entity_genesis: wire.entity_genesis,
            annotation_bytes: wire.annotation_bytes,
            annotation_byte_offset: wire.annotation_byte_offset,
            governing_owner_record_index: wire.governing_owner_record_index,
            governing_owner_reference_offset: wire.governing_owner_reference_offset,
            paired_class_tag: wire.paired_class_tag.try_into()?,
            paired_byte_offset: wire.paired_byte_offset,
            owner_reference: wire.owner_reference,
            owner_reference_offset: wire.owner_reference_offset,
        })
    }
}
impl From<DesignDimensionAnnotationFrame> for DesignDimensionAnnotationFrameWire {
    fn from(value: DesignDimensionAnnotationFrame) -> Self {
        let (return_members, return_member_offsets) = value
            .return_members
            .into_iter()
            .map(|member| (member.value.get(), member.offset))
            .unzip();
        Self {
            return_members,
            return_member_offsets,
            id: value.id,
            companion_record_index: value.companion_record_index,
            governing_companion_record_index: value.governing_companion_record_index,
            byte_offset: value.byte_offset,
            class_tag: value.class_tag.into(),
            record_index: value.record_index,
            frame_length: value.frame_length,
            operands: value.operands,
            entity_genesis: value.entity_genesis,
            annotation_bytes: value.annotation_bytes,
            annotation_byte_offset: value.annotation_byte_offset,
            governing_owner_record_index: value.governing_owner_record_index,
            governing_owner_reference_offset: value.governing_owner_reference_offset,
            paired_class_tag: value.paired_class_tag.into(),
            paired_byte_offset: value.paired_byte_offset,
            owner_reference: value.owner_reference,
            owner_reference_offset: value.owner_reference_offset,
        }
    }
}

/// Paired Fusion presentation frame that directly identifies a dimension's
/// measured sketch geometry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignDimensionPresentationFrame {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the primary indexed record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: DesignClassTag,
    /// Source indexed-record identity.
    pub record_index: u32,
    /// Byte length from the primary through the paired-header boundary.
    pub frame_length: u64,
    /// Ordered typed sketch-geometry operands.
    pub operands: Vec<DesignDimensionPresentationOperand>,
    /// Opaque presentation bytes between the operand run and the paired
    /// `EntityTracking` header.
    pub presentation_bytes: Vec<u8>,
    /// Byte offset of `presentation_bytes`.
    pub presentation_byte_offset: u64,
    /// Dynamic class tag of the paired `EntityTracking` header.
    pub paired_class_tag: DesignClassTag,
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
#[serde(
    try_from = "DesignDimensionLocusGroupWire",
    into = "DesignDimensionLocusGroupWire"
)]
pub struct DesignDimensionLocusGroup {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Companion record containing this frame.
    pub companion_record_index: u32,
    /// Byte offset of the indexed record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: DesignClassTag,
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
    pub next_class_tag: DesignClassTag,
    /// Identity of the immediately following indexed record.
    pub next_record_index: u32,
    /// Byte offset of the immediately following indexed record.
    pub next_byte_offset: u64,
}

/// One typed geometry locus and its dimension-role code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        if wire.constraint_kinds != kinds {
            return Err("constraint_kinds must match state".into());
        }
        if u64::from(wire.unknown_constraint_bits) != unknown {
            return Err("unknown_constraint_bits must match state".into());
        }
        let loci = wire
            .loci
            .into_iter()
            .zip(
                wire.return_members
                    .into_iter()
                    .zip(wire.return_member_offsets),
            )
            .map(|(locus, (value, offset))| DesignDimensionLocus {
                geometry_record_index: locus.geometry_record_index,
                geometry_reference_offset: locus.geometry_reference_offset,
                role: locus.role,
                role_offset: locus.role_offset,
                returned: Located { value, offset },
            })
            .collect();
        Ok(Self {
            loci,
            id: wire.id,
            companion_record_index: wire.companion_record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            record_index: wire.record_index,
            frame_length: wire.frame_length,
            owner_reference: wire.owner_reference,
            owner_reference_offset: wire.owner_reference_offset,
            owner_role: wire.owner_role,
            owner_role_offset: wire.owner_role_offset,
            state: wire.state,
            state_offset: wire.state_offset,
            next_class_tag: wire.next_class_tag.try_into()?,
            next_record_index: wire.next_record_index,
            next_byte_offset: wire.next_byte_offset,
        })
    }
}
impl From<DesignDimensionLocusGroup> for DesignDimensionLocusGroupWire {
    // Output cardinalities are bounded by already-materialized input vectors.
    #[allow(clippy::disallowed_methods)]
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
        Self {
            loci,
            return_members,
            return_member_offsets,
            constraint_kinds,
            unknown_constraint_bits,
            id: value.id,
            companion_record_index: value.companion_record_index,
            byte_offset: value.byte_offset,
            class_tag: value.class_tag.into(),
            record_index: value.record_index,
            frame_length: value.frame_length,
            owner_reference: value.owner_reference,
            owner_reference_offset: value.owner_reference_offset,
            owner_role: value.owner_role,
            owner_role_offset: value.owner_role_offset,
            state: value.state,
            state_offset: value.state_offset,
            next_class_tag: value.next_class_tag.into(),
            next_record_index: value.next_record_index,
            next_byte_offset: value.next_byte_offset,
        }
    }
}

/// Typed sketch-container visibility bound to a Design sketch entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignSketchVisibilityWire",
    into = "DesignSketchVisibilityWire"
)]
pub struct DesignSketchVisibility {
    /// One-based ordinal among sketch Geometry members in the Design stream.
    pub stream_ordinal: NonZeroU32,
    stream_ordinal_offset: u64,
    /// Direct display visibility.
    pub visible: bool,
}

impl DesignSketchVisibility {
    pub fn new(
        stream_ordinal: NonZeroU32,
        stream_ordinal_offset: u64,
        visible: bool,
    ) -> Result<Self, String> {
        stream_ordinal_offset
            .checked_add(5)
            .ok_or("stream_ordinal_offset overflows visible_offset")?;
        Ok(Self {
            stream_ordinal,
            stream_ordinal_offset,
            visible,
        })
    }

    pub fn stream_ordinal_offset(&self) -> u64 {
        self.stream_ordinal_offset
    }

    pub fn visible_offset(&self) -> u64 {
        self.stream_ordinal_offset + 5
    }
}

#[derive(Serialize, Deserialize)]
struct DesignSketchVisibilityWire {
    stream_ordinal: u32,
    stream_ordinal_offset: u64,
    visible_offset: u64,
    visible: bool,
}

impl TryFrom<DesignSketchVisibilityWire> for DesignSketchVisibility {
    type Error = String;

    fn try_from(wire: DesignSketchVisibilityWire) -> Result<Self, Self::Error> {
        let value = Self::new(
            NonZeroU32::new(wire.stream_ordinal).ok_or("stream_ordinal must be nonzero")?,
            wire.stream_ordinal_offset,
            wire.visible,
        )?;
        if wire.visible_offset != value.visible_offset() {
            return Err("visible_offset must equal stream_ordinal_offset + 5".into());
        }
        Ok(value)
    }
}

impl From<DesignSketchVisibility> for DesignSketchVisibilityWire {
    fn from(value: DesignSketchVisibility) -> Self {
        Self {
            stream_ordinal: value.stream_ordinal.get(),
            stream_ordinal_offset: value.stream_ordinal_offset(),
            visible_offset: value.visible_offset(),
            visible: value.visible,
        }
    }
}

/// Local-to-model placement frame referenced by a Design sketch scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignSketchPlacementWire",
    into = "DesignSketchPlacementWire"
)]
pub struct DesignSketchPlacement {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Owning parameter scope, when the sketch has a parameter scope.
    pub scope_record_index: Option<u32>,
    /// Full Design entity id of the placed sketch.
    pub entity_id: DesignEntityId,
    /// Typed sketch-container visibility for the placed sketch entity.
    pub visibility: Option<DesignSketchVisibility>,
    /// Source dynamic three-digit ASCII primary class tag.
    pub class_tag: DesignClassTag,
    /// Shared logical record identity.
    pub record_index: u32,
    /// Source dynamic class tag of the paired header.
    pub paired_class_tag: DesignClassTag,
    /// Source layout and its checked placement matrix and byte extent.
    pub frame: DesignSketchFrame,
}

pub(crate) fn valid_sketch_transform(transform: &[[f64; 4]; 4]) -> bool {
    const EPSILON: f64 = 1.0e-10;
    if !transform.iter().flatten().all(|value| value.is_finite())
        || transform[3] != [0.0, 0.0, 0.0, 1.0]
    {
        return false;
    }
    let columns = [
        [transform[0][0], transform[1][0], transform[2][0]],
        [transform[0][1], transform[1][1], transform[2][1]],
        [transform[0][2], transform[1][2], transform[2][2]],
    ];
    for (ordinal, column) in columns.iter().enumerate() {
        let norm = column.iter().map(|value| value * value).sum::<f64>();
        if (norm - 1.0).abs() > EPSILON {
            return false;
        }
        for other in &columns[..ordinal] {
            let dot = column
                .iter()
                .zip(other)
                .map(|(left, right)| left * right)
                .sum::<f64>();
            if dot.abs() > EPSILON {
                return false;
            }
        }
    }
    true
}

/// A finite affine placement with orthonormal basis columns.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[[f64; 4]; 4]", into = "[[f64; 4]; 4]")]
pub struct SketchPlacementMatrix([[f64; 4]; 4]);

impl SketchPlacementMatrix {
    /// The identity placement.
    pub const IDENTITY: Self = Self(IDENTITY_MATRIX);
    /// The row-major matrix coefficients.
    pub fn rows(self) -> [[f64; 4]; 4] {
        self.0
    }
    /// The matrix rows in storage order.
    pub fn iter(&self) -> std::slice::Iter<'_, [f64; 4]> {
        self.0.iter()
    }
}

impl AsRef<[[f64; 4]; 4]> for SketchPlacementMatrix {
    fn as_ref(&self) -> &[[f64; 4]; 4] {
        &self.0
    }
}

impl std::ops::Index<usize> for SketchPlacementMatrix {
    type Output = [f64; 4];
    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

impl From<SketchPlacementMatrix> for [[f64; 4]; 4] {
    fn from(matrix: SketchPlacementMatrix) -> Self {
        matrix.0
    }
}

impl TryFrom<[[f64; 4]; 4]> for SketchPlacementMatrix {
    type Error = String;
    fn try_from(value: [[f64; 4]; 4]) -> Result<Self, Self::Error> {
        if valid_sketch_transform(&value) {
            Ok(Self(value))
        } else {
            Err("transform must be a finite affine matrix with orthonormal columns".into())
        }
    }
}

/// The matrix-bearing payload of each source placement layout.
#[derive(Debug, Clone, PartialEq)]
pub enum DesignSketchFrameForm {
    ScopeCompact,
    ScopeGenesisCompact,
    ScopeLegacy305(SketchPlacementMatrix),
    ScopeLegacy325(SketchPlacementMatrix),
    ScopeExplicit(SketchPlacementMatrix),
    ScopeGenesisExplicit(SketchPlacementMatrix),
    MemberCompact {
        paired_byte_offset: u64,
    },
    MemberExplicit {
        paired_byte_offset: u64,
        transform: SketchPlacementMatrix,
    },
}

/// A placement layout whose complete byte extent fits in its address space.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignSketchFrame {
    byte_offset: u64,
    form: DesignSketchFrameForm,
}

impl DesignSketchFrame {
    pub(crate) fn form(&self) -> &DesignSketchFrameForm {
        &self.form
    }
    pub(crate) fn new(byte_offset: u64, form: DesignSketchFrameForm) -> Result<Self, String> {
        let value = Self { byte_offset, form };
        byte_offset
            .checked_add(value.frame_length())
            .ok_or("byte_offset overflows the sketch frame extent")?;
        Ok(value)
    }
    fn frame_length(&self) -> u64 {
        match self.form {
            DesignSketchFrameForm::ScopeCompact => 201,
            DesignSketchFrameForm::ScopeGenesisCompact => 213,
            DesignSketchFrameForm::ScopeLegacy305(_) => 305,
            DesignSketchFrameForm::ScopeLegacy325(_) => 325,
            DesignSketchFrameForm::ScopeExplicit(_) => 329,
            DesignSketchFrameForm::ScopeGenesisExplicit(_) => 341,
            DesignSketchFrameForm::MemberCompact { .. } => 34,
            DesignSketchFrameForm::MemberExplicit { .. } => 162,
        }
    }
    fn transform(&self) -> &[[f64; 4]; 4] {
        match &self.form {
            DesignSketchFrameForm::ScopeCompact
            | DesignSketchFrameForm::ScopeGenesisCompact
            | DesignSketchFrameForm::MemberCompact { .. } => &IDENTITY_MATRIX,
            DesignSketchFrameForm::ScopeLegacy305(matrix)
            | DesignSketchFrameForm::ScopeLegacy325(matrix)
            | DesignSketchFrameForm::ScopeExplicit(matrix)
            | DesignSketchFrameForm::ScopeGenesisExplicit(matrix)
            | DesignSketchFrameForm::MemberExplicit {
                transform: matrix, ..
            } => &matrix.0,
        }
    }
    fn transform_offset(&self) -> Option<u64> {
        let relative = match self.form {
            DesignSketchFrameForm::ScopeCompact
            | DesignSketchFrameForm::ScopeGenesisCompact
            | DesignSketchFrameForm::MemberCompact { .. } => return None,
            DesignSketchFrameForm::ScopeLegacy305(_) | DesignSketchFrameForm::ScopeLegacy325(_) => {
                48
            }
            DesignSketchFrameForm::ScopeExplicit(_) => 55,
            DesignSketchFrameForm::ScopeGenesisExplicit(_) => 66,
            DesignSketchFrameForm::MemberExplicit { .. } => 22,
        };
        Some(self.byte_offset + relative)
    }
    fn paired_byte_offset(&self) -> u64 {
        match self.form {
            DesignSketchFrameForm::MemberCompact { paired_byte_offset }
            | DesignSketchFrameForm::MemberExplicit {
                paired_byte_offset, ..
            } => paired_byte_offset,
            _ => self.byte_offset + self.frame_length(),
        }
    }
}

impl DesignSketchPlacement {
    pub(crate) fn byte_offset(&self) -> u64 {
        self.frame.byte_offset
    }
    pub(crate) fn frame_length(&self) -> u64 {
        self.frame.frame_length()
    }
    pub(crate) fn transform(&self) -> &[[f64; 4]; 4] {
        self.frame.transform()
    }
    pub(crate) fn transform_offset(&self) -> Option<u64> {
        self.frame.transform_offset()
    }
    pub(crate) fn paired_byte_offset(&self) -> u64 {
        self.frame.paired_byte_offset()
    }
    pub(crate) fn member_run_head(&self) -> bool {
        matches!(
            self.frame.form,
            DesignSketchFrameForm::MemberCompact { .. }
                | DesignSketchFrameForm::MemberExplicit { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignSketchPlacementWire {
    /// Globally unique deterministic identifier for this native record.
    id: String,
    /// Owning parameter-scope record; absent when the sketch has no parameter
    /// scope. A localized Sketch scope can own a member-run head placement
    /// through record interval order without directly referencing it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scope_record_index: Option<u32>,
    /// Full Design entity id of the placed sketch.
    entity_id: String,
    /// Numeric suffix of `entity_id`.
    entity_suffix: u64,
    /// Typed sketch-container visibility for the placed sketch entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    visibility: Option<DesignSketchVisibility>,
    /// Byte offset of the primary indexed record header.
    byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    class_tag: String,
    /// Shared logical record identity.
    record_index: u32,
    /// Byte length from the primary header to the paired header.
    frame_length: u64,
    /// Row-major local-to-model affine transform.
    transform: [[f64; 4]; 4],
    /// Byte offset of the explicit 16-f64 matrix; absent for the compact identity form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transform_offset: Option<u64>,
    /// Per-file dynamic class tag of the paired header.
    paired_class_tag: String,
    /// Byte offset of the paired indexed record header.
    paired_byte_offset: u64,
    /// Whether this placement is the transform-carrying member-run head
    /// record named by the sketch entity's paired record rather than a
    /// parameter-scope placement frame.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    member_run_head: bool,
}

impl TryFrom<DesignSketchPlacementWire> for DesignSketchPlacement {
    type Error = String;
    fn try_from(wire: DesignSketchPlacementWire) -> Result<Self, Self::Error> {
        use DesignSketchFrameForm as Form;
        let entity_id = DesignEntityId::try_from(wire.entity_id)?;
        if entity_id.suffix() != wire.entity_suffix {
            return Err("entity_suffix disagrees with entity_id".into());
        }

        let form = match (wire.member_run_head, wire.frame_length) {
            (false, 201) => Form::ScopeCompact,
            (false, 213) => Form::ScopeGenesisCompact,
            (false, 305) => Form::ScopeLegacy305(wire.transform.try_into()?),
            (false, 325) => Form::ScopeLegacy325(wire.transform.try_into()?),
            (false, 329) => Form::ScopeExplicit(wire.transform.try_into()?),
            (false, 341) => Form::ScopeGenesisExplicit(wire.transform.try_into()?),
            (true, 34) => Form::MemberCompact {
                paired_byte_offset: wire.paired_byte_offset,
            },
            (true, 162) => Form::MemberExplicit {
                paired_byte_offset: wire.paired_byte_offset,
                transform: wire.transform.try_into()?,
            },
            _ => return Err("frame_length is invalid for member_run_head".into()),
        };
        let frame = DesignSketchFrame::new(wire.byte_offset, form)?;
        if frame
            .transform()
            .iter()
            .flatten()
            .zip(wire.transform.iter().flatten())
            .any(|(left, right)| left.to_bits() != right.to_bits())
        {
            return Err("transform disagrees with the compact frame identity".into());
        }
        if wire.transform_offset != frame.transform_offset() {
            return Err("transform_offset disagrees with the sketch frame layout".into());
        }
        if wire.paired_byte_offset != frame.paired_byte_offset() {
            return Err("paired_byte_offset disagrees with the sketch frame extent".into());
        }
        Ok(Self {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            entity_id,
            visibility: wire.visibility,
            class_tag: DesignClassTag::try_from(wire.class_tag)?,
            record_index: wire.record_index,
            paired_class_tag: DesignClassTag::try_from(wire.paired_class_tag)
                .map_err(|error| format!("paired_class_tag: {error}"))?,
            frame,
        })
    }
}

impl From<DesignSketchPlacement> for DesignSketchPlacementWire {
    fn from(value: DesignSketchPlacement) -> Self {
        let entity_suffix = value.entity_id.suffix();
        let byte_offset = value.byte_offset();
        let frame_length = value.frame_length();
        let transform = *value.transform();
        let transform_offset = value.transform_offset();
        let paired_byte_offset = value.paired_byte_offset();
        let member_run_head = value.member_run_head();
        Self {
            id: value.id,
            scope_record_index: value.scope_record_index,
            entity_id: value.entity_id.0,
            entity_suffix,
            visibility: value.visibility,
            byte_offset,
            class_tag: value.class_tag.into(),
            record_index: value.record_index,
            frame_length,
            transform,
            transform_offset,
            paired_class_tag: value.paired_class_tag.into(),
            paired_byte_offset,
            member_run_head,
        }
    }
}

/// Persistent-reference channel in the Design construction stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// A per-file dynamic class tag encoded as three ASCII digits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DesignClassTag(String);

impl TryFrom<String> for DesignClassTag {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_digit()) {
            Ok(Self(value))
        } else {
            Err("class_tag must contain three ASCII digits".into())
        }
    }
}
impl From<DesignClassTag> for String {
    fn from(tag: DesignClassTag) -> Self {
        tag.0
    }
}
impl DesignClassTag {
    /// Numeric value of the three-digit tag.
    pub(crate) fn code(&self) -> u32 {
        self.0
            .bytes()
            .fold(0, |value, digit| value * 10 + u32::from(digit - b'0'))
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
    /// Dynamic type ordinal: the three digits minus the 256 fixed-class base.
    pub(crate) fn dynamic_ordinal(&self) -> Option<usize> {
        self.0.parse::<usize>().ok()?.checked_sub(256)
    }
    pub(crate) fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

/// A construction-history edge selection that Fusion could not re-resolve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "LostEdgeReferenceWire", into = "LostEdgeReferenceWire")]
pub struct LostEdgeReference {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the unresolved record's indexed header.
    record_byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag of the unresolved record.
    pub class_tag: DesignClassTag,
    /// Source `BulkStream` record index of the unresolved edge selection.
    pub record_index: u32,
    /// Per-file dynamic class tag of the following indexed record.
    pub next_class_tag: DesignClassTag,
    /// Record index of the following indexed record.
    pub next_record_index: u32,
}

impl LostEdgeReference {
    pub(crate) fn new(
        id: String,
        record_byte_offset: u64,
        class_tag: String,
        record_index: u32,
        next_class_tag: String,
        next_record_index: u32,
    ) -> Result<Self, String> {
        record_byte_offset
            .checked_add(48)
            .ok_or("lost_edge_reference.record_byte_offset overflows the record extent")?;
        Ok(Self {
            id,
            record_byte_offset,
            class_tag: DesignClassTag::try_from(class_tag)?,
            record_index,
            next_class_tag: DesignClassTag::try_from(next_class_tag)
                .map_err(|error| format!("lost_edge_reference.next_class_tag: {error}"))?,
            next_record_index,
        })
    }
    pub(crate) fn record_byte_offset(&self) -> u64 {
        self.record_byte_offset
    }
    pub(crate) fn class_tag_offset(&self) -> u64 {
        self.record_byte_offset + 4
    }
    pub(crate) fn record_index_offset(&self) -> u64 {
        self.record_byte_offset + 7
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.record_byte_offset + 29
    }
    pub(crate) fn next_byte_offset(&self) -> u64 {
        self.record_byte_offset + 48
    }
}

/// A construction-history edge selection that Fusion could not re-resolve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct LostEdgeReferenceWire {
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

impl TryFrom<LostEdgeReferenceWire> for LostEdgeReference {
    type Error = String;
    fn try_from(wire: LostEdgeReferenceWire) -> Result<Self, Self::Error> {
        let record = Self::new(
            wire.id,
            wire.record_byte_offset,
            wire.class_tag,
            wire.record_index,
            wire.next_class_tag,
            wire.next_record_index,
        )?;
        if wire.class_tag_offset != record.class_tag_offset() {
            return Err(
                "lost_edge_reference.class_tag_offset disagrees with record_byte_offset".into(),
            );
        }
        if wire.record_index_offset != record.record_index_offset() {
            return Err(
                "lost_edge_reference.record_index_offset disagrees with record_byte_offset".into(),
            );
        }
        if wire.byte_offset != record.byte_offset() {
            return Err("lost_edge_reference.byte_offset disagrees with record_byte_offset".into());
        }
        if wire.next_byte_offset != record.next_byte_offset() {
            return Err(
                "lost_edge_reference.next_byte_offset disagrees with record_byte_offset".into(),
            );
        }
        Ok(record)
    }
}
impl From<LostEdgeReference> for LostEdgeReferenceWire {
    fn from(record: LostEdgeReference) -> Self {
        Self {
            record_byte_offset: record.record_byte_offset(),
            class_tag_offset: record.class_tag_offset(),
            record_index_offset: record.record_index_offset(),
            byte_offset: record.byte_offset(),
            next_byte_offset: record.next_byte_offset(),
            id: record.id,
            class_tag: record.class_tag.into(),
            record_index: record.record_index,
            next_class_tag: record.next_class_tag.into(),
            next_record_index: record.next_record_index,
        }
    }
}

/// A complete serialized visual-appearance identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DesignVisualToken(String);

impl TryFrom<String> for DesignVisualToken {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        crate::design::presentation::visual_token(&value)
            .ok_or("visual_guid must be a complete visual token")?;
        Ok(Self(value))
    }
}
impl From<DesignVisualToken> for String {
    fn from(value: DesignVisualToken) -> Self {
        value.0
    }
}
impl std::ops::Deref for DesignVisualToken {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Display for DesignVisualToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl DesignVisualToken {
    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

/// One Design `BulkStream` material assignment joining a design entity to visual assets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignMaterialAssignmentWire",
    into = "DesignMaterialAssignmentWire"
)]
pub struct DesignMaterialAssignment {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// ASM body key resolved through the Design body map.
    pub asm_body_key: u64,
    /// Byte offset of the body-map ASM key.
    pub asm_body_key_offset: u64,
    /// Byte offset of the body-map entity suffix.
    pub entity_suffix_offset: u64,
    /// UTF-16 design-entity id.
    pub entity_id: DesignEntityId,
    /// Byte offset of the UTF-16 entity-id code units.
    pub entity_id_offset: u64,
    /// Complete serialized visual token.
    pub visual_guid: DesignVisualToken,
    /// Byte offset of the UTF-16 visual-token code units.
    pub visual_guid_offset: u64,
    /// Physical-material token, when present.
    pub physical_token: Option<RecordedValue<String>>,
    /// Visual preset name, when present.
    pub visual_preset: Option<RecordedValue<String>>,
}

/// One Design `BulkStream` material assignment joining a design entity to visual assets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub visual_guid: DesignVisualToken,
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
        let entity_id = DesignEntityId::try_from(wire.entity_id)?;
        if entity_id.suffix() != wire.entity_suffix {
            return Err("entity_suffix disagrees with entity_id".into());
        }
        Ok(Self {
            id: wire.id,
            asm_body_key: wire.asm_body_key,
            asm_body_key_offset: wire.asm_body_key_offset,
            entity_suffix_offset: wire.entity_suffix_offset,
            entity_id,
            entity_id_offset: wire.entity_id_offset,
            visual_guid: wire.visual_guid,
            visual_guid_offset: wire.visual_guid_offset,
            physical_token: RecordedValue::from_wire(
                wire.physical_token,
                wire.physical_token_offset,
                "physical_token",
            )?,
            visual_preset: RecordedValue::from_wire(
                wire.visual_preset,
                wire.visual_preset_offset,
                "visual_preset",
            )?,
        })
    }
}

impl From<DesignMaterialAssignment> for DesignMaterialAssignmentWire {
    fn from(value: DesignMaterialAssignment) -> Self {
        Self {
            id: value.id,
            asm_body_key: value.asm_body_key,
            asm_body_key_offset: value.asm_body_key_offset,
            entity_suffix: value.entity_id.suffix(),
            entity_suffix_offset: value.entity_suffix_offset,
            entity_id: value.entity_id.0,
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

#[derive(Debug, Clone, PartialEq)]
struct NativeRecordId {
    text: String,
    stream_end: usize,
}

impl NativeRecordId {
    fn try_new(text: String, kind: &str, key: impl std::fmt::Display) -> Result<Self, String> {
        let stream = crate::ids::native_stream(&text).ok_or("id must contain a native stream")?;
        if text != format!("{stream}:{kind}#{key}") {
            return Err(format!("id must identify {kind} at {key}"));
        }
        let stream_end = stream.len();
        Ok(Self { text, stream_end })
    }
    fn stream(&self) -> &str {
        &self.text[..self.stream_end]
    }
}

/// JSON configuration payload stored in a Fusion design-configuration entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignConfigurationWire", into = "DesignConfigurationWire")]
pub struct DesignConfiguration {
    /// Stable identity derived from the ZIP entry name.
    id: String,
    /// Complete ZIP entry name used for native regeneration.
    entry_name: String,
    /// Native configuration entry family.
    pub kind: DesignConfigurationKind,
    /// Variant names in serialized object-member order.
    #[serde(default)]
    pub variant_order: Vec<String>,
    /// Complete decoded JSON payload, including unrecognized fields.
    pub payload: serde_json::Value,
}

/// Serialized configuration identity and payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignConfigurationWire {
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

impl TryFrom<DesignConfigurationWire> for DesignConfiguration {
    type Error = String;
    fn try_from(wire: DesignConfigurationWire) -> Result<Self, String> {
        let record = Self::new(wire.entry_name, wire.kind, wire.variant_order, wire.payload);
        if wire.id != record.id {
            return Err("configuration.id must identify entry_name".into());
        }
        Ok(record)
    }
}
impl From<DesignConfiguration> for DesignConfigurationWire {
    fn from(value: DesignConfiguration) -> Self {
        Self {
            id: value.id,
            entry_name: value.entry_name,
            kind: value.kind,
            variant_order: value.variant_order,
            payload: value.payload,
        }
    }
}
impl DesignConfiguration {
    /// Constructs a configuration with the identity of its entry name.
    pub(crate) fn new(
        entry_name: String,
        kind: DesignConfigurationKind,
        variant_order: Vec<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: crate::ids::configuration_entry_id(&entry_name),
            entry_name,
            kind,
            variant_order,
            payload,
        }
    }

    /// Returns the admitted native identity.
    pub(crate) fn id(&self) -> &String {
        &self.id
    }
    /// Returns the configuration entry name.
    pub(crate) fn entry_name(&self) -> &String {
        &self.entry_name
    }
}

/// Native Fusion design-configuration entry family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[serde(try_from = "SegmentTypeWire", into = "SegmentTypeWire")]
pub struct SegmentType {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of this type-table entry in its `MetaStream`.
    pub byte_offset: u64,
    /// GUID naming this entry's record type. Class tags are segment-local, so
    /// this GUID is the only discriminator that is stable across files.
    pub type_guid: DesignRelaxedGuidText,
    /// Byte offset of the type-GUID bytes in the `MetaStream`.
    pub type_guid_offset: u64,
    /// Base GUID field and location; its value is `None` for an explicit empty root GUID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_type_guid: Option<RecordedValue<Option<DesignRelaxedGuidText>>>,
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
struct SegmentTypeWire {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of this type-table entry in its `MetaStream`.
    pub byte_offset: u64,
    /// GUID naming this entry's record type. Class tags are segment-local, so
    /// this GUID is the only discriminator that is stable across files.
    pub type_guid: DesignRelaxedGuidText,
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
    /// Nonempty `base_type_guid` text outside the relaxed GUID domain is not decoder-producible and is rejected deliberately.
    fn try_from(wire: SegmentTypeWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            type_guid: wire.type_guid,
            type_guid_offset: wire.type_guid_offset,
            version: wire.version,
            version_offset: wire.version_offset,
            module: wire.module,
            entities: ReferenceRun::from_columns(
                wire.entity_ids,
                wire.entity_id_offsets,
                "entity_ids/entity_id_offsets",
            )?,
            base_type_guid: RecordedValue::from_wire(
                wire.base_type_guid
                    .map(|guid| {
                        if guid.is_empty() {
                            Ok(None)
                        } else {
                            DesignRelaxedGuidText::try_from(guid)
                                .map(Some)
                                .map_err(|error| format!("base_type_guid: {error}"))
                        }
                    })
                    .transpose()?,
                wire.base_type_guid_offset,
                "base_type_guid",
            )?,
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
            base_type_guid: value
                .base_type_guid
                .map(|field| field.value.map(String::from).unwrap_or_default()),
        }
    }
}

/// Source timeline frame with bounded, ordered reference locations.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignTimelineFrame {
    byte_offset: u64,
    frame_length: u64,
    context_record_index_offset: u64,
    item_count_offset: u64,
    items: Vec<Located<u64>>,
}

impl DesignTimelineFrame {
    pub fn new(
        byte_offset: u64,
        frame_length: u64,
        context_record_index_offset: u64,
        item_count_offset: u64,
        items: Vec<Located<u64>>,
    ) -> Result<Self, String> {
        let end = byte_offset
            .checked_add(frame_length)
            .ok_or("timeline.frame_length overflows byte_offset")?;
        if context_record_index_offset <= byte_offset
            || context_record_index_offset
                .checked_add(10)
                .is_none_or(|after| after > item_count_offset)
        {
            return Err("timeline.context_record_index_offset is outside the context span".into());
        }
        if item_count_offset
            .checked_add(4)
            .is_none_or(|after| after > end)
        {
            return Err("timeline.item_count_offset is outside the frame".into());
        }
        if items
            .first()
            .is_some_and(|item| item_count_offset.checked_add(5) != Some(item.offset))
        {
            return Err(
                "timeline.item_record_index_offsets must start after the item count".into(),
            );
        }
        if items.windows(2).any(|pair| {
            pair[0]
                .offset
                .checked_add(11)
                .is_none_or(|minimum| pair[1].offset < minimum)
        }) || items
            .iter()
            .any(|item| item.offset.checked_add(10).is_none_or(|after| after > end))
        {
            return Err("timeline.item_record_index_offsets overlap or exceed the frame".into());
        }
        let mut unique = std::collections::HashSet::with_capacity(items.len());
        if items
            .iter()
            .any(|item| item.value == 0 || !unique.insert(item.value))
        {
            return Err("timeline.item_record_indices must be nonzero and unique".into());
        }
        Ok(Self {
            byte_offset,
            frame_length,
            context_record_index_offset,
            item_count_offset,
            items,
        })
    }
    pub fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    #[cfg(test)]
    pub fn frame_length(&self) -> u64 {
        self.frame_length
    }
    pub fn items(&self) -> &[Located<u64>] {
        &self.items
    }

    #[cfg(test)]
    pub fn test_items(byte_offset: u64, items: Vec<Located<u64>>) -> Self {
        let items = items
            .into_iter()
            .enumerate()
            .map(|(index, item)| Located {
                value: item.value,
                offset: byte_offset + 35 + index as u64 * 11,
            })
            .collect::<Vec<_>>();
        Self::new(
            byte_offset,
            34 + items.len() as u64 * 11,
            byte_offset + 20,
            byte_offset + 30,
            items,
        )
        .unwrap()
    }
}

/// Counted Design timeline-item list that carries authored feature order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignFeatureTimelineWire",
    into = "DesignFeatureTimelineWire"
)]
pub struct DesignFeatureTimeline {
    /// Globally unique deterministic identifier for this native record.
    id: NativeRecordId,
    segment_end: usize,
    /// Checked source frame and ordered item locations.
    frame: DesignTimelineFrame,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: DesignClassTag,
    /// Design entity identity of the timeline record.
    pub record_index: std::num::NonZeroU64,
    /// Zero-based position in the `MetaStream` timeline-record list.
    pub source_ordinal: u32,
    /// Same-segment context record referenced before the scope list.
    pub context_record_index: std::num::NonZeroU64,
}

impl DesignFeatureTimeline {
    /// Returns the admitted native identity.
    pub(crate) fn id(&self) -> &String {
        &self.id.text
    }
    /// Returns the timeline source frame.
    pub(crate) fn frame(&self) -> &DesignTimelineFrame {
        &self.frame
    }
    /// Returns the Design segment encoded in the identity.
    pub(crate) fn segment(&self) -> &str {
        &self.id.text[..self.segment_end]
    }
    /// Admits a record whose identity matches its source location.
    pub(crate) fn try_new(
        id: String,
        frame: DesignTimelineFrame,
        class_tag: DesignClassTag,
        record_index: std::num::NonZeroU64,
        source_ordinal: u32,
        context_record_index: std::num::NonZeroU64,
    ) -> Result<Self, String> {
        let id = NativeRecordId::try_new(id, "design-feature-timeline", frame.byte_offset())?;
        let segment_end = crate::ids::design_segment(&id.text)
            .ok_or("timeline.id must contain a Design segment")?
            .len();
        Ok(Self {
            id,
            segment_end,
            frame,
            class_tag,
            record_index,
            source_ordinal,
            context_record_index,
        })
    }
}

/// Counted Design timeline-item list that carries authored feature order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
            return Err(
                "item_record_indices and item_record_index_offsets must have equal lengths".into(),
            );
        }
        let items = wire
            .item_record_indices
            .into_iter()
            .zip(wire.item_record_index_offsets)
            .map(|(value, offset)| Located { value, offset })
            .collect();
        Self::try_new(
            wire.id,
            DesignTimelineFrame::new(
                wire.byte_offset,
                wire.frame_length,
                wire.context_record_index_offset,
                wire.item_count_offset,
                items,
            )?,
            DesignClassTag::try_from(wire.class_tag)?,
            std::num::NonZeroU64::new(wire.record_index)
                .ok_or("timeline.record_index must be nonzero")?,
            wire.source_ordinal,
            std::num::NonZeroU64::new(wire.context_record_index)
                .ok_or("timeline.context_record_index must be nonzero")?,
        )
    }
}
impl From<DesignFeatureTimeline> for DesignFeatureTimelineWire {
    fn from(value: DesignFeatureTimeline) -> Self {
        let (item_record_indices, item_record_index_offsets) = value
            .frame
            .items
            .into_iter()
            .map(|item| (item.value, item.offset))
            .unzip();
        Self {
            item_record_indices,
            item_record_index_offsets,
            id: value.id.text,
            byte_offset: value.frame.byte_offset,
            class_tag: value.class_tag.into(),
            record_index: value.record_index.get(),
            source_ordinal: value.source_ordinal,
            frame_length: value.frame.frame_length,
            context_record_index: value.context_record_index.get(),
            context_record_index_offset: value.frame.context_record_index_offset,
            item_count_offset: value.frame.item_count_offset,
        }
    }
}

/// Self-validating entity-bound header in the Design `BulkStream`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignEntityHeaderWire", into = "DesignEntityHeaderWire")]
pub struct DesignEntityHeader {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of this entity header in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Full UTF-16LE-decoded design-entity id string for this header.
    pub entity_id: DesignEntityId,
    /// Source per-file dynamic three-digit ASCII class tag naming this header's record type.
    pub class_tag: DesignClassTag,
    /// Whether the flag-selected four-byte optional slot is present.
    pub optional_slot_present: bool,
    /// Module registration and its sketch-owned data.
    pub registration: DesignEntityRegistration,
}

/// A sketch header's located reference-list slot.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchHeaderReferences {
    /// Owning record, absent for the no-base-record sentinel.
    pub record_reference: Option<u32>,
    /// Byte offset of the owning-record slot.
    pub record_reference_offset: u64,
    /// Located references in the counted list.
    pub references: Vec<Located<u32>>,
}

/// Module registration with data owned only by sketch headers.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignEntityRegistration(DesignEntityRegistrationKind);

#[derive(Debug, Clone, PartialEq)]
enum DesignEntityRegistrationKind {
    Other(Option<String>),
    Sketch {
        references: Option<SketchHeaderReferences>,
        members: ReferenceRun<u32>,
    },
}

impl DesignEntityRegistration {
    /// Construct a module registration and its sketch data.
    pub fn new(
        module: Option<String>,
        references: Option<SketchHeaderReferences>,
        members: ReferenceRun<u32>,
    ) -> Result<Self, String> {
        if module.as_deref() == Some(DESIGN_MODULE_SKETCH) {
            Ok(Self(DesignEntityRegistrationKind::Sketch {
                references,
                members,
            }))
        } else if references.is_some() || !members.is_empty() {
            Err("module must be MSketch for sketch references or members".into())
        } else {
            Ok(Self(DesignEntityRegistrationKind::Other(module)))
        }
    }
}

impl DesignEntityHeader {
    /// Declared reference count for a present sketch reference list.
    pub fn declared_reference_count(&self) -> Option<usize> {
        self.sketch_references().map(|list| list.references.len())
    }

    /// Registered module name.
    pub fn module(&self) -> Option<&str> {
        match &self.registration.0 {
            DesignEntityRegistrationKind::Other(module) => module.as_deref(),
            DesignEntityRegistrationKind::Sketch { .. } => Some(DESIGN_MODULE_SKETCH),
        }
    }

    /// Located sketch reference-list slot.
    pub fn sketch_references(&self) -> Option<&SketchHeaderReferences> {
        match &self.registration.0 {
            DesignEntityRegistrationKind::Sketch { references, .. } => references.as_ref(),
            DesignEntityRegistrationKind::Other(_) => None,
        }
    }

    /// Mutable located sketch reference-list slot.
    pub fn sketch_references_mut(&mut self) -> Option<&mut SketchHeaderReferences> {
        match &mut self.registration.0 {
            DesignEntityRegistrationKind::Sketch { references, .. } => references.as_mut(),
            DesignEntityRegistrationKind::Other(_) => None,
        }
    }

    /// Referenced record indices.
    pub fn reference_values(&self) -> impl Iterator<Item = &u32> {
        self.sketch_references()
            .into_iter()
            .flat_map(|list| list.references.iter().map(|row| &row.value))
    }

    /// Member record indices.
    pub fn member_values(&self) -> impl Iterator<Item = &u32> {
        match &self.registration.0 {
            DesignEntityRegistrationKind::Sketch { members, .. } => Some(members),
            DesignEntityRegistrationKind::Other(_) => None,
        }
        .into_iter()
        .flat_map(ReferenceRun::values)
    }

    /// Whether the entity belongs to the sketch module.
    pub fn in_sketch_module(&self) -> bool {
        matches!(
            self.registration.0,
            DesignEntityRegistrationKind::Sketch { .. }
        )
    }
}

/// Self-validating entity-bound header in the Design `BulkStream`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Header references without paired `record_reference_offset` and `declared_reference_count` metadata are not decoder-producible and are rejected deliberately.
    fn try_from(wire: DesignEntityHeaderWire) -> Result<Self, Self::Error> {
        if wire
            .declared_reference_count
            .is_some_and(|count| count != wire.reference_indices.len())
        {
            return Err("declared_reference_count must match reference_indices".into());
        }
        let entity_id = DesignEntityId::try_from(wire.entity_id)?;
        if entity_id.suffix() != wire.entity_suffix {
            return Err("entity_suffix disagrees with entity_id".into());
        }
        let references = match (wire.record_reference_offset, wire.declared_reference_count) {
            (Some(offset), Some(_)) => {
                if wire.reference_indices.len() != wire.reference_offsets.len() {
                    return Err("reference_offsets must locate every reference_indices entry".into());
                }
                Some(SketchHeaderReferences {
                    record_reference: wire.record_reference,
                    record_reference_offset: offset,
                    references: wire.reference_indices.into_iter().zip(wire.reference_offsets)
                        .map(|(value, offset)| Located { value, offset }).collect(),
                })
            }
            (None, None) if wire.record_reference.is_none() && wire.reference_indices.is_empty() && wire.reference_offsets.is_empty() => None,
            _ => return Err("record_reference_offset and declared_reference_count must accompany reference_indices and record_reference".into()),
        };
        let members = ReferenceRun::from_columns(
            wire.member_indices,
            wire.member_offsets,
            "member_indices/member_offsets",
        )?;
        Ok(Self {
            registration: DesignEntityRegistration::new(wire.module, references, members)?,
            id: wire.id,
            byte_offset: wire.byte_offset,
            entity_id,
            class_tag: DesignClassTag::try_from(wire.class_tag)?,
            optional_slot_present: wire.optional_slot_present,
        })
    }
}

impl From<DesignEntityHeader> for DesignEntityHeaderWire {
    fn from(header: DesignEntityHeader) -> Self {
        let declared_reference_count = header.declared_reference_count();
        let (module, references, members) = match header.registration.0 {
            DesignEntityRegistrationKind::Other(module) => {
                (module, None, ReferenceRun::unlocated(Vec::new()))
            }
            DesignEntityRegistrationKind::Sketch {
                references,
                members,
            } => (Some(DESIGN_MODULE_SKETCH.to_owned()), references, members),
        };
        let (record_reference, record_reference_offset, reference_indices, reference_offsets) =
            match references {
                Some(list) => {
                    let (values, offsets) = list
                        .references
                        .into_iter()
                        .map(|row| (row.value, row.offset))
                        .unzip();
                    (
                        list.record_reference,
                        Some(list.record_reference_offset),
                        values,
                        offsets,
                    )
                }
                None => (None, None, Vec::new(), Vec::new()),
            };
        let (member_indices, member_offsets) = members.into_wire();
        Self {
            declared_reference_count,
            reference_indices,
            reference_offsets,
            member_indices,
            member_offsets,
            id: header.id,
            byte_offset: header.byte_offset,
            entity_suffix: header.entity_id.suffix(),
            entity_id: header.entity_id.0,
            class_tag: header.class_tag.into(),
            optional_slot_present: header.optional_slot_present,
            module,
            record_reference,
            record_reference_offset,
        }
    }
}

/// Exact identity and source extent of one indexed Design mesh record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignMeshRecordIdentityWire",
    into = "DesignMeshRecordIdentityWire"
)]
pub struct DesignMeshRecordIdentity {
    class_tag: DesignClassTag,
    record_index: std::num::NonZeroU32,
    byte_offset: u64,
    frame_length: u64,
}

impl DesignMeshRecordIdentity {
    pub fn new(
        class_tag: DesignClassTag,
        record_index: u32,
        byte_offset: u64,
        frame_length: u64,
    ) -> Result<Self, String> {
        let record_index =
            std::num::NonZeroU32::new(record_index).ok_or("mesh record_index must be nonzero")?;
        if frame_length < 11 {
            return Err("mesh frame_length must contain the indexed header".into());
        }
        byte_offset
            .checked_add(frame_length)
            .ok_or("mesh frame_length overflows byte_offset")?;
        Ok(Self {
            class_tag,
            record_index,
            byte_offset,
            frame_length,
        })
    }
    pub fn class_tag(&self) -> &DesignClassTag {
        &self.class_tag
    }
    pub fn record_index(&self) -> u32 {
        self.record_index.get()
    }
    pub fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    pub fn frame_length(&self) -> u64 {
        self.frame_length
    }
}

/// Exact identity and source extent of one indexed Design mesh record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DesignMeshRecordIdentityWire {
    /// Source per-file dynamic three-digit ASCII class tag.
    class_tag: String,
    /// Stream-local indexed-record identity.
    record_index: u32,
    /// Byte offset of the indexed header in the Design `BulkStream`.
    byte_offset: u64,
    /// Complete primary or nested record length in bytes.
    frame_length: u64,
}

impl TryFrom<DesignMeshRecordIdentityWire> for DesignMeshRecordIdentity {
    type Error = String;
    fn try_from(wire: DesignMeshRecordIdentityWire) -> Result<Self, Self::Error> {
        Self::new(
            DesignClassTag::try_from(wire.class_tag)?,
            wire.record_index,
            wire.byte_offset,
            wire.frame_length,
        )
    }
}
impl From<DesignMeshRecordIdentity> for DesignMeshRecordIdentityWire {
    fn from(record: DesignMeshRecordIdentity) -> Self {
        Self {
            class_tag: record.class_tag.into(),
            record_index: record.record_index.get(),
            byte_offset: record.byte_offset,
            frame_length: record.frame_length,
        }
    }
}

/// An indexed mesh record with a fixed byte length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignMeshFixedRecord<const LENGTH: u64> {
    class_tag: DesignClassTag,
    record_index: std::num::NonZeroU32,
    byte_offset: u64,
}
impl<const LENGTH: u64> DesignMeshFixedRecord<LENGTH> {
    pub fn record_index(&self) -> u32 {
        self.record_index.get()
    }
    pub fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
}
impl<const LENGTH: u64> TryFrom<DesignMeshRecordIdentity> for DesignMeshFixedRecord<LENGTH> {
    type Error = String;
    fn try_from(record: DesignMeshRecordIdentity) -> Result<Self, Self::Error> {
        if record.frame_length() != LENGTH {
            return Err(format!("record.frame_length must be {LENGTH}"));
        }
        Ok(Self {
            class_tag: record.class_tag,
            record_index: record.record_index,
            byte_offset: record.byte_offset,
        })
    }
}
impl<const LENGTH: u64> From<DesignMeshFixedRecord<LENGTH>> for DesignMeshRecordIdentity {
    fn from(record: DesignMeshFixedRecord<LENGTH>) -> Self {
        Self {
            class_tag: record.class_tag,
            record_index: record.record_index,
            byte_offset: record.byte_offset,
            frame_length: LENGTH,
        }
    }
}

/// A hyphenated hexadecimal GUID with its original letter case.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DesignGuidText(String);

impl DesignGuidText {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for DesignGuidText {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if !crate::bytes::is_guid_hyphenated(&value) {
            return Err("GUID must be 36 hyphenated hexadecimal characters".into());
        }
        Ok(Self(value))
    }
}
impl From<DesignGuidText> for String {
    fn from(value: DesignGuidText) -> Self {
        value.0
    }
}

/// A relaxed GUID with its original text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DesignRelaxedGuidText(String);

impl DesignRelaxedGuidText {
    /// The original GUID text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for DesignRelaxedGuidText {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if !crate::bytes::is_guid_relaxed(&value) {
            return Err(
                "GUID must be 36 through 38 alphanumeric, hyphen, or underscore characters".into(),
            );
        }
        Ok(Self(value))
    }
}
impl From<DesignRelaxedGuidText> for String {
    fn from(value: DesignRelaxedGuidText) -> Self {
        value.0
    }
}

/// One texture resource owned by a Design mesh feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignMeshTextureResource {
    /// Zero-based position in the serialized flags map.
    pub ordinal: u32,
    /// Stable resource GUID used as the key in both texture maps.
    pub resource_guid: DesignGuidText,
    /// Opaque resource flags retained without reinterpretation.
    pub flags: u32,
    /// Zero-based position of the same GUID in the serialized filename map.
    pub filename_ordinal: u32,
    /// Filename record joined to its archive entry.
    pub file: DesignMeshTextureFile,
    /// Neutral embedded asset projected from the matching archive entry.
    pub asset: AssetId,
}

/// A filename record and the archive entry with its exact basename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignMeshTextureFile {
    record: DesignMeshRecordIdentity,
    archive_entry_name: String,
}

impl DesignMeshTextureFile {
    pub fn new(
        record: DesignMeshRecordIdentity,
        filename: &str,
        archive_entry_name: String,
    ) -> Result<Self, String> {
        let value = Self {
            record,
            archive_entry_name,
        };
        if filename.is_empty() || value.filename() != filename {
            return Err("archive_entry_name basename must match nonempty filename".into());
        }
        let byte_length = u64::try_from(filename.encode_utf16().count())
            .ok()
            .and_then(|units| units.checked_mul(2))
            .and_then(|bytes| {
                bytes.checked_add(crate::layout::paramesh_texture_filename_prefix::LEN as u64)
            });
        if byte_length != Some(value.record.frame_length()) {
            return Err("filename_record.frame_length must contain exactly filename".into());
        }
        Ok(value)
    }
    pub fn record(&self) -> &DesignMeshRecordIdentity {
        &self.record
    }
    pub fn filename(&self) -> &str {
        self.archive_entry_name
            .rsplit_once('/')
            .map_or(self.archive_entry_name.as_str(), |(_, basename)| basename)
    }
    pub fn archive_entry_name(&self) -> &str {
        &self.archive_entry_name
    }
    pub fn filename_offset(&self) -> u64 {
        self.record.byte_offset() + crate::layout::paramesh_texture_filename_prefix::LEN as u64
    }
}

/// One texture resource owned by a Design mesh feature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DesignMeshTextureResourceWire {
    /// Zero-based position in the serialized flags map.
    ordinal: u32,
    /// Stable resource GUID used as the key in both texture maps.
    resource_guid: DesignGuidText,
    /// Byte offset of the flags-map GUID payload.
    flags_guid_offset: u64,
    /// Opaque resource flags retained without reinterpretation.
    flags: u32,
    /// Byte offset of `flags`.
    flags_offset: u64,
    /// Zero-based position of the same GUID in the serialized filename map.
    filename_ordinal: u32,
    /// Byte offset of the filename-map GUID payload.
    filename_guid_offset: u64,
    /// Record storing the archive-entry basename.
    filename_record: DesignMeshRecordIdentity,
    /// Byte offset of the filename-record reference.
    filename_record_reference_offset: u64,
    /// Archive-entry basename stored by `filename_record`.
    filename: String,
    /// Byte offset of the UTF-16LE filename code units.
    filename_offset: u64,
    /// Complete matching archive-entry name.
    archive_entry_name: String,
    /// Neutral embedded asset projected from the matching archive entry.
    asset: AssetId,
}

const MESH_TEXTURE_FLAGS_ENTRY_BYTES: u64 = 4 + 36 + 4;
const MESH_TEXTURE_FILENAME_ENTRY_BYTES: u64 = 4 + 36 + 11;

/// Texture resources with complete flags and filename ordinal permutations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignMeshTextureTable {
    record: DesignMeshRecordIdentity,
    resources: Vec<DesignMeshTextureResource>,
}
impl DesignMeshTextureTable {
    pub fn new(
        record: DesignMeshRecordIdentity,
        resources: Vec<DesignMeshTextureResource>,
    ) -> Result<Self, String> {
        let count =
            u32::try_from(resources.len()).map_err(|_| "textures exceeds the u32 map count")?;
        let expected = crate::layout::paramesh_texture_table_prefix::LEN as u64
            + 4
            + u64::from(count)
                * (MESH_TEXTURE_FLAGS_ENTRY_BYTES + MESH_TEXTURE_FILENAME_ENTRY_BYTES);
        if record.frame_length() != expected {
            return Err(
                "texture_table_record.frame_length must contain exactly both texture maps".into(),
            );
        }
        let mut flags = std::collections::HashSet::new();
        let mut filenames = std::collections::HashSet::new();
        let mut guids = std::collections::HashSet::new();
        for resource in &resources {
            if resource.ordinal >= count || !flags.insert(resource.ordinal) {
                return Err("textures.ordinal must be a complete map permutation".into());
            }
            if resource.filename_ordinal >= count || !filenames.insert(resource.filename_ordinal) {
                return Err("textures.filename_ordinal must be a complete map permutation".into());
            }
            if !guids.insert(resource.resource_guid.as_str().to_ascii_uppercase()) {
                return Err("textures.resource_guid must be unique ignoring letter case".into());
            }
        }
        Ok(Self { record, resources })
    }
    pub fn record(&self) -> &DesignMeshRecordIdentity {
        &self.record
    }
    pub fn resources(&self) -> &[DesignMeshTextureResource] {
        &self.resources
    }
    /// Borrow resources in the serialized flags-map order.
    pub fn resources_in_flags_order(&self) -> Vec<&DesignMeshTextureResource> {
        let mut resources = self.resources.iter().collect::<Vec<_>>();
        resources.sort_by_key(|resource| resource.ordinal);
        resources
    }
    fn flags_count_offset(&self) -> u64 {
        self.record.byte_offset()
            + crate::layout::paramesh_texture_table_prefix::FLAGS_MAP_COUNT as u64
    }
    fn filename_count_offset(&self) -> u64 {
        self.record.byte_offset()
            + crate::layout::paramesh_texture_table_prefix::LEN as u64
            + MESH_TEXTURE_FLAGS_ENTRY_BYTES * self.resources.len() as u64
    }
    // Output cardinalities are bounded by already-materialized input vectors.
    #[allow(clippy::disallowed_methods)]
    fn from_wire(
        record: DesignMeshRecordIdentity,
        flags_count_offset: u64,
        filename_count_offset: u64,
        rows: Vec<DesignMeshTextureResourceWire>,
    ) -> Result<Self, String> {
        let count = u32::try_from(rows.len()).map_err(|_| "textures exceeds the u32 map count")?;
        let flags_start = record
            .byte_offset()
            .checked_add(crate::layout::paramesh_texture_table_prefix::LEN as u64);
        let filenames_start = flags_start.and_then(|start| {
            start.checked_add(MESH_TEXTURE_FLAGS_ENTRY_BYTES * u64::from(count) + 4)
        });
        let mut resources = Vec::with_capacity(rows.len());
        for row in rows {
            let flags_guid = flags_start.and_then(|start| {
                start.checked_add(MESH_TEXTURE_FLAGS_ENTRY_BYTES * u64::from(row.ordinal) + 4)
            });
            let filename_guid = filenames_start.and_then(|start| {
                start.checked_add(
                    MESH_TEXTURE_FILENAME_ENTRY_BYTES * u64::from(row.filename_ordinal) + 4,
                )
            });
            if flags_guid != Some(row.flags_guid_offset)
                || flags_guid.and_then(|offset| offset.checked_add(36)) != Some(row.flags_offset)
            {
                return Err(
                    "flags_guid_offset/flags_offset must match the texture map ordinal".into(),
                );
            }
            if filename_guid != Some(row.filename_guid_offset)
                || filename_guid.and_then(|offset| offset.checked_add(36))
                    != Some(row.filename_record_reference_offset)
            {
                return Err("filename_guid_offset/filename_record_reference_offset must match the texture map ordinal".into());
            }
            let file = DesignMeshTextureFile::new(
                row.filename_record,
                &row.filename,
                row.archive_entry_name,
            )?;
            if file.filename_offset() != row.filename_offset {
                return Err("filename_offset must follow filename_record header".into());
            }
            resources.push(DesignMeshTextureResource {
                ordinal: row.ordinal,
                resource_guid: row.resource_guid,
                flags: row.flags,
                filename_ordinal: row.filename_ordinal,
                file,
                asset: row.asset,
            });
        }
        let table = Self::new(record, resources)?;
        if table.flags_count_offset() != flags_count_offset
            || table.filename_count_offset() != filename_count_offset
        {
            return Err("texture_flags_count_offset/texture_filename_count_offset must match the texture maps".into());
        }
        Ok(table)
    }
    fn into_wire(
        self,
    ) -> (
        DesignMeshRecordIdentity,
        u64,
        u64,
        Vec<DesignMeshTextureResourceWire>,
    ) {
        let flags_count_offset = self.flags_count_offset();
        let filename_count_offset = self.filename_count_offset();
        let flags_start =
            self.record.byte_offset() + crate::layout::paramesh_texture_table_prefix::LEN as u64;
        let filenames_start = filename_count_offset + 4;
        let rows = self
            .resources
            .into_iter()
            .map(|value| {
                let flags_guid_offset =
                    flags_start + MESH_TEXTURE_FLAGS_ENTRY_BYTES * u64::from(value.ordinal) + 4;
                let filename_guid_offset = filenames_start
                    + MESH_TEXTURE_FILENAME_ENTRY_BYTES * u64::from(value.filename_ordinal)
                    + 4;
                DesignMeshTextureResourceWire {
                    ordinal: value.ordinal,
                    resource_guid: value.resource_guid,
                    flags_guid_offset,
                    flags: value.flags,
                    flags_offset: flags_guid_offset + 36,
                    filename_ordinal: value.filename_ordinal,
                    filename_guid_offset,
                    filename_record_reference_offset: filename_guid_offset + 36,
                    filename: value.file.filename().to_owned(),
                    filename_offset: value.file.filename_offset(),
                    filename_record: value.file.record,
                    archive_entry_name: value.file.archive_entry_name,
                    asset: value.asset,
                }
            })
            .collect();
        (self.record, flags_count_offset, filename_count_offset, rows)
    }
}

/// One finite axis-aligned bound stored by a mesh Scene record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesignMeshSceneBounds {
    maximum: [f64; 3],
    minimum: [f64; 3],
}
impl DesignMeshSceneBounds {
    pub fn new(maximum: [f64; 3], minimum: [f64; 3]) -> Result<Self, String> {
        if !maximum
            .iter()
            .chain(&minimum)
            .all(|value| value.is_finite())
        {
            return Err("scene bounds maximum and minimum must be finite".into());
        }
        if minimum
            .iter()
            .zip(maximum)
            .any(|(minimum, maximum)| *minimum > maximum)
        {
            return Err("scene bounds minimum exceeds maximum".into());
        }
        Ok(Self { maximum, minimum })
    }
    pub fn maximum(&self) -> [f64; 3] {
        self.maximum
    }
    pub fn minimum(&self) -> [f64; 3] {
        self.minimum
    }
    // This conversion consumes the input carrier at the typed construction boundary.
    #[allow(clippy::needless_pass_by_value)]
    fn from_wire(wire: DesignMeshSceneBoundsWire, offsets: [u64; 2]) -> Result<Self, String> {
        if wire.offsets != offsets {
            return Err("scene bounds offsets must match their owning record".into());
        }
        Self::new(wire.maximum, wire.minimum)
    }
    fn into_wire(self, offsets: [u64; 2]) -> DesignMeshSceneBoundsWire {
        DesignMeshSceneBoundsWire {
            maximum: self.maximum(),
            minimum: self.minimum(),
            offsets,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignMeshSceneBoundsWire {
    /// Component-wise upper corner, serialized first.
    maximum: [f64; 3],
    /// Component-wise lower corner, serialized second.
    minimum: [f64; 3],
    /// Byte offsets of the serialized upper and lower corners.
    offsets: [u64; 2],
}

/// A fixed Scene-state record and its optional finite bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignMeshSceneState {
    record: DesignMeshFixedRecord<{ crate::layout::paramesh_scene_state::LEN as u64 }>,
    bounds: Option<DesignMeshSceneBounds>,
}
impl DesignMeshSceneState {
    pub fn new(
        record: DesignMeshFixedRecord<{ crate::layout::paramesh_scene_state::LEN as u64 }>,
        bounds: Option<DesignMeshSceneBounds>,
    ) -> Self {
        Self { record, bounds }
    }
    pub fn record(
        &self,
    ) -> &DesignMeshFixedRecord<{ crate::layout::paramesh_scene_state::LEN as u64 }> {
        &self.record
    }
    fn bounds_offsets(&self) -> [u64; 2] {
        let start =
            self.record.byte_offset() + crate::layout::paramesh_scene_state::FOOTER_MASK as u64;
        [start, start + 24]
    }
    fn from_wire(
        record: DesignMeshRecordIdentity,
        bounds: Option<DesignMeshSceneBoundsWire>,
    ) -> Result<Self, String> {
        let record = DesignMeshFixedRecord::try_from(record)?;
        let mut state = Self::new(record, None);
        state.bounds = bounds
            .map(|bounds| DesignMeshSceneBounds::from_wire(bounds, state.bounds_offsets()))
            .transpose()?;
        Ok(state)
    }
    fn into_wire(self) -> (DesignMeshRecordIdentity, Option<DesignMeshSceneBoundsWire>) {
        let bounds = self
            .bounds
            .map(|bounds| bounds.into_wire(self.bounds_offsets()));
        (self.record.into(), bounds)
    }
}

/// A compact or placed Scene-node record with bounds at the form's fixed location.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignMeshSceneNode {
    form: DesignMeshSceneNodeForm,
    bounds: Option<DesignMeshSceneBounds>,
}
#[derive(Debug, Clone, PartialEq)]
enum DesignMeshSceneNodeForm {
    Compact(DesignMeshFixedRecord<{ crate::layout::paramesh_scene_node::LEN as u64 }>),
    Placed {
        record: DesignMeshFixedRecord<{ crate::layout::paramesh_scene_node_placed::LEN as u64 }>,
        transform: MeshAffineTransform,
    },
}
impl DesignMeshSceneNode {
    pub fn new(
        record: DesignMeshRecordIdentity,
        bounds: Option<DesignMeshSceneBounds>,
        transform: Option<MeshAffineTransform>,
    ) -> Result<Self, String> {
        let form = match transform {
            None => DesignMeshSceneNodeForm::Compact(record.try_into()?),
            Some(transform) => DesignMeshSceneNodeForm::Placed {
                record: record.try_into()?,
                transform,
            },
        };
        Ok(Self { form, bounds })
    }
    pub fn record_index(&self) -> u32 {
        match &self.form {
            DesignMeshSceneNodeForm::Compact(record) => record.record_index(),
            DesignMeshSceneNodeForm::Placed { record, .. } => record.record_index(),
        }
    }
    pub fn byte_offset(&self) -> u64 {
        match &self.form {
            DesignMeshSceneNodeForm::Compact(record) => record.byte_offset(),
            DesignMeshSceneNodeForm::Placed { record, .. } => record.byte_offset(),
        }
    }
    pub fn frame_length(&self) -> u64 {
        match &self.form {
            DesignMeshSceneNodeForm::Compact(_) => crate::layout::paramesh_scene_node::LEN as u64,
            DesignMeshSceneNodeForm::Placed { .. } => {
                crate::layout::paramesh_scene_node_placed::LEN as u64
            }
        }
    }
    pub fn bounds(&self) -> Option<&DesignMeshSceneBounds> {
        self.bounds.as_ref()
    }
    pub fn bounds_offsets(&self) -> [u64; 2] {
        let relative = match &self.form {
            DesignMeshSceneNodeForm::Compact(_) => crate::layout::paramesh_scene_node::FOOTER_MASK,
            DesignMeshSceneNodeForm::Placed { .. } => {
                crate::layout::paramesh_scene_node_placed::FOOTER_MASK
            }
        };
        let start = self.byte_offset() + relative as u64;
        [start, start + 24]
    }
    pub fn transform(&self) -> Option<Located<MeshAffineTransform>> {
        match &self.form {
            DesignMeshSceneNodeForm::Compact(_) => None,
            DesignMeshSceneNodeForm::Placed { record, transform } => Some(Located {
                value: *transform,
                offset: record.byte_offset()
                    + crate::layout::paramesh_scene_node_placed::TRANSFORM as u64,
            }),
        }
    }
    pub fn state_reference_offset(&self) -> u64 {
        self.byte_offset() + crate::layout::paramesh_scene_node::SCENE_STATE_REFERENCE as u64
    }
    pub fn auxiliary_reference_offset(&self) -> u64 {
        self.byte_offset() + crate::layout::paramesh_scene_node::AUXILIARY_RECORD_REFERENCE as u64
    }
    fn from_wire(
        record: DesignMeshRecordIdentity,
        bounds: Option<DesignMeshSceneBoundsWire>,
        transform: Option<Located<MeshAffineTransform>>,
    ) -> Result<Self, String> {
        let mut node = Self::new(record, None, transform.map(|located| located.value))?;
        if node.transform().map(|located| located.offset) != transform.map(|located| located.offset)
        {
            return Err("scene_node_transform_offset must match the placed record layout".into());
        }
        node.bounds = bounds
            .map(|bounds| DesignMeshSceneBounds::from_wire(bounds, node.bounds_offsets()))
            .transpose()?;
        Ok(node)
    }
    fn into_wire(
        self,
    ) -> (
        DesignMeshRecordIdentity,
        Option<DesignMeshSceneBoundsWire>,
        Option<Located<MeshAffineTransform>>,
    ) {
        let transform = self.transform();
        let bounds = self
            .bounds()
            .copied()
            .map(|bounds| bounds.into_wire(self.bounds_offsets()));
        let record = match self.form {
            DesignMeshSceneNodeForm::Compact(record) => record.into(),
            DesignMeshSceneNodeForm::Placed { record, .. } => record.into(),
        };
        (record, bounds, transform)
    }
}

/// A lowercase RFC 4122 version-4 UUID from the mesh registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DesignMeshUuid(DesignGuidText);

impl DesignMeshUuid {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}
impl TryFrom<String> for DesignMeshUuid {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let value = Self(DesignGuidText::try_from(value)?);
        let bytes = value.as_str().as_bytes();
        if bytes.iter().any(u8::is_ascii_uppercase)
            || bytes[14] != b'4'
            || !matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
        {
            return Err("mesh UUID must be a lowercase version-4 UUID".into());
        }
        Ok(value)
    }
}
impl From<DesignMeshUuid> for String {
    fn from(value: DesignMeshUuid) -> Self {
        value.0.into()
    }
}

/// A container GUID in a record with the complete fixed join prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignMeshGuid {
    record: DesignMeshRecordIdentity,
    value: DesignGuidText,
}
impl DesignMeshGuid {
    pub fn new(record: DesignMeshRecordIdentity, value: DesignGuidText) -> Result<Self, String> {
        if record.frame_length() < crate::layout::paramesh_guid_join_prefix::LEN as u64 {
            return Err(
                "guid_record.frame_length must contain the complete GUID join prefix".into(),
            );
        }
        Ok(Self { record, value })
    }
    pub fn record(&self) -> &DesignMeshRecordIdentity {
        &self.record
    }
    pub fn value(&self) -> &str {
        self.value.as_str()
    }
    pub fn value_offset(&self) -> u64 {
        self.record.byte_offset() + crate::layout::paramesh_guid_join_prefix::FUSION_UUID as u64 + 4
    }
    pub fn entry_reference_offset(&self) -> u64 {
        self.record.byte_offset()
            + crate::layout::paramesh_guid_join_prefix::ENTRY_NAME_BACKLINK as u64
    }
}

/// An entry-name record whose UTF-16 name ends at the record boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignMeshEntryName {
    record: DesignMeshRecordIdentity,
    name: String,
}
impl DesignMeshEntryName {
    pub fn new(record: DesignMeshRecordIdentity, name: String) -> Result<Self, String> {
        let expected = u64::try_from(name.encode_utf16().count())
            .ok()
            .and_then(|units| units.checked_mul(2))
            .and_then(|bytes| {
                bytes.checked_add(crate::layout::paramesh_entry_name_prefix::LEN as u64 + 4)
            });
        if name.is_empty() || expected != Some(record.frame_length()) {
            return Err(
                "entry_name_record.frame_length must contain exactly its nonempty entry_name"
                    .into(),
            );
        }
        Ok(Self { record, name })
    }
    pub fn record(&self) -> &DesignMeshRecordIdentity {
        &self.record
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn name_offset(&self) -> u64 {
        self.record.byte_offset() + crate::layout::paramesh_entry_name_prefix::LEN as u64 + 4
    }
    pub fn guid_reference_offset(&self) -> u64 {
        self.record.byte_offset()
            + crate::layout::paramesh_entry_name_prefix::GUID_RECORD_REFERENCE as u64
    }
}

/// Mesh-body placement and the complete prefix plus terminal collection reference.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignMeshPlacement {
    record: DesignMeshRecordIdentity,
    transform: MeshAffineTransform,
}
impl DesignMeshPlacement {
    pub fn new(
        record: DesignMeshRecordIdentity,
        transform: MeshAffineTransform,
    ) -> Result<Self, String> {
        if record.frame_length() < crate::layout::paramesh_mesh_body_join_prefix::LEN as u64 + 11 {
            return Err("body_record.frame_length must contain the join prefix and final collection reference".into());
        }
        Ok(Self { record, transform })
    }
    pub fn record(&self) -> &DesignMeshRecordIdentity {
        &self.record
    }
    pub fn transform(&self) -> MeshAffineTransform {
        self.transform
    }
    pub fn transform_offsets(&self) -> [u64; 2] {
        [
            self.record.byte_offset()
                + crate::layout::paramesh_mesh_body_join_prefix::FIRST_TRANSFORM as u64,
            self.record.byte_offset()
                + crate::layout::paramesh_mesh_body_join_prefix::SECOND_TRANSFORM as u64,
        ]
    }
    pub fn scope_reference_offset(&self) -> u64 {
        self.record.byte_offset()
            + crate::layout::paramesh_mesh_body_join_prefix::FEATURE_SCOPE_REFERENCE as u64
    }
    pub fn wrapper_reference_offset(&self) -> u64 {
        self.record.byte_offset()
            + crate::layout::paramesh_mesh_body_join_prefix::WRAPPER_REFERENCE as u64
    }
    pub fn owner_reference_offset(&self) -> u64 {
        self.record.byte_offset()
            + crate::layout::paramesh_mesh_body_join_prefix::BODY_OWNER_REFERENCE as u64
    }
    pub fn guid_reference_offset(&self) -> u64 {
        self.record.byte_offset()
            + crate::layout::paramesh_mesh_body_join_prefix::CONTAINER_GUID_REFERENCE as u64
    }
    pub fn scene_node_reference_offset(&self) -> u64 {
        self.record.byte_offset()
            + crate::layout::paramesh_mesh_body_join_prefix::SCENE_NODE_REFERENCE as u64
    }
    pub fn collection_reference_offset(&self) -> u64 {
        self.record.byte_offset() + self.record.frame_length() - 11
    }
}

/// One mesh body and its complete Design identity graph.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignMeshBody {
    /// Mesh-body record carrying placement and graph references.
    pub placement: DesignMeshPlacement,
    /// Entry-name record joining the body to one `.paramesh` archive entry.
    pub entry: DesignMeshEntryName,
    /// GUID record joining the body to the container's `fusion_uuid`.
    pub guid: DesignMeshGuid,
    /// One-to-one `ParaMesh` wrapper around the mesh-body record.
    pub wrapper_record: DesignMeshFixedRecord<{ crate::layout::paramesh_body_wrapper::LEN as u64 }>,
    /// Fixed Scene-state record and optional bounds.
    pub scene_state: DesignMeshSceneState,
    /// Compact or placed Scene node with its optional bounds.
    pub scene_node: DesignMeshSceneNode,
    /// Separately typed Scene auxiliary cache reached through the Scene node.
    pub scene_auxiliary_record: DesignMeshRecordIdentity,
    /// Typed Design body-owner record referenced by the mesh-body record.
    /// Multiple mesh bodies can reference the same owner.
    pub owner_record: DesignMeshRecordIdentity,
    /// Container-local version-4 mesh UUID from protobuf registry field 12,
    /// when the geometry container joined this Design body.
    pub container_mesh_uuid: Option<DesignMeshUuid>,
    /// Neutral tessellation projected from the joined container, when present.
    pub tessellation_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
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
    scene_state_bounds: Option<DesignMeshSceneBoundsWire>,
    /// Scene node connecting `body_record` to its state and auxiliary cache.
    scene_node_record: DesignMeshRecordIdentity,
    /// Finite bound carried by the Scene-node footer; absent for its unset sentinel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scene_node_bounds: Option<DesignMeshSceneBoundsWire>,
    /// Optional row-major affine transform carried by the placed Scene-node form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scene_node_transform: Option<MeshAffineTransform>,
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
    fusion_uuid: DesignGuidText,
    /// Container-local version-4 mesh UUID from protobuf registry field 12,
    /// when the geometry container joined this Design body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    container_mesh_uuid: Option<DesignMeshUuid>,
    /// Byte offset of the ASCII `fusion_uuid` payload.
    fusion_uuid_offset: u64,
    /// Equal row-major container-to-model-centimetre affine transform.
    transform: MeshAffineTransform,
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
        let wrapper_body_reference_offset = value.wrapper_body_reference_offset();
        let fusion_uuid_offset = value.guid.value_offset();
        let guid_entry_reference_offset = value.guid.entry_reference_offset();
        let entry_name_offset = value.entry.name_offset();
        let entry_guid_reference_offset = value.entry.guid_reference_offset();
        let transform_offsets = value.placement.transform_offsets();
        let scope_reference_offset = value.placement.scope_reference_offset();
        let wrapper_reference_offset = value.placement.wrapper_reference_offset();
        let owner_reference_offset = value.placement.owner_reference_offset();
        let guid_reference_offset = value.placement.guid_reference_offset();
        let scene_node_reference_offset = value.placement.scene_node_reference_offset();
        let collection_reference_offset = value.placement.collection_reference_offset();
        let scene_state_reference_offset = value.scene_node.state_reference_offset();
        let scene_auxiliary_reference_offset = value.scene_node.auxiliary_reference_offset();
        let (scene_state_record, scene_state_bounds) = value.scene_state.into_wire();
        let (scene_node_record, scene_node_bounds, scene_node_transform) =
            value.scene_node.into_wire();
        Self {
            body_record: value.placement.record,
            entry_name_record: value.entry.record,
            guid_record: value.guid.record,
            wrapper_record: value.wrapper_record.into(),
            scene_state_record,
            scene_state_bounds,
            scene_node_record,
            scene_node_bounds,
            scene_node_transform: scene_node_transform.map(|located| located.value),
            scene_node_transform_offset: scene_node_transform.map(|located| located.offset),
            scene_auxiliary_record: value.scene_auxiliary_record,
            owner_record: value.owner_record,
            entry_name: value.entry.name,
            entry_name_offset,
            fusion_uuid: value.guid.value,
            container_mesh_uuid: value.container_mesh_uuid,
            fusion_uuid_offset,
            transform: value.placement.transform,
            transform_offsets,
            scope_reference_offset,
            wrapper_reference_offset,
            owner_reference_offset,
            guid_reference_offset,
            scene_node_reference_offset,
            collection_reference_offset,
            wrapper_body_reference_offset,
            entry_guid_reference_offset,
            guid_entry_reference_offset,
            scene_state_reference_offset,
            scene_auxiliary_reference_offset,
            tessellation_id: value.tessellation_id,
        }
    }
}

impl DesignMeshBody {
    pub fn wrapper_body_reference_offset(&self) -> u64 {
        self.wrapper_record.byte_offset()
            + crate::layout::paramesh_body_wrapper::BODY_REFERENCE as u64
    }
    fn from_wire(value: DesignMeshBodyWire) -> Result<Self, String> {
        let wrapper_record = DesignMeshFixedRecord::try_from(value.wrapper_record)?;
        if wrapper_record.byte_offset()
            + crate::layout::paramesh_body_wrapper::BODY_REFERENCE as u64
            != value.wrapper_body_reference_offset
        {
            return Err("wrapper_body_reference_offset must match wrapper_record layout".into());
        }
        let scene_state =
            DesignMeshSceneState::from_wire(value.scene_state_record, value.scene_state_bounds)?;
        let scene_node = DesignMeshSceneNode::from_wire(
            value.scene_node_record,
            value.scene_node_bounds,
            Located::from_wire(
                value.scene_node_transform,
                value.scene_node_transform_offset,
                "scene_node_transform",
            )?,
        )?;
        if scene_node.state_reference_offset() != value.scene_state_reference_offset
            || scene_node.auxiliary_reference_offset() != value.scene_auxiliary_reference_offset
        {
            return Err("scene_state_reference_offset/scene_auxiliary_reference_offset must match scene_node_record layout".into());
        }
        let guid = DesignMeshGuid::new(value.guid_record, value.fusion_uuid)?;
        if value.fusion_uuid_offset != guid.value_offset()
            || value.guid_entry_reference_offset != guid.entry_reference_offset()
        {
            return Err(
                "fusion_uuid_offset/guid_entry_reference_offset must match guid_record layout"
                    .into(),
            );
        }
        let entry = DesignMeshEntryName::new(value.entry_name_record, value.entry_name)?;
        if entry.name_offset() != value.entry_name_offset
            || entry.guid_reference_offset() != value.entry_guid_reference_offset
        {
            return Err(
                "entry_name_offset/entry_guid_reference_offset must match entry_name_record layout"
                    .into(),
            );
        }
        let placement = DesignMeshPlacement::new(value.body_record, value.transform)?;
        if placement.transform_offsets() != value.transform_offsets {
            return Err("transform_offsets must match body_record layout".into());
        }
        if placement.scope_reference_offset() != value.scope_reference_offset {
            return Err("scope_reference_offset must match body_record layout".into());
        }
        if placement.wrapper_reference_offset() != value.wrapper_reference_offset {
            return Err("wrapper_reference_offset must match body_record layout".into());
        }
        if placement.owner_reference_offset() != value.owner_reference_offset {
            return Err("owner_reference_offset must match body_record layout".into());
        }
        if placement.guid_reference_offset() != value.guid_reference_offset {
            return Err("guid_reference_offset must match body_record layout".into());
        }
        if placement.scene_node_reference_offset() != value.scene_node_reference_offset {
            return Err("scene_node_reference_offset must match body_record layout".into());
        }
        if placement.collection_reference_offset() != value.collection_reference_offset {
            return Err("collection_reference_offset must match body_record layout".into());
        }
        Ok(Self {
            placement,
            entry,
            guid,
            wrapper_record,
            scene_state,
            scene_node,
            scene_auxiliary_record: value.scene_auxiliary_record,
            owner_record: value.owner_record,
            container_mesh_uuid: value.container_mesh_uuid,
            tessellation_id: value.tessellation_id,
        })
    }
}

/// A finite, nonsingular row-major affine map.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[[f64; 4]; 4]", into = "[[f64; 4]; 4]")]
pub struct MeshAffineTransform([f64; 16]);

impl MeshAffineTransform {
    /// Check finite coefficients, an affine last row, and a nonzero finite determinant.
    pub fn new(cells: [f64; 16]) -> Result<Self, String> {
        let value = Self(cells);
        let rows = value.rows();
        if !cells.iter().all(|cell| cell.is_finite()) || rows[3] != [0.0, 0.0, 0.0, 1.0] {
            return Err("transform must be finite and affine".into());
        }
        let determinant = rows[0][0] * (rows[1][1] * rows[2][2] - rows[1][2] * rows[2][1])
            - rows[0][1] * (rows[1][0] * rows[2][2] - rows[1][2] * rows[2][0])
            + rows[0][2] * (rows[1][0] * rows[2][1] - rows[1][1] * rows[2][0]);
        if !determinant.is_finite() || determinant == 0.0 {
            return Err("transform must have a nonzero finite determinant".into());
        }
        Ok(value)
    }

    /// Row-major coefficients.
    pub fn cells(self) -> [f64; 16] {
        self.0
    }

    /// Four row-major rows.
    pub fn rows(self) -> [[f64; 4]; 4] {
        let cells = self.0;
        [
            [cells[0], cells[1], cells[2], cells[3]],
            [cells[4], cells[5], cells[6], cells[7]],
            [cells[8], cells[9], cells[10], cells[11]],
            [cells[12], cells[13], cells[14], cells[15]],
        ]
    }
}

impl TryFrom<[[f64; 4]; 4]> for MeshAffineTransform {
    type Error = String;
    fn try_from(rows: [[f64; 4]; 4]) -> Result<Self, Self::Error> {
        Self::new(std::array::from_fn(|i| rows[i / 4][i % 4]))
    }
}

impl From<MeshAffineTransform> for [[f64; 4]; 4] {
    fn from(value: MeshAffineTransform) -> Self {
        value.rows()
    }
}

/// Collection-owner record with a fixed-prefix or terminal backlink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignMeshCollectionOwner {
    record: DesignMeshRecordIdentity,
    backlink: DesignMeshCollectionBacklink,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesignMeshCollectionBacklink {
    Fixed241,
    Fixed262,
    Terminal,
}
impl DesignMeshCollectionOwner {
    pub fn new(record: DesignMeshRecordIdentity, backlink_offset: u64) -> Result<Self, String> {
        let relative = backlink_offset
            .checked_sub(record.byte_offset())
            .ok_or("collection_owner_backlink_offset precedes its record")?;
        let backlink = if relative
            == crate::layout::paramesh_collection_owner_v17::COLLECTION_BACKLINK as u64
            && record.frame_length() >= crate::layout::paramesh_collection_owner_v17::LEN as u64
        {
            DesignMeshCollectionBacklink::Fixed241
        } else if relative
            == crate::layout::paramesh_collection_owner_backlink_prefix::COLLECTION_BACKLINK as u64
            && record.frame_length()
                >= crate::layout::paramesh_collection_owner_backlink_prefix::LEN as u64
        {
            DesignMeshCollectionBacklink::Fixed262
        } else if relative == record.frame_length() - 11 {
            DesignMeshCollectionBacklink::Terminal
        } else {
            return Err("collection_owner_backlink_offset must identify a complete fixed-prefix or terminal reference".into());
        };
        Ok(Self { record, backlink })
    }
    pub fn record(&self) -> &DesignMeshRecordIdentity {
        &self.record
    }
    pub fn backlink_offset(&self) -> u64 {
        let relative = match self.backlink {
            DesignMeshCollectionBacklink::Fixed241 => {
                crate::layout::paramesh_collection_owner_v17::COLLECTION_BACKLINK as u64
            }
            DesignMeshCollectionBacklink::Fixed262 => {
                crate::layout::paramesh_collection_owner_backlink_prefix::COLLECTION_BACKLINK as u64
            }
            DesignMeshCollectionBacklink::Terminal => self.record.frame_length() - 11,
        };
        self.record.byte_offset() + relative
    }
}

/// Feature-scope record with its same-index closing base and owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignMeshScope {
    record: DesignMeshRecordIdentity,
    base_class_tag: DesignClassTag,
    owner_record_index: std::num::NonZeroU32,
}
impl DesignMeshScope {
    pub fn new(
        record: DesignMeshRecordIdentity,
        base_record: DesignMeshRecordIdentity,
        owner_record_index: u32,
    ) -> Result<Self, String> {
        let base_length = crate::layout::paramesh_feature_scope_base::LEN as u64;
        if record.frame_length()
            < crate::layout::paramesh_feature_scope_prefix::LEN as u64 + base_length
        {
            return Err(
                "scope_record.frame_length must contain the prefix and closing base".into(),
            );
        }
        if base_record.record_index() != record.record_index()
            || base_record.frame_length() != base_length
            || base_record.byte_offset()
                != record.byte_offset() + record.frame_length() - base_length
        {
            return Err(
                "scope_base_record must be the same-index closing base of scope_record".into(),
            );
        }
        let owner_record_index = std::num::NonZeroU32::new(owner_record_index)
            .ok_or("scope_owner_record_index must be nonzero")?;
        Ok(Self {
            record,
            base_class_tag: base_record.class_tag,
            owner_record_index,
        })
    }
    pub fn record(&self) -> &DesignMeshRecordIdentity {
        &self.record
    }
    pub fn base_record(&self) -> DesignMeshRecordIdentity {
        let frame_length = crate::layout::paramesh_feature_scope_base::LEN as u64;
        DesignMeshRecordIdentity {
            class_tag: self.base_class_tag.clone(),
            record_index: self.record.record_index,
            byte_offset: self.record.byte_offset() + self.record.frame_length() - frame_length,
            frame_length,
        }
    }
    pub fn owner_record_index(&self) -> u32 {
        self.owner_record_index.get()
    }
    pub fn owner_reference_offset(&self) -> u64 {
        self.record.byte_offset() + self.record.frame_length()
            - crate::layout::paramesh_feature_scope_base::LEN as u64
            + crate::layout::paramesh_feature_scope_base::SCOPE_OWNER_REFERENCE as u64
    }
}

/// Mesh collection with its same-index nested base and complete body-reference run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignMeshCollection {
    record: DesignMeshRecordIdentity,
    base_class_tag: DesignClassTag,
}
impl DesignMeshCollection {
    pub fn new(
        record: DesignMeshRecordIdentity,
        base_record: DesignMeshRecordIdentity,
    ) -> Result<Self, String> {
        let prefix = crate::layout::paramesh_mesh_collection_prefix::LEN as u64;
        let fixed_length =
            prefix + crate::layout::paramesh_mesh_collection_base_prefix::LEN as u64 + 11;
        let body_bytes = record.frame_length().checked_sub(fixed_length).ok_or(
            "collection_record.frame_length must contain both prefixes and the owner reference",
        )?;
        if body_bytes % 11 != 0 || u32::try_from(body_bytes / 11).is_err() {
            return Err(
                "collection_record.frame_length must contain a u32-counted body-reference run"
                    .into(),
            );
        }
        if base_record.record_index() != record.record_index()
            || base_record.byte_offset() != record.byte_offset() + prefix
            || base_record.frame_length() != record.frame_length() - prefix
        {
            return Err(
                "collection_base_record must be the same-index nested base of collection_record"
                    .into(),
            );
        }
        Ok(Self {
            record,
            base_class_tag: base_record.class_tag,
        })
    }
    pub fn record(&self) -> &DesignMeshRecordIdentity {
        &self.record
    }
    pub fn base_record(&self) -> DesignMeshRecordIdentity {
        let prefix = crate::layout::paramesh_mesh_collection_prefix::LEN as u64;
        DesignMeshRecordIdentity {
            class_tag: self.base_class_tag.clone(),
            record_index: self.record.record_index,
            byte_offset: self.record.byte_offset() + prefix,
            frame_length: self.record.frame_length() - prefix,
        }
    }
    pub fn body_count(&self) -> u64 {
        (self.record.frame_length()
            - crate::layout::paramesh_mesh_collection_prefix::LEN as u64
            - crate::layout::paramesh_mesh_collection_base_prefix::LEN as u64
            - 11)
            / 11
    }
    pub fn texture_table_reference_offset(&self) -> u64 {
        self.record.byte_offset()
            + crate::layout::paramesh_mesh_collection_prefix::TEXTURE_TABLE_REFERENCE as u64
    }
    pub fn owner_reference_offset(&self) -> u64 {
        self.record.byte_offset() + self.record.frame_length() - 11
    }
}

/// One complete `Base Mesh Feature` Design graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignMeshFeatureWire", into = "DesignMeshFeatureWire")]
pub struct DesignMeshFeature {
    /// Globally unique deterministic identity keyed by the feature-scope record.
    pub id: String,
    /// Feature scope and its closing owner reference.
    scope: DesignMeshScope,
    /// Mesh collection and its nested base.
    collection: DesignMeshCollection,
    /// Typed `ParaMesh` texture-table record owned by the collection.
    pub texture_table: DesignMeshTextureTable,
    /// Typed Design owner of the mesh-body collection.
    pub collection_owner: DesignMeshCollectionOwner,
    /// Mesh bodies in the source collection order.
    bodies: Vec<DesignMeshBody>,
}

impl DesignMeshFeature {
    pub fn new(
        id: String,
        scope: DesignMeshScope,
        collection: DesignMeshCollection,
        texture_table: DesignMeshTextureTable,
        collection_owner: DesignMeshCollectionOwner,
        bodies: Vec<DesignMeshBody>,
    ) -> Result<Self, String> {
        let body_count = u32::try_from(bodies.len()).map_err(|_| "bodies count must fit u32")?;
        if collection.body_count() != u64::from(body_count) {
            return Err("bodies count must match collection_record.frame_length".into());
        }
        let scope_body_bytes = u64::from(body_count) * 11;
        let scope_prefix = crate::layout::paramesh_feature_scope_prefix::LEN as u64;
        let scope_base_length = crate::layout::paramesh_feature_scope_base::LEN as u64;
        if scope_body_bytes > scope.record().frame_length() - scope_prefix - scope_base_length {
            return Err("bodies reference run must end before scope_base_record".into());
        }
        Ok(Self {
            id,
            scope,
            collection,
            texture_table,
            collection_owner,
            bodies,
        })
    }
    pub fn scope(&self) -> &DesignMeshScope {
        &self.scope
    }
    pub fn collection(&self) -> &DesignMeshCollection {
        &self.collection
    }
    pub fn bodies(&self) -> &[DesignMeshBody] {
        &self.bodies
    }
    pub fn bodies_mut(&mut self) -> &mut [DesignMeshBody] {
        &mut self.bodies
    }
    fn body_count_offsets(&self) -> [u64; 3] {
        [
            self.scope.record().byte_offset()
                + crate::layout::paramesh_feature_scope_prefix::BODY_COUNT as u64,
            self.collection.record().byte_offset()
                + crate::layout::paramesh_mesh_collection_prefix::BODY_COUNT as u64,
            self.collection.record().byte_offset()
                + crate::layout::paramesh_mesh_collection_prefix::LEN as u64
                + crate::layout::paramesh_mesh_collection_base_prefix::BODY_COUNT as u64,
        ]
    }
    fn scope_body_reference_offsets(&self) -> impl Iterator<Item = u64> + '_ {
        let start = self.scope.record().byte_offset()
            + crate::layout::paramesh_feature_scope_prefix::LEN as u64;
        (0..self.collection.body_count()).map(move |ordinal| start + 11 * ordinal)
    }
    fn collection_body_reference_offsets(&self) -> impl Iterator<Item = u64> + '_ {
        let start = self.collection.record().byte_offset()
            + crate::layout::paramesh_mesh_collection_prefix::LEN as u64
            + crate::layout::paramesh_mesh_collection_base_prefix::LEN as u64;
        (0..self.collection.body_count()).map(move |ordinal| start + 11 * ordinal)
    }
}

/// One complete `Base Mesh Feature` Design graph.
#[derive(Serialize, Deserialize)]
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
    textures: Vec<DesignMeshTextureResourceWire>,
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
        if !wire.body_record_indices.iter().copied().eq(wire
            .bodies
            .iter()
            .map(|body| body.body_record.record_index()))
        {
            return Err(
                "body_record_indices must repeat bodies.body_record.record_index in order".into(),
            );
        }
        let bodies = wire
            .bodies
            .into_iter()
            .map(DesignMeshBody::from_wire)
            .collect::<Result<Vec<_>, _>>()?;
        let scope = DesignMeshScope::new(
            wire.scope_record,
            wire.scope_base_record,
            wire.scope_owner_record_index,
        )?;
        if scope.owner_reference_offset() != wire.scope_owner_reference_offset {
            return Err(
                "scope_owner_reference_offset must locate the closing base owner reference".into(),
            );
        }
        let collection =
            DesignMeshCollection::new(wire.collection_record, wire.collection_base_record)?;
        if collection.texture_table_reference_offset() != wire.texture_table_reference_offset {
            return Err(
                "texture_table_reference_offset must locate the collection texture-table reference"
                    .into(),
            );
        }
        if collection.owner_reference_offset() != wire.collection_owner_reference_offset {
            return Err("collection_owner_reference_offset must locate the terminal collection owner reference".into());
        }
        let feature = Self::new(
            wire.id,
            scope,
            collection,
            DesignMeshTextureTable::from_wire(
                wire.texture_table_record,
                wire.texture_flags_count_offset,
                wire.texture_filename_count_offset,
                wire.textures,
            )?,
            DesignMeshCollectionOwner::new(
                wire.collection_owner_record,
                wire.collection_owner_backlink_offset,
            )?,
            bodies,
        )?;
        if wire.body_count_offsets != feature.body_count_offsets() {
            return Err("body_count_offsets must locate the scope and collection counts".into());
        }
        if !wire
            .scope_body_reference_offsets
            .into_iter()
            .eq(feature.scope_body_reference_offsets())
        {
            return Err(
                "scope_body_reference_offsets must locate the ordered scope body references".into(),
            );
        }
        if !wire
            .collection_body_reference_offsets
            .into_iter()
            .eq(feature.collection_body_reference_offsets())
        {
            return Err("collection_body_reference_offsets must locate the ordered collection body references".into());
        }
        Ok(feature)
    }
}

impl From<DesignMeshFeature> for DesignMeshFeatureWire {
    // Output cardinalities are bounded by already-materialized input vectors.
    #[allow(clippy::disallowed_methods)]
    fn from(value: DesignMeshFeature) -> Self {
        let mut body_record_indices = Vec::with_capacity(value.bodies.len());
        let body_count_offsets = value.body_count_offsets();
        let scope_body_reference_offsets = value.scope_body_reference_offsets().collect();
        let collection_body_reference_offsets = value.collection_body_reference_offsets().collect();
        let mut bodies = Vec::with_capacity(value.bodies.len());
        for body in value.bodies {
            body_record_indices.push(body.placement.record().record_index());
            bodies.push(body.into());
        }
        let collection_owner_backlink_offset = value.collection_owner.backlink_offset();
        let (
            texture_table_record,
            texture_flags_count_offset,
            texture_filename_count_offset,
            textures,
        ) = value.texture_table.into_wire();
        Self {
            body_record_indices,
            scope_body_reference_offsets,
            collection_body_reference_offsets,
            bodies,
            id: value.id,
            scope_record: value.scope.record().clone(),
            scope_base_record: value.scope.base_record(),
            collection_record: value.collection.record().clone(),
            collection_base_record: value.collection.base_record(),
            texture_table_record,
            body_count_offsets,
            texture_table_reference_offset: value.collection.texture_table_reference_offset(),
            collection_owner_record: value.collection_owner.record,
            collection_owner_reference_offset: value.collection.owner_reference_offset(),
            collection_owner_backlink_offset,
            scope_owner_record_index: value.scope.owner_record_index(),
            scope_owner_reference_offset: value.scope.owner_reference_offset(),
            texture_flags_count_offset,
            texture_filename_count_offset,
            textures,
        }
    }
}

const EPS_CANVAS_DECODE_GEOMETRY_PAYLOAD_E9: f64 = 1.0e-9;
const DESIGN_CANVAS_LENGTH_TO_MM: f64 = 10.0;

/// Canvas opacity and source-space frame; the fixed payload is emitted from these values.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignCanvasGeometryPayload {
    opacity: f32,
    origin_centimetres: [f64; 3],
    u_axis: Vector3,
    v_axis: Vector3,
}
impl TryFrom<&[u8]> for DesignCanvasGeometryPayload {
    type Error = String;
    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() != 77 {
            return Err("geometry_payload must contain 77 bytes".into());
        }
        let mut view = cadmpeg_core::decode::View::over_retained(bytes);
        let opacity = view
            .req_f32_le()
            .map_err(|error| format!("geometry_payload: {error:?}"))?;
        let reserved = view
            .req_u8()
            .map_err(|error| format!("geometry_payload: {error:?}"))?;
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) || reserved != 0 {
            return Err(
                "geometry_payload must contain normalized finite opacity and a zero reserved byte"
                    .into(),
            );
        }
        let mut vector = || -> Result<[f64; 3], String> {
            Ok([
                view.req_f64_le()
                    .map_err(|error| format!("geometry_payload: {error:?}"))?,
                view.req_f64_le()
                    .map_err(|error| format!("geometry_payload: {error:?}"))?,
                view.req_f64_le()
                    .map_err(|error| format!("geometry_payload: {error:?}"))?,
            ])
        };
        let origin_centimetres = vector()?;
        let u = vector()?;
        let v = vector()?;
        let u_axis = Vector3::new(u[0], u[1], u[2]);
        let v_axis = Vector3::new(v[0], v[1], v[2]);
        if !origin_centimetres.into_iter().all(f64::is_finite)
            || (u_axis.norm() - 1.0).abs() > EPS_CANVAS_DECODE_GEOMETRY_PAYLOAD_E9
            || (v_axis.norm() - 1.0).abs() > EPS_CANVAS_DECODE_GEOMETRY_PAYLOAD_E9
            || u_axis.dot(v_axis).abs() > EPS_CANVAS_DECODE_GEOMETRY_PAYLOAD_E9
        {
            return Err(
                "geometry_payload must contain a finite origin and an admitted orthonormal frame"
                    .into(),
            );
        }
        Ok(Self {
            opacity,
            origin_centimetres,
            u_axis,
            v_axis,
        })
    }
}
impl DesignCanvasGeometryPayload {
    pub fn decoded(&self) -> (f32, Point3, Vector3, Vector3) {
        (
            self.opacity,
            Point3::new(
                self.origin_centimetres[0] * DESIGN_CANVAS_LENGTH_TO_MM,
                self.origin_centimetres[1] * DESIGN_CANVAS_LENGTH_TO_MM,
                self.origin_centimetres[2] * DESIGN_CANVAS_LENGTH_TO_MM,
            ),
            self.u_axis,
            self.v_axis,
        )
    }
    pub fn bytes(&self) -> [u8; 77] {
        let mut bytes = [0; 77];
        bytes[..4].copy_from_slice(&self.opacity.to_le_bytes());
        let mut at = 5;
        for value in self
            .origin_centimetres
            .into_iter()
            .chain([self.u_axis.x, self.u_axis.y, self.u_axis.z])
            .chain([self.v_axis.x, self.v_axis.y, self.v_axis.z])
        {
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            at += 8;
        }
        bytes
    }
}

/// Authored Canvas boundary segments in an admitted horizontal or vertical form.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesignCanvasBounds {
    form: DesignCanvasBoundaryForm,
}
#[derive(Debug, Clone, Copy, PartialEq)]
enum DesignCanvasBoundaryForm {
    Horizontal([[Point2; 2]; 2]),
    Vertical([[Point2; 2]; 2]),
}
impl TryFrom<[[Point2; 2]; 2]> for DesignCanvasBounds {
    type Error = String;
    fn try_from(segments: [[Point2; 2]; 2]) -> Result<Self, Self::Error> {
        if segments
            .iter()
            .flatten()
            .any(|point| !point.u.is_finite() || !point.v.is_finite())
        {
            return Err("boundary_segments must contain finite coordinates".into());
        }
        let [[a, b], [c, d]] = segments;
        let close = |left: f64, right: f64| {
            (left - right).abs() <= 64.0 * f64::EPSILON * left.abs().max(right.abs()).max(1.0)
        };
        let horizontal = close(a.v, b.v)
            && close(c.v, d.v)
            && close(a.u, c.u)
            && close(b.u, d.u)
            && !close(a.v, c.v);
        let vertical = close(a.u, b.u)
            && close(c.u, d.u)
            && close(a.v, c.v)
            && close(b.v, d.v)
            && !close(a.u, c.u);
        let form = if horizontal {
            DesignCanvasBoundaryForm::Horizontal(segments)
        } else if vertical {
            DesignCanvasBoundaryForm::Vertical(segments)
        } else {
            return Err(
                "boundary_segments must form an admitted horizontal or vertical pair".into(),
            );
        };
        Ok(Self { form })
    }
}
impl DesignCanvasBounds {
    pub fn segments(self) -> [[Point2; 2]; 2] {
        match self.form {
            DesignCanvasBoundaryForm::Horizontal(segments)
            | DesignCanvasBoundaryForm::Vertical(segments) => segments,
        }
    }
    pub fn mirroring(self) -> (bool, bool) {
        match self.form {
            DesignCanvasBoundaryForm::Horizontal([[a, b], [c, _]]) => (a.u > b.u, a.v > c.v),
            DesignCanvasBoundaryForm::Vertical([[a, b], [c, _]]) => (a.u > c.u, a.v > b.v),
        }
    }
    pub fn extents(self) -> [Point2; 2] {
        let [[a, b], [c, d]] = self.segments();
        let (minimum, maximum) = [b, c, d]
            .into_iter()
            .fold((a, a), |(minimum, maximum), point| {
                (
                    Point2::new(minimum.u.min(point.u), minimum.v.min(point.v)),
                    Point2::new(maximum.u.max(point.u), maximum.v.max(point.v)),
                )
            });
        [minimum, maximum]
    }
}

/// Canvas geometry flags; all other prologue bytes are fixed zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesignCanvasPrologue {
    first_flag: bool,
    visible: bool,
}
impl TryFrom<[u8; 15]> for DesignCanvasPrologue {
    type Error = String;
    fn try_from(bytes: [u8; 15]) -> Result<Self, Self::Error> {
        if bytes[..10] != [0; 10]
            || !matches!(bytes[10], 0 | 1)
            || bytes[11..14] != [0; 3]
            || !matches!(bytes[14], 0 | 1)
        {
            return Err(
                "geometry_prologue must contain only its two boolean flags and fixed zero bytes"
                    .into(),
            );
        }
        Ok(Self {
            first_flag: bytes[10] != 0,
            visible: bytes[14] != 0,
        })
    }
}
impl DesignCanvasPrologue {
    pub fn visible(self) -> bool {
        self.visible
    }
    pub fn bytes(self) -> [u8; 15] {
        let mut bytes = [0; 15];
        bytes[10] = u8::from(self.first_flag);
        bytes[14] = u8::from(self.visible);
        bytes
    }
}

const CANVAS_GEOMETRY_PREFIX_BYTES: u64 = 217;
const CANVAS_PAIRED_GEOMETRY_BYTES: u64 = 30;
const CANVAS_IMAGE_ASSET_PREFIX_BYTES: u64 = 25;

/// Canvas geometry and its same-index closing record.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignCanvasGeometry {
    class_tags: [String; 2],
    record_index: u32,
    byte_offset: u64,
    label: String,
    /// Flags from the fixed geometry prologue.
    pub prologue: DesignCanvasPrologue,
    /// Authored image boundaries and their orientation.
    pub boundary: DesignCanvasBounds,
    /// Opacity and source-space image frame.
    pub payload: DesignCanvasGeometryPayload,
}
impl DesignCanvasGeometry {
    pub fn new(
        class_tags: [String; 2],
        record_index: u32,
        byte_offset: u64,
        label: String,
        prologue: DesignCanvasPrologue,
        boundary: DesignCanvasBounds,
        payload: DesignCanvasGeometryPayload,
    ) -> Result<Self, String> {
        for (name, tag) in ["geometry_class_tag", "paired_geometry_class_tag"]
            .into_iter()
            .zip(&class_tags)
        {
            if tag.is_empty() || !tag.bytes().all(|byte| byte.is_ascii_graphic()) {
                return Err(format!("{name} must contain printable ASCII characters"));
            }
        }
        if label.is_empty() {
            return Err("label must be nonempty".into());
        }
        let units = u32::try_from(label.encode_utf16().count())
            .map_err(|_| "label UTF-16 count must fit u32")?;
        let frame_length = CANVAS_GEOMETRY_PREFIX_BYTES + 2 * u64::from(units);
        byte_offset
            .checked_add(frame_length)
            .and_then(|end| end.checked_add(CANVAS_PAIRED_GEOMETRY_BYTES))
            .ok_or("geometry_byte_offset and label must leave a complete paired geometry record")?;
        Ok(Self {
            class_tags,
            record_index,
            byte_offset,
            label,
            prologue,
            boundary,
            payload,
        })
    }
    pub fn record_index(&self) -> u32 {
        self.record_index
    }
    fn frame_length(&self) -> u64 {
        CANVAS_GEOMETRY_PREFIX_BYTES + 2 * self.label.encode_utf16().count() as u64
    }
    fn paired_byte_offset(&self) -> u64 {
        self.byte_offset + self.frame_length()
    }
    fn scope_reference_offset(&self) -> u64 {
        self.byte_offset + 147
    }
    fn visibility_offset(&self) -> u64 {
        self.byte_offset + 25
    }
    fn paired_component_reference_offset(&self) -> u64 {
        self.paired_byte_offset() + 20
    }
    fn boundary_coordinate_offsets(&self) -> [u64; 8] {
        [26, 34, 42, 50, 181, 189, 197, 205].map(|relative| self.byte_offset + relative)
    }
    fn second_boundary_present_offset(&self) -> u64 {
        self.byte_offset + 180
    }
    fn plane_reference_offset(&self) -> u64 {
        self.byte_offset + 59
    }
    fn component_reference_offset(&self) -> u64 {
        self.byte_offset + 158
    }
    fn asset_reference_offset(&self) -> u64 {
        self.byte_offset + 170
    }
    fn label_offset(&self) -> u64 {
        self.byte_offset + CANVAS_GEOMETRY_PREFIX_BYTES
    }
}

/// Canvas image-asset record with a nonempty UTF-16 name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignCanvasAsset {
    class_tag: DesignClassTag,
    record_index: u32,
    name: String,
}
impl DesignCanvasAsset {
    pub fn new(class_tag: DesignClassTag, record_index: u32, name: String) -> Result<Self, String> {
        if name.is_empty() {
            return Err("asset_name must be nonempty".into());
        }
        u32::try_from(name.encode_utf16().count())
            .map_err(|_| "asset_name UTF-16 count must fit u32")?;
        Ok(Self {
            class_tag,
            record_index,
            name,
        })
    }
    fn frame_length(&self) -> u64 {
        CANVAS_IMAGE_ASSET_PREFIX_BYTES + 2 * self.name.encode_utf16().count() as u64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesignCanvasScopeForm {
    Compact,
    Expanded,
}
impl DesignCanvasScopeForm {
    fn reference_offset(self) -> u64 {
        match self {
            Self::Compact => 22,
            Self::Expanded => 26,
        }
    }
}

/// Exact image-plane binding owned by one Design `Canvas` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignCanvasImageWire", into = "DesignCanvasImageWire")]
pub struct DesignCanvasImage {
    /// Globally unique native binding identity.
    pub id: String,
    /// Owning Canvas scope.
    pub scope_record_index: u32,
    /// Supporting construction-plane entity.
    pub plane_entity_suffix: u32,
    /// Component entity owning the Canvas.
    pub component_entity_suffix: u32,
    geometry: DesignCanvasGeometry,
    asset: DesignCanvasAsset,
    scope_form: DesignCanvasScopeForm,
}
impl DesignCanvasImage {
    pub fn new(
        id: String,
        scope_record_index: u32,
        geometry_reference_offset: u64,
        geometry: DesignCanvasGeometry,
        asset: DesignCanvasAsset,
        plane_entity_suffix: u32,
        component_entity_suffix: u32,
    ) -> Result<Self, String> {
        if geometry.record_index == asset.record_index {
            return Err("asset_record_index must differ from geometry_record_index".into());
        }
        let scope_offset = geometry
            .paired_byte_offset()
            .checked_add(CANVAS_PAIRED_GEOMETRY_BYTES)
            .and_then(|offset| offset.checked_add(asset.frame_length()))
            .ok_or("asset_name must end at a representable scope byte offset")?;
        let scope_form = match geometry_reference_offset.checked_sub(scope_offset) {
            Some(22) => DesignCanvasScopeForm::Compact,
            Some(26) => DesignCanvasScopeForm::Expanded,
            _ => return Err("geometry_reference_offset must locate a compact or expanded Canvas scope reference".into()),
        };
        geometry_reference_offset
            .checked_add(4)
            .ok_or("geometry_reference_offset must leave a complete u32 reference")?;
        Ok(Self {
            id,
            scope_record_index,
            plane_entity_suffix,
            component_entity_suffix,
            geometry,
            asset,
            scope_form,
        })
    }
    pub fn geometry(&self) -> &DesignCanvasGeometry {
        &self.geometry
    }
    pub fn asset_name(&self) -> &str {
        &self.asset.name
    }
    pub fn scope_byte_offset(&self) -> u64 {
        self.asset_byte_offset() + self.asset.frame_length()
    }
    fn asset_byte_offset(&self) -> u64 {
        self.geometry.paired_byte_offset() + CANVAS_PAIRED_GEOMETRY_BYTES
    }
    fn asset_name_offset(&self) -> u64 {
        self.asset_byte_offset() + CANVAS_IMAGE_ASSET_PREFIX_BYTES
    }
    fn geometry_reference_offset(&self) -> u64 {
        self.scope_byte_offset() + self.scope_form.reference_offset()
    }
}

/// Exact image-plane binding owned by one Design `Canvas` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignCanvasImageWire {
    /// Globally unique deterministic identifier for this native binding.
    id: String,
    /// Canvas scope record index.
    scope_record_index: u32,
    /// Byte offset of the marked scope reference in the geometry record.
    scope_reference_offset: u64,
    /// Dynamic class tag of the primary geometry record.
    geometry_class_tag: String,
    /// Geometry record index.
    geometry_record_index: u32,
    /// Byte offset of the scope's marked geometry-record reference.
    geometry_reference_offset: u64,
    /// Byte offset of the primary geometry record.
    geometry_byte_offset: u64,
    /// Fixed geometry prologue immediately following the primary record header.
    geometry_prologue: [u8; 15],
    /// Whether the Canvas raster is visible.
    visible: bool,
    /// Byte offset of the visibility byte in the geometry prologue.
    visibility_offset: u64,
    /// Byte length from the primary geometry header to its paired header.
    geometry_frame_length: u64,
    /// Dynamic class tag of the paired geometry record.
    paired_geometry_class_tag: String,
    /// Byte offset of the paired geometry record.
    paired_geometry_byte_offset: u64,
    /// Byte offset of the paired record's marked component reference.
    paired_component_reference_offset: u64,
    /// Two opposite boundary segments in plane-local coordinates.
    boundary_segments: [[Point2; 2]; 2],
    /// Byte offsets of the eight boundary-coordinate f64 values.
    boundary_coordinate_offsets: [u64; 8],
    /// Byte offset of the presence marker preceding the second boundary segment.
    second_boundary_present_offset: u64,
    /// Design entity suffix of the supporting construction plane.
    plane_entity_suffix: u32,
    /// Byte offset of the marked construction-plane reference.
    plane_reference_offset: u64,
    /// Design entity suffix of the component owning the Canvas.
    component_entity_suffix: u32,
    /// Byte offset of the marked component reference.
    component_reference_offset: u64,
    /// Dynamic class tag of the standalone image-asset record.
    asset_class_tag: String,
    /// Image-asset record index.
    asset_record_index: u32,
    /// Byte offset of the marked image-asset reference.
    asset_reference_offset: u64,
    /// Byte offset of the image-asset record.
    asset_byte_offset: u64,
    /// Archive entry basename stored by the image-asset record.
    asset_name: String,
    /// Byte offset of the asset name's UTF-16LE code units.
    asset_name_offset: u64,
    /// Persistent Canvas label stored after the boundary segments.
    label: String,
    /// Byte offset of the label's UTF-16LE code units.
    label_offset: u64,
    /// Normalized raster opacity.
    opacity: f32,
    /// Image-plane origin in model-space millimeters.
    origin: Point3,
    /// Unit direction of increasing image u coordinate.
    u_axis: Vector3,
    /// Unit direction of increasing image v coordinate.
    v_axis: Vector3,
    /// Uninterpreted fixed geometry payload between the plane reference and scope link.
    geometry_payload: Vec<u8>,
}

impl TryFrom<DesignCanvasImageWire> for DesignCanvasImage {
    type Error = String;
    fn try_from(wire: DesignCanvasImageWire) -> Result<Self, Self::Error> {
        let geometry_prologue = DesignCanvasPrologue::try_from(wire.geometry_prologue)?;
        if geometry_prologue.visible() != wire.visible {
            return Err("visible must match geometry_prologue".into());
        }
        let geometry_payload =
            DesignCanvasGeometryPayload::try_from(wire.geometry_payload.as_slice())?;
        let (opacity, origin, u_axis, v_axis) = geometry_payload.decoded();
        if wire.opacity.to_bits() != opacity.to_bits() {
            return Err("opacity must match geometry_payload".into());
        }
        for (name, declared, decoded) in [
            (
                "origin",
                [wire.origin.x, wire.origin.y, wire.origin.z],
                [origin.x, origin.y, origin.z],
            ),
            (
                "u_axis",
                [wire.u_axis.x, wire.u_axis.y, wire.u_axis.z],
                [u_axis.x, u_axis.y, u_axis.z],
            ),
            (
                "v_axis",
                [wire.v_axis.x, wire.v_axis.y, wire.v_axis.z],
                [v_axis.x, v_axis.y, v_axis.z],
            ),
        ] {
            if declared
                .into_iter()
                .zip(decoded)
                .any(|(left, right)| left.to_bits() != right.to_bits())
            {
                return Err(format!("{name} must match geometry_payload"));
            }
        }
        let geometry = DesignCanvasGeometry::new(
            [wire.geometry_class_tag, wire.paired_geometry_class_tag],
            wire.geometry_record_index,
            wire.geometry_byte_offset,
            wire.label,
            geometry_prologue,
            DesignCanvasBounds::try_from(wire.boundary_segments)?,
            geometry_payload,
        )?;
        let asset = DesignCanvasAsset::new(
            DesignClassTag::try_from(wire.asset_class_tag)
                .map_err(|error| format!("asset_class_tag: {error}"))?,
            wire.asset_record_index,
            wire.asset_name,
        )?;
        let image = Self::new(
            wire.id,
            wire.scope_record_index,
            wire.geometry_reference_offset,
            geometry,
            asset,
            wire.plane_entity_suffix,
            wire.component_entity_suffix,
        )?;
        for (name, declared, derived) in [
            (
                "scope_reference_offset",
                wire.scope_reference_offset,
                image.geometry.scope_reference_offset(),
            ),
            (
                "visibility_offset",
                wire.visibility_offset,
                image.geometry.visibility_offset(),
            ),
            (
                "geometry_frame_length",
                wire.geometry_frame_length,
                image.geometry.frame_length(),
            ),
            (
                "paired_geometry_byte_offset",
                wire.paired_geometry_byte_offset,
                image.geometry.paired_byte_offset(),
            ),
            (
                "paired_component_reference_offset",
                wire.paired_component_reference_offset,
                image.geometry.paired_component_reference_offset(),
            ),
            (
                "second_boundary_present_offset",
                wire.second_boundary_present_offset,
                image.geometry.second_boundary_present_offset(),
            ),
            (
                "plane_reference_offset",
                wire.plane_reference_offset,
                image.geometry.plane_reference_offset(),
            ),
            (
                "component_reference_offset",
                wire.component_reference_offset,
                image.geometry.component_reference_offset(),
            ),
            (
                "asset_reference_offset",
                wire.asset_reference_offset,
                image.geometry.asset_reference_offset(),
            ),
            (
                "asset_byte_offset",
                wire.asset_byte_offset,
                image.asset_byte_offset(),
            ),
            (
                "asset_name_offset",
                wire.asset_name_offset,
                image.asset_name_offset(),
            ),
            (
                "label_offset",
                wire.label_offset,
                image.geometry.label_offset(),
            ),
        ] {
            if declared != derived {
                return Err(format!("{name} must match the Canvas record layout"));
            }
        }
        if wire.boundary_coordinate_offsets != image.geometry.boundary_coordinate_offsets() {
            return Err("boundary_coordinate_offsets must match the Canvas record layout".into());
        }
        Ok(image)
    }
}

impl From<DesignCanvasImage> for DesignCanvasImageWire {
    fn from(value: DesignCanvasImage) -> Self {
        let (opacity, origin, u_axis, v_axis) = value.geometry.payload.decoded();
        let scope_reference_offset = value.geometry.scope_reference_offset();
        let geometry_record_index = value.geometry.record_index;
        let geometry_reference_offset = value.geometry_reference_offset();
        let geometry_byte_offset = value.geometry.byte_offset;
        let geometry_prologue = value.geometry.prologue.bytes();
        let visible = value.geometry.prologue.visible();
        let visibility_offset = value.geometry.visibility_offset();
        let geometry_frame_length = value.geometry.frame_length();
        let paired_geometry_byte_offset = value.geometry.paired_byte_offset();
        let paired_component_reference_offset = value.geometry.paired_component_reference_offset();
        let boundary_segments = value.geometry.boundary.segments();
        let boundary_coordinate_offsets = value.geometry.boundary_coordinate_offsets();
        let second_boundary_present_offset = value.geometry.second_boundary_present_offset();
        let plane_reference_offset = value.geometry.plane_reference_offset();
        let component_reference_offset = value.geometry.component_reference_offset();
        let asset_record_index = value.asset.record_index;
        let asset_reference_offset = value.geometry.asset_reference_offset();
        let asset_byte_offset = value.asset_byte_offset();
        let asset_name_offset = value.asset_name_offset();
        let label_offset = value.geometry.label_offset();
        let geometry_payload = value.geometry.payload.bytes().to_vec();
        let [geometry_class_tag, paired_geometry_class_tag] = value.geometry.class_tags;
        Self {
            id: value.id,
            scope_record_index: value.scope_record_index,
            scope_reference_offset,
            geometry_class_tag,
            geometry_record_index,
            geometry_reference_offset,
            geometry_byte_offset,
            geometry_prologue,
            visible,
            visibility_offset,
            geometry_frame_length,
            paired_geometry_class_tag,
            paired_geometry_byte_offset,
            paired_component_reference_offset,
            boundary_segments,
            boundary_coordinate_offsets,
            second_boundary_present_offset,
            plane_entity_suffix: value.plane_entity_suffix,
            plane_reference_offset,
            component_entity_suffix: value.component_entity_suffix,
            component_reference_offset,
            asset_class_tag: value.asset.class_tag.into(),
            asset_record_index,
            asset_reference_offset,
            asset_byte_offset,
            asset_name: value.asset.name,
            asset_name_offset,
            label: value.geometry.label,
            label_offset,
            opacity,
            origin,
            u_axis,
            v_axis,
            geometry_payload,
        }
    }
}

/// Primary Decal asset and its consecutive image-name record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignDecalAsset {
    class_tags: [String; 2],
    record_index: u32,
    byte_offset: u64,
    entity_suffix: u32,
    name: String,
}
impl DesignDecalAsset {
    pub fn new(
        class_tags: [String; 2],
        record_indices: [u32; 2],
        byte_offset: u64,
        entity_suffix: u32,
        name: String,
    ) -> Result<Self, String> {
        if record_indices[0].checked_add(1) != Some(record_indices[1]) {
            return Err("name_record_index must immediately follow asset_record_index".into());
        }
        for (field, tag) in ["asset_class_tag", "name_class_tag"]
            .into_iter()
            .zip(&class_tags)
        {
            if tag.is_empty() || !tag.bytes().all(|byte| byte.is_ascii_graphic()) {
                return Err(format!("{field} must contain printable ASCII characters"));
            }
        }
        if name.is_empty() {
            return Err("asset_name must be nonempty".into());
        }
        let units = u32::try_from(name.encode_utf16().count())
            .map_err(|_| "asset_name UTF-16 count must fit u32")?;
        let length = crate::layout::design_decal_image_asset_record::LEN as u64
            + crate::layout::design_decal_image_name_prefix::LEN as u64
            + 2 * u64::from(units);
        byte_offset
            .checked_add(length)
            .ok_or("asset_byte_offset and asset_name must leave a complete name record")?;
        Ok(Self {
            class_tags,
            record_index: record_indices[0],
            byte_offset,
            entity_suffix,
            name,
        })
    }
    pub fn record_index(&self) -> u32 {
        self.record_index
    }
    pub fn entity_suffix(&self) -> u32 {
        self.entity_suffix
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub const fn primary_frame_length() -> u64 {
        crate::layout::design_decal_image_asset_record::LEN as u64
    }
    pub fn name_frame_length(&self) -> u64 {
        crate::layout::design_decal_image_name_prefix::LEN as u64
            + 2 * self.name.encode_utf16().count() as u64
    }
    fn name_record_index(&self) -> u32 {
        self.record_index + 1
    }
    fn name_byte_offset(&self) -> u64 {
        self.byte_offset + Self::primary_frame_length()
    }
    fn entity_reference_offset(&self) -> u64 {
        self.byte_offset
            + crate::layout::design_decal_image_asset_record::DESIGN_ENTITY_SUFFIX_REFERENCE as u64
            + 1
    }
    fn name_offset(&self) -> u64 {
        self.name_byte_offset() + crate::layout::design_decal_image_name_prefix::LEN as u64
    }
}

/// Exact image and target binding owned by one Design `Decal` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignDecalImageWire", into = "DesignDecalImageWire")]
pub struct DesignDecalImage {
    /// Globally unique native binding identity.
    pub id: String,
    scope: Located<u32>,
    /// Source mapping-mode byte.
    pub mapping_mode: DesignDecalMappingMode,
    /// Target construction-group record index.
    pub target_group_record_index: u32,
    /// Consecutive asset and image-name records.
    pub asset: DesignDecalAsset,
}
impl DesignDecalImage {
    pub fn new(
        id: String,
        scope: Located<u32>,
        mapping_mode: DesignDecalMappingMode,
        target_group_record_index: u32,
        asset: DesignDecalAsset,
    ) -> Result<Self, String> {
        scope
            .offset
            .checked_add(crate::layout::design_decal_scope_prefix::LEN as u64)
            .ok_or("asset_reference_offset must belong to a complete Decal scope prefix")?;
        Ok(Self {
            id,
            scope,
            mapping_mode,
            target_group_record_index,
            asset,
        })
    }
    pub fn scope_record_index(&self) -> u32 {
        self.scope.value
    }
    pub fn scope_byte_offset(&self) -> u64 {
        self.scope.offset
    }
    fn asset_reference_offset(&self) -> u64 {
        self.scope.offset + crate::layout::design_decal_scope_prefix::ASSET_REFERENCE as u64 + 1
    }
    fn mapping_mode_offset(&self) -> u64 {
        self.scope.offset + crate::layout::design_decal_scope_prefix::MAPPING_MODE as u64
    }
    fn target_group_reference_offset(&self) -> u64 {
        self.scope.offset
            + crate::layout::design_decal_scope_prefix::TARGET_GROUP_REFERENCE as u64
            + 1
    }
}

/// Exact image and target binding owned by one Design `Decal` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignDecalImageWire {
    /// Globally unique deterministic identifier for this native binding.
    id: String,
    /// Decal scope record index.
    scope_record_index: u32,
    /// Byte offset of the scope's marked image-asset reference.
    asset_reference_offset: u64,
    /// Source mapping-mode byte.
    mapping_mode: DesignDecalMappingMode,
    /// Byte offset of `mapping_mode`.
    mapping_mode_offset: u64,
    /// Target construction-group record index.
    target_group_record_index: u32,
    /// Byte offset of the scope's marked target-group reference.
    target_group_reference_offset: u64,
    /// Dynamic class tag of the primary image-asset record.
    asset_class_tag: String,
    /// Primary image-asset record index.
    asset_record_index: u32,
    /// Byte offset of the primary image-asset record.
    asset_byte_offset: u64,
    /// Byte length from the primary image header to the name-record header.
    asset_frame_length: u64,
    /// Design entity suffix carried by the primary image record.
    asset_entity_suffix: u32,
    /// Byte offset of the marked Design entity-suffix reference.
    asset_entity_reference_offset: u64,
    /// Dynamic class tag of the image-name record.
    name_class_tag: String,
    /// Image-name record index.
    name_record_index: u32,
    /// Byte offset of the image-name record.
    name_byte_offset: u64,
    /// Byte length of the complete image-name record.
    name_frame_length: u64,
    /// Archive entry basename stored by the image-name record.
    asset_name: String,
    /// Byte offset of the asset name's UTF-16LE code units.
    asset_name_offset: u64,
}

impl TryFrom<DesignDecalImageWire> for DesignDecalImage {
    type Error = String;
    fn try_from(wire: DesignDecalImageWire) -> Result<Self, Self::Error> {
        let scope_offset = wire
            .asset_reference_offset
            .checked_sub(crate::layout::design_decal_scope_prefix::ASSET_REFERENCE as u64 + 1)
            .ok_or("asset_reference_offset precedes the Decal scope prefix")?;
        let asset = DesignDecalAsset::new(
            [wire.asset_class_tag, wire.name_class_tag],
            [wire.asset_record_index, wire.name_record_index],
            wire.asset_byte_offset,
            wire.asset_entity_suffix,
            wire.asset_name,
        )?;
        let image = Self::new(
            wire.id,
            Located {
                value: wire.scope_record_index,
                offset: scope_offset,
            },
            wire.mapping_mode,
            wire.target_group_record_index,
            asset,
        )?;
        for (name, declared, derived) in [
            (
                "mapping_mode_offset",
                wire.mapping_mode_offset,
                image.mapping_mode_offset(),
            ),
            (
                "target_group_reference_offset",
                wire.target_group_reference_offset,
                image.target_group_reference_offset(),
            ),
            (
                "asset_frame_length",
                wire.asset_frame_length,
                DesignDecalAsset::primary_frame_length(),
            ),
            (
                "asset_entity_reference_offset",
                wire.asset_entity_reference_offset,
                image.asset.entity_reference_offset(),
            ),
            (
                "name_byte_offset",
                wire.name_byte_offset,
                image.asset.name_byte_offset(),
            ),
            (
                "name_frame_length",
                wire.name_frame_length,
                image.asset.name_frame_length(),
            ),
            (
                "asset_name_offset",
                wire.asset_name_offset,
                image.asset.name_offset(),
            ),
        ] {
            if declared != derived {
                return Err(format!("{name} must match the Decal record layout"));
            }
        }
        Ok(image)
    }
}

impl From<DesignDecalImage> for DesignDecalImageWire {
    fn from(value: DesignDecalImage) -> Self {
        let scope_record_index = value.scope_record_index();
        let asset_reference_offset = value.asset_reference_offset();
        let mapping_mode_offset = value.mapping_mode_offset();
        let target_group_reference_offset = value.target_group_reference_offset();
        let asset_record_index = value.asset.record_index;
        let asset_byte_offset = value.asset.byte_offset;
        let asset_frame_length = DesignDecalAsset::primary_frame_length();
        let asset_entity_suffix = value.asset.entity_suffix;
        let asset_entity_reference_offset = value.asset.entity_reference_offset();
        let name_record_index = value.asset.name_record_index();
        let name_byte_offset = value.asset.name_byte_offset();
        let name_frame_length = value.asset.name_frame_length();
        let asset_name_offset = value.asset.name_offset();
        let [asset_class_tag, name_class_tag] = value.asset.class_tags;
        Self {
            id: value.id,
            scope_record_index,
            asset_reference_offset,
            mapping_mode: value.mapping_mode,
            mapping_mode_offset,
            target_group_record_index: value.target_group_record_index,
            target_group_reference_offset,
            asset_class_tag,
            asset_record_index,
            asset_byte_offset,
            asset_frame_length,
            asset_entity_suffix,
            asset_entity_reference_offset,
            name_class_tag,
            name_record_index,
            name_byte_offset,
            name_frame_length,
            asset_name: value.asset.name,
            asset_name_offset,
        }
    }
}

/// One indexed record header in the recursive Design `BulkStream` tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignRecordHeader {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Index of this record within the recursive `BulkStream` tree.
    pub record_index: u32,
    /// Source per-file dynamic three-digit ASCII class tag naming this record's type.
    pub class_tag: DesignClassTag,
    /// Byte offset of this header within its Design `BulkStream`.
    pub byte_offset: u64,
}

const SKETCH_CONSTRAINT_DEFINITIONS: [(u64, SketchConstraintKind); 20] = [
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

pub(crate) const SKETCH_CONSTRAINT_MASK: u64 = {
    let mut mask = 0;
    let mut index = 0;
    while index < SKETCH_CONSTRAINT_DEFINITIONS.len() {
        mask |= SKETCH_CONSTRAINT_DEFINITIONS[index].0;
        index += 1;
    }
    mask
};

/// Decode the constraint kinds and unknown bits selected by a sketch-relation mask.
#[must_use]
pub(crate) fn constraint_kinds_from_state(state: u64) -> (Vec<SketchConstraintKind>, u64) {
    let mut kinds = if state == 0 {
        vec![SketchConstraintKind::Coincident]
    } else {
        Vec::new()
    };
    for (bit, kind) in SKETCH_CONSTRAINT_DEFINITIONS {
        if state & bit != 0 {
            kinds.push(kind);
        }
    }
    (kinds, state & !SKETCH_CONSTRAINT_MASK)
}

/// An indexed relation reference before or after sketch identity resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SketchRelationReference {
    /// Indexed Design record whose identity has not been resolved.
    Index(u32),
    /// Resolved identity, which contains its indexed Design record reference.
    Resolved(SketchRelationOperand),
}

impl SketchRelationReference {
    /// Indexed Design record referenced by this member.
    #[must_use]
    pub fn record_index(&self) -> u32 {
        match self {
            Self::Index(index) => *index,
            Self::Resolved(operand) => operand.record_index(),
        }
    }

    /// Identity after resolution, including a record with no sketch identity.
    #[must_use]
    pub fn resolved(&self) -> Option<&SketchRelationOperand> {
        match self {
            Self::Index(_) => None,
            Self::Resolved(operand) => Some(operand),
        }
    }

    fn from_wire(
        record_index: u32,
        resolved: Option<SketchRelationOperand>,
        field: &str,
    ) -> Result<Self, SketchRelationPayloadError> {
        match resolved {
            None => Ok(Self::Index(record_index)),
            Some(operand) if operand.record_index() == record_index => Ok(Self::Resolved(operand)),
            Some(_) => Err(SketchRelationPayloadError(format!(
                "sketch relation {field} record_index disagrees with its reference"
            ))),
        }
    }
}

/// One first-run sketch-relation member with its offset and ordinal.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchRelationMember {
    /// Indexed record or its resolved sketch identity.
    pub reference: SketchRelationReference,
    /// Payload offset of the member, relative to the record.
    pub offset: u32,
    /// Count of relations already recorded on this member, when retained by the wire.
    pub relation_ordinal: Option<u32>,
}

impl SketchRelationMember {
    /// An unresolved member with zero offset and no retained ordinal.
    #[must_use]
    #[cfg(test)]
    pub fn from_index(record_index: u32) -> Self {
        Self {
            reference: SketchRelationReference::Index(record_index),
            offset: 0,
            relation_ordinal: None,
        }
    }
}

/// One return-run sketch-relation member with its offset.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchRelationReturnMember {
    /// Indexed record or its resolved sketch identity.
    pub reference: SketchRelationReference,
    /// Payload offset of the return member, relative to the record.
    pub offset: u32,
}

impl SketchRelationReturnMember {
    /// An unresolved return member with zero offset.
    #[must_use]
    #[cfg(test)]
    pub fn from_index(record_index: u32) -> Self {
        Self {
            reference: SketchRelationReference::Index(record_index),
            offset: 0,
        }
    }
}

/// A complete first member run, either unresolved or resolved throughout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SketchRelationMembers(Vec<SketchRelationMember>);

impl SketchRelationMembers {
    /// Construct the unresolved source run with its member locations.
    pub(crate) fn from_indices(rows: impl IntoIterator<Item = (u32, u32, u32)>) -> Self {
        Self(
            rows.into_iter()
                .map(
                    |(record_index, offset, relation_ordinal)| SketchRelationMember {
                        reference: SketchRelationReference::Index(record_index),
                        offset,
                        relation_ordinal: Some(relation_ordinal),
                    },
                )
                .collect(),
        )
    }

    /// Resolve every member while retaining its position metadata.
    pub(crate) fn resolve(&mut self, mut resolve: impl FnMut(u32) -> SketchRelationOperand) {
        self.0 = self
            .0
            .iter()
            .map(|row| SketchRelationMember {
                reference: SketchRelationReference::Resolved(resolve(row.reference.record_index())),
                offset: row.offset,
                relation_ordinal: row.relation_ordinal,
            })
            .collect();
    }
}

impl TryFrom<Vec<SketchRelationMember>> for SketchRelationMembers {
    type Error = SketchRelationPayloadError;

    fn try_from(rows: Vec<SketchRelationMember>) -> Result<Self, Self::Error> {
        if rows.windows(2).any(|pair| {
            pair[0].reference.resolved().is_some() != pair[1].reference.resolved().is_some()
        }) {
            return Err(SketchRelationPayloadError(
                "sketch relation members run mixes resolved and unresolved references".into(),
            ));
        }
        if rows
            .windows(2)
            .any(|pair| pair[0].relation_ordinal.is_some() != pair[1].relation_ordinal.is_some())
        {
            return Err(SketchRelationPayloadError(
                "sketch relation member ordinals are only partially present".into(),
            ));
        }
        Ok(Self(rows))
    }
}

impl std::ops::Deref for SketchRelationMembers {
    type Target = [SketchRelationMember];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A complete return member run, either unresolved or resolved throughout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SketchRelationReturnMembers(Vec<SketchRelationReturnMember>);

impl SketchRelationReturnMembers {
    /// Construct the unresolved source run with its member locations.
    pub(crate) fn from_indices(rows: impl IntoIterator<Item = (u32, u32)>) -> Self {
        Self(
            rows.into_iter()
                .map(|(record_index, offset)| SketchRelationReturnMember {
                    reference: SketchRelationReference::Index(record_index),
                    offset,
                })
                .collect(),
        )
    }

    /// Resolve every member while retaining its position metadata.
    pub(crate) fn resolve(&mut self, mut resolve: impl FnMut(u32) -> SketchRelationOperand) {
        self.0 = self
            .0
            .iter()
            .map(|row| SketchRelationReturnMember {
                reference: SketchRelationReference::Resolved(resolve(row.reference.record_index())),
                offset: row.offset,
            })
            .collect();
    }
}

impl TryFrom<Vec<SketchRelationReturnMember>> for SketchRelationReturnMembers {
    type Error = SketchRelationPayloadError;

    fn try_from(rows: Vec<SketchRelationReturnMember>) -> Result<Self, Self::Error> {
        if rows.windows(2).any(|pair| {
            pair[0].reference.resolved().is_some() != pair[1].reference.resolved().is_some()
        }) {
            return Err(SketchRelationPayloadError(
                "sketch relation return_members run mixes resolved and unresolved references"
                    .into(),
            ));
        }
        Ok(Self(rows))
    }
}

impl std::ops::Deref for SketchRelationReturnMembers {
    type Target = [SketchRelationReturnMember];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Finite row-major native glyph placement in centimetres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[[f64; 4]; 4]", into = "[[f64; 4]; 4]")]
pub struct SketchGlyphTransform([[f64; 4]; 4]);

impl SketchGlyphTransform {
    /// Native coefficients, without an affine or invertibility restriction.
    #[must_use]
    pub fn rows(self) -> [[f64; 4]; 4] {
        self.0
    }
}

impl TryFrom<[[f64; 4]; 4]> for SketchGlyphTransform {
    type Error = SketchRelationPayloadError;

    fn try_from(rows: [[f64; 4]; 4]) -> Result<Self, Self::Error> {
        if rows.iter().flatten().any(|value| !value.is_finite()) {
            return Err(SketchRelationPayloadError(
                "sketch relation glyph_transforms contains a non-finite coefficient".into(),
            ));
        }
        Ok(Self(rows))
    }
}

impl From<SketchGlyphTransform> for [[f64; 4]; 4] {
    fn from(transform: SketchGlyphTransform) -> Self {
        transform.0
    }
}

/// Constraint mask and its matching pattern or text payload.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchRelationDefinition {
    state: u64,
    pattern: Option<SketchPatternDefinition>,
}

impl SketchRelationDefinition {
    /// Reject a payload that does not match the mask's first constraint kind.
    pub(crate) fn new(
        state: u64,
        pattern: Option<SketchPatternDefinition>,
    ) -> Result<Self, SketchRelationPayloadError> {
        let (kinds, _) = constraint_kinds_from_state(state);
        let first = kinds.first().copied();
        let agrees = match &pattern {
            None => !matches!(
                first,
                Some(
                    SketchConstraintKind::CircularPattern
                        | SketchConstraintKind::RectangularPattern
                        | SketchConstraintKind::TextFrame
                        | SketchConstraintKind::TextPath
                )
            ),
            Some(SketchPatternDefinition::Circular { .. }) => {
                first == Some(SketchConstraintKind::CircularPattern)
            }
            Some(SketchPatternDefinition::Rectangular { .. }) => {
                first == Some(SketchConstraintKind::RectangularPattern)
            }
            Some(SketchPatternDefinition::TextFrame { .. }) => {
                first == Some(SketchConstraintKind::TextFrame)
            }
            Some(SketchPatternDefinition::TextPath { .. }) => {
                first == Some(SketchConstraintKind::TextPath)
            }
        };
        if !agrees {
            return Err(SketchRelationPayloadError(
                "sketch relation pattern disagrees with the first constraint kind".into(),
            ));
        }
        Ok(Self { state, pattern })
    }

    /// Source sketch-constraint bitmask.
    #[must_use]
    pub fn state(&self) -> u64 {
        self.state
    }

    /// Pattern or text payload selected by the mask.
    #[must_use]
    pub fn pattern(&self) -> Option<&SketchPatternDefinition> {
        self.pattern.as_ref()
    }
}

/// Rejected sketch-relation payload or inconsistent native wire columns.
#[derive(Debug)]
pub struct SketchRelationPayloadError(String);

impl std::fmt::Display for SketchRelationPayloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SketchRelationPayloadError {}

/// Counted constraint relation owned by a sketch container.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "SketchRelationSerde", into = "SketchRelationSerde")]
pub struct SketchRelation {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Index of this relation record within the `BulkStream` tree.
    pub record_index: u32,
    /// Source per-file dynamic three-digit ASCII class tag naming this relation's type.
    pub class_tag: DesignClassTag,
    /// Byte offset of this record within its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of the constraint mask relative to the record start.
    pub state_offset: u32,
    /// Numeric design-entity suffix of the sketch container that owns this relation.
    pub owner_reference: u32,
    /// Full Design entity id resolved from `owner_reference`.
    #[serde(default)]
    pub owner_entity_id: Option<cadmpeg_ir::NonEmptyString>,
    /// Nullable or role-specific references stored before the owner reference.
    pub auxiliary_references: ReferenceRun<u32, u32>,
    /// Serialized count of the rectangular class's reference run. Zero selects
    /// seed-to-final spans; a nonzero count selects adjacent spacing. `None`
    /// for other relation classes and native data that did not retain it.
    pub rectangular_counted_reference_count: Option<u32>,
    /// First reference run, interleaved with per-member relation ordinals.
    /// Its order does not define relation operand order.
    pub members: SketchRelationMembers,
    /// Payload offset of `owner_reference`, relative to the record.
    pub owner_reference_offset: u32,
    /// Constraint mask and the payload it selects.
    pub definition: SketchRelationDefinition,
    /// `EntityGenesis` origin bitfield stored by the relation record, when present.
    pub entity_genesis: Option<u64>,
    /// Second reference run in semantic member order.
    pub return_members: SketchRelationReturnMembers,
    /// Complete variable-width source record for native replay/write.
    pub raw_bytes: Vec<u8>,
}

impl SketchRelation {
    /// Constraint kinds selected by `state`.
    #[must_use]
    pub fn constraint_kinds(&self) -> Vec<SketchConstraintKind> {
        constraint_kinds_from_state(self.definition.state()).0
    }

    /// Bits in `state` outside the defined constraint mask.
    #[must_use]
    pub fn unknown_constraint_bits(&self) -> u64 {
        constraint_kinds_from_state(self.definition.state()).1
    }

    /// Record indices of the first reference run.
    #[must_use]
    pub fn member_indices(&self) -> Vec<u32> {
        self.members
            .iter()
            .map(|member| member.reference.record_index())
            .collect()
    }

    /// Record indices of the return reference run.
    #[must_use]
    pub fn return_member_indices(&self) -> Vec<u32> {
        self.return_members
            .iter()
            .map(|member| member.reference.record_index())
            .collect()
    }

    /// First-run then return-run record indices.
    pub fn all_member_indices(&self) -> impl Iterator<Item = u32> + '_ {
        self.members
            .iter()
            .map(|member| member.reference.record_index())
            .chain(
                self.return_members
                    .iter()
                    .map(|member| member.reference.record_index()),
            )
    }

    /// Resolved first-run members, empty for an unresolved run.
    #[must_use]
    pub fn resolved_members(&self) -> Vec<SketchRelationOperand> {
        self.members
            .iter()
            .filter_map(|member| member.reference.resolved().cloned())
            .collect()
    }

    /// Resolved return-run members, empty for an unresolved run.
    #[must_use]
    pub fn resolved_return_members(&self) -> Vec<SketchRelationOperand> {
        self.return_members
            .iter()
            .filter_map(|member| member.reference.resolved().cloned())
            .collect()
    }
}

fn zip_relation_members(
    members: Vec<u32>,
    offsets: Vec<u32>,
    ordinals: Vec<u32>,
    resolved: Vec<SketchRelationOperand>,
) -> Result<SketchRelationMembers, SketchRelationPayloadError> {
    let len = members.len();
    let offsets = pad_or_check("member_offsets", offsets, len)?;
    let ordinals = if ordinals.is_empty() {
        (0..len).map(|_| None).collect::<Vec<_>>()
    } else if ordinals.len() == len {
        ordinals.into_iter().map(Some).collect()
    } else {
        return Err(SketchRelationPayloadError(
            "sketch relation member_relation_ordinals length does not match members".into(),
        ));
    };
    let resolved = pad_resolved("resolved_members", resolved, len)?;
    members
        .into_iter()
        .zip(offsets)
        .zip(ordinals)
        .zip(resolved)
        .map(|(((record_index, offset), relation_ordinal), resolved)| {
            Ok(SketchRelationMember {
                reference: SketchRelationReference::from_wire(
                    record_index,
                    resolved,
                    "resolved_members",
                )?,
                offset,
                relation_ordinal,
            })
        })
        .collect::<Result<Vec<_>, SketchRelationPayloadError>>()?
        .try_into()
}

fn zip_return_members(
    members: Vec<u32>,
    offsets: Vec<u32>,
    resolved: Vec<SketchRelationOperand>,
) -> Result<SketchRelationReturnMembers, SketchRelationPayloadError> {
    let len = members.len();
    let offsets = pad_or_check("return_member_offsets", offsets, len)?;
    let resolved = pad_resolved("resolved_return_members", resolved, len)?;
    members
        .into_iter()
        .zip(offsets)
        .zip(resolved)
        .map(|((record_index, offset), resolved)| {
            Ok(SketchRelationReturnMember {
                reference: SketchRelationReference::from_wire(
                    record_index,
                    resolved,
                    "resolved_return_members",
                )?,
                offset,
            })
        })
        .collect::<Result<Vec<_>, SketchRelationPayloadError>>()?
        .try_into()
}

fn pad_or_check(
    name: &str,
    values: Vec<u32>,
    len: usize,
) -> Result<Vec<u32>, SketchRelationPayloadError> {
    if values.is_empty() {
        cadmpeg_core::decode::alloc_filled(len, 0, "pad sketch relation offsets")
            .map_err(|error| SketchRelationPayloadError(error.to_string()))
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
        cadmpeg_core::decode::alloc_filled(len, None, "pad sketch relation resolutions")
            .map_err(|error| SketchRelationPayloadError(error.to_string()))
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
        let definition = SketchRelationDefinition::new(wire.state, wire.pattern)?;
        Ok(Self {
            id: wire.id,
            record_index: wire.record_index,
            class_tag: DesignClassTag::try_from(wire.class_tag)
                .map_err(SketchRelationPayloadError)?,
            byte_offset: wire.byte_offset,
            state_offset: wire.state_offset,
            owner_reference: wire.owner_reference,
            owner_entity_id: cadmpeg_ir::NonEmptyString::new(wire.owner_entity_id),
            auxiliary_references: ReferenceRun::from_columns(
                wire.auxiliary_references,
                wire.auxiliary_reference_offsets,
                "auxiliary_reference",
            )
            .map_err(SketchRelationPayloadError)?,
            rectangular_counted_reference_count: wire.rectangular_counted_reference_count,
            members: zip_relation_members(
                wire.members,
                wire.member_offsets,
                wire.member_relation_ordinals,
                wire.resolved_members,
            )?,
            owner_reference_offset: wire.owner_reference_offset,
            definition,
            entity_genesis: wire.entity_genesis,
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
            constraint_kinds_from_state(relation.definition.state());
        let (auxiliary_references, auxiliary_reference_offsets) =
            relation.auxiliary_references.into_wire();
        Self {
            id: relation.id,
            record_index: relation.record_index,
            class_tag: relation.class_tag.into(),
            byte_offset: relation.byte_offset,
            state_offset: relation.state_offset,
            owner_reference: relation.owner_reference,
            owner_entity_id: relation
                .owner_entity_id
                .map(|owner| owner.as_str().to_owned())
                .unwrap_or_default(),
            auxiliary_references,
            auxiliary_reference_offsets,
            rectangular_counted_reference_count: relation.rectangular_counted_reference_count,
            members: relation
                .members
                .iter()
                .map(|member| member.reference.record_index())
                .collect(),
            resolved_members: relation
                .members
                .iter()
                .filter_map(|member| member.reference.resolved().cloned())
                .collect(),
            member_offsets: relation
                .members
                .iter()
                .map(|member| member.offset)
                .collect(),
            owner_reference_offset: relation.owner_reference_offset,
            state: relation.definition.state(),
            constraint_kinds,
            unknown_constraint_bits,
            member_relation_ordinals: relation
                .members
                .iter()
                .filter_map(|member| member.relation_ordinal)
                .collect(),
            entity_genesis: relation.entity_genesis,
            pattern: relation.definition.pattern,
            return_members: relation
                .return_members
                .iter()
                .map(|member| member.reference.record_index())
                .collect(),
            resolved_return_members: relation
                .return_members
                .iter()
                .filter_map(|member| member.reference.resolved().cloned())
                .collect(),
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

impl SketchRelationOperand {
    /// Indexed Design record that owns this identity.
    #[must_use]
    pub fn record_index(&self) -> u32 {
        match self {
            Self::Point { record_index, .. }
            | Self::Curve { record_index, .. }
            | Self::Surface { record_index, .. }
            | Self::Record { record_index } => *record_index,
        }
    }
}

/// One bit in a Fusion sketch-constraint state mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// A sketch pattern instance count in 1..=100000.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct SketchPatternCount(u32);
impl TryFrom<u32> for SketchPatternCount {
    type Error = &'static str;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if !(1..=100_000).contains(&value) {
            return Err("evaluated_count must be in 1..=100000");
        }
        Ok(Self(value))
    }
}
impl From<SketchPatternCount> for u32 {
    fn from(value: SketchPatternCount) -> Self {
        value.0
    }
}
impl SketchPatternCount {
    pub(crate) fn get(self) -> u32 {
        self.0
    }
}

/// Class-specific auxiliary payload of a pattern or text sketch relation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
        evaluated_count: SketchPatternCount,
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
        glyph_transforms: Vec<SketchGlyphTransform>,
    },
}

/// One direction clause of a rectangular-pattern sketch relation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchPatternDirection {
    /// Evaluated instance count along this direction.
    pub evaluated_count: SketchPatternCount,
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
#[serde(try_from = "SketchTextSerde", into = "SketchTextSerde")]
pub struct SketchText {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Index of this text record within the `BulkStream` tree.
    pub record_index: u32,
    /// Owning sketch record index.
    pub owner_reference: u32,
    /// Source per-file dynamic ASCII class tag naming this record's type.
    pub class_tag: DesignClassTag,
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
    pub raw_bytes: Vec<u8>,
}

/// Horizontal and vertical alignment members of a sketch-text record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SketchTextAlignment {
    pub horizontal: u32,
    pub vertical: u32,
}

/// `txt_tag` versus `textex_tag` member layout of one sketch-text record.
#[derive(Debug, Clone, PartialEq)]
pub enum SketchTextLayout {
    TxtTag {
        placement: TextPlacement,
    },
    TextexTag {
        width_factor: f64,
        alignment: Option<SketchTextAlignment>,
        first_reference: Option<u32>,
        second_reference: Option<u32>,
        placement: Option<TextPlacement>,
    },
}

impl SketchText {
    pub(crate) fn width_factor(&self) -> Option<f64> {
        match self.layout {
            SketchTextLayout::TxtTag { .. } => None,
            SketchTextLayout::TextexTag { width_factor, .. } => Some(width_factor),
        }
    }

    pub(crate) fn placement(&self) -> Option<TextPlacement> {
        match self.layout {
            SketchTextLayout::TxtTag { placement } => Some(placement),
            SketchTextLayout::TextexTag { placement, .. } => placement,
        }
    }

    pub(crate) fn alignment(&self) -> Option<SketchTextAlignment> {
        match self.layout {
            SketchTextLayout::TxtTag { .. } => None,
            SketchTextLayout::TextexTag { alignment, .. } => alignment,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    raw_bytes: Vec<u8>,
}

impl TryFrom<SketchTextSerde> for SketchText {
    type Error = String;

    fn try_from(wire: SketchTextSerde) -> Result<Self, Self::Error> {
        let placement = match (wire.anchor, wire.rotation) {
            (None, None) => None,
            (Some(anchor), Some(rotation)) => Some(TextPlacement {
                anchor,
                rotation: Angle(rotation),
            }),
            _ => return Err("sketch text anchor and rotation must occur together".into()),
        };
        let alignment =
            match (wire.horizontal_alignment, wire.vertical_alignment) {
                (None, None) => None,
                (Some(horizontal), Some(vertical)) => Some(SketchTextAlignment {
                    horizontal,
                    vertical,
                }),
                _ => return Err(
                    "sketch text horizontal_alignment and vertical_alignment must occur together"
                        .into(),
                ),
            };
        let layout = match (
            wire.width_factor,
            alignment,
            wire.first_reference,
            wire.second_reference,
            placement,
        ) {
            (None, None, None, None, Some(placement)) => SketchTextLayout::TxtTag { placement },
            (Some(width_factor), alignment, first_reference, second_reference, placement) => {
                SketchTextLayout::TextexTag {
                    width_factor,
                    alignment,
                    first_reference,
                    second_reference,
                    placement,
                }
            }
            _ => {
                return Err(
                    "sketch text layout disagrees with width_factor, alignment, and placement"
                        .into(),
                )
            }
        };
        Ok(Self {
            id: wire.id,
            record_index: wire.record_index,
            owner_reference: wire.owner_reference,
            class_tag: wire.class_tag.try_into()?,
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
        let (width_factor, alignment, first_reference, second_reference, placement) =
            match text.layout {
                SketchTextLayout::TxtTag { placement } => (None, None, None, None, Some(placement)),
                SketchTextLayout::TextexTag {
                    width_factor,
                    alignment,
                    first_reference,
                    second_reference,
                    placement,
                } => (
                    Some(width_factor),
                    alignment,
                    first_reference,
                    second_reference,
                    placement,
                ),
            };
        Self {
            id: text.id,
            record_index: text.record_index,
            owner_reference: text.owner_reference,
            class_tag: text.class_tag.into(),
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
            anchor: placement.map(|value| value.anchor),
            rotation: placement.map(|value| value.rotation.0),
            horizontal_alignment: alignment.map(|value| value.horizontal),
            vertical_alignment: alignment.map(|value| value.vertical),
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
#[derive(Debug, Clone, PartialEq)]
pub enum SketchPointRecordForm {
    /// Class version 0: one flag, two coordinates, and no persistent identity.
    Version0 { flag: bool },
    /// Class version 8: seven flags and an eight-zero closure lane.
    Version8 {
        persistent_id: std::num::NonZeroU64,
        flags: [bool; 7],
        /// Third sketch coordinate in millimetres.
        depth: f64,
    },
    /// Class version 10 with same-segment references and seven flags.
    Version10 {
        /// Third sketch coordinate in millimetres.
        depth: f64,
        persistent_id: std::num::NonZeroU64,
        flags: [bool; 7],
        closure: SketchPointClosure10,
    },
    /// Class version 10 with inline target-type GUIDs on its references.
    Version10InlineTyped {
        /// Third sketch coordinate in millimetres.
        depth: f64,
        /// Final inline-typed reference following the repeated companion reference.
        trailing_reference: u32,
        persistent_id: std::num::NonZeroU64,
        flags: [bool; 7],
        closure: SketchPointClosure10Inline,
    },
    /// Class version 11 with same-segment references and eight flags.
    Version11 {
        /// Third sketch coordinate in millimetres.
        depth: f64,
        /// Optional origin bitfield preceding the persistent identity.
        entity_genesis: Option<u64>,
        /// Whether four fixed zero bytes follow the repeated companion reference.
        padded_paired_reference: bool,
        persistent_id: std::num::NonZeroU64,
        flags: [bool; 8],
        closure: SketchPointClosure,
    },
    /// Class version 11 with inline target-type GUIDs on its references and eight flags.
    Version11InlineTyped {
        /// Third sketch coordinate in millimetres.
        depth: f64,
        /// Optional origin bitfield preceding the persistent identity.
        entity_genesis: Option<u64>,
        /// Final inline-typed reference following the repeated companion reference.
        trailing_reference: u32,
        persistent_id: std::num::NonZeroU64,
        flags: [bool; 8],
        closure: SketchPointClosure,
    },
}

impl SketchPointRecordForm {
    #[cfg(test)]
    pub(crate) fn version11(
        persistent_id: u64,
        closure: SketchPointClosure,
        entity_genesis: Option<u64>,
        depth: f64,
    ) -> Self {
        Self::Version11 {
            depth,
            entity_genesis,
            padded_paired_reference: false,
            persistent_id: std::num::NonZeroU64::new(persistent_id).unwrap(),
            flags: [false; 8],
            closure,
        }
    }

    pub(crate) fn depth(&self) -> f64 {
        match *self {
            Self::Version0 { .. } => 0.0,
            Self::Version8 { depth, .. }
            | Self::Version10 { depth, .. }
            | Self::Version10InlineTyped { depth, .. }
            | Self::Version11 { depth, .. }
            | Self::Version11InlineTyped { depth, .. } => depth,
        }
    }

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
            | Self::Version11InlineTyped { persistent_id, .. } => Some(persistent_id.get()),
        }
    }

    pub(crate) fn flags(&self) -> [u8; 8] {
        let mut flags = [0; 8];
        match self {
            Self::Version0 { flag, .. } => flags[0] = u8::from(*flag),
            Self::Version8 { flags: source, .. }
            | Self::Version10 { flags: source, .. }
            | Self::Version10InlineTyped { flags: source, .. } => {
                flags[..7].copy_from_slice(&source.map(u8::from));
            }
            Self::Version11 { flags: source, .. }
            | Self::Version11InlineTyped { flags: source, .. } => flags = source.map(u8::from),
        }
        flags
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
#[serde(rename_all = "snake_case")]
pub enum SketchPointCompanionReferenceEncoding {
    /// Target entity ID followed directly by the same-segment flags.
    #[default]
    SameSegment,
    /// Target entity ID followed by the target type GUID and same-segment flags.
    InlineTyped,
}

/// Reverse curve-incidence record paired with a version-11 sketch point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SketchPointCompanion {
    /// Whether the companion prefix carries the fixed present-zero member.
    pub prefix_present_zero: bool,
    /// Incident sketch-curve record indexes in serialized order.
    pub incident_curves: Vec<u32>,
}

impl SketchPointCompanion {
    fn validate_for(&self, form: &SketchPointRecordForm) -> Result<(), String> {
        if self.prefix_present_zero && form.class_version() != 11 {
            return Err("sketch point companion.prefix_present_zero requires version 11".into());
        }
        let unique: std::collections::HashSet<_> = self.incident_curves.iter().collect();
        if unique.len() != self.incident_curves.len() {
            return Err("sketch point companion.incident_curves must be distinct".into());
        }
        Ok(())
    }
}

/// Borrowed companion payload with the prefix derived for older point forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SketchPointCompanionRef<'a> {
    pub prefix_present_zero: bool,
    pub incident_curves: &'a [u32],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SketchPointCompanionWire {
    prefix_present_zero: bool,
    #[serde(default)]
    reference_encoding: SketchPointCompanionReferenceEncoding,
    #[serde(default)]
    incident_curves: Vec<u32>,
}

// Serde requires `skip_serializing_if` predicates to borrow the field.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn sketch_point_flags_are_zero(flags: &[u8; 8]) -> bool {
    flags.iter().all(|flag| *flag == 0)
}

/// One point in a Fusion sketch coordinate system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub class_tag: DesignClassTag,
    /// Byte offset of this record within its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of the first coordinate relative to the record start.
    pub coordinate_offset: u32,
    /// Serialized point-record member sequence, identity, flags, and closure.
    record_form: SketchPointRecordForm,
    companion: SketchPointCompanion,
    /// Record index of the paired reverse curve-incidence companion.
    pub paired_reference: u32,
    /// First two sketch coordinates in millimetres.
    coordinates: Point2,
}

#[derive(Debug, Clone)]
pub(crate) struct SketchPointDraft {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Index of this point record within the `BulkStream` tree.
    pub record_index: u32,
    /// Resolved owning-sketch reference from a direct backlink, typed relation,
    /// or sketch-container member run.
    pub owner_reference: Option<u32>,
    /// Source per-file dynamic three-digit ASCII class tag naming this point's record type.
    pub class_tag: DesignClassTag,
    /// Byte offset of this record within its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of the first coordinate relative to the record start.
    pub coordinate_offset: u32,
    /// Serialized point-record member sequence, identity, flags, and closure.
    pub record_form: SketchPointRecordForm,
    pub companion: SketchPointCompanion,
    /// Record index of the paired reverse curve-incidence companion.
    pub paired_reference: u32,
    /// First two sketch coordinates in millimetres.
    pub coordinates: Point2,
}
impl TryFrom<SketchPointDraft> for SketchPoint {
    type Error = String;
    fn try_from(draft: SketchPointDraft) -> Result<Self, Self::Error> {
        if !draft.coordinates.u.is_finite() || !draft.coordinates.v.is_finite() {
            return Err("sketch point coordinates must be finite".into());
        }
        if !draft.record_form.depth().is_finite() {
            return Err("sketch point depth must be finite".into());
        }
        draft.companion.validate_for(&draft.record_form)?;
        Ok(Self {
            id: draft.id,
            record_index: draft.record_index,
            owner_reference: draft.owner_reference,
            class_tag: draft.class_tag,
            byte_offset: draft.byte_offset,
            coordinate_offset: draft.coordinate_offset,
            record_form: draft.record_form,
            companion: draft.companion,
            paired_reference: draft.paired_reference,
            coordinates: draft.coordinates,
        })
    }
}

impl SketchPoint {
    pub(crate) fn coordinates(&self) -> Point2 {
        self.coordinates
    }
    pub(crate) fn record_form(&self) -> &SketchPointRecordForm {
        &self.record_form
    }
    pub(crate) fn try_set_coordinates(&mut self, coordinates: Point2) -> Result<(), String> {
        if !coordinates.u.is_finite() || !coordinates.v.is_finite() {
            return Err("sketch point coordinates must be finite".into());
        }
        self.coordinates = coordinates;
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn try_set_record_form(
        &mut self,
        record_form: SketchPointRecordForm,
    ) -> Result<(), String> {
        if !record_form.depth().is_finite() {
            return Err("sketch point depth must be finite".into());
        }
        self.companion.validate_for(&record_form)?;
        self.record_form = record_form;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn try_set_companion(
        &mut self,
        companion: SketchPointCompanion,
    ) -> Result<(), String> {
        companion.validate_for(&self.record_form)?;
        self.companion = companion;
        Ok(())
    }
    pub(crate) fn companion(&self) -> SketchPointCompanionRef<'_> {
        SketchPointCompanionRef {
            prefix_present_zero: self.companion.prefix_present_zero,
            incident_curves: &self.companion.incident_curves,
        }
    }

    pub(crate) fn depth(&self) -> f64 {
        self.record_form.depth()
    }

    pub(crate) fn entity_genesis(&self) -> Option<u64> {
        match self.record_form {
            SketchPointRecordForm::Version11 { entity_genesis, .. }
            | SketchPointRecordForm::Version11InlineTyped { entity_genesis, .. } => entity_genesis,
            SketchPointRecordForm::Version0 { .. }
            | SketchPointRecordForm::Version8 { .. }
            | SketchPointRecordForm::Version10 { .. }
            | SketchPointRecordForm::Version10InlineTyped { .. } => None,
        }
    }

    pub(crate) fn persistent_id(&self) -> Option<u64> {
        self.record_form.persistent_id()
    }

    pub(crate) fn flags(&self) -> [u8; 8] {
        self.record_form.flags()
    }

    pub(crate) fn closure(&self) -> Option<SketchPointClosure> {
        self.record_form.closure()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    companion: Option<SketchPointCompanionWire>,
}

impl Default for SketchPointRecordFormSerde {
    fn default() -> Self {
        Self::Version11 {
            padded_paired_reference: false,
        }
    }
}

fn seven_flags(flags: [bool; 8]) -> Result<[bool; 7], String> {
    if flags[7] {
        return Err("sketch point flags beyond the form width must be zero".into());
    }
    let mut dest = [false; 7];
    dest.copy_from_slice(&flags[..7]);
    Ok(dest)
}

impl TryFrom<SketchPointSerde> for SketchPoint {
    type Error = String;

    fn try_from(wire: SketchPointSerde) -> Result<Self, Self::Error> {
        if wire.flags.iter().any(|flag| *flag > 1) {
            return Err("sketch point flags must be zero or one".into());
        }
        if wire.entity_genesis.is_some()
            && !matches!(
                wire.record_form,
                SketchPointRecordFormSerde::Version11 { .. }
                    | SketchPointRecordFormSerde::Version11InlineTyped { .. }
            )
        {
            return Err("sketch point entity_genesis requires version 11".into());
        }
        if matches!(wire.record_form, SketchPointRecordFormSerde::Version0) && wire.depth != 0.0 {
            return Err("sketch point depth must be zero for version 0".into());
        }
        let flags = wire.flags.map(|flag| flag == 1);
        let closure = wire.closure.map(SketchPointClosure::try_from).transpose()?;
        let persistent_id = wire
            .persistent_id
            .map(|value| std::num::NonZeroU64::new(value).ok_or("persistent_id must be nonzero"))
            .transpose()?;
        let record_form = match (wire.record_form, persistent_id, closure) {
            (SketchPointRecordFormSerde::Version0, None, None) => {
                SketchPointRecordForm::Version0 { flag: flags[0] }
            }
            (
                SketchPointRecordFormSerde::Version8,
                Some(persistent_id),
                Some(SketchPointClosure::Selector0State0),
            ) => SketchPointRecordForm::Version8 {
                depth: wire.depth,
                persistent_id,
                flags: seven_flags(flags)?,
            },
            (SketchPointRecordFormSerde::Version10, Some(persistent_id), Some(closure)) => {
                SketchPointRecordForm::Version10 {
                    depth: wire.depth,
                    persistent_id,
                    flags: seven_flags(flags)?,
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
                depth: wire.depth,
                trailing_reference,
                persistent_id,
                flags: seven_flags(flags)?,
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
                depth: wire.depth,
                entity_genesis: wire.entity_genesis,
                padded_paired_reference,
                persistent_id,
                flags,
                closure,
            },
            (
                SketchPointRecordFormSerde::Version11InlineTyped { trailing_reference },
                Some(persistent_id),
                Some(closure),
            ) => SketchPointRecordForm::Version11InlineTyped {
                depth: wire.depth,
                entity_genesis: wire.entity_genesis,
                trailing_reference,
                persistent_id,
                flags,
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
        let companion = wire.companion.ok_or("sketch point companion is required")?;
        let inline_typed =
            companion.reference_encoding == SketchPointCompanionReferenceEncoding::InlineTyped;
        if inline_typed != record_form.uses_inline_typed_references() {
            return Err(
                "sketch point companion.reference_encoding disagrees with record_form".into(),
            );
        }
        let companion = SketchPointCompanion {
            prefix_present_zero: companion.prefix_present_zero,
            incident_curves: companion.incident_curves,
        };
        Self::try_from(SketchPointDraft {
            id: wire.id,
            record_index: wire.record_index,
            owner_reference: wire.owner_reference,
            class_tag: wire.class_tag.try_into()?,
            byte_offset: wire.byte_offset,
            coordinate_offset: wire.coordinate_offset,
            record_form,
            companion,
            paired_reference: wire.paired_reference,
            coordinates: wire.coordinates,
        })
    }
}

impl From<SketchPoint> for SketchPointSerde {
    fn from(point: SketchPoint) -> Self {
        let reference_encoding = if point.record_form.uses_inline_typed_references() {
            SketchPointCompanionReferenceEncoding::InlineTyped
        } else {
            SketchPointCompanionReferenceEncoding::SameSegment
        };
        let depth = point.depth();
        let entity_genesis = point.entity_genesis();
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
        let companion = Some(SketchPointCompanionWire {
            prefix_present_zero: point.companion.prefix_present_zero,
            reference_encoding,
            incident_curves: point.companion.incident_curves,
        });
        Self {
            id: point.id,
            record_index: point.record_index,
            owner_reference: point.owner_reference,
            class_tag: point.class_tag.into(),
            byte_offset: point.byte_offset,
            coordinate_offset: point.coordinate_offset,
            entity_genesis,
            record_form,
            persistent_id,
            paired_reference: point.paired_reference,
            flags,
            coordinates: point.coordinates,
            depth,
            closure,
            companion,
        }
    }
}

/// Persistent identity pair attached to one source sketch-curve record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchCurveIdentity {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Index of this identity record within the `BulkStream` tree.
    pub record_index: u32,
    /// Direct owning-sketch backlink when the curve record form carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_reference: Option<u32>,
    /// Source per-file dynamic three-digit ASCII class tag naming this record's type.
    pub class_tag: DesignClassTag,
    /// Byte offset of this record within its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of the fixed analytic geometry payload relative to the record start.
    pub geometry_offset: u32,
    /// Optional `EntityGenesis` origin bitfield carried ahead of the curve identities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_genesis: Option<u64>,
    /// Primary persistent identifier of the source sketch curve.
    pub primary_id: std::num::NonZeroU64,
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
pub struct SketchSurface {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Index of this surface record within the `BulkStream` tree.
    pub record_index: u32,
    /// Owning sketch entity derived from relations using this surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_reference: Option<u32>,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: DesignClassTag,
    /// Byte offset of this record within its Design `BulkStream`.
    pub byte_offset: u64,
    /// Optional `EntityGenesis` origin bitfield carried ahead of the surface identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_genesis: Option<u64>,
    /// Persistent Fusion identifier for the sketch surface.
    pub persistent_id: std::num::NonZeroU64,
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
#[serde(try_from = "SketchCurveGeometryWire", into = "SketchCurveGeometryWire")]
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
        subtype_class_tag: DesignClassTag,
        /// Record index of the NURBS subtype record.
        subtype_record_index: u32,
        /// Polynomial degree of the curve.
        degree: u32,
        /// Source fit tolerance used when the curve was fitted, in millimetres.
        fit_tolerance: f64,
        /// Width in scalars of each control-point record as stored in the source
        /// (control point components plus weight, before decoding into `poles`).
        scalar_width: u32,
        /// Knot vector, non-decreasing, length `poles.point_count() + degree + 1`.
        knots: Vec<f64>,
        /// Polynomial control points or rational point/weight pairs.
        poles: SketchNurbsPoles,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SketchCurveGeometryWire {
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

impl TryFrom<SketchCurveGeometryWire> for SketchCurveGeometry {
    type Error = String;
    fn try_from(wire: SketchCurveGeometryWire) -> Result<Self, Self::Error> {
        Ok(match wire {
            SketchCurveGeometryWire::Line {
                start,
                end,
                direction,
                normal,
            } => Self::Line {
                start,
                end,
                direction,
                normal,
            },
            SketchCurveGeometryWire::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } => Self::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            },
            SketchCurveGeometryWire::Nurbs {
                carrier_reference,
                subtype_class_tag,
                subtype_record_index,
                degree,
                fit_tolerance,
                scalar_width,
                knots,
                weights,
                control_points,
            } => Self::Nurbs {
                carrier_reference,
                subtype_class_tag: DesignClassTag::try_from(subtype_class_tag)
                    .map_err(|error| format!("subtype_class_tag: {error}"))?,
                subtype_record_index,
                degree,
                fit_tolerance,
                scalar_width,
                knots,
                poles: SketchNurbsPoles::from_wire(control_points, weights)?,
            },
        })
    }
}

impl From<SketchCurveGeometry> for SketchCurveGeometryWire {
    fn from(geometry: SketchCurveGeometry) -> Self {
        match geometry {
            SketchCurveGeometry::Line {
                start,
                end,
                direction,
                normal,
            } => Self::Line {
                start,
                end,
                direction,
                normal,
            },
            SketchCurveGeometry::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } => Self::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            },
            SketchCurveGeometry::Nurbs {
                carrier_reference,
                subtype_class_tag,
                subtype_record_index,
                degree,
                fit_tolerance,
                scalar_width,
                knots,
                poles,
            } => {
                let (control_points, weights) = match poles {
                    SketchNurbsPoles::Polynomial(points) => (points, Vec::new()),
                    SketchNurbsPoles::Rational(poles) => poles
                        .into_iter()
                        .map(|pole| (pole.point, pole.weight))
                        .unzip(),
                };
                Self::Nurbs {
                    carrier_reference,
                    subtype_class_tag: subtype_class_tag.into(),
                    subtype_record_index,
                    degree,
                    fit_tolerance,
                    scalar_width,
                    knots,
                    weights,
                    control_points,
                }
            }
        }
    }
}

/// Control data for a polynomial or rational sketch spline.
#[derive(Debug, Clone)]
pub enum SketchNurbsPoles {
    Polynomial(Vec<Point3>),
    Rational(Vec<SketchNurbsPole>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SketchNurbsPole {
    pub point: Point3,
    pub weight: f64,
}

impl PartialEq for SketchNurbsPoles {
    fn eq(&self, other: &Self) -> bool {
        self.points().eq(other.points()) && self.weights().eq(other.weights())
    }
}

impl SketchNurbsPoles {
    pub(crate) fn from_wire(points: Vec<Point3>, weights: Vec<f64>) -> Result<Self, String> {
        if weights.is_empty() {
            return Ok(Self::Polynomial(points));
        }
        if points.len() != weights.len() {
            return Err("weights must be absent or match every control_points entry".into());
        }
        Ok(Self::Rational(
            points
                .into_iter()
                .zip(weights)
                .map(|(point, weight)| SketchNurbsPole { point, weight })
                .collect(),
        ))
    }

    pub fn point_count(&self) -> usize {
        match self {
            Self::Polynomial(points) => points.len(),
            Self::Rational(poles) => poles.len(),
        }
    }

    pub fn points(&self) -> impl DoubleEndedIterator<Item = &Point3> {
        let (points, poles): (&[Point3], &[SketchNurbsPole]) = match self {
            Self::Polynomial(points) => (points, &[]),
            Self::Rational(poles) => (&[], poles),
        };
        points.iter().chain(poles.iter().map(|pole| &pole.point))
    }

    pub fn weights(&self) -> impl ExactSizeIterator<Item = &f64> {
        let poles: &[SketchNurbsPole] = match self {
            Self::Polynomial(_) => &[],
            Self::Rational(poles) => poles,
        };
        poles.iter().map(|pole| &pole.weight)
    }

    #[cfg(test)]
    pub fn points_mut(&mut self) -> impl Iterator<Item = &mut Point3> {
        let (points, poles): (&mut [Point3], &mut [SketchNurbsPole]) = match self {
            Self::Polynomial(points) => (points, &mut []),
            Self::Rational(poles) => (&mut [], poles),
        };
        points
            .iter_mut()
            .chain(poles.iter_mut().map(|pole| &mut pole.point))
    }
}

/// One member of the Design `BulkStream` `BodiesRoot` list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[serde(try_from = "DesignBodyBoundsWire", into = "DesignBodyBoundsWire")]
pub struct DesignBodyBounds {
    /// Globally unique deterministic identifier for this native record set.
    pub id: String,
    /// Numeric suffix of the owning Design body entity.
    entity_suffix: u32,
    /// Byte offset of the owning Design entity header.
    pub entity_byte_offset: u64,
    /// Indexed-header byte offsets parallel to `record_indices`.
    record_byte_offsets: [u64; 3],
    /// First f64 byte of each repeated sextuple.
    value_byte_offsets: [u64; 3],
    /// Design BREP body-map pairs carrying this entity suffix, in stream order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body_binding_ids: Vec<String>,
    corners: DesignMeshSceneBounds,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct DesignBodyBoundsWire {
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
impl TryFrom<DesignBodyBoundsWire> for DesignBodyBounds {
    type Error = String;
    fn try_from(wire: DesignBodyBoundsWire) -> Result<Self, Self::Error> {
        let entity_suffix = u32::try_from(wire.entity_suffix)
            .map_err(|_| "entity_suffix exceeds indexed record range")?;
        let last = entity_suffix
            .checked_add(3)
            .ok_or("entity_suffix leaves no room for three cache records")?;
        if wire.record_indices != [last - 2, last - 1, last] {
            return Err("record_indices must follow entity_suffix consecutively".into());
        }
        if !wire
            .record_byte_offsets
            .windows(2)
            .all(|pair| pair[0] < pair[1])
        {
            return Err("record_byte_offsets must be strictly increasing".into());
        }
        if !wire
            .value_byte_offsets
            .iter()
            .zip(wire.record_byte_offsets)
            .all(|(value, record)| *value > record)
        {
            return Err("value_byte_offsets must follow record_byte_offsets".into());
        }
        let maximum = [wire.maximum.x, wire.maximum.y, wire.maximum.z];
        let minimum = [wire.minimum.x, wire.minimum.y, wire.minimum.z];
        let corners = DesignMeshSceneBounds::new(maximum, minimum)?;
        if maximum == minimum {
            return Err("maximum and minimum must not define a degenerate box".into());
        }
        Ok(Self {
            id: wire.id,
            entity_suffix,
            entity_byte_offset: wire.entity_byte_offset,
            record_byte_offsets: wire.record_byte_offsets,
            value_byte_offsets: wire.value_byte_offsets,
            body_binding_ids: wire.body_binding_ids,
            corners,
        })
    }
}
impl DesignBodyBounds {
    pub(crate) fn entity_suffix(&self) -> u64 {
        u64::from(self.entity_suffix)
    }
    fn record_indices(&self) -> [u32; 3] {
        [
            self.entity_suffix + 1,
            self.entity_suffix + 2,
            self.entity_suffix + 3,
        ]
    }
}
impl From<DesignBodyBounds> for DesignBodyBoundsWire {
    fn from(value: DesignBodyBounds) -> Self {
        let [x, y, z] = value.corners.maximum();
        let maximum = Point3::new(x, y, z);
        let [x, y, z] = value.corners.minimum();
        Self {
            entity_suffix: value.entity_suffix(),
            record_indices: value.record_indices(),
            id: value.id,
            entity_byte_offset: value.entity_byte_offset,
            record_byte_offsets: value.record_byte_offsets,
            value_byte_offsets: value.value_byte_offsets,
            body_binding_ids: value.body_binding_ids,
            maximum,
            minimum: Point3::new(x, y, z),
        }
    }
}

/// One ordered pair in a Design `BulkStream` BREP body-map record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignBodyBindingWire", into = "DesignBodyBindingWire")]
pub struct DesignBodyBinding {
    /// Globally unique deterministic identifier for this native map entry.
    pub id: String,
    /// Design `BulkStream` ZIP entry containing the map.
    pub stream: String,
    /// Number of pairs in the enclosing body map.
    pair_count: std::num::NonZeroU32,
    /// Zero-based position in the enclosing body map.
    pair_ordinal: u32,
    /// BREP body selector stored by this pair.
    pub asm_body_key: u64,
    /// Byte offset of `asm_body_key` within `stream`.
    asm_body_key_offset: u64,
    /// Numeric Design entity suffix stored by this pair.
    pub entity_suffix: u64,
    /// Basename of the BREP blob whose body namespace contains the key.
    blob_name: String,
    /// Byte offset of the UTF-16LE `blob_name` code units within `stream`.
    blob_name_offset: u64,
    /// Solved body in the BREP blob named by this pair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<BodyId>,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct DesignBodyBindingWire {
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
impl TryFrom<DesignBodyBindingWire> for DesignBodyBinding {
    type Error = String;
    fn try_from(wire: DesignBodyBindingWire) -> Result<Self, Self::Error> {
        let pair_count =
            std::num::NonZeroU32::new(wire.pair_count).ok_or("pair_count must be nonzero")?;
        if wire.pair_ordinal >= pair_count.get() {
            return Err("pair_ordinal must be less than pair_count".into());
        }
        let entity_suffix_offset = wire
            .asm_body_key_offset
            .checked_add(8)
            .ok_or("asm_body_key_offset overflows entity_suffix_offset")?;
        if wire.entity_suffix_offset != entity_suffix_offset {
            return Err(
                "entity_suffix_offset must follow asm_body_key_offset by eight bytes".into(),
            );
        }
        if !wire.blob_name.starts_with("BREP.") {
            return Err("blob_name must start with BREP.".into());
        }
        if wire.blob_name_offset <= entity_suffix_offset {
            return Err("blob_name_offset must follow entity_suffix_offset".into());
        }
        Ok(Self {
            pair_count,
            id: wire.id,
            stream: wire.stream,
            pair_ordinal: wire.pair_ordinal,
            asm_body_key: wire.asm_body_key,
            asm_body_key_offset: wire.asm_body_key_offset,
            entity_suffix: wire.entity_suffix,
            blob_name: wire.blob_name,
            blob_name_offset: wire.blob_name_offset,
            body: wire.body,
        })
    }
}
impl DesignBodyBinding {
    pub(crate) fn pair_count(&self) -> u32 {
        self.pair_count.get()
    }
    pub(crate) fn pair_ordinal(&self) -> u32 {
        self.pair_ordinal
    }
    pub(crate) fn asm_body_key_offset(&self) -> u64 {
        self.asm_body_key_offset
    }
    pub(crate) fn entity_suffix_offset(&self) -> u64 {
        self.asm_body_key_offset + 8
    }
    pub(crate) fn blob_name(&self) -> &str {
        &self.blob_name
    }
    pub(crate) fn blob_name_offset(&self) -> u64 {
        self.blob_name_offset
    }
}
impl From<DesignBodyBinding> for DesignBodyBindingWire {
    fn from(value: DesignBodyBinding) -> Self {
        Self {
            pair_count: value.pair_count(),
            entity_suffix_offset: value.entity_suffix_offset(),
            id: value.id,
            stream: value.stream,
            pair_ordinal: value.pair_ordinal,
            asm_body_key: value.asm_body_key,
            asm_body_key_offset: value.asm_body_key_offset,
            entity_suffix: value.entity_suffix,
            blob_name: value.blob_name,
            blob_name_offset: value.blob_name_offset,
            body: value.body,
        }
    }
}

/// Design browser-node visibility joined to one solved ASM body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    record_index_offset: u64,
}

impl ActTableRow {
    pub(crate) fn new(record_index_offset: u64) -> Result<Self, String> {
        if record_index_offset.checked_add(14).is_none() {
            return Err("table_record_index_offset overflows table_entity_id_offset".into());
        }
        Ok(Self {
            record_index_offset,
        })
    }

    fn entity_id_offset(&self) -> u64 {
        self.record_index_offset + 14
    }
}

/// Non-padding bytes following an ACT channel group.
#[derive(Debug, Clone, PartialEq)]
pub struct ActClassTail {
    bytes: Vec<u8>,
    offset: u64,
}

impl ActClassTail {
    pub(crate) fn new(bytes: Vec<u8>, offset: u64) -> Result<Self, String> {
        if bytes.iter().all(|byte| *byte == 0) {
            return Err("channel_class_tail must contain non-padding bytes".into());
        }
        if u64::try_from(bytes.len())
            .ok()
            .and_then(|len| offset.checked_add(len))
            .is_none()
        {
            return Err("channel_class_tail_offset and channel_class_tail length overflow".into());
        }
        Ok(Self { bytes, offset })
    }

    #[cfg(test)]
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn offset(&self) -> u64 {
        self.offset
    }
}

/// Channel-group payload owned by one ACT entity.
#[derive(Debug, Clone, PartialEq)]
pub struct ActChannelGroup {
    record_index_offset: u64,
    entity_id_offset: Option<u64>,
    class_tag: DesignClassTag,
    channels: BTreeMap<String, Located<DesignGuidText>>,
    class_tail: Option<ActClassTail>,
}

fn validate_act_channel_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 128 || !name.is_ascii() {
        return Err("ACT channel name must contain 1 through 128 ASCII bytes".into());
    }
    Ok(())
}

impl ActChannelGroup {
    pub(crate) fn try_new(
        record_index_offset: u64,
        entity_id_offset: Option<u64>,
        class_tag: DesignClassTag,
        channels: BTreeMap<String, Located<DesignGuidText>>,
        class_tail: Option<ActClassTail>,
    ) -> Result<Self, String> {
        if channels.is_empty() || channels.len() > 8 {
            return Err("ACT channels must contain 1 through 8 entries".into());
        }
        if entity_id_offset.is_some_and(|offset| offset <= record_index_offset) {
            return Err("channel_entity_id_offset must follow channel_record_index_offset".into());
        }
        for (name, guid) in &channels {
            validate_act_channel_name(name)?;
            let end = guid
                .offset
                .checked_add(72)
                .ok_or("channel_guid_offsets overflow")?;
            if guid.offset <= record_index_offset
                || entity_id_offset.is_some_and(|offset| end > offset)
            {
                return Err(
                    "channel_guid_offsets must follow the record index and precede the entity key"
                        .into(),
                );
            }
            if class_tail.as_ref().is_some_and(|tail| end > tail.offset()) {
                return Err("channel_guid_offsets must precede channel_class_tail_offset".into());
            }
        }
        if class_tail.as_ref().is_some_and(|tail| {
            record_index_offset >= tail.offset()
                || entity_id_offset.is_some_and(|offset| offset >= tail.offset())
        }) {
            return Err(
                "channel record and entity offsets must precede channel_class_tail_offset".into(),
            );
        }
        Ok(Self {
            record_index_offset,
            entity_id_offset,
            class_tag,
            channels,
            class_tail,
        })
    }
    pub(crate) fn channels(&self) -> &BTreeMap<String, Located<DesignGuidText>> {
        &self.channels
    }
}

/// One Fusion ACT change-version channel group and its optional inline table row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ActEntitySerde", into = "ActEntitySerde")]
pub struct ActEntity {
    /// Globally unique deterministic identifier for this native record.
    id: NativeRecordId,
    /// Record index shared by the channel group and its optional ACTTable row.
    record_index: u32,
    entity_id: String,
    table_row: Option<ActTableRow>,
    channel_group: ActChannelGroup,
}

impl ActEntity {
    /// Returns the admitted native identity.
    pub(crate) fn id(&self) -> &String {
        &self.id.text
    }
    /// Returns the ACT entity record index.
    pub(crate) fn record_index(&self) -> u32 {
        self.record_index
    }
    /// Returns the native stream encoded in the identity.
    pub(crate) fn stream(&self) -> &str {
        self.id.stream()
    }
    pub(crate) fn try_new(
        id: String,
        record_index: u32,
        entity_id: String,
        table_row: Option<ActTableRow>,
        channel_group: ActChannelGroup,
    ) -> Result<Self, String> {
        let id = NativeRecordId::try_new(id, "act-entity", record_index)?;
        if !crate::act::is_entity_key(&entity_id) {
            return Err("ACT entity_id must be a decimal segment_entity key".into());
        }
        if table_row.is_none() && channel_group.entity_id_offset.is_none() {
            return Err("channel_entity_id_offset is required without an ACTTable row".into());
        }
        Ok(Self {
            id,
            record_index,
            entity_id,
            table_row,
            channel_group,
        })
    }
    pub(crate) fn entity_id(&self) -> &str {
        &self.entity_id
    }
    pub(crate) fn try_set_entity_id(&mut self, entity_id: String) -> Result<(), String> {
        if !crate::act::is_entity_key(&entity_id) {
            return Err("ACT entity_id must be a decimal segment_entity key".into());
        }
        self.entity_id = entity_id;
        Ok(())
    }
    pub(crate) fn in_table(&self) -> bool {
        self.table_row.is_some()
    }
    pub(crate) fn channel_group(&self) -> &ActChannelGroup {
        &self.channel_group
    }
    pub(crate) fn set_channel_guid(
        &mut self,
        name: &str,
        value: DesignGuidText,
    ) -> Result<(), String> {
        let guid = self
            .channel_group
            .channels
            .get_mut(name)
            .ok_or("ACT channel does not exist")?;
        guid.value = value;
        Ok(())
    }
    pub(crate) fn table_record_index_offset(&self) -> Option<u64> {
        self.table_row.as_ref().map(|row| row.record_index_offset)
    }
    pub(crate) fn table_entity_id_offset(&self) -> Option<u64> {
        self.table_row.as_ref().map(ActTableRow::entity_id_offset)
    }
    pub(crate) fn channel_record_index_offset(&self) -> u64 {
        self.channel_group.record_index_offset
    }
    pub(crate) fn channel_entity_id_offset(&self) -> Option<u64> {
        self.channel_group.entity_id_offset
    }
    pub(crate) fn channel_class_tag(&self) -> &str {
        self.channel_group.class_tag.as_str()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
        if wire.channels.keys().ne(wire.channel_guid_offsets.keys()) {
            return Err("channels and channel_guid_offsets must have identical keys".into());
        }
        let table = match (
            wire.in_table,
            wire.table_record_index_offset,
            wire.table_entity_id_offset,
        ) {
            (true, Some(record_index_offset), Some(entity_id_offset)) => {
                let row = ActTableRow::new(record_index_offset)?;
                if row.entity_id_offset() != entity_id_offset {
                    return Err(
                        "table_entity_id_offset must follow table_record_index_offset by 14 bytes"
                            .into(),
                    );
                }
                Some(row)
            }
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
            (Some(class_tag), Some(record_index_offset)) => Some(ActChannelGroup::try_new(
                record_index_offset,
                wire.channel_entity_id_offset,
                class_tag
                    .try_into()
                    .map_err(|error| format!("channel_class_tag: {error}"))?,
                wire.channels
                    .into_iter()
                    .zip(wire.channel_guid_offsets)
                    .map(|((name, value), (_, offset))| {
                        Ok((
                            name,
                            Located {
                                value: value.try_into()?,
                                offset,
                            },
                        ))
                    })
                    .collect::<Result<_, String>>()?,
                match (wire.channel_class_tail, wire.channel_class_tail_offset) {
                    (bytes, None) if bytes.is_empty() => None,
                    (bytes, Some(offset)) => Some(ActClassTail::new(bytes, offset)?),
                    _ => return Err("channel_class_tail requires channel_class_tail_offset".into()),
                },
            )?),
            _ => {
                return Err(
                    "act entity channel_class_tag disagrees with channel_record_index_offset"
                        .into(),
                );
            }
        };
        Self::try_new(
            wire.id,
            wire.record_index,
            wire.entity_id,
            table,
            group.ok_or("ACT entity requires a channel group")?,
        )
    }
}

impl From<ActEntity> for ActEntitySerde {
    fn from(entity: ActEntity) -> Self {
        let in_table = entity.in_table();
        let table_record_index_offset = entity.table_record_index_offset();
        let table_entity_id_offset = entity.table_entity_id_offset();
        let channel_record_index_offset = Some(entity.channel_record_index_offset());
        let channel_entity_id_offset = entity.channel_entity_id_offset();
        let channel_class_tag = Some(entity.channel_class_tag().to_owned());
        let group = entity.channel_group;
        let (channels, channel_guid_offsets) = group
            .channels
            .into_iter()
            .map(|(name, guid)| {
                (
                    (name.clone(), guid.value.as_str().to_owned()),
                    (name, guid.offset),
                )
            })
            .unzip();
        let (channel_class_tail, channel_class_tail_offset) = match group.class_tail {
            Some(tail) => (tail.bytes, Some(tail.offset)),
            None => (Vec::new(), None),
        };
        Self {
            id: entity.id.text,
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
#[serde(try_from = "ActGuidWire", into = "ActGuidWire")]
pub struct ActGuid {
    /// Globally unique deterministic identifier for this native record.
    id: NativeRecordId,
    /// Byte offset of the UTF-16 length prefix in the ACT `BulkStream`.
    byte_offset: u64,
    /// Position in the pool in source order; does not assign a GUID to one table entry.
    pub ordinal: u32,
    /// The pooled GUID string.
    pub guid: DesignGuidText,
}

impl ActGuid {
    /// Returns the admitted native identity.
    pub(crate) fn id(&self) -> &String {
        &self.id.text
    }
    /// Returns the native stream encoded in the identity.
    pub(crate) fn stream(&self) -> &str {
        self.id.stream()
    }
    pub fn new(id: String, byte_offset: u64, ordinal: u32, guid: String) -> Result<Self, String> {
        let id = NativeRecordId::try_new(id, "act-guid", byte_offset)?;
        byte_offset
            .checked_add(4)
            .ok_or("ACT GUID byte_offset overflows guid_offset")?;
        Ok(Self {
            id,
            byte_offset,
            ordinal,
            guid: guid.try_into()?,
        })
    }

    pub fn byte_offset(&self) -> u64 {
        self.byte_offset
    }

    pub fn guid_offset(&self) -> u64 {
        self.byte_offset + 4
    }
}

#[derive(Serialize, Deserialize)]
struct ActGuidWire {
    id: String,
    byte_offset: u64,
    guid_offset: u64,
    ordinal: u32,
    guid: String,
}

impl TryFrom<ActGuidWire> for ActGuid {
    type Error = String;

    fn try_from(wire: ActGuidWire) -> Result<Self, Self::Error> {
        let guid = Self::new(wire.id, wire.byte_offset, wire.ordinal, wire.guid)?;
        if wire.guid_offset != guid.guid_offset() {
            return Err("ACT GUID guid_offset must follow byte_offset by four bytes".into());
        }
        Ok(guid)
    }
}

impl From<ActGuid> for ActGuidWire {
    fn from(guid: ActGuid) -> Self {
        let guid_offset = guid.guid_offset();
        Self {
            id: guid.id.text,
            byte_offset: guid.byte_offset,
            guid_offset,
            ordinal: guid.ordinal,
            guid: guid.guid.into(),
        }
    }
}

/// One reference in the ACT table run between the GUID pool and channel registry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ActTableReferenceWire", into = "ActTableReferenceWire")]
pub struct ActTableReference {
    /// Globally unique deterministic identifier for this native record.
    id: NativeRecordId,
    /// Position in the counted reference run, in source order.
    pub ordinal: u32,
    /// Byte offset of the reference-presence marker in the ACT `BulkStream`.
    byte_offset: u64,
    /// Target ACT record index.
    pub target_record: u32,
}

impl ActTableReference {
    /// Returns the admitted native identity.
    pub(crate) fn id(&self) -> &String {
        &self.id.text
    }
    /// Returns the native stream encoded in the identity.
    pub(crate) fn stream(&self) -> &str {
        self.id.stream()
    }
    pub fn new(
        id: String,
        ordinal: u32,
        byte_offset: u64,
        target_record: u32,
    ) -> Result<Self, String> {
        let id = NativeRecordId::try_new(id, "act-table-reference", byte_offset)?;
        byte_offset
            .checked_add(1)
            .ok_or("ACT table reference byte_offset overflows target_record_offset")?;
        Ok(Self {
            id,
            ordinal,
            byte_offset,
            target_record,
        })
    }

    pub fn byte_offset(&self) -> u64 {
        self.byte_offset
    }

    fn target_record_offset(&self) -> u64 {
        self.byte_offset + 1
    }
}

#[derive(Serialize, Deserialize)]
struct ActTableReferenceWire {
    id: String,
    ordinal: u32,
    byte_offset: u64,
    target_record: u32,
    target_record_offset: u64,
}

impl TryFrom<ActTableReferenceWire> for ActTableReference {
    type Error = String;

    fn try_from(wire: ActTableReferenceWire) -> Result<Self, Self::Error> {
        let reference = Self::new(wire.id, wire.ordinal, wire.byte_offset, wire.target_record)?;
        if wire.target_record_offset != reference.target_record_offset() {
            return Err("ACT table reference target_record_offset must follow byte_offset".into());
        }
        Ok(reference)
    }
}

impl From<ActTableReference> for ActTableReferenceWire {
    fn from(reference: ActTableReference) -> Self {
        let target_record_offset = reference.target_record_offset();
        Self {
            id: reference.id.text,
            ordinal: reference.ordinal,
            byte_offset: reference.byte_offset,
            target_record: reference.target_record,
            target_record_offset,
        }
    }
}

/// One named entry in the ACT table's stream-wide channel registry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ActRegistryChannelWire", into = "ActRegistryChannelWire")]
pub struct ActRegistryChannel {
    id: NativeRecordId,
    pub ordinal: u32,
    byte_offset: u64,
    name: String,
    pub guid: DesignGuidText,
}

impl ActRegistryChannel {
    /// Returns the admitted native identity.
    pub(crate) fn id(&self) -> &String {
        &self.id.text
    }
    /// Returns the native stream encoded in the identity.
    pub(crate) fn stream(&self) -> &str {
        self.id.stream()
    }
    pub fn new(
        id: String,
        ordinal: u32,
        byte_offset: u64,
        name: String,
        guid: String,
    ) -> Result<Self, String> {
        let id = NativeRecordId::try_new(id, "act-registry-channel", byte_offset)?;
        validate_act_channel_name(&name)?;
        byte_offset
            .checked_add(8 + name.len() as u64)
            .ok_or("ACT registry offset overflow")?;
        Ok(Self {
            id,
            ordinal,
            byte_offset,
            name,
            guid: guid.try_into()?,
        })
    }
    pub fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn name_offset(&self) -> u64 {
        self.byte_offset + 4
    }
    pub fn guid_offset(&self) -> u64 {
        self.byte_offset + 8 + self.name.len() as u64
    }
}

#[derive(Serialize, Deserialize)]
struct ActRegistryChannelWire {
    id: String,
    ordinal: u32,
    byte_offset: u64,
    name: String,
    name_offset: u64,
    guid: String,
    guid_offset: u64,
}

impl TryFrom<ActRegistryChannelWire> for ActRegistryChannel {
    type Error = String;
    fn try_from(wire: ActRegistryChannelWire) -> Result<Self, Self::Error> {
        let channel = Self::new(
            wire.id,
            wire.ordinal,
            wire.byte_offset,
            wire.name,
            wire.guid,
        )?;
        if wire.name_offset != channel.name_offset() || wire.guid_offset != channel.guid_offset() {
            return Err("ACT registry offsets must follow the stored name layout".into());
        }
        Ok(channel)
    }
}

impl From<ActRegistryChannel> for ActRegistryChannelWire {
    fn from(channel: ActRegistryChannel) -> Self {
        let name_offset = channel.name_offset();
        let guid_offset = channel.guid_offset();
        Self {
            id: channel.id.text,
            ordinal: channel.ordinal,
            byte_offset: channel.byte_offset,
            name: channel.name,
            name_offset,
            guid: channel.guid.as_str().into(),
            guid_offset,
        }
    }
}

/// ACT link from the document root entity to the instance/component registries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ActRootComponentWire", into = "ActRootComponentWire")]
pub struct ActRootComponent {
    /// Globally unique deterministic identifier for this native record.
    id: NativeRecordId,
    /// Index of this record within the ACT `BulkStream`.
    pub record_index: u32,
    /// Source per-file dynamic three-digit ASCII class tag naming this record's type.
    pub class_tag: DesignClassTag,
    /// Record index of the instance registry root.
    pub instance_root_record: u32,
    /// Record index of the components registry root.
    pub components_root_record: u32,
    /// Source counter/registry flag; 0 and 1 are both valid.
    pub registry_flag: ActRegistryFlag,
    /// Checked source layout and the two variable-length strings.
    layout: ActRootLayout,
}

impl ActRootComponent {
    /// Returns the admitted native identity.
    pub(crate) fn id(&self) -> &String {
        &self.id.text
    }
    /// Returns the ACT root source layout.
    pub(crate) fn layout(&self) -> &ActRootLayout {
        &self.layout
    }
    /// Returns the native stream encoded in the identity.
    pub(crate) fn stream(&self) -> &str {
        self.id.stream()
    }
    /// Admits a record whose identity matches its source location.
    pub(crate) fn try_new(
        id: String,
        record_index: u32,
        class_tag: DesignClassTag,
        instance_root_record: u32,
        components_root_record: u32,
        registry_flag: ActRegistryFlag,
        layout: ActRootLayout,
    ) -> Result<Self, String> {
        let id = NativeRecordId::try_new(id, "act-root-component", layout.byte_offset())?;
        Ok(Self {
            id,
            record_index,
            class_tag,
            instance_root_record,
            components_root_record,
            registry_flag,
            layout,
        })
    }
    /// Changes root strings without changing the identity offset.
    pub(crate) fn try_set_strings(
        &mut self,
        entity_id: String,
        display_name: String,
    ) -> Result<(), String> {
        self.layout = self.layout.with_strings(entity_id, display_name)?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct ActRootComponentWire {
    /// Globally unique deterministic identifier for this native record.
    id: String,
    /// Byte offset of this record in the ACT `BulkStream`.
    byte_offset: u64,
    /// Index of this record within the ACT `BulkStream`.
    record_index: u32,
    /// Byte offset of `record_index`.
    record_index_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag naming this record's type.
    class_tag: String,
    /// Record index of the instance registry root.
    instance_root_record: u32,
    /// Byte offset of `instance_root_record`.
    instance_root_record_offset: u64,
    /// Record index of the Design entity tracked by this link. Value `3`
    /// identifies the document root.
    #[serde(default)]
    tracked_entity_record: u32,
    /// Byte offset of `tracked_entity_record`.
    #[serde(default)]
    tracked_entity_record_offset: u64,
    /// Record index of the components registry root.
    components_root_record: u32,
    /// Byte offset of `components_root_record`.
    components_root_record_offset: u64,
    /// Source counter/registry flag; 0 and 1 are both valid.
    registry_flag: ActRegistryFlag,
    /// Byte offset of `registry_flag`.
    registry_flag_offset: u64,
    /// UTF-16LE-decoded design-entity id of the document root entity.
    entity_id: String,
    /// Byte offset of the UTF-16 `entity_id` code units.
    entity_id_offset: u64,
    /// Document display name as stored alongside this root-component link.
    display_name: String,
    /// Byte offset of the UTF-16 `display_name` code units.
    display_name_offset: u64,
}

impl TryFrom<ActRootComponentWire> for ActRootComponent {
    type Error = String;

    fn try_from(wire: ActRootComponentWire) -> Result<Self, Self::Error> {
        if wire.tracked_entity_record != 3 {
            return Err("tracked_entity_record must identify document root record 3".into());
        }
        let display_bytes = u64::try_from(wire.display_name.encode_utf16().count())
            .ok()
            .and_then(|length| length.checked_mul(2))
            .ok_or("display_name length overflows")?;
        let padding = wire
            .display_name_offset
            .checked_add(display_bytes)
            .and_then(|end| wire.components_root_record_offset.checked_sub(end))
            .and_then(|gap| gap.checked_sub(1))
            .ok_or("components_root_record_offset does not follow display_name")?;
        let layout =
            ActRootLayout::new(wire.byte_offset, wire.entity_id, wire.display_name, padding)?;
        if wire.record_index_offset != layout.record_index_offset() {
            return Err("record_index_offset disagrees with ACT root layout".into());
        }
        if wire.instance_root_record_offset != layout.instance_root_record_offset() {
            return Err("instance_root_record_offset disagrees with ACT root layout".into());
        }
        if wire.tracked_entity_record_offset != layout.tracked_entity_record_offset() {
            return Err("tracked_entity_record_offset disagrees with ACT root layout".into());
        }
        if wire.registry_flag_offset != layout.registry_flag_offset() {
            return Err("registry_flag_offset disagrees with ACT root layout".into());
        }
        if wire.entity_id_offset != layout.entity_id_offset() {
            return Err("entity_id_offset disagrees with ACT root layout".into());
        }
        if wire.display_name_offset != layout.display_name_offset() {
            return Err("display_name_offset disagrees with ACT root layout".into());
        }
        Self::try_new(
            wire.id,
            wire.record_index,
            wire.class_tag
                .try_into()
                .map_err(|error| format!("class_tag: {error}"))?,
            wire.instance_root_record,
            wire.components_root_record,
            wire.registry_flag,
            layout,
        )
    }
}

impl From<ActRootComponent> for ActRootComponentWire {
    fn from(root: ActRootComponent) -> Self {
        Self {
            id: root.id.text,
            record_index: root.record_index,
            class_tag: root.class_tag.into(),
            instance_root_record: root.instance_root_record,
            components_root_record: root.components_root_record,
            registry_flag: root.registry_flag,
            record_index_offset: root.layout.record_index_offset(),
            instance_root_record_offset: root.layout.instance_root_record_offset(),
            tracked_entity_record_offset: root.layout.tracked_entity_record_offset(),
            registry_flag_offset: root.layout.registry_flag_offset(),
            entity_id_offset: root.layout.entity_id_offset(),
            display_name_offset: root.layout.display_name_offset(),
            components_root_record_offset: root.layout.components_root_record_offset(),
            tracked_entity_record: 3,
            byte_offset: root.layout.byte_offset,
            entity_id: root.layout.entity_id,
            display_name: root.layout.display_name,
        }
    }
}

/// Source extent of an ACT root link. Offsets follow its fixed grammar.
#[derive(Debug, Clone, PartialEq)]
pub struct ActRootLayout {
    byte_offset: u64,
    entity_id: String,
    display_name: String,
    padding: u64,
}

impl ActRootLayout {
    pub fn new(
        byte_offset: u64,
        entity_id: String,
        display_name: String,
        padding: u64,
    ) -> Result<Self, String> {
        if !crate::act::is_entity_key(&entity_id) {
            return Err("entity_id must be an ACT entity key".into());
        }
        if !(1..=8).contains(&padding) {
            return Err(
                "components_root_record_offset requires one through eight padding bytes".into(),
            );
        }
        let string_bytes = entity_id
            .encode_utf16()
            .count()
            .checked_add(display_name.encode_utf16().count())
            .and_then(|length| u64::try_from(length).ok())
            .and_then(|length| length.checked_mul(2))
            .ok_or("ACT root string lengths overflow")?;
        byte_offset
            .checked_add(56)
            .and_then(|offset| offset.checked_add(string_bytes))
            .and_then(|offset| offset.checked_add(padding))
            .ok_or("components_root_record_offset overflows ACT root layout")?;
        Ok(Self {
            byte_offset,
            entity_id,
            display_name,
            padding,
        })
    }

    pub fn with_strings(&self, entity_id: String, display_name: String) -> Result<Self, String> {
        Self::new(self.byte_offset, entity_id, display_name, self.padding)
    }

    pub fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    pub fn entity_id(&self) -> &str {
        &self.entity_id
    }
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
    pub fn record_index_offset(&self) -> u64 {
        self.byte_offset + 7
    }
    pub fn instance_root_record_offset(&self) -> u64 {
        self.byte_offset + 22
    }
    pub fn entity_id_offset(&self) -> u64 {
        self.byte_offset + 36
    }
    pub fn tracked_entity_record_offset(&self) -> u64 {
        self.entity_id_offset() + self.entity_id.encode_utf16().count() as u64 * 2 + 1
    }
    pub fn registry_flag_offset(&self) -> u64 {
        self.tracked_entity_record_offset() + 10
    }
    pub fn display_name_offset(&self) -> u64 {
        self.registry_flag_offset() + 8
    }
    pub fn components_root_record_offset(&self) -> u64 {
        self.display_name_offset()
            + self.display_name.encode_utf16().count() as u64 * 2
            + self.padding
            + 1
    }
}

/// One design entry of the top-level `RedirectionsStream.dat` table
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
/// The first source entry describes the document itself; each further entry
/// describes one referenced document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// The role also accepts a GUID prefix followed by an underscore and URN, beyond relaxed GUID text.
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

pub(crate) mod dimension_locus_arenas;
pub(crate) mod dimension_null_locus_wire;
