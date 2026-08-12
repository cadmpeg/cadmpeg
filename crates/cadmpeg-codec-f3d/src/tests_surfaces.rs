// SPDX-License-Identifier: Apache-2.0
//! Surfaces-domain synthetic tests and fixtures.

use super::*;

#[test]
fn zero_payload_mesh_surface_is_typed_as_a_native_sentinel() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let source = f3d_with_smbh(&synthetic_geometry_with_mesh_surface_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("mesh-surface decode");

    assert_eq!(result.ir.model.faces.len(), 1);
    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Unknown { .. }
    ));
    let native = f3d_native(&result.ir);
    assert_eq!(native.mesh_surface_sentinels.len(), 1);
    assert_eq!(
        native.mesh_surface_sentinels[0].surface,
        result.ir.model.surfaces[0].id
    );
    assert!(result.report.losses.iter().any(|loss| {
        loss.severity == cadmpeg_ir::report::Severity::Info
            && loss.message.contains("zero-payload mesh_surface")
    }));
    assert!(!result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("spline/procedural surfaces")));

    let mut replay = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &result.ir,
            fidelity: Some(&result.source_fidelity),
        })
        .and_then(|plan| plan.write_to(&mut replay))
        .expect("mesh-surface native replay");
    assert_eq!(replay, source);

    let mut edited = result.ir.clone();
    f3d_native_mut(&mut edited).mesh_surface_sentinels[0].id =
        "f3d:asm:mesh-surface-sentinel#edited".into();
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &edited,
            fidelity: Some(&result.source_fidelity),
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("mesh-surface structural metadata is immutable");
    assert!(error.to_string().contains("edits beyond supported"));

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.model.surfaces[0].geometry = SurfaceGeometry::Unknown { record: None };
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("mesh-surface sentinel requires retained ASM bytes");
    assert!(error
        .to_string()
        .contains("cannot serialize mesh-surface sentinel"));
}

#[test]
fn nurbs_surface_block_decodes_to_carrier() {
    use cadmpeg_asm::nurbs::core::decode_surface_cache;

    // A degree-1 × degree-1 nubs surface with a 2×2 control grid. Endpoint
    // multiplicities are stored as `degree` (=1); the clamped knot vector adds
    // one at each end, giving [0,0,1,1] in each direction.
    let mut b = Vec::new();
    b.extend_from_slice(b"\x0d\x04nubs");
    push_tagged_i64(&mut b, 0x04, 1); // degree_u
    push_tagged_i64(&mut b, 0x04, 1); // degree_v
    for _ in 0..4 {
        push_tagged_i64(&mut b, 0x15, 0); // periodic/singularity enums = open
    }
    push_tagged_i64(&mut b, 0x04, 2); // n_unique_knots_u
    push_tagged_i64(&mut b, 0x04, 2); // n_unique_knots_v
    for (k, m) in [(0.0, 1i64), (1.0, 1)] {
        push_tagged_f64(&mut b, k);
        push_tagged_i64(&mut b, 0x04, m);
    }
    for (k, m) in [(0.0, 1i64), (1.0, 1)] {
        push_tagged_f64(&mut b, k);
        push_tagged_i64(&mut b, 0x04, m);
    }
    // Control grid stored v-major (v outer, u inner); coordinates in cm.
    let grid = [
        [0.0, 0.0, 0.0], // (u0,v0)
        [1.0, 0.0, 0.0], // (u1,v0)
        [0.0, 1.0, 0.0], // (u0,v1)
        [1.0, 1.0, 0.0], // (u1,v1)
    ];
    for p in grid {
        for c in p {
            push_tagged_f64(&mut b, c);
        }
    }

    let s = decode_surface_cache(&b).expect("surface block decodes");
    assert_eq!((s.u_degree, s.v_degree), (1, 1));
    assert_eq!((s.u_count, s.v_count), (2, 2));
    assert_eq!(s.u_knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(s.v_knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(s.control_points.len(), 4);
    assert!(s.weights.is_none());
    // Transposed to u-major: index u*v_count+v. Pole (u1,v0) sits at index 2,
    // and coordinates are cm→mm scaled (×10).
    assert_eq!(s.control_points[2].x, 10.0);
    assert_eq!(s.control_points[2].y, 0.0);
}

#[test]
fn generated_exact_spline_surfaces_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SplineSurfaceParameters};

    for name in ["exact_spl_sur", "exactsur"] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_exact_spl_sur_smbh(name))),
                &DecodeOptions::default(),
            )
            .expect("exact spline surface decode");
        let procedural = result.ir.model.procedural_surfaces.first().unwrap();
        assert_eq!(procedural.cache_fit_tolerance, Some(0.015));
        assert_eq!(
            procedural.definition,
            ProceduralSurfaceDefinition::Exact {
                parameters: SplineSurfaceParameters::OrderedRanges {
                    ranges: [[-2.0, 3.0], [-4.0, 5.0]],
                },
                extension: 7,
                revision_form: None,
            }
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
            .expect("source-less exact spline surface encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less exact spline surface round trip");
        assert_eq!(
            round_trip.ir.model.procedural_surfaces[0].definition,
            ProceduralSurfaceDefinition::Exact {
                parameters: SplineSurfaceParameters::OrderedRanges {
                    ranges: [[-2.0, 3.0], [-4.0, 5.0]],
                },
                extension: 7,
                revision_form: None,
            }
        );
    }
}

