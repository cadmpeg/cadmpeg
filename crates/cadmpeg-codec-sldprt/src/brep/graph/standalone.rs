// SPDX-License-Identifier: Apache-2.0
//! Geometry-only streams framed by analytic, spline and point records.

use super::{admit_brep_entity, emit_curve, id_surf, retain_surface_geometry, Brep};
use crate::brep::{
    parse_carrier, spline, topology, Carrier, CurveCarrier, SurfaceCarrier, LEN_TO_MM,
};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::geometry::Surface;
use cadmpeg_ir::topology::Point;
use std::collections::HashSet;

enum Record {
    Curve(CurveCarrier),
    Surface(SurfaceCarrier),
    Point(topology::Point, usize),
    Spline(usize),
    Auxiliary(usize),
}

impl Record {
    fn identity(&self) -> Option<u32> {
        match self {
            Self::Curve(curve) => Some(curve.attr),
            Self::Surface(surface) => Some(surface.attr),
            Self::Point(point, _) => Some(point.attr),
            Self::Spline(_) | Self::Auxiliary(_) => None,
        }
    }

    fn end(&self) -> usize {
        match self {
            Self::Curve(curve) => curve.end,
            Self::Surface(surface) => surface.end,
            Self::Point(_, end) | Self::Spline(end) | Self::Auxiliary(end) => *end,
        }
    }
}

fn record(body: &[u8], offset: usize, arrays: &HashSet<u32>) -> Option<Record> {
    if body.get(offset..offset.checked_add(2)?) == Some(&[0, 0x1d]) {
        let point = topology::parse_point(body, offset, false)?;
        let end = point
            .xyz_offset
            .checked_add(std::mem::size_of::<[f64; 3]>())?;
        body.get(offset..end)?;
        Some(Record::Point(point, end))
    } else if body.get(offset..offset.checked_add(2)?) == Some(&[0, 0x86]) {
        let mut end = spline::curve_prefix_end(body, offset)?;
        cadmpeg_parasolid::pointer::read_at(body, &mut end)?;
        Some(Record::Spline(end))
    } else if body.get(offset..offset.checked_add(2)?) == Some(&[0, 0x88]) {
        Some(Record::Auxiliary(spline::curve_prefix_end(body, offset)?))
    } else if matches!(
        body.get(offset..offset.checked_add(2)?),
        Some([0, 0x2d | 0x7f | 0x80])
    ) {
        Some(Record::Auxiliary(spline::array_record_end(
            body, offset, arrays,
        )?))
    } else {
        match parse_carrier(body, offset)? {
            Carrier::Curve(curve) => Some(Record::Curve(curve)),
            Carrier::Surface(surface) => Some(Record::Surface(surface)),
        }
    }
}

fn has_top_level_spline(body: &[u8]) -> bool {
    let mut offset = 0;
    let arrays = HashSet::new();
    while offset < body.len() {
        if body
            .get(offset..)
            .is_some_and(|rest| rest.starts_with(&[0, 0x86]))
        {
            return true;
        }
        match record(body, offset, &arrays) {
            Some(carrier @ (Record::Curve(_) | Record::Surface(_) | Record::Point(_, _))) => {
                offset = carrier.end();
            }
            _ => return false,
        }
    }
    false
}

