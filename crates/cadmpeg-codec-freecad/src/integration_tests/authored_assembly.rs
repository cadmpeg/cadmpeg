// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::products::{
    ExternalDocument, JointLimits, JointOperands, OccurrenceParent, OperandContainer,
    PairedJointKind, PrototypeReference,
};
use cadmpeg_ir::{Codec, DecodeOptions};

use crate::FcstdCodec;

const LENGTH_TOLERANCE: f64 = 1.0e-12;
const ANGLE_TOLERANCE: f64 = 1.0e-12;
const DIRECTION_TOLERANCE: f64 = 1.0e-12;
const SCALE_TOLERANCE: f64 = 1.0e-12;

#[test]
fn authored_assembly_keeps_prototypes_occurrences_and_joints() {
    let data = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/product_assembly.FCStd");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .unwrap();
    let model = &decoded.ir().model;
    let definition = |name: &str| {
        model
            .product_definitions
            .iter()
            .find(|p| p.source_name.as_deref() == Some(name))
            .unwrap()
    };
    for (name, label, part_number) in [
        ("BracketPrototype", "Bracket", "CC0-BRACKET-01"),
        ("PinPrototype", "Pin", "CC0-PIN-01"),
    ] {
        let item = definition(name);
        assert_eq!(item.label.as_deref(), Some(label));
        assert_eq!(item.part_number.as_deref(), Some(part_number));
        assert_eq!(item.bodies.len(), 1);
    }
    let occurrence = |name: &str, suffix: &str| {
        model
            .occurrences
            .iter()
            .find(|item| item.id.as_str() == format!("fcstd:model:occurrence#{name}:{suffix}"))
            .unwrap()
    };
    let assembly = occurrence("Assembly", "container");
    let group = occurrence("OccurrenceGroup", "container");
    assert!(matches!(assembly.parent, OccurrenceParent::Root {}));
    assert!(
        matches!(&group.parent, OccurrenceParent::Occurrence { occurrence } if occurrence == &assembly.id)
    );
    assert_eq!(group.ordinal, 2);
    for (name, prototype, origin, ordinal) in [
        ("BracketOccurrence", "BracketPrototype", [10.0, 0.0, 0.0], 0),
        ("PinOccurrence", "PinPrototype", [20.0, 6.0, 3.0], 1),
    ] {
        let item = occurrence(name, "instance");
        assert_eq!(item.ordinal, ordinal);
        assert!(
            matches!(&item.parent, OccurrenceParent::Occurrence { occurrence } if occurrence == &assembly.id)
        );
        assert!(
            matches!(&item.prototype, PrototypeReference::Local { definition: id } if *id == definition(prototype).id)
        );
        for (row, expected) in item.transform.affine_rows().iter().zip(origin) {
            assert!((row[3] - expected).abs() < LENGTH_TOLERANCE, "{name}");
        }
    }
    let pin_rows = occurrence("PinOccurrence", "instance")
        .transform
        .affine_rows();
    for (row, expected) in pin_rows
        .iter()
        .zip([[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]])
    {
        for (actual, expected) in row[..3].iter().zip(expected) {
            assert!((actual - expected).abs() < DIRECTION_TOLERANCE);
        }
    }
    let nested = occurrence("NestedBracketOccurrence", "instance");
    assert!(
        matches!(&nested.parent, OccurrenceParent::Occurrence { occurrence } if occurrence == &group.id)
    );
    assert_eq!(nested.ordinal, 0);
    assert!(
        matches!(&nested.prototype, PrototypeReference::Local { definition: id } if *id == definition("BracketOccurrence").id)
    );
    let rows = nested.effective_transform().unwrap().affine_rows();
    for (row, expected) in rows.iter().zip([10.0, 20.0, 0.0]) {
        assert!((row[3] - expected).abs() < LENGTH_TOLERANCE);
    }
    let nodes = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::ProductNodeRecord>("product_nodes")
        .unwrap();
    let array = nodes
        .iter()
        .find(|node| node.object.ends_with("#PinArray"))
        .unwrap();
    assert_eq!(array.element_transforms().len(), 3);
    assert_eq!(array.element_scales().len(), 3);
    assert_eq!(
        model
            .occurrences
            .iter()
            .filter(|item| item.native_ref.as_deref() == Some("fcstd:native:object#PinArray"))
            .count(),
        3
    );
    for index in 0..3 {
        let element = format!("fcstd:native:object#PinArray_i{index}");
        assert!(!model
            .occurrences
            .iter()
            .any(|item| item.native_ref.as_deref() == Some(element.as_str())));
    }
    for (index, (x, y, scale)) in [(0.0, 0.0, 1.0), (1.0, 10.0, 1.25), (2.0, 20.0, 1.5)]
        .into_iter()
        .enumerate()
    {
        let saved = array.element_transforms()[index].rows();
        for (row, expected) in saved.iter().take(3).zip([0.0, y, 0.0]) {
            assert!((row[3] - expected).abs() < LENGTH_TOLERANCE);
        }
        for (actual, expected) in array.element_scales()[index]
            .get()
            .into_iter()
            .zip([1.0, scale, 1.0])
        {
            assert!((actual - expected).abs() < SCALE_TOLERANCE);
        }
        let item = occurrence("PinArray", &index.to_string());
        assert_eq!(item.ordinal, u32::try_from(index).unwrap() + 1);
        assert!(
            matches!(&item.parent, OccurrenceParent::Occurrence { occurrence } if occurrence == &group.id)
        );
        assert!(
            matches!(&item.prototype, PrototypeReference::Local { definition: id } if *id == definition("PinPrototype").id)
        );
        let rows = item.transform.affine_rows();
        assert!((rows[0][3] - x).abs() < LENGTH_TOLERANCE);
        assert!(rows[1][3].abs() < LENGTH_TOLERANCE);
        assert!(rows[2][3].abs() < LENGTH_TOLERANCE);
        for (i, (saved_row, row)) in saved.iter().zip(rows.iter()).take(3).enumerate() {
            for (j, (saved_value, value)) in saved_row.iter().zip(row.iter()).take(3).enumerate() {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!((saved_value - expected).abs() < DIRECTION_TOLERANCE);
                assert!((value - expected).abs() < DIRECTION_TOLERANCE);
            }
        }
    }
    let external = occurrence("ExternalOccurrence", "instance");
    assert_eq!(external.ordinal, 4);
    assert!(
        matches!(&external.parent, OccurrenceParent::Occurrence { occurrence } if occurrence == &group.id)
    );
    assert!(
        matches!(&external.prototype, PrototypeReference::External { document: ExternalDocument::Path { path }, object: Some(name) } if path.as_str() == "external_component.FCStd" && name == "ExternalPrototype")
    );
    assert!((external.transform.affine_rows()[0][3] - 40.0).abs() < LENGTH_TOLERANCE);
    let joint = model
        .assembly_joints
        .iter()
        .find(|j| j.id.as_str().ends_with("#RevoluteJoint:constraint"))
        .unwrap();
    assert!(!joint.suppressed);
    let JointOperands::Pair {
        kind:
            PairedJointKind::Revolute {
                angle: Some(angle),
                angular_limits: Some(JointLimits::Range(range)),
            },
        connectors,
        offset_frames: Some(offsets),
        ..
    } = joint.operands()
    else {
        panic!("revolute joint")
    };
    assert!((angle.get() - 12.0_f64.to_radians()).abs() < ANGLE_TOLERANCE);
    assert!((range.minimum().get() - (-35.0_f64).to_radians()).abs() < ANGLE_TOLERANCE);
    assert!((range.maximum().get() - 55.0_f64.to_radians()).abs() < ANGLE_TOLERANCE);
    for (index, (name, subs, z)) in [
        ("BracketOccurrence", ["Face1", "Edge1"], 0.5),
        ("PinOccurrence", ["Face2", "Edge3"], -0.5),
    ]
    .into_iter()
    .enumerate()
    {
        let connector = &connectors[index];
        assert_eq!(
            connector.operand.object,
            format!("fcstd:native:object#{name}")
        );
        assert!(
            matches!(&connector.operand.container, OperandContainer::Occurrence { occurrence: id, occurrence_path } if id == &occurrence(name, "instance").id && occurrence_path.is_empty())
        );
        assert_eq!(connector.operand.subelements, subs);
        assert_eq!(connector.detached, index == 0);
        for (row, expected) in connector.frame.affine_rows().iter().zip([20.0, 6.0, 3.0]) {
            assert!((row[3] - expected).abs() < LENGTH_TOLERANCE);
        }
        assert!((offsets[index].affine_rows()[2][3] - z).abs() < LENGTH_TOLERANCE);
    }
    let ground = model
        .assembly_joints
        .iter()
        .find(|j| j.id.as_str().ends_with("#GroundBracket:constraint"))
        .unwrap();
    let JointOperands::Grounded { connector, .. } = ground.operands() else {
        panic!("ground")
    };
    assert_eq!(
        connector.operand.object,
        "fcstd:native:object#BracketOccurrence"
    );
    assert!(
        matches!(&connector.operand.container, OperandContainer::Occurrence { occurrence: id, occurrence_path } if id == &occurrence("BracketOccurrence", "instance").id && occurrence_path.is_empty())
    );
    assert!((connector.frame.affine_rows()[0][3] - 10.0).abs() < LENGTH_TOLERANCE);
}

