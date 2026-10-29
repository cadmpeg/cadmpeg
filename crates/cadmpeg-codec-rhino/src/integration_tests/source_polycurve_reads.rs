// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{ProceduralCurve, ProceduralCurveDefinition};

use crate::test_support::test_archive;
use crate::RhinoCodec;

fn source(parameters: &[f64]) -> test_archive::PolycurveSource {
    test_archive::polycurve_source(
        parameters,
        &[
            (
                test_archive::LINE_CLASS,
                test_archive::line_payload([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0]),
            ),
            (
                test_archive::LINE_CLASS,
                test_archive::line_payload([1.0, 0.0, 0.0], [2.0, 1.0, 0.0], [0.0, 1.0]),
            ),
        ],
    )
}

fn archive(payload: &[u8]) -> Vec<u8> {
    test_archive::archive(&[test_archive::object_record(
        4,
        crate::curves::POLYCURVE.to_wire(),
        payload,
    )])
}

fn assert_parameters(record: &ProceduralCurve, expected: &[f64]) {
    let ProceduralCurveDefinition::Compound(compound) = record.definition() else {
        panic!("actual final compound definition")
    };
    assert_eq!(
        compound
            .parameters()
            .iter()
            .map(|value| value.get())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(compound.components().len() + 1, expected.len());
    for (component, parameter) in compound.components().iter().zip(expected) {
        assert_eq!(component.parameter.get(), *parameter);
    }
}

#[test]
fn public_polycurve_parameters_reach_the_final_compound() {
    let mut fixture = source(&[-2.0, 1.0, 6.0]);
    fixture.bytes.extend(b"unparsed-extension");
    let bytes = archive(&fixture.bytes);
    let result = RhinoCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("source compound");
    assert_eq!(result.ir().model.curves.len(), 3);
    assert_eq!(result.ir().model.procedural_curves.len(), 1);
    let record = &result.ir().model.procedural_curves[0];
    assert_parameters(record, &[-2.0, 1.0, 6.0]);
}

#[test]
fn nested_polycurve_parameters_have_their_actual_final_record_ids() {
    let inner = source(&[-2.0, 1.0, 6.0]);
    let outer = test_archive::polycurve_source(
        &[10.0, 20.0],
        &[(crate::curves::POLYCURVE.to_wire(), inner.bytes.clone())],
    );
    let bytes = archive(&outer.bytes);
    let result = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("nested compound");
    assert_eq!(result.ir().model.curves.len(), 4);
    assert_eq!(result.ir().model.procedural_curves.len(), 2);
    let model = &result.ir().model;
    let mut roots = model.procedural_curves.iter().filter(|record| {
        matches!(record.definition(), ProceduralCurveDefinition::Compound(compound)
            if compound.components().len() == 1)
    });
    let outer_record = roots.next().expect("one-component outer construction");
    assert!(roots.next().is_none());
    let ProceduralCurveDefinition::Compound(compound) = outer_record.definition() else {
        panic!("outer compound construction");
    };
    let inner_carrier = model
        .curves
        .iter()
        .find(|curve| curve.id == compound.components()[0].component)
        .expect("outer component is the inner carrier");
    let inner_construction = inner_carrier
        .geometry
        .procedural_construction()
        .expect("inner carrier has a construction");
    let inner_record = model
        .procedural_curves
        .iter()
        .find(|record| &record.id == inner_construction)
        .expect("inner construction is in the final arena");
    assert_ne!(inner_record.id, outer_record.id);
    assert_parameters(inner_record, &[-2.0, 1.0, 6.0]);
    assert_parameters(outer_record, &[10.0, 20.0]);
}

#[test]
fn late_malformed_polycurve_children_keep_a_valid_peer() {
    let inner = source(&[-2.0, 1.0, 6.0]);
    let invalid_child = test_archive::polycurve_source(
        &[10.0, 20.0, 30.0],
        &[
            (crate::curves::POLYCURVE.to_wire(), inner.bytes),
            (
                crate::test_support::test_dump::POINT_CLASS,
                test_archive::point_payload([2.0, 1.0, 0.0]),
            ),
        ],
    );
    let mut invalid_parameters = source(&[-2.0, 1.0, 6.0]);
    let second_parameter = invalid_parameters.parameter_values.start + std::mem::size_of::<f64>();
    invalid_parameters.bytes[second_parameter..second_parameter + std::mem::size_of::<f64>()]
        .copy_from_slice(&f64::NAN.to_le_bytes());
    let valid = source(&[-2.0, 1.0, 6.0]);
    let bytes = test_archive::archive(&[
        test_archive::object_record(4, crate::curves::POLYCURVE.to_wire(), &invalid_child.bytes),
        test_archive::object_record(
            4,
            crate::curves::POLYCURVE.to_wire(),
            &invalid_parameters.bytes,
        ),
        test_archive::object_record(4, crate::curves::POLYCURVE.to_wire(), &valid.bytes),
    ]);
    let result = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("malformed compounds preserve the valid peer");
    assert_eq!(result.ir().model.curves.len(), 3);
    assert_eq!(result.ir().model.procedural_curves.len(), 1);
    let record = &result.ir().model.procedural_curves[0];
    assert_parameters(record, &[-2.0, 1.0, 6.0]);
}

#[test]
fn reserved_polycurve_bytes_do_not_change_the_final_model() {
    let original = source(&[-2.0, 1.0, 6.0]);
    let mut modified = source(&[-2.0, 1.0, 6.0]);
    modified.bytes[modified.reserved.clone()].fill(0xff);
    let original = RhinoCodec
        .decode(
            &mut Cursor::new(archive(&original.bytes)),
            &DecodeOptions::default(),
        )
        .expect("original reserved values");
    let modified = RhinoCodec
        .decode(
            &mut Cursor::new(archive(&modified.bytes)),
            &DecodeOptions::default(),
        )
        .expect("reserved values have no procedural meaning");
    assert_eq!(original.ir().model.curves, modified.ir().model.curves);
    assert_eq!(
        original.ir().model.procedural_curves,
        modified.ir().model.procedural_curves
    );
}

#[test]
fn polycurve_line_source_edits_reach_the_exact_final_child() {
    for origin in [0.0, 4.0] {
        let mut line =
            test_archive::line_payload([origin, 0.0, 0.0], [origin + 1.0, 0.0, 0.0], [0.0, 1.0]);
        let fields_len = line.len();
        line.extend(b"unparsed-line-extension");
        let fixture =
            test_archive::polycurve_source(&[-2.0, 6.0], &[(test_archive::LINE_CLASS, line)]);
        let bytes = archive(&fixture.bytes);
        let scan = crate::container::scan_owned(bytes.clone()).expect("actual bounded source");
        let range = scan.objects[0]
            .framed()
            .expect("source object")
            .class_data_range
            .clone();
        assert!(range.start > 0);
        let child_start = range.start + fixture.parameter_array.end;
        let wrapper =
            crate::chunks::chunk_at(scan.data, child_start, range.end, scan.archive, false)
                .expect("actual child wrapper");
        let child = crate::objects::parse_class_wrapper(
            scan.data,
            child_start..wrapper.next_offset(),
            scan.archive,
            &mut crate::loss::Diagnostics::new(),
        )
        .expect("actual child class-data");
        let result = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("source compound with a bounded native line");
        let model = &result.ir().model;
        assert_eq!(model.curves.len(), 2);
        assert_eq!(model.procedural_curves.len(), 1);
        let record = &model.procedural_curves[0];
        assert_parameters(record, &[-2.0, 6.0]);
        let ProceduralCurveDefinition::Compound(compound) = record.definition() else {
            panic!("compound construction");
        };
        let carrier = model
            .curves
            .iter()
            .find(|curve| curve.id == compound.components()[0].component)
            .expect("actual final child carrier");
        let cadmpeg_ir::geometry::CurveGeometry::Solved(
            cadmpeg_ir::geometry::SolvedCurveGeometry::Line(line),
        ) = &carrier.geometry
        else {
            panic!("exact finite line carrier");
        };
        assert_eq!(
            line.origin().get(),
            cadmpeg_ir::math::Point3::new(origin, 0.0, 0.0)
        );
        assert_eq!(line.direction(), cadmpeg_ir::units::UnitVector3::X_AXIS);
        assert_eq!(
            carrier
                .parameter_range
                .expect("native line domain")
                .endpoints(),
            [0.0, 1.0]
        );
        let field_end = child.class_data_range.start + fields_len;
        assert!(field_end < child.class_data_range.end);
    }
}
