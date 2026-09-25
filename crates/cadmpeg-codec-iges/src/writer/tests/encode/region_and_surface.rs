use crate::test_support::test_owned::explicit_void_solid_file;
use crate::test_support::test_solids_and_structure::explicit_non_manifold_open_shell_file;
use crate::test_support::test_surface_fixtures::trimmed_plane_file;
use crate::IgesCodec;
use crate::IgesVersion;
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::Curve;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::CadIr;
use cadmpeg_ir::Codec;
use std::io::Cursor;

fn emitted_label(ir: &CadIr, entity_type: i64) -> String {
    let record = ir.native.namespace("iges").expect("IGES native").arenas()["entities"]
        .iter()
        .find(|record| {
            record.field("entity_type").and_then(|value| value.as_i64()) == Some(entity_type)
        })
        .expect("owning entity");
    let bytes = record
        .field("label")
        .and_then(|value| value.as_array().cloned())
        .expect("Directory label")
        .iter()
        .map(|value| u8::try_from(value.as_u64().expect("label byte")).expect("ASCII byte"))
        .collect::<Vec<_>>();
    String::from_utf8(bytes)
        .expect("ASCII label")
        .trim()
        .to_owned()
}

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
        .find(|shell| Some(&shell.id) == source_region.shells.get(1))
        .unwrap()
        .faces()
        .iter()
        .all(|face_id| decoded
            .ir()
            .model
            .faces
            .iter()
            .find(|face| face.id == *face_id)
            .is_some_and(|face| face.sense == Sense::Reversed)));
    let plan = IgesCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit(IgesVersion::V5_3.descriptor().id.as_str()),
        )
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
    assert!(void_shell.faces().iter().all(|face_id| {
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
fn synthesized_solid_reports_long_name_and_preserves_color_visibility() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_void_solid_file().0),
            &DecodeOptions::default(),
        )
        .expect("source solid");
    let mut ir = decoded.ir().clone();
    let body = ir.model.bodies.first_mut().expect("body");
    body.name = Some("Hidden Red Body".into());
    body.color = cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0);
    body.visible = Some(false);

    let generated = crate::writer::synthesize(&ir, IgesVersion::V5_3).expect("synthesis");
    assert!(generated.losses.iter().any(|loss| {
        loss.code == crate::loss::IgesLossCode::WriterBodyNameNotRepresented.kind()
            && loss.message.contains("Hidden Red Body")
    }));
    let round_trip = IgesCodec
        .decode(&mut Cursor::new(generated.bytes), &DecodeOptions::default())
        .expect("generated IGES decodes");
    let body = round_trip
        .ir()
        .model
        .bodies
        .first()
        .expect("round-trip body");
    assert_eq!(emitted_label(round_trip.ir(), 186), "SOLID");
    assert_eq!(body.color, ir.model.bodies[0].color);
    assert_eq!(body.visible, Some(false));
}

#[test]
fn synthesized_solid_writes_short_body_name_to_directory() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_void_solid_file().0),
            &DecodeOptions::default(),
        )
        .expect("source solid");
    let mut ir = decoded.ir().clone();
    ir.model.bodies[0].name = Some("RED_BODY".into());
    let generated = crate::writer::synthesize(&ir, IgesVersion::V5_3).expect("synthesis");
    assert!(!generated.losses.iter().any(|loss| {
        loss.code == crate::loss::IgesLossCode::WriterBodyNameNotRepresented.kind()
    }));
    let round_trip = IgesCodec
        .decode(&mut Cursor::new(generated.bytes), &DecodeOptions::default())
        .expect("generated IGES decodes");
    assert_eq!(emitted_label(round_trip.ir(), 186), "RED_BODY");
}

#[test]
fn synthesized_solid_writes_custom_rgb_and_reports_opacity() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_void_solid_file().0),
            &DecodeOptions::default(),
        )
        .expect("source solid");
    let mut ir = decoded.ir().clone();
    ir.model.bodies[0].color = cadmpeg_ir::topology::Color::new(0.2, 0.4, 0.6, 0.5);
    let generated = crate::writer::synthesize(&ir, IgesVersion::V5_3).expect("synthesis");
    assert!(!generated.losses.iter().any(|loss| {
        loss.code == crate::loss::IgesLossCode::WriterBodyColorNotRepresented.kind()
    }));
    assert!(generated.losses.iter().any(|loss| {
        loss.code == crate::loss::IgesLossCode::WriterBodyOpacityNotRepresented.kind()
            && loss.message.contains(ir.model.bodies[0].id.as_str())
    }));
    assert_eq!(generated.counts.get("314_color_definition"), Some(&1));
    let round_trip = IgesCodec
        .decode(&mut Cursor::new(generated.bytes), &DecodeOptions::default())
        .expect("generated IGES decodes");
    assert_eq!(
        round_trip.ir().model.bodies[0].color,
        cadmpeg_ir::topology::Color::new(0.2, 0.4, 0.6, 1.0)
    );
    assert!(!round_trip.report().losses.iter().any(|loss| {
        loss.message
            .contains("Directory color number or definition pointer is invalid")
            || loss
                .message
                .contains("color definition Directory fields are invalid")
    }));
}

