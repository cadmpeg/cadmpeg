// SPDX-License-Identifier: Apache-2.0
//! Loft subdata, profiles, sections, and compound loft constructions.

use super::pcurve::PcurveGeometry;
use super::revision_forms::{RevisionCacheForm, RevisionSurfaceParameterization};
use crate::ids::{CurveId, SurfaceId};
use crate::math::Vector3;
use crate::scalar::PositiveI64;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroI64;

/// One scalar row in native loft subdata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: ::serde::de::DeserializeOwned"))]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub struct LoftSubdataRow<R = f64> {
    /// Leading ordered scalar pair.
    pub parameters: [R; 2],
    /// Ordered per-column scalar pairs; empty for subdata type 211.
    pub columns: Vec<[R; 2]>,
    /// Trailing scalar pair stored by the revision-gated row encoding.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_extra"
    )]
    pub extra: Option<[R; 2]>,
}

/// Native loft constraint table with structurally consistent dimensions.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub enum LoftSubdata<R = f64> {
    /// Type 211 stores exactly one leading pair and no column pairs.
    Type211 {
        /// Native row/column header values. They do not count this form's payload.
        dimensions: [i64; 2],
        /// The sole leading scalar pair.
        row: [R; 2],
    },
    /// All other table types store rows of one shared column width.
    Table(LoftSubdataTable<R>),
}

/// Checked non-211 loft table payload.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(into = "LoftSubdataTableWire<R>"))]
#[serde(
    try_from = "LoftSubdataTableWire<R>",
    bound(deserialize = "R: ::serde::de::DeserializeOwned")
)]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub struct LoftSubdataTable<R = f64> {
    pub(super) type_code: i64,
    pub(super) rows: Vec<LoftSubdataRow<R>>,
    pub(super) row_count: i64,
    pub(super) column_count: i64,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
#[serde(bound(deserialize = "R: ::serde::de::DeserializeOwned"))]
pub(super) struct LoftSubdataTableWire<R = f64> {
    /// Native loft table type code.
    pub(super) type_code: i64,
    /// Table rows, all of one column width.
    pub(super) rows: Vec<LoftSubdataRow<R>>,
}

/// The written form of a loft table, borrowing its rows.
#[derive(Serialize)]
pub(super) struct LoftSubdataTableWriteWire<'a, R> {
    pub(super) type_code: i64,
    pub(super) rows: &'a [LoftSubdataRow<R>],
}

impl<R: crate::structural_order::StructuralOrd> crate::structural_order::StructuralOrd
    for LoftSubdataTable<R>
{
    /// The order of the written form, `type_code` and then `rows`.
    fn structural_cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.type_code
            .structural_cmp(&other.type_code)
            .then_with(|| self.rows.structural_cmp(&other.rows))
    }
}

impl<R: Serialize> Serialize for LoftSubdataTable<R> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        LoftSubdataTableWriteWire {
            type_code: self.type_code,
            rows: &self.rows,
        }
        .serialize(serializer)
    }
}

/// The rows of a loft table do not share one column width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("loft subdata rows do not share one column width")]
pub struct RaggedLoftTable;

impl<R> LoftSubdataTable<R> {
    /// Admit a table whose rows share one column width.
    pub fn new(type_code: i64, rows: Vec<LoftSubdataRow<R>>) -> Result<Self, RaggedLoftTable> {
        let row_count = i64::try_from(rows.len()).map_err(|_| RaggedLoftTable)?;
        let column_count = rows.first().map_or(0, |row| row.columns.len());
        let column_count_i64 = i64::try_from(column_count).map_err(|_| RaggedLoftTable)?;
        if rows.iter().any(|row| row.columns.len() != column_count) {
            return Err(RaggedLoftTable);
        }
        Ok(Self {
            type_code,
            rows,
            row_count,
            column_count: column_count_i64,
        })
    }
}

impl<R> TryFrom<LoftSubdataTableWire<R>> for LoftSubdataTable<R> {
    type Error = RaggedLoftTable;

