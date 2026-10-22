// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use crate::test_support::test_archive;
use crate::RhinoCodec;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::math::Point3;

fn archive() -> Vec<u8> {
    let mut fixture = test_archive::nurbs_surface_source(Some([2.0, 4.0, 2.0, 4.0]));
    fixture.bytes.extend(b"unparsed-extension");
    test_archive::archive(&[test_archive::object_record(
        8,
        crate::surfaces::NURBS_SURFACE.to_wire(),
        &fixture.bytes,
    )])
}

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
            surface.weight(u, v).map(|value| value.get()),
            weights.map(|values| values[index])
        );
    }
    assert!(!surface.u_periodic());
    assert!(!surface.v_periodic());
    assert!(record.source_object.is_some());
}

#[test]
fn public_surface_decode_keeps_polynomial_and_rational_tensors_with_exact_source_reads() {
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
            .class_data_range();
        let result = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("source surface");
        assert_eq!(result.ir().model.surfaces.len(), 1);
        let surface = &result.ir().model.surfaces[0];
        assert_surface(surface, weights);
        let layout = &result.source_fidelity().record_layout()[surface.id.as_str()];
        assert_eq!(
            layout.offset(),
            u64::try_from(range.start).expect("small start")
        );
        assert_eq!(
            layout.byte_len(),
            u64::try_from(range.len()).expect("small extent")
        );
        assert_eq!(
            layout.reads()["file"],
            [
                u64::try_from(range.start).expect("small start")
                    ..u64::try_from(range.start + fixture.reserved_bounds.start)
                        .expect("small bounds start"),
                u64::try_from(range.start + fixture.reserved_bounds.end).expect("small bounds end")
                    ..u64::try_from(range.start + fields_len).expect("small fields end")
            ]
        );
        assert!(range.start + fields_len < range.end);
    }
}

#[test]
fn detached_nurbs_surface_output_keeps_the_same_surface_without_source_captures() {
    let bytes = archive();
    let source = RhinoCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("source surface");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let ctx = DecodeContext::new(&arena, &policy, cadmpeg_core::decode::DecodeDepth::Full);
    let detached = RhinoCodec
        .decode_with_context(&ctx, View::over_retained(&bytes))
        .expect("pure authored-output parse");
    assert_eq!(detached.ir().model.surfaces, source.ir().model.surfaces);
    assert!(detached.source_fidelity().record_layout().is_empty());
    assert!(ctx
        .take_record_captures()
        .expect("no fabricated source captures")
        .is_empty());
}

#[test]
fn a_foreign_registered_nurbs_surface_view_cannot_enter_the_producer_scope() {
    let bytes = archive();
    let first_arena = DecodeArena::new();
    let (_first, source) =
        DecodeContext::from_root_bytes(&bytes, &first_arena, &DecodePolicy::default())
            .expect("first session");
    let second_arena = DecodeArena::new();
    let (second, _) =
        DecodeContext::from_root_bytes(b"other", &second_arena, &DecodePolicy::default())
            .expect("second session");
    let failure = RhinoCodec
        .decode_with_context(&second, source)
        .expect_err("foreign source cannot enter the surface record scope");
    assert!(failure
        .to_string()
        .contains("another or detached decode session"));
    assert!(second
        .take_record_captures()
        .expect("no foreign capture committed")
        .is_empty());
}

#[test]
fn malformed_nurbs_surface_rows_leave_no_capture_and_the_valid_peer_survives() {
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
    assert_eq!(result.source_fidelity().record_layout().len(), 1);
    assert!(result
        .source_fidelity()
        .record_layout()
        .contains_key(surface.id.as_str()));
}
