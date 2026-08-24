use super::*;

#[test]
fn encode_regenerates_decoded_brep_void_shell_without_source_bytes() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_void_solid_file().0),
            &DecodeOptions::default(),
        )
        .unwrap();
    let source_region = &decoded.ir().model.regions[0];
    assert_eq!(source_region.shells.len(), 2);
    assert!(decoded
        .ir()
        .model
        .shells
        .iter()
        .find(|shell| Some(&shell.id) == source_region.void_shells().next())
        .unwrap()
        .faces
        .iter()
        .all(|face_id| decoded
            .ir()
            .model
            .faces
            .iter()
            .find(|face| face.id == *face_id)
            .is_some_and(|face| face.sense == Sense::Reversed)));
    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: None,
        })
        .unwrap();
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();

    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    let body = round_trip
        .ir()
        .model
        .bodies
        .iter()
        .find(|body| body.kind == BodyKind::Solid)
        .unwrap();
    let region = round_trip
        .ir()
        .model
        .regions
        .iter()
        .find(|region| region.id == body.regions[0])
        .unwrap();
    assert_eq!(region.shells.len(), 2);
    let void_shell = round_trip
        .ir()
        .model
        .shells
        .iter()
        .find(|shell| shell.id == region.shells[1])
        .unwrap();
    assert!(void_shell.faces.iter().all(|face_id| {
        round_trip
            .ir()
            .model
            .faces
            .iter()
            .find(|face| face.id == *face_id)
            .is_some_and(|face| face.sense == Sense::Reversed)
    }));
    assert!(
        round_trip.report().losses.is_empty(),
        "{:#?}",
        round_trip.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn encode_type_186_uses_ordered_region_shell_roles() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_void_solid_file().0),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    let region = &mut ir.model.regions[0];
    let source_outer = region.shells[0].clone();
    let source_void = region.shells[1].clone();
    region.shells.reverse();

    let (exterior, voids) = crate::writer::solid_shell_roles(region).unwrap();
    assert_eq!(exterior, &source_void);
    assert_eq!(voids, std::slice::from_ref(&source_outer));

    let entities = crate::writer::brep_entities(&ir).unwrap();
    let shell_indices = entities
        .iter()
        .enumerate()
        .filter_map(|(index, entity)| (entity.type_code == 514).then_some(index))
        .collect::<Vec<_>>();
    assert_eq!(shell_indices.len(), 2);
    let solid = entities
        .iter()
        .find(|entity| entity.type_code == 186)
        .unwrap();
    let expected = format!(
        "186,{},1,1,{},1;",
        crate::writer::reference_marker(shell_indices[0]),
        crate::writer::reference_marker(shell_indices[1])
    );
    assert_eq!(
        String::from_utf8(solid.parameters.clone()).unwrap(),
        expected
    );
}

#[test]
fn encode_nurbs_declares_actual_planarity_and_closedness() {
    let cases = [
        (
            "planar-open",
            NurbsCurve {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 2.0, 2.0],
                control_points: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(2.0, 0.0, 0.0),
                ],
                weights: None,
                periodic: false,
            },
            [0, 0, 1, 0],
        ),
        (
            "unique-planar-open",
            NurbsCurve {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 2.0, 2.0],
                control_points: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(1.0, 1.0, 0.0),
                ],
                weights: None,
                periodic: false,
            },
            [1, 0, 1, 0],
        ),
        (
            "nonplanar-open",
            NurbsCurve {
                degree: 2,
                knots: vec![0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0],
                control_points: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 1.0),
                    Point3::new(2.0, 1.0, 0.0),
                    Point3::new(3.0, 0.0, 0.0),
                ],
                weights: None,
                periodic: false,
            },
            [0, 0, 1, 0],
        ),
        (
            "closed-planar",
            NurbsCurve {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 2.0, 2.0],
                control_points: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(0.0, 0.0, 0.0),
                ],
                weights: None,
                periodic: false,
            },
            [0, 1, 1, 0],
        ),
        (
            "equal-weight-rational",
            NurbsCurve {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                weights: Some(vec![2.0, 2.0]),
                periodic: false,
            },
            [0, 0, 1, 0],
        ),
    ];
    for (name, nurbs, expected) in cases {
        let mut ir = CadIr::empty(Units::default());
        ir.model.curves.push(Curve {
            id: CurveId(format!("curve#{name}")),
            geometry: CurveGeometry::Nurbs(nurbs),
            source_object: None,
        });
        let plan = IgesEncoder::default()
            .plan(EncodeInput {
                ir: &ir,
                fidelity: None,
            })
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let mut written = Vec::new();
        plan.write_to(&mut written)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let decoded = IgesCodec
            .decode(&mut Cursor::new(written), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let entity = decoded.ir().native.namespace("iges").unwrap().arenas["entities"]
            .iter()
            .find(|record| {
                record.field("entity_type").and_then(|value| value.as_i64()) == Some(126)
            })
            .unwrap_or_else(|| panic!("{name}: missing Type 126 entity"));
        let fields = entity.fields();
        let parameters = fields["parameters"].as_array().unwrap();
        for (index, expected_value) in expected.into_iter().enumerate() {
            assert_eq!(
                parameters[index + 3]["value"]["value"].as_i64(),
                Some(i64::from(expected_value)),
                "{name}: Type 126 property {}",
                index + 1
            );
        }
        if expected[0] == 1 {
            let normal = parameters
                .get(parameters.len().saturating_sub(3)..)
                .unwrap_or_default()
                .iter()
                .map(|parameter| parameter["value"]["value"].as_f64())
                .collect::<Option<Vec<_>>>()
                .unwrap_or_else(|| panic!("{name}: missing Type 126 plane normal"));
            assert_eq!(normal, vec![0.0, 0.0, 1.0], "{name}: Type 126 plane normal");
        } else {
            let normal = parameters
                .get(parameters.len().saturating_sub(3)..)
                .unwrap_or_default()
                .iter()
                .map(|parameter| parameter["value"]["value"].as_f64())
                .collect::<Option<Vec<_>>>()
                .unwrap_or_else(|| panic!("{name}: missing Type 126 ignored normal fields"));
            assert_eq!(
                normal,
                vec![0.0, 0.0, 0.0],
                "{name}: Type 126 ignored normal"
            );
        }
        assert!(
            decoded.report().losses.is_empty(),
            "{name}: {:?}",
            decoded.report().losses
        );
    }
}
