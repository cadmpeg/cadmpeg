// SPDX-License-Identifier: Apache-2.0
//! T-spline subtransforms and surface constructions.

use super::legacy_cache::{CacheContract, LegacyCache, LegacyCacheSlot, ProceduralGeometryError};
use super::revision_forms::RevisionSurfaceForm;
use crate::scalar::FiniteReal;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A non-negative native subtype-table index.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "i64", into = "i64")]
pub struct SubtypeTableIndex(i64);

impl SubtypeTableIndex {
    /// Admit a non-negative native subtype-table index.
    pub fn try_new(index: i64) -> Result<Self, &'static str> {
        if index >= 0 {
            Ok(Self(index))
        } else {
            Err("subtype table index must be non-negative")
        }
    }

    /// Native subtype-table index.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl TryFrom<i64> for SubtypeTableIndex {
    type Error = &'static str;
    fn try_from(index: i64) -> Result<Self, Self::Error> {
        Self::try_new(index)
    }
}

impl From<SubtypeTableIndex> for i64 {
    fn from(index: SubtypeTableIndex) -> Self {
        index.get()
    }
}

/// An inline T-spline program and its non-empty companion values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    rename = "inline",
    try_from = "InlineTSplineSubtransformWire"
)]
pub struct InlineTSplineSubtransform {
    /// Line-oriented topology and geometry program.
    pub program: cadmpeg_core::text::NonBlankString,
    /// Optional native separator boolean.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub separator: Option<bool>,
    /// Companion values program.
    pub values: cadmpeg_core::text::NonBlankString,
}

#[derive(cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub(super) enum InlineTSplineSubtransformWire {
    /// Inline line-oriented T-spline program and companion values.
    Inline {
        /// Line-oriented topology and geometry program.
        program: String,
        /// Optional native separator boolean.
        #[serde(
            default,
            deserialize_with = "deserialize_inline_t_spline_subtransform_wire_separator"
        )]
        separator: Option<bool>,
        /// Companion values program.
        values: String,
    },
}

impl TryFrom<InlineTSplineSubtransformWire> for InlineTSplineSubtransform {
    type Error = &'static str;
    fn try_from(wire: InlineTSplineSubtransformWire) -> Result<Self, Self::Error> {
        let InlineTSplineSubtransformWire::Inline {
            program,
            separator,
            values,
        } = wire;
        Self::try_new(program, separator, values)
    }
}

impl InlineTSplineSubtransform {
    /// Admit a non-empty T-spline program and companion values.
    pub fn try_new(
        program: impl Into<String>,
        separator: Option<bool>,
        values: impl Into<String>,
    ) -> Result<Self, &'static str> {
        Ok(Self {
            program: cadmpeg_core::text::NonBlankString::new(program)
                .ok_or("T-spline program must not be empty")?,
            separator,
            values: cadmpeg_core::text::NonBlankString::new(values)
                .ok_or("T-spline values must not be empty")?,
        })
    }
}

/// Native T-spline subtransform storage form.
#[derive(Debug, Clone, PartialEq, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "TSplineSubtransformWire")]
pub enum TSplineSubtransform {
    /// Inline line-oriented T-spline program and companion values.
    Inline(InlineTSplineSubtransform),
    /// Resolved reference to an earlier subtype-table entry.
    Resolved {
        /// Native subtype-table index.
        index: SubtypeTableIndex,
        /// Resolved shared program.
        transform: Box<InlineTSplineSubtransform>,
    },
}

#[derive(cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub(super) enum TSplineSubtransformWire {
    /// Inline line-oriented T-spline program and companion values.
    Inline {
        /// Line-oriented topology and geometry program.
        program: String,
        /// Optional native separator boolean.
        #[serde(default, deserialize_with = "deserialize_separator")]
        separator: Option<bool>,
        /// Companion values program.
        values: String,
    },
    /// Resolved reference to an earlier subtype-table entry.
    Reference {
        /// Native subtype-table index.
        index: SubtypeTableIndex,
        /// Resolved shared program.
        resolved: Box<InlineTSplineSubtransform>,
    },
}

impl TryFrom<TSplineSubtransformWire> for TSplineSubtransform {
    type Error = &'static str;
    fn try_from(wire: TSplineSubtransformWire) -> Result<Self, Self::Error> {
        match wire {
            TSplineSubtransformWire::Inline {
                program,
                separator,
                values,
            } => InlineTSplineSubtransform::try_new(program, separator, values).map(Self::Inline),
            TSplineSubtransformWire::Reference { index, resolved } => Ok(Self::Resolved {
                index,
                transform: resolved,
            }),
        }
    }
}

impl Serialize for TSplineSubtransform {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum Wire<'a> {
            Reference {
                index: SubtypeTableIndex,
                resolved: &'a InlineTSplineSubtransform,
            },
        }
        match self {
            Self::Inline(inline) => inline.serialize(serializer),
            Self::Resolved { index, transform } => Wire::Reference {
                index: *index,
                resolved: transform,
            }
            .serialize(serializer),
        }
    }
}

impl TSplineSubtransform {
    /// Effective inline program, including resolved references.
    #[must_use]
    pub fn inline(&self) -> &InlineTSplineSubtransform {
        match self {
            Self::Inline(inline) => inline,
            Self::Resolved { transform, .. } => transform,
        }
    }
}