#[test]
fn generated_ruled_spline_surfaces_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    for name in ["rule_sur", "rulesur"] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_ruled_spl_sur_smbh(name, true))),
                &DecodeOptions::default(),
            )
            .expect("ruled spline surface decode");
        let procedural = result.ir.model.procedural_surfaces.first().unwrap();
        assert_eq!(procedural.cache_fit_tolerance, Some(0.025));
        let ProceduralSurfaceDefinition::Ruled { first, second } = &procedural.definition else {
            panic!("expected ruled surface construction")
        };
        assert!(result
            .ir
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *first));
        assert!(result
            .ir
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *second));
        let profiles = [first.clone(), second.clone()];

        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        for (ordinal, profile) in profiles.into_iter().enumerate() {
            source_less
                .model
                .curves
                .iter_mut()
                .find(|curve| curve.id == profile)
                .expect("ruled profile")
                .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
                origin: cadmpeg_ir::math::Point3::new(ordinal as f64, 2.0, 3.0),
                direction: cadmpeg_ir::math::Vector3::new(4.0, 1.0, -2.0),
            };
        }
        let mut encoded = Vec::new();
        F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &source_less,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less ruled surface encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less ruled surface round trip");
        let ProceduralSurfaceDefinition::Ruled { first, second } =
            &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected round-trip ruled surface")
        };
        for profile in [first, second] {
            assert!(matches!(
                round_trip
                    .ir
                    .model
                    .curves
                    .iter()
                    .find(|curve| curve.id == *profile)
                    .map(|curve| &curve.geometry),
                Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                    if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
            ));
        }
    }
}

#[test]
fn generated_sum_spline_surfaces_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    for name in ["sum_spl_sur", "sumsur"] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_sum_spl_sur_smbh(name, true))),
                &DecodeOptions::default(),
            )
            .expect("sum spline surface decode");
        let procedural = result.ir.model.procedural_surfaces.first().unwrap();
        let ProceduralSurfaceDefinition::Sum {
            first,
            second,
            basepoint,
            revision_form: None,
        } = &procedural.definition
        else {
            panic!("expected sum surface construction")
        };
        assert_eq!(
            *basepoint,
            cadmpeg_ir::math::Vector3::new(10.0, -20.0, 30.0)
        );
        let source_curves = [first.clone(), second.clone()];
        assert!(result
            .ir
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *first));
        assert!(result
            .ir
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *second));

        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        for (ordinal, source) in source_curves.into_iter().enumerate() {
            source_less
                .model
                .curves
                .iter_mut()
                .find(|curve| curve.id == source)
                .expect("sum source curve")
                .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
                origin: cadmpeg_ir::math::Point3::new(1.0, ordinal as f64, -1.0),
                direction: cadmpeg_ir::math::Vector3::new(2.0, 3.0, 4.0),
            };
        }
        let mut encoded = Vec::new();
        F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &source_less,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less sum surface encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less sum surface round trip");
        assert!(matches!(
            round_trip.ir.model.procedural_surfaces[0].definition,
            ProceduralSurfaceDefinition::Sum {
                basepoint: cadmpeg_ir::math::Vector3 {
                    x: 10.0,
                    y: -20.0,
                    z: 30.0
                },
                ..
            }
        ));
    }
}

#[test]
fn generated_cacheless_ruled_and_sum_surfaces_are_exact_carriers() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};

    for bytes in [
        synthetic_ruled_spl_sur_smbh("rule_sur", false),
        synthetic_sum_spl_sur_smbh("sum_spl_sur", false),
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&bytes)),
                &DecodeOptions::default(),
            )
            .expect("cacheless exact surface decode");
        let procedural = result
            .ir
            .model
            .procedural_surfaces
            .first()
            .expect("cacheless procedural surface");
        assert!(procedural.cache_fit_tolerance.is_none());
        assert!(matches!(
            procedural.definition,
            ProceduralSurfaceDefinition::Ruled { .. } | ProceduralSurfaceDefinition::Sum { .. }
        ));
        assert!(matches!(
            result
                .ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == procedural.surface)
                .map(|surface| &surface.geometry),
            Some(SurfaceGeometry::Procedural { construction })
                if construction == &procedural.id
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
            .expect("cacheless exact surface source-less encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("cacheless exact surface source-less round trip");
        assert!(matches!(
            round_trip.ir.model.procedural_surfaces[0].definition,
            ProceduralSurfaceDefinition::Ruled { .. } | ProceduralSurfaceDefinition::Sum { .. }
        ));
    }
}

#[test]
fn generated_revolution_spline_surfaces_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    for name in ["rot_spl_sur", "rotsur"] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_rot_spl_sur_smbh(name))),
                &DecodeOptions::default(),
            )
            .expect("revolution spline surface decode");
        let procedural = result.ir.model.procedural_surfaces.first().unwrap();
        let ProceduralSurfaceDefinition::Revolution {
            directrix,
            axis_origin,
            axis_direction,
            angular_interval,
            angular_parameter_interval,
            parameter_interval,
            transposed,
            revision_form: None,
        } = &procedural.definition
        else {
            panic!("expected revolution surface construction")
        };
        assert_eq!(
            *axis_origin,
            cadmpeg_ir::math::Point3::new(10.0, -20.0, 30.0)
        );
        assert_eq!(
            *axis_direction,
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
        );
        assert_eq!(*angular_interval, [0.0, 1.0]);
        assert_eq!(*angular_parameter_interval, None);
        assert_eq!(*parameter_interval, Some([0.0, 1.0]));
        assert!(!transposed);
        assert!(result
            .ir
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *directrix));
        let directrix = directrix.clone();

        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == directrix)
            .expect("revolution directrix")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(2.0, 3.0, 4.0),
            direction: cadmpeg_ir::math::Vector3::new(5.0, -2.0, 1.0),
        };
        let mut encoded = Vec::new();
        F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &source_less,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less revolution surface encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less revolution surface round trip");
        assert!(matches!(
            round_trip.ir.model.procedural_surfaces[0].definition,
            ProceduralSurfaceDefinition::Revolution {
                transposed: false,
                ..
            }
        ));
        let ProceduralSurfaceDefinition::Revolution { directrix, .. } =
            &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            unreachable!()
        };
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *directrix)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1
                    && curve.knots == [0.0, 0.0, 1.0, 1.0]
                    && curve.control_points == [
                        cadmpeg_ir::math::Point3::new(2.0, 3.0, 4.0),
                        cadmpeg_ir::math::Point3::new(7.0, 1.0, 5.0),
                    ]
        ));
    }
}

