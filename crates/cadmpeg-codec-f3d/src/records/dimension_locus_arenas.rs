use super::{dimension_null_locus_wire, dimensions::DesignDimensionLocusPair};
use cadmpeg_core::decode::DecodeContext;
use serde::{Deserialize, Serialize};
use std::ops::Deref;

const NONNULL_LOCUS_ERROR: &str = "design_dimension_locus_pairs requires two nonnull geometry loci and opaque_index";

/// An arena containing only pairs of nonnull geometry loci with an opaque index.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "Vec<DesignDimensionLocusPair>",
    into = "Vec<DesignDimensionLocusPair>"
)]
pub(crate) struct DesignDimensionLocusPairs(Vec<DesignDimensionLocusPair>);

impl TryFrom<Vec<DesignDimensionLocusPair>> for DesignDimensionLocusPairs {
    type Error = String;

    fn try_from(pairs: Vec<DesignDimensionLocusPair>) -> Result<Self, Self::Error> {
        if pairs.iter().any(|pair| pair.opaque_index().is_none()) {
            return Err(NONNULL_LOCUS_ERROR.into());
        }
        Ok(Self(pairs))
    }
}

impl DesignDimensionLocusPairs {
    /// Validate the reached pairs under the caller's admission policy.
    pub(crate) fn from_pairs_charged(
        ctx: &DecodeContext<'_>,
        pairs: Vec<DesignDimensionLocusPair>,
    ) -> Result<Self, cadmpeg_ir::NativeConvertError> {
        const OPERATION: &str = "validate F3D nonnull locus pairs";
        ctx.charge_work(0, OPERATION)?;
        if !pairs.is_empty()
            && ctx.any_by(&pairs, |pair| Ok(pair.opaque_index().is_none()), OPERATION)?
        {
            let message = ctx.copy_retained_text(NONNULL_LOCUS_ERROR, OPERATION)?;
            return Err(cadmpeg_ir::NativeConvertError::ConversionMessage(message));
        }
        Ok(Self(pairs))
    }
}

impl From<DesignDimensionLocusPairs> for Vec<DesignDimensionLocusPair> {
    fn from(pairs: DesignDimensionLocusPairs) -> Self {
        pairs.0
    }
}

impl Deref for DesignDimensionLocusPairs {
    type Target = [DesignDimensionLocusPair];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> IntoIterator for &'a DesignDimensionLocusPairs {
    type Item = &'a DesignDimensionLocusPair;
    type IntoIter = std::slice::Iter<'a, DesignDimensionLocusPair>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// An arena containing only null-first geometry pairs without an opaque index.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    from = "Vec<dimension_null_locus_wire::Entry>",
    into = "Vec<dimension_null_locus_wire::Wire>"
)]
pub(crate) struct DesignDimensionNullLocusPairs(Vec<DesignDimensionLocusPair>);

impl TryFrom<Vec<DesignDimensionLocusPair>> for DesignDimensionNullLocusPairs {
    type Error = String;

    fn try_from(pairs: Vec<DesignDimensionLocusPair>) -> Result<Self, Self::Error> {
        if pairs.iter().any(|pair| pair.opaque_index().is_some()) {
            return Err("design_dimension_null_locus_pairs requires a null first locus, a nonnull second locus, and no opaque_index".into());
        }
        Ok(Self(pairs))
    }
}

impl From<Vec<dimension_null_locus_wire::Entry>> for DesignDimensionNullLocusPairs {
    fn from(entries: Vec<dimension_null_locus_wire::Entry>) -> Self {
        Self(entries.into_iter().map(dimension_null_locus_wire::Entry::into_pair).collect())
    }
}

impl DesignDimensionNullLocusPairs {
    pub(crate) fn from_entries_charged(
        ctx: &DecodeContext<'_>,
        entries: Vec<dimension_null_locus_wire::Entry>,
    ) -> Result<Self, cadmpeg_ir::NativeConvertError> {
        let pairs = ctx.collect_vec(
            entries.into_iter().map(dimension_null_locus_wire::Entry::into_pair),
            "load F3D null locus pairs",
        )?;
        Ok(Self(pairs))
    }
}

impl From<DesignDimensionNullLocusPairs> for Vec<dimension_null_locus_wire::Wire> {
    fn from(pairs: DesignDimensionNullLocusPairs) -> Self {
        pairs
            .0
            .iter()
            .map(dimension_null_locus_wire::Wire::from)
            .collect()
    }
}

impl Deref for DesignDimensionNullLocusPairs {
    type Target = [DesignDimensionLocusPair];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> IntoIterator for &'a DesignDimensionNullLocusPairs {
    type Item = &'a DesignDimensionLocusPair;
    type IntoIter = std::slice::Iter<'a, DesignDimensionLocusPair>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests;
