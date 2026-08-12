// SPDX-License-Identifier: Apache-2.0
//! Curves-domain synthetic tests and fixtures.

use super::*;

#[test]
fn transform_decodes_column_major_basis_and_scaled_translation() {
    use cadmpeg_asm::sab::{Record, Token};

    let record = Record {
        index: 0,
        name: "transform".into(),
        head: "transform".into(),
        tokens: vec![
            Token::Vector3([1.0, 0.0, 0.0]),
            Token::Vector3([0.0, 1.0, 0.0]),
            Token::Vector3([0.0, 0.0, 1.0]),
            Token::Position([1.0, 2.0, 3.0]),
            Token::Double(1.0),
        ]
        .into(),
        offset: 0,
        len: 0,
    };
    let transform = cadmpeg_asm::brep::attributes::decode_transform(&record, 60.0).unwrap();
    assert_eq!(transform.rows[0], [1.0, 0.0, 0.0, 600.0]);
    assert_eq!(transform.rows[1], [0.0, 1.0, 0.0, 1200.0]);
    assert_eq!(transform.rows[2], [0.0, 0.0, 1.0, 1800.0]);
    assert_eq!(transform.rows[3], [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn nurbs_curve_block_decodes_to_carrier() {
    use cadmpeg_asm::nurbs::core::decode_curve_cache;

    // A degree-2 nubs curve with two unique knots at stored multiplicity 2:
    // sum(mults) 4, n_poles = 4 - (degree - 1) = 3.
    let mut b = Vec::new();
    b.extend_from_slice(b"\x0d\x04nubs");
    push_tagged_i64(&mut b, 0x04, 2); // degree
    push_tagged_i64(&mut b, 0x15, 0); // closure = open
    push_tagged_i64(&mut b, 0x04, 2); // n_unique_knots
    for (k, m) in [(0.0, 2i64), (1.0, 2)] {
        push_tagged_f64(&mut b, k);
        push_tagged_i64(&mut b, 0x04, m);
    }
    for p in [[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [2.0, 0.0, 0.0]] {
        for c in p {
            push_tagged_f64(&mut b, c);
        }
    }

    let c = decode_curve_cache(&b).expect("curve block decodes");
    assert_eq!(c.degree, 2);
    assert_eq!(c.control_points.len(), 3);
    // Clamped knots: [0,0,0,1,1,1] (endpoint mult 2 + 1 = 3 each).
    assert_eq!(c.knots, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(c.control_points[1].x, 10.0);
    assert_eq!(c.control_points[1].y, 20.0);
    assert!(c.weights.is_none());
}

#[test]
fn decode_retains_generated_procedural_curve_fit_contract() {
    let f3d = f3d_with_smbh(&synthetic_geometry_with_procedural_curve_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    let procedural = result.ir.model.procedural_curves.first().unwrap();
    assert!(matches!(
        &procedural.definition,
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Unknown {
            native_kind: Some(native_kind),
            record: None,
        } if native_kind == "surf_surf_int_cur"
    ));
    assert_eq!(procedural.cache_fit_tolerance, Some(0.005));
    assert_eq!(result.ir.model.curves.len(), 1);
}

#[test]
fn decode_retains_generated_helix_construction() {
    use cadmpeg_ir::{geometry::ProceduralCurveDefinition, math::Point3};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_helix_curve_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated helix decode");
    let procedural = result
        .ir
        .model
        .procedural_curves
        .first()
        .expect("helix construction");
    let ProceduralCurveDefinition::Helix {
        angle_range,
        center,
        major,
        minor,
        pitch,
        apex_factor,
        axis,
    } = procedural.definition
    else {
        panic!("expected helix construction")
    };
    assert_eq!(angle_range, [0.0, std::f64::consts::TAU]);
    assert_eq!(center, Point3::new(10.0, 20.0, 30.0));
    assert_eq!(major, cadmpeg_ir::math::Vector3::new(20.0, 0.0, 0.0));
    assert_eq!(minor, cadmpeg_ir::math::Vector3::new(0.0, 20.0, 0.0));
    assert_eq!(pitch, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 40.0));
    assert_eq!(apex_factor, 0.25);
    assert_eq!(axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(procedural.cache_fit_tolerance, Some(0.005));

    let mut edited = result.ir.clone();
    edited.model.procedural_curves[0].definition = ProceduralCurveDefinition::Helix {
        angle_range: [-1.0, 7.0],
        center: Point3::new(12.0, 23.0, 34.0),
        major: cadmpeg_ir::math::Vector3::new(30.0, 0.0, 0.0),
        minor: cadmpeg_ir::math::Vector3::new(0.0, -30.0, 0.0),
        pitch: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 55.0),
        apex_factor: 0.5,
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
    };
    edited.model.procedural_curves[0].cache_fit_tolerance = Some(0.012);
    let solved_curve_id = edited.model.procedural_curves[0].curve.clone();
    let solved_curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == solved_curve_id)
        .expect("helix solved curve");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(solved_cache) = &mut solved_curve.geometry
    else {
        panic!("expected helix NURBS cache")
    };
    solved_cache.control_points[1].x = 17.0;
    solved_cache.control_points[1].z = -2.0;
    let edited_definition = edited.model.procedural_curves[0].definition.clone();
    let edited_cache = solved_curve.geometry.clone();
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
        .expect("helix definition regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated helix decode");
    assert_eq!(
        regenerated.ir.model.procedural_curves[0].definition,
        edited_definition
    );
    assert_eq!(
        regenerated.ir.model.procedural_curves[0].cache_fit_tolerance,
        Some(0.012)
    );
    assert!(regenerated
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve.geometry == edited_cache));

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = source_less.model.procedural_curves[0].definition.clone();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less helix encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less helix round trip");
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].definition,
        expected
    );
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].cache_fit_tolerance,
        Some(0.005)
    );
}

#[test]
fn cacheless_helix_construction_is_the_exact_edge_carrier() {
    use cadmpeg_ir::geometry::{CurveGeometry, ProceduralCurveDefinition};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_cacheless_helix_curve_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("cacheless helix decode");
    let procedural = result
        .ir
        .model
        .procedural_curves
        .first()
        .expect("helix construction");
    assert!(matches!(
        procedural.definition,
        ProceduralCurveDefinition::Helix { .. }
    ));
    assert_eq!(procedural.cache_fit_tolerance, None);
    assert!(matches!(
        result
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == procedural.curve)
            .map(|curve| &curve.geometry),
        Some(CurveGeometry::Procedural { construction }) if *construction == procedural.id
    ));
    let validation = cadmpeg_ir::validate::validate_neutral(&result.ir, Vec::new());
    assert!(
        validation.is_ok(),
        "validation findings: {:?}",
        validation.findings
    );
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("procedural intcurve")));

    let expected = procedural.definition.clone();
    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("cacheless helix source-less encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("cacheless helix source-less round trip");
    assert!(matches!(
        round_trip.ir.model.curves[0].geometry,
        CurveGeometry::Procedural { .. }
    ));
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].definition,
        expected
    );
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].cache_fit_tolerance,
        None
    );
}

#[test]
fn generated_law_intcurve_decodes_and_writes_recursive_formulas() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralCurveDefinition};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_law_curve_smbh())),
            &DecodeOptions::default(),
        )
        .expect("law intcurve decode");
    let procedural = decoded
        .ir
        .model
        .procedural_curves
        .iter()
        .find(|curve| matches!(curve.definition, ProceduralCurveDefinition::Law { .. }))
        .expect("law intcurve construction");
    let ProceduralCurveDefinition::Law {
        context,
        extension,
        primary,
        additional,
        ..
    } = &procedural.definition
    else {
        unreachable!()
    };
    assert_eq!(context.parameter_range, [-1.0, 2.0]);
    assert_eq!(*extension, 0);
    assert_eq!(primary.name, "primary_law");
    assert!(matches!(
        primary.variables[0],
        LawExpression::Edge { parameters, .. } if parameters == [-0.5, 1.5]
    ));
    assert_eq!(additional.len(), 2);

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less law intcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less law intcurve round trip");
    assert!(round_trip.ir.model.procedural_curves.iter().any(|curve| {
        matches!(
            &curve.definition,
            ProceduralCurveDefinition::Law { primary, .. }
                if matches!(primary.variables[0], LawExpression::Edge { .. })
        )
    }));
}