#[test]
fn generated_offset_spline_surfaces_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    for (name, expected_flags) in [("off_spl_sur", vec![true, false, true]), ("offsur", vec![])] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_off_spl_sur_smbh(name))),
                &DecodeOptions::default(),
            )
            .expect("offset spline surface decode");
        let procedural = result.ir.model.procedural_surfaces.first().unwrap();
        let ProceduralSurfaceDefinition::Offset {
            support,
            revision_form: _,
            distance,
            u_sense,
            v_sense,
            extension_flags,
        } = &procedural.definition
        else {
            panic!("expected offset surface construction")
        };
        assert_eq!(*distance, -12.5);
        assert_eq!((*u_sense, *v_sense), (Some(3), Some(-4)));
        assert_eq!(*extension_flags, expected_flags);
        assert!(result
            .ir
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == *support));

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
            .expect("source-less offset surface encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less offset surface round trip");
        let ProceduralSurfaceDefinition::Offset {
            distance,
            u_sense,
            v_sense,
            extension_flags,
            ..
        } = &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected round-trip offset surface")
        };
        assert_eq!((*distance, *u_sense, *v_sense), (-12.5, Some(3), Some(-4)));
        assert_eq!(*extension_flags, expected_flags);
    }
}

#[test]
fn generated_compound_spline_surface_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_comp_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("compound spline surface decode");
    let procedural = result.ir.model.procedural_surfaces.first().unwrap();
    let ProceduralSurfaceDefinition::Compound {
        parameters,
        components,
    } = &procedural.definition
    else {
        panic!("expected compound surface construction")
    };
    assert_eq!(parameters, &[-0.5, 1.5]);
    assert_eq!(components.len(), 2);
    let solved = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == procedural.surface)
        .expect("compound solved surface");
    let SurfaceGeometry::Nurbs(solved) = &solved.geometry else {
        panic!("expected solved NURBS surface")
    };
    assert!(solved.weights.is_none());
    let rational_component = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == components[1])
        .expect("compound rational component");
    assert!(matches!(
        rational_component.geometry,
        SurfaceGeometry::Nurbs(ref surface) if surface.weights.is_some()
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
        .expect("source-less compound surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less compound surface round trip");
    assert!(matches!(
        round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::Compound { ref parameters, ref components }
            if parameters == &[-0.5, 1.5] && components.len() == 2
    ));
}

#[test]
fn generated_taper_surface_family_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, TaperSurfaceKind};

    let cases = [
        ("taper_spl_sur", 0),
        ("ortho_spl_sur", 1),
        ("orthosur", 1),
        ("edge_tpr_spl_sur", 2),
        ("shadow_tpr_spl_sur", 3),
        ("shadowtapersur", 3),
        ("ruled_tpr_spl_sur", 4),
        ("ruledtapersur", 4),
        ("swept_tpr_spl_sur", 5),
        ("swepttapersur", 5),
    ];
    for (name, expected_kind) in cases {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_taper_spl_sur_smbh(name))),
                &DecodeOptions::default(),
            )
            .expect("taper surface decode");
        let ProceduralSurfaceDefinition::Taper {
            support,
            revision_form: _,
            reference,
            pcurve,
            parameter,
            taper,
        } = &result.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected taper surface")
        };
        assert_eq!(*parameter, 0.35);
        assert!(pcurve.is_some());
        assert!(result
            .ir
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == *support));
        assert!(result
            .ir
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *reference));
        let actual_kind = match taper {
            TaperSurfaceKind::Standard => 0,
            TaperSurfaceKind::Orthogonal { sense: true } => 1,
            TaperSurfaceKind::Edge { .. } => 2,
            TaperSurfaceKind::Shadow { sine, cosine, .. } if (*sine, *cosine) == (0.6, 0.8) => 3,
            TaperSurfaceKind::Ruled { factor, .. } if *factor == 1.25 => 4,
            TaperSurfaceKind::Swept { sine, cosine, .. } if (*sine, *cosine) == (0.6, 0.8) => 5,
            _ => panic!("unexpected taper tail"),
        };
        assert_eq!(actual_kind, expected_kind);
        let reference = reference.clone();

        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == reference)
            .expect("taper reference curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            direction: cadmpeg_ir::math::Vector3::new(4.0, -1.0, 2.0),
        };
        let mut encoded = Vec::new();
        F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &source_less,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less taper encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less taper round trip");
        let ProceduralSurfaceDefinition::Taper { reference, .. } =
            &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected round-trip taper")
        };
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *reference)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1
                    && curve.knots == [0.0, 0.0, 1.0, 1.0]
                    && curve.control_points == [
                        cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
                        cadmpeg_ir::math::Point3::new(5.0, 1.0, 5.0),
                    ]
        ));
    }
}

