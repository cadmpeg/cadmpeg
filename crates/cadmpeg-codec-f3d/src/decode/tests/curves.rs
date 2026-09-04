// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::write::TargetRequest;
use std::io::{Cursor, Read};

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::F3dCodec;

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
    assert_eq!(c.degree(), 2);
    assert_eq!(c.control_points().len(), 3);
    // Clamped knots: [0,0,0,1,1,1] (endpoint mult 2 + 1 = 3 each).
    assert_eq!(c.knots(), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(c.control_points()[1].x, 10.0);
    assert_eq!(c.control_points()[1].y, 20.0);
    assert!(c.weights().is_none());
}

#[test]
fn decode_retains_generated_procedural_curve_fit_contract() {
    let f3d = f3d_with_smbh(&synthetic_geometry_with_procedural_curve_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    let procedural = result.ir().model.procedural_curves.first().unwrap();
    assert!(matches!(
        procedural.definition(),
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Unknown {
            native_kind: Some(native_kind),
            record: None,
        } if native_kind == "surf_surf_int_cur"
    ));
    assert_eq!(procedural.cache_fit_tolerance(), Some(0.005));
    assert_eq!(result.ir().model.curves.len(), 1);
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
        .ir()
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
    } = procedural.definition()
    else {
        panic!("expected helix construction")
    };
    assert_eq!(*angle_range, [0.0, std::f64::consts::TAU]);
    assert_eq!(*center, Point3::new(10.0, 20.0, 30.0));
    assert_eq!(*major, cadmpeg_ir::math::Vector3::new(20.0, 0.0, 0.0));
    assert_eq!(*minor, cadmpeg_ir::math::Vector3::new(0.0, 20.0, 0.0));
    assert_eq!(*pitch, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 40.0));
    assert_eq!(*apex_factor, 0.25);
    assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(procedural.cache_fit_tolerance(), Some(0.005));

    let mut edited = result.ir().clone();
    edited.model.procedural_curves[0].replace_definition(ProceduralCurveDefinition::Helix {
        angle_range: [-1.0, 7.0],
        center: Point3::new(12.0, 23.0, 34.0),
        major: cadmpeg_ir::math::Vector3::new(30.0, 0.0, 0.0),
        minor: cadmpeg_ir::math::Vector3::new(0.0, -30.0, 0.0),
        pitch: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 55.0),
        apex_factor: 0.5,
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
    });
    edited.model.procedural_curves[0]
        .set_cache_fit_tolerance(Some(0.012))
        .unwrap();
    let solved_curve_id = edited
        .model
        .procedural_curve_owner(&edited.model.procedural_curves[0].id)
        .expect("helix solved curve owner")
        .clone();
    let solved_curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == solved_curve_id)
        .expect("helix solved curve");
    let cadmpeg_ir::geometry::CurveGeometry::Procedural {
        cache: Some(solved_cache),
        ..
    } = &mut solved_curve.geometry
    else {
        panic!("expected procedural helix carrier")
    };
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(mut edited_cache) =
        solved_cache.as_geometry().clone()
    else {
        panic!("expected helix NURBS cache")
    };
    edited_cache.control_points_mut()[1].x = 17.0;
    edited_cache.control_points_mut()[1].z = -2.0;
    *solved_cache = cadmpeg_ir::geometry::SolvedCurveGeometry::new(
        cadmpeg_ir::geometry::CurveGeometry::Nurbs(edited_cache),
    )
    .unwrap();
    let edited_definition = edited.model.procedural_curves[0].definition().clone();
    let edited_cache = solved_curve.geometry.clone();
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut regenerated)
        .expect("helix definition regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated helix decode");
    assert_eq!(
        regenerated.ir().model.procedural_curves[0].definition(),
        &edited_definition
    );
    assert_eq!(
        regenerated.ir().model.procedural_curves[0].cache_fit_tolerance(),
        Some(0.012)
    );
    assert!(regenerated
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.geometry == edited_cache));

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = source_less.model.procedural_curves[0].definition().clone();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less helix encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less helix round trip");
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].definition(),
        &expected
    );
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].cache_fit_tolerance(),
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
        .ir()
        .model
        .procedural_curves
        .first()
        .expect("helix construction");
    assert!(matches!(
        procedural.definition(),
        ProceduralCurveDefinition::Helix { .. }
    ));
    assert_eq!(procedural.cache_fit_tolerance(), None);
    assert!(matches!(
        result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| {
                result.ir().model.procedural_curve_owner(&procedural.id) == Some(&curve.id)
            })
            .map(|curve| &curve.geometry),
        Some(CurveGeometry::Procedural { construction, .. }) if *construction == procedural.id
    ));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(
        validation.is_ok(),
        "validation findings: {:?}",
        validation.findings
    );
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("procedural intcurve")));

    let expected = procedural.definition().clone();
    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("cacheless helix source-less encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("cacheless helix source-less round trip");
    assert!(matches!(
        round_trip.ir().model.curves[0].geometry,
        CurveGeometry::Procedural { .. }
    ));
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].definition(),
        &expected
    );
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].cache_fit_tolerance(),
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
        .ir()
        .model
        .procedural_curves
        .iter()
        .find(|curve| matches!(curve.definition(), ProceduralCurveDefinition::Law { .. }))
        .expect("law intcurve construction");
    let ProceduralCurveDefinition::Law {
        context,
        extension,
        primary,
        additional,
        ..
    } = procedural.definition()
    else {
        unreachable!()
    };
    assert_eq!(context.parameter_range, [-1.0, 2.0]);
    assert_eq!(*extension, 0);
    assert_eq!(primary.name(), "primary_law");
    assert!(matches!(
        primary.variables()[0],
        LawExpression::Edge { parameters, .. } if parameters == [-0.5, 1.5]
    ));
    assert_eq!(additional.len(), 2);

    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less law intcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less law intcurve round trip");
    assert!(round_trip.ir().model.procedural_curves.iter().any(|curve| {
        matches!(
            curve.definition(),
            ProceduralCurveDefinition::Law { primary, .. }
                if matches!(primary.variables()[0], LawExpression::Edge { .. })
        )
    }));
}

