// SPDX-License-Identifier: Apache-2.0
//! Retained numeric continuations of consolidated rolling-ball jets.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::layout::{
    rolling_ball_tail_prefix as prefix, rolling_ball_tail_scalar_branch as scalar_branch,
    rolling_ball_tail_unmapped_suffix as unmapped,
};
use crate::wire::bytes::compact_int;
use crate::wire::records::{
    ConsolidatedFamily, ConsolidatedFrameWidth, ConsolidatedRawFrame, ConsolidatedRecord,
};

const SCALAR_LANES: usize = 3;
const WORD_BYTES: usize = std::mem::size_of::<u64>();

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ScalarJet {
    Absent,
    Explicit {
        values_bits: Vec<u64>,
        first_derivative_bits: Vec<u64>,
        second_derivative_bits: Vec<u64>,
        terminal: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ContinuationWire")]
pub(crate) struct CatiaRollingBallContinuation {
    pub(super) id: String,
    pub(super) source_index: u64,
    pub(super) source_offset: u64,
    pub(super) frame: ConsolidatedRawFrame<u64>,
    pub(super) site_count: u64,
    pub(super) tail_offset: u64,
    pub(super) current_u_bits: [u64; 2],
    pub(super) current_v_bits: [u64; 2],
    pub(super) reference_scalar_bits: u64,
    pub(super) raw_control: u8,
    pub(super) maximal_u_bits: [u64; 2],
    pub(super) maximal_v_bits: [u64; 2],
    pub(super) reference_scalar_repeat_bits: Option<u64>,
    pub(super) scalar_jet: ScalarJet,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContinuationWire {
    id: String,
    source_index: u64,
    source_offset: u64,
    frame: ConsolidatedRawFrame<u64>,
    site_count: u64,
    tail_offset: u64,
    current_u_bits: [u64; 2],
    current_v_bits: [u64; 2],
    reference_scalar_bits: u64,
    raw_control: u8,
    maximal_u_bits: [u64; 2],
    maximal_v_bits: [u64; 2],
    reference_scalar_repeat_bits: Option<u64>,
    scalar_jet: ScalarJet,
}

#[derive(Clone, Copy)]
enum ScalarSource {
    Absent,
    Explicit { start: usize, terminal: u32 },
}

struct Fields {
    count: usize,
    tail_start: usize,
    current: [u64; 5],
    maximal_u: [u64; 2],
    maximal_v: [u64; 2],
    control: u8,
    repeat: Option<u64>,
    scalar: ScalarSource,
}

fn word(view: &mut View<'_>) -> Option<u64> {
    let value = view.f64_le()?;
    value.is_finite().then(|| value.to_bits())
}

fn pair(view: &mut View<'_>) -> Option<[u64; 2]> {
    Some([word(view)?, word(view)?])
}

fn fields(payload: &[u8]) -> Option<Fields> {
    let mut at = 0;
    let count = usize::try_from(compact_int(payload, &mut at)?).ok()?;
    if count < 2
        || compact_int(payload, &mut at)? != 5
        || usize::try_from(compact_int(payload, &mut at)?).ok()? != count
    {
        return None;
    }
    let mut view = View::over_retained(payload);
    view.seek(at)?;
    match view.u8()? {
        0x0c => {}
        0x08 if view.u8()? % 4 == 1 => {}
        _ => return None,
    }
    view.counted(u64_from_index(count), 31usize.checked_mul(WORD_BYTES)?)?;
    let first = word(&mut view)?;
    let mut previous = f64::from_bits(first);
    for _ in 1..count {
        let next = f64::from_bits(word(&mut view)?);
        if next <= previous {
            return None;
        }
        previous = next;
    }
    for _ in 0..count.checked_mul(30)? {
        word(&mut view)?;
    }
    let tail_start = view.position();
    let tail = payload.get(tail_start..)?;
    let mut view = View::over_retained(tail);
    view.seek(prefix::PARAMETERS)?;
    let current = [
        word(&mut view)?,
        word(&mut view)?,
        word(&mut view)?,
        word(&mut view)?,
        word(&mut view)?,
    ];
    view.seek(prefix::CONTROL)?;
    let control = view.u8()?;
    view.seek(prefix::MAXIMAL_V)?;
    let maximal_v = pair(&mut view)?;
    let current_values = current.map(f64::from_bits);
    let maximal_v_values = maximal_v.map(f64::from_bits);
    if !matches!(control, 0x03 | 0x05)
        || current_values[0] >= current_values[1]
        || current_values[2] >= current_values[3]
        || current_values[4] <= 0.0
        || current[3] != current[4]
        || maximal_v_values[0] > current_values[2]
        || maximal_v_values[1] < current_values[3]
    {
        return None;
    }
    let absent_length = prefix::LEN.checked_add(unmapped::LEN)?;
    let repeated_length = absent_length.checked_add(WORD_BYTES)?;
    let scalar = if tail.len() == absent_length || tail.len() == repeated_length {
        view.seek(prefix::LEN.checked_add(unmapped::BRANCH)?)?;
        for expected in unmapped::BRANCH_VALUE {
            if view.u8()? != expected {
                return None;
            }
        }
        view.seek(prefix::LEN.checked_add(unmapped::MAXIMAL_U)?)?;
        ScalarSource::Absent
    } else {
        let lane_words = count.checked_mul(SCALAR_LANES)?;
        if tail.len() != repeated_length.checked_add(lane_words.checked_mul(WORD_BYTES)?)? {
            return None;
        }
        view.seek(prefix::LEN)?;
        for expected in scalar_branch::BRANCH_VALUE {
            if view.u8()? != expected {
                return None;
            }
        }
        let start = tail_start.checked_add(view.position())?;
        view.counted(u64_from_index(lane_words), WORD_BYTES)?;
        for _ in 0..lane_words {
            word(&mut view)?;
        }
        let terminal = view.u32_le()?;
        if terminal > 1 {
            return None;
        }
        ScalarSource::Explicit { start, terminal }
    };
    let maximal_u = pair(&mut view)?;
    let maximal_u_values = maximal_u.map(f64::from_bits);
    if maximal_u_values[0] < f64::from_bits(first)
        || maximal_u_values[0] > current_values[0]
        || maximal_u_values[1] < current_values[1]
        || maximal_u_values[1] > previous
    {
        return None;
    }
    let repeat = if tail.len() == absent_length {
        None
    } else {
        let repeat = word(&mut view)?;
        if repeat != current[4] {
            return None;
        }
        Some(repeat)
    };
    if view.remaining() != 0 {
        return None;
    }
    Some(Fields {
        count,
        tail_start,
        current,
        maximal_u,
        maximal_v,
        control,
        repeat,
        scalar,
    })
}

fn header_valid(width: ConsolidatedFrameWidth, token: u32) -> bool {
    width != ConsolidatedFrameWidth::One || token % 4 == 1
}

fn source_scalar_matches(
    source: ScalarSource,
    jet: &ScalarJet,
    count: usize,
    payload: &[u8],
) -> bool {
    match (source, jet) {
        (ScalarSource::Absent, ScalarJet::Absent) => true,
        (
            ScalarSource::Explicit { start, terminal },
            ScalarJet::Explicit {
                values_bits,
                first_derivative_bits,
                second_derivative_bits,
                terminal: actual,
            },
        ) => {
            let mut view = View::over_retained(payload);
            if view.seek(start).is_none() || terminal != *actual {
                return false;
            }
            for lane in [values_bits, first_derivative_bits, second_derivative_bits] {
                if lane.len() != count
                    || lane
                        .iter()
                        .any(|expected| word(&mut view) != Some(*expected))
                {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

fn valid_record(record: &CatiaRollingBallContinuation) -> bool {
    let width = record.frame.width();
    header_valid(width, record.frame.header_token())
        && fields(&record.frame.payload).is_some_and(|parsed| {
            let header = 7 + usize::from(u8::from(width));
            record.site_count == u64_from_index(parsed.count)
                && record.tail_offset == u64_from_index(header + parsed.tail_start)
                && record.current_u_bits == [parsed.current[0], parsed.current[1]]
                && record.current_v_bits == [parsed.current[2], parsed.current[3]]
                && record.reference_scalar_bits == parsed.current[4]
                && record.raw_control == parsed.control
                && record.maximal_u_bits == parsed.maximal_u
                && record.maximal_v_bits == parsed.maximal_v
                && record.reference_scalar_repeat_bits == parsed.repeat
                && source_scalar_matches(
                    parsed.scalar,
                    &record.scalar_jet,
                    parsed.count,
                    &record.frame.payload,
                )
        })
}

impl TryFrom<ContinuationWire> for CatiaRollingBallContinuation {
    type Error = &'static str;
    fn try_from(wire: ContinuationWire) -> Result<Self, Self::Error> {
        let record = Self {
            id: wire.id,
            source_index: wire.source_index,
            source_offset: wire.source_offset,
            frame: wire.frame,
            site_count: wire.site_count,
            tail_offset: wire.tail_offset,
            current_u_bits: wire.current_u_bits,
            current_v_bits: wire.current_v_bits,
            reference_scalar_bits: wire.reference_scalar_bits,
            raw_control: wire.raw_control,
            maximal_u_bits: wire.maximal_u_bits,
            maximal_v_bits: wire.maximal_v_bits,
            reference_scalar_repeat_bits: wire.reference_scalar_repeat_bits,
            scalar_jet: wire.scalar_jet,
        };
        if !valid_record(&record) {
            return Err("rolling-ball continuation fields disagree with the complete source frame");
        }
        Ok(record)
    }
}

fn candidates<'a>(
    bytes: &'a [u8],
    records: &'a [ConsolidatedRecord],
) -> impl Iterator<Item = (&'a ConsolidatedRecord, &'a [u8], Fields)> + 'a {
    records
        .iter()
        .filter(|r| {
            r.family() == ConsolidatedFamily::A
                && r.class() == 0x32
                && header_valid(r.width(), r.header_token())
        })
        .filter_map(|r| {
            let payload = bytes.get(r.payload()?)?;
            Some((r, payload, fields(payload)?))
        })
}

fn scalar_jet(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    parsed: &Fields,
) -> Result<ScalarJet, CodecError> {
    let ScalarSource::Explicit { start, terminal } = parsed.scalar else {
        return Ok(ScalarJet::Absent);
    };
    let mut view = View::over_retained(payload);
    view.seek(start)
        .ok_or_else(|| CodecError::malformed("invalid CATIA scalar-jet offset"))?;
    let mut lane = || -> Result<Vec<u64>, CodecError> {
        let mut words = ctx.alloc_filled(parsed.count, 0u64, "CATIA continuation scalar words")?;
        for target in &mut words {
            *target = word(&mut view)
                .ok_or_else(|| CodecError::malformed("invalid CATIA continuation scalar word"))?;
        }
        Ok(words)
    };
    Ok(ScalarJet::Explicit {
        values_bits: lane()?,
        first_derivative_bits: lane()?,
        second_derivative_bits: lane()?,
        terminal,
    })
}

fn admit(
    ctx: &DecodeContext<'_>,
    ordinal: usize,
    source: &ConsolidatedRecord,
    payload: &[u8],
    parsed: &Fields,
) -> Result<CatiaRollingBallContinuation, CodecError> {
    let mut retained = ctx.alloc_filled(payload.len(), 0u8, "CATIA continuation source payload")?;
    retained.copy_from_slice(payload);
    let frame = ConsolidatedRawFrame::from_record(source, retained)?.into();
    let jet = scalar_jet(ctx, payload, parsed)?;
    Ok(CatiaRollingBallContinuation {
        id: format!("catia:consolidated:rolling-ball-continuation#{ordinal}"),
        source_index: u64_from_index(source.source_index()),
        source_offset: u64_from_index(source.source_range().start),
        frame,
        site_count: u64_from_index(parsed.count),
        tail_offset: u64_from_index(7 + usize::from(u8::from(source.width())) + parsed.tail_start),
        current_u_bits: [parsed.current[0], parsed.current[1]],
        current_v_bits: [parsed.current[2], parsed.current[3]],
        reference_scalar_bits: parsed.current[4],
        raw_control: parsed.control,
        maximal_u_bits: parsed.maximal_u,
        maximal_v_bits: parsed.maximal_v,
        reference_scalar_repeat_bits: parsed.repeat,
        scalar_jet: jet,
    })
}

pub(super) fn derive(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaRollingBallContinuation>, CodecError> {
    let mut source = candidates(bytes, records);
    let Some((r, p, f)) = source.next() else {
        return Ok(Vec::new());
    };
    let count = source.count() + 1;
    ctx.charge_entities(
        u64_from_index(count),
        "admit CATIA rolling-ball continuations",
    )?;
    let first = admit(ctx, 0, r, p, &f)?;
    let mut result = ctx.alloc_filled(count, first, "CATIA continuation records")?;
    for (ordinal, (target, (r, p, f))) in result
        .iter_mut()
        .zip(candidates(bytes, records))
        .enumerate()
    {
        *target = admit(ctx, ordinal, r, p, &f)?;
    }
    Ok(result)
}

#[cfg(test)]
pub(super) fn validate(
    records: &[CatiaRollingBallContinuation],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (i, r) in records.iter().enumerate() {
        if r.id != format!("catia:consolidated:rolling-ball-continuation#{i}")
            || !valid_record(r)
            || (i > 0
                && (records[i - 1].source_index, records[i - 1].source_offset)
                    >= (r.source_index, r.source_offset))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "invalid rolling-ball continuation source identity or fields".to_owned(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