#[test]
fn generated_vector_offset_curve_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_vector_offset_curve_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("generated vector-offset decode");
    let procedural = &result.ir.model.procedural_curves[0];
    let ProceduralCurveDefinition::VectorOffset {
        source,
        parameter_range,
        offset,
        labels,
        codes,
    } = &procedural.definition
    else {
        panic!("expected vector offset construction")
    };
    assert_eq!(*parameter_range, [-2.0, 5.0]);
    assert_eq!(*offset, cadmpeg_ir::math::Vector3::new(5.0, -10.0, 20.0));
    assert_eq!(labels, &["source".to_string(), "offset".to_string()]);
    assert_eq!(*codes, [7, 9]);
    assert!(result
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *source));
    assert_eq!(procedural.cache_fit_tolerance, Some(0.008));
    let expected_range = *parameter_range;
    let expected_offset = *offset;
    let expected_labels = labels.clone();
    let expected_codes = *codes;

    let mut edited = result.ir.clone();
    let ProceduralCurveDefinition::VectorOffset {
        parameter_range,
        offset,
        ..
    } = &mut edited.model.procedural_curves[0].definition
    else {
        panic!("expected editable vector offset")
    };
    *parameter_range = [-3.0, 6.0];
    *offset = cadmpeg_ir::math::Vector3::new(8.0, -12.0, 25.0);
    edited.model.procedural_curves[0].cache_fit_tolerance = Some(0.015);
    let edited_definition = edited.model.procedural_curves[0].definition.clone();
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
        .expect("vector-offset regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated vector-offset decode");
    assert_eq!(
        regenerated.ir.model.procedural_curves[0].definition,
        edited_definition
    );
    assert_eq!(
        regenerated.ir.model.procedural_curves[0].cache_fit_tolerance,
        Some(0.015)
    );

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let source_id = match &source_less.model.procedural_curves[0].definition {
        ProceduralCurveDefinition::VectorOffset { source, .. } => source.clone(),
        _ => unreachable!(),
    };
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == source_id)
        .expect("vector-offset source carrier")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(-5.0, 4.0, 2.0),
        direction: cadmpeg_ir::math::Vector3::new(2.0, 1.0, -0.5),
    };
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less vector-offset encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less vector-offset round trip");
    let ProceduralCurveDefinition::VectorOffset {
        source,
        parameter_range,
        offset,
        labels,
        codes,
    } = &round_trip.ir.model.procedural_curves[0].definition
    else {
        panic!("expected round-trip vector offset")
    };
    assert_eq!(*parameter_range, expected_range);
    assert_eq!(*offset, expected_offset);
    assert_eq!(*labels, expected_labels);
    assert_eq!(*codes, expected_codes);
    assert!(round_trip
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *source));
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].cache_fit_tolerance,
        Some(0.008)
    );
    assert!(matches!(
        round_trip
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *source)
            .map(|curve| &curve.geometry),
        Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
            if curve.degree == 1
                && curve.knots == [-2.0, -2.0, 5.0, 5.0]
                && curve.control_points == [
                    cadmpeg_ir::math::Point3::new(-9.0, 2.0, 3.0),
                    cadmpeg_ir::math::Point3::new(5.0, 9.0, -0.5),
                ]
    ));
}

#[test]
fn generated_subset_curve_decodes_edits_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_subset_curve_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated subset decode");
    let ProceduralCurveDefinition::Subset {
        source,
        parameter_range,
        sense: _,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected subset construction")
    };
    assert_eq!(*parameter_range, [-1.5, 3.5]);
    assert!(result
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *source));
    assert!(
        (result.ir.model.procedural_curves[0]
            .cache_fit_tolerance
            .expect("subset fit tolerance")
            - 0.006)
            .abs()
            < 1e-12
    );

    let mut edited = result.ir.clone();
    let ProceduralCurveDefinition::Subset {
        parameter_range, ..
    } = &mut edited.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    *parameter_range = [-2.0, 4.0];
    let expected_edit = edited.model.procedural_curves[0].definition.clone();
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
        .expect("subset regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated subset decode");
    assert_eq!(
        regenerated.ir.model.procedural_curves[0].definition,
        expected_edit
    );

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let source_id = match &source_less.model.procedural_curves[0].definition {
        ProceduralCurveDefinition::Subset { source, .. } => source.clone(),
        _ => unreachable!(),
    };
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == source_id)
        .expect("subset source carrier")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0),
        direction: cadmpeg_ir::math::Vector3::new(1.0, -2.0, 0.5),
    };
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less subset encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less subset round trip");
    let ProceduralCurveDefinition::Subset {
        source,
        parameter_range,
        sense: _,
    } = &round_trip.ir.model.procedural_curves[0].definition
    else {
        panic!("expected round-trip subset")
    };
    assert_eq!(*parameter_range, [-1.5, 3.5]);
    assert!(round_trip
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *source));
    let source_curve = round_trip
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *source)
        .expect("round-trip subset source");
    assert_eq!(
        source_curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
            degree: 1,
            knots: vec![-1.5, -1.5, 3.5, 3.5],
            control_points: vec![
                cadmpeg_ir::math::Point3::new(8.5, 23.0, 29.25),
                cadmpeg_ir::math::Point3::new(13.5, 13.0, 31.75),
            ],
            weights: None,
            periodic: false,
        })
    );
}

#[test]
fn generated_exact_intcurve_preserves_native_construction_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_exact_curve_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated exact intcurve decode");
    assert_eq!(
        result.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::Exact
    );
    assert_eq!(
        result.ir.model.procedural_curves[0].cache_fit_tolerance,
        Some(0.004)
    );

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less exact intcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less exact intcurve round trip");
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::Exact
    );
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].cache_fit_tolerance,
        Some(0.004)
    );
}

#[test]
fn generated_spline_carriers_write_explicit_forward_sense() {
    for (smbh, head) in [
        (synthetic_geometry_with_exact_curve_smbh(), "intcurve"),
        (synthetic_exact_spl_sur_smbh("exact_spl_sur"), "spline"),
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("generated spline carrier decode");
        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();

        let mut encoded = Vec::new();
        F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &source_less,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less spline carrier encode");
        let mut archive = zip::ZipArchive::new(Cursor::new(&encoded)).expect("generated F3D ZIP");
        let mut generated_smbh = Vec::new();
        archive
            .by_name("FusionAssetName[Active]/Breps.BlobParts/BREP.generated.smbh")
            .expect("generated BREP stream")
            .read_to_end(&mut generated_smbh)
            .expect("generated BREP bytes");
        let record_start = generated_smbh
            .windows(b"\x0d\x09asmheader".len())
            .position(|window| window == b"\x0d\x09asmheader")
            .expect("generated ASM record table");
        let records =
            cadmpeg_asm::sab::frame(&generated_smbh, record_start, generated_smbh.len(), 8)
                .expect("generated ASM records must frame");
        let record = records
            .iter()
            .find(|record| record.head == head)
            .expect("generated spline carrier record");
        let subtype = record
            .tokens
            .iter()
            .position(|token| matches!(token, cadmpeg_asm::sab::Token::SubtypeOpen))
            .expect("spline carrier subtype scope");
        assert!(subtype > 0);
        assert_eq!(record.tokens[subtype - 1], cadmpeg_asm::sab::Token::False);
    }
}

#[test]
fn generated_intcurve_sense_uses_token_adjacent_to_subtype() {
    let decode_curve = |smbh: Vec<u8>| {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("generated exact intcurve decode");
        let curve_id = &result.ir.model.procedural_curves[0].curve;
        result
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *curve_id)
            .expect("exact intcurve carrier")
            .geometry
            .clone()
    };

    assert_eq!(
        decode_curve(synthetic_geometry_with_decoy_curve_sense_smbh()),
        decode_curve(synthetic_geometry_with_exact_curve_smbh())
    );
}

#[test]
fn generated_spline_surface_sense_uses_token_adjacent_to_subtype() {
    let decode_surface = |smbh: Vec<u8>| {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("generated exact spline-surface decode");
        let surface_id = &result.ir.model.procedural_surfaces[0].surface;
        let geometry = result
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *surface_id)
            .expect("exact spline-surface carrier")
            .geometry
            .clone();
        let face_sense = result
            .ir
            .model
            .faces
            .iter()
            .find(|face| face.surface == *surface_id)
            .expect("spline-surface face")
            .sense;
        (geometry, face_sense)
    };

    assert_eq!(
        decode_surface(synthetic_exact_spl_sur_with_decoy_sense_smbh()),
        decode_surface(synthetic_exact_spl_sur_smbh("exact_spl_sur"))
    );
}

