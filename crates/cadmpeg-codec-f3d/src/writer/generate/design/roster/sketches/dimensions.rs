// SPDX-License-Identifier: Apache-2.0
//! The records of a `Sketch` item's dimensions (f3d.md §3.1, "Sketch
//! dimensions"): per dimension the frame of its kind, then the parameter, its
//! owner and the owner's companion.

use std::collections::HashMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use super::super::super::frames::DesignRecords;
use super::super::super::sketch::dimensions::{
    DimensionKind, Measure, PlannedDimension, DRIVEN_DECIMALS,
};
use super::super::super::types;
use super::super::owned::Owned;
use super::super::{count, Context, PARAMETER_DISCRIMINATOR};
use super::{curve_payload, Item};
use crate::writer::generate::presentation::TypeRow;

/// The relation ordinal of the first frame's null locus; the `j`-th frame of
/// the container's frame run takes `j` more (f3d-open-items.md DR-917).
const NULL_LOCUS_ORDINAL: u32 = 3;
/// The prefix of a frame whose expression names a parameter.
const FORMULA_PREFIX: &str = "fx: ";
/// The source-kind suffix of a sketch's first dimension of a kind.
const FIRST_SUFFIX: u32 = 2;
/// The text frame's default style byte and values.
const STYLE: u8 = 0;
const STYLE_VALUES: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
/// The text frame's style byte and values in a driven dimension.
const DRIVEN_STYLE: u8 = 1;
const DRIVEN_STYLE_VALUES: [f32; 4] = [0.25, 0.25, 0.25, 1.0];
/// The text frame's byte before its zero byte.
const TEXT_MARKER: u8 = 1;
/// The text frame's origin and axes.
const TEXT_ORIGIN: [f64; 3] = [0.0, 0.0, 0.0];
const TEXT_AXES: [[f64; 3]; 2] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
/// The u32 of a linear kind block and its two f64 values.
const LINEAR_KIND_WORD: u32 = 2;
const LINEAR_KIND_VALUES: [f64; 2] = [0.0, 1.0];
/// The flag byte of a radial and of a diameter measurement.
const RADIAL_FLAG: u8 = 1;
const DIAMETER_FLAG: u8 = 0;

/// The frame type of `kind`.
pub(super) const fn frame_row(kind: DimensionKind) -> TypeRow {
    match kind {
        DimensionKind::Aligned | DimensionKind::Horizontal | DimensionKind::Vertical => {
            types::LINEAR_FRAME
        }
        DimensionKind::Angle => types::ANGULAR_FRAME,
        DimensionKind::Diameter => types::DIAMETER_FRAME,
        DimensionKind::Radius => types::RADIAL_FRAME,
    }
}

