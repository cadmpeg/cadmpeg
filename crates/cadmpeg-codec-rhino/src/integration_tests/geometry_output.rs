// SPDX-License-Identifier: Apache-2.0
//! Empty geometry is determined from written carriers, not annotation or block records.

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::{transform::Transform, CadIr, Codec, DecodeOptions};
use std::io::Cursor;

use crate::loss::RhinoLossCode;
use crate::RhinoCodec;

const FINER_THAN_RHINO_WRITE_TOLERANCE: f64 = 1e-12;

fn cube_product() -> CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let definition = "test:model:product#cube".try_into().unwrap();
    ir.model.product_definitions.push(ProductDefinition {
        id: definition,
        kind: ProductDefinitionKind::Part,
        source_name: Some("Cube".into()),
        label: None,
        description: None,
        part_number: None,
        bom_properties: Default::default(),
        bodies: vec![ir.model.bodies[0].id.clone()],
        sketches: Vec::new(),
        native_ref: None,
    });
    ir.model.occurrences.push(Occurrence {
        id: "test:model:occurrence#cube".try_into().unwrap(),
        prototype: PrototypeReference::Local {
            definition: ir.model.product_definitions[0].id.clone(),
        },
        parent: OccurrenceParent::Root {},
        ordinal: 0,
        transform: Some(Transform::identity()),
        linked_prototype: None,
        name: Some("Cube use".into()),
        visible: None,
        link: None,
        native_ref: None,
    });
    ir
}

#[test]
fn a_written_instance_record_cannot_hide_the_omission_of_its_definition_geometry() {
    let original = cube_product();
    for omitted in [false, true] {
        let mut ir = original.clone();
        if omitted {
            ir.tolerances = cadmpeg_ir::units::Tolerances::new(
                FINER_THAN_RHINO_WRITE_TOLERANCE,
                ir.tolerances.angular.get(),
            )
            .unwrap();
        }
        let plan = RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit("rhino:archive-80"),
            )
            .unwrap();
        let empty = plan
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == RhinoLossCode::NoExportableSolids.kind())
            .collect::<Vec<_>>();
        assert_eq!(empty.len(), usize::from(omitted));
        if omitted {
            assert_eq!(empty[0].severity, cadmpeg_ir::report::Severity::Warning);
            assert!(plan.report().losses.iter().any(|loss| {
                loss.code == RhinoLossCode::WriterBodyOmitted.kind()
                    && loss.message.contains(ir.model.bodies[0].id.as_str())
            }));
        }
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let scan = crate::container::scan_owned(bytes).unwrap();
        assert_eq!(
            scan.objects
                .iter()
                .filter(|record| record.framed().unwrap().object_type == 0x1000)
                .count(),
            1
        );
        assert_eq!(
            scan.objects
                .iter()
                .filter(|record| record.framed().unwrap().object_type == 0x10)
                .count(),
            usize::from(!omitted)
        );
        assert_eq!(scan.objects.len(), if omitted { 1 } else { 2 });
        assert_eq!(
            ir.model.product_definitions,
            original.model.product_definitions
        );
        assert_eq!(ir.model.occurrences, original.model.occurrences);
        assert_eq!(ir.model.bodies, original.model.bodies);
    }
}

fn text_dot_document() -> CadIr {
    let mut dot = vec![0x11];
    for value in [1.0_f64, 2.0, 3.0] {
        dot.extend(value.to_le_bytes());
    }
    dot.extend(12_i32.to_le_bytes());
    dot.extend(crate::test_support::test_dump::utf16_bytes(
        "A real annotation",
    ));
    dot.extend(crate::test_support::test_dump::utf16_bytes("Arial"));
    dot.extend(0_i32.to_le_bytes());
    dot.extend(crate::test_support::test_dump::utf16_bytes(""));
    let class = crate::wire::Uuid::from_canonical([
        0x74, 0x19, 0x83, 0x02, 0xcd, 0xf4, 0x4f, 0x95, 0x96, 0x09, 0x6d, 0x68, 0x4f, 0x22, 0xab,
        0x37,
    ])
    .to_wire();
    let record = crate::test_support::test_archive::object_record(0x2000, class, &dot);
    let source = crate::test_support::test_archive::archive(&[record]);
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.ir().model.points.is_empty());
    assert_eq!(decoded.ir().model.semantic_annotations.len(), 1);
    assert_eq!(
        decoded.ir().model.semantic_annotations[0].text[0],
        "A real annotation"
    );
    decoded.ir().clone()
}

#[test]
fn a_written_annotation_is_empty_geometry_until_an_actual_point_is_written() {
    let original = text_dot_document();
    for geometric in [false, true] {
        let mut ir = original.clone();
        if geometric {
            ir.model.points.push(cadmpeg_ir::topology::Point::new(
                "test:model:point#actual".try_into().unwrap(),
                cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                    4.0, 5.0, 6.0,
                ))
                .unwrap(),
                None,
            ));
        }
        let plan = RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit("rhino:archive-80"),
            )
            .unwrap();
        assert_eq!(
            plan.report()
                .losses
                .iter()
                .filter(|loss| { loss.code == RhinoLossCode::NoExportableSolids.kind() })
                .count(),
            usize::from(!geometric)
        );
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let scan = crate::container::scan_owned(bytes).unwrap();
        assert_eq!(
            scan.objects
                .iter()
                .filter(|record| record.framed().unwrap().object_type == 0x2000)
                .count(),
            1
        );
        assert_eq!(
            scan.objects
                .iter()
                .filter(|record| record.framed().unwrap().object_type == 1)
                .count(),
            usize::from(geometric)
        );
        assert_eq!(scan.objects.len(), 1 + usize::from(geometric));
        assert_eq!(
            ir.model.semantic_annotations,
            original.model.semantic_annotations
        );
    }
}