#[test]
fn generated_legacy_intcurve_aliases_decode_and_write_canonically() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let cases = [
        with_legacy_subtype(
            synthetic_geometry_with_exact_curve_smbh(),
            "exact_int_cur",
            "exactcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_vector_offset_curve_smbh(),
            "offset_int_cur",
            "offsetintcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_subset_curve_smbh(),
            "subset_int_cur",
            "subsetintcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_analytic_offset_supports_smbh(),
            "off_int_cur",
            "offintcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_surface_offset_smbh(),
            "off_surf_int_cur",
            "offsurfintcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_projection_smbh(),
            "proj_int_cur",
            "projcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_surface_intersection_smbh(),
            "int_int_cur",
            "surfintcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_spring_smbh(),
            "spring_int_cur",
            "blndsprngcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_surface_curve_smbh("blend_int_cur"),
            "blend_int_cur",
            "bldcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_surface_curve_smbh("surf_int_cur"),
            "surf_int_cur",
            "surfcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_surface_curve_smbh("par_int_cur"),
            "par_int_cur",
            "parcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_surface_curve_smbh("skin_int_cur"),
            "skin_int_cur",
            "d5c2_cur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_silhouette_smbh("para_silh_int_cur", None),
            "para_silh_int_cur",
            "parasil",
        ),
    ];

    for bytes in cases {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&bytes)),
                &DecodeOptions::default(),
            )
            .expect("legacy intcurve alias decode");
        assert!(!matches!(
            result.ir.model.procedural_curves[0].definition,
            ProceduralCurveDefinition::Unknown { .. }
        ));
        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &source_less,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("canonical source-less intcurve encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("canonical intcurve round trip");
        assert!(!matches!(
            round_trip.ir.model.procedural_curves[0].definition,
            ProceduralCurveDefinition::Unknown { .. }
        ));
    }
}

#[test]
fn generated_compound_intcurve_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_compound_curve_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated compound intcurve decode");
    let ProceduralCurveDefinition::Compound {
        parameters,
        component_parameters,
        components,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected compound construction")
    };
    assert_eq!(parameters, &[0.0, 0.5, 1.0]);
    assert_eq!(component_parameters, &[-2.0, 4.0]);
    assert_eq!(components.len(), 2);
    assert!(components.iter().all(|component| result
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *component)));
    assert!(
        (result.ir.model.procedural_curves[0]
            .cache_fit_tolerance
            .expect("compound fit tolerance")
            - 0.003)
            .abs()
            < 1e-12
    );
    let component_ids = components.clone();

    let mut edited = result.ir.clone();
    let ProceduralCurveDefinition::Compound {
        parameters,
        component_parameters,
        ..
    } = &mut edited.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    *parameters = vec![-0.25, 0.75, 1.25];
    *component_parameters = vec![-3.0, 5.0];
    let expected_edit = edited.model.procedural_curves[0].definition.clone();
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
        .expect("compound intcurve regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated compound intcurve decode");
    assert_eq!(
        regenerated.ir.model.procedural_curves[0].definition,
        expected_edit
    );

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    for (ordinal, component) in component_ids.iter().enumerate() {
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == *component)
            .expect("compound component curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(ordinal as f64, -1.0, 2.0),
            direction: cadmpeg_ir::math::Vector3::new(2.0, 3.0, -4.0),
        };
    }
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less compound intcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less compound intcurve round trip");
    let ProceduralCurveDefinition::Compound {
        parameters,
        component_parameters,
        components,
    } = &round_trip.ir.model.procedural_curves[0].definition
    else {
        panic!("expected round-trip compound construction")
    };
    assert_eq!(parameters, &[0.0, 0.5, 1.0]);
    assert_eq!(component_parameters, &[-2.0, 4.0]);
    assert_eq!(components.len(), 2);
    for (ordinal, component) in components.iter().enumerate() {
        let curve = round_trip
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *component)
            .expect("round-trip compound component");
        let cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve) = &curve.geometry else {
            panic!("compound line component was not lowered to NURBS")
        };
        assert_eq!(curve.degree, 1);
        let range = [ordinal as f64 * 0.5, (ordinal + 1) as f64 * 0.5];
        assert_eq!(curve.knots, [range[0], range[0], range[1], range[1]]);
    }
}

#[test]
fn generated_two_sided_offset_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_two_sided_offset_curve_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("generated two-sided offset decode");
    let ProceduralCurveDefinition::TwoSidedOffset {
        context,
        discontinuity_flag,
        offsets,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected two-sided offset construction")
    };
    assert_eq!(context.parameter_range, [-1.0, 2.0]);
    assert!(*discontinuity_flag);
    assert_eq!(
        context.discontinuities,
        [vec![0.25, 0.75], vec![], vec![0.5]]
    );
    assert!(context
        .sides
        .iter()
        .all(|side| side.surface.is_none() && side.pcurve.is_none()));
    assert_eq!(*offsets, [-2.0, 4.0]);

    let mut edited = result.ir.clone();
    let ProceduralCurveDefinition::TwoSidedOffset {
        context,
        discontinuity_flag,
        offsets,
    } = &mut edited.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    context.parameter_range = [-2.0, 3.0];
    context.discontinuities = [vec![0.2, 0.8], vec![], vec![0.6]];
    *discontinuity_flag = false;
    *offsets = [-3.0, 5.0];
    let expected_edit = edited.model.procedural_curves[0].definition.clone();
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
        .expect("two-sided offset regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated two-sided offset decode");
    assert_eq!(
        regenerated.ir.model.procedural_curves[0].definition,
        expected_edit
    );

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less two-sided offset encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less two-sided offset round trip");
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].definition,
        source_less.model.procedural_curves[0].definition
    );
}

#[test]
fn generated_embedded_offset_supports_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::{PcurveGeometry, ProceduralCurveDefinition, SurfaceGeometry};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_embedded_offset_supports_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("embedded offset-support decode");
    let ProceduralCurveDefinition::TwoSidedOffset {
        context, offsets, ..
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected embedded two-sided offset")
    };
    assert_eq!(*offsets, [-1.0, 3.0]);
    for side in &context.sides {
        let surface_id = side.surface.as_ref().expect("embedded support surface");
        assert!(result.ir.model.surfaces.iter().any(|surface| {
            surface.id == *surface_id && matches!(surface.geometry, SurfaceGeometry::Nurbs(_))
        }));
        assert!(matches!(side.pcurve, Some(PcurveGeometry::Nurbs { .. })));
    }
    assert!(matches!(
        context.sides[1].pcurve,
        Some(PcurveGeometry::Nurbs {
            weights: Some(_),
            ..
        })
    ));

    let mut retained = result.ir.clone();
    let ProceduralCurveDefinition::TwoSidedOffset {
        context,
        discontinuity_flag,
        offsets,
    } = &mut retained.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    context.parameter_range = [-2.0, 5.0];
    for (side, discontinuities) in context.discontinuities.iter_mut().enumerate() {
        for (ordinal, value) in discontinuities.iter_mut().enumerate() {
            *value = 0.125 * (side + ordinal + 1) as f64;
        }
    }
    *discontinuity_flag = false;
    *offsets = [-2.5, 4.5];
    let expected_retained = retained.model.procedural_curves[0].definition.clone();
    let mut retained_bytes = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(
            &retained,
            &result.source_fidelity,
            &mut retained_bytes,
        )
        .expect("retained embedded offset-support edit");
    let retained_round_trip = F3dCodec
        .decode(&mut Cursor::new(retained_bytes), &DecodeOptions::default())
        .expect("retained embedded offset-support round trip");
    assert_eq!(
        retained_round_trip.ir.model.procedural_curves[0].definition,
        expected_retained
    );

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut expected = source_less.model.procedural_curves[0].definition.clone();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less embedded offset-support encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less embedded offset-support round trip");
    let ProceduralCurveDefinition::TwoSidedOffset {
        context: expected_context,
        ..
    } = &mut expected
    else {
        unreachable!()
    };
    let ProceduralCurveDefinition::TwoSidedOffset {
        context: actual_context,
        ..
    } = &round_trip.ir.model.procedural_curves[0].definition
    else {
        panic!("expected round-trip embedded offset supports")
    };
    for side in 0..2 {
        let expected_surface = source_less
            .model
            .surfaces
            .iter()
            .find(|surface| Some(&surface.id) == expected_context.sides[side].surface.as_ref())
            .expect("source support surface");
        let actual_surface = round_trip
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| Some(&surface.id) == actual_context.sides[side].surface.as_ref())
            .expect("round-trip support surface");
        assert_eq!(actual_surface.geometry, expected_surface.geometry);
        expected_context.sides[side].surface = actual_context.sides[side].surface.clone();
    }
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].definition,
        expected
    );
}

