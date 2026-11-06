// SPDX-License-Identifier: Apache-2.0
//! Geometry-only streams framed by compact analytic and point records.

use super::{admit_brep_entity, emit_curve, id_surf, retain_surface_geometry, Brep};
use crate::brep::{parse_carrier, topology, Carrier, CurveCarrier, SurfaceCarrier, LEN_TO_MM};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::geometry::Surface;
use cadmpeg_ir::topology::Point;

enum Record {
    Curve(CurveCarrier),
    Surface(SurfaceCarrier),
    Point(topology::Point, usize),
}

impl Record {
    fn identity(&self) -> u32 {
        match self {
            Self::Curve(curve) => curve.attr,
            Self::Surface(surface) => surface.attr,
            Self::Point(point, _) => point.attr,
        }
    }

    fn end(&self) -> usize {
        match self {
            Self::Curve(curve) => curve.end,
            Self::Surface(surface) => surface.end,
            Self::Point(_, end) => *end,
        }
    }
}

fn record(body: &[u8], offset: usize) -> Option<Record> {
    if body.get(offset..offset.checked_add(2)?) == Some(&[0, 0x1d]) {
        let point = topology::parse_point(body, offset, false)?;
        let end = point
            .xyz_offset
            .checked_add(std::mem::size_of::<[f64; 3]>())?;
        body.get(offset..end)?;
        Some(Record::Point(point, end))
    } else {
        match parse_carrier(body, offset)? {
            Carrier::Curve(curve) => Some(Record::Curve(curve)),
            Carrier::Surface(surface) => Some(Record::Surface(surface)),
        }
    }
}

pub(super) fn decode(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    stream: &cadmpeg_ir::StreamName,
) -> Result<Option<Brep>, cadmpeg_core::CodecError> {
    let mut offset = 0;
    let mut count = 0usize;
    while offset < body.len() {
        let Some(carrier) = record(body, offset) else {
            return Ok(None);
        };
        offset = carrier.end();
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
        let Some(carrier) = record(body, offset) else {
            return Ok(None);
        };
        identities.push(carrier.identity());
        offset = carrier.end();
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
        let Some(carrier) = record(body, offset) else {
            return Ok(None);
        };
        match carrier {
            Record::Curve(curve) => {
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
            Record::Surface(surface) => {
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
            Record::Point(point, end) => {
                offset = end;
                let id = super::id_point(point.attr);
                annotations.note(
                    id.as_str(),
                    &stream,
                    u64_from_index(point.offset),
                    Some("00_1d"),
                );
                let [x, y, z] = point.xyz_m;
                let position = cadmpeg_ir::features::FinitePoint3::new(
                    cadmpeg_ir::math::Point3::new(x * LEN_TO_MM, y * LEN_TO_MM, z * LEN_TO_MM),
                )
                .ok_or(Point::NON_FINITE_POSITION)
                .map_err(cadmpeg_core::CodecError::malformed)?;
                admit_brep_entity(ctx)?;
                out.points.push(Point::new(id, position, None));
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
        let point = crate::test_support::parasolid::world_point(3, [1.0, 2.0, 3.0]);
        let mixed = [line.as_slice(), point.as_slice()].concat();
        assert!(super::decode(&ctx, &mixed, &stream)?.is_some_and(|brep| brep.points.len() == 1));
        assert!(super::decode(&ctx, &mixed[..mixed.len() - 1], &stream)?.is_none());
        Ok(())
    }
}