#[test]
fn generated_loft_surface_decodes_full_nested_graph() {
    use cadmpeg_ir::geometry::{
        LoftBridgeToken, ProceduralSurfaceDefinition, SplineSurfaceParameters,
    };

    for name in ["loft_spl_sur", "loftsur"] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_loft_spl_sur_smbh(name))),
                &DecodeOptions::default(),
            )
            .expect("loft surface decode");
        let ProceduralSurfaceDefinition::Loft {
            sections,
            revision_form: _,
            parameters,
            closures,
            singularities,
            mode,
            bridge,
        } = &result.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected loft surface")
        };
        assert_eq!(
            parameters,
            &SplineSurfaceParameters::OrderedRanges {
                ranges: [[-1.0, 2.0], [-3.0, 4.0]],
            }
        );
        assert_eq!(*closures, [1, 2]);
        assert_eq!(*singularities, [3, 4]);
        assert_eq!(*mode, 2);
        assert_eq!(
            bridge,
            &[
                LoftBridgeToken::Boolean(true),
                LoftBridgeToken::Integer(17),
                LoftBridgeToken::Double(0.125),
                LoftBridgeToken::Text("bridge".into()),
                LoftBridgeToken::Enum(-7),
            ]
        );
        assert!(sections.iter().all(|section| section.entries.len() == 1));
        assert_eq!(
            sections[0].entries[0].profile[0].data.subdata.type_code,
            211
        );
        assert_eq!(
            sections[0].entries[0].profile[0].data.direction,
            Some(cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0))
        );
        assert!(sections[1].entries[0].profile[0].data.direction.is_none());
        assert!(sections
            .iter()
            .flat_map(|section| &section.entries)
            .all(|entry| entry.path.auxiliaries.len() == 1));
        let line_profile = sections[0].entries[0].profile[0].curve.clone();

        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == line_profile)
            .expect("loft line profile")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(4.0, -1.0, 2.0),
            direction: cadmpeg_ir::math::Vector3::new(2.0, 3.0, -1.0),
        };
        let mut encoded = Vec::new();
        F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &source_less,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less loft encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less loft round trip");
        let ProceduralSurfaceDefinition::Loft {
            sections,
            revision_form: _,
            parameters,
            closures,
            singularities,
            mode,
            bridge,
        } = &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected round-trip loft surface")
        };
        assert_eq!(
            parameters,
            &SplineSurfaceParameters::OrderedRanges {
                ranges: [[-1.0, 2.0], [-3.0, 4.0]],
            }
        );
        assert_eq!((*closures, *singularities, *mode), ([1, 2], [3, 4], 2));
        assert_eq!(bridge.len(), 5);
        assert!(sections.iter().all(|section| {
            section.entries.len() == 1
                && section.entries[0].profile.len() == 1
                && section.entries[0].path.auxiliaries.len() == 1
        }));
        assert_eq!(
            sections[0].entries[0].profile[0].data.direction,
            Some(cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0))
        );
        assert!(sections[1].entries[0].profile[0].data.direction.is_none());
        let profile = &sections[0].entries[0].profile[0].curve;
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *profile)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1
                    && curve.knots == [-1.0, -1.0, 2.0, 2.0]
                    && curve.control_points == [
                        cadmpeg_ir::math::Point3::new(2.0, -4.0, 3.0),
                        cadmpeg_ir::math::Point3::new(8.0, 5.0, 0.0),
                    ]
        ));
    }
}

#[test]
fn generated_net_surface_decodes_and_writes_full_graph() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_net_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("net surface decode");
    let ProceduralSurfaceDefinition::Net { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected net surface")
    };
    assert!(construction
        .sections
        .iter()
        .all(|section| section.entries.len() == 1));
    assert_eq!(construction.frame_parameters[11], 1.1);
    assert_eq!(construction.flag, 17);
    assert_eq!(construction.directions[2].z, 1.0);
    assert!(construction
        .formulas
        .iter()
        .all(|formula| formula.name == "null_law"));
    assert_eq!(construction.discontinuities[0], [0.25]);

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
        .expect("source-less net surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less net surface round trip");
    assert!(matches!(
        round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::Net { .. }
    ));
}

#[test]
fn generated_profile_first_sweep_decodes_and_writes_full_graph() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SweepSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_profile_first_sweep_smbh())),
            &DecodeOptions::default(),
        )
        .expect("profile-first sweep decode");
    let ProceduralSurfaceDefinition::Sweep {
        native: Some(native),
        ..
    } = &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected native sweep")
    };
    assert_eq!(native.primary_kind, 3);
    let SweepSurfaceLayout::ProfileFirst {
        secondary_kind,
        directions,
        origin,
        parameters,
        formulas,
    } = &native.layout
    else {
        panic!("expected profile-first sweep")
    };
    assert_eq!(*secondary_kind, 4);
    assert_eq!(directions[2].z, 1.0);
    assert_eq!(origin.z, 30.0);
    assert_eq!(*parameters, [0.1, 0.2, 0.3, 0.4]);
    assert!(formulas.iter().all(|formula| formula.name == "null_law"));
    assert_eq!(native.discontinuities[0], [0.25]);

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
        .expect("source-less profile-first sweep encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less profile-first sweep round trip");
    assert!(matches!(
        round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::Sweep {
            native: Some(_),
            ..
        }
    ));
}

#[test]
fn generated_t_spline_surface_decodes_and_writes_inline_subtransform() {
    use cadmpeg_ir::geometry::{
        ProceduralSurfaceDefinition, TSplineSubtransform, TSplineSurfaceConstruction,
    };

    fn construction(definition: &ProceduralSurfaceDefinition) -> &TSplineSurfaceConstruction {
        let ProceduralSurfaceDefinition::TSpline { construction } = definition else {
            panic!("expected T-spline surface")
        };
        construction
    }

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_t_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("T-spline surface decode");
    let native = construction(&decoded.ir.model.procedural_surfaces[0].definition).clone();
    assert_eq!(native.parameter_ranges, [[-20.0, 30.0], [-40.0, 50.0]]);
    assert_eq!((native.type_code, native.trailing_value), (7, 9));
    let TSplineSubtransform::Inline {
        program,
        separator,
        values,
    } = &native.subtransform
    else {
        panic!("expected inline T-spline subtransform")
    };
    assert!(program.contains("v 1 0 0 0"));
    assert_eq!(*separator, Some(false));
    assert_eq!(values, "100verts 1 2\n");
    let graph = native
        .program_graph
        .as_ref()
        .expect("parsed T-spline graph");
    assert_eq!(graph.headers.len(), 2);
    assert_eq!(graph.records.len(), 3);
    assert_eq!(graph.records[0].kind, "v");
    assert!(graph.unparsed_lines.is_empty());
    assert_eq!(
        native.values_graph.as_ref().unwrap().records[0].kind,
        "100verts"
    );

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
        .expect("source-less T-spline encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less T-spline round trip");
    assert_eq!(
        construction(&round_trip.ir.model.procedural_surfaces[0].definition),
        &native
    );
}

