// SPDX-License-Identifier: Apache-2.0
//! Cache-first surface curves, spring curves, and surface-curve families.

use super::intersection_curves::IntcurveSupportContext;
use super::legacy_cache::{
    write_revision_form_tolerance, LegacyCache, RevisionCacheWrite, ToleranceWrite,
};
use super::pcurve::PcurveGeometry;
use super::revision_forms::RevisionCacheForm;
use super::RecordBounds;
use crate::ids::SurfaceId;
use crate::scalar::{FiniteReal, PositiveI64};
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Cache-first shared-context fields absent from the context-first layout.
// A source states raw scalars; a curve construction holds the admitted form,
// whose scalars are `FiniteReal` values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: ::serde::de::DeserializeOwned"))]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub struct CacheFirstCurveForm<R = f64> {
    /// Positive serializer-revision integer selecting the cache-first layout.
    pub revision: PositiveI64,
    /// Approximation-cache form selected by the shared context enum.
    pub cache: RevisionCacheForm<CacheFirstCurveParameterization<R>>,
    /// Optional U/V bound fields following each ordered support surface.
    pub support_bounds: [[Option<R>; 4]; 2],
    /// Optional solved-curve interval endpoints; absent endpoints inherit the
    /// solved NURBS domain.
    pub solved_range: [Option<R>; 2],
    /// Native integer ASM extension following the discontinuity arrays.
    pub extension: i64,
}

/// One support slot in a context-first spring construction.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SpringSupport<R = f64> {
    /// Resolved support surface.
    Surface(SurfaceId),
    /// Native U/V ranges stored in place of `null_surface`.
    Ranges([[R; 2]; 2]),
}

/// First pcurve slot in a context-first spring construction.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SpringPcurve<R = f64> {
    /// Resolved parameter-space curve.
    Pcurve(PcurveGeometry),
    /// Native interval stored in place of `nullbs`.
    Range([R; 2]),
}

/// Mutually exclusive spring construction layouts.
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
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, I: JsonSchema + Serialize")
)]
pub enum SpringLayout<R = f64, I = [f64; 2]> {
    /// Support-first layout with inline null-carrier replacement ranges.
    ContextFirst {
        /// Two ordered support slots.
        supports: [SpringSupport<R>; 2],
        /// First pcurve or its null replacement range.
        first_pcurve: Box<SpringPcurve<R>>,
        /// Nullable second pcurve slot.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_second_pcurve"
        )]
        second_pcurve: Option<PcurveGeometry>,
        /// Native solved-curve parameter interval.
        parameter_range: I,
        /// Three ordered discontinuity arrays.
        discontinuities: [Vec<R>; 3],
        /// Native boolean following the discontinuity arrays.
        discontinuity_flag: bool,
        /// Solved-cache fit contract this layout states itself.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
    /// Cache-first layout, which carries no inline replacement ranges or
    /// context-first discontinuity flag.
    CacheFirst {
        /// Shared support context following the solved cache.
        context: IntcurveSupportContext,
        /// Cache-first serializer fields.
        form: CacheFirstCurveForm<R>,
    },
}

impl<R, I> SpringLayout<R, I> {
    pub(super) fn cache_first(&self) -> Option<&CacheFirstCurveForm<R>> {
        match self {
            Self::CacheFirst { form, .. } => Some(form),
            Self::ContextFirst { .. } => None,
        }
    }

    /// Write the fit tolerance of the cache-first layout's cache form.
    ///
    /// The narrow write route: the borrow of the form stays inside this
    /// method, so the layout lends no interior of the admitted payload that
    /// holds it. The context-first layout owns no cache form.
    pub(super) fn write_revision_fit_tolerance(
        &mut self,
        write: ToleranceWrite,
    ) -> RevisionCacheWrite {
        match self {
            Self::CacheFirst { form, .. } => form.cache.write_fit_tolerance(write),
            Self::ContextFirst { .. } => RevisionCacheWrite::NoForm,
        }
    }
}

