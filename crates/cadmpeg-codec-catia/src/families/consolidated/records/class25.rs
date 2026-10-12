// SPDX-License-Identifier: Apache-2.0
//! B-family class-25 parameter maps between source-local curve representations.

use cadmpeg_core::decode::View;
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal, NonZeroReal};
use cadmpeg_ir::topology::IncreasingParameterInterval;
use serde::{Deserialize, Serialize};

use crate::wire::bytes::{allocation_reference, AllocationReferenceEncoding};

use super::parameter_relation::{endpoints_agree, TargetSites};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub(crate) struct Class25Selector(u16);

impl TryFrom<u16> for Class25Selector {
    type Error = &'static str;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0x0d87 | 0x0d86 | 0x0d85 | 0x0d84 | 0x0585 | 0x1587 | 0x1586 | 0x1585 | 0x1584 => {
                Ok(Self(value))
            }
            _ => Err("unobserved class-25 selector"),
        }
    }
}

impl From<Class25Selector> for u16 {
    fn from(value: Class25Selector) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Class25Operand {
    pub(crate) value: u32,
    pub(crate) encoding: AllocationReferenceEncoding,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "closure", rename_all = "snake_case")]
pub(crate) enum Class25Accuracy {
    Fitted {
        signed: FiniteReal,
        fit_tolerance: NonNegativeReal,
    },
    Elided {
        negative_zero: bool,
    },
}

// An 8-bit B-family payload length leaves room for at most twelve pairs after
// the operands, selector, five leading words, count marker, and fit tolerance.
const MAX_SAMPLE_PAIRS: usize = 12;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "SamplePairsWire")]
pub(crate) struct Class25SamplePairs {
    target: [FiniteReal; MAX_SAMPLE_PAIRS],
    source: [FiniteReal; MAX_SAMPLE_PAIRS],
    len: usize,
}

#[derive(Deserialize)]
struct SamplePairsWire {
    target: Vec<FiniteReal>,
    source: Vec<FiniteReal>,
}

impl Class25SamplePairs {
    fn valid(&self) -> bool {
        (2..=MAX_SAMPLE_PAIRS).contains(&self.len)
            && self.target[..self.len].windows(2).all(|p| p[0] < p[1])
            && (self.source[..self.len].windows(2).all(|p| p[0] < p[1])
                || self.source[..self.len].windows(2).all(|p| p[0] > p[1]))
    }
}

impl Serialize for Class25SamplePairs {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct Wire<'a> {
            target: &'a [FiniteReal],
            source: &'a [FiniteReal],
        }
        Wire {
            target: &self.target[..self.len],
            source: &self.source[..self.len],
        }
        .serialize(serializer)
    }
}

impl TryFrom<SamplePairsWire> for Class25SamplePairs {
    type Error = &'static str;