#[test]
fn generated_helix_surfaces_decode_and_write_exact_constructions() {
    use cadmpeg_ir::geometry::{HelixSurfaceProfile, ProceduralSurfaceDefinition, SurfaceGeometry};

    for circular in [true, false] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_helix_surface_smbh(circular))),
                &DecodeOptions::default(),
            )
            .expect("helix surface decode");
        let ProceduralSurfaceDefinition::Helix { construction } =
            &decoded.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected helix surface")
        };
        assert_eq!(construction.angle_range, [-0.5, 0.5]);
        assert_eq!(construction.path.center.z, 30.0);
        assert_eq!(construction.path.pitch.z, 40.0);
        assert_eq!(
            circular,
            matches!(construction.profile, HelixSurfaceProfile::Circle { .. })
        );

        let surface_id = decoded.ir.model.procedural_surfaces[0].surface.clone();
        let mut source_less = decoded.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let surface = source_less
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == surface_id)
            .unwrap();
        assert!(
            matches!(
                &surface.geometry,
                SurfaceGeometry::Procedural { construction }
                    if *construction == source_less.model.procedural_surfaces[0].id
            ),
            "unexpected helix carrier: {:?}",
            surface.geometry
        );
        let mut encoded = Vec::new();
        F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &source_less,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less helix surface encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less helix surface round trip");
        assert!(matches!(
            round_trip.ir.model.procedural_surfaces[0].definition,
            ProceduralSurfaceDefinition::Helix { .. }
        ));
    }
}

#[test]
fn generated_source_less_rejects_duplicate_procedural_surface_owners() {
    for (smbh, label) in [
        (synthetic_cyl_spl_sur_smbh(), "cached"),
        (synthetic_helix_surface_smbh(true), "cacheless"),
    ] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("generated {label} surface decode: {error}"));
        let mut source_less = decoded.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut duplicate = source_less.model.procedural_surfaces[0].clone();
        duplicate.id = format!("generated:duplicate-{label}").into();
        source_less.model.procedural_surfaces.push(duplicate);

        let error = F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &source_less,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut Vec::new()))
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("multiple procedural constructions"),
            "unexpected {label} duplicate-owner error: {error}"
        );
    }
}

#[test]
fn generated_source_less_refuses_procedural_construction_loss_on_analytic_carriers() {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_cyl_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated procedural surface decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let surface_id = source_less.model.procedural_surfaces[0].surface.clone();
    source_less
        .model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == surface_id)
        .unwrap()
        .geometry = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("analytic carrier must not discard its procedural surface");
    assert!(error
        .to_string()
        .contains("cannot retain its construction on analytic carrier"));

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_helix_curve_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated procedural curve decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let curve_id = source_less.model.procedural_curves[0].curve.clone();
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry = CurveGeometry::Line {
        origin: Point3::new(0.0, 0.0, 0.0),
        direction: Vector3::new(1.0, 0.0, 0.0),
    };
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("analytic carrier must not discard its procedural curve");
    assert!(error
        .to_string()
        .contains("cannot retain its construction on carrier"));
}

#[test]
fn generated_minimal_deformable_surface_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{DeformableSurfaceData, ProceduralSurfaceDefinition};
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_minimal_deformable_surface_smbh())),
            &DecodeOptions::default(),
        )
        .expect("deformable surface decode");
    let ProceduralSurfaceDefinition::Deformable { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected deformable surface")
    };
    let DeformableSurfaceData::Minimal { vectors, selector } = &construction.data else {
        panic!("expected minimal deformable surface")
    };
    assert_eq!(vectors[2].z, 1.0);
    assert_eq!(*selector, 0);
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
        .unwrap();
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::Deformable { .. }
    ));
}

#[test]
fn generated_framed_deformable_surfaces_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::{DeformableSurfaceData, ProceduralSurfaceDefinition};
    for mode in [1, 3] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_framed_deformable_surface_smbh(
                    mode,
                ))),
                &DecodeOptions::default(),
            )
            .unwrap();
        let ProceduralSurfaceDefinition::Deformable { construction } =
            &decoded.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected deformable surface")
        };
        match &construction.data {
            DeformableSurfaceData::Plain {
                frame,
                parameter_triples,
            } => {
                assert_eq!(mode, 1);
                assert_eq!(frame.point.z, 60.0);
                assert_eq!(parameter_triples.len(), 2);
            }
            DeformableSurfaceData::Guided {
                frame,
                guide_parameter,
                ..
            } => {
                assert_eq!(mode, 3);
                assert_eq!(frame.point.z, 60.0);
                assert_eq!(*guide_parameter, 0.9);
            }
            DeformableSurfaceData::Minimal { .. }
            | DeformableSurfaceData::SurfaceCurve { .. }
            | DeformableSurfaceData::Full { .. } => {
                panic!("wrong mode")
            }
        }
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
            .unwrap();
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap();
        assert!(matches!(
            round_trip.ir.model.procedural_surfaces[0].definition,
            ProceduralSurfaceDefinition::Deformable { .. }
        ));
    }
}