#[test]
fn generated_vector_offset_curve_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, VectorOffsetRoles};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_vector_offset_curve_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("generated vector-offset decode");
    let procedural = &result.ir().model.procedural_curves[0];
    let ProceduralCurveDefinition::VectorOffset {
        source,
        parameter_range,
        offset,
        roles,
    } = procedural.definition()
    else {
        panic!("expected vector offset construction")
    };
    assert_eq!(*parameter_range, [-2.0, 5.0]);
    assert_eq!(*offset, cadmpeg_ir::math::Vector3::new(5.0, -10.0, 20.0));
    assert_eq!(
        *roles,
        VectorOffsetRoles {
            source_code: 7,
            offset_code: 9,
        }
    );
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *source));
    assert_eq!(procedural.cache_fit_tolerance(), Some(0.008));
    let expected_range = *parameter_range;
    let expected_offset = *offset;
    let expected_roles = *roles;

    let mut edited = result.ir().clone();
    edited.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::VectorOffset {
            parameter_range,
            offset,
            ..
        } = definition
        else {
            panic!("expected editable vector offset")
        };
        *parameter_range = [-3.0, 6.0];
        *offset = cadmpeg_ir::math::Vector3::new(8.0, -12.0, 25.0);
    });
    edited.model.procedural_curves[0]
        .set_cache_fit_tolerance(Some(0.015))
        .unwrap();
    let edited_definition = edited.model.procedural_curves[0].definition().clone();
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut regenerated)
        .expect("vector-offset regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated vector-offset decode");
    assert_eq!(
        regenerated.ir().model.procedural_curves[0].definition(),
        &edited_definition
    );
    assert_eq!(
        regenerated.ir().model.procedural_curves[0].cache_fit_tolerance(),
        Some(0.015)
    );

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let source_id = match source_less.model.procedural_curves[0].definition() {
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less vector-offset encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less vector-offset round trip");
    let ProceduralCurveDefinition::VectorOffset {
        source,
        parameter_range,
        offset,
        roles,
    } = &round_trip.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected round-trip vector offset")
    };
    assert_eq!(*parameter_range, expected_range);
    assert_eq!(*offset, expected_offset);
    assert_eq!(*roles, expected_roles);
    assert!(round_trip
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *source));
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].cache_fit_tolerance(),
        Some(0.008)
    );
    assert!(matches!(
        round_trip
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *source)
            .map(|curve| &curve.geometry),
        Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
            if curve.degree() == 1
                && curve.knots() == [-2.0, -2.0, 5.0, 5.0]
                && curve.control_points() == [
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
    } = &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected subset construction")
    };
    assert_eq!(*parameter_range, [-1.5, 3.5]);
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *source));
    assert!(
        (result.ir().model.procedural_curves[0]
            .cache_fit_tolerance()
            .expect("subset fit tolerance")
            - 0.006)
            .abs()
            < 1.0e-12
    );

    let mut edited = result.ir().clone();
    edited.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::Subset {
            parameter_range, ..
        } = definition
        else {
            unreachable!()
        };
        *parameter_range = [-2.0, 4.0];
    });
    let expected_edit = edited.model.procedural_curves[0].definition().clone();
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut regenerated)
        .expect("subset regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated subset decode");
    assert_eq!(
        regenerated.ir().model.procedural_curves[0].definition(),
        &expected_edit
    );

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let source_id = match source_less.model.procedural_curves[0].definition() {
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less subset encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less subset round trip");
    let ProceduralCurveDefinition::Subset {
        source,
        parameter_range,
        sense: _,
    } = &round_trip.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected round-trip subset")
    };
    assert_eq!(*parameter_range, [-1.5, 3.5]);
    assert!(round_trip
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *source));
    let source_curve = round_trip
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *source)
        .expect("round-trip subset source");
    assert_eq!(
        source_curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Nurbs(
            cadmpeg_ir::geometry::NurbsCurve::new(
                1,
                vec![-1.5, -1.5, 3.5, 3.5],
                vec![
                    cadmpeg_ir::math::Point3::new(8.5, 23.0, 29.25),
                    cadmpeg_ir::math::Point3::new(13.5, 13.0, 31.75),
                ],
                None,
                false,
            )
            .expect("valid subset source curve")
        )
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
        result.ir().model.procedural_curves[0].definition(),
        &ProceduralCurveDefinition::Exact
    );
    assert_eq!(
        result.ir().model.procedural_curves[0].cache_fit_tolerance(),
        Some(0.004)
    );

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less exact intcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less exact intcurve round trip");
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].definition(),
        &ProceduralCurveDefinition::Exact
    );
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].cache_fit_tolerance(),
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
        let (mut source_less, _, _) = result.into_parts();
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();

        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        let curve_id = result
            .ir()
            .model
            .procedural_curve_owner(&result.ir().model.procedural_curves[0].id)
            .expect("exact intcurve owner");
        result
            .ir()
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
        let surface_id = result
            .ir()
            .model
            .procedural_surface_owner(&result.ir().model.procedural_surfaces[0].id)
            .expect("exact spline-surface owner");
        let geometry = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *surface_id)
            .expect("exact spline-surface carrier")
            .geometry
            .clone();
        let face_sense = result
            .ir()
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
            result.ir().model.procedural_curves[0].definition(),
            ProceduralCurveDefinition::Unknown { .. }
        ));
        let (mut source_less, _, _) = result.into_parts();
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("canonical source-less intcurve encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("canonical intcurve round trip");
        assert!(!matches!(
            round_trip.ir().model.procedural_curves[0].definition(),
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
    } = &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected compound construction")
    };
    assert_eq!(parameters, &[0.0, 0.5, 1.0]);
    assert_eq!(component_parameters, &[-2.0, 4.0]);
    assert_eq!(components.len(), 2);
    assert!(components.iter().all(|component| result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *component)));
    assert!(
        (result.ir().model.procedural_curves[0]
            .cache_fit_tolerance()
            .expect("compound fit tolerance")
            - 0.003)
            .abs()
            < 1.0e-12
    );
    let component_ids = components.clone();

    let mut edited = result.ir().clone();
    edited.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::Compound {
            parameters,
            component_parameters,
            ..
        } = definition
        else {
            unreachable!()
        };
        *parameters = vec![-0.25, 0.75, 1.25];
        *component_parameters = vec![-3.0, 5.0];
    });
    let expected_edit = edited.model.procedural_curves[0].definition().clone();
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut regenerated)
        .expect("compound intcurve regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated compound intcurve decode");
    assert_eq!(
        regenerated.ir().model.procedural_curves[0].definition(),
        &expected_edit
    );

    let (mut source_less, _, _) = result.into_parts();
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less compound intcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less compound intcurve round trip");
    let ProceduralCurveDefinition::Compound {
        parameters,
        component_parameters,
        components,
    } = &round_trip.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected round-trip compound construction")
    };
    assert_eq!(parameters, &[0.0, 0.5, 1.0]);
    assert_eq!(component_parameters, &[-2.0, 4.0]);
    assert_eq!(components.len(), 2);
    for (ordinal, component) in components.iter().enumerate() {
        let curve = round_trip
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *component)
            .expect("round-trip compound component");
        let cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve) = &curve.geometry else {
            panic!("compound line component was not lowered to NURBS")
        };
        assert_eq!(curve.degree(), 1);
        let range = [ordinal as f64 * 0.5, (ordinal + 1) as f64 * 0.5];
        assert_eq!(curve.knots(), [range[0], range[0], range[1], range[1]]);
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
    } = &result.ir().model.procedural_curves[0].definition()
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

    let mut edited = result.ir().clone();
    edited.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::TwoSidedOffset {
            context,
            discontinuity_flag,
            offsets,
        } = definition
        else {
            unreachable!()
        };
        context.parameter_range = [-2.0, 3.0];
        context.discontinuities = [vec![0.2, 0.8], vec![], vec![0.6]];
        *discontinuity_flag = false;
        *offsets = [-3.0, 5.0];
    });
    let expected_edit = edited.model.procedural_curves[0].definition().clone();
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut regenerated)
        .expect("two-sided offset regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated two-sided offset decode");
    assert_eq!(
        regenerated.ir().model.procedural_curves[0].definition(),
        &expected_edit
    );

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less two-sided offset encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less two-sided offset round trip");
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].definition(),
        source_less.model.procedural_curves[0].definition()
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
    } = &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected embedded two-sided offset")
    };
    assert_eq!(*offsets, [-1.0, 3.0]);
    for side in &context.sides {
        let surface_id = side.surface.as_ref().expect("embedded support surface");
        assert!(result.ir().model.surfaces.iter().any(|surface| {
            surface.id == *surface_id && matches!(surface.geometry, SurfaceGeometry::Nurbs(_))
        }));
        assert!(matches!(side.pcurve, Some(PcurveGeometry::Nurbs { .. })));
    }
    assert!(matches!(
        context.sides[1].pcurve,
        Some(PcurveGeometry::Nurbs { ref nurbs }) if nurbs.weights().is_some()
    ));

    let mut retained = result.ir().clone();
    retained.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::TwoSidedOffset {
            context,
            discontinuity_flag,
            offsets,
        } = definition
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
    });
    let expected_retained = retained.model.procedural_curves[0].definition().clone();
    let mut retained_bytes = Vec::new();
    crate::test_support::plan_inherited_write(
        &retained,
        result.source_fidelity(),
        &mut retained_bytes,
    )
    .expect("retained embedded offset-support edit");
    let retained_round_trip = F3dCodec
        .decode(&mut Cursor::new(retained_bytes), &DecodeOptions::default())
        .expect("retained embedded offset-support round trip");
    assert_eq!(
        retained_round_trip.ir().model.procedural_curves[0].definition(),
        &expected_retained
    );

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut expected = source_less.model.procedural_curves[0].definition().clone();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
    } = &round_trip.ir().model.procedural_curves[0].definition()
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
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| Some(&surface.id) == actual_context.sides[side].surface.as_ref())
            .expect("round-trip support surface");
        assert_eq!(actual_surface.geometry, expected_surface.geometry);
        expected_context.sides[side].surface = actual_context.sides[side].surface.clone();
    }
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].definition(),
        &expected
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
    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let first_support = source_less.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::TwoSidedOffset { context, .. } = definition else {
            panic!("expected two-sided offset construction")
        };
        context.sides[1].surface = None;
        context.sides[1].pcurve = None;
        context.sides[0].pcurve = Some(cadmpeg_ir::geometry::PcurveGeometry::Line {
            origin: cadmpeg_ir::math::Point2::new(1.0, 2.0),
            direction: cadmpeg_ir::math::Point2::new(3.0, -1.0),
        });
        context.sides[0]
            .surface
            .clone()
            .expect("retained first support id")
    });
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less mixed offset-support encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less mixed offset-support round trip");
    let ProceduralCurveDefinition::TwoSidedOffset { context, .. } =
        &round_trip.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected round-trip two-sided offset construction")
    };
    assert!(context.sides[1].surface.is_none() && context.sides[1].pcurve.is_none());
    assert_eq!(
        context.sides[0].pcurve,
        Some(cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
            nurbs: cadmpeg_ir::geometry::PcurveNurbs::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![
                    cadmpeg_ir::math::Point2::new(1.0, 2.0),
                    cadmpeg_ir::math::Point2::new(4.0, 1.0),
                ],
                None,
                false,
            )
            .unwrap(),
        })
    );
    let actual_surface = round_trip
        .ir()
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
    } = &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected analytic two-sided offset")
    };
    assert_eq!(*offsets, [-1.5, 2.5]);
    let supports = context.sides.each_ref().map(|side| {
        result
            .ir()
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

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected_geometries = supports;
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less analytic offset-support encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less analytic offset-support round trip");
    let ProceduralCurveDefinition::TwoSidedOffset {
        context, offsets, ..
    } = &round_trip.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected round-trip analytic offset supports")
    };
    assert_eq!(*offsets, [-1.5, 2.5]);
    for (side, expected) in context.sides.iter().zip(expected_geometries) {
        let actual = round_trip
            .ir()
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
    } = &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected surface intersection")
    };
    assert!(*discontinuity_flag);
    let expected_geometries = context.sides.each_ref().map(|side| {
        result
            .ir()
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

    let mut edited = result.ir().clone();
    edited.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::Intersection {
            context,
            discontinuity_flag,
        } = definition
        else {
            unreachable!()
        };
        context.parameter_range = [-1.0, 2.0];
        *discontinuity_flag = false;
    });
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut regenerated)
        .expect("intersection context regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated intersection decode");
    assert!(matches!(
        regenerated.ir().model.procedural_curves[0].definition(),
        ProceduralCurveDefinition::Intersection {
            ref context,
            discontinuity_flag: false,
        } if context.parameter_range == [-1.0, 2.0]
    ));

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less surface intersection encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less surface intersection round trip");
    let ProceduralCurveDefinition::Intersection {
        context,
        discontinuity_flag,
    } = &round_trip.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected round-trip surface intersection")
    };
    assert!(*discontinuity_flag);
    for (side, expected) in context.sides.iter().zip(expected_geometries) {
        let actual = round_trip
            .ir()
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
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, ProjectionRole, ProjectionTail};

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
    } = &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected projection")
    };
    assert!(context.sides.iter().all(|side| side.surface.is_some()));
    assert!(*discontinuity_flag);
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *source));
    assert_eq!(
        tail,
        &ProjectionTail::Ranged {
            flag: true,
            parameter_range: [-2.0, 3.0],
            role: ProjectionRole::Surf2,
        }
    );

    let mut edited = result.ir().clone();
    edited.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::Projection {
            context,
            discontinuity_flag,
            tail,
            ..
        } = definition
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
        *role = ProjectionRole::Surf1;
    });
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut regenerated)
        .expect("projection context regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated projection decode");
    assert!(matches!(
        regenerated.ir().model.procedural_curves[0].definition(),
        ProceduralCurveDefinition::Projection {
            ref context,
            discontinuity_flag: false,
            tail: ProjectionTail::Ranged {
                flag: false,
                parameter_range: [-4.0, 5.0],
                ref role,
            },
            ..
        } if context.parameter_range == [-1.0, 2.0] && *role == ProjectionRole::Surf1
    ));

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less projection encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less projection round trip");
    let ProceduralCurveDefinition::Projection {
        discontinuity_flag,
        tail,
        ..
    } = &round_trip.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected round-trip projection")
    };
    assert!(*discontinuity_flag);
    assert_eq!(
        tail,
        &ProjectionTail::Ranged {
            flag: true,
            parameter_range: [-2.0, 3.0],
            role: ProjectionRole::Surf2,
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
        result.ir().model.procedural_curves[0].definition(),
        ProceduralCurveDefinition::Projection {
            discontinuity_flag: true,
            tail: ProjectionTail::EarlyClose { flag: true },
            ..
        }
    ));

    let mut edited = result.ir().clone();
    edited.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::Projection {
            tail: ProjectionTail::EarlyClose { flag },
            ..
        } = definition
        else {
            unreachable!()
        };
        *flag = false;
    });
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut regenerated)
        .expect("early-close projection regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated early-close projection decode");
    assert!(matches!(
        regenerated.ir().model.procedural_curves[0].definition(),
        ProceduralCurveDefinition::Projection {
            tail: ProjectionTail::EarlyClose { flag: false },
            ..
        }
    ));

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less early-close projection encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less early-close projection round trip");
    assert!(matches!(
        round_trip.ir().model.procedural_curves[0].definition(),
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
    } = &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected three-surface intersection")
    };
    assert_eq!(*selector, 7);
    assert!(context.sides.iter().all(|side| side.surface.is_some()));
    let third_surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| Some(&surface.id) == third.surface.as_ref())
        .expect("third support surface");
    assert!(matches!(
        third_surface.geometry,
        SurfaceGeometry::Sphere { radius: -12.5, .. }
    ));

    let mut edited = result.ir().clone();
    edited.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::ThreeSurfaceIntersection {
            context, selector, ..
        } = definition
        else {
            unreachable!()
        };
        context.parameter_range = [-1.0, 2.0];
        *selector = -4;
    });
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut regenerated)
        .expect("three-surface intersection regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated three-surface intersection decode");
    assert!(matches!(
        regenerated.ir().model.procedural_curves[0].definition(),
        ProceduralCurveDefinition::ThreeSurfaceIntersection {
            ref context,
            selector: -4,
            ..
        } if context.parameter_range == [-1.0, 2.0]
    ));

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less three-surface intersection encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less three-surface intersection round trip");
    let ProceduralCurveDefinition::ThreeSurfaceIntersection {
        selector, third, ..
    } = &round_trip.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected round-trip three-surface intersection")
    };
    assert_eq!(*selector, 7);
    let third_surface = round_trip
        .ir()
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
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SurfaceCurveFamilyKind};

    for (name, expected_family) in [
        ("blend_int_cur", SurfaceCurveFamilyKind::Blend),
        ("surf_int_cur", SurfaceCurveFamilyKind::SurfaceConstrained),
        ("par_int_cur", SurfaceCurveFamilyKind::Parametric),
        ("skin_int_cur", SurfaceCurveFamilyKind::Skin),
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_surface_curve_smbh(
                    name,
                ))),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{name} decode failed: {error}"));
        let ProceduralCurveDefinition::SurfaceCurve { family } =
            &result.ir().model.procedural_curves[0].definition()
        else {
            panic!("expected {name} surface curve")
        };
        assert_eq!(family.kind(), expected_family);
        let context = family.context();
        assert!(context.sides.iter().all(|side| side.surface.is_some()));

        let mut edited = result.ir().clone();
        edited.model.procedural_curves[0].edit_definition(|definition| {
            let ProceduralCurveDefinition::SurfaceCurve { family } = definition else {
                unreachable!()
            };
            family.context_mut().parameter_range = [-1.0, 2.0];
        });
        let mut regenerated = Vec::new();
        crate::test_support::plan_inherited_write(
            &edited,
            result.source_fidelity(),
            &mut regenerated,
        )
        .unwrap_or_else(|error| panic!("{name} context regeneration failed: {error}"));
        let regenerated = F3dCodec
            .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("regenerated {name} decode failed: {error}"));
        assert!(matches!(
            regenerated.ir().model.procedural_curves[0].definition(),
            ProceduralCurveDefinition::SurfaceCurve { ref family }
                if family.context().parameter_range == [-1.0, 2.0]
        ));

        let (mut source_less, _, _) = result.into_parts();
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .unwrap_or_else(|error| panic!("{name} source-less encode failed: {error}"));
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{name} round trip failed: {error}"));
        assert!(matches!(
            &round_trip.ir().model.procedural_curves[0].definition(),
            ProceduralCurveDefinition::SurfaceCurve { family } if family.kind() == expected_family
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
        } = &result.ir().model.procedural_curves[0].definition()
        else {
            panic!("expected {name} silhouette")
        };
        assert!(result
            .ir()
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

        let mut edited = result.ir().clone();
        edited.model.procedural_curves[0].edit_definition(|definition| {
            let ProceduralCurveDefinition::Silhouette {
                silhouette,
                light_direction,
                ..
            } = definition
            else {
                unreachable!()
            };
            *light_direction = cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0);
            if let SilhouetteKind::Taper { draft_factor } = silhouette {
                *draft_factor = -0.2;
            }
        });
        let mut regenerated = Vec::new();
        crate::test_support::plan_inherited_write(
            &edited,
            result.source_fidelity(),
            &mut regenerated,
        )
        .unwrap_or_else(|error| panic!("{name} regeneration failed: {error}"));
        let regenerated = F3dCodec
            .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("regenerated {name} decode failed: {error}"));
        assert!(matches!(
            regenerated.ir().model.procedural_curves[0].definition(),
            ProceduralCurveDefinition::Silhouette {
                ref silhouette,
                light_direction,
                ..
            } if *light_direction == cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
                && match silhouette {
                    SilhouetteKind::Taper { draft_factor } => *draft_factor == -0.2,
                    _ => true,
                }
        ));

        let (mut source_less, _, _) = result.into_parts();
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .unwrap_or_else(|error| panic!("{name} source-less encode failed: {error}"));
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{name} round trip failed: {error}"));
        assert!(matches!(
            round_trip.ir().model.procedural_curves[0].definition(),
            ProceduralCurveDefinition::Silhouette { .. }
        ));
    }
}