    fn try_from(wire: LoftSubdataTableWire<R>) -> Result<Self, Self::Error> {
        Self::new(wire.type_code, wire.rows)
    }
}

impl LoftSubdata {
    /// Construct the fixed type-211 form.
    #[must_use]
    pub fn type_211(dimensions: [i64; 2], row: [f64; 2]) -> Self {
        Self::Type211 { dimensions, row }
    }

    /// Construct a non-211 table whose rows have one shared column width.
    #[must_use]
    pub fn table(type_code: i64, rows: Vec<LoftSubdataRow>) -> Option<Self> {
        LoftSubdataTable::new(type_code, rows).ok().map(Self::Table)
    }
}

impl<R> LoftSubdata<R> {
    /// Native table type discriminator.
    #[must_use]
    pub fn type_code(&self) -> i64 {
        match self {
            Self::Type211 { .. } => 211,
            Self::Table(table) => table.type_code,
        }
    }

    /// Native row header: a stored value for type 211, otherwise the row count.
    #[must_use]
    pub fn row_count(&self) -> i64 {
        match self {
            Self::Type211 { dimensions, .. } => dimensions[0],
            Self::Table(table) => table.row_count,
        }
    }

    /// Native column header: stored for type 211, otherwise the shared row width.
    #[must_use]
    pub fn column_count(&self) -> i64 {
        match self {
            Self::Type211 { dimensions, .. } => dimensions[1],
            Self::Table(table) => table.column_count,
        }
    }

    /// Visit the leading and column pairs in each row.
    pub fn visit_rows(&self, mut visit: impl FnMut(&[R; 2], &[[R; 2]], Option<&[R; 2]>)) {
        match self {
            Self::Type211 { row, .. } => visit(row, &[], None),
            Self::Table(table) => {
                for row in &table.rows {
                    visit(&row.parameters, &row.columns, row.extra.as_ref());
                }
            }
        }
    }
}

/// The complete constraint payload of a classic loft or skin profile.
///
/// Every field the classic form carries is required and every field it cannot
/// carry is absent from the type, so the revision-gated `support_bounds` and
/// the type-zero `secondary_pcurve` are unknown keys here rather than values a
/// reader has to refuse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(rename = "LoftProfileData"))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: ::serde::de::DeserializeOwned, V: ::serde::de::DeserializeOwned"))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
pub struct ClassicLoftProfileData<R = f64, V = Vector3> {
    /// Required support surface.
    pub surface: SurfaceId,
    /// Nullable parameter curve on the support.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_pcurve"
    )]
    pub pcurve: Option<PcurveGeometry>,
    /// First native constraint flag.
    pub first_flag: bool,
    /// ASM extension integer preceding the subdata.
    pub asm_extension: i64,
    /// Native constraint table.
    pub subdata: LoftSubdata<R>,
    /// Optional direction selected by the second native flag.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_direction"
    )]
    pub direction: Option<V>,
}

/// Type-selected fields of one loft profile member.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[serde(bound(deserialize = "R: ::serde::de::DeserializeOwned, V: ::serde::de::DeserializeOwned"))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
pub enum LoftMemberForm<R = f64, V = Vector3> {
    /// Support-surface form. Legacy layouts can use type zero; revision-gated
    /// layouts select this form with a nonzero type code.
    Support {
        /// Native member type discriminator.
        type_code: i64,
        /// Constraint support surface, absent for the native `null_surface`
        /// sentinel.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_surface"
        )]
        surface: Option<SurfaceId>,
        /// Optional U/V bound fields following the support surface in the
        /// revision-gated encoding.
        support_bounds: [Option<R>; 4],
        /// UV curve on the support, absent for `nullbs`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_pcurve"
        )]
        pcurve: Option<PcurveGeometry>,
        /// First native constraint flag.
        first_flag: bool,
        /// ASM extension integer when the stream version carries it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_asm_extension"
        )]
        asm_extension: Option<i64>,
        /// Native constraint table.
        subdata: LoftSubdata<R>,
        /// Optional direction selected by the second native flag.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_direction"
        )]
        direction: Option<V>,
    },
    /// Revision-gated type-zero form with two nullable UV curve slots.
    PcurvePair {
        /// First UV curve slot, absent for `nullbs`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_pcurve"
        )]
        pcurve: Option<PcurveGeometry>,
        /// Second UV curve slot, absent for `nullbs`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_secondary_pcurve"
        )]
        secondary_pcurve: Option<PcurveGeometry>,
        /// ASM extension integer when the stream version carries it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_asm_extension"
        )]
        asm_extension: Option<i64>,
        /// Native constraint table.
        subdata: LoftSubdata<R>,
        /// Optional direction selected by the second native flag.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_direction"
        )]
        direction: Option<V>,
    },
}

