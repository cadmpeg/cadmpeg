// SPDX-License-Identifier: Apache-2.0
//! Actual triangulation lanes and face-use admission feed mesh producers.
use super::*;

impl ProjectionSources<'_> {
    pub(crate) fn triangulation<T>(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
        payload: &ShapePayloadRecord,
        dense: usize,
        face: Option<(usize, Transform, Option<usize>)>,
        build: impl FnOnce(TextTriangulation, Transform) -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        self.triangulation_owned(ctx, id, payload, dense, face, build)
            .map(|(value, _)| value)
    }
    pub(crate) fn unowned_triangulation<T>(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
        payload: &ShapePayloadRecord,
        dense: usize,
        build: impl FnOnce(TextTriangulation, Transform) -> Result<T, CodecError>,
    ) -> Result<Option<T>, CodecError> {
        let (value, owner) =
            self.triangulation_owned(ctx, id, payload, dense, None, |value, matrix| {
                if value.nodes().is_empty() && value.triangles().is_empty() {
                    Ok(None)
                } else {
                    build(value, matrix).map(Some)
                }
            })?;
        if value.is_none() {
            ctx.retire_record_owner(&owner)?;
        }
        Ok(value)
    }
    fn triangulation_owned<'session, T>(
        &self,
        ctx: &DecodeContext<'session>,
        id: &str,
        payload: &ShapePayloadRecord,
        dense: usize,
        face: Option<(usize, Transform, Option<usize>)>,
        build: impl FnOnce(TextTriangulation, Transform) -> Result<T, CodecError>,
    ) -> Result<(T, cadmpeg_core::decode::RecordOwner<'session>), CodecError> {
        let source = self.payloads.get(&payload.id).ok_or_else(|| {
            CodecError::malformed("triangulation payload has no original source authority")
        })?;
        let extent = source.row(6, payload.source_index(6, dense))?;
        ctx.record_owner(id, extent, || {
            let version = payload.payload.topology_version().ok_or_else(|| {
                CodecError::malformed("triangulation source has no topology version")
            })?;
            let value = if source.binary {
                source_rows::binary_triangulation(
                    &mut BinaryCursor {
                        view: extent,
                        census_marks: vec![],
                        representation_marks: vec![],
                    },
                    version,
                )?
            } else {
                let (tokens, ranges) = text_tokens(extent);
                source_rows::text_triangulation(&mut cursor(&tokens, &ranges, extent), version)?
            };
            let matrix = if let Some((face, expected, ordinal)) = face {
                let shape = source.shape(payload, face)?;
                if !matches!(shape.geometry, TextTShapeGeometry::Face { triangulation: Some(index), .. } if index.index() == dense)
                    || !source.reaches(payload, face)?
                {
                    return Err(CodecError::malformed("face source does not admit this triangulation"));
                }
                source.placement(payload, face, expected, ordinal)?
            } else { Transform::identity() };
            build(value, matrix)
        })
    }
}
