// SPDX-License-Identifier: Apache-2.0
//! Curve-on-surface construction decoding.

use std::ops::Range;

use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader, FramingError};
use crate::curves::{DecodedCurve, DecodedGeometry, GeometryError};
use crate::objects::parse_class_wrapper;
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
    pub(crate) warnings: Vec<String>,
}

fn malformed(offset: usize, message: impl Into<String>) -> GeometryError {
    GeometryError::Malformed(FramingError::Structural {
        offset,
        message: message.into(),
    })
}

fn class(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<crate::objects::ClassDescriptor, GeometryError> {
    let start = reader.position();
    let wrapper = chunk_at(data, start, reader.end(), archive, false)?;
    let class = parse_class_wrapper(data, start..wrapper.next_offset, archive, warnings)?;
    reader.skip(wrapper.next_offset - start)?;
    Ok(class)
}

pub(crate) fn decode(
    data: &[u8],
    range: Range<usize>,
    scale: f64,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<CurveOnSurface, GeometryError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let mut warnings = Vec::new();
    let c2 = class(data, &mut reader, archive, &mut warnings)?;
    let decoded =
        crate::curves::decode_inner_2d(data, c2.class_uuid, c2.class_data_range, archive, depth)?;
    let DecodedGeometry::Curve {
        curve: parameter_curve,
    } = decoded
    else {
        return Err(malformed(
            range.start,
            "curve-on-surface C2 object is not a curve",
        ));
    };
    let has_model_curve = match reader.i32()? {
        0 => false,
        1 => true,
        _ => {
            return Err(malformed(
                reader.position() - 4,
                "invalid curve-on-surface C3 presence",
            ))
        }
    };
    let model_curve = if has_model_curve {
        let c3 = class(data, &mut reader, archive, &mut warnings)?;
        if c3.class_uuid == CLASS {
            return Err(malformed(
                reader.position(),
                "nested curve-on-surface C3 carrier is invalid",
            ));
        }
        let decoded = crate::curves::decode_inner(
            data,
            c3.class_uuid,
            c3.class_data_range,
            scale,
            archive,
            depth,
        )?;
        let DecodedGeometry::Curve { curve } = decoded else {
            return Err(malformed(
                reader.position(),
                "curve-on-surface C3 object is not a curve",
            ));
        };
        Some(curve)
    } else {
        None
    };
    let support = class(data, &mut reader, archive, &mut warnings)?;
    if !crate::curves::surface_class(support.class_uuid) {
        return Err(malformed(
            reader.position(),
            "curve-on-surface support is not a surface",
        ));
    }
    let surface = crate::surfaces::decode(
        data,
        support.class_uuid,
        support.class_data_range,
        scale,
        archive,
        depth,
    )?;
    if reader.remaining() != 0 {
        return Err(malformed(
            reader.position(),
            "curve-on-surface has trailing bytes",
        ));
    }
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
    use super::*;
    use crate::archive_test_support::{
        class_wrapper, line_payload, polyline_payload, LINE_CLASS, POLYLINE_CLASS,
    };

    const PLANE_SURFACE: [u8; 16] = [
        0xdf, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ];

    fn plane_surface() -> Vec<u8> {
        let mut payload = vec![0x10];
        payload.extend(
            [
                0.0_f64, 0.0, 0.0, // origin
                1.0, 0.0, 0.0, // x axis
                0.0, 1.0, 0.0, // y axis
                0.0, 0.0, 1.0, // z axis
                0.0, 0.0, 1.0, 0.0, // equation
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

        let decoded = decode(&bytes, 0..bytes.len(), 10.0, ArchiveVersion::V8, 0)
            .expect("required invariant");
        assert!(decoded.model_curve.is_some());
        let cadmpeg_ir::geometry::CurveGeometry::Nurbs(c2) = decoded.parameter_curve.geometry
        else {
            panic!("expected NURBS parameter curve");
        };
        assert_eq!(c2.control_points[1].x, 1.0);
        let DecodedSurface::Typed { geometry, .. } = decoded.surface else {
            panic!("expected typed support surface");
        };
        assert!(matches!(
            geometry,
            cadmpeg_ir::geometry::SurfaceGeometry::Plane { .. }
        ));
    }
}
