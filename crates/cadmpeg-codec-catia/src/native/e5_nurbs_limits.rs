// SPDX-License-Identifier: Apache-2.0
//! Physical source records carrying complete NURBS limit programs.

use cadmpeg_core::decode::{bounded_len, u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::families::e5::limit_program::{CurveLimits, SurfaceLimits};
use crate::layout::e5_record_frame as header;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum LimitProgram {
    Pcurve(CurveLimits),
    Surface(SurfaceLimits),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CatiaE5NurbsLimitRecord {
    pub(crate) id: String,
    byte_offset: u64,
    byte_len: u64,
    record_id: u32,
    class_word: u16,
    limit_payload_offset: u64,
    program: LimitProgram,
}

fn axis_control_count(view: &mut View<'_>) -> Option<usize> {
    let degree = usize::try_from(view.u32_le()?).ok()?;
    if degree == 0 || view.u32_le()? != 0 || view.u32_le()? != 0 {
        return None;
    }
    let count = usize::try_from(view.u32_le()?).ok()?;
    if count < 2 || view.u32_le()? != 0 {
        return None;
    }
    view.counted(u64_from_index(count), 12)?;
    let mut previous = None;
    for _ in 0..count {
        let knot = view.f64_le()?;
        if !knot.is_finite() || previous.is_some_and(|value| knot <= value) {
            return None;
        }
        previous = Some(knot);
    }
    let mut sum = 0usize;
    for _ in 0..count {
        let multiplicity = usize::try_from(view.u32_le()?).ok()?;
        if multiplicity == 0 {
            return None;
        }
        sum = sum.checked_add(multiplicity)?;
    }
    let controls = sum.checked_sub(degree.checked_add(1)?)?;
    (controls > degree).then_some(controls)
}

fn finite_coordinates(view: &mut View<'_>, count: usize, dimensions: usize) -> Option<()> {
    view.counted(u64_from_index(count), dimensions.checked_mul(8)?)?;
    for _ in 0..count.checked_mul(dimensions)? {
        if !view.f64_le()?.is_finite() {
            return None;
        }
    }
    Some(())
}

fn program(payload: &[u8], class_word: u16) -> Option<(usize, LimitProgram)> {
    let mut view = View::over_retained(payload);
    match class_word {
        0x00aa => {
            if view.u8()? != 0x81 {
                return None;
            }
            let mut position = 1;
            crate::wire::tokens::object_ref(payload, &mut position, false)?;
            view.seek(position)?;
            if view.u16_le()? != 0 {
                return None;
            }
            let count = axis_control_count(&mut view)?;
            finite_coordinates(&mut view, count, 2)?;
            let offset = payload.len().checked_sub(view.remaining())?;
            let limits = CurveLimits::read(&mut view)?;
            Some((offset, LimitProgram::Pcurve(limits)))
        }
        0x00e7 => {
            if view.u8()? != 0x80 {
                return None;
            }
            let u = axis_control_count(&mut view)?;
            let v = axis_control_count(&mut view)?;
            let mode = view.u16_le()?;
            if !matches!(mode, 0 | 1) {
                return None;
            }
            let count = u.checked_mul(v)?;
            finite_coordinates(&mut view, count, 3)?;
            if mode == 1 {
                view.counted(u64_from_index(count), 8)?;
                for _ in 0..count {
                    let weight = view.f64_le()?;
                    if !weight.is_finite() || weight == 0.0 {
                        return None;
                    }
                }
            }
            let offset = payload.len().checked_sub(view.remaining())?;
            let limits = SurfaceLimits::read(&mut view)?;
            Some((offset, LimitProgram::Surface(limits)))
        }
        _ => None,
    }
}

fn records(bytes: &[u8]) -> impl Iterator<Item = CatiaE5NurbsLimitRecord> + '_ {
    crate::container::all_e5_record_spans(bytes).filter_map(|span| {
        let frame = bytes.get(span.clone())?;
        let class_word = View::u16_le_at(frame, header::CLASS)?;
        if !matches!(class_word, 0x00aa | 0x00e7) {
            return None;
        }
        let (offset, program) = program(frame.get(header::LEN..)?, class_word)?;
        Some(CatiaE5NurbsLimitRecord {
            id: format!("catia:e5:nurbs-limits#{}", span.start),
            byte_offset: u64_from_index(span.start),
            byte_len: u64_from_index(span.len()),
            record_id: View::u32_le_at(frame, header::RECORD_ID)?,
            class_word,
            limit_payload_offset: u64_from_index(offset),
            program,
        })
    })
}

pub(super) fn derive(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<CatiaE5NurbsLimitRecord>, CodecError> {
    let mut source = records(bytes);
    let Some(first) = source.next() else {
        return Ok(Vec::new());
    };
    let count = source.count() + 1;
    if bounded_len(
        u64_from_index(count),
        header::LEN + CurveLimits::LEN,
        bytes.len(),
    )
    .is_none()
    {
        return Err(CodecError::malformed(
            "E5 limit-program count exceeds source bytes",
        ));
    }
    ctx.charge_entities(u64_from_index(count), "admit CATIA E5 NURBS limit programs")?;
    let mut output = ctx.alloc_filled(count, first, "CATIA E5 NURBS limit programs")?;
    for (target, source) in output.iter_mut().zip(records(bytes)) {
        *target = source;
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