impl<R, V> LoftMemberForm<R, V> {
    /// Return the native type code selected by this form.
    #[must_use]
    pub fn type_code(&self) -> i64 {
        match self {
            Self::Support { type_code, .. } => *type_code,
            Self::PcurvePair { .. } => 0,
        }
    }

    /// Return the support surface when this is the support form.
    #[must_use]
    pub fn surface(&self) -> Option<&SurfaceId> {
        match self {
            Self::Support { surface, .. } => surface.as_ref(),
            Self::PcurvePair { .. } => None,
        }
    }

    /// Return the constraint subdata.
    #[must_use]
    pub fn subdata(&self) -> &LoftSubdata<R> {
        match self {
            Self::Support { subdata, .. } | Self::PcurvePair { subdata, .. } => subdata,
        }
    }

    /// Return the optional direction selected by the second native flag.
    #[must_use]
    pub fn direction(&self) -> Option<&V> {
        match self {
            Self::Support { direction, .. } | Self::PcurvePair { direction, .. } => {
                direction.as_ref()
            }
        }
    }
}

/// One referenced curve together with its optional native parameter bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: ::serde::de::DeserializeOwned"))]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub struct LoftPathCurve<R = f64> {
    /// Referenced curve carrier.
    #[serde(rename = "curve")]
    pub id: CurveId,
    /// Optional parameter endpoints following the curve in a revision-gated encoding.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_endpoints"
    )]
    pub endpoints: Option<[Option<R>; 2]>,
}

/// One curve member of a loft profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
#[serde(bound(deserialize = "R: ::serde::de::DeserializeOwned, V: ::serde::de::DeserializeOwned"))]
pub struct LoftProfileMember<R = f64, V = Vector3> {
    /// Profile curve and its revision-gated parameter endpoints.
    pub profile: LoftPathCurve<R>,
    /// Structurally selected surface-side constraint form.
    pub form: LoftMemberForm<R, V>,
}

/// Native path data attached to one loft section entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: ::serde::de::DeserializeOwned"))]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub struct LoftPath<R = f64> {
    /// Primary path curve and its optional endpoints, absent for `null_curve`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_path"
    )]
    pub path: Option<LoftPathCurve<R>>,
    /// Ordered auxiliary BS3 curves.
    pub auxiliaries: Vec<CurveId>,
    /// Native path tail integer.
    pub flag: i64,
}

/// One parameterized entry in a native loft section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
#[serde(bound(deserialize = "R: ::serde::de::DeserializeOwned, V: ::serde::de::DeserializeOwned"))]
pub struct LoftSectionEntry<R = f64, V = Vector3> {
    /// Native section parameter.
    pub parameter: R,
    /// Ordered profile members.
    pub profile: Vec<LoftProfileMember<R, V>>,
    /// Native path data.
    pub path: LoftPath<R>,
}

