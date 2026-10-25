// SPDX-License-Identifier: Apache-2.0
//! Accepted original source getters used by multi-input neutral producers.

use super::*;
use std::ops::Range;
pub(crate) mod geometry;
mod location;
mod placed;
pub(crate) mod polygon;
mod roots;
mod topology;
mod triangulation;

#[derive(Default)]
pub(super) struct RowMarks {
    pub(super) counts: BTreeMap<u8, usize>,
    pub(super) markers: BTreeMap<u8, Range<usize>>,
    pub(super) root: Option<Range<usize>>,
    pub(super) rows: BTreeMap<(u8, usize), Range<usize>>,
    pub(super) pcurves: BTreeMap<(usize, usize), Range<usize>>,
}

struct PayloadSource<'a> {
    source: View<'a>,
    binary: bool,
    marks: RowMarks,
}

#[derive(Default)]
pub(crate) struct ProjectionSources<'a> {
    payloads: BTreeMap<String, PayloadSource<'a>>,
}

pub(crate) struct EdgeInputs {
    pub(crate) shape: TextTShape,
    pub(crate) transform: Transform,
    pub(crate) curve: Option<TextCurve>,
    pub(crate) carrier_available: bool,
}

pub(crate) struct PcurveUse {
    pub(crate) shape: usize,
    pub(crate) representation: usize,
    pub(crate) curve: usize,
    pub(crate) secondary: bool,
}

pub(crate) struct PcurveInputs {
    pub(crate) curve: TextCurve2d,
    pub(crate) parameter_range: [FiniteReal; 2],
    pub(crate) affine: Option<SurfaceParameterAffine>,
}

impl<'a> ProjectionSources<'a> {
    pub(super) fn insert(
        &mut self,
        payload: &ShapePayloadRecord,
        source: View<'a>,
        binary: bool,
        marks: RowMarks,
    ) {
        self.payloads.insert(
            payload.id.clone(),
            PayloadSource {
                source,
                binary,
                marks,
            },
        );
    }

    /// Construct a Pcurve from actual independent getters. The primary curve,
    /// required topology range/indices, and surface chart all feed its value.
    pub(crate) fn produce<T>(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
        payload: &ShapePayloadRecord,
        usage: PcurveUse,
        build: impl FnOnce(PcurveInputs) -> Result<Option<T>, CodecError>,
    ) -> Result<Option<T>, CodecError> {
        let source = self.payloads.get(&payload.id).ok_or_else(|| {
            CodecError::malformed("Pcurve payload has no original source authority")
        })?;
        let curve_index = payload.source_index(1, usage.curve);
        let primary = source.row(1, curve_index)?;
        let (curve, owner) =
            ctx.record_owner(id, primary, || source.curve(primary, curve_index))?;
        let shape = payload.source_index(7, usage.shape);
        let range = source
            .marks
            .pcurves
            .get(&(shape, usage.representation))
            .ok_or_else(|| {
                CodecError::malformed("Pcurve has no accepted topology representation source")
            })?;
        let representation = window(source.source, range.start, range.end)?;
        let (reference, surface, parameter_range) =
            ctx.record_fragment(&owner, representation, || {
                source.representation(representation, usage.secondary)
            })?;
        if reference != curve_index {
            return Err(CodecError::malformed(
                "Pcurve source reference differs from its actual curve home",
            ));
        }
        let surface = source.row(5, surface)?;
        let affine = ctx.record_fragment(&owner, surface, || {
            source
                .surface(surface)
                .map(|surface| surface_parameter_affine(&surface))
        })?;
        if source.marks.root.is_none() {
            ctx.retire_record_owner(&owner)?;
            return Ok(None);
        }

        let admitted = ctx.record_fragment(&owner, source.source, || {
            if !source.reaches(payload, usage.shape)? {
                return Ok(false);
            }
            source.edge_admitted(payload, usage.shape)
        })?;
        if !admitted {
            ctx.retire_record_owner(&owner)?;
            return Ok(None);
        }
        let result = build(PcurveInputs {
            curve,
            parameter_range,
            affine: Some(affine),
        })?;
        if result.is_none() {
            ctx.retire_record_owner(&owner)?;
        }
        Ok(result)
    }
}