/// Parameterization carried by cache form `2` of the shared cache-first
/// intcurve context. This form stores no solved curve cache and no fit
/// tolerance; it stores the curve interval followed by the closed-form enum, in
/// the order the fields appear below.
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, Default, cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: ::serde::de::DeserializeOwned"))]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub struct CacheFirstCurveParameterization<R = f64> {
    /// Curve interval, an ordered `[lo, hi]` pair of optional bounds. `None` is
    /// a false bound-presence flag.
    pub interval: [Option<R>; 2],
    /// Closed-form enum following the interval.
    pub closed_form: i64,
}

/// Family-independent tail fields carried by a cache-first surface curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SurfaceCurveTailWire")]
pub struct SurfaceCurveTail {
    /// Native integer following the discontinuity arrays.
    pub(super) extension: i64,
    /// Positive serializer-revision integer opening the cache-first layout.
    pub(super) revision: PositiveI64,
    /// Approximation-cache form selected by the shared context enum.
    #[cfg_attr(
        feature = "schema",
        schemars(with = "RevisionCacheForm<CacheFirstCurveParameterization>")
    )]
    pub(super) cache: RevisionCacheForm<CacheFirstCurveParameterization<FiniteReal>>,
    /// Optional U/V bound fields following each ordered support surface.
    pub(super) support_bounds: [RecordBounds; 2],
    /// Optional solved-curve interval endpoints; absent endpoints inherit the
    /// solved NURBS domain.
    #[serde(default)]
    pub(super) solved_range: [Option<FiniteReal>; 2],
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub(super) struct SurfaceCurveTailWire {
    /// Native integer following the discontinuity arrays.
    pub(super) extension: i64,
    /// Positive serializer-revision integer opening the cache-first layout.
    pub(super) revision: PositiveI64,
    /// Approximation-cache form selected by the shared context enum.
    pub(super) cache: RevisionCacheForm<CacheFirstCurveParameterization>,
    /// Optional U/V bound fields following each ordered support surface.
    pub(super) support_bounds: [[Option<f64>; 4]; 2],
    /// Optional solved-curve interval endpoints; absent endpoints inherit the
    /// solved NURBS domain.
    pub(super) solved_range: [Option<f64>; 2],
}

impl TryFrom<SurfaceCurveTailWire> for SurfaceCurveTail {
    type Error = &'static str;

    fn try_from(wire: SurfaceCurveTailWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.extension,
            wire.revision,
            wire.cache,
            wire.support_bounds,
            wire.solved_range,
        )
    }
}

impl SurfaceCurveTail {
    /// Admit a cache-first surface-curve tail around an admitted revision.
    /// The support bounds, the solved range and the cache interval are raw
    /// values; a non-finite one is refused.
    pub fn try_new(
        extension: i64,
        revision: PositiveI64,
        cache: RevisionCacheForm<CacheFirstCurveParameterization>,
        support_bounds: [[Option<f64>; 4]; 2],
        solved_range: [Option<f64>; 2],
    ) -> Result<Self, &'static str> {
        const INVALID: &str = "surface curve tail bounds must be finite";
        let [first, second] = support_bounds.map(|bounds| RecordBounds::try_new(bounds).ok());
        let (Some(first), Some(second), Some(solved_range), Some(cache)) = (
            first,
            second,
            FiniteReal::optional(solved_range),
            cache.admit(),
        ) else {
            return Err(INVALID);
        };
        Ok(Self {
            extension,
            revision,
            cache,
            support_bounds: [first, second],
            solved_range,
        })
    }

    /// Native integer following the discontinuity arrays.
    #[must_use]
    pub const fn extension(&self) -> i64 {
        self.extension
    }

    /// Serializer-revision integer opening the cache-first layout.
    #[must_use]
    pub const fn revision(&self) -> PositiveI64 {
        self.revision
    }

    /// Approximation-cache form selected by the shared context enum.
    #[must_use]
    pub const fn cache(&self) -> &RevisionCacheForm<CacheFirstCurveParameterization<FiniteReal>> {
        &self.cache
    }

    /// Optional U/V bound fields following each ordered support surface.
    #[must_use]
    pub const fn support_bounds(&self) -> &[RecordBounds; 2] {
        &self.support_bounds
    }

    /// Optional solved-curve interval endpoints.
    #[must_use]
    pub const fn solved_range(&self) -> &[Option<FiniteReal>; 2] {
        &self.solved_range
    }
}