/// Revision-gated `loft_spl_sur` form fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: ::serde::de::DeserializeOwned"))]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub struct LoftRevisionForm<R = f64> {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: PositiveI64,
    /// Four booleans following the parameter intervals.
    pub flags: [bool; 4],
    /// Two integers preceding the shared tail.
    pub ints: [i64; 2],
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: RevisionCacheForm<RevisionSurfaceParameterization<R>>,
    /// Six ordered discontinuity arrays following the fit tolerance.
    pub discontinuities: [Vec<R>; 6],
    /// Boolean terminating the shared tail.
    pub tail_flag: bool,
}

/// Ordered native loft section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
#[serde(bound(deserialize = "R: ::serde::de::DeserializeOwned, V: ::serde::de::DeserializeOwned"))]
pub struct LoftSection<R = f64, V = Vector3> {
    /// Ordered entries in the section.
    pub entries: Vec<LoftSectionEntry<R, V>>,
}

/// Token retained from the variable bridge preceding a loft solved cache.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum LoftBridgeToken<R = f64> {
    /// Native boolean token.
    Boolean(bool),
    /// Native integer token.
    Integer(i64),
    /// Native double token.
    Double(R),
    /// Native string token.
    Text(String),
    /// Native enum token.
    Enum(i64),
}

/// One member of a compound-loft scale block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
#[serde(bound(deserialize = "R: ::serde::de::DeserializeOwned, V: ::serde::de::DeserializeOwned"))]
pub struct CompoundLoftScaleMember<R = f64, V = Vector3> {
    /// Native member integer.
    pub type_code: i64,
    /// Member curve.
    pub curve: CurveId,
    /// Native loft constraint data.
    pub data: ClassicLoftProfileData<R, V>,
}

/// Complete `_readScaleClLoft` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
#[serde(bound(deserialize = "R: ::serde::de::DeserializeOwned, V: ::serde::de::DeserializeOwned"))]
pub struct CompoundLoftScale<R = f64, V = Vector3> {
    /// Ordered scale members.
    pub members: Vec<CompoundLoftScaleMember<R, V>>,
    /// Scale path curve.
    pub path: CurveId,
    /// Ordered BS3 auxiliary curves.
    pub auxiliaries: Vec<CurveId>,
    /// Two native trailing integers.
    pub tail: [i64; 2],
}

/// Direction carrier in the zero-kind compound-loft tail.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompoundLoftDirection<V = Vector3> {
    /// Inline direction vector. This form has no native selector.
    Vector {
        /// Stored direction.
        value: V,
    },
    /// BS3 direction curve and its nonzero native selector.
    Curve {
        /// Stored curve.
        curve: CurveId,
        /// Exact nonzero native selector retained for byte-faithful export.
        selector: NonZeroI64,
    },
}

impl<V> CompoundLoftDirection<V> {
    /// Native selector for this direction form.
    #[must_use]
    pub const fn selector(&self) -> i64 {
        match self {
            Self::Vector { .. } => 0,
            Self::Curve { selector, .. } => selector.get(),
        }
    }
}

/// Structurally selected tail of `cl_loft_spl_sur`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: ::serde::de::DeserializeOwned, V: ::serde::de::DeserializeOwned"))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
pub enum CompoundLoftTail<R = f64, V = Vector3> {
    /// Native kind `6` tail.
    Six {
        /// Two leading flags.
        flags: [bool; 2],
        /// Required scale block.
        scale: Box<CompoundLoftScale<R, V>>,
        /// Native integer following the scale.
        selector: i64,
        /// Stored direction.
        direction: V,
        /// Native parameter interval.
        parameter_range: [R; 2],
        /// BS3 tail curve.
        curve: CurveId,
    },
    /// Native kind `7` tail.
    Seven {
        /// First flag.
        first_flag: bool,
        /// First optional scale block.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_first_scale"
        )]
        first_scale: Option<Box<CompoundLoftScale<R, V>>>,
        /// Second flag.
        second_flag: bool,
        /// Required second scale block.
        second_scale: Box<CompoundLoftScale<R, V>>,
        /// Native selector integer.
        selector: i64,
        /// Stored direction.
        direction: V,
        /// Two trailing flags.
        trailing_flags: [bool; 2],
    },
    /// Native kind `0` tail.
    Zero {
        /// Two leading flags.
        flags: [bool; 2],
        /// Vector or BS3 curve with its derived native selector.
        direction: CompoundLoftDirection<V>,
        /// Two trailing flags.
        trailing_flags: [bool; 2],
    },
}

