// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;
use std::ops::Range;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{CurveGeometry, ProceduralCurveDefinition, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;

use crate::chunks::ArchiveVersion;
use crate::test_support::test_archive;
use crate::RhinoCodec;

fn mixed(height: f64, middle: f64) -> (test_archive::PolycurveSource, [usize; 2]) {
    let line = test_archive::line_payload([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0]);
    let mut polyline = test_archive::polyline_payload(
        &[[1.0, 0.0, 0.0], [2.0, height, 0.0], [4.0, height, 0.0]],
        &[-2.0, middle, 6.0],
    );
    let lengths = [line.len(), polyline.len()];
    polyline.extend(b"unparsed-polyline-extension");
    (
        test_archive::polycurve_source(
            &[10.0, 20.0, 40.0],
            &[
                (test_archive::LINE_CLASS, line),
                (test_archive::POLYLINE_CLASS, polyline),
            ],
        ),
        lengths,
    )
}

fn archive(payloads: &[&[u8]]) -> Vec<u8> {
    test_archive::archive(
        &payloads
            .iter()
            .map(|payload| {
                test_archive::object_record(4, crate::curves::POLYCURVE.to_wire(), payload)
            })
            .collect::<Vec<_>>(),
    )
}

fn child_windows(
    data: &[u8],
    range: Range<usize>,
    fixture: &test_archive::PolycurveSource,
) -> [Range<usize>; 2] {
    let mut cursor = range.start + fixture.parameter_array.end;
    std::array::from_fn(|_| {
        let wrapper = crate::chunks::chunk_at(data, cursor, range.end, ArchiveVersion::V5, false)
            .expect("actual child wrapper");
        let child = crate::objects::parse_class_wrapper(
            data,
            cursor..wrapper.next_offset(),
            ArchiveVersion::V5,
            &mut crate::loss::Diagnostics::new(),
        )
        .expect("actual child class-data");
        cursor = wrapper.next_offset();
        child.class_data_range
    })
}

#[test]
fn source_polyline_edits_survive_in_the_final_ordered_compound_child() {
    for (height, middle) in [(2.0, 0.5), (5.0, 1.25)] {
        let (fixture, lengths) = mixed(height, middle);
        let bytes = archive(&[&fixture.bytes]);
        let scan = crate::container::scan_owned(bytes.clone()).expect("actual archive");
        let range = scan.objects[0]
            .framed()
            .expect("actual object")
            .class_data_range
            .clone();
        let windows = child_windows(scan.data, range.clone(), &fixture);
        assert!(windows[1].start > range.start);
        assert!(windows[1].start + lengths[1] < windows[1].end);
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("source mixed compound");
        let model = &decoded.ir().model;
        assert_eq!(model.curves.len(), 3);
        assert_eq!(model.procedural_curves.len(), 1);
        let record = &model.procedural_curves[0];
        let ProceduralCurveDefinition::Compound(compound) = record.definition() else {
            panic!("retained compound construction");
        };
        assert_eq!(
            compound
                .parameters()
                .iter()
                .map(|v| v.get())
                .collect::<Vec<_>>(),
            [10.0, 20.0, 40.0]
        );
        assert_eq!(compound.components().len(), 2);
        let children = compound
            .components()
            .iter()
            .map(|component| {
                model
                    .curves
                    .iter()
                    .find(|curve| curve.id == component.component)
                    .expect("final child identity")
            })
            .collect::<Vec<_>>();
        assert!(matches!(
            &children[0].geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
        ));
        let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(polyline)) = &children[1].geometry
        else {
            panic!("retained polyline NURBS carrier");
        };
        assert_eq!(polyline.degree(), 1);
        assert_eq!(polyline.knots().as_slice(), [-2.0, -2.0, middle, 6.0, 6.0]);
        assert_eq!(
            polyline
                .control_points()
                .into_iter()
                .map(FinitePoint3::get)
                .collect::<Vec<_>>(),
            [
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(2.0, height, 0.0),
                Point3::new(4.0, height, 0.0),
            ]
        );
        assert!(polyline.weights().is_none());
        assert!(!polyline.periodic());
        assert!(children[1].parameter_range.is_none());
    }
}

#[test]
fn malformed_late_polyline_parameters_keep_the_real_valid_peer() {
    let line = test_archive::line_payload([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0]);
    let bad = test_archive::polyline_payload(
        &[[1.0, 0.0, 0.0], [2.0, 2.0, 0.0], [4.0, 2.0, 0.0]],
        &[-2.0, 6.0],
    );
    let bad = test_archive::polycurve_source(
        &[10.0, 20.0, 40.0],
        &[
            (test_archive::LINE_CLASS, line),
            (test_archive::POLYLINE_CLASS, bad),
        ],
    );
    let (valid, _) = mixed(2.0, 0.5);
    let bytes = archive(&[&bad.bytes, &valid.bytes]);
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("malformed child does not discard its valid peer");
    assert_eq!(decoded.ir().model.curves.len(), 3);
    assert_eq!(decoded.ir().model.procedural_curves.len(), 1);
}