#[test]
fn generated_mixed_offset_supports_write_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_embedded_offset_supports_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("generated embedded offset-support decode");
    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let ProceduralCurveDefinition::TwoSidedOffset { context, .. } =
        &mut source_less.model.procedural_curves[0].definition
    else {
        panic!("expected two-sided offset construction")
    };
    context.sides[1].surface = None;
    context.sides[1].pcurve = None;
    context.sides[0].pcurve = Some(cadmpeg_ir::geometry::PcurveGeometry::Line {
        origin: cadmpeg_ir::math::Point2::new(1.0, 2.0),
        direction: cadmpeg_ir::math::Point2::new(3.0, -1.0),
    });
    let first_support = context.sides[0]
        .surface
        .clone()
        .expect("retained first support id");
    let expected_surface = source_less
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == first_support)
        .expect("retained first support")
        .geometry
        .clone();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less mixed offset-support encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less mixed offset-support round trip");
    let ProceduralCurveDefinition::TwoSidedOffset { context, .. } =
        &round_trip.ir.model.procedural_curves[0].definition
    else {
        panic!("expected round-trip two-sided offset construction")
    };
    assert!(context.sides[1].surface.is_none() && context.sides[1].pcurve.is_none());
    assert_eq!(
        context.sides[0].pcurve,
        Some(cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![
                cadmpeg_ir::math::Point2::new(1.0, 2.0),
                cadmpeg_ir::math::Point2::new(4.0, 1.0),
            ],
            weights: None,
            periodic: false,
        })
    );
    let actual_surface = round_trip
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| Some(&surface.id) == context.sides[0].surface.as_ref())
        .expect("round-trip first support");
    assert_eq!(actual_surface.geometry, expected_surface);
}

#[test]
fn generated_analytic_offset_supports_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SurfaceGeometry};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_analytic_offset_supports_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("analytic offset-support decode");
    let ProceduralCurveDefinition::TwoSidedOffset {
        context, offsets, ..
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected analytic two-sided offset")
    };
    assert_eq!(*offsets, [-1.5, 2.5]);
    let supports = context.sides.each_ref().map(|side| {
        result
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| Some(&surface.id) == side.surface.as_ref())
            .expect("analytic support surface")
            .geometry
            .clone()
    });
    assert!(matches!(
        supports[0],
        SurfaceGeometry::Cone {
            radius: 10.0,
            ratio: 0.4,
            half_angle,
            axis,
            ..
        } if (half_angle - std::f64::consts::FRAC_PI_6).abs() < 1.0e-12
            && axis == cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0)
    ));
    assert!(matches!(
        supports[1],
        SurfaceGeometry::Torus {
            minor_radius: -7.5,
            ..
        }
    ));

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected_geometries = supports;
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less analytic offset-support encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less analytic offset-support round trip");
    let ProceduralCurveDefinition::TwoSidedOffset {
        context, offsets, ..
    } = &round_trip.ir.model.procedural_curves[0].definition
    else {
        panic!("expected round-trip analytic offset supports")
    };
    assert_eq!(*offsets, [-1.5, 2.5]);
    for (side, expected) in context.sides.iter().zip(expected_geometries) {
        let actual = round_trip
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| Some(&surface.id) == side.surface.as_ref())
            .expect("round-trip analytic support surface");
        assert_eq!(actual.geometry, expected);
    }
}

#[test]
fn generated_surface_intersection_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SurfaceGeometry};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_surface_intersection_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("surface intersection decode");
    let ProceduralCurveDefinition::Intersection {
        context,
        discontinuity_flag,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected surface intersection")
    };
    assert!(*discontinuity_flag);
    let expected_geometries = context.sides.each_ref().map(|side| {
        result
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| Some(&surface.id) == side.surface.as_ref())
            .expect("intersection support surface")
            .geometry
            .clone()
    });
    assert!(matches!(
        expected_geometries[0],
        SurfaceGeometry::Cone { half_angle, .. }
            if (half_angle - std::f64::consts::FRAC_PI_6).abs() < 1.0e-12
    ));
    assert!(matches!(
        expected_geometries[1],
        SurfaceGeometry::Torus { .. }
    ));

    let mut edited = result.ir.clone();
    let ProceduralCurveDefinition::Intersection {
        context,
        discontinuity_flag,
    } = &mut edited.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    context.parameter_range = [-1.0, 2.0];
    *discontinuity_flag = false;
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
        .expect("intersection context regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated intersection decode");
    assert!(matches!(
        regenerated.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::Intersection {
            ref context,
            discontinuity_flag: false,
        } if context.parameter_range == [-1.0, 2.0]
    ));

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less surface intersection encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less surface intersection round trip");
    let ProceduralCurveDefinition::Intersection {
        context,
        discontinuity_flag,
    } = &round_trip.ir.model.procedural_curves[0].definition
    else {
        panic!("expected round-trip surface intersection")
    };
    assert!(*discontinuity_flag);
    for (side, expected) in context.sides.iter().zip(expected_geometries) {
        let actual = round_trip
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| Some(&surface.id) == side.surface.as_ref())
            .expect("round-trip intersection support");
        assert_eq!(actual.geometry, expected);
    }
}

#[test]
fn generated_projection_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, ProjectionTail};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_projection_smbh())),
            &DecodeOptions::default(),
        )
        .expect("projection decode");
    let ProceduralCurveDefinition::Projection {
        context,
        discontinuity_flag,
        source,
        tail,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected projection")
    };
    assert!(context.sides.iter().all(|side| side.surface.is_some()));
    assert!(*discontinuity_flag);
    assert!(result
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *source));
    assert_eq!(
        tail,
        &ProjectionTail::Ranged {
            flag: true,
            parameter_range: [-2.0, 3.0],
            role: "surf2".into(),
        }
    );

    let mut edited = result.ir.clone();
    let ProceduralCurveDefinition::Projection {
        context,
        discontinuity_flag,
        tail,
        ..
    } = &mut edited.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    context.parameter_range = [-1.0, 2.0];
    *discontinuity_flag = false;
    let ProjectionTail::Ranged {
        flag,
        parameter_range,
        role,
    } = tail
    else {
        unreachable!()
    };
    *flag = false;
    *parameter_range = [-4.0, 5.0];
    *role = "surf1".into();
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
        .expect("projection context regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated projection decode");
    assert!(matches!(
        regenerated.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::Projection {
            ref context,
            discontinuity_flag: false,
            tail: ProjectionTail::Ranged {
                flag: false,
                parameter_range: [-4.0, 5.0],
                ref role,
            },
            ..
        } if context.parameter_range == [-1.0, 2.0] && role == "surf1"
    ));

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less projection encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less projection round trip");
    let ProceduralCurveDefinition::Projection {
        discontinuity_flag,
        tail,
        ..
    } = &round_trip.ir.model.procedural_curves[0].definition
    else {
        panic!("expected round-trip projection")
    };
    assert!(*discontinuity_flag);
    assert_eq!(
        tail,
        &ProjectionTail::Ranged {
            flag: true,
            parameter_range: [-2.0, 3.0],
            role: "surf2".into(),
        }
    );
}

#[test]
fn generated_early_close_projection_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, ProjectionTail};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_early_close_projection_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("early-close projection decode");
    assert!(matches!(
        result.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::Projection {
            discontinuity_flag: true,
            tail: ProjectionTail::EarlyClose { flag: true },
            ..
        }
    ));

    let mut edited = result.ir.clone();
    let ProceduralCurveDefinition::Projection {
        tail: ProjectionTail::EarlyClose { flag },
        ..
    } = &mut edited.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    *flag = false;
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
        .expect("early-close projection regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated early-close projection decode");
    assert!(matches!(
        regenerated.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::Projection {
            tail: ProjectionTail::EarlyClose { flag: false },
            ..
        }
    ));

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less early-close projection encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less early-close projection round trip");
    assert!(matches!(
        round_trip.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::Projection {
            discontinuity_flag: true,
            tail: ProjectionTail::EarlyClose { flag: true },
            ..
        }
    ));
}