/// A bounded list of compound-loft scales.
#[derive(Debug, Clone, PartialEq, Deserialize, cadmpeg_derive::StructuralOrd)]
#[serde(
    try_from = "Vec<CompoundLoftScale<R, V>>",
    bound(deserialize = "R: ::serde::de::DeserializeOwned, V: ::serde::de::DeserializeOwned")
)]
pub struct CompoundLoftScales<const CAPACITY: usize, R = f64, V = Vector3>(
    pub(super) Vec<CompoundLoftScale<R, V>>,
);

impl<const CAPACITY: usize, R: Serialize, V: Serialize> Serialize
    for CompoundLoftScales<CAPACITY, R, V>
{
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

#[cfg(feature = "schema")]
impl<const CAPACITY: usize, R, V> JsonSchema for CompoundLoftScales<CAPACITY, R, V>
where
    CompoundLoftScale<R, V>: JsonSchema,
{
    fn schema_name() -> std::borrow::Cow<'static, str> {
        format!("CompoundLoftScales_{CAPACITY}").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut schema = Vec::<CompoundLoftScale<R, V>>::json_schema(generator);
        schema.insert("maxItems".into(), CAPACITY.into());
        schema
    }
}

impl<const CAPACITY: usize, R, V> CompoundLoftScales<CAPACITY, R, V> {
    /// Admit a leading scale list within the native slot capacity.
    pub fn try_new(scales: Vec<CompoundLoftScale<R, V>>) -> Result<Self, &'static str> {
        if scales.len() > CAPACITY {
            return Err("compound loft scales exceed slot capacity");
        }
        Ok(Self(scales))
    }

    /// Admit native optional slots whose present values form a leading prefix.
    pub fn try_from_slots(
        slots: impl IntoIterator<Item = Option<CompoundLoftScale<R, V>>>,
    ) -> Result<Self, &'static str> {
        let mut scales = Vec::new();
        let mut absent = false;
        for (index, slot) in slots.into_iter().enumerate() {
            if index >= CAPACITY {
                return Err("compound loft scales exceed slot capacity");
            }
            match slot {
                Some(scale) if !absent => scales.push(scale),
                Some(_) => return Err("compound loft scales must form a leading prefix"),
                None => absent = true,
            }
        }
        Ok(Self(scales))
    }

    /// Present scales in native order.
    #[must_use]
    pub fn as_slice(&self) -> &[CompoundLoftScale<R, V>] {
        &self.0
    }
}

impl<const CAPACITY: usize, R, V> TryFrom<Vec<CompoundLoftScale<R, V>>>
    for CompoundLoftScales<CAPACITY, R, V>
{
    type Error = &'static str;
    fn try_from(scales: Vec<CompoundLoftScale<R, V>>) -> Result<Self, Self::Error> {
        Self::try_new(scales)
    }
}

/// Complete native compound-loft construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
#[serde(bound(deserialize = "R: ::serde::de::DeserializeOwned, V: ::serde::de::DeserializeOwned"))]
pub struct CompoundLoftConstruction<R = f64, V = Vector3> {
    /// Present scales, up to the five native slots.
    pub scales: CompoundLoftScales<5, R, V>,
    /// Two flags before the tail kind.
    pub flags: [bool; 2],
    /// Kind-specific trailing graph.
    pub tail: CompoundLoftTail<R, V>,
}

