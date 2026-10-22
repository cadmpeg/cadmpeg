// SPDX-License-Identifier: Apache-2.0
//! Source reads of parameters retained by compound curve definitions.

use std::ops::Range;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::{CurveId, Identity, IdentityKey, ProceduralCurveId};

use crate::chunks::{ArchiveVersion, BoundedReader, FramingError};
use crate::settings::MillimeterScale;
use crate::wire::Uuid;

use super::{DecodedCurve, DecodedGeometry, GeometryError};

pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    data: &[u8],
    range: Range<usize>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
    key: &IdentityKey,
) -> Result<DecodedGeometry, GeometryError> {
    if !source.has_source_tracking() {
        return super::decode(ctx, data, super::POLYCURVE, range, scale, archive);
    }
    let reads = ctx.read_transaction()?;
    let result = Source {
        ctx,
        source,
        data,
        scale,
        archive,
        key,
    }
    .decode(super::POLYCURVE, range, "root", 0)?;
    reads.commit()?;
    Ok(result)
}

struct Source<'ctx, 'arena, 'bytes> {
    ctx: &'ctx DecodeContext<'arena>,
    source: View<'bytes>,
    data: &'bytes [u8],
    scale: MillimeterScale,
    archive: ArchiveVersion,
    key: &'ctx IdentityKey,
}

impl Source<'_, '_, '_> {
    fn decode(
        &self,
        class: Uuid,
        range: Range<usize>,
        path: &str,
        depth: usize,
    ) -> Result<DecodedGeometry, GeometryError> {
        let leaf = super::is_line(class) || super::is_polyline(class);
        if !super::is_polycurve(class) && !leaf {
            return super::decode_inner(
                self.ctx,
                self.data,
                class,
                range,
                self.scale,
                self.archive,
                depth,
            );
        }
        let _nested = self.ctx.enter_nested("Rhino curve tree")?;
        if depth > super::MAX_CURVE_DEPTH {
            return Err(GeometryError::malformed(
                range.start,
                "curve recursion limit exceeded",
            ));
        }
        let key = if path == "root" {
            self.key.clone()
        } else {
            self.key.clone().then(cadmpeg_ir::identity_key!(".")).then(
                IdentityKey::try_new(path.to_owned())
                    .map_err(|error| GeometryError::malformed(range.start, error.to_string()))?,
            )
        };
        let id: Identity = if leaf {
            CurveId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "curve"),
                key,
            )
            .into()
        } else {
            ProceduralCurveId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "procedural-curve"),
                key,
            )
            .into()
        };
        let extent =
            self.source
                .child(range.start, range.end)
                .ok_or(FramingError::OutOfBounds {
                    offset: range.start,
                    end: range.end,
                    bound: self.source.end(),
                })?;
        self.ctx.record(id.as_str(), extent, || {
            Ok(if leaf {
                self.read_leaf(class, range)
            } else {
                self.read(range, path, depth)
            })
        })?
    }

    fn read_leaf(
        &self,
        class: Uuid,
        range: Range<usize>,
    ) -> Result<DecodedGeometry, GeometryError> {
        let mut reader = BoundedReader::from_view(self.source, range.start, range.end)?;
        let curve = if super::is_line(class) {
            DecodedCurve::line(super::read_line(&mut reader, self.scale, None)?)
        } else {
            DecodedCurve::leaf(
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(super::read_polyline(
                    self.ctx,
                    &mut reader,
                    self.scale,
                    None,
                )?)),
                crate::loss::Diagnostics::new(),
            )
        };
        reader.skip_remaining()?;
        Ok(DecodedGeometry::Curve { curve })
    }

    fn read(
        &self,
        range: Range<usize>,
        path: &str,
        depth: usize,
    ) -> Result<DecodedGeometry, GeometryError> {
        let mut reader = BoundedReader::from_view(self.source, range.start, range.end)?;
        let curve = super::read_polycurve_with(
            self.data,
            &mut reader,
            self.archive,
            |reader, count| super::read_polycurve_parameters(self.ctx, reader, count, "polycurve"),
            |index, class, range| {
                self.decode(
                    class,
                    range,
                    &format!("{path}.component-{index}"),
                    depth + 1,
                )
            },
        )?;
        reader.skip_remaining()?;
        Ok(DecodedGeometry::Curve { curve })
    }
}

#[cfg(test)]
mod tests;