#[test]
fn generated_surface_curve_deformable_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{DeformableSurfaceData, ProceduralSurfaceDefinition};
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_surface_curve_deformable_smbh())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let ProceduralSurfaceDefinition::Deformable { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!()
    };
    let DeformableSurfaceData::SurfaceCurve {
        native_id,
        first_parameter,
        selector,
        second_parameter,
        curve,
        parameter_triples,
        ..
    } = &construction.data
    else {
        panic!()
    };
    assert_eq!((*native_id, *selector), (42, 3));
    assert_eq!(parameter_triples, &[[0.1, 0.2, 0.3]]);
    let curve = curve.clone();
    let range = [*first_parameter, *second_parameter];
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    source_less
        .model
        .curves
        .iter_mut()
        .find(|candidate| candidate.id == curve)
        .expect("surface-curve deformable curve")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(1.0, -2.0, 3.0),
        direction: cadmpeg_ir::math::Vector3::new(4.0, 2.0, -1.0),
    };
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .unwrap();
    let round = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        round.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::Deformable { .. }
    ));
    assert!(round.ir.model.curves.iter().any(|curve| matches!(
        &curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve)
            if curve.degree == 1
                && curve.knots == [range[0], range[0], range[1], range[1]]
    )));
}

#[test]
fn generated_full_deformable_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{DeformableSurfaceData, ProceduralSurfaceDefinition};
    for expected_version_value in [None, Some(226)] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_full_deformable_surface_smbh(
                    expected_version_value,
                ))),
                &DecodeOptions::default(),
            )
            .unwrap();
        let ProceduralSurfaceDefinition::Deformable { construction } =
            &decoded.ir.model.procedural_surfaces[0].definition
        else {
            panic!()
        };
        let DeformableSurfaceData::Full {
            selector,
            native_id,
            first_parameter,
            version_value,
            second_parameter,
            curve,
            frames,
            trailing_value,
            ..
        } = &construction.data
        else {
            panic!()
        };
        assert_eq!((*selector, *native_id), (7, 42));
        assert_eq!(*version_value, expected_version_value);
        assert_eq!(frames[0].parameter, 0.4);
        assert_eq!(frames[1].parameter, 0.5);
        assert_eq!(*trailing_value, 99);
        let curve = curve.clone();
        let range = [*first_parameter, *second_parameter];
        let mut source_less = decoded.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less
            .model
            .curves
            .iter_mut()
            .find(|candidate| candidate.id == curve)
            .expect("full deformable curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(-1.0, 2.0, 3.0),
            direction: cadmpeg_ir::math::Vector3::new(3.0, -4.0, 2.0),
        };
        let mut encoded = Vec::new();
        F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &source_less,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut encoded))
            .unwrap();
        let round = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap();
        let ProceduralSurfaceDefinition::Deformable { construction } =
            &round.ir.model.procedural_surfaces[0].definition
        else {
            panic!()
        };
        assert!(matches!(
            construction.data,
            DeformableSurfaceData::Full { version_value, .. }
                if version_value == expected_version_value
        ));
        assert!(round.ir.model.curves.iter().any(|curve| matches!(
            &curve.geometry,
            cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve)
                if curve.degree == 1
                    && curve.knots == [range[0], range[0], range[1], range[1]]
        )));
    }
}

#[test]
fn generated_t_spline_surface_resolves_shared_subtransform_source_less() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, TSplineSubtransform};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_referenced_t_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("referenced T-spline decode");
    let ProceduralSurfaceDefinition::TSpline { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected T-spline surface")
    };
    let TSplineSubtransform::Reference {
        index,
        resolved: Some(resolved),
    } = &construction.subtransform
    else {
        panic!("expected resolved T-spline reference")
    };
    assert!(*index >= 0);
    assert!(matches!(
        resolved.as_ref(),
        TSplineSubtransform::Inline { program, .. } if program.contains("v 1 0 0 0")
    ));
    assert_eq!(
        construction.program_graph.as_ref().unwrap().records.len(),
        1
    );
    assert_eq!(
        construction.values_graph.as_ref().unwrap().records[0].kind,
        "100verts"
    );

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
        .expect("source-less referenced T-spline encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less referenced T-spline round trip");
    let ProceduralSurfaceDefinition::TSpline { construction } =
        &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected round-trip T-spline surface")
    };
    assert!(matches!(
        construction.subtransform,
        TSplineSubtransform::Inline { .. }
    ));
}

#[test]
fn generated_explicit_formula_sweep_decodes_and_writes_full_graph() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SweepSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_explicit_formula_sweep_smbh())),
            &DecodeOptions::default(),
        )
        .expect("explicit formula sweep decode");
    let ProceduralSurfaceDefinition::Sweep {
        profile,
        spine,
        native: Some(native),
        ..
    } = &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected native sweep")
    };
    let SweepSurfaceLayout::ExplicitFormula {
        mode,
        profile_range,
        profile_frame,
        origin,
        path_range,
        formula,
        ..
    } = &native.layout
    else {
        panic!("expected explicit formula sweep")
    };
    assert_eq!(*mode, 7);
    assert_eq!(*profile_range, [-0.5, 1.5]);
    assert_eq!(profile_frame.as_ref().unwrap().0.z, 30.0);
    assert_eq!(origin.z, 60.0);
    assert_eq!(*path_range, [-20.0, 30.0]);
    assert_eq!(formula.name, "null_law");
    let profile = profile.clone();
    let spine = spine.clone();

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    for (ordinal, curve_id) in [&profile, &spine].into_iter().enumerate() {
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == *curve_id)
            .expect("explicit sweep curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(ordinal as f64, 2.0, -1.0),
            direction: cadmpeg_ir::math::Vector3::new(3.0, -2.0, 4.0),
        };
    }
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less explicit formula sweep encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less explicit formula sweep round trip");
    let ProceduralSurfaceDefinition::Sweep {
        profile,
        spine,
        native: Some(native),
        ..
    } = &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected round-trip explicit formula sweep")
    };
    assert!(matches!(
        native.layout,
        SweepSurfaceLayout::ExplicitFormula { .. }
    ));
    for (curve_id, knots) in [
        (profile, [-0.5, -0.5, 1.5, 1.5]),
        (spine, [-2.0, -2.0, 3.0, 3.0]),
    ] {
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *curve_id)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1 && curve.knots == knots
        ));
    }
}

