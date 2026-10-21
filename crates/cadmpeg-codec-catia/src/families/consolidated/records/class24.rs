// SPDX-License-Identifier: Apache-2.0
//! Class-24 parameter domains, stored sites, and accuracy closure.

use cadmpeg_core::decode::View;
use cadmpeg_ir::scalar::{FiniteReal, NonZeroReal};
use cadmpeg_ir::topology::IncreasingParameterInterval;
use serde::{Deserialize, Serialize};

use crate::wire::bytes::compact_int;

use super::parameter_relation::{endpoints_agree, TargetSites};

/// An observed selector, including the distinct aliases 1 and 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub(crate) struct Class24Selector(u16);

impl TryFrom<u16> for Class24Selector {
    type Error = &'static str;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0x0587 | 0x0586 | 0x0585 | 0x0584 | 0x0987 | 0x0985 | 0x0984 | 0x0d87 | 0x0d86
            | 0x0d85 | 0x0d84 | 1 | 3 => Ok(Self(value)),
            _ => Err("unobserved class-24 selector"),
        }
    }
}

impl From<Class24Selector> for u16 {
    fn from(selector: Class24Selector) -> Self {
        selector.0
    }
}

/// Accuracy with either a repeated absolute word or an elided zero word.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "closure", rename_all = "snake_case")]
pub(crate) enum Class24Accuracy {
    /// The signed word; its absolute value is repeated in the frame.
    Repeated { signed: FiniteReal },
    /// Zero with its sign retained; no final word is stored.
    Elided { negative_zero: bool },
}

/// The source interval and mapping actually stored in the frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Class24SourceRelation {
    Absent,
    Identity {
        range: IncreasingParameterInterval,
    },
    Affine {
        range: IncreasingParameterInterval,
        scale: NonZeroReal,
        offset: FiniteReal,
    },
}

/// A complete scalar class-24 parameter relation; operand identities remain local.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Class24DomainRelation {
    pub(crate) source_operand: u32,
    pub(crate) target_operand: u32,
    pub(crate) selector: Class24Selector,
    pub(crate) target_range: IncreasingParameterInterval,
    pub(crate) accuracy: Class24Accuracy,
    pub(crate) source: Class24SourceRelation,
    pub(crate) sites: TargetSites,
}

/// Admit only a complete observed selector production and its numeric invariants.
pub(crate) fn parse(payload: &[u8]) -> Option<Class24DomainRelation> {
    if payload.first() != Some(&0x82) {
        return None;
    }
    let mut at = 1;
    let source_operand = compact_int(payload, &mut at)?;
    let target_operand = compact_int(payload, &mut at)?;
    let lead = *payload.get(at)?;
    if !matches!(lead, 0x0a | 0x0b) {
        return None;
    }
    let selector = Class24Selector::try_from(View::u16_le_at(payload, at + 1)?).ok()?;
    let mut view = View::over_retained(payload.get(at + 3..)?);
    let target_range = IncreasingParameterInterval::new([view.f64_le()?, view.f64_le()?])?;
    let signed_accuracy = FiniteReal::new(view.f64_le()?)?;
    let source = match selector.0 {
        0x0587 | 0x0586 | 0x0585 | 0x0584 | 1 => Class24SourceRelation::Absent,
        0x0987 | 0x0985 | 0x0984 => {
            let range = IncreasingParameterInterval::new([view.f64_le()?, view.f64_le()?])?;
            if !range
                .endpoints()
                .into_iter()
                .zip(target_range.endpoints())
                .all(|(source, target)| endpoints_agree(source, target))
            {
                return None;
            }
            Class24SourceRelation::Identity { range }
        }
        0x0d87 | 0x0d86 | 0x0d85 | 0x0d84 | 3 => {
            let range = IncreasingParameterInterval::new([view.f64_le()?, view.f64_le()?])?;
            let scale = NonZeroReal::new(view.f64_le()?)?;
            let offset = FiniteReal::new(view.f64_le()?)?;
            let mut source_endpoints = range.endpoints();
            if scale.get() < 0.0 {
                source_endpoints.reverse();
            }
            if !source_endpoints
                .into_iter()
                .zip(target_range.endpoints())
                .all(|(source, target)| {
                    endpoints_agree(scale.get() * source + offset.get(), target)
                })
            {
                return None;
            }
            Class24SourceRelation::Affine {
                range,
                scale,
                offset,
            }
        }
        _ => return None,
    };
    let read_site = |view: &mut View<'_>| -> Option<FiniteReal> {
        let parameter = FiniteReal::new(view.f64_le()?)?;
        let [lower, upper] = target_range.endpoints();
        ((parameter.get() >= lower || endpoints_agree(parameter.get(), lower))
            && (parameter.get() <= upper || endpoints_agree(parameter.get(), upper)))
        .then_some(parameter)
    };
    let sites = match selector.0 {
        0x0587 | 0x0987 | 0x0d87 => TargetSites::Absent,
        0x0586 | 0x0d86 => TargetSites::Lower {
            parameter: read_site(&mut view)?,
        },
        0x0585 | 0x0985 | 0x0d85 => TargetSites::Upper {
            parameter: read_site(&mut view)?,
        },
        0x0584 | 0x0984 | 0x0d84 | 1 | 3 => {
            let lower = read_site(&mut view)?;
            let upper = read_site(&mut view)?;
            if lower.get() > upper.get() {
                return None;
            }
            TargetSites::Both { lower, upper }
        }
        _ => return None,
    };
    let accuracy = if lead == 0x0a {
        let absolute = view.f64_le()?;
        if absolute.to_bits() != signed_accuracy.get().abs().to_bits() {
            return None;
        }
        Class24Accuracy::Repeated {
            signed: signed_accuracy,
        }
    } else {
        if signed_accuracy.get() != 0.0 {
            return None;
        }
        Class24Accuracy::Elided {
            negative_zero: signed_accuracy.get().is_sign_negative(),
        }
    };
    view.is_empty().then_some(Class24DomainRelation {
        source_operand,
        target_operand,
        selector,
        target_range,
        accuracy,
        source,
        sites,
    })
}

#[cfg(test)]
mod tests;
