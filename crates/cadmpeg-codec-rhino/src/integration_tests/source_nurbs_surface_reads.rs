// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use crate::test_support::test_archive;
use crate::RhinoCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::math::Point3;

fn assert_surface(record: &Surface, weights: Option<[f64; 4]>) {
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) = &record.geometry else {
        panic!("native NURBS class stays a NURBS surface")
    };
    assert_eq!((surface.u_degree(), surface.v_degree()), (1, 1));
    assert_eq!((surface.u_count(), surface.v_count()), (2, 2));
    assert_eq!(surface.u_knots().as_slice(), [-1.0, -1.0, 2.0, 2.0]);
    assert_eq!(surface.v_knots().as_slice(), [5.0, 5.0, 9.0, 9.0]);
    for (index, (u, v, expected)) in [
        (0, 0, Point3::new(0.0, 0.0, 0.0)),
        (0, 1, Point3::new(0.0, 2.0, 0.0)),
        (1, 0, Point3::new(3.0, 0.0, 1.0)),
        (1, 1, Point3::new(3.0, 2.0, 1.0)),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(surface.pole(u, v).expect("source pole").get(), expected);
        assert_eq!(
            surface
                .weight(u, v)
                .map(cadmpeg_ir::scalar::NonZeroReal::get),
            weights.map(|values| values[index])
        );
    }
    assert!(!surface.u_periodic());
    assert!(!surface.v_periodic());
    assert!(record.source_object.is_some());
}

#[test]
fn public_surface_decode_keeps_polynomial_and_rational_tensors() {
    for weights in [None, Some([2.0, 4.0, 2.0, 4.0])] {
        let mut fixture = test_archive::nurbs_surface_source(weights);
        let fields_len = fixture.bytes.len();
        fixture.bytes.extend(b"unparsed-extension");
        let bytes = test_archive::archive(&[test_archive::object_record(
            8,
            crate::surfaces::NURBS_SURFACE.to_wire(),
            &fixture.bytes,
        )]);
        let scan = crate::container::scan_owned(bytes.clone()).expect("bounded source");
        let range = scan.objects[0]
            .framed()
            .expect("surface record")
            .class_data_range
            .clone();
        let result = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("source surface");
        assert_eq!(result.ir().model.surfaces.len(), 1);
        let surface = &result.ir().model.surfaces[0];
        assert_surface(surface, weights);
        assert!(range.start + fields_len < range.end);
    }
}

#[test]
fn malformed_nurbs_surface_rows_keep_the_valid_peer() {
    let malformed = test_archive::nurbs_surface_source(Some([2.0, 4.0, 0.0, 4.0]));
    let mut mismatch = test_archive::nurbs_surface_source(None);
    mismatch.bytes[mismatch.cv_count_offset..mismatch.cv_count_offset + std::mem::size_of::<i32>()]
        .copy_from_slice(&3_i32.to_le_bytes());
    let valid = test_archive::nurbs_surface_source(Some([2.0, 4.0, 2.0, 4.0]));
    let bytes = test_archive::archive(&[
        test_archive::object_record(
            8,
            crate::surfaces::NURBS_SURFACE.to_wire(),
            &malformed.bytes,
        ),
        test_archive::object_record(8, crate::surfaces::NURBS_SURFACE.to_wire(), &mismatch.bytes),
        test_archive::object_record(8, crate::surfaces::NURBS_SURFACE.to_wire(), &valid.bytes),
    ]);
    let result = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("bounded malformed surfaces retain the valid peer");
    assert_eq!(result.ir().model.surfaces.len(), 1);
    let surface = &result.ir().model.surfaces[0];
    assert_surface(surface, Some([2.0, 4.0, 2.0, 4.0]));
}