impl PayloadSource<'_> {
    fn row(&self, table: u8, index: usize) -> Result<View<'_>, CodecError> {
        let range = self
            .marks
            .rows
            .get(&(table, index))
            .ok_or_else(|| CodecError::malformed("Pcurve requires an unaccepted source row"))?;
        window(self.source, range.start, range.end)
    }
    fn curve(&self, source: View<'_>, index: usize) -> Result<TextCurve2d, CodecError> {
        if self.binary {
            parse_binary_curve2d(
                &mut BinaryCursor {
                    view: source,
                    census_marks: vec![],
                    representation_marks: vec![],
                },
                0,
            )
        } else {
            let (tokens, ranges) = text_tokens(source);
            parse_curve2d(&mut cursor(&tokens, &ranges, source), 0, index)
        }
    }
    fn surface(&self, source: View<'_>) -> Result<TextSurface, CodecError> {
        if self.binary {
            parse_binary_surface(
                &mut BinaryCursor {
                    view: source,
                    census_marks: vec![],
                    representation_marks: vec![],
                },
                0,
            )
        } else {
            let (tokens, ranges) = text_tokens(source);
            parse_surface(&mut cursor(&tokens, &ranges, source), 0, 1)
        }
    }
    fn representation(
        &self,
        source: View<'_>,
        secondary: bool,
    ) -> Result<(usize, usize, [FiniteReal; 2]), CodecError> {
        if self.binary {
            let mut cursor = BinaryCursor {
                view: source,
                census_marks: vec![],
                representation_marks: vec![],
            };
            let kind = cursor.u8("Pcurve representation kind")?;
            if !matches!(kind, 2 | 3) {
                return Err(CodecError::malformed(
                    "Pcurve source is not a Pcurve representation",
                ));
            }
            let first = positive(cursor.i32("Pcurve curve reference")?)?;
            let second = if kind == 3 {
                let value = positive(cursor.i32("Pcurve second curve reference")?)?;
                cursor.view = window(source, cursor.view.position() + 1, source.end())?;
                Some(value)
            } else {
                None
            };
            let curve = if secondary {
                second.ok_or_else(|| CodecError::malformed("Pcurve source has no second curve"))?
            } else {
                first
            };
            let surface = positive(cursor.i32("Pcurve surface reference")?)?;
            // The location and continuity do not change the chart coordinates.
            // Fixed-width navigation locates the real range getters below.
            cursor.view = window(source, cursor.view.position() + 4, source.end())?;
            let range = [
                cursor.finite_f64("Pcurve range start")?,
                cursor.finite_f64("Pcurve range end")?,
            ];
            Ok((curve, surface, range))
        } else {
            let (tokens, ranges) = text_tokens(source);
            let mut cursor = cursor(&tokens, &ranges, source);
            let kind = cursor.integer("Pcurve representation kind")?;
            if !matches!(kind, 2 | 3) {
                return Err(CodecError::malformed(
                    "Pcurve source is not a Pcurve representation",
                ));
            }
            let first = parse_reference(&mut cursor, "Pcurve curve reference", None, false)?;
            let second = if kind == 3 {
                let (value, continuity) =
                    parse_reference_suffix(&mut cursor, "Pcurve second curve reference", None)?;
                if continuity.is_none() {
                    cursor.index += 1;
                }
                Some(value)
            } else {
                None
            };
            let curve = if secondary {
                second.ok_or_else(|| CodecError::malformed("Pcurve source has no second curve"))?
            } else {
                first
            };
            let surface = parse_reference(&mut cursor, "Pcurve surface reference", None, false)?;
            cursor.index += 1;
            Ok((curve, surface, parse_range(&mut cursor, "Pcurve range")?))
        }
    }
}
fn positive(value: i32) -> Result<usize, CodecError> {
    usize::try_from(value)
        .ok()
        .filter(|value| *value != 0)
        .ok_or_else(|| CodecError::malformed("Pcurve requires a positive source reference"))
}

#[cfg(test)]
mod tests;