#[test]
fn generated_three_surface_intersection_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SurfaceGeometry};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_three_surface_intersection_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("three-surface intersection decode");
    let ProceduralCurveDefinition::ThreeSurfaceIntersection {
        context,
        selector,
        third,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected three-surface intersection")
    };
    assert_eq!(*selector, 7);
    assert!(context.sides.iter().all(|side| side.surface.is_some()));
    let third_surface = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| Some(&surface.id) == third.surface.as_ref())
        .expect("third support surface");
    assert!(matches!(
        third_surface.geometry,
        SurfaceGeometry::Sphere { radius: -12.5, .. }
    ));

    let mut edited = result.ir.clone();
    let ProceduralCurveDefinition::ThreeSurfaceIntersection {
        context, selector, ..
    } = &mut edited.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    context.parameter_range = [-1.0, 2.0];
    *selector = -4;
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
        .expect("three-surface intersection regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated three-surface intersection decode");
    assert!(matches!(
        regenerated.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::ThreeSurfaceIntersection {
            ref context,
            selector: -4,
            ..
        } if context.parameter_range == [-1.0, 2.0]
    ));

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less three-surface intersection encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less three-surface intersection round trip");
    let ProceduralCurveDefinition::ThreeSurfaceIntersection {
        selector, third, ..
    } = &round_trip.ir.model.procedural_curves[0].definition
    else {
        panic!("expected round-trip three-surface intersection")
    };
    assert_eq!(*selector, 7);
    let third_surface = round_trip
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| Some(&surface.id) == third.surface.as_ref())
        .expect("round-trip third support surface");
    assert!(matches!(
        third_surface.geometry,
        SurfaceGeometry::Sphere { radius: -12.5, .. }
    ));
}

#[test]
fn generated_prefix_only_surface_curves_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SurfaceCurveFamily};

    for (name, expected_family) in [
        ("blend_int_cur", SurfaceCurveFamily::Blend),
        ("surf_int_cur", SurfaceCurveFamily::SurfaceConstrained),
        ("par_int_cur", SurfaceCurveFamily::Parametric),
        ("skin_int_cur", SurfaceCurveFamily::Skin),
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_surface_curve_smbh(
                    name,
                ))),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{name} decode failed: {error}"));
        let ProceduralCurveDefinition::SurfaceCurve {
            family, context, ..
        } = &result.ir.model.procedural_curves[0].definition
        else {
            panic!("expected {name} surface curve")
        };
        assert_eq!(family, &expected_family);
        assert!(context.sides.iter().all(|side| side.surface.is_some()));

        let mut edited = result.ir.clone();
        let ProceduralCurveDefinition::SurfaceCurve { context, .. } =
            &mut edited.model.procedural_curves[0].definition
        else {
            unreachable!()
        };
        context.parameter_range = [-1.0, 2.0];
        let mut regenerated = Vec::new();
        F3dCodec
            .write_preserved_with_source_fidelity(
                &edited,
                &result.source_fidelity,
                &mut regenerated,
            )
            .unwrap_or_else(|error| panic!("{name} context regeneration failed: {error}"));
        let regenerated = F3dCodec
            .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("regenerated {name} decode failed: {error}"));
        assert!(matches!(
            regenerated.ir.model.procedural_curves[0].definition,
            ProceduralCurveDefinition::SurfaceCurve { ref context, .. }
                if context.parameter_range == [-1.0, 2.0]
        ));

        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &source_less,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut encoded))
            .unwrap_or_else(|error| panic!("{name} source-less encode failed: {error}"));
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{name} round trip failed: {error}"));
        assert!(matches!(
            &round_trip.ir.model.procedural_curves[0].definition,
            ProceduralCurveDefinition::SurfaceCurve { family, .. } if family == &expected_family
        ));
    }
}

#[test]
fn generated_silhouette_curves_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SilhouetteKind};

    for (name, draft_factor) in [
        ("silh_int_cur", None),
        ("para_silh_int_cur", None),
        ("taper_silh_int_cur", Some(0.35)),
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_silhouette_smbh(
                    name,
                    draft_factor,
                ))),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{name} decode failed: {error}"));
        let ProceduralCurveDefinition::Silhouette {
            silhouette,
            cast_surface,
            light_direction,
            ..
        } = &result.ir.model.procedural_curves[0].definition
        else {
            panic!("expected {name} silhouette")
        };
        assert!(result
            .ir
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == *cast_surface));
        assert_eq!(
            *light_direction,
            cadmpeg_ir::math::Vector3::new(0.0, -1.0, 0.0)
        );
        match (silhouette, draft_factor) {
            (SilhouetteKind::Standard, None) if name == "silh_int_cur" => {}
            (SilhouetteKind::Parametric, None) if name == "para_silh_int_cur" => {}
            (
                SilhouetteKind::Taper {
                    draft_factor: actual,
                },
                Some(expected),
            ) => {
                assert_eq!(*actual, expected);
            }
            _ => panic!("wrong silhouette family for {name}"),
        }

        let mut edited = result.ir.clone();
        let ProceduralCurveDefinition::Silhouette {
            silhouette,
            light_direction,
            ..
        } = &mut edited.model.procedural_curves[0].definition
        else {
            unreachable!()
        };
        *light_direction = cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0);
        if let SilhouetteKind::Taper { draft_factor } = silhouette {
            *draft_factor = -0.2;
        }
        let mut regenerated = Vec::new();
        F3dCodec
            .write_preserved_with_source_fidelity(
                &edited,
                &result.source_fidelity,
                &mut regenerated,
            )
            .unwrap_or_else(|error| panic!("{name} regeneration failed: {error}"));
        let regenerated = F3dCodec
            .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("regenerated {name} decode failed: {error}"));
        assert!(matches!(
            regenerated.ir.model.procedural_curves[0].definition,
            ProceduralCurveDefinition::Silhouette {
                ref silhouette,
                light_direction,
                ..
            } if light_direction == cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
                && match silhouette {
                    SilhouetteKind::Taper { draft_factor } => *draft_factor == -0.2,
                    _ => true,
                }
        ));

        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &source_less,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut encoded))
            .unwrap_or_else(|error| panic!("{name} source-less encode failed: {error}"));
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{name} round trip failed: {error}"));
        assert!(matches!(
            round_trip.ir.model.procedural_curves[0].definition,
            ProceduralCurveDefinition::Silhouette { .. }
        ));
    }
}

#[test]
fn generated_surface_offset_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_surface_offset_smbh())),
            &DecodeOptions::default(),
        )
        .expect("surface-offset decode");
    let ProceduralCurveDefinition::SurfaceOffset {
        context,
        discontinuity_flag,
        base_u_range,
        base_v_range,
        base,
        base_range,
        distance,
        shift,
        scale,
        ..
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected surface-offset construction")
    };
    assert_eq!(*base_u_range, [-1.0, 2.0]);
    assert_eq!(context.parameter_range, [0.0, 1.0]);
    assert!(*discontinuity_flag);
    assert_eq!(*base_v_range, [-3.0, 4.0]);
    assert_eq!(*base_range, [-0.5, 1.5]);
    assert_eq!((*distance, *shift, *scale), (-2.5, 0.75, 1.25));
    assert!(result.ir.model.curves.iter().any(|curve| curve.id == *base));

    let mut edited = result.ir.clone();
    let ProceduralCurveDefinition::SurfaceOffset {
        context,
        discontinuity_flag,
        base_u_range,
        base_v_range,
        base_range,
        distance,
        shift,
        scale,
        ..
    } = &mut edited.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    context.parameter_range = [-1.5, 2.5];
    *discontinuity_flag = false;
    *base_u_range = [-2.0, 5.0];
    *base_v_range = [-6.0, 7.0];
    *base_range = [-0.75, 1.75];
    (*distance, *shift, *scale) = (3.5, -0.25, 0.8);
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
        .expect("surface-offset scalar regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated surface-offset decode");
    assert!(matches!(
        regenerated.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::SurfaceOffset {
            ref context,
            discontinuity_flag: false,
            base_u_range: [-2.0, 5.0],
            base_v_range: [-6.0, 7.0],
            base_range: [-0.75, 1.75],
            distance: 3.5,
            shift: -0.25,
            scale: 0.8,
            ..
        } if context.parameter_range == [-1.5, 2.5]
    ));

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less surface-offset encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less surface-offset round trip");
    let ProceduralCurveDefinition::SurfaceOffset {
        discontinuity_flag,
        base_u_range,
        base_v_range,
        base_range,
        distance,
        shift,
        scale,
        ..
    } = &round_trip.ir.model.procedural_curves[0].definition
    else {
        panic!("expected round-trip surface offset")
    };
    assert_eq!(*base_u_range, [-1.0, 2.0]);
    assert!(*discontinuity_flag);
    assert_eq!(*base_v_range, [-3.0, 4.0]);
    assert_eq!(*base_range, [-0.5, 1.5]);
    assert_eq!((*distance, *shift, *scale), (-2.5, 0.75, 1.25));
}

