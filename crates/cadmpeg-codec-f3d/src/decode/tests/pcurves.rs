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

use std::io::Cursor;

use cadmpeg_asm::asm_header;
use cadmpeg_ir::codec::{Codec, DecodeOptions, Encoder};

use crate::test_support::*;
use crate::F3dCodec;

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
    } = &result.ir().model.procedural_curves[0].definition
    else {
        panic!("expected surface-offset construction")
    };
    assert_eq!(*base_u_range, [-1.0, 2.0]);
    assert_eq!(context.parameter_range, [0.0, 1.0]);
    assert!(*discontinuity_flag);
    assert_eq!(*base_v_range, [-3.0, 4.0]);
    assert_eq!(*base_range, [-0.5, 1.5]);
    assert_eq!((*distance, *shift, *scale), (-2.5, 0.75, 1.25));
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *base));

    let mut edited = result.ir().clone();
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
        .write_preserved_with_source_fidelity(&edited, result.source_fidelity(), &mut regenerated)
        .expect("surface-offset scalar regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated surface-offset decode");
    assert!(matches!(
        regenerated.ir().model.procedural_curves[0].definition,
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

    let (mut source_less, _, _) = result.into_parts();
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
    } = &round_trip.ir().model.procedural_curves[0].definition
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
    } = &result.ir().model.procedural_curves[0].definition
    else {
        panic!("expected spring construction")
    };
    assert_eq!(*direction, -3);
    assert!(context
        .sides
        .iter()
        .all(|side| side.surface.is_some() && side.pcurve.is_some()));

    let mut edited = result.ir().clone();
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
        .write_preserved_with_source_fidelity(&edited, result.source_fidelity(), &mut regenerated)
        .expect("spring tail regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated spring decode");
    assert!(matches!(
        regenerated.ir().model.procedural_curves[0].definition,
        ProceduralCurveDefinition::Spring {
            ref context,
            discontinuity_flag,
            direction: 4,
            ..
        } if discontinuity_flag == expected_flag && context.parameter_range == [-2.0, 3.0]
    ));

    let (mut source_less, _, _) = result.into_parts();
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
        round_trip.ir().model.procedural_curves[0].definition,
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
    } = &result.ir().model.procedural_curves[0].definition
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

    let (mut source_less, _, _) = result.into_parts();
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
        round_trip.ir().model.procedural_curves[0].definition,
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
        } = &result.ir().model.procedural_curves[0].definition
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
            .ir()
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

        let (mut source_less, _, _) = result.into_parts();
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
        } = &round_trip.ir().model.procedural_curves[0].definition
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
            .ir()
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *round_source));
        assert!(matches!(
            round_trip
                .ir()
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
    let (mut source_less, _, _) = decoded.into_parts();
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
        &round_trip.ir().model.procedural_curves[0].definition,
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
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.procedural_curves[0].cache_fit_tolerance = Some(0.025);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("procedural-curve fit regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated procedural-curve decode");
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].cache_fit_tolerance,
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
    let (mut source_less, _, _) = decoded.into_parts();
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
    let (mut source_less, _, _) = decoded.into_parts();
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
    let (mut edited, _, fidelity) = decoded.into_parts();
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
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("topology-bound NURBS regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated intcurve decode");
    assert!(round_trip
        .ir()
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

    assert_eq!(result.ir().model.pcurves.len(), 1);
    assert_eq!(
        result
            .ir()
            .model
            .coedges
            .iter()
            .filter(|c| !c.pcurves.is_empty())
            .count(),
        1
    );
    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
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

    assert_eq!(result.ir().model.pcurves.len(), 1);
    assert_eq!(
        result
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    assert!(result
        .report()
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

    assert_eq!(result.ir().model.pcurves.len(), 1);
    assert!(result
        .report()
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

    assert_eq!(result.ir().model.pcurves.len(), 1);
    assert_eq!(
        result
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    assert!(result
        .report()
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

        assert_eq!(result.ir().model.pcurves.len(), 1);
        assert_eq!(
            result
                .ir()
                .model
                .coedges
                .iter()
                .filter(|coedge| !coedge.pcurves.is_empty())
                .count(),
            1
        );
        assert!(result
            .report()
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
        &result.ir().model.pcurves[0].geometry
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
        &result.ir().model.pcurves[0].geometry
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
            .into_parts()
            .0
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
    assert_eq!(result.ir().model.pcurves[0].fit_tolerance, Some(0.001));
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
        assert!(result.ir().model.pcurves.is_empty());
        assert!(result
            .ir()
            .model
            .coedges
            .iter()
            .all(|coedge| coedge.pcurves.is_empty()));
        let note = result
            .report()
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
        .report()
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
    let (mut edited, _, fidelity) = decoded.into_parts();
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
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("pcurve regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated pcurve decode");
    assert_eq!(round_trip.ir().model.pcurves, [expected.clone()]);
}

#[test]
fn generated_f3d_scopes_inline_pcurve_edits() {
    let source =
        f3d_with_smbh(&synthetic_geometry_with_additional_out_of_scope_pcurve_cache_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated scoped pcurve decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
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
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("scoped pcurve regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated scoped pcurve decode");
    assert_eq!(round_trip.ir().model.pcurves, [expected]);
}

#[test]
fn generated_f3d_rewrites_rational_pcurve_weights() {
    let source = f3d_with_smbh(&synthetic_geometry_with_rational_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated rational pcurve decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
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
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("rational pcurve regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated rational pcurve decode");
    assert_eq!(round_trip.ir().model.pcurves, [expected]);
}

#[test]
fn generated_f3d_rewrites_ref_form_pcurve_geometry_and_range() {
    let source = f3d_with_smbh(&synthetic_geometry_with_ref_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated ref-form pcurve decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
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
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("ref-form pcurve regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated ref-form pcurve decode");
    assert_eq!(round_trip.ir().model.pcurves, [expected.clone()]);

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
    let actual = &source_less_round_trip.ir().model.pcurves[0];
    assert_eq!(actual.geometry, expected.geometry);
    assert_eq!(actual.wrapper_reversed, expected.wrapper_reversed);
    assert_eq!(actual.native_tail_flags, expected.native_tail_flags);
    assert_eq!(actual.parameter_range, expected.parameter_range);
    assert_eq!(actual.fit_tolerance, expected.fit_tolerance);
    assert!(source_less_round_trip
        .ir()
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
    assert_eq!(mixed_round_trip.ir().model.pcurves.len(), 2);
    assert!(mixed_round_trip
        .ir()
        .model
        .pcurves
        .iter()
        .any(|pcurve| pcurve.wrapper_reversed.is_none()));
    assert!(mixed_round_trip
        .ir()
        .model
        .pcurves
        .iter()
        .any(|pcurve| pcurve.wrapper_reversed == Some(false)));
    assert!(mixed_round_trip
        .ir()
        .model
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter().map(|use_| &use_.pcurve))
        .all(|pcurve_id| mixed_round_trip
            .ir()
            .model
            .pcurves
            .iter()
            .any(|pcurve| pcurve.id == *pcurve_id)));
}
