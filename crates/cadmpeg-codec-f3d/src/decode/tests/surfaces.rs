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
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::F3dCodec;

#[test]
fn zero_payload_mesh_surface_is_typed_as_a_native_sentinel() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let source = f3d_with_smbh(&synthetic_geometry_with_mesh_surface_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("mesh-surface decode");

    assert_eq!(result.ir().model.faces.len(), 1);
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Unknown { .. }
    ));
    let native = f3d_native(result.ir());
    assert_eq!(native.mesh_surface_sentinels.len(), 1);
    assert_eq!(
        native.mesh_surface_sentinels[0].surface,
        result.ir().model.surfaces[0].id
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.severity == cadmpeg_ir::report::Severity::Info
            && loss.message.contains("zero-payload mesh_surface")
    }));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("spline/procedural surfaces")));

    let mut replay = Vec::new();
    F3dCodec
        .plan(
            EncodeInput::new(result.ir(), Some(result.source_fidelity())),
            TargetRequest::Inherit,
        )
        .and_then(|plan| plan.write_to(&mut replay))
        .expect("mesh-surface native replay");
    assert_eq!(replay, source);

    let mut edited = result.ir().clone();
    f3d_native_mut(&mut edited).mesh_surface_sentinels[0].id =
        "f3d:asm:mesh-surface-sentinel#edited".into();
    let error = F3dCodec
        .plan(
            EncodeInput::new(&edited, Some(result.source_fidelity())),
            TargetRequest::Inherit,
        )
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("mesh-surface structural metadata is immutable");
    assert!(error.to_string().contains("edits beyond supported"));

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less.model.surfaces[0].geometry = SurfaceGeometry::Unknown { record: None };
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        let procedural = result.ir().model.procedural_surfaces.first().unwrap();
        assert_eq!(procedural.cache_fit_tolerance(), Some(0.015));
        assert_eq!(
            procedural.definition(),
            &ProceduralSurfaceDefinition::Exact {
                parameters: SplineSurfaceParameters::OrderedRanges {
                    ranges: [[-2.0, 3.0], [-4.0, 5.0]],
                },
                extension: 7,
                revision_form: None,
            }
        );

        let (mut source_less, _, _) = result.into_parts();
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less exact spline surface encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less exact spline surface round trip");
        assert_eq!(
            round_trip.ir().model.procedural_surfaces[0].definition(),
            &ProceduralSurfaceDefinition::Exact {
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
        let procedural = result.ir().model.procedural_surfaces.first().unwrap();
        assert_eq!(procedural.cache_fit_tolerance(), Some(0.025));
        let ProceduralSurfaceDefinition::Ruled { first, second } = procedural.definition() else {
            panic!("expected ruled surface construction")
        };
        assert!(result
            .ir()
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *first));
        assert!(result
            .ir()
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *second));
        let profiles = [first.clone(), second.clone()];

        let (mut source_less, _, _) = result.into_parts();
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
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less ruled surface encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less ruled surface round trip");
        let ProceduralSurfaceDefinition::Ruled { first, second } =
            &round_trip.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected round-trip ruled surface")
        };
        for profile in [first, second] {
            assert!(matches!(
                round_trip
                    .ir()
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
        let procedural = result.ir().model.procedural_surfaces.first().unwrap();
        let ProceduralSurfaceDefinition::Sum {
            first,
            second,
            basepoint,
            revision_form: None,
        } = procedural.definition()
        else {
            panic!("expected sum surface construction")
        };
        assert_eq!(
            *basepoint,
            cadmpeg_ir::math::Vector3::new(10.0, -20.0, 30.0)
        );
        let source_curves = [first.clone(), second.clone()];
        assert!(result
            .ir()
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *first));
        assert!(result
            .ir()
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *second));

        let (mut source_less, _, _) = result.into_parts();
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
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less sum surface encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less sum surface round trip");
        assert!(matches!(
            round_trip.ir().model.procedural_surfaces[0].definition(),
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
            .ir()
            .model
            .procedural_surfaces
            .first()
            .expect("cacheless procedural surface");
        assert!(procedural.cache_fit_tolerance().is_none());
        assert!(matches!(
            procedural.definition(),
            ProceduralSurfaceDefinition::Ruled { .. } | ProceduralSurfaceDefinition::Sum { .. }
        ));
        assert!(matches!(
            result
                .ir()
                .model
                .surfaces
                .iter()
                .find(|surface| {
                    result.ir().model.procedural_surface_owner(&procedural.id)
                        == Some(&surface.id)
                })
                .map(|surface| &surface.geometry),
            Some(SurfaceGeometry::Procedural { construction, .. })
                if construction == &procedural.id
        ));

        let (mut source_less, _, _) = result.into_parts();
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("cacheless exact surface source-less encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("cacheless exact surface source-less round trip");
        assert!(matches!(
            round_trip.ir().model.procedural_surfaces[0].definition(),
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
        let procedural = result.ir().model.procedural_surfaces.first().unwrap();
        let ProceduralSurfaceDefinition::Revolution {
            directrix,
            axis_origin,
            axis_direction,
            angular_interval,
            angular_parameter_interval,
            parameter_interval,
            transposed,
            revision_form: None,
        } = procedural.definition()
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
            .ir()
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *directrix));
        let directrix = directrix.clone();

        let (mut source_less, _, _) = result.into_parts();
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
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less revolution surface encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less revolution surface round trip");
        assert!(matches!(
            round_trip.ir().model.procedural_surfaces[0].definition(),
            ProceduralSurfaceDefinition::Revolution {
                transposed: false,
                ..
            }
        ));
        let ProceduralSurfaceDefinition::Revolution { directrix, .. } =
            &round_trip.ir().model.procedural_surfaces[0].definition()
        else {
            unreachable!()
        };
        assert!(matches!(
            round_trip
                .ir()
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
        let procedural = result.ir().model.procedural_surfaces.first().unwrap();
        let ProceduralSurfaceDefinition::Offset {
            support,
            revision_form: _,
            distance,
            u_sense,
            v_sense,
            support_extension: _,
            extension_flags,
        } = procedural.definition()
        else {
            panic!("expected offset surface construction")
        };
        assert_eq!(*distance, -12.5);
        assert_eq!((*u_sense, *v_sense), (Some(3), Some(-4)));
        assert_eq!(*extension_flags, expected_flags);
        assert!(result
            .ir()
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == *support));

        let (mut source_less, _, _) = result.into_parts();
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        } = &round_trip.ir().model.procedural_surfaces[0].definition()
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
    let procedural = result.ir().model.procedural_surfaces.first().unwrap();
    let ProceduralSurfaceDefinition::Compound {
        parameters,
        components,
    } = procedural.definition()
    else {
        panic!("expected compound surface construction")
    };
    assert_eq!(parameters, &[-0.5, 1.5]);
    assert_eq!(components.len(), 2);
    let solved = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| {
            result.ir().model.procedural_surface_owner(&procedural.id) == Some(&surface.id)
        })
        .expect("compound solved surface");
    let Some(SurfaceGeometry::Nurbs(solved)) = solved.geometry.solved_cache() else {
        panic!("expected solved NURBS surface")
    };
    assert!(solved.weights.is_none());
    let rational_component = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == components[1])
        .expect("compound rational component");
    assert!(matches!(
        rational_component.geometry,
        SurfaceGeometry::Nurbs(ref surface) if surface.weights.is_some()
    ));

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less compound surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less compound surface round trip");
    assert!(matches!(
        round_trip.ir().model.procedural_surfaces[0].definition(),
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
        } = &result.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected taper surface")
        };
        assert_eq!(*parameter, 0.35);
        assert!(pcurve.is_some());
        assert!(result
            .ir()
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == *support));
        assert!(result
            .ir()
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

        let (mut source_less, _, _) = result.into_parts();
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
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less taper encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less taper round trip");
        let ProceduralSurfaceDefinition::Taper { reference, .. } =
            &round_trip.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected round-trip taper")
        };
        assert!(matches!(
            round_trip
                .ir()
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
        } = &result.ir().model.procedural_surfaces[0].definition()
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
            sections[0].entries[0].profile[0].form.subdata().type_code(),
            211
        );
        assert_eq!(
            sections[0].entries[0].profile[0].form.direction().copied(),
            Some(cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0))
        );
        assert!(sections[1].entries[0].profile[0].form.direction().is_none());
        assert!(sections
            .iter()
            .flat_map(|section| &section.entries)
            .all(|entry| entry.path.auxiliaries.len() == 1));
        let line_profile = sections[0].entries[0].profile[0].curve.clone();

        let (mut source_less, _, _) = result.into_parts();
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
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        } = &round_trip.ir().model.procedural_surfaces[0].definition()
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
            sections[0].entries[0].profile[0].form.direction().copied(),
            Some(cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0))
        );
        assert!(sections[1].entries[0].profile[0].form.direction().is_none());
        let profile = &sections[0].entries[0].profile[0].curve;
        assert!(matches!(
            round_trip
                .ir()
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
        &decoded.ir().model.procedural_surfaces[0].definition()
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

    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less net surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less net surface round trip");
    assert!(matches!(
        round_trip.ir().model.procedural_surfaces[0].definition(),
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
    } = &decoded.ir().model.procedural_surfaces[0].definition()
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

    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less profile-first sweep encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less profile-first sweep round trip");
    assert!(matches!(
        round_trip.ir().model.procedural_surfaces[0].definition(),
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
    let native = construction(&decoded.ir().model.procedural_surfaces[0].definition()).clone();
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
    let graph = native.program_graph().expect("parsed T-spline graph");
    assert_eq!(graph.headers.len(), 2);
    assert_eq!(graph.records.len(), 3);
    assert_eq!(graph.records[0].kind, "v");
    assert!(graph.unparsed_lines.is_empty());
    assert_eq!(native.values_graph().unwrap().records[0].kind, "100verts");

    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less T-spline encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less T-spline round trip");
    assert_eq!(
        construction(&round_trip.ir().model.procedural_surfaces[0].definition()),
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
            &decoded.ir().model.procedural_surfaces[0].definition()
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

        let surface_id = decoded
            .ir()
            .model
            .procedural_surface_owner(&decoded.ir().model.procedural_surfaces[0].id)
            .expect("helix surface owner")
            .clone();
        let (mut source_less, _, _) = decoded.into_parts();
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
                SurfaceGeometry::Procedural { construction, .. }
                    if *construction == source_less.model.procedural_surfaces[0].id
            ),
            "unexpected helix carrier: {:?}",
            surface.geometry
        );
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less helix surface encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less helix surface round trip");
        assert!(matches!(
            round_trip.ir().model.procedural_surfaces[0].definition(),
            ProceduralSurfaceDefinition::Helix { .. }
        ));
    }
}