#[test]
fn generated_source_less_sweep_refuses_missing_native_graph() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let mut decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_explicit_formula_sweep_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated native sweep decode")
        .ir;
    decoded.source = None;
    decoded.set_native_unknowns("f3d", &[]).unwrap();
    let ProceduralSurfaceDefinition::Sweep { native, .. } =
        &mut decoded.model.procedural_surfaces[0].definition
    else {
        panic!("expected generated sweep")
    };
    *native = None;

    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &decoded,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("a sweep without its native graph must not be guessed");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::NotImplemented(message)
            if message.contains("lacks its native construction graph")
    ));
}

#[test]
fn generated_explicit_guide_sweep_decodes_and_writes_full_graph() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SweepSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_explicit_guide_sweep_smbh())),
            &DecodeOptions::default(),
        )
        .expect("explicit guide sweep decode");
    let ProceduralSurfaceDefinition::Sweep {
        profile,
        spine,
        native: Some(native),
        ..
    } = &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected native sweep")
    };
    let SweepSurfaceLayout::ExplicitGuide {
        mode,
        profile_range,
        profile_frame,
        path_range,
        guide_curve,
        guide_range,
        guide_modes,
        guide_parameters,
        trailing_flags,
        ..
    } = &native.layout
    else {
        panic!("expected explicit guide sweep")
    };
    assert_eq!(*mode, 8);
    assert!(profile_frame.is_none());
    assert_eq!(*guide_range, [0.0, 1.0]);
    assert_eq!(*guide_modes, [11, 12]);
    assert_eq!(guide_parameters[5], 0.6);
    assert_eq!(*trailing_flags, [true, false, true]);
    let bounded_curves = [
        (profile.clone(), *profile_range),
        (spine.clone(), [path_range[0] / 10.0, path_range[1] / 10.0]),
        (guide_curve.clone(), *guide_range),
    ];

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    for (ordinal, (curve_id, _)) in bounded_curves.iter().enumerate() {
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == *curve_id)
            .expect("explicit guide sweep curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(ordinal as f64, -2.0, 1.0),
            direction: cadmpeg_ir::math::Vector3::new(2.0, 4.0, -3.0),
        };
    }
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less explicit guide sweep encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less explicit guide sweep round trip");
    assert!(matches!(
        &round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::Sweep {
            native: Some(native),
            ..
        } if matches!(native.layout, SweepSurfaceLayout::ExplicitGuide { .. })
    ));
    for (curve_id, range) in bounded_curves {
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == curve_id)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1
                    && curve.knots == [range[0], range[0], range[1], range[1]]
        ));
    }
}

#[test]
fn generated_explicit_surface_sweep_decodes_and_writes_full_graph() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SweepSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_explicit_surface_sweep_smbh())),
            &DecodeOptions::default(),
        )
        .expect("explicit surface sweep decode");
    let ProceduralSurfaceDefinition::Sweep {
        profile,
        spine,
        native: Some(native),
        ..
    } = &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected native sweep")
    };
    let SweepSurfaceLayout::ExplicitSurface {
        mode,
        profile_range,
        path_range,
        singularity,
        auxiliary_curve,
        support_flag,
        legacy_flag,
        ..
    } = &native.layout
    else {
        panic!("expected explicit surface sweep")
    };
    assert_eq!((*mode, *singularity), (9, 1));
    assert!(auxiliary_curve.is_some());
    assert!(*support_flag);
    assert_eq!(*legacy_flag, Some(false));
    let bounded_curves = [
        (profile.clone(), *profile_range),
        (spine.clone(), [path_range[0] / 10.0, path_range[1] / 10.0]),
    ];

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    for (ordinal, (curve_id, _)) in bounded_curves.iter().enumerate() {
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == *curve_id)
            .expect("explicit surface sweep curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(ordinal as f64, 1.0, -2.0),
            direction: cadmpeg_ir::math::Vector3::new(4.0, 2.0, -3.0),
        };
    }
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less explicit surface sweep encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less explicit surface sweep round trip");
    assert!(matches!(
        &round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::Sweep {
            native: Some(native),
            ..
        } if matches!(native.layout, SweepSurfaceLayout::ExplicitSurface { .. })
    ));
    for (curve_id, range) in bounded_curves {
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == curve_id)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1
                    && curve.knots == [range[0], range[0], range[1], range[1]]
        ));
    }
}