/// Initial solved-shape branch of a scaled compound loft.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum ScaledCompoundLoftShape<R = f64> {
    /// A solved NURBS cache follows the singularity enum.
    Full {},
    /// The cache is replaced by two intervals and two scalar arrays.
    None {
        /// Two ordered native intervals.
        parameter_ranges: [[R; 2]; 2],
        /// Two ordered native scalar arrays.
        parameters: [Vec<R>; 2],
    },
}

/// Structurally selected middle branch of a scaled compound loft.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: ::serde::de::DeserializeOwned, V: ::serde::de::DeserializeOwned"))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
pub enum ScaledCompoundLoftBranch<R = f64, V = Vector3> {
    /// Extended branch ending in a direction vector.
    ExtendedVector {
        /// Optional first scale block.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_first_scale"
        )]
        first_scale: Option<Box<CompoundLoftScale<R, V>>>,
        /// Required second scale block.
        second_scale: Box<CompoundLoftScale<R, V>>,
        /// Native selector integer.
        selector: i64,
        /// Stored direction vector.
        direction: V,
    },
    /// Extended branch ending in a singularity and curve.
    ExtendedCurve {
        /// Optional scale block.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_scale"
        )]
        scale: Option<Box<CompoundLoftScale<R, V>>>,
        /// Native branch flag.
        flag: bool,
        /// Native singularity enum.
        singularity: i64,
        /// Stored BS3 curve.
        curve: CurveId,
    },
    /// Direct vector-or-curve branch.
    Direct {
        /// Native branch flag.
        flag: bool,
        /// Vector or BS3 curve with its derived native selector.
        direction: CompoundLoftDirection<V>,
    },
}

/// Complete native scaled compound-loft construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
#[serde(bound(deserialize = "R: ::serde::de::DeserializeOwned, V: ::serde::de::DeserializeOwned"))]
pub struct ScaledCompoundLoftConstruction<R = f64, V = Vector3> {
    /// Native leading singularity enum.
    pub singularity: i64,
    /// Singularity-selected solved-shape payload.
    pub shape: ScaledCompoundLoftShape<R>,
    /// Six ordered discontinuity arrays.
    pub discontinuities: [Vec<R>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
    /// Present leading scales within the three native slots.
    pub scales: CompoundLoftScales<3, R, V>,
    /// Two native flags preceding the selector.
    pub flags: [bool; 2],
    /// Native integer preceding the middle branch.
    pub selector: i64,
    /// Structurally selected middle branch.
    pub branch: ScaledCompoundLoftBranch<R, V>,
    /// Two trailing branch flags.
    pub trailing_flags: [bool; 2],
    /// Native trailing kind integer.
    pub tail_kind: i64,
    /// Two native trailing vectors.
    pub tail_directions: [V; 2],
    /// Native trailing singularity enum.
    pub tail_singularity: i64,
    /// Native trailing BS3 curve.
    pub tail_curve: CurveId,
}

cadmpeg_core::named_optional_field!(deserialize_extra<R>, [R; 2], "extra");
cadmpeg_core::named_optional_field!(deserialize_pcurve, PcurveGeometry, "pcurve");
cadmpeg_core::named_optional_field!(deserialize_direction<V>, V, "direction");
cadmpeg_core::named_optional_field!(deserialize_surface, SurfaceId, "surface");
cadmpeg_core::named_optional_field!(deserialize_asm_extension, i64, "asm_extension");
cadmpeg_core::named_optional_field!(
    deserialize_secondary_pcurve,
    PcurveGeometry,
    "secondary_pcurve"
);
cadmpeg_core::named_optional_field!(deserialize_endpoints<R>, [Option<R>; 2], "endpoints");
cadmpeg_core::named_optional_field!(deserialize_path<R>, LoftPathCurve<R>, "path");

cadmpeg_core::named_optional_field!(
    deserialize_first_scale<R, V>,
    Box<CompoundLoftScale<R, V>>,
    "first_scale"
);
cadmpeg_core::named_optional_field!(
    deserialize_scale<R, V>,
    Box<CompoundLoftScale<R, V>>,
    "scale"
);
