// SPDX-License-Identifier: Apache-2.0
//! Planar curve members of a sketch: the record grammar and its retained values.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};

use super::{PmDcSketchEntityKind, PmDcSketchEntityPayload};
use crate::pmdc::{content_header, reference_list_with_context, Cursor, PmDcReferenceList};

pub(super) const TYPE: [u8; 16] = [
    0x46, 0x73, 0x7b, 0x31, 0xd3, 0x11, 0x7c, 0x7a, 0x60, 0x00, 0x1c, 0xb3, 0xd1, 0xc1, 0xfb, 0xb0,
];

/// Width in bytes of every scalar of a stored curve lane.
const SCALAR_WIDTH: u32 = 8;
/// Body kind of a curve stored in the record.
const KIND_CURVE: u32 = 4;
/// Body kind of a curve stored as a reference value.
const KIND_REFERENCE: u32 = 5;
/// Marker of a type-2 list.
const LIST_MARKER: u32 = 0x3000_0002;

/// A planar curve member. The curve rule that selects a neutral curve from the
/// stored values is not resolved, so the record stays native.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct MemberCurveRecord {
    pub(crate) points: PmDcReferenceList,
    pub(crate) auxiliary: Vec<PmDcReferenceList>,
    state: u32,
    body: MemberCurveBody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "body", rename_all = "snake_case")]
enum MemberCurveBody {
    /// Body kind 5: one u32 value and no curve lanes.
    Reference { value: u32 },
    /// Body kind 4: one curve.
    Curve(Box<StoredCurve>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoredCurve {
    form: u32,
    degree: u32,
    tolerance: FiniteReal,
    knot_value: u32,
    knots: Vec<FiniteReal>,
    weight_value: u32,
    weights: Vec<FiniteReal>,
    pole_value: u32,
    poles: Vec<[FiniteReal; 2]>,
    pole_tolerance: FiniteReal,
    values: [u32; 2],
    interval: [FiniteReal; 2],
    terminal_value: FiniteReal,
}

fn width(cursor: &mut Cursor<'_>) -> Result<(), CodecError> {
    if cursor.u32("member curve scalar width")? != SCALAR_WIDTH {
        return Err(CodecError::NotImplemented(
            "Inventor member curve scalar width is not 8".into(),
        ));
    }
    Ok(())
}

fn reals(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    count: u32,
) -> Result<Vec<FiniteReal>, CodecError> {
    let count = cursor.view().counted(u64::from(count), 8).ok_or_else(|| {
        CodecError::malformed("Inventor member curve scalar count exceeds remaining payload")
    })?;
    let mut values = ctx.alloc_filled(
        count.get(),
        FiniteReal::ZERO,
        "read Inventor member curve scalars",
    )?;
    for value in &mut values {
        *value = cursor.f64("member curve scalar")?;
    }
    Ok(values)
}

/// The auxiliary lists: one type-2 list, or a u32 gate pair. Gate `0, 0`
/// stores no list. Gate `1, 0` stores one or two type-2 lists.
fn auxiliary_lists(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<Vec<PmDcReferenceList>, CodecError> {
    let mut lists = Vec::new();
    if cursor.peek_u32("member curve auxiliary marker")? == LIST_MARKER {
        lists.push(reference_list_with_context(
            ctx,
            cursor,
            2,
            "member curve auxiliary list",
        )?);
        return Ok(lists);
    }
    let gate = [
        cursor.u32("member curve list gate 0")?,
        cursor.u32("member curve list gate 1")?,
    ];
    match gate {
        [0, 0] => {}
        [1, 0] => {
            lists.push(reference_list_with_context(
                ctx,
                cursor,
                2,
                "member curve auxiliary list",
            )?);
            if cursor.peek_u32("member curve auxiliary marker")? == LIST_MARKER {
                lists.push(reference_list_with_context(
                    ctx,
                    cursor,
                    2,
                    "member curve auxiliary list",
                )?);
            }
        }
        _ => {
            return Err(CodecError::NotImplemented(
                "Inventor member curve list gate is not 0, 0 or 1, 0".into(),
            ))
        }
    }
    Ok(lists)
}

fn stored_curve(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<StoredCurve, CodecError> {
    let form = cursor.u32("member curve form")?;
    let degree = cursor.u32("member curve degree")?;
    let tolerance = cursor.f64("member curve tolerance")?;
    let knot_count = cursor.u32("member curve knot count")?;
    let knot_value = cursor.u32("member curve knot value")?;
    width(cursor)?;
    let knots = reals(ctx, cursor, knot_count)?;
    let weight_count = cursor.u32("member curve weight count")?;
    let weight_value = cursor.u32("member curve weight value")?;
    width(cursor)?;
    let weights = reals(ctx, cursor, weight_count)?;
    let pole_count = cursor.u32("member curve pole count")?;
    let pole_value = cursor.u32("member curve pole value")?;
    width(cursor)?;
    if weight_count != 0 && weight_count != pole_count {
        return Err(CodecError::NotImplemented(
            "Inventor member curve weight and pole counts differ".into(),
        ));
    }
    let scalar_count = pole_count
        .checked_mul(2)
        .ok_or_else(|| CodecError::malformed("Inventor member curve pole count overflows"))?;
    let poles = reals(ctx, cursor, scalar_count)?
        .chunks_exact(2)
        .map(|pole| [pole[0], pole[1]])
        .collect();
    let pole_tolerance = cursor.f64("member curve pole tolerance")?;
    let values = cursor.u32_array("member curve values")?;
    let interval = super::point2(cursor, "member curve interval")?;
    let terminal_value = cursor.f64("member curve terminal value")?;
    width(cursor)?;
    Ok(StoredCurve {
        form,
        degree,
        tolerance,
        knot_value,
        knots,
        weight_value,
        weights,
        pole_value,
        poles,
        pole_tolerance,
        values,
        interval,
        terminal_value,
    })
}

pub(crate) fn parse(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcSketchEntityPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    let entity_flags = cursor.u32("member curve entity flags")?;
    let sketch = cursor.reference("member curve sketch reference")?;
    let points = reference_list_with_context(ctx, &mut cursor, 2, "member curve point list")?;
    let auxiliary = auxiliary_lists(ctx, &mut cursor)?;
    let kind = cursor.u32("member curve body kind")?;
    let state = cursor.u32("member curve state")?;
    let body = match kind {
        KIND_REFERENCE => MemberCurveBody::Reference {
            value: cursor.u32("member curve reference value")?,
        },
        KIND_CURVE => MemberCurveBody::Curve(Box::new(stored_curve(ctx, &mut cursor)?)),
        _ => {
            return Err(CodecError::NotImplemented(
                "Inventor member curve body kind is not 4 or 5".into(),
            ))
        }
    };
    cursor.finish("member curve")?;
    Ok(PmDcSketchEntityPayload {
        save_version_major: version,
        header,
        entity_flags,
        sketch,
        kind: PmDcSketchEntityKind::MemberCurve {
            data: Box::new(MemberCurveRecord {
                points,
                auxiliary,
                state,
                body,
            }),
        },
    })
}

impl MemberCurveRecord {
    /// Why the record stays native.
    pub(super) fn native_reason(&self) -> &'static str {
        match self.body {
            MemberCurveBody::Reference { .. } => {
                "curve is stored as a reference value whose referent is unresolved"
            }
            MemberCurveBody::Curve(_) => {
                "curve knot, weight, and pole lanes require an exact curve rule"
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