impl Item<'_> {
    /// Each dimension's frame, parameter, owner and companion. `named` is
    /// the number of relation ordinals each member has taken in the
    /// relations.
    pub(super) fn dimensions(
        &self,
        out: &mut DesignRecords,
        mut named: HashMap<u32, u32>,
    ) -> Result<(), CodecError> {
        let records = self.records;
        if records.dimensions.is_empty() {
            return Ok(());
        }
        #[cfg(test)]
        if let Some(error) = super::super::super::requested_dimension_failure() {
            return Err(error);
        }
        let paired = self.ctx.class(types::DIMENSION_FRAME_PAIRED.guid)?;
        let mut stems = HashMap::<&str, u32>::new();
        for (
            (local, dimension),
            super::DimensionRecords {
                frame,
                parameter,
                owner,
                companion,
                ..
            },
        ) in (0u32..)
            .zip(&self.planned.dimensions)
            .zip(&records.dimensions)
        {
            let semantic = dimension
                .members
                .iter()
                .map(|member| self.record_of(*member))
                .collect::<Result<Vec<_>, _>>()?;
            let mut ascending = semantic.clone();
            ascending.sort_unstable();
            ascending.dedup();
            let null_ordinal = NULL_LOCUS_ORDINAL
                .checked_add(local)
                .ok_or_else(|| CodecError::NotImplemented("dimension count exceeds u32".into()))?;
            out.record(&self.ctx.class(frame_row(dimension.kind).guid)?, *frame)
                .zeros(8)
                .byte(1)
                .u32(count(ascending.len() + 1)?)
                .reference(0)
                .u32(null_ordinal);
            for member in &ascending {
                let ordinal = named.entry(*member).or_insert(0);
                out.reference(*member).u32(*ordinal);
                *ordinal += 1;
            }
            out.byte(0);
            self.measurement(out, dimension)?;
            text_frame(out, dimension.driven);
            let mut prefix = String::new();
            if dimension.parameter.names_parameter {
                prefix.push_str(FORMULA_PREFIX);
            }
            prefix.push_str(dimension.kind.symbol());
            out.lp_utf16(&prefix)?.lp_utf16(&dimension.text())?;
            kind_block(out, dimension)?;
            out.reference(*owner).u32(count(semantic.len())?);
            for member in &semantic {
                out.reference(*member);
            }
            out.byte(0)
                .header(&paired, *frame)
                .zeros(8)
                .reference(records.container)
                .zeros(12)
                .f32(1.0)
                .zeros(2)
                .reference(records.container);
            let stem = dimension.kind.stem();
            let suffix = stems.entry(stem).or_insert(FIRST_SUFFIX);
            let kind = format!("{stem}-{suffix}");
            *suffix += 1;
            super::super::owned::write(
                self.ctx,
                out,
                &Owned {
                    owner: *owner,
                    parameter: *parameter,
                    companion: *companion,
                    scope: records.scope,
                    local,
                    kind: &kind,
                    value: &dimension.parameter,
                    driven: dimension.driven,
                },
            )?;
        }
        Ok(())
    }

    /// The measurement of the frame's type.
    fn measurement(
        &self,
        out: &mut DesignRecords,
        dimension: &PlannedDimension,
    ) -> Result<(), CodecError> {
        match &dimension.measure {
            Measure::Linear {
                p0,
                p1,
                a1,
                a2,
                value,
                ..
            } => {
                for point in [p0, p1, a2, a1] {
                    position(out, *point);
                }
                out.byte(0).f64(*value);
                position(out, *a1);
            }
            Measure::Radial {
                curve,
                centre,
                attach,
                radius,
                ..
            } => {
                let geometry = &self
                    .planned
                    .curves
                    .get(*curve)
                    .ok_or_else(|| CodecError::malformed(format_args!("sketch curve {curve}")))?
                    .geometry;
                for value in curve_payload(geometry, &self.planned.points)? {
                    out.f64(value);
                }
                position(out, *centre);
                position(out, *attach);
                out.byte(if dimension.kind == DimensionKind::Diameter {
                    DIAMETER_FLAG
                } else {
                    RADIAL_FLAG
                })
                .f64(*radius);
            }
            Measure::Angle {
                points,
                radius,
                quadrant,
                ..
            } => {
                out.f64(dimension.measure.value())
                    .u32(*quadrant)
                    .zeros(2)
                    .u32(*quadrant);
                for point in points {
                    position(out, *point);
                }
                position(out, arc(&dimension.measure)?);
                out.f64(*radius);
            }
        }
        Ok(())
    }
}

/// The arc point of an angular measurement.
fn arc(measure: &Measure) -> Result<[f64; 2], CodecError> {
    measure
        .arc()
        .ok_or_else(|| CodecError::Malformed("an angular dimension between parallel lines".into()))
}

/// A sketch point as three f64 with `w = 0`.
fn position(out: &mut DesignRecords, [u, v]: [f64; 2]) {
    out.f64(u).f64(v).f64(0.0);
}