#[test]
fn generated_spring_curve_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_spring_smbh())),
            &DecodeOptions::default(),
        )
        .expect("spring decode");
    let ProceduralCurveDefinition::Spring {
        context, direction, ..
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected spring construction")
    };
    assert_eq!(*direction, -3);
    assert!(context
        .sides
        .iter()
        .all(|side| side.surface.is_some() && side.pcurve.is_some()));

    let mut edited = result.ir.clone();
    let ProceduralCurveDefinition::Spring {
        context,
        discontinuity_flag,
        direction,
        ..
    } = &mut edited.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    context.parameter_range = [-2.0, 3.0];
    let expected_flag = !*discontinuity_flag;
    *discontinuity_flag = expected_flag;
    *direction = 4;
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
        .expect("spring tail regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated spring decode");
    assert!(matches!(
        regenerated.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::Spring {
            ref context,
            discontinuity_flag,
            direction: 4,
            ..
        } if discontinuity_flag == expected_flag && context.parameter_range == [-2.0, 3.0]
    ));

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less spring encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less spring round trip");
    assert!(matches!(
        round_trip.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::Spring { direction: -3, .. }
    ));
}

#[test]
fn generated_null_support_spring_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_null_support_spring_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("null-support spring decode");
    let ProceduralCurveDefinition::Spring {
        context,
        surface_parameter_ranges,
        first_pcurve_parameter_range,
        discontinuity_flag,
        cache_first,
        direction,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected spring construction")
    };
    assert_eq!(*cache_first, None);
    assert_eq!(*direction, 4);
    assert!(*discontinuity_flag);
    assert!(context
        .sides
        .iter()
        .all(|side| side.surface.is_none() && side.pcurve.is_none()));
    assert_eq!(
        surface_parameter_ranges[0],
        Some([[-2.0, 3.0], [-4.0, 5.0]])
    );
    assert_eq!(
        surface_parameter_ranges[1],
        Some([[-6.0, 7.0], [-8.0, 9.0]])
    );
    assert_eq!(*first_pcurve_parameter_range, Some([-10.0, 11.0]));
    assert_eq!(context.parameter_range, [-1.0, 2.0]);

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less null-support spring encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less null-support spring round trip");
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].definition,
        source_less.model.procedural_curves[0].definition
    );
}

#[test]
fn generated_deformable_curves_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::{DeformableCurveData, ProceduralCurveDefinition};

    for mode in [8, 3] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(
                    &synthetic_geometry_with_deformable_curve_smbh(mode),
                )),
                &DecodeOptions::default(),
            )
            .expect("deformable decode");
        let ProceduralCurveDefinition::Deformable {
            context,
            cache_first,
            source,
            source_parameter_range,
            data,
        } = &result.ir.model.procedural_curves[0].definition
        else {
            panic!("expected deformable construction")
        };
        let cadmpeg_ir::geometry::DeformableCurveSource::Curve { curve: source } = source else {
            panic!("expected resolved deformable source")
        };
        assert_eq!(cache_first.revision, 23100);
        assert_eq!(context.parameter_range, [-1.0, 2.0]);
        assert_eq!(*source_parameter_range, [Some(0.0), Some(1.0)]);
        assert!(result
            .ir
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *source));
        match (mode, data) {
            (
                8,
                DeformableCurveData::VectorField {
                    vectors,
                    parameter_pairs,
                },
            ) => {
                assert_eq!(vectors[3], cadmpeg_ir::math::Vector3::new(10.0, 11.0, 12.0));
                assert_eq!(parameter_pairs, &[[-1.0, 0.25], [2.0, 3.5]]);
            }
            (3, DeformableCurveData::Mode3 { trailing_value, .. }) => {
                assert_eq!(*trailing_value, 6);
            }
            _ => panic!("wrong deformable discriminator payload"),
        }
        let expected_data = data.clone();
        let source = source.clone();

        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == source)
            .expect("deformable source carrier")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(3.0, -2.0, 5.0),
            direction: cadmpeg_ir::math::Vector3::new(2.0, 4.0, -1.0),
        };
        let mut encoded = Vec::new();
        F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &source_less,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less deformable encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less deformable round trip");
        let ProceduralCurveDefinition::Deformable {
            source: round_source,
            data: round_data,
            ..
        } = &round_trip.ir.model.procedural_curves[0].definition
        else {
            panic!("expected round-trip deformable construction")
        };
        let cadmpeg_ir::geometry::DeformableCurveSource::Curve {
            curve: round_source,
        } = round_source
        else {
            panic!("expected resolved round-trip deformable source")
        };
        match (&expected_data, round_data) {
            (DeformableCurveData::VectorField { .. }, DeformableCurveData::VectorField { .. }) => {
                assert_eq!(round_data, &expected_data)
            }
            (DeformableCurveData::Mode3 { .. }, DeformableCurveData::Mode3 { .. }) => {
                assert_eq!(round_data, &expected_data)
            }
            _ => panic!("round-trip deformable discriminator changed"),
        }
        assert!(round_trip
            .ir
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *round_source));
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *round_source)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1
                    && curve.knots == [0.0, 0.0, 1.0, 1.0]
                    && curve.control_points == [
                        cadmpeg_ir::math::Point3::new(3.0, -2.0, 5.0),
                        cadmpeg_ir::math::Point3::new(5.0, 2.0, 4.0),
                    ]
        ));
    }

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_deformable_curve_smbh(8),
            )),
            &DecodeOptions::default(),
        )
        .expect("native-reference deformable decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let ProceduralCurveDefinition::Deformable { source, .. } =
        &mut source_less.model.procedural_curves[0].definition
    else {
        panic!("expected deformable construction")
    };
    *source = cadmpeg_ir::geometry::DeformableCurveSource::NativeReference {
        flag: false,
        index: 10_000,
    };
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("native-reference deformable encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("native-reference deformable round trip");
    assert!(matches!(
        &round_trip.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::Deformable {
            source: cadmpeg_ir::geometry::DeformableCurveSource::NativeReference {
                flag: false,
                index: 10_000,
            },
            ..
        }
    ));
}

#[test]
fn generated_f3d_rewrites_procedural_curve_fit_tolerance() {
    let source = f3d_with_smbh(&synthetic_geometry_with_procedural_curve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated procedural-curve decode");
    let mut edited = decoded.ir;
    edited.model.procedural_curves[0].cache_fit_tolerance = Some(0.025);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("procedural-curve fit regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated procedural-curve decode");
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].cache_fit_tolerance,
        Some(0.025)
    );
}

#[test]
fn generated_source_less_refuses_lossy_procedural_curve_fallbacks() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_procedural_curve_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("generated procedural curve decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    source_less.model.procedural_curves[0].definition = ProceduralCurveDefinition::BlendSpine {
        blend_surface: None,
    };
    let mut encoded = Vec::new();
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect_err("typed intersection must not degrade to a cache-only curve");
    assert!(error
        .to_string()
        .contains("lacks its native blend construction"));

    source_less.model.procedural_curves[0].definition = ProceduralCurveDefinition::Unknown {
        native_kind: None,
        record: None,
    };
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("unknown construction must not degrade to a cache-only curve");
    assert!(error
        .to_string()
        .contains("cannot be regenerated losslessly"));
}

#[test]
fn generated_source_less_rejects_duplicate_procedural_curve_owners() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_helix_curve_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated helix decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut duplicate = source_less.model.procedural_curves[0].clone();
    duplicate.id = "generated:duplicate-helix".into();
    source_less.model.procedural_curves.push(duplicate);
    let mut encoded = Vec::new();
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect_err("duplicate procedural construction must be rejected");
    assert!(error
        .to_string()
        .contains("multiple procedural constructions"));
}

