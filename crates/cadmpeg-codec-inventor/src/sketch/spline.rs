// SPDX-License-Identifier: Apache-2.0
//! Stored planar spline arrays and their closed polynomial branch.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::sketches::{SketchEntity, SketchGeometryDefinition};
use serde::{Deserialize, Serialize};

use super::{PmDcSketchEntityKind, PmDcSketchEntityPayload};
use crate::pmdc::{
    content_header, reference_list_with_context, u32_list_with_context, Cursor, PmDcReferenceList,
    PmDcU32List,
};

pub(super) const TYPE: [u8; 16] = [
    0xd4, 0x2f, 0x37, 0xf9, 0xd1, 0x11, 0x15, 0xd3, 0x00, 0x08, 0x47, 0xb0, 0x05, 0x24, 0xdc, 0x09,
];
pub(super) const POINT_RELATION_TYPE: [u8; 16] = [
    0x9e, 0x14, 0x6d, 0x7c, 0xd1, 0x11, 0x5b, 0xd5, 0x00, 0x08, 0x47, 0xb0, 0x05, 0x24, 0xdc, 0x09,
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SplineRecord {
    pub(crate) points: PmDcReferenceList,
    values: [u32; 9],
    point_values: PmDcU32List,
    state: u32,
    primary: SplineCurve,
    curve_state: u8,
    pub(crate) auxiliary: [PmDcReferenceList; 2],
    auxiliary_values: [u32; 3],
    secondary_prefix: u8,
    secondary_present: u32,
    secondary_state: u8,
    secondary: SplineCurve,
    terminal_values: [u32; 9],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct SplineCurve {
    form: u32,
    degree: u32,
    tolerance: FiniteReal,
    knot_value: u32,
    knots: Vec<FiniteReal>,
    knot_values: [u32; 2],
    pole_value: u32,
    poles: Vec<[FiniteReal; 2]>,
    pole_tolerance: FiniteReal,
    values: [u32; 2],
    interval: [FiniteReal; 2],
    terminal_value: FiniteReal,
}

fn reals(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    count: u32,
) -> Result<Vec<FiniteReal>, CodecError> {
    let count = cursor.view().counted(u64::from(count), 8).ok_or_else(|| {
        CodecError::malformed("Inventor spline scalar count exceeds remaining payload")
    })?;
    let mut values = ctx.alloc_filled(
        count.get(),
        FiniteReal::ZERO,
        "read Inventor spline scalars",
    )?;
    for value in &mut values {
        *value = cursor.f64("spline scalar")?;
    }
    Ok(values)
}

fn width(cursor: &mut Cursor<'_>) -> Result<(), CodecError> {
    if cursor.u32("spline scalar width")? != 8 {
        return Err(CodecError::NotImplemented(
            "Inventor spline scalar width is not 8".into(),
        ));
    }
    Ok(())
}

fn curve(ctx: &DecodeContext<'_>, cursor: &mut Cursor<'_>) -> Result<SplineCurve, CodecError> {
    let form = cursor.u32("spline curve form")?;
    let degree = cursor.u32("spline degree")?;
    let tolerance = cursor.f64("spline tolerance")?;
    let knot_count = cursor.u32("spline knot count")?;
    let knot_value = cursor.u32("spline knot value")?;
    width(cursor)?;
    let knots = reals(ctx, cursor, knot_count)?;
    let knot_values = cursor.u32_array("spline knot values")?;
    if knot_values != [0, 0] {
        return Err(CodecError::NotImplemented(
            "Inventor spline has nonzero knot values".into(),
        ));
    }
    width(cursor)?;
    let pole_count = cursor.u32("spline pole count")?;
    let pole_value = cursor.u32("spline pole value")?;
    width(cursor)?;
    let count = cursor
        .view()
        .counted(u64::from(pole_count), 16)
        .ok_or_else(|| {
            CodecError::malformed("Inventor spline pole count exceeds remaining payload")
        })?;
    let mut poles = ctx.alloc_filled(
        count.get(),
        [FiniteReal::ZERO; 2],
        "read Inventor spline poles",
    )?;
    for pole in &mut poles {
        *pole = super::point2(cursor, "spline pole")?;
    }
    let pole_tolerance = cursor.f64("spline pole tolerance")?;
    let values = cursor.u32_array("spline curve values")?;
    let interval = super::point2(cursor, "spline interval")?;
    let terminal_value = cursor.f64("spline curve terminal value")?;
    width(cursor)?;
    Ok(SplineCurve {
        form,
        degree,
        tolerance,
        knot_value,
        knots,
        knot_values,
        pole_value,
        poles,
        pole_tolerance,
        values,
        interval,
        terminal_value,
    })
}

pub(super) fn parse(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcSketchEntityPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    let entity_flags = cursor.u32("spline entity flags")?;
    let sketch = cursor.reference("spline sketch reference")?;
    let data = SplineRecord {
        points: reference_list_with_context(ctx, &mut cursor, 2, "spline point list")?,
        values: cursor.u32_array("spline values")?,
        point_values: u32_list_with_context(ctx, &mut cursor, 2, "spline point value list")?,
        state: cursor.u32("spline state")?,
        primary: curve(ctx, &mut cursor)?,
        curve_state: cursor.u8("spline curve state")?,
        auxiliary: [
            reference_list_with_context(ctx, &mut cursor, 2, "spline auxiliary list")?,
            reference_list_with_context(ctx, &mut cursor, 2, "spline auxiliary list")?,
        ],
        auxiliary_values: cursor.u32_array("spline auxiliary values")?,
        secondary_prefix: cursor.u8("spline secondary prefix")?,
        secondary_present: cursor.u32("spline secondary presence")?,
        secondary_state: cursor.u8("spline secondary state")?,
        secondary: curve(ctx, &mut cursor)?,
        terminal_values: cursor.u32_array("spline terminal values")?,
    };
    cursor.finish("spline")?;
    Ok(PmDcSketchEntityPayload {
        save_version_major: version,
        header,
        entity_flags,
        sketch,
        kind: PmDcSketchEntityKind::Spline {
            data: Box::new(data),
        },
    })
}

impl SplineRecord {
    pub(super) fn closed_curve(&self) -> Option<PcurveNurbs> {
        let curve = &self.primary;
        if self.curve_state != 2
            || self.secondary_present != 1
            || curve != &self.secondary
            || curve.form != 1
            || curve.degree == 0
            || curve.values != [1, 1]
            || curve.poles.first()? != curve.poles.last()?
            || self.state != 0
            || self.values != [0; 9]
            || self.auxiliary_values != [0; 3]
            || self.secondary_prefix != 0
            || self.secondary_state != 0
            || self.terminal_values != [2, 0, 0, 0, 0, 0, 0, 0, 0]
            || curve.knot_values != [0, 0]
            || curve.terminal_value != FiniteReal::ZERO
            || usize::try_from(curve.pole_value).ok()? != curve.poles.len()
        {
            return None;
        }
        let degree = usize::try_from(curve.degree).ok()?;
        if curve.interval
            != [
                *curve.knots.get(degree)?,
                *curve.knots.get(curve.poles.len())?,
            ]
        {
            return None;
        }
        let first = *curve.knots.get(degree)?;
        let last = *curve.knots.get(curve.poles.len())?;
        if first >= last
            || !curve
                .knots
                .get(..=degree)?
                .iter()
                .all(|knot| *knot == first)
            || !curve
                .knots
                .get(curve.poles.len()..)?
                .iter()
                .all(|knot| *knot == last)
        {
            return None;
        }
        PcurveNurbs::from_lanes(
            curve.degree,
            curve.knots.iter().map(|x| x.get()).collect::<Vec<_>>(),
            curve
                .poles
                .iter()
                .map(|p| Point2::new(p[0].get(), p[1].get()))
                .collect(),
            None,
            true,
        )
        .ok()
    }
}

pub(super) fn is_closed(entity: &SketchEntity) -> bool {
    let SketchGeometryDefinition::Nurbs { curve } = entity.geometry.definition() else {
        return false;
    };
    let points = curve.control_points();
    curve.periodic()
        && points
            .first()
            .is_some_and(|first| Some(first) == points.last())
}

#[cfg(test)]
mod tests;