#[test]
fn authored_assembly_prototypes_match_source_dimensions() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_measure::{MeasureInput, MeasureOptions, Measurer, Selection};
    const VOLUME_TOLERANCE: f64 = 1.0e-5;
    const AREA_TOLERANCE: f64 = 1.0e-5;
    const BOX_TOLERANCE: f64 = 1.0e-6;
    let data = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/product_assembly.FCStd");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .unwrap();
    let model = &decoded.ir().model;
    let pi = std::f64::consts::PI;
    let expected = [
        (
            "BracketPrototype",
            20.0 * 12.0 * 6.0,
            2.0 * (20.0 * 12.0 + 20.0 * 6.0 + 12.0 * 6.0),
            [0.0, 0.0, 0.0, 20.0, 12.0, 6.0],
        ),
        (
            "PinPrototype",
            pi * 3.0 * 3.0 * 20.0,
            2.0 * pi * 3.0 * (3.0 + 20.0),
            [-3.0, -3.0, 0.0, 3.0, 3.0, 20.0],
        ),
    ];
    let bodies = expected
        .iter()
        .map(|(name, _, _, _)| {
            let prototype = model
                .product_definitions
                .iter()
                .find(|p| p.source_name.as_deref() == Some(name))
                .unwrap();
            let [body] = prototype.bodies.as_slice() else {
                panic!("{name}: body")
            };
            body.clone()
        })
        .collect::<Vec<_>>();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
    let report = Measurer::new(
        MeasureInput {
            ir: decoded.ir(),
            fidelity: None,
            geometry_transferred: true,
        },
        MeasureOptions::default(),
    )
    .measure(
        &ctx,
        &Selection {
            bodies: bodies.clone(),
            ..Selection::default()
        },
    )
    .unwrap();
    for ((name, volume, area, bounds), id) in expected.into_iter().zip(bodies) {
        let body = report
            .bodies()
            .iter()
            .find(|body| body.body() == id.as_str())
            .unwrap();
        assert!(
            (body
                .volume()
                .as_ref()
                .unwrap()
                .measured()
                .unwrap()
                .value()
                .value
                .get()
                - volume)
                .abs()
                < VOLUME_TOLERANCE,
            "{name}"
        );
        assert!(
            (body
                .area()
                .as_ref()
                .unwrap()
                .measured()
                .unwrap()
                .value()
                .value
                .get()
                - area)
                .abs()
                < AREA_TOLERANCE,
            "{name}"
        );
        let bbox = body.bbox().measured().unwrap().value();
        for (actual, expected) in bbox
            .min
            .coordinates()
            .into_iter()
            .chain(bbox.max.coordinates())
            .zip(bounds)
        {
            assert!((actual.get() - expected).abs() < BOX_TOLERANCE, "{name}");
        }
    }
    ctx.finish_session().unwrap();
}