#[test]
fn generated_f3d_rewrites_topology_bound_nurbs_curve() {
    let source = f3d_with_smbh(&synthetic_geometry_with_procedural_curve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated intcurve decode");
    let mut edited = decoded.ir;
    let curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id.as_str() == "f3d:brep:entity#19")
        .expect("topology-bound intcurve");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &mut curve.geometry else {
        panic!("expected NURBS edge carrier")
    };
    nurbs.control_points[1].x = 14.0;
    nurbs.control_points[1].z = -3.0;
    nurbs.degree = 1;
    nurbs.knots = vec![-1.0, -1.0, 2.0, 2.0, 2.0];
    let expected = curve.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("topology-bound NURBS regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated intcurve decode");
    assert!(round_trip
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve == &expected));
}

#[test]
fn nurbs_pcurve_block_decodes_without_length_scaling() {
    use cadmpeg_asm::nurbs::pcurve::decode_pcurve_cache;

    // A degree-1 2D pcurve. Unlike model-space NURBS control points, these
    // are UV parameters and therefore must not be converted from cm to mm.
    let b = generated_pcurve_block();

    let pcurve = decode_pcurve_cache(&b).expect("2D pcurve block decodes");
    assert_eq!(pcurve.degree, 1);
    assert_eq!(pcurve.knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(pcurve.control_points[0].u, 0.25);
    assert_eq!(pcurve.control_points[1].v, 1.5);
}

#[test]
fn ref_pcurve_resolves_intcurve_uv_slot() {
    let mut intcurve = generated_curve_block();
    intcurve.extend_from_slice(&generated_pcurve_block());

    let pcurve = cadmpeg_asm::nurbs::proc_curve::pcurve_for_selector_resolving_refs(
        &cadmpeg_asm::nurbs::toks::lex_test_span(&intcurve, 8),
        2,
        &cadmpeg_asm::nurbs::toks::test_table(&intcurve, 8),
    )
    .expect("intcurve slot 2 carries the UV cache");
    assert_eq!(pcurve.control_points[0].u, 0.25);
    assert_eq!(pcurve.control_points[1].v, 1.5);
    assert!(
        cadmpeg_asm::nurbs::proc_curve::pcurve_for_selector_resolving_refs(
            &cadmpeg_asm::nurbs::toks::lex_test_span(&intcurve, 8),
            1,
            &cadmpeg_asm::nurbs::toks::test_table(&intcurve, 8),
        )
        .is_none()
    );
}

#[test]
fn ref_pcurve_rejects_orphan_typed_slot() {
    let mut target = b"\x0f\x0d\x0bint_int_cur".to_vec();
    target.extend_from_slice(&generated_curve_block());
    target.extend_from_slice(&generated_pcurve_block());
    target.push(0x10);
    let mut source = b"\x0f\x0d\x03ref\x04".to_vec();
    source.extend_from_slice(&0i64.to_le_bytes());
    source.push(0x10);
    let mut active = target;
    active.extend_from_slice(&source);

    assert!(
        cadmpeg_asm::nurbs::proc_curve::pcurve_for_selector_resolving_refs(
            &cadmpeg_asm::nurbs::toks::lex_test_span(&source, 8),
            2,
            &cadmpeg_asm::nurbs::toks::test_table(&active, 8),
        )
        .is_none(),
        "a pcurve without its typed support surface is not a carrier"
    );
}

#[test]
fn decode_attaches_generated_pcurve_to_its_coedge() {
    let f3d = f3d_with_smbh(&synthetic_geometry_with_pcurve_smbh());
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.pcurves.len(), 1);
    assert_eq!(
        result
            .ir
            .model
            .coedges
            .iter()
            .filter(|c| !c.pcurves.is_empty())
            .count(),
        1
    );
    let report = cadmpeg_ir::validate::validate_neutral(&result.ir, Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
}

#[test]
fn inline_pcurve_scope_is_its_exact_carrier_identity() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_inline_pcurve_on_nurbs_surface_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("structurally unique inline pcurve decode");

    assert_eq!(result.ir.model.pcurves.len(), 1);
    assert_eq!(
        result
            .ir
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("explicit UV pcurve reference")));
}

#[test]
fn inline_pcurve_owns_its_carrier_ahead_of_referenced_supports() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_inline_pcurve_with_referenced_support_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("inline pcurve with referenced support");

    assert_eq!(result.ir.model.pcurves.len(), 1);
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("explicit UV pcurve reference")));
}

#[test]
fn wrapped_ref_pcurve_resolves_its_subtype_carrier() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_wrapped_ref_pcurve_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("wrapped ref pcurve decode");

    assert_eq!(result.ir.model.pcurves.len(), 1);
    assert_eq!(
        result
            .ir
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("explicit UV pcurve reference")));
}

#[test]
fn unique_bs2_intcurve_role_is_its_ref_pcurve_carrier() {
    for discriminator in [2, -2] {
        let smbh = with_pcurve_discriminator(
            synthetic_geometry_with_ref_pcurve_on_nurbs_surface_smbh(),
            discriminator,
        );
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("structurally unique ref pcurve decode");

        assert_eq!(result.ir.model.pcurves.len(), 1);
        assert_eq!(
            result
                .ir
                .model
                .coedges
                .iter()
                .filter(|coedge| !coedge.pcurves.is_empty())
                .count(),
            1
        );
        assert!(result
            .report
            .losses
            .iter()
            .all(|loss| !loss.message.contains("explicit UV pcurve reference")));
    }
}

#[test]
fn negative_ref_pcurve_reverses_its_uv_parameterization() {
    let smbh = with_pcurve_discriminator(
        synthetic_geometry_with_ref_pcurve_on_nurbs_surface_smbh(),
        -2,
    );
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("reversed ref pcurve decode");
    let cadmpeg_ir::geometry::PcurveGeometry::Nurbs { control_points, .. } =
        &result.ir.model.pcurves[0].geometry
    else {
        panic!("ref pcurve is not a NURBS");
    };
    assert_eq!(
        control_points.first(),
        Some(&cadmpeg_ir::math::Point2::new(0.75, 1.5))
    );
}

#[test]
fn ref_pcurve_selector_reversal_xors_intcurve_reversal() {
    let smbh = with_pcurve_discriminator(
        with_ref_pcurve_companion_reversed(
            synthetic_geometry_with_ref_pcurve_on_nurbs_surface_smbh(),
        ),
        -2,
    );
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("doubly reversed ref pcurve decode");
    let cadmpeg_ir::geometry::PcurveGeometry::Nurbs { control_points, .. } =
        &result.ir.model.pcurves[0].geometry
    else {
        panic!("ref pcurve is not a NURBS");
    };
    assert_eq!(
        control_points.first(),
        Some(&cadmpeg_ir::math::Point2::new(0.25, 0.5))
    );
}

#[test]
fn generated_inline_pcurve_tail_requires_four_adjacent_booleans() {
    let decode = |smbh: Vec<u8>| {
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("generated inline pcurve decode")
            .ir
            .model
            .pcurves
            .into_iter()
            .next()
            .expect("generated inline pcurve")
    };

    let complete = decode(synthetic_geometry_with_pcurve_smbh());
    assert_eq!(complete.native_tail_flags, Some([true, false, true, false]));
    assert_eq!(complete.parameter_range, Some([-1.0, 2.0]));

    let short = decode(synthetic_geometry_with_short_pcurve_tail_smbh());
    assert_eq!(short.native_tail_flags, None);
    assert_eq!(short.parameter_range, Some([-1.0, 2.0]));
}

#[test]
fn generated_inline_pcurve_fit_tolerance_is_scoped() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_additional_out_of_scope_pcurve_cache_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("generated inline pcurve decode");
    assert_eq!(result.ir.model.pcurves[0].fit_tolerance, Some(0.001));
}

