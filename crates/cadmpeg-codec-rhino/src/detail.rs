// SPDX-License-Identifier: Apache-2.0
//! Detail-view boundary carrier decoding.

use std::ops::Range;

use cadmpeg_ir::geometry::CurveGeometry;

use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader};
use crate::curves::{DecodedCurve, GeometryError};
use crate::wire::Uuid;

const ANONYMOUS: u32 = 0x4000_8000;
pub(crate) const CLASS: Uuid = Uuid::from_canonical([
    0xc8, 0xc6, 0x6e, 0xfa, 0xb3, 0xcb, 0x4e, 0x00, 0x94, 0x40, 0x2a, 0xd6, 0x62, 0x03, 0x37, 0x9e,
]);

#[derive(Debug, Clone)]
pub(crate) struct Detail {
    pub(crate) source_range: Range<usize>,
    pub(crate) view_range: Range<usize>,
    pub(crate) boundary: DecodedCurve,
    pub(crate) page_per_model_ratio: f64,
}

fn anonymous<'a>(
    data: &'a [u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
    family: &str,
) -> Result<(BoundedReader<'a>, usize, i32), GeometryError> {
    let chunk = chunk_at(data, offset, end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(GeometryError::malformed(
            offset,
            format!("{family} is not anonymous"),
        ));
    }
    let mut reader = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    let major = reader.i32()?;
    let minor = reader.i32()?;
    if major != 1 {
        return Err(GeometryError::UnsupportedVersion {
            offset: chunk.body.start,
            message: format!("unsupported {family} version {major}.{minor}"),
        });
    }
    Ok((reader, chunk.next_offset, minor))
}

pub(crate) fn decode(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
) -> Result<Detail, GeometryError> {
    let (mut outer, next, minor) = anonymous(data, range.start, range.end, archive, "detail")?;
    if next != range.end || minor < 0 {
        return Err(GeometryError::UnsupportedVersion {
            offset: range.start,
            message: format!("unsupported detail version 1.{minor}"),
        });
    }
    let view_start = outer.position();
    let (view, view_next, _view_minor) =
        anonymous(data, view_start, outer.end(), archive, "detail view state")?;
    let view_range = view.position()..view.end();
    outer.skip(view_next - outer.position())?;

    let boundary_start = outer.position();
    let (mut boundary, boundary_next, _boundary_minor) = anonymous(
        data,
        boundary_start,
        outer.end(),
        archive,
        "detail boundary",
    )?;
    let geometry = crate::surfaces::read_nurbs_curve(&mut boundary, 1.0)?;
    boundary.skip_remaining()?;
    outer.skip(boundary_next - outer.position())?;
    let page_per_model_ratio = if minor >= 1 { outer.f64()? } else { 0.0 };
    if !page_per_model_ratio.is_finite() || page_per_model_ratio < 0.0 {
        return Err(GeometryError::malformed(
            outer.position().saturating_sub(8),
            "detail page-to-model ratio is invalid",
        ));
    }
    outer.skip_remaining()?;
    Ok(Detail {
        source_range: range,
        view_range,
        boundary: DecodedCurve {
            geometry: CurveGeometry::Nurbs(geometry),
            compound: None,
            warnings: Vec::new(),
        },
        page_per_model_ratio,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::crc_chunk;

    fn anonymous(minor: i32, suffix: &[u8]) -> Vec<u8> {
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(minor.to_le_bytes());
        body.extend(suffix);
        crc_chunk(ANONYMOUS, &body)
    }

    fn boundary() -> Vec<u8> {
        let mut bytes = vec![0x11];
        for value in [2_i32, 0, 2, 2, 0, 0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend([0; 48]);
        bytes.extend(2_i32.to_le_bytes());
        bytes.extend(0.0_f64.to_le_bytes());
        bytes.extend(1.0_f64.to_le_bytes());
        bytes.extend(2_i32.to_le_bytes());
        for value in [0.0_f64, 0.0, 2.0, 0.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.push(0);
        bytes
    }

    fn boundary_3d() -> Vec<u8> {
        let mut bytes = vec![0x11];
        for value in [3_i32, 0, 2, 2, 0, 0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend([0; 48]);
        bytes.extend(2_i32.to_le_bytes());
        bytes.extend(0.0_f64.to_le_bytes());
        bytes.extend(1.0_f64.to_le_bytes());
        bytes.extend(2_i32.to_le_bytes());
        for value in [0.0_f64, 0.0, 7.0, 2.0, 0.0, 8.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.push(0);
        bytes
    }

    #[test]
    fn decodes_boundary_and_bounds_native_view_state() {
        let view = anonymous(2, &[7, 8, 9]);
        let mut boundary_body = boundary();
        boundary_body.extend([0xaa, 0xbb]);
        let boundary = anonymous(4, &boundary_body);
        let mut content = view;
        content.extend(boundary);
        content.extend(0.5_f64.to_le_bytes());
        content.extend([0xcc, 0xdd]);
        let bytes = anonymous(4, &content);

        let detail =
            decode(&bytes, 0..bytes.len(), ArchiveVersion::V8).expect("required invariant");
        assert_eq!(detail.page_per_model_ratio, 0.5);
        assert_eq!(&bytes[detail.view_range], &[7, 8, 9]);
        let CurveGeometry::Nurbs(boundary) = detail.boundary.geometry else {
            panic!("detail boundary must be NURBS");
        };
        assert_eq!(boundary.control_points()[1].x, 2.0);
    }

    #[test]
    fn minor_zero_defaults_ratio_and_skips_outer_suffix() {
        let view = anonymous(0, &[7, 8, 9]);
        let boundary = anonymous(0, &boundary());
        let mut content = view;
        content.extend(boundary);
        content.extend([0xcc, 0xdd]);
        let bytes = anonymous(0, &content);

        let detail =
            decode(&bytes, 0..bytes.len(), ArchiveVersion::V8).expect("required invariant");

        assert_eq!(detail.page_per_model_ratio, 0.0);
        assert_eq!(&bytes[detail.view_range], &[7, 8, 9]);
    }

    #[test]
    fn preserves_three_dimensional_boundary_poles() {
        let view = anonymous(0, &[]);
        let boundary = anonymous(0, &boundary_3d());
        let mut content = view;
        content.extend(boundary);
        let bytes = anonymous(0, &content);

        let detail =
            decode(&bytes, 0..bytes.len(), ArchiveVersion::V5).expect("required invariant");
        let CurveGeometry::Nurbs(boundary) = detail.boundary.geometry else {
            panic!("detail boundary must be NURBS");
        };
        assert_eq!(boundary.control_points()[0].z, 7.0);
        assert_eq!(boundary.control_points()[1].z, 8.0);
    }
}