/// Complete native `t_spl_sur` wrapper.
#[derive(Debug, Clone, PartialEq, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    try_from = "TSplineSurfaceConstructionWire",
    into = "TSplineSurfaceConstructionWire"
)]
pub struct TSplineSurfaceConstruction {
    /// Ordered U and V native parameter intervals.
    pub(super) parameter_ranges: [crate::topology::ParameterInterval; 2],
    /// Native T-spline type integer.
    pub(super) type_code: i64,
    /// Inline or referenced shared subtransform object.
    pub(super) subtransform: TSplineSubtransform,
    /// Native trailing integer.
    pub(super) trailing_value: i64,
    /// Six ordered solved-surface discontinuity arrays.
    pub(super) discontinuities: [Vec<FiniteReal>; 6],
    /// Native discontinuity tail flag.
    pub(super) discontinuity_flag: bool,
    /// Cache contract: the revision-gated form, or the legacy solved-cache
    /// tolerance this construction states instead. The revision layout stores
    /// the shared tail first, then four optional parameter values
    /// (`support_bounds`), the type code as an enum, the nested subtransform
    /// scope, and the trailing integer.
    pub(super) cache: CacheContract<RevisionSurfaceForm<Vec<bool>, FiniteReal>>,
}

impl TSplineSurfaceConstruction {
    /// Admit finite ordered ranges, finite discontinuities, a valid revision
    /// cache, and a resolved subtransform.
    pub fn try_new(
        parameter_ranges: [[f64; 2]; 2],
        type_code: i64,
        subtransform: TSplineSubtransform,
        trailing_value: i64,
        discontinuities: [Vec<f64>; 6],
        discontinuity_flag: bool,
        cache: CacheContract<RevisionSurfaceForm>,
    ) -> Result<Self, ProceduralGeometryError> {
        let parameter_ranges = [
            crate::topology::ParameterInterval::new(parameter_ranges[0])
                .map_err(ProceduralGeometryError::Payload)?,
            crate::topology::ParameterInterval::new(parameter_ranges[1])
                .map_err(ProceduralGeometryError::Payload)?,
        ];
        let discontinuities = FiniteReal::lanes(discontinuities).ok_or(
            ProceduralGeometryError::Payload("T-spline discontinuities must be finite"),
        )?;
        let cache = cache.admit_form(RevisionSurfaceForm::admit).ok_or(
            ProceduralGeometryError::Payload("T-spline revision cache form is invalid"),
        )?;
        Ok(Self {
            parameter_ranges,
            type_code,
            subtransform,
            trailing_value,
            discontinuities,
            discontinuity_flag,
            cache,
        })
    }

    /// Return ordered U and V intervals.
    pub fn parameter_ranges(&self) -> [crate::topology::ParameterInterval; 2] {
        self.parameter_ranges
    }

    /// Return the native type code value.
    pub const fn type_code(&self) -> i64 {
        self.type_code
    }

    /// Return the native subtransform value.
    pub const fn subtransform(&self) -> &TSplineSubtransform {
        &self.subtransform
    }

    /// Return the native trailing integer.
    pub const fn trailing_value(&self) -> i64 {
        self.trailing_value
    }

    /// Return the native discontinuities value.
    pub const fn discontinuities(&self) -> &[Vec<FiniteReal>; 6] {
        &self.discontinuities
    }

    /// Return the native discontinuity flag value.
    pub const fn discontinuity_flag(&self) -> bool {
        self.discontinuity_flag
    }

    /// Return the native revision form value.
    pub const fn revision_form(&self) -> Option<&RevisionSurfaceForm<Vec<bool>, FiniteReal>> {
        self.cache.form()
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(rename = "TSplineSurfaceConstruction"))]
#[serde(deny_unknown_fields)]
pub(super) struct TSplineSurfaceConstructionWire {
    /// Ordered U and V native parameter intervals.
    pub(super) parameter_ranges: [[f64; 2]; 2],
    /// Native T-spline type integer.
    pub(super) type_code: i64,
    /// Inline or referenced shared subtransform object.
    pub(super) subtransform: TSplineSubtransform,
    /// Native trailing integer.
    pub(super) trailing_value: i64,
    /// Six ordered solved-surface discontinuity arrays.
    pub(super) discontinuities: [Vec<f64>; 6],
    /// Native discontinuity tail flag.
    pub(super) discontinuity_flag: bool,
    /// Cache contract: the revision-gated form, or the legacy solved-cache
    /// tolerance this construction states instead. The revision layout stores
    /// the shared tail first, then four optional parameter values
    /// (`support_bounds`), the type code as an enum, the nested subtransform
    /// scope, and the trailing integer.
    pub(super) cache: CacheContract<RevisionSurfaceForm>,
}

impl TryFrom<TSplineSurfaceConstructionWire> for TSplineSurfaceConstruction {
    type Error = ProceduralGeometryError;

    fn try_from(wire: TSplineSurfaceConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.parameter_ranges,
            wire.type_code,
            wire.subtransform,
            wire.trailing_value,
            wire.discontinuities,
            wire.discontinuity_flag,
            wire.cache,
        )
    }
}

impl TSplineSurfaceConstruction {
    /// Solved-cache fit contract this construction states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        match self.cache.legacy_fit_tolerance() {
            Some(fit_tolerance) => Some(LegacyCache { fit_tolerance }),
            None => None,
        }
    }

    /// Mutable legacy solved-cache slot, absent when a revision-gated form
    /// states the tolerance instead.
    pub(super) fn legacy_cache_slot_mut(&mut self) -> Option<LegacyCacheSlot<'_>> {
        self.cache.legacy_cache_mut().map(LegacyCacheSlot::Optional)
    }
}

cadmpeg_core::named_optional_field!(deserialize_separator, bool, "separator");
cadmpeg_core::named_optional_field!(
    deserialize_inline_t_spline_subtransform_wire_separator,
    bool,
    "separator"
);