/// Cache-first surface-curve tail paired with its family-specific flags.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SurfaceCurveCacheFirst<F> {
    /// Family-independent cache-first fields.
    pub form: SurfaceCurveTail,
    /// Flags admitted by the selected surface-curve family.
    pub flags: F,
}

/// Two terminating flags carried only by a parametric surface curve.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ParametricSurfaceCurveFlags {
    /// Support-slot selector.
    pub flag: bool,
    /// Optional later-revision terminating flag.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_second_flag"
    )]
    pub second_flag: Option<bool>,
}

/// Mutually exclusive tail forms of a native projected intcurve.
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
pub enum ProjectionTail<R = f64> {
    /// The ASM flag is followed immediately by the subtype close.
    EarlyClose {
        /// Native ASM projection flag.
        flag: bool,
    },
    /// The ASM flag is followed by a retained source interval and role text.
    Ranged {
        /// Native ASM projection flag.
        flag: bool,
        /// Native parameter interval on the projected source curve.
        parameter_range: [R; 2],
        /// Projection support role.
        role: ProjectionRole,
    },
}

/// Support selected by a ranged projection tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum ProjectionRole {
    /// First support surface.
    #[serde(rename = "surf1")]
    Surf1,
    /// Second support surface.
    #[serde(rename = "surf2")]
    Surf2,
}

impl ProjectionRole {
    /// Parse the native ASM identifier.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "surf1" => Some(Self::Surf1),
            "surf2" => Some(Self::Surf2),
            _ => None,
        }
    }

    /// Native ASM identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Surf1 => "surf1",
            Self::Surf2 => "surf2",
        }
    }
}

impl<'de> Deserialize<'de> for ProjectionRole {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "invalid projection role field value `{value}`; expected `surf1` or `surf2`"
            ))
        })
    }
}

/// Native surface-curve family with its support context and optional
/// cache-first form.
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
pub enum SurfaceCurveFamily {
    /// Blend edge curve whose construction details live on its blend support.
    Blend {
        /// Shared support context.
        context: IntcurveSupportContext,
        /// Cache-first fields, when this is not the prefix-first layout.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_tail"
        )]
        tail: Option<SurfaceCurveCacheFirst<bool>>,
    },
    /// Curve constrained to a support surface.
    SurfaceConstrained {
        /// Shared support context.
        context: IntcurveSupportContext,
        /// Cache-first fields, when this is not the prefix-first layout.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_tail"
        )]
        tail: Option<SurfaceCurveCacheFirst<bool>>,
    },
    /// Parametric curve on a support surface.
    Parametric {
        /// Shared support context.
        context: IntcurveSupportContext,
        /// Cache-first fields with the parametric-only second flag.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_surface_curve_family_tail"
        )]
        tail: Option<SurfaceCurveCacheFirst<ParametricSurfaceCurveFlags>>,
    },
    /// Skin curve on a support surface.
    Skin {
        /// Shared support context.
        context: IntcurveSupportContext,
        /// Cache-first fields, when this is not the prefix-first layout.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_tail"
        )]
        tail: Option<SurfaceCurveCacheFirst<bool>>,
    },
}

