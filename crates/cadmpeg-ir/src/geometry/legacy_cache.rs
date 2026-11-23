// SPDX-License-Identifier: Apache-2.0
//! Legacy cache slots, cache contracts, and fit tolerances.

use super::revision_forms::RevisionCacheForm;
use crate::scalar::NonNegativeReal;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// What a write to a revision-gated cache form found.
pub(super) enum RevisionCacheWrite {
    /// The form states a solved cache, whose tolerance the write states.
    Written,
    /// The form states a parameterization, which has no solved cache.
    Parameterized,
    /// The construction owns no revision-gated cache form.
    NoForm,
    /// Scaling the held tolerance overflowed.
    InvalidValue(f64),
}

impl RevisionCacheWrite {
    /// The outcome the fit-tolerance setter states for this write.
    pub(super) const fn into_set_result(self) -> Result<(), CacheContractError> {
        match self {
            Self::Written => Ok(()),
            Self::Parameterized => Err(CacheContractError::Parameterized),
            Self::NoForm => Err(CacheContractError::Layout(NO_LEGACY_SLOT)),
            Self::InvalidValue(value) => Err(CacheContractError::InvalidValue { value }),
        }
    }
}

/// Whether a tolerance write states the value or only raises to it.
#[derive(Clone, Copy)]
pub(super) enum ToleranceWrite {
    /// State the value in place of the tolerance the form carries.
    State(FitTolerance),
    /// Keep the tolerance the form carries unless the value exceeds it.
    Raise(FitTolerance),
    /// Scale the tolerance already carried by a solved cache.
    Scale(crate::scalar::PositiveReal),
}

/// State the fit tolerance of an optional solved cache form.
pub(super) fn write_revision_form_tolerance<P>(
    form: Option<&mut RevisionCacheForm<P>>,
    write: ToleranceWrite,
) -> RevisionCacheWrite {
    form.map_or(RevisionCacheWrite::NoForm, |form| {
        form.write_fit_tolerance(write)
    })
}

/// The outcome of clearing the fit tolerance of a revision-gated cache form: a
/// solved cache cannot lose the tolerance it states, and a parameterized form
/// states none to lose.
pub(super) const fn clear_revision_form_tolerance<P>(
    form: Option<&RevisionCacheForm<P>>,
) -> Result<(), CacheContractError> {
    match form {
        Some(RevisionCacheForm::SolvedCache { .. }) => Err(CacheContractError::MissingSolved),
        Some(RevisionCacheForm::Parameterization(_)) | None => Ok(()),
    }
}

/// A finite, non-negative fit tolerance.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "f64", into = "f64")]
pub struct FitTolerance(f64);

impl FitTolerance {
    /// A zero fit tolerance.
    pub const ZERO: Self = Self(0.0);

    /// Admit a finite, non-negative fit tolerance.
    pub fn try_new(value: f64) -> Result<Self, CacheContractError> {
        if value.is_finite() && value >= 0.0 {
            Ok(Self(value))
        } else {
            Err(CacheContractError::InvalidValue { value })
        }
    }

    /// The fit tolerance in carrier units.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }

    /// The tolerance times `scale`.
    ///
    /// A positive scale keeps the sign of a non-negative tolerance: zero
    /// stays zero, and rounding does not change a sign. The product is
    /// refused only when it overflows.
    #[must_use]
    pub fn scaled(self, scale: crate::scalar::PositiveReal) -> Option<Self> {
        let value = self.0 * scale.get();
        value.is_finite().then_some(Self(value))
    }
}

impl TryFrom<f64> for FitTolerance {
    type Error = CacheContractError;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl From<FitTolerance> for f64 {
    fn from(value: FitTolerance) -> Self {
        value.get()
    }
}

/// A fit tolerance admits exactly the finite, non-negative values.
impl From<NonNegativeReal> for FitTolerance {
    fn from(value: NonNegativeReal) -> Self {
        Self(value.get())
    }
}

/// Fit contract of a solved cache that the construction states itself, used by
/// constructions with no revision-gated [`RevisionCacheForm`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LegacyCache {
    /// Fit tolerance of the solved cache.
    pub fit_tolerance: FitTolerance,
}

impl LegacyCache {
    /// A legacy cache with this fit tolerance.
    #[must_use]
    pub const fn new(fit_tolerance: FitTolerance) -> Self {
        Self { fit_tolerance }
    }

    /// A legacy cache stating an admissible fit tolerance.
    pub fn try_new(fit_tolerance: f64) -> Result<Self, CacheContractError> {
        FitTolerance::try_new(fit_tolerance).map(Self::new)
    }
}

/// The cache contract of a construction that has both a legacy and a
/// revision-gated layout: the tolerance is stated in exactly one of them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CacheContract<F> {
    /// Legacy layout, which states its own solved-cache tolerance.
    Legacy {
        /// Solved-cache fit contract, absent when the record stated none.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
    /// Revision-gated layout, which states the tolerance in its cache form.
    Revision {
        /// Revision-gated form.
        form: F,
    },
}

impl<F> CacheContract<F> {
    /// A legacy layout with no solved-cache tolerance.
    #[must_use]
    pub const fn legacy() -> Self {
        Self::Legacy { cache: None }
    }

    /// The contract an optional revision-gated form states.
    #[must_use]
    pub fn from_form(form: Option<F>) -> Self {
        match form {
            Some(form) => Self::Revision { form },
            None => Self::legacy(),
        }
    }