#[test]
fn generated_pcurve_geometry_dispatch_follows_discriminator() {
    for smbh in [
        with_pcurve_discriminator(synthetic_geometry_with_pcurve_smbh(), 2),
        with_inline_pcurve_non_boolean_wrapper(synthetic_geometry_with_pcurve_smbh()),
        renamed_generated_subtype(
            synthetic_geometry_with_pcurve_smbh(),
            "exp_par_cur",
            "bad_par_cur",
        ),
        synthetic_geometry_with_out_of_scope_pcurve_cache_smbh(),
        with_pcurve_discriminator(synthetic_geometry_with_ref_pcurve_smbh(), 0),
        with_pcurve_discriminator(synthetic_geometry_with_ref_pcurve_smbh(), 1),
        with_pcurve_discriminator(synthetic_geometry_with_ref_pcurve_smbh(), -1),
        with_pcurve_discriminator(synthetic_geometry_with_ref_pcurve_smbh(), 7),
        with_ref_pcurve_companion_name(synthetic_geometry_with_ref_pcurve_smbh(), b"badcurve"),
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("generated mismatched pcurve decode");
        assert!(result.ir.model.pcurves.is_empty());
        assert!(result
            .ir
            .model
            .coedges
            .iter()
            .all(|coedge| coedge.pcurves.is_empty()));
        let note = result
            .report
            .losses
            .iter()
            .find(|loss| loss.message.contains("explicit UV pcurve reference"))
            .expect("undecoded pcurve loss note");
        assert!(note.message.contains("Native kinds: pcurve=1."));
    }
}

#[test]
fn generated_pcurve_reports_dangling_carrier_reference() {
    let mut smbh = synthetic_geometry_with_pcurve_smbh();
    let start = asm_header::record_stream_start(&smbh).unwrap();
    let limit = asm_header::solved_record_limit(&smbh).unwrap();
    let records = cadmpeg_asm::sab::frame(&smbh, start, limit, 8).unwrap();
    let coedge = &records[7];
    let record = &mut smbh[coedge.offset..coedge.offset + coedge.len];
    let pcurve_ref = record.iter().rposition(|byte| *byte == 0x0c).unwrap();
    record[pcurve_ref + 1..pcurve_ref + 9].copy_from_slice(&999i64.to_le_bytes());

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("dangling pcurve reference remains a successful topology decode");
    let note = result
        .report
        .losses
        .iter()
        .find(|loss| loss.message.contains("explicit UV pcurve reference"))
        .expect("dangling pcurve loss note");
    assert!(note.message.contains("Native kinds: dangling-reference=1."));
}

#[test]
fn generated_f3d_rewrites_nurbs_pcurve_control_points() {
    let source = f3d_with_smbh(&synthetic_geometry_with_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated pcurve decode");
    let mut edited = decoded.ir;
    let pcurve = &mut edited.model.pcurves[0];
    assert_eq!(pcurve.wrapper_reversed, Some(false));
    assert_eq!(pcurve.native_tail_flags, Some([true, false, true, false]));
    assert_eq!(pcurve.parameter_range, Some([-1.0, 2.0]));
    assert_eq!(pcurve.fit_tolerance, Some(0.001));
    let cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        periodic,
        ..
    } = &mut pcurve.geometry
    else {
        panic!("expected NURBS pcurve")
    };
    control_points[0].u = -0.5;
    control_points[1].v = 2.25;
    *degree = 2;
    *knots = vec![-1.0, -1.0, -1.0, 2.0, 2.0];
    *periodic = true;
    pcurve.wrapper_reversed = Some(true);
    pcurve.native_tail_flags = Some([false, true, false, true]);
    pcurve.parameter_range = Some([-2.0, 3.0]);
    pcurve.fit_tolerance = Some(0.0025);
    let expected = pcurve.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("pcurve regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated pcurve decode");
    assert_eq!(round_trip.ir.model.pcurves, [expected.clone()]);
}

#[test]
fn generated_f3d_scopes_inline_pcurve_edits() {
    let source =
        f3d_with_smbh(&synthetic_geometry_with_additional_out_of_scope_pcurve_cache_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated scoped pcurve decode");
    let mut edited = decoded.ir;
    let pcurve = &mut edited.model.pcurves[0];
    let cadmpeg_ir::geometry::PcurveGeometry::Nurbs { control_points, .. } = &mut pcurve.geometry
    else {
        panic!("expected NURBS pcurve")
    };
    control_points[0].u = -0.75;
    pcurve.fit_tolerance = Some(0.0025);
    let expected = pcurve.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("scoped pcurve regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated scoped pcurve decode");
    assert_eq!(round_trip.ir.model.pcurves, [expected]);
}

#[test]
fn generated_f3d_rewrites_rational_pcurve_weights() {
    let source = f3d_with_smbh(&synthetic_geometry_with_rational_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated rational pcurve decode");
    let mut edited = decoded.ir;
    let cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
        control_points,
        weights: Some(weights),
        ..
    } = &mut edited.model.pcurves[0].geometry
    else {
        panic!("expected rational pcurve")
    };
    control_points[0].u = -0.25;
    weights[1] = 0.75;
    let expected = edited.model.pcurves[0].clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("rational pcurve regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated rational pcurve decode");
    assert_eq!(round_trip.ir.model.pcurves, [expected]);
}

#[test]
fn generated_f3d_rewrites_ref_form_pcurve_geometry_and_range() {
    let source = f3d_with_smbh(&synthetic_geometry_with_ref_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated ref-form pcurve decode");
    let mut edited = decoded.ir;
    let pcurve = &mut edited.model.pcurves[0];
    assert_eq!(pcurve.wrapper_reversed, None);
    assert_eq!(pcurve.fit_tolerance, None);
    assert_eq!(pcurve.parameter_range, Some([-2.0, 4.0]));
    let cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
        control_points,
        knots,
        ..
    } = &mut pcurve.geometry
    else {
        panic!("expected ref-form NURBS pcurve")
    };
    control_points[0].u = -0.75;
    control_points[1].v = 3.5;
    *knots = vec![-1.0, -1.0, 2.0, 2.0];
    pcurve.parameter_range = Some([-3.0, 5.0]);
    let expected = pcurve.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("ref-form pcurve regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated ref-form pcurve decode");
    assert_eq!(round_trip.ir.model.pcurves, [expected.clone()]);

    edited.source = None;
    edited.set_native_unknowns("f3d", &[]).unwrap();
    let mut source_less = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &edited,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut source_less))
        .expect("source-less ref-form pcurve encode");
    let source_less_round_trip = F3dCodec
        .decode(&mut Cursor::new(source_less), &DecodeOptions::default())
        .expect("source-less ref-form pcurve round trip");
    let actual = &source_less_round_trip.ir.model.pcurves[0];
    assert_eq!(actual.geometry, expected.geometry);
    assert_eq!(actual.wrapper_reversed, expected.wrapper_reversed);
    assert_eq!(actual.native_tail_flags, expected.native_tail_flags);
    assert_eq!(actual.parameter_range, expected.parameter_range);
    assert_eq!(actual.fit_tolerance, expected.fit_tolerance);
    assert!(source_less_round_trip
        .ir
        .model
        .coedges
        .iter()
        .any(|coedge| coedge.pcurves.iter().any(|use_| use_.pcurve == actual.id)));

    let mut mixed = edited;
    let mut inline = mixed.model.pcurves[0].clone();
    inline.id = cadmpeg_ir::ids::PcurveId("generated:mixed-inline-pcurve#0".into());
    inline.wrapper_reversed = Some(false);
    inline.native_tail_flags = Some([true, false, true, false]);
    inline.fit_tolerance = Some(0.002);
    mixed.model.coedges[1].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
        pcurve: inline.id.clone(),
        isoparametric: None,
        parameter_range: None,
    }];
    mixed.model.pcurves.push(inline);
    let mut mixed_bytes = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &mixed,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut mixed_bytes))
        .expect("mixed inline/ref-form pcurve encode");
    let mixed_round_trip = F3dCodec
        .decode(&mut Cursor::new(mixed_bytes), &DecodeOptions::default())
        .expect("mixed inline/ref-form pcurve round trip");
    assert_eq!(mixed_round_trip.ir.model.pcurves.len(), 2);
    assert!(mixed_round_trip
        .ir
        .model
        .pcurves
        .iter()
        .any(|pcurve| pcurve.wrapper_reversed.is_none()));
    assert!(mixed_round_trip
        .ir
        .model
        .pcurves
        .iter()
        .any(|pcurve| pcurve.wrapper_reversed == Some(false)));
    assert!(mixed_round_trip
        .ir
        .model
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter().map(|use_| &use_.pcurve))
        .all(|pcurve_id| mixed_round_trip
            .ir
            .model
            .pcurves
            .iter()
            .any(|pcurve| pcurve.id == *pcurve_id)));
}