/// Discriminant of a native surface-curve family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SurfaceCurveFamilyKind {
    /// Blend edge curve whose construction details live on its blend support.
    Blend,
    /// Curve constrained to a support surface.
    SurfaceConstrained,
    /// Parametric curve on a support surface.
    Parametric,
    /// Skin curve on a support surface.
    Skin,
}

impl SurfaceCurveFamily {
    /// Return the family discriminant.
    #[must_use]
    pub const fn kind(&self) -> SurfaceCurveFamilyKind {
        match self {
            Self::Blend { .. } => SurfaceCurveFamilyKind::Blend,
            Self::SurfaceConstrained { .. } => SurfaceCurveFamilyKind::SurfaceConstrained,
            Self::Parametric { .. } => SurfaceCurveFamilyKind::Parametric,
            Self::Skin { .. } => SurfaceCurveFamilyKind::Skin,
        }
    }

    /// Borrow the shared support context.
    #[must_use]
    pub const fn context(&self) -> &IntcurveSupportContext {
        match self {
            Self::Blend { context, .. }
            | Self::SurfaceConstrained { context, .. }
            | Self::Parametric { context, .. }
            | Self::Skin { context, .. } => context,
        }
    }

    pub(super) fn revision_cache(
        &self,
    ) -> Option<&RevisionCacheForm<CacheFirstCurveParameterization<FiniteReal>>> {
        match self {
            Self::Blend { tail, .. }
            | Self::SurfaceConstrained { tail, .. }
            | Self::Skin { tail, .. } => tail.as_ref().map(|first| &first.form.cache),
            Self::Parametric { tail, .. } => tail.as_ref().map(|first| &first.form.cache),
        }
    }

    /// Write the fit tolerance of the revision-gated solved cache.
    ///
    /// The narrow write route: the borrow of the cache form stays inside this
    /// method, so the family lends no admitted interior for writing.
    pub(super) fn write_revision_fit_tolerance(
        &mut self,
        write: ToleranceWrite,
    ) -> RevisionCacheWrite {
        let form = match self {
            Self::Blend { tail, .. }
            | Self::SurfaceConstrained { tail, .. }
            | Self::Skin { tail, .. } => tail.as_mut().map(|first| &mut first.form.cache),
            Self::Parametric { tail, .. } => tail.as_mut().map(|first| &mut first.form.cache),
        };
        write_revision_form_tolerance(form, write)
    }

    /// Return whether two families have the same discriminant and cache-first
    /// tail, excluding the editable support context.
    #[must_use]
    pub fn has_same_form(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Blend { tail: first, .. }, Self::Blend { tail: second, .. })
            | (
                Self::SurfaceConstrained { tail: first, .. },
                Self::SurfaceConstrained { tail: second, .. },
            )
            | (Self::Skin { tail: first, .. }, Self::Skin { tail: second, .. }) => first == second,
            (Self::Parametric { tail: first, .. }, Self::Parametric { tail: second, .. }) => {
                first == second
            }
            _ => false,
        }
    }
}

/// Native silhouette construction family and its exclusive tail fields.
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
pub enum SilhouetteKind {
    /// Standard implicit silhouette.
    Standard {},
    /// Parametric silhouette.
    Parametric {},
    /// Draft/taper silhouette with an explicit factor.
    Taper {
        /// Native unscaled draft factor.
        draft_factor: crate::scalar::FiniteReal,
    },
}

cadmpeg_core::named_optional_field!(deserialize_cache, LegacyCache, "cache");

cadmpeg_core::named_optional_field!(deserialize_second_flag, bool, "second_flag");
cadmpeg_core::named_optional_field!(deserialize_tail, SurfaceCurveCacheFirst<bool>, "tail");
cadmpeg_core::named_optional_field!(
    deserialize_surface_curve_family_tail,
    SurfaceCurveCacheFirst<ParametricSurfaceCurveFlags>,
    "tail"
);

cadmpeg_core::named_optional_field!(deserialize_second_pcurve, PcurveGeometry, "second_pcurve");
