// SPDX-License-Identifier: Apache-2.0
//! Geometry-only streams framed entirely by compact analytic records.

use super::{admit_brep_entity, emit_curve, id_surf, retain_surface_geometry, Brep};
use crate::brep::{parse_carrier, Carrier};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::geometry::Surface;

pub(super) fn decode(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    stream: &cadmpeg_ir::StreamName,
) -> Result<Option<Brep>, cadmpeg_core::CodecError> {
    let mut offset = 0;
    let mut count = 0usize;
    while offset < body.len() {
        let Some(carrier) = parse_carrier(body, offset) else {
            return Ok(None);
        };
        offset = match carrier {
            Carrier::Curve(curve) => curve.end,
            Carrier::Surface(surface) => surface.end,
        };
        count += 1;
    }
    if count == 0 {
        return Ok(None);
    }
    let mut identities = Vec::new();
    ctx.reserve_working(
        &mut identities,
        count,
        "check standalone carrier identities",
    )?;
    offset = 0;
    while offset < body.len() {
        let Some(carrier) = parse_carrier(body, offset) else {
            return Ok(None);
        };
        let (attr, end) = match carrier {
            Carrier::Curve(curve) => (curve.attr, curve.end),
            Carrier::Surface(surface) => (surface.attr, surface.end),
        };
        identities.push(attr);
        offset = end;
    }
    identities.sort_unstable();
    if identities.windows(2).any(|pair| pair[0] == pair[1]) {
        return Ok(None);
    }
    let mut out = Brep::default();
    let mut annotations = AnnotationBuilder::new();
    let stream = StreamHandle::new(stream.clone());
    offset = 0;
    while offset < body.len() {
        let Some(carrier) = parse_carrier(body, offset) else {
            return Ok(None);
        };
        match carrier {
            Carrier::Curve(curve) => {
                offset = curve.end;
                let id = super::id_curve(curve.attr);
                annotations.note(
                    id.as_str(),
                    &stream,
                    u64_from_index(curve.offset),
                    Some("compact_curve"),
                );
                emit_curve(ctx, &mut out, &curve)?;
            }
            Carrier::Surface(surface) => {
                offset = surface.end;
                let id = id_surf(surface.attr);
                annotations.note(
                    id.as_str(),
                    &stream,
                    u64_from_index(surface.offset),
                    Some("compact_surface"),
                );
                admit_brep_entity(ctx)?;
                out.surfaces.push(Surface {
                    native_ref: None,
                    id,
                    source_object: None,
                    geometry: retain_surface_geometry(ctx, &surface.geometry)?,
                });
            }
        }
    }
    out.annotations = annotations.build();
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    #[test]
    fn requires_complete_records_and_unique_identities() -> Result<(), cadmpeg_core::CodecError> {
        let line =
            crate::test_support::parasolid::line_carrier(2, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::desktop();
        let (ctx, _) = DecodeContext::from_root_bytes(&line, &arena, &policy)?;
        let stream = cadmpeg_ir::stream_name!("standalone");
        assert!(super::decode(&ctx, &line, &stream)?.is_some());
        assert!(super::decode(&ctx, &line[..line.len() - 1], &stream)?.is_none());
        let duplicate = [line.as_slice(), line.as_slice()].concat();
        assert!(super::decode(&ctx, &duplicate, &stream)?.is_none());
        Ok(())
    }
}