    fn try_from(wire: SamplePairsWire) -> Result<Self, Self::Error> {
        let len = wire.target.len();
        if !(2..=MAX_SAMPLE_PAIRS).contains(&len) || wire.source.len() != len {
            return Err("class-25 samples require equal bounded array lengths");
        }
        let mut pairs = Self {
            target: [FiniteReal::ZERO; MAX_SAMPLE_PAIRS],
            source: [FiniteReal::ZERO; MAX_SAMPLE_PAIRS],
            len,
        };
        pairs.target[..len].copy_from_slice(&wire.target);
        pairs.source[..len].copy_from_slice(&wire.source);
        pairs
            .valid()
            .then_some(pairs)
            .ok_or("class-25 samples must be strictly monotone")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Class25SourceRelation {
    Absent,
    Affine {
        range: IncreasingParameterInterval,
        scale: NonZeroReal,
        offset: FiniteReal,
    },
    Sampled {
        range: IncreasingParameterInterval,
        samples: Class25SamplePairs,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Class25ParameterMap {
    pub(crate) source_operand: Class25Operand,
    pub(crate) target_operand: Class25Operand,
    pub(crate) selector: Class25Selector,
    pub(crate) target_range: IncreasingParameterInterval,
    pub(crate) accuracy: Class25Accuracy,
    pub(crate) source: Class25SourceRelation,
    pub(crate) sites: TargetSites,
}

pub(crate) fn parse(payload: &[u8]) -> Option<Class25ParameterMap> {
    if payload.first() != Some(&0x82) || payload.len() > usize::from(u8::MAX) {
        return None;
    }
    let mut at = 1;
    let source_operand = allocation_reference(payload, &mut at)?;
    let target_operand = allocation_reference(payload, &mut at)?;
    let lead = *payload.get(at)?;
    if !matches!(lead, 0x0a | 0x0b) {
        return None;
    }
    let selector = Class25Selector::try_from(View::u16_le_at(payload, at + 1)?).ok()?;
    let mut view = View::over_retained(payload.get(at + 3..)?);
    let target_range = IncreasingParameterInterval::new([view.f64_le()?, view.f64_le()?])?;
    let signed = FiniteReal::new(view.f64_le()?)?;
    let source = match selector.0 >> 8 {
        5 => Class25SourceRelation::Absent,
        13 => {
            let range = IncreasingParameterInterval::new([view.f64_le()?, view.f64_le()?])?;
            let scale = NonZeroReal::new(view.f64_le()?)?;
            let offset = FiniteReal::new(view.f64_le()?)?;
            let mut endpoints = range.endpoints();
            if scale.get() < 0.0 {
                endpoints.reverse();
            }
            if !endpoints
                .into_iter()
                .zip(target_range.endpoints())
                .all(|(s, t)| endpoints_agree(scale.get() * s + offset.get(), t))
            {
                return None;
            }
            Class25SourceRelation::Affine {
                range,
                scale,
                offset,
            }
        }
        21 if lead == 0x0a => {
            let range = IncreasingParameterInterval::new([view.f64_le()?, view.f64_le()?])?;
            let len = usize::from(view.u8()?.checked_sub(0x80)?);
            if !(2..=MAX_SAMPLE_PAIRS).contains(&len) {
                return None;
            }
            let mut samples = Class25SamplePairs {
                target: [FiniteReal::ZERO; MAX_SAMPLE_PAIRS],
                source: [FiniteReal::ZERO; MAX_SAMPLE_PAIRS],
                len,
            };
            for value in &mut samples.target[..len] {
                *value = FiniteReal::new(view.f64_le()?)?;
                if !(target_range.lower()..=target_range.upper()).contains(&value.get()) {
                    return None;
                }
            }
            for value in &mut samples.source[..len] {
                *value = FiniteReal::new(view.f64_le()?)?;
                if !(range.lower()..=range.upper()).contains(&value.get()) {
                    return None;
                }
            }
            if !samples.valid() {
                return None;
            }
            Class25SourceRelation::Sampled { range, samples }
        }
        _ => return None,
    };
    let read_site = |view: &mut View<'_>| -> Option<FiniteReal> {
        let parameter = FiniteReal::new(view.f64_le()?)?;
        (target_range.lower()..=target_range.upper())
            .contains(&parameter.get())
            .then_some(parameter)
    };
    let sites = match selector.0 & 0xff {
        0x87 => TargetSites::Absent,
        0x86 => TargetSites::Lower {
            parameter: read_site(&mut view)?,
        },
        0x85 => TargetSites::Upper {
            parameter: read_site(&mut view)?,
        },
        0x84 => {
            let lower = read_site(&mut view)?;
            let upper = read_site(&mut view)?;
            if lower > upper {
                return None;
            }
            TargetSites::Both { lower, upper }
        }
        _ => return None,
    };
    let accuracy = if lead == 0x0a {
        let fit_tolerance = NonNegativeReal::new(view.f64_le()?)?;
        if fit_tolerance.get() < signed.get().abs() {
            return None;
        }
        Class25Accuracy::Fitted {
            signed,
            fit_tolerance,
        }
    } else {
        if signed.get() != 0.0 {
            return None;
        }
        Class25Accuracy::Elided {
            negative_zero: signed.get().is_sign_negative(),
        }
    };
    view.is_empty().then_some(Class25ParameterMap {
        source_operand: Class25Operand {
            value: source_operand.value,
            encoding: source_operand.encoding,
        },
        target_operand: Class25Operand {
            value: target_operand.value,
            encoding: target_operand.encoding,
        },
        selector,
        target_range,
        accuracy,
        source,
        sites,
    })
}

#[cfg(test)]
mod tests;
