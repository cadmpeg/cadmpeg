// SPDX-License-Identifier: Apache-2.0
//! Placed carriers use original geometry and the actual topology use that admits them.
use super::*;

impl ProjectionSources<'_> {
    pub(crate) fn placed_curve<T>(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
        payload: &ShapePayloadRecord,
        dense: usize,
        edge: usize,
        expected: Transform,
        ordinal: Option<usize>,
        build: impl FnOnce(TextCurve, Transform) -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        let source = self.payloads.get(&payload.id).ok_or_else(|| {
            CodecError::malformed("placed curve has no original source authority")
        })?;
        let index = payload.source_index(2, dense);
        let view = source.row(2, index)?;
        ctx.record(id, view, || {
            let value = if source.binary {
                parse_binary_curve(
                    &mut BinaryCursor {
                        view,
                        census_marks: vec![],
                        representation_marks: vec![],
                    },
                    0,
                )?
            } else {
                let (tokens, ranges) = text_tokens(view);
                parse_curve(&mut cursor(&tokens, &ranges, view), 0, index)?
            };
            let shape = source.shape(payload, edge)?;
            let TextTShapeGeometry::Edge {
                representations, ..
            } = shape.geometry
            else {
                return Err(CodecError::malformed(
                    "placed curve requires an edge source",
                ));
            };
            let mut uses = representations.iter().filter_map(|representation| {
                if let TextEdgeRepresentation::Curve3d {
                    curve, location, ..
                } = representation
                {
                    (*curve == dense).then_some(*location)
                } else {
                    None
                }
            });
            let location = uses.next().ok_or_else(|| {
                CodecError::malformed("edge source does not admit this placed curve")
            })?;
            if uses.next().is_some() {
                return Err(CodecError::malformed(
                    "edge has ambiguous placed curve uses",
                ));
            }
            let matrix = source.placement_local(payload, edge, expected, ordinal, location)?;
            build(value, matrix)
        })
    }
    pub(crate) fn placed_surface<T>(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
        payload: &ShapePayloadRecord,
        dense: usize,
        face: usize,
        expected: Transform,
        ordinal: Option<usize>,
        build: impl FnOnce(TextSurface, Transform) -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        let source = self.payloads.get(&payload.id).ok_or_else(|| {
            CodecError::malformed("placed surface has no original source authority")
        })?;
        let view = source.row(5, payload.source_index(5, dense))?;
        ctx.record(id, view, || {
            let value = source.surface(view)?;
            let shape = source.shape(payload, face)?;
            let TextTShapeGeometry::Face {
                surface: Some(index),
                location,
                ..
            } = shape.geometry
            else {
                return Err(CodecError::malformed(
                    "placed surface requires a face source",
                ));
            };
            if index.index() != dense {
                return Err(CodecError::malformed(
                    "face source does not admit this placed surface",
                ));
            }
            let matrix =
                source.placement_local(payload, face, expected, ordinal, location.index())?;
            build(value, matrix)
        })
    }
}