/// The 91-byte text frame in the default style, or the driven style for a
/// driven dimension, its origin at the sketch origin and its axes the sketch
/// axes.
fn text_frame(out: &mut DesignRecords, driven: bool) {
    let (style, values) = if driven {
        (DRIVEN_STYLE, DRIVEN_STYLE_VALUES)
    } else {
        (STYLE, STYLE_VALUES)
    };
    out.byte(style);
    for value in values {
        out.f32(value);
    }
    out.byte(TEXT_MARKER).byte(0);
    for vector in [TEXT_ORIGIN, TEXT_AXES[0], TEXT_AXES[1]] {
        for component in vector {
            out.f64(component);
        }
    }
}

/// The kind block: the text position and, in a linear frame, `u32 2` and
/// two f64 values; then the kind mask.
fn kind_block(out: &mut DesignRecords, dimension: &PlannedDimension) -> Result<(), CodecError> {
    match &dimension.measure {
        Measure::Linear { text, .. } => {
            position(out, *text);
            out.u32(LINEAR_KIND_WORD);
            for value in LINEAR_KIND_VALUES {
                out.f64(value);
            }
        }
        Measure::Radial { text, .. } => position(out, *text),
        Measure::Angle { .. } => position(out, arc(&dimension.measure)?),
    }
    out.u64(dimension.kind.mask());
    Ok(())
}

/// The values the writer chooses where f3d.md and f3d-open-items.md DR-901
/// and DR-917 leave the rule open, in the records of the sketch relations
/// and dimensions of `ctx`.
pub(in crate::writer::generate::design::roster) fn losses(ctx: &Context<'_>) -> Vec<LossNote> {
    let mut related = Vec::new();
    let mut frames = Vec::new();
    let mut parameters = Vec::new();
    let mut driven = Vec::new();
    for records in &ctx.roster.sketches.items {
        related.extend(records.relations.iter().map(u32::to_string));
        for dimension in &records.dimensions {
            related.push(dimension.frame.to_string());
            frames.push(dimension.frame.to_string());
            parameters.push(dimension.parameter.to_string());
            if dimension.driven {
                driven.push(format!("{} {}", dimension.frame, dimension.parameter));
            }
        }
    }
    if related.is_empty() {
        return Vec::new();
    }
    let mut values = vec![format!(
        "relation ordinals {} (a member's ordinal in a relation or dimension frame the number of relations and frames of its sketch written before it that name the member)",
        related.join(" ")
    )];
    if !frames.is_empty() {
        values.extend([
            format!(
                "dimension frames {} (null-locus ordinal {NULL_LOCUS_ORDINAL} plus the frame's position in its sketch; the fourth point of a linear measurement its first anchor; each angular line's point positions, end then start; text frame style {STYLE} with values (0, 0, 0, 1) in a driving dimension, byte {TEXT_MARKER}, origin (0, 0, 0) and axes (1, 0, 0), (0, 1, 0); linear kind-block values (0, 1); diameter flag {DIAMETER_FLAG}; the second quadrant word equal to the first; no property block)",
                frames.join(" ")
            ),
            format!(
                "dimension parameters {} (family discriminator {}, the owner's parameter number the source ordinal, the dimension suffix {FIRST_SUFFIX} for a sketch's first dimension of a kind and one more for each later one; the expression trailer of ten zero bytes for a driving dimension)",
                parameters.join(" "),
                PARAMETER_DISCRIMINATOR.code()
            ),
        ]);
        if !driven.is_empty() {
            values.push(format!(
                "driven dimension frames and parameters {} (the second flag byte of the parameter's expression trailer `1` as its driven flag, f3d-open-items.md DR-144; the display text with {DRIVEN_DECIMALS} decimals and the unit of the parameter, DR-917)",
                driven.join(", ")
            ));
        }
    }
    cadmpeg_ir::codec::write::record_residual_unavailable(
        "F3D",
        "writes the sketch relations and dimensions with these values where f3d-open-items.md DR-901 and DR-917 state no rule",
        values,
    )
    .into_iter()
    .collect()
}