    /// The revision-gated form, absent in the legacy layout.
    #[must_use]
    pub const fn form(&self) -> Option<&F> {
        match self {
            Self::Revision { form } => Some(form),
            Self::Legacy { .. } => None,
        }
    }

    /// Mutable revision-gated form, absent in the legacy layout.
    pub const fn form_mut(&mut self) -> Option<&mut F> {
        match self {
            Self::Revision { form } => Some(form),
            Self::Legacy { .. } => None,
        }
    }

    /// The legacy solved-cache tolerance, absent in the revision layout.
    #[must_use]
    pub const fn legacy_fit_tolerance(&self) -> Option<FitTolerance> {
        match self {
            Self::Legacy { cache: Some(cache) } => Some(cache.fit_tolerance),
            Self::Legacy { cache: None } | Self::Revision { .. } => None,
        }
    }

    /// Mutable legacy solved-cache slot, absent in the revision layout.
    pub(super) const fn legacy_cache_mut(&mut self) -> Option<&mut Option<LegacyCache>> {
        match self {
            Self::Legacy { cache } => Some(cache),
            Self::Revision { .. } => None,
        }
    }
}

impl<F> CacheContract<F> {
    /// Whether this is the legacy layout with no stated tolerance, which is
    /// what a record that states neither carries.
    #[must_use]
    pub const fn is_bare_legacy(&self) -> bool {
        matches!(self, Self::Legacy { cache: None })
    }
}

impl<F> Default for CacheContract<F> {
    fn default() -> Self {
        Self::legacy()
    }
}

/// A fit tolerance, or the cache contract that states it, is not admissible.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CacheContractError {
    /// A fit tolerance is negative or non-finite.
    #[error("fit_tolerance must be finite and non-negative, got {value}")]
    InvalidValue {
        /// Rejected tolerance.
        value: f64,
    },
    /// A parameterized cache form states no solved-cache tolerance.
    #[error("a parameterized cache form states no fit tolerance")]
    Parameterized,
    /// A solved cache form cannot lose its required tolerance.
    #[error("a solved cache form states a fit tolerance")]
    MissingSolved,
    /// The construction's layout states no solved-cache tolerance.
    #[error("{0}")]
    Layout(&'static str),
}

impl From<CacheContractError> for cadmpeg_core::CodecError {
    fn from(error: CacheContractError) -> Self {
        Self::Malformed(error.to_string())
    }
}

/// A rejected procedural definition or cache contract.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ProceduralGeometryError {
    /// A local construction field violates its numeric contract.
    #[error("{0}")]
    Payload(&'static str),
    /// The cache contract is invalid for the definition.
    #[error(transparent)]
    Cache(#[from] CacheContractError),
    /// A member list violates its member contract.
    #[error(transparent)]
    Members(#[from] crate::features::selections::BodySelectionError),
}

/// A construction whose layout states no solved-cache fit tolerance.
pub(super) const NO_LEGACY_SLOT: &str = "this construction states no solved-cache fit tolerance";

pub(super) const PARAMETERIZED_NO_SOLVED_CACHE: &str =
    "a parameterized cache form takes no solved-cache fit tolerance";

/// A construction whose layout always states one.
pub(super) const REQUIRED_LEGACY_SLOT: &str =
    "this construction states a solved-cache fit tolerance";

/// The legacy solved-cache slot of one surface construction, borrowed for
/// writing.
///
/// A layout that may omit the cache lends `Optional`; a layout that always
/// states one lends `Required`, which is why losing the cache is the one
/// refusal a slot makes. Surface-only: `ProceduralCurveDefinition` has no
/// `Required` slot, so a curve lends its slot as a plain
/// `&mut Option<LegacyCache>`.
pub(super) enum LegacyCacheSlot<'a> {
    /// Slot of a layout whose record may state no cache.
    Optional(&'a mut Option<LegacyCache>),
    /// Slot of a layout that always states a cache.
    Required(&'a mut LegacyCache),
}

impl LegacyCacheSlot<'_> {
    /// Replace the cache the slot holds.
    pub(super) const fn set(
        &mut self,
        cache: Option<LegacyCache>,
    ) -> Result<(), CacheContractError> {
        match (self, cache) {
            (Self::Optional(slot), cache) => {
                **slot = cache;
                Ok(())
            }
            (Self::Required(slot), Some(cache)) => {
                **slot = cache;
                Ok(())
            }
            (Self::Required(_), None) => Err(CacheContractError::Layout(REQUIRED_LEGACY_SLOT)),
        }
    }

    /// Scale the tolerance held by this slot, if the optional cache is present.
    pub(super) fn scale_fit_tolerance(
        &mut self,
        scale: crate::scalar::PositiveReal,
    ) -> Result<(), CacheContractError> {
        let cache = match self {
            Self::Optional(slot) => slot.as_mut(),
            Self::Required(cache) => Some(&mut **cache),
        };
        if let Some(cache) = cache {
            let value = cache.fit_tolerance;
            cache.fit_tolerance =
                value
                    .scaled(scale)
                    .ok_or_else(|| CacheContractError::InvalidValue {
                        value: value.get() * scale.get(),
                    })?;
        }
        Ok(())
    }
}

cadmpeg_core::named_optional_field!(deserialize_cache, LegacyCache, "cache");