#[test]
fn synthesized_trimmed_sheet_writes_custom_rgb_in_legacy_versions() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_plane_file()),
            &DecodeOptions::default(),
        )
        .expect("source sheet");
    let mut ir = decoded.ir().clone();
    let body = ir
        .model
        .bodies
        .iter_mut()
        .find(|body| body.kind == BodyKind::Sheet)
        .expect("sheet body");
    body.color = cadmpeg_ir::topology::Color::new(0.2, 0.4, 0.6, 1.0);
    for version in [IgesVersion::V4_0, IgesVersion::V5_0] {
        let generated = crate::writer::synthesize(&ir, version).expect("synthesis");
        assert_eq!(generated.counts.get("314_color_definition"), Some(&1));
        let round_trip = IgesCodec
            .decode(&mut Cursor::new(generated.bytes), &DecodeOptions::default())
            .expect("generated IGES decodes");
        let body = round_trip
            .ir()
            .model
            .bodies
            .iter()
            .find(|body| body.kind == BodyKind::Sheet)
            .expect("round-trip sheet");
        assert_eq!(
            body.color,
            cadmpeg_ir::topology::Color::new(0.2, 0.4, 0.6, 1.0)
        );
    }
}

#[test]
fn synthesized_trimmed_sheet_presents_owning_body() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_plane_file()),
            &DecodeOptions::default(),
        )
        .expect("source sheet");
    let mut ir = decoded.ir().clone();
    let body = ir
        .model
        .bodies
        .iter_mut()
        .find(|body| body.kind == BodyKind::Sheet)
        .expect("sheet body");
    body.name = Some("SHEET_A".into());
    body.color = cadmpeg_ir::topology::Color::new(0.0, 1.0, 0.0, 1.0);
    body.visible = Some(false);
    let generated = crate::writer::synthesize(&ir, IgesVersion::V5_3).expect("synthesis");
    let round_trip = IgesCodec
        .decode(&mut Cursor::new(generated.bytes), &DecodeOptions::default())
        .expect("generated IGES decodes");
    assert_eq!(emitted_label(round_trip.ir(), 144), "SHEET_A");
    let body = round_trip
        .ir()
        .model
        .bodies
        .iter()
        .find(|body| body.kind == BodyKind::Sheet)
        .expect("round-trip sheet");
    assert_eq!(
        body.color,
        ir.model
            .bodies
            .iter()
            .find(|body| body.kind == BodyKind::Sheet)
            .expect("source sheet")
            .color
    );
    assert_eq!(body.visible, Some(false));
}

#[test]
fn synthesized_brep_sheet_presents_owning_shell() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_non_manifold_open_shell_file()),
            &DecodeOptions::default(),
        )
        .expect("source shell");
    let mut ir = decoded.ir().clone();
    let body = ir
        .model
        .bodies
        .iter_mut()
        .find(|body| body.kind == BodyKind::Sheet)
        .expect("sheet body");
    body.name = Some("SHELL_A".into());
    body.color = cadmpeg_ir::topology::Color::new(0.0, 0.0, 1.0, 1.0);
    body.visible = Some(false);
    let generated = crate::writer::synthesize(&ir, IgesVersion::V5_3).expect("synthesis");
    let round_trip = IgesCodec
        .decode(&mut Cursor::new(generated.bytes), &DecodeOptions::default())
        .expect("generated IGES decodes");
    assert_eq!(emitted_label(round_trip.ir(), 514), "SHELL_A");
    let body = round_trip
        .ir()
        .model
        .bodies
        .iter()
        .find(|body| body.kind == BodyKind::Sheet)
        .expect("round-trip sheet");
    assert_eq!(
        body.color,
        ir.model
            .bodies
            .iter()
            .find(|body| body.kind == BodyKind::Sheet)
            .expect("source sheet")
            .color
    );
    assert_eq!(body.visible, Some(false));
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

    let entities = crate::writer::brep_entities(
        crate::writer::validate_brep_topology(&ir, crate::IgesVersion::V5_3).unwrap(),
        &mut std::collections::BTreeMap::new(),
        &mut Vec::new(),
    )
    .unwrap();
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
    assert_eq!(String::from_utf8(solid.parameter_text()).unwrap(), expected);
}

#[test]
fn encode_nurbs_declares_actual_planarity_and_closedness() {
    let cases = [
        (
            "planar-open",
            NurbsCurve::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 2.0, 2.0],
                vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(2.0, 0.0, 0.0),
                ],
                None,
                false,
            )
            .expect("valid planar-open NURBS"),
            [0, 0, 1, 0],
        ),
        (
            "unique-planar-open",
            NurbsCurve::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 2.0, 2.0],
                vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(1.0, 1.0, 0.0),
                ],
                None,
                false,
            )
            .expect("valid unique-planar-open NURBS"),
            [1, 0, 1, 0],
        ),
        (
            "nonplanar-open",
            NurbsCurve::from_lanes(
                2,
                vec![0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0],
                vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 1.0),
                    Point3::new(2.0, 1.0, 0.0),
                    Point3::new(3.0, 0.0, 0.0),
                ],
                None,
                false,
            )
            .expect("valid nonplanar-open NURBS"),
            [0, 0, 1, 0],
        ),
        (
            "closed-planar",
            NurbsCurve::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 2.0, 2.0],
                vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(0.0, 0.0, 0.0),
                ],
                None,
                false,
            )
            .expect("valid closed-planar NURBS"),
            [0, 1, 1, 0],
        ),
        (
            "equal-weight-rational",
            NurbsCurve::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                Some(vec![2.0, 2.0]),
                false,
            )
            .expect("valid equal-weight rational NURBS"),
            [0, 0, 1, 0],
        ),
    ];
    for (name, nurbs, expected) in cases {
        let mut ir = CadIr::empty();
        ir.model.curves.push(Curve {
            id: CurveId::mint(format!("test:model:curve#{name}")).expect("identity grammar"),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)),
            source_object: None,
        });
        let plan = IgesCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(IgesVersion::V5_3.descriptor().id.as_str()),
            )
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let mut written = Vec::new();
        plan.write_to(&mut written)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let decoded = IgesCodec
            .decode(&mut Cursor::new(written), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let entity = decoded.ir().native.namespace("iges").unwrap().arenas()["entities"]
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
