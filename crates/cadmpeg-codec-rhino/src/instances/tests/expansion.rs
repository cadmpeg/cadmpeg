// SPDX-License-Identifier: Apache-2.0
use super::support::{install_definitions, static_definition};
use crate::chunks::ArchiveVersion;
use crate::test_support::test_dump::{
    instance_reference_payload, object_record_with_payload, point_payload, scan_with_objects,
    set_identity, transform, utf16_bytes, INSTANCE_REFERENCE_CLASS, POINT_CLASS,
};
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::math::Point3;

#[test]
fn instance_expansion_keeps_a_compound_curve_construction_with_placed_components() {
    use crate::test_support::test_archive::{
        line_payload, polycurve_payload, LINE_CLASS, POLYCURVE_CLASS,
    };
    let archive = ArchiveVersion::V5;
    let member_id = [0x51; 16];
    let definition_id = [0x61; 16];
    let reference_id = [0x71; 16];
    let polycurve = polycurve_payload(
        &[0.0, 1.0, 2.0],
        &[
            (
                LINE_CLASS,
                line_payload([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0]),
            ),
            (
                LINE_CLASS,
                line_payload([1.0, 0.0, 0.0], [2.0, 1.0, 0.0], [0.0, 1.0]),
            ),
        ],
    );
    let member = object_record_with_payload(archive, 4, POLYCURVE_CLASS, &polycurve);
    let reference = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(definition_id, transform(1.0, [10.0, 0.0, 0.0])),
    );
    let mut scan = scan_with_objects(&[member, reference]);
    set_identity(&mut scan, 0, member_id, "polycurve", None, true);
    set_identity(&mut scan, 1, reference_id, "reference", None, true);
    install_definitions(
        &mut scan,
        vec![static_definition(definition_id, &[member_id])],
    );

    let result = crate::decode::decode_for_test(&scan);
    assert!(
        !result
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("cannot be transformed exactly")),
        "{:#?}",
        result.report().losses
    );
    assert_eq!(result.ir().model.procedural_curves.len(), 1);
    let starts = result
        .ir()
        .model
        .curves
        .iter()
        .filter_map(|curve| match &curve.geometry {
            cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
                Some(nurbs.control_points()[0])
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        starts,
        vec![Point3::new(10.0, 0.0, 0.0), Point3::new(11.0, 0.0, 0.0)]
    );
}

#[test]
fn instance_expansion_places_geometry_members_and_reports_an_annotation_member() {
    let archive = ArchiveVersion::V5;
    let point_id = [0x51; 16];
    let dot_id = [0x52; 16];
    let definition_id = [0x61; 16];
    let reference_id = [0x71; 16];
    let point =
        object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([1.0, 2.0, 3.0]));
    let mut dot_payload = vec![0x10];
    for coordinate in [1.0_f64, 2.0, 3.0] {
        dot_payload.extend(coordinate.to_le_bytes());
    }
    dot_payload.extend(12_i32.to_le_bytes());
    dot_payload.extend(utf16_bytes("dot"));
    dot_payload.extend(utf16_bytes("Witness Sans"));
    dot_payload.extend(0_i32.to_le_bytes());
    let dot = object_record_with_payload(
        archive,
        0x20,
        crate::annotations::TEXT_DOT.to_wire(),
        &dot_payload,
    );
    let reference = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(definition_id, transform(1.0, [10.0, 0.0, 0.0])),
    );
    let mut scan = scan_with_objects(&[point, dot, reference]);
    set_identity(&mut scan, 0, point_id, "point", None, true);
    set_identity(&mut scan, 1, dot_id, "dot", None, true);
    set_identity(&mut scan, 2, reference_id, "reference", None, true);
    install_definitions(
        &mut scan,
        vec![static_definition(definition_id, &[point_id, dot_id])],
    );

    let result = crate::decode::decode_for_test(&scan);
    assert_eq!(
        result
            .ir()
            .model
            .points
            .iter()
            .map(|point| point.position().get())
            .collect::<Vec<_>>(),
        vec![Point3::new(11.0, 2.0, 3.0)]
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == crate::loss::RhinoLossCode::AnnotationRecordDropped.kind()
            && loss.message.contains("definition-member annotation")
    }));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("did not decode")));
}
