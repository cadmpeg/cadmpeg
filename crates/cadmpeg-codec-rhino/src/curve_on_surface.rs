// SPDX-License-Identifier: Apache-2.0
//! Curve-on-surface construction decoding.

use crate::loss::Diagnostics;
use cadmpeg_core::decode::DecodeContext;
use std::ops::Range;

use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader};
use crate::curves::{DecodedCurve, DecodedGeometry, GeometryError};
use crate::objects::parse_class_wrapper;
use crate::settings::MillimeterScale;
use crate::surfaces::DecodedSurface;
use crate::wire::Uuid;

pub(crate) const CLASS: Uuid = Uuid::from_canonical([
    0x4e, 0xd7, 0xd4, 0xd8, 0xe9, 0x47, 0x11, 0xd3, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);

#[derive(Debug, Clone)]
pub(crate) struct CurveOnSurface {
    pub(crate) source_range: Range<usize>,
    pub(crate) parameter_curve: DecodedCurve,
    pub(crate) model_curve: Option<DecodedCurve>,
    pub(crate) surface: DecodedSurface,
    pub(crate) warnings: Diagnostics,
}

fn class(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Diagnostics,
) -> Result<crate::objects::ClassDescriptor, GeometryError> {
    let start = reader.position();
    let wrapper = chunk_at(data, start, reader.end(), archive, false)?;
    let class = parse_class_wrapper(ctx, data, start..wrapper.next_offset(), archive, warnings)?;
    reader.skip(wrapper.next_offset() - start)?;
    Ok(class)
}

pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    range: Range<usize>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<CurveOnSurface, GeometryError> {
    decode_components(ctx, data, range, scale, archive, depth, None)
}

/// Decodes the model carrier with temporary parameter and support components.
pub(crate) fn decode_model_curve(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    range: Range<usize>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<DecodedCurve, GeometryError> {
    let mut discarded_storage =
        ctx.reserve_scoped(0, "Rhino curve-on-surface discarded components")?;
    let construction = decode_components(
        ctx,
        data,
        range,
        scale,
        archive,
        depth,
        Some(&mut discarded_storage),
    )?;
    let CurveOnSurface {
        source_range,
        parameter_curve,
        model_curve,
        surface,
        warnings,
    } = construction;
    drop(parameter_curve);
    drop(surface);
    drop(discarded_storage);
    let Some(mut curve) = model_curve else {
        return Err(GeometryError::unsupported(
            source_range.start,
            "curve-on-surface has no stored model-space carrier",
        ));
    };
    curve.warnings_mut().prepend_admitted(ctx, warnings)?;
    Ok(curve)
}

fn decode_components(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    range: Range<usize>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
    depth: usize,
    mut discarded_storage: Option<&mut cadmpeg_core::decode::ScopedReservation<'_>>,
) -> Result<CurveOnSurface, GeometryError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let mut warnings = Diagnostics::new();
    let c2 = class(ctx, data, &mut reader, archive, &mut warnings)?;
    let decode_parameter = || {
        crate::curves::decode_inner_2d(
            ctx,
            data,
            c2.class_uuid,
            c2.class_data_range,
            archive,
            depth,
        )
    };
    let decoded = match discarded_storage.as_deref_mut() {
        Some(storage) => storage.with_storage(decode_parameter)?,
        None => decode_parameter()?,
    };
    let DecodedGeometry::Curve {
        curve: parameter_curve,
    } = decoded
    else {
        return Err(GeometryError::malformed(
            range.start,
            "curve-on-surface C2 object is not a curve",
        ));
    };
    let has_model_curve = match reader.i32()? {
        0 => false,
        1 => true,
        _ => {
            return Err(GeometryError::malformed(
                reader.position() - 4,
                "invalid curve-on-surface C3 presence",
            ))
        }
    };
    let model_curve = if has_model_curve {
        let c3 = class(ctx, data, &mut reader, archive, &mut warnings)?;
        if c3.class_uuid == CLASS {
            return Err(GeometryError::malformed(
                reader.position(),
                "nested curve-on-surface C3 carrier is invalid",
            ));
        }
        let decoded = crate::curves::decode_inner(
            ctx,
            data,
            c3.class_uuid,
            c3.class_data_range,
            scale,
            archive,
            depth,
        )?;
        let DecodedGeometry::Curve { curve } = decoded else {
            return Err(GeometryError::malformed(
                reader.position(),
                "curve-on-surface C3 object is not a curve",
            ));
        };
        Some(curve)
    } else {
        None
    };
    let support = class(ctx, data, &mut reader, archive, &mut warnings)?;
    if !crate::curves::surface_class(support.class_uuid) {
        return Err(GeometryError::malformed(
            reader.position(),
            "curve-on-surface support is not a surface",
        ));
    }
    let decode_support = || {
        crate::surfaces::decode(
            ctx,
            data,
            support.class_uuid,
            support.class_data_range,
            scale,
            archive,
            depth,
        )
    };
    let surface = match discarded_storage {
        Some(storage) => storage.with_storage(decode_support)?,
        None => decode_support()?,
    };
    reader.skip_remaining()?;
    Ok(CurveOnSurface {
        source_range: range,
        parameter_curve,
        model_curve,
        surface,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use crate::chunks::ArchiveVersion;
    use crate::surfaces::DecodedSurface;
    use crate::test_support::test_archive::{
        class_wrapper, line_payload, polyline_payload, LINE_CLASS, POLYLINE_CLASS,
    };
    use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};

    fn decode(
        data: &[u8],
        range: std::ops::Range<usize>,
        scale: crate::settings::MillimeterScale,
        archive: ArchiveVersion,
        depth: usize,
    ) -> Result<super::CurveOnSurface, crate::curves::GeometryError> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
            .expect("test input fits service profile");
        super::decode(&ctx, data, range, scale, archive, depth)
    }

    const PLANE_SURFACE: [u8; 16] = [
        0xdf, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ];

    fn plane_surface() -> Vec<u8> {
        let mut payload = vec![0x10];
        payload.extend(
            [
                1.0_f64, 2.0, 3.0, // origin
                1.0, 0.0, 0.0, // x axis
                0.0, 1.0, 0.0, // y axis
                0.0, 0.0, 1.0, // z axis
                0.0, 0.0, 1.0, -3.0, // equation
                0.0, 1.0, // U extent
                0.0, 1.0, // V extent
            ]
            .into_iter()
            .flat_map(f64::to_le_bytes),
        );
        payload
    }

    #[test]
    fn decodes_parameter_model_and_support_carriers() {
        let mut c2 = polyline_payload(&[[0.0, 0.0, 0.0], [1.0, 1.0, 0.0]], &[0.0, 1.0]);
        let end = c2.len();
        c2[end - 4..].copy_from_slice(&2_i32.to_le_bytes());
        let mut bytes = class_wrapper(POLYLINE_CLASS, &c2);
        bytes.extend(1_i32.to_le_bytes());
        bytes.extend(class_wrapper(
            LINE_CLASS,
            &line_payload([0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 1.0]),
        ));
        bytes.extend(class_wrapper(PLANE_SURFACE, &plane_surface()));

        let decoded = decode(
            &bytes,
            0..bytes.len(),
            crate::test_support::millimeter_scale(10.0),
            ArchiveVersion::V8,
            0,
        )
        .expect("required invariant");
        assert!(decoded.model_curve.is_some());
        let crate::curves::DecodedCurve::Leaf {
            geometry: cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(c2)),
            ..
        } = decoded.parameter_curve
        else {
            panic!("expected NURBS parameter curve");
        };
        assert_eq!(c2.control_points()[1].x, 1.0);
        let Some(crate::curves::DecodedCurve::Leaf {
            geometry:
                cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(model_curve)),
            ..
        }) = decoded.model_curve
        else {
            panic!("expected NURBS model curve");
        };
        assert_eq!(model_curve.control_points()[1].x, 20.0);
        let DecodedSurface::Typed { geometry, .. } = decoded.surface else {
            panic!("expected typed support surface");
        };
        let cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            plane_surface,
        )) = geometry.into_geometry()
        else {
            panic!("expected plane support surface");
        };
        let origin = plane_surface.origin().get();
        assert_eq!(origin.x, 10.0);
        assert_eq!(origin.y, 20.0);
        assert_eq!(origin.z, 30.0);
    }
    fn large_carrier_payload() -> Vec<u8> {
        let points: Vec<_> = (0..32).map(|i| [f64::from(i), 0.0, 0.0]).collect();
        let parameters: Vec<_> = (0..32).map(f64::from).collect();
        let mut c2 = polyline_payload(&points, &parameters);
        let end = c2.len();
        c2[end - 4..].copy_from_slice(&2_i32.to_le_bytes());
        let mut bytes = class_wrapper(POLYLINE_CLASS, &c2);
        bytes.extend(1_i32.to_le_bytes());
        bytes.extend(class_wrapper(
            LINE_CLASS,
            &line_payload([0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 1.0]),
        ));
        let mut support = vec![0x10];
        for value in [3_i32, 0, 2, 2, 2, 16, 0, 0] {
            support.extend(value.to_le_bytes());
        }
        support.extend([0; 48]);
        for count in [2_i32, 16] {
            support.extend(count.to_le_bytes());
            for i in 0..count {
                support.extend(f64::from(i).to_le_bytes());
            }
        }
        support.extend(32_i32.to_le_bytes());
        for i in 0..2 {
            for j in 0..16 {
                for value in [f64::from(i), f64::from(j), 0.0] {
                    support.extend(value.to_le_bytes());
                }
            }
        }
        bytes.extend(class_wrapper(
            crate::surfaces::NURBS_SURFACE.to_wire(),
            &support,
        ));
        bytes
    }

    #[test]
    fn carrier_selection_keeps_only_model_storage() {
        let bytes = large_carrier_payload();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_materialized_bytes = 64 * 1024;
        // The C3 line retains four knots and two finite control points.
        policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(
            4 * std::mem::size_of::<f64>()
                + 2 * std::mem::size_of::<cadmpeg_ir::features::FinitePoint3>(),
        );
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("root");
        let decoded = crate::curves::decode_inner(
            &ctx,
            &bytes,
            super::CLASS,
            0..bytes.len(),
            crate::settings::MillimeterScale::IDENTITY,
            ArchiveVersion::V8,
            0,
        )
        .expect("carrier fits its exact retained storage");
        let crate::curves::DecodedGeometry::Curve {
            curve:
                crate::curves::DecodedCurve::Leaf {
                    geometry:
                        cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                    ..
                },
        } = decoded
        else {
            panic!("model line");
        };
        assert_eq!(curve.control_points()[1].x, 2.0);
        let scratch = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "carrier scratch reuse",
            )
            .expect("all discarded component storage is released");
        drop(scratch);
        ctx.finish_session().expect("no refusal");
    }

    #[test]
    fn full_construction_keeps_parameter_and_support_storage() {
        let bytes = large_carrier_payload();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_materialized_bytes = 64 * 1024;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("root");
        let error = super::decode(
            &ctx,
            &bytes,
            0..bytes.len(),
            crate::settings::MillimeterScale::IDENTITY,
            ArchiveVersion::V8,
            0,
        )
        .expect_err("full construction retains C2");
        assert!(matches!(error, crate::curves::GeometryError::Codec(
            cadmpeg_core::CodecError::ResourceLimit(ref limit))
            if limit.operation == "Rhino polyline points"));
    }
}