pub(super) fn decode(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    stream: &cadmpeg_ir::StreamName,
) -> Result<Option<Brep>, cadmpeg_core::CodecError> {
    let has_splines = has_top_level_spline(body);
    let arrays = if has_splines {
        spline::referenced_array_attrs(body)
    } else {
        HashSet::new()
    };
    let mut offset = 0;
    let mut count = 0usize;
    let mut spline_count = 0usize;
    while offset < body.len() {
        let Some(carrier) = record(body, offset, &arrays) else {
            return Ok(None);
        };
        if matches!(carrier, Record::Spline(_)) {
            spline_count += 1;
        }
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
    let mut spline_offsets = Vec::new();
    ctx.reserve_working(
        &mut spline_offsets,
        spline_count,
        "index standalone spline offsets",
    )?;
    offset = 0;
    while offset < body.len() {
        let Some(carrier) = record(body, offset, &arrays) else {
            return Ok(None);
        };
        if let Some(identity) = carrier.identity() {
            identities.push(identity);
        } else if matches!(carrier, Record::Spline(_)) {
            spline_offsets.push(offset);
        }
        offset = carrier.end();
    }
    let mut refusals = Vec::new();
    let indexed = if has_splines {
        spline::scan_curve_carriers_at(body, &mut refusals, |offset| {
            spline_offsets.binary_search(&offset).is_ok()
        })
    } else {
        Default::default()
    };
    if !refusals.is_empty() || indexed.len() != spline_offsets.len() {
        return Ok(None);
    }
    let mut splines = Vec::new();
    ctx.reserve_working(&mut splines, indexed.len(), "index standalone splines")?;
    splines.extend(indexed.into_values());
    splines.sort_by_key(|curve| curve.offset);
    identities.extend(splines.iter().map(|curve| curve.attr));
    identities.sort_unstable();
    if identities.is_empty() || identities.windows(2).any(|pair| pair[0] == pair[1]) {
        return Ok(None);
    }
    let mut out = Brep::default();
    let mut annotations = AnnotationBuilder::new();
    let stream = StreamHandle::new(stream.clone());
    offset = 0;
    while offset < body.len() {
        let Some(carrier) = record(body, offset, &arrays) else {
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
            Record::Spline(end) => {
                let Ok(index) = splines.binary_search_by_key(&offset, |curve| curve.offset) else {
                    return Ok(None);
                };
                let curve = &splines[index];
                let id = super::id_curve(curve.attr);
                annotations.note(
                    id.as_str(),
                    &stream,
                    u64_from_index(curve.offset),
                    Some("nurbs_curve"),
                );
                emit_curve(ctx, &mut out, curve)?;
                offset = end;
            }
            Record::Auxiliary(end) => offset = end,
        }
    }
    out.annotations = annotations.build();
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    #[test]
    fn written_quadratic_rational_curve_is_decoded() {
        use cadmpeg_ir::codec::{Codec, DecodeOptions};
        use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3};
        use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
        use cadmpeg_ir::ids::CurveId;
        use cadmpeg_ir::math::Point3;

        let poles = NurbsPoles3::from_lanes(
            vec![
                Point3::new(2.0, 0.0, 0.0),
                Point3::new(2.0, 2.0, 0.0),
                Point3::new(0.0, 2.0, 0.0),
            ],
            Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]),
        )
        .unwrap();
        let curve = NurbsCurve::new(2, vec![4.0, 4.0, 4.0, 9.0, 9.0, 9.0], poles, false).unwrap();
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model.curves.push(Curve {
            native_ref: None,
            id: CurveId::mint("test:model:curve#quadratic").unwrap(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
            parameter_range: None,
            source_object: None,
        });
        let body = crate::writer::brep_body(&ir, 0.001, false).unwrap();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::desktop();
        let (ctx, _) = DecodeContext::from_root_bytes(&body, &arena, &policy).unwrap();
        let decoded = super::decode(&ctx, &body, &cadmpeg_ir::stream_name!("standalone")).unwrap();
        assert!(decoded.is_some_and(|brep| brep.curves.len() == 1));
        let source = crate::test_support::container::sldprt_with_body(&body);
        let decoded = crate::SldprtCodec
            .decode(&mut std::io::Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        assert_eq!(decoded.ir().model.curves.len(), 1);
    }

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
        let incidental = crate::test_support::parasolid::line_carrier(
            4,
            [f64::from_bits(0x3ff0_0086_0000_0000), 0.0, 0.0],
            [1.0, 0.0, 0.0],
        );
        assert!(!super::has_top_level_spline(&incidental));
        assert!(super::decode(&ctx, &incidental, &stream)?.is_some());
        let point = crate::test_support::parasolid::world_point(3, [1.0, 2.0, 3.0]);
        let mixed = [line.as_slice(), point.as_slice()].concat();
        assert!(super::decode(&ctx, &mixed, &stream)?.is_some_and(|brep| brep.points.len() == 1));
        assert!(super::decode(&ctx, &mixed[..mixed.len() - 1], &stream)?.is_none());
        for spline in [
            crate::test_support::parasolid::linear_nurbs_curve_carrier(10, 20),
            crate::test_support::parasolid::rational_linear_nurbs_curve_carrier(10, 20),
        ] {
            assert!(super::decode(&ctx, &spline, &stream)?.is_some_and(|brep| {
                matches!(brep.curves.as_slice(), [curve] if matches!(curve.geometry, cadmpeg_ir::geometry::CurveGeometry::Solved(cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(_))))
            }));
            assert!(super::decode(&ctx, &spline[..spline.len() - 1], &stream)?.is_none());
        }
        Ok(())
    }
}
