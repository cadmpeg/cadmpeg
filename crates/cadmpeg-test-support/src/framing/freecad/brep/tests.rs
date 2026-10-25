// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::framing::PartTable;
use crate::zip_fixture::{archive, RawEntry};
use cadmpeg_core::decode::{ByteRange, DecodeArena, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::PartLevel;

fn fixture(name: &str, payload: &[u8], inspect: impl FnOnce(&[FramedPart<'_>])) {
    let bytes = archive(&[RawEntry::stored(name, payload)]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    // docs/layouts/zip.toml: a local header precedes the filename by 30 bytes.
    let start = 30 + name.len();
    let end = start + payload.len();
    let member = ctx
        .register_named_slice(
            root,
            ByteRange {
                start: u64_from_index(start),
                end: u64_from_index(end),
            },
            name,
        )
        .unwrap();
    let name = root.child(30, start).unwrap();
    let mut frames = vec![
        FramedPart::new(
            &ctx,
            PartLevel::parse("zip.archive").unwrap(),
            root,
            FrameParent::File,
            vec![],
            vec![],
        )
        .unwrap(),
        FramedPart::new(
            &ctx,
            PartLevel::parse("zip.entry").unwrap(),
            root.child(0, end).unwrap(),
            FrameParent::Part(0),
            vec![0..u64_from_index(start)],
            vec![],
        )
        .unwrap(),
    ];
    ctx.record("test:framing#brep", root, || {
        frame_brep_member(&ctx, member, name, 1, &mut frames)
            .map_err(cadmpeg_core::CodecError::Malformed)
    })
    .unwrap();
    PartTable::load().unwrap().validate(&ctx, &frames).unwrap();
    inspect(&frames);
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert!(captures[0].reads().is_empty());
}
#[test]
fn zero_count_binary_tables_have_no_record_attempt_and_empty_members_have_no_children() {
    let empty=b"Open CASCADE Topology V4, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n";
    fixture("Shape.BIN", empty, |frames| {
        assert_eq!(frames.len(), 12);
        assert_eq!(frames[2].level().as_str(), "freecad.brep_binary");
        assert_eq!(frames[3].level().as_str(), "freecad.brep_binary_header");
    });
    fixture("Empty.brp", b"", |frames| {
        assert_eq!(frames.len(), 3);
    });
}
#[test]
fn unknown_binary_record_keeps_one_prefix_and_stops_before_any_later_selector_or_root() {
    let mut bytes =
        b"Open CASCADE Topology V4, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 2\n".to_vec();
    let first = bytes.len();
    bytes.push(1);
    for value in [0.0_f64, 0.0, 0.0, 1.0, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let failed = bytes.len();
    bytes.push(255);
    bytes.extend_from_slice(
        b"\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n",
    );
    fixture("Shape.bin", &bytes, |frames| {
        let records = frames
            .iter()
            .filter(|frame| frame.level().as_str() == "freecad.brep_binary_record")
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 5);
        let base = frames[2].view().start();
        assert_eq!(
            (records[3].view().start(), records[3].view().end()),
            (base + first, base + failed)
        );
        assert_eq!(records[4].view().end(), base + bytes.len());
        assert!(!frames
            .iter()
            .any(|frame| frame.level().as_str() == "freecad.brep_binary_root"));
    });
}
#[test]
fn malformed_unlisted_selector_has_no_synthetic_binary_child() {
    fixture(
        "Shape.bin",
        b"Open CASCADE Topology V4, (c) Open Cascade\nLocations invalid\nCurves 1\n",
        |frames| {
            // The new literal row 92 locates the actual failed marker.
            assert_eq!(frames.len(), 5);
            assert_eq!(frames[4].level().as_str(), "freecad.brep_binary_record");
            assert_eq!(frames[4].view().start(), frames[3].view().end());
            assert_eq!(frames[4].view().end(), frames[2].view().end());
        },
    );
}

#[test]
fn zero_binary_child_index_does_not_borrow_a_later_records_terminator() {
    let mut bytes=b"Open CASCADE Topology V4, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 2\n".to_vec();
    let start = bytes.len();
    bytes.extend_from_slice(&[0; 9]);
    bytes.extend_from_slice(&[0, 1, 0, 0, 1, 0, 0, 0, b'*']);
    bytes.extend_from_slice(&[255; 12]);
    fixture("Shape.bin", &bytes, |frames| {
        let records = frames
            .iter()
            .filter(|frame| frame.level().as_str() == "freecad.brep_binary_record")
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 9);
        assert_eq!(records[8].view().start(), frames[2].view().start() + start);
        assert_eq!(records[8].view().end(), frames[2].view().end());
        assert!(!frames
            .iter()
            .any(|frame| { frame.level().as_str() == "freecad.brep_binary_root" }));
    });
}
#[test]
fn text_curves_and_terminated_shapes_have_independent_exact_extents() {
    let bytes=b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 2\n1 0 0 0 1 0 0\n8 0 1 1 0 0 0 0 1 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 2\nZZ 0101000 *\nCo 0101000 +2 0000000 *\n+1 0 *";
    fixture("Shape.brp", bytes, |frames| {
        let curves = frames
            .iter()
            .filter(|frame| frame.level().as_str() == "freecad.brep_text_curve")
            .collect::<Vec<_>>();
        assert_eq!(curves.len(), 2);
        assert_eq!(curves[0].view().window(), b"1 0 0 0 1 0 0");
        assert_eq!(curves[1].view().window(), b"8 0 1 1 0 0 0 0 1 0");
        let shapes = frames
            .iter()
            .filter(|frame| frame.level().as_str() == "freecad.brep_text_tshape")
            .collect::<Vec<_>>();
        assert_eq!(shapes.len(), 2);
        assert_eq!(shapes[0].view().window(), b"ZZ 0101000 *");
        assert_eq!(shapes[1].view().window(), b"Co 0101000 +2 0000000 *");
        let flags = &shapes[1].dispatch()[1];
        assert_eq!(
            shapes[1]
                .view()
                .child(
                    usize::try_from(flags.start).unwrap(),
                    usize::try_from(flags.end).unwrap()
                )
                .unwrap()
                .window(),
            b"0101000"
        );
        assert_eq!(frames.last().unwrap().view().window(), b"\n+1 0 *");
    });
}

#[test]
fn binary_root_locator_requires_twelve_bytes_between_last_shape_and_member_end() {
    for length in [0, 1, 11, 12, 13, 24] {
        let mut bytes=b"Open CASCADE Topology V4, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n".to_vec();
        bytes.extend(std::iter::repeat_n(255, length));
        fixture("Shape.bin", &bytes, |frames| {
            let roots = frames
                .iter()
                .filter(|frame| frame.level().as_str() == "freecad.brep_binary_root")
                .collect::<Vec<_>>();
            assert_eq!(roots.len(), usize::from((1..=12).contains(&length)));
            if let Some(root) = roots.first() {
                assert_eq!(root.view().window().len(), length);
                assert_eq!(root.view().end(), frames[2].view().end());
            }
        });
    }
}

#[test]
fn preserved_decimal_count_and_child_stars_bound_a_table_after_separator_damage() {
    let prefix=b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 3";
    for fill in [0, 255] {
        let mut bytes = prefix.to_vec();
        for _ in 0..3 {
            bytes.extend(std::iter::repeat_n(fill, 11));
            bytes.push(b'*');
        }
        let end = bytes.len();
        bytes.extend_from_slice(b"\n+1 0 *");
        fixture("Shape.brp", &bytes, |frames| {
            let table = frames
                .iter()
                .find(|frame| {
                    frame.level().as_str() == "freecad.brep_text_table"
                        && frame.view().window().starts_with(b"TShapes")
                })
                .unwrap();
            let base = frames[2].view().start();
            assert_eq!(table.view().end(), base + end);
            let children = frames
                .iter()
                .filter(|frame| frame.level().as_str() == "freecad.brep_text_tshape")
                .collect::<Vec<_>>();
            assert_eq!(children.len(), 3);
            assert!(children
                .iter()
                .all(|frame| frame.view().window().last() == Some(&b'*')));
            let root = frames.last().unwrap();
            assert_eq!(root.level().as_str(), "freecad.brep_text_root");
            assert_eq!(root.view().window(), b"\n+1 0 *");
        });
    }
}