#[test]
fn generated_law_driven_sweep_decodes_and_writes_full_graph() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition, SweepSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_law_driven_sweep_smbh())),
            &DecodeOptions::default(),
        )
        .expect("law-driven sweep decode");
    let ProceduralSurfaceDefinition::Sweep {
        profile,
        spine,
        native: Some(native),
        ..
    } = &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected native sweep")
    };
    let SweepSurfaceLayout::LawDriven {
        mode,
        profile_range,
        first_law,
        first_mode,
        second_law,
        formula_mode,
        formula,
        path_range,
        ..
    } = &native.layout
    else {
        panic!("expected law-driven sweep")
    };
    assert_eq!((*mode, *first_mode, *formula_mode), (10, 21, 23));
    assert!(matches!(first_law.as_ref(), LawExpression::Double { value } if *value == 2.5));
    assert!(matches!(second_law.as_ref(), LawExpression::Vector { value } if value.z == 3.0));
    assert_eq!(formula.name, "null_law");
    let bounded_curves = [
        (profile.clone(), *profile_range),
        (spine.clone(), *path_range),
    ];

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    for (ordinal, (curve_id, _)) in bounded_curves.iter().enumerate() {
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == *curve_id)
            .expect("law-driven sweep curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(ordinal as f64, -1.0, 2.0),
            direction: cadmpeg_ir::math::Vector3::new(3.0, 4.0, -2.0),
        };
    }
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less law-driven sweep encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less law-driven sweep round trip");
    assert!(matches!(
        &round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::Sweep {
            native: Some(native),
            ..
        } if matches!(native.layout, SweepSurfaceLayout::LawDriven { .. })
    ));
    for (curve_id, range) in bounded_curves {
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == curve_id)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1
                    && curve.knots == [range[0], range[0], range[1], range[1]]
        ));
    }
}

#[test]
fn generated_legacy_surface_names_select_modern_layouts() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let cases = [
        (
            renamed_generated_subtype(
                synthetic_skin_spl_sur_smbh(0, false),
                "skin_spl_sur",
                "skinsur",
            ),
            "skin",
        ),
        (
            renamed_generated_subtype(synthetic_net_spl_sur_smbh(), "net_spl_sur", "netsur"),
            "net",
        ),
        (
            renamed_generated_subtype(
                synthetic_profile_first_sweep_smbh(),
                "sweep_spl_sur",
                "sweepsur",
            ),
            "sweep",
        ),
        (
            renamed_generated_subtype(
                synthetic_scaled_compound_loft_smbh(true),
                "scaled_cloft_spl_sur",
                "sclclftsur",
            ),
            "scaled_compound_loft",
        ),
        (
            renamed_generated_subtype(synthetic_cyl_spl_sur_smbh(), "cyl_spl_sur", "cylsur"),
            "extrusion",
        ),
    ];
    for (smbh, expected) in cases {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{expected} legacy decode: {error}"));
        let definition = &decoded.ir.model.procedural_surfaces[0].definition;
        assert!(
            matches!(
                (expected, definition),
                ("skin", ProceduralSurfaceDefinition::Skin { .. })
                    | ("net", ProceduralSurfaceDefinition::Net { .. })
                    | ("sweep", ProceduralSurfaceDefinition::Sweep { .. })
                    | (
                        "scaled_compound_loft",
                        ProceduralSurfaceDefinition::ScaledCompoundLoft { .. }
                    )
                    | ("extrusion", ProceduralSurfaceDefinition::Extrusion { .. })
            ),
            "wrong definition for {expected}: {definition:?}"
        );
    }
}

#[test]
fn generated_procedural_surface_tolerance_presence_matches_native_grammar() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let required = [
        (
            synthetic_minimal_deformable_surface_smbh(),
            "deformable surface",
        ),
        (synthetic_t_spl_sur_smbh(), "T-spline surface"),
        (
            synthetic_exact_spl_sur_smbh("exact_spl_sur"),
            "exact spline surface",
        ),
        (
            synthetic_variable_blend_smbh("var_blend_spl_sur"),
            "variable blend",
        ),
        (
            synthetic_full_rolling_ball_smbh("rb_blend_spl_sur"),
            "rolling-ball blend",
        ),
        (synthetic_skin_spl_sur_smbh(0, false), "skin surface"),
        (synthetic_net_spl_sur_smbh(), "net surface"),
        (synthetic_profile_first_sweep_smbh(), "sweep surface"),
    ];
    for (smbh, family) in required {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{family} decode: {error}"));
        assert!(decoded.ir.model.procedural_surfaces[0]
            .cache_fit_tolerance
            .is_some());
        let mut source_less = decoded.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less.model.procedural_surfaces[0].cache_fit_tolerance = None;
        let error = F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &source_less,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut Vec::new()))
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains(&format!("{family} requires a native cache-fit tolerance")),
            "unexpected {family} error: {error}"
        );
    }

    let optional = [
        (synthetic_comp_spl_sur_smbh(), "compound"),
        (synthetic_taper_spl_sur_smbh("taper_spl_sur"), "taper"),
        (synthetic_ruled_spl_sur_smbh("rule_sur", true), "ruled"),
        (synthetic_sum_spl_sur_smbh("sum_spl_sur", true), "sum"),
        (synthetic_rot_spl_sur_smbh("rot_spl_sur"), "revolution"),
        (synthetic_off_spl_sur_smbh("off_spl_sur"), "offset"),
        (synthetic_cyl_spl_sur_smbh(), "extrusion"),
        (
            synthetic_g2_blend_spl_sur_smbh("g2_blend_spl_sur", false),
            "G2 blend",
        ),
    ];
    for (smbh, family) in optional {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("optional-tolerance surface decode");
        let mut source_less = decoded.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less.model.procedural_surfaces[0].cache_fit_tolerance = None;
        let mut encoded = Vec::new();
        F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &source_less,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less surface without optional tolerance");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{family} round trip: {error}"));
        assert_eq!(
            round_trip.ir.model.procedural_surfaces.len(),
            1,
            "{family} procedural surface was not reconstructed"
        );
        assert_eq!(
            round_trip.ir.model.procedural_surfaces[0].cache_fit_tolerance,
            None
        );
    }

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_loft_spl_sur_smbh("loft_spl_sur"))),
            &DecodeOptions::default(),
        )
        .expect("loft decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    source_less.model.procedural_surfaces[0].cache_fit_tolerance = None;
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less loft without optional tolerance");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less loft round trip");
    assert!(matches!(
        round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::Loft { .. }
    ));
    assert_eq!(
        round_trip.ir.model.procedural_surfaces[0].cache_fit_tolerance,
        None
    );
}
