// SPDX-License-Identifier: Apache-2.0
//! Procedural-domain synthetic tests and fixtures.

use super::*;

#[test]
fn generated_procedural_curve_optional_tolerance_absence_round_trips() {
    let cases = [
        (synthetic_geometry_with_exact_curve_smbh(), "exact"),
        (synthetic_geometry_with_law_curve_smbh(), "law"),
        (synthetic_geometry_with_projection_smbh(), "projection"),
        (
            synthetic_geometry_with_early_close_projection_smbh(),
            "early-close projection",
        ),
        (synthetic_geometry_with_compound_curve_smbh(), "compound"),
        (
            synthetic_geometry_with_surface_curve_smbh("surf_int_cur"),
            "surface curve",
        ),
        (
            synthetic_geometry_with_silhouette_smbh("para_silh_int_cur", None),
            "silhouette",
        ),
        (
            synthetic_geometry_with_surface_offset_smbh(),
            "surface offset",
        ),
        (synthetic_geometry_with_spring_smbh(), "spring"),
        (
            synthetic_geometry_with_three_surface_intersection_smbh(),
            "three-surface intersection",
        ),
        (
            synthetic_geometry_with_two_sided_offset_curve_smbh(),
            "two-sided offset",
        ),
        (
            synthetic_geometry_with_vector_offset_curve_smbh(),
            "vector offset",
        ),
        (synthetic_geometry_with_subset_curve_smbh(), "subset"),
        (synthetic_geometry_with_helix_curve_smbh(), "helix"),
    ];
    for (smbh, family) in cases {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{family} decode: {error}"));
        assert_eq!(
            decoded.ir.model.procedural_curves.len(),
            1,
            "{family} fixture must decode one procedural curve"
        );
        let mut source_less = decoded.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less.model.procedural_curves[0].cache_fit_tolerance = None;
        let mut encoded = Vec::new();
        F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &source_less,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut encoded))
            .unwrap_or_else(|error| panic!("{family} source-less encode: {error}"));
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{family} round trip: {error}"));
        assert_eq!(
            round_trip.ir.model.procedural_curves.len(),
            1,
            "{family} procedural curve was not reconstructed"
        );
        assert_eq!(
            round_trip.ir.model.procedural_curves[0].cache_fit_tolerance, None,
            "{family} invented a cache-fit tolerance"
        );
    }
}

#[test]
fn cache_first_deformable_refuses_a_missing_fit_tolerance() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_deformable_curve_smbh(8),
            )),
            &DecodeOptions::default(),
        )
        .expect("deformable decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    source_less.model.procedural_curves[0].cache_fit_tolerance = None;
    let mut encoded = Vec::new();
    let error = F3dCodec.encode(&source_less, &mut encoded).unwrap_err();
    assert!(error
        .to_string()
        .contains("cache-first intcurve requires a native cache-fit tolerance"));
    assert!(encoded.is_empty());
}

#[test]
fn generated_compound_loft_decodes_scale_and_zero_tail() {
    use cadmpeg_ir::geometry::{
        CompoundLoftDirection, CompoundLoftTail, ProceduralSurfaceDefinition,
    };

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_compound_loft_smbh())),
            &DecodeOptions::default(),
        )
        .expect("compound-loft decode");
    let ProceduralSurfaceDefinition::CompoundLoft { construction } =
        &result.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected compound loft")
    };
    let scale = construction.scales[0].as_ref().expect("first scale");
    assert!(construction.scales[1..].iter().all(Option::is_none));
    assert_eq!(scale.members.len(), 1);
    assert!(scale.members[0].data.pcurve.is_some());
    assert_eq!(scale.auxiliaries.len(), 1);
    assert_eq!(scale.tail, [2, 3]);
    assert_eq!(construction.flags, [true, false]);
    let CompoundLoftTail::Zero {
        flags,
        selector,
        direction,
        trailing_flags,
    } = &construction.tail
    else {
        panic!("expected zero tail")
    };
    assert_eq!(*flags, [false, true]);
    assert_eq!(*selector, 0);
    assert!(matches!(direction, CompoundLoftDirection::Vector { .. }));
    assert_eq!(*trailing_flags, [true, false]);
    let member_curve = scale.members[0].curve.clone();

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut missing_tolerance = source_less.clone();
    missing_tolerance.model.procedural_surfaces[0].cache_fit_tolerance = None;
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &missing_tolerance,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("compound loft without its required tolerance must be rejected");
    assert!(
        error
            .to_string()
            .contains("compound-loft surface requires a native cache-fit tolerance"),
        "unexpected error: {error}"
    );
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == member_curve)
        .expect("compound-loft member curve")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(-1.0, 2.0, 3.0),
        direction: cadmpeg_ir::math::Vector3::new(4.0, -3.0, 2.0),
    };
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less compound-loft encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less compound-loft round trip");
    let ProceduralSurfaceDefinition::CompoundLoft { construction } =
        &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected round-trip compound loft")
    };
    assert!(construction.scales[0].is_some());
    assert!(construction.scales[1..].iter().all(Option::is_none));
    assert_eq!(construction.flags, [true, false]);
    assert!(matches!(
        construction.tail,
        CompoundLoftTail::Zero {
            selector: 0,
            direction: CompoundLoftDirection::Vector { .. },
            ..
        }
    ));
    let member_curve = &construction.scales[0]
        .as_ref()
        .expect("round-trip scale")
        .members[0]
        .curve;
    assert!(matches!(
        round_trip
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *member_curve)
            .map(|curve| &curve.geometry),
        Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
            if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
    ));
}

#[test]
fn generated_compound_loft_writes_every_tail_shape_source_less() {
    use cadmpeg_ir::geometry::{
        CompoundLoftDirection, CompoundLoftTail, ProceduralSurfaceDefinition,
    };
    use cadmpeg_ir::math::Vector3;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_compound_loft_smbh())),
            &DecodeOptions::default(),
        )
        .expect("compound-loft decode");
    let ProceduralSurfaceDefinition::CompoundLoft { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected compound loft")
    };
    let scale = construction.scales[0].clone().expect("generated scale");
    let curve = scale.path.clone();
    let line_curve = cadmpeg_ir::ids::CurveId("generated:compound_loft_tail_line#0".into());
    let tails = [
        CompoundLoftTail::Six {
            flags: [true, false],
            scale: Box::new(scale.clone()),
            selector: 31,
            direction: Vector3::new(0.0, 1.0, 0.0),
            parameter_range: [-0.5, 1.5],
            curve: line_curve.clone(),
        },
        CompoundLoftTail::Seven {
            first_flag: true,
            first_scale: Some(Box::new(scale.clone())),
            second_flag: false,
            second_scale: Box::new(scale.clone()),
            selector: -7,
            direction: Vector3::new(1.0, 0.0, 0.0),
            trailing_flags: [false, true],
        },
        CompoundLoftTail::Zero {
            flags: [false, true],
            selector: 4,
            direction: CompoundLoftDirection::Curve { curve },
            trailing_flags: [true, true],
        },
    ];

    for (tail_index, expected) in tails.into_iter().enumerate() {
        let mut source_less = decoded.ir.clone();
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less.model.curves.push(cadmpeg_ir::geometry::Curve {
            id: line_curve.clone(),
            geometry: cadmpeg_ir::geometry::CurveGeometry::Line {
                origin: cadmpeg_ir::math::Point3::new(-1.0, 2.0, 3.0),
                direction: cadmpeg_ir::math::Vector3::new(4.0, -2.0, 1.0),
            },
            source_object: None,
        });
        let ProceduralSurfaceDefinition::CompoundLoft { construction } =
            &mut source_less.model.procedural_surfaces[0].definition
        else {
            unreachable!()
        };
        construction.tail = expected.clone();
        let mut encoded = Vec::new();
        F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &source_less,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less compound-loft encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less compound-loft round trip");
        assert_eq!(
            round_trip.ir.model.procedural_surfaces.len(),
            1,
            "tail {tail_index} did not decode"
        );
        let ProceduralSurfaceDefinition::CompoundLoft { construction } =
            &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected round-trip compound loft")
        };
        match (&expected, &construction.tail) {
            (
                CompoundLoftTail::Six { .. },
                CompoundLoftTail::Six {
                    parameter_range,
                    curve,
                    ..
                },
            ) => {
                assert!(matches!(
                    round_trip
                        .ir
                        .model
                        .curves
                        .iter()
                        .find(|candidate| candidate.id == *curve)
                        .map(|curve| &curve.geometry),
                    Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                        if curve.degree == 1
                            && curve.knots
                                == [
                                    parameter_range[0],
                                    parameter_range[0],
                                    parameter_range[1],
                                    parameter_range[1],
                                ]
                ));
            }
            (CompoundLoftTail::Seven { .. }, CompoundLoftTail::Seven { first_scale, .. }) => {
                assert!(first_scale.is_some());
            }
            (
                CompoundLoftTail::Zero { .. },
                CompoundLoftTail::Zero {
                    selector: 4,
                    direction: CompoundLoftDirection::Curve { .. },
                    ..
                },
            ) => {}
            _ => panic!("compound-loft tail shape changed"),
        }
    }
}

#[test]
fn generated_scaled_compound_loft_decodes_full_direct_branch() {
    use cadmpeg_ir::geometry::{
        CompoundLoftDirection, ProceduralSurfaceDefinition, ScaledCompoundLoftBranch,
        ScaledCompoundLoftShape,
    };

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_scaled_compound_loft_smbh(true))),
            &DecodeOptions::default(),
        )
        .expect("scaled compound-loft decode");
    let ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected scaled compound loft")
    };
    assert!(matches!(construction.shape, ScaledCompoundLoftShape::Full));
    assert_eq!(construction.singularity, 11);
    assert_eq!(construction.discontinuities[0], [0.25]);
    assert!(construction.discontinuities[1..].iter().all(Vec::is_empty));
    assert!(construction.discontinuity_flag);
    assert!(construction.scales[0].is_some());
    assert!(construction.scales[1..].iter().all(Option::is_none));
    assert_eq!(construction.flags, [true, false]);
    assert_eq!(construction.selector, 0);
    assert!(matches!(
        construction.branch,
        ScaledCompoundLoftBranch::Direct {
            flag: true,
            selector: 0,
            direction: CompoundLoftDirection::Vector { .. },
        }
    ));
    assert_eq!(construction.trailing_flags, [false, true]);
    assert_eq!(construction.tail_kind, 2);
    assert_eq!(construction.tail_singularity, 12);

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut missing_tolerance = source_less.clone();
    missing_tolerance.model.procedural_surfaces[0].cache_fit_tolerance = None;
    assert!(F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &missing_tolerance,
            fidelity: None
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("full scaled compound loft without tolerance must fail")
        .to_string()
        .contains("full shape requires a native cache-fit tolerance"));
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less scaled compound-loft encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less scaled compound-loft round trip");
    assert!(matches!(
        round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::ScaledCompoundLoft { .. }
    ));
}

#[test]
fn generated_scaled_compound_loft_writes_all_middle_branches_source_less() {
    use cadmpeg_ir::geometry::{
        CompoundLoftDirection, ProceduralSurfaceDefinition, ScaledCompoundLoftBranch,
        ScaledCompoundLoftShape,
    };
    use cadmpeg_ir::math::Vector3;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_scaled_compound_loft_smbh(true))),
            &DecodeOptions::default(),
        )
        .expect("scaled compound-loft decode");
    let ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected scaled compound loft")
    };
    let scale = construction.scales[0].clone().expect("generated scale");
    let curve = scale.path.clone();
    let cases = [
        (
            ScaledCompoundLoftShape::Full,
            ScaledCompoundLoftBranch::ExtendedVector {
                first_scale: None,
                second_scale: Box::new(scale.clone()),
                selector: 9,
                direction: Vector3::new(1.0, 0.0, 0.0),
            },
        ),
        (
            ScaledCompoundLoftShape::Full,
            ScaledCompoundLoftBranch::ExtendedCurve {
                scale: None,
                flag: true,
                singularity: 13,
                curve: curve.clone(),
            },
        ),
        (
            ScaledCompoundLoftShape::Full,
            ScaledCompoundLoftBranch::Direct {
                flag: false,
                selector: 4,
                direction: CompoundLoftDirection::Curve {
                    curve: curve.clone(),
                },
            },
        ),
    ];

    for (case_index, (shape, branch)) in cases.into_iter().enumerate() {
        let mut source_less = decoded.ir.clone();
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } =
            &mut source_less.model.procedural_surfaces[0].definition
        else {
            unreachable!()
        };
        construction.shape = shape;
        construction.branch = branch;
        let mut encoded = Vec::new();
        F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &source_less,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less scaled compound-loft encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less scaled compound-loft round trip");
        assert_eq!(
            round_trip.ir.model.procedural_surfaces.len(),
            1,
            "scaled compound-loft case {case_index} did not decode"
        );
        let ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } =
            &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected round-trip scaled compound loft")
        };
        assert!(matches!(
            (&construction.shape, &construction.branch),
            (
                ScaledCompoundLoftShape::Full,
                ScaledCompoundLoftBranch::ExtendedVector { .. }
                    | ScaledCompoundLoftBranch::ExtendedCurve { .. }
                    | ScaledCompoundLoftBranch::Direct {
                        direction: CompoundLoftDirection::Curve { .. },
                        ..
                    }
            )
        ));
    }
}

#[test]
fn generated_scaled_compound_loft_none_shape_round_trips_as_procedural_face() {
    use cadmpeg_ir::geometry::{
        ProceduralSurfaceDefinition, ScaledCompoundLoftShape, SurfaceGeometry,
    };

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_scaled_compound_loft_smbh(false))),
            &DecodeOptions::default(),
        )
        .expect("scaled compound-loft none-shape decode");
    let ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected scaled compound loft")
    };
    assert!(matches!(
        construction.shape,
        ScaledCompoundLoftShape::None {
            parameter_ranges: [[-1.0, 2.0], [-3.0, 4.0]],
            ..
        }
    ));
    let owner = decoded.ir.model.procedural_surfaces[0].surface.clone();
    assert!(matches!(
        decoded
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == owner)
            .expect("procedural owner")
            .geometry,
        SurfaceGeometry::Procedural { ref construction }
            if *construction == decoded.ir.model.procedural_surfaces[0].id
    ));
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut unexpected_tolerance = source_less.clone();
    unexpected_tolerance.model.procedural_surfaces[0].cache_fit_tolerance = Some(0.04);
    assert!(F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &unexpected_tolerance,
            fidelity: None
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("none-shape scaled compound loft with tolerance must fail")
        .to_string()
        .contains("none shape cannot carry a cache-fit tolerance"));
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less scaled compound-loft none-shape encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less scaled compound-loft none-shape round trip");
    assert!(matches!(
        round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::ScaledCompoundLoft { .. }
    ));
}

#[test]
fn generated_skin_surface_decodes_recursive_spline_law() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition, SkinSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_skin_spl_sur_smbh(0, false))),
            &DecodeOptions::default(),
        )
        .expect("skin surface decode");
    let ProceduralSurfaceDefinition::Skin { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected skin surface")
    };
    assert_eq!(construction.surface_boolean, 1);
    assert_eq!(construction.surface_normal, 2);
    assert_eq!(construction.surface_direction, 3);
    assert_eq!(construction.count, 4);
    assert_eq!(construction.parameter, 0.25);
    assert!(matches!(
        construction.layout,
        SkinSurfaceLayout::Compact { .. }
    ));
    assert_eq!(construction.direction.z, 1.0);
    assert_eq!(construction.trailing_parameter, 0.75);
    assert_eq!(construction.formula.name, "skin-law");
    assert!(matches!(
        construction.formula.variables.as_slice(),
        [LawExpression::Spline {
            native_id: 5,
            knots,
            controls,
            ..
        }] if knots == &[0.0, 0.5, 1.0] && controls == &[1.0, 2.0, 3.0]
    ));
    assert_eq!(construction.discontinuities[0], [0.1]);
    assert_eq!(construction.discontinuities[1], [0.2, 0.3]);
    assert!(construction.discontinuity_flag);

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
        .expect("source-less skin surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less skin surface round trip");
    let ProceduralSurfaceDefinition::Skin { construction } =
        &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected round-trip skin surface")
    };
    assert!(matches!(
        construction.formula.variables.as_slice(),
        [LawExpression::Spline { native_id: 5, .. }]
    ));
}

#[test]
fn generated_law_surfaces_decode_and_round_trip_modern_and_legacy_layouts() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition};

    for (name, legacy_ranges) in [("law_spl_sur", false), ("lawsur", true)] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_law_spl_sur_smbh(
                    name,
                    legacy_ranges,
                    0,
                ))),
                &DecodeOptions::default(),
            )
            .expect("law surface decode");
        let ProceduralSurfaceDefinition::Law { construction } =
            &decoded.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected law surface")
        };
        assert_eq!(
            construction.parameter_ranges,
            legacy_ranges.then_some([[-1.0, 2.0], [-3.0, 4.0]])
        );
        assert_eq!(construction.primary.name, "primary-law");
        assert!(matches!(
            construction.primary.variables.as_slice(),
            [LawExpression::Algebraic { operator, operands }]
                if operator == "SET" && operands.len() == 1
        ));
        assert_eq!(construction.additional.len(), 1);
        assert_eq!(construction.additional[0].name, "aux-law");
        assert!(matches!(
            construction.additional[0].variables.as_slice(),
            [LawExpression::Algebraic { operator, operands }]
                if operator == "TERM" && operands.len() == 2
        ));
        assert_eq!(construction.discontinuities[0], [0.1]);
        assert_eq!(construction.discontinuities[1], [0.2, 0.3]);
        assert_eq!(
            decoded.ir.model.procedural_surfaces[0].cache_fit_tolerance,
            Some(0.07)
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
            .unwrap();
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap();
        let ProceduralSurfaceDefinition::Law { construction } =
            &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected round-trip law surface")
        };
        assert_eq!(
            construction.parameter_ranges,
            legacy_ranges.then_some([[-1.0, 2.0], [-3.0, 4.0]])
        );
        assert_eq!(construction.additional.len(), 1);
    }
}

#[test]
fn generated_sub_surfaces_decode_and_write_exact_support_graphs() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};

    for name in ["sub_spl_sur", "subsur"] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_sub_spl_sur_smbh(name))),
                &DecodeOptions::default(),
            )
            .unwrap();
        let procedural = &decoded.ir.model.procedural_surfaces[0];
        let ProceduralSurfaceDefinition::SubSurface {
            support,
            parameter_ranges,
        } = &procedural.definition
        else {
            panic!("expected sub-surface")
        };
        assert_eq!(*parameter_ranges, [[-1.0, 2.0], [-3.0, 4.0]]);
        assert!(matches!(
            decoded
                .ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == *support)
                .map(|surface| &surface.geometry),
            Some(SurfaceGeometry::Plane { origin, .. })
                if *origin == cadmpeg_ir::math::Point3::new(1.0, -2.0, 3.0)
        ));
        assert!(matches!(
            decoded
                .ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == procedural.surface)
                .map(|surface| &surface.geometry),
            Some(SurfaceGeometry::Procedural { .. })
        ));

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
            ProceduralSurfaceDefinition::SubSurface {
                parameter_ranges: [[-1.0, 2.0], [-3.0, 4.0]],
                ..
            }
        ));
    }
}

#[test]
fn generated_law_surfaces_round_trip_every_standard_tail_mode() {
    use cadmpeg_ir::geometry::{LawSurfaceTail, ProceduralSurfaceDefinition};

    for selector in 1..=4 {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_law_spl_sur_smbh(
                    "law_spl_sur",
                    false,
                    selector,
                ))),
                &DecodeOptions::default(),
            )
            .unwrap();
        let ProceduralSurfaceDefinition::Law { construction } =
            &decoded.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected law surface")
        };
        assert!(match (&construction.tail, selector) {
            (
                LawSurfaceTail::Summary {
                    parameters,
                    fit_tolerance,
                    closures: [0, 2],
                    singularities: [1, 3],
                },
                1,
            ) => parameters[0] == [0.0, 0.5, 1.0] && *fit_tolerance == 0.08,
            (
                LawSurfaceTail::None {
                    parameter_ranges: [[-0.5, 1.5], [-2.0, 2.0]],
                    closures: [1, 2],
                    singularities: [0, 4],
                },
                2,
            ) => true,
            (LawSurfaceTail::Historical, 3) | (LawSurfaceTail::Optimal, 4) => true,
            _ => false,
        });
        assert_eq!(
            decoded.ir.model.procedural_surfaces[0].cache_fit_tolerance,
            None
        );
        assert!(matches!(
            decoded
                .ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == decoded.ir.model.procedural_surfaces[0].surface)
                .map(|surface| &surface.geometry),
            Some(cadmpeg_ir::geometry::SurfaceGeometry::Procedural { .. })
        ));
        let expected_tail = construction.tail.clone();

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
        let ProceduralSurfaceDefinition::Law { construction } =
            &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected round-trip law surface")
        };
        assert_eq!(construction.tail, expected_tail);
    }
}

#[test]
fn generated_skin_surface_round_trips_structural_law_nodes() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_skin_spl_sur_smbh(1, false))),
            &DecodeOptions::default(),
        )
        .expect("skin structural-law decode");
    let ProceduralSurfaceDefinition::Skin { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected skin surface")
    };
    assert!(matches!(
        construction.formula.variables.as_slice(),
        [
            LawExpression::Null,
            LawExpression::Transform {
                enums: [4, 5, 6],
                ..
            },
            LawExpression::Edge {
                parameters: [-0.25, 1.25],
                ..
            }
        ]
    ));
    let LawExpression::Edge { curve, .. } = &construction.formula.variables[2] else {
        unreachable!()
    };
    let law_edge = curve.clone();

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == law_edge)
        .expect("law edge curve")
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
        .expect("source-less structural-law encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less structural-law round trip");
    let ProceduralSurfaceDefinition::Skin { construction } =
        &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected round-trip skin surface")
    };
    assert_eq!(construction.formula.variables.len(), 3);
    let LawExpression::Edge { curve, .. } = &construction.formula.variables[2] else {
        panic!("expected round-trip edge law")
    };
    assert!(matches!(
        round_trip
            .ir
            .model
            .curves
            .iter()
            .find(|candidate| candidate.id == *curve)
            .map(|curve| &curve.geometry),
        Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
            if curve.degree == 1
                && curve.knots == [-0.25, -0.25, 1.25, 1.25]
    ));
}

#[test]
fn generated_skin_surface_round_trips_expanded_profiles() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SkinSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_skin_spl_sur_smbh(0, true))),
            &DecodeOptions::default(),
        )
        .expect("expanded skin decode");
    let ProceduralSurfaceDefinition::Skin { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected skin surface")
    };
    let SkinSurfaceLayout::Profiles { profiles, tail, .. } = &construction.layout else {
        panic!("expected expanded skin profiles")
    };
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].type_code, 9);
    assert_eq!(profiles[0].data.asm_extension, Some(-1));
    assert!(profiles[0].data.pcurve.is_some());
    assert!(profiles[0].data.direction.is_some());
    assert_eq!(*tail, [-1, 7]);
    let profile_curve = profiles[0].curve.clone();

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == profile_curve)
        .expect("skin profile curve")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(2.0, -1.0, 3.0),
        direction: cadmpeg_ir::math::Vector3::new(4.0, 2.0, -3.0),
    };
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less expanded skin encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less expanded skin round trip");
    let ProceduralSurfaceDefinition::Skin { construction } =
        &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected round-trip skin surface")
    };
    assert!(matches!(
        &construction.layout,
        SkinSurfaceLayout::Profiles { profiles, .. }
            if profiles.len() == 1 && profiles[0].data.direction.is_some()
    ));
    let SkinSurfaceLayout::Profiles { profiles, .. } = &construction.layout else {
        unreachable!()
    };
    assert!(matches!(
        round_trip
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == profiles[0].curve)
            .map(|curve| &curve.geometry),
        Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
            if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
    ));
}

#[test]
fn generated_skin_surface_round_trips_fixed_arity_algebraic_laws() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_skin_spl_sur_smbh(2, false))),
            &DecodeOptions::default(),
        )
        .expect("algebraic skin law decode");
    let ProceduralSurfaceDefinition::Skin { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected skin surface")
    };
    assert!(matches!(
        construction.formula.variables.as_slice(),
        [
            LawExpression::Algebraic {
                operator,
                operands,
            },
            LawExpression::Algebraic {
                operator: dot,
                operands: vectors,
            }
        ] if operator == "SIN"
            && matches!(operands.as_slice(), [LawExpression::Algebraic { operator, operands }]
                if operator == "ABS"
                    && matches!(operands.as_slice(), [LawExpression::Double { value }] if *value == -2.5))
            && dot == "DOT"
            && matches!(vectors.as_slice(), [LawExpression::Vector { .. }, LawExpression::Vector { .. }])
    ));

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
        .expect("source-less algebraic skin encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less algebraic skin round trip");
    let ProceduralSurfaceDefinition::Skin { construction } =
        &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected round-trip skin surface")
    };
    assert_eq!(construction.formula.variables.len(), 2);
}

#[test]
fn source_less_writer_rejects_invalid_and_unframed_law_arities() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_skin_spl_sur_smbh(2, false))),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let ProceduralSurfaceDefinition::Skin { construction } =
        &mut source_less.model.procedural_surfaces[0].definition
    else {
        panic!()
    };
    construction.formula.variables[0] = LawExpression::Algebraic {
        operator: "SIN".into(),
        operands: Vec::new(),
    };
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .unwrap_err();
    assert!(error.to_string().contains("requires 1 operands, got 0"));

    let ProceduralSurfaceDefinition::Skin { construction } =
        &mut source_less.model.procedural_surfaces[0].definition
    else {
        panic!()
    };
    construction.formula.variables[0] = LawExpression::Algebraic {
        operator: "MIN".into(),
        operands: vec![LawExpression::Double { value: 1.0 }],
    };
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .unwrap_err();
    assert!(error.to_string().contains("unresolved variable arity"));
}

#[test]
fn generated_skin_surface_round_trips_set_rotate_and_term_laws() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition};
    use cadmpeg_ir::math::Vector3;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_skin_spl_sur_smbh(2, false))),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let ProceduralSurfaceDefinition::Skin { construction } =
        &mut source_less.model.procedural_surfaces[0].definition
    else {
        panic!()
    };
    construction.formula.variables = vec![
        LawExpression::Algebraic {
            operator: "SET".into(),
            operands: vec![LawExpression::Double { value: -2.0 }],
        },
        LawExpression::Algebraic {
            operator: "ROTATE".into(),
            operands: vec![
                LawExpression::Vector {
                    value: Vector3::new(1.0, 2.0, 3.0),
                },
                LawExpression::Transform {
                    scalars: [0.0; 13],
                    enums: [0, 0, 0],
                },
            ],
        },
        LawExpression::Algebraic {
            operator: "TERM".into(),
            operands: vec![
                LawExpression::Vector {
                    value: Vector3::new(4.0, 5.0, 6.0),
                },
                LawExpression::Integer { value: 1 },
            ],
        },
    ];

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
    let ProceduralSurfaceDefinition::Skin { construction } =
        &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!()
    };
    assert!(matches!(
        construction.formula.variables.as_slice(),
        [
            LawExpression::Algebraic { operator: set, operands: set_operands },
            LawExpression::Algebraic { operator: rotate, operands: rotate_operands },
            LawExpression::Algebraic { operator: term, operands: term_operands },
        ] if set == "SET" && set_operands.len() == 1
            && rotate == "ROTATE" && rotate_operands.len() == 2
            && term == "TERM" && term_operands.len() == 2
    ));
}

#[test]
fn generated_g2_blend_surfaces_decode_both_singularity_branches() {
    use cadmpeg_ir::geometry::{G2BlendFirstShape, LoftBridgeToken, ProceduralSurfaceDefinition};

    for name in ["g2_blend_spl_sur", "g2blnsur"] {
        for full in [true, false] {
            let result = F3dCodec
                .decode(
                    &mut Cursor::new(f3d_with_smbh(&synthetic_g2_blend_spl_sur_smbh(name, full))),
                    &DecodeOptions::default(),
                )
                .expect("G2 blend decode");
            let ProceduralSurfaceDefinition::G2Blend { construction } =
                &result.ir.model.procedural_surfaces[0].definition
            else {
                panic!("expected G2 blend")
            };
            assert_eq!(construction.first.label, "first");
            assert_eq!(construction.second.label, "second");
            assert_eq!(construction.singularity, if full { 11 } else { 12 });
            assert_eq!(construction.center_parameters, [-0.5, 1.5]);
            assert_eq!(construction.parameter_ranges, [[-1.0, 2.0], [-3.0, 4.0]]);
            assert_eq!(construction.trailing_parameters, [0.1, 0.2, 0.3, 0.4]);
            assert_eq!(
                construction.discontinuities,
                [vec![0.25], vec![], vec![0.5, 0.75]]
            );
            match &construction.first_shape {
                G2BlendFirstShape::Full { surface, tolerance } if full => {
                    assert!(surface.is_some());
                    assert_eq!(*tolerance, Some(0.02));
                }
                G2BlendFirstShape::None {
                    coefficients,
                    tolerance,
                    extension,
                    pcurve,
                } if !full => {
                    assert_eq!(*coefficients, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
                    assert_eq!(*tolerance, 0.03);
                    assert_eq!(*extension, Some(LoftBridgeToken::Integer(44)));
                    assert!(pcurve.is_some());
                }
                _ => panic!("wrong G2 singularity payload"),
            }
            let side_curves = [
                construction.first.curve.clone(),
                construction.second.curve.clone(),
            ];
            let center_curve = construction.center_curve.clone();

            let mut source_less = result.ir;
            source_less.source = None;
            source_less.set_native_unknowns("f3d", &[]).unwrap();
            for (ordinal, side) in side_curves.into_iter().enumerate() {
                source_less
                    .model
                    .curves
                    .iter_mut()
                    .find(|curve| curve.id == side)
                    .expect("G2 side curve")
                    .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
                    origin: cadmpeg_ir::math::Point3::new(ordinal as f64, 2.0, -1.0),
                    direction: cadmpeg_ir::math::Vector3::new(3.0, -2.0, 4.0),
                };
            }
            source_less
                .model
                .curves
                .iter_mut()
                .find(|curve| curve.id == center_curve)
                .expect("G2 center curve")
                .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
                origin: cadmpeg_ir::math::Point3::new(-2.0, 1.0, 3.0),
                direction: cadmpeg_ir::math::Vector3::new(4.0, -3.0, 2.0),
            };
            let mut encoded = Vec::new();
            F3dCodec
                .plan(cadmpeg_ir::codec::EncodeInput {
                    ir: &source_less,
                    fidelity: None,
                })
                .and_then(|plan| plan.write_to(&mut encoded))
                .expect("source-less G2 encode");
            let round_trip = F3dCodec
                .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
                .expect("source-less G2 round trip");
            let ProceduralSurfaceDefinition::G2Blend { construction } =
                &round_trip.ir.model.procedural_surfaces[0].definition
            else {
                panic!("expected round-trip G2 blend")
            };
            assert_eq!(construction.singularity, if full { 11 } else { 12 });
            assert_eq!(construction.center_parameters, [-0.5, 1.5]);
            assert_eq!(construction.parameter_ranges, [[-1.0, 2.0], [-3.0, 4.0]]);
            assert_eq!(construction.discontinuities[2], [0.5, 0.75]);
            assert_eq!(
                matches!(construction.first_shape, G2BlendFirstShape::Full { .. }),
                full
            );
            for side in [&construction.first, &construction.second] {
                assert!(matches!(
                    round_trip
                        .ir
                        .model
                        .curves
                        .iter()
                        .find(|curve| curve.id == side.curve)
                        .map(|curve| &curve.geometry),
                    Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                        if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
                ));
            }
            assert!(matches!(
                round_trip
                    .ir
                    .model
                    .curves
                    .iter()
                    .find(|curve| curve.id == construction.center_curve)
                    .map(|curve| &curve.geometry),
                Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                    if curve.degree == 1
                        && curve.knots == [-0.5, -0.5, 1.5, 1.5]
            ));
        }
    }
}

#[test]
fn generated_rolling_ball_and_sss_blends_decode_full_native_graphs() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, RollingBallRadiusSelector};

    for name in [
        "rb_blend_spl_sur",
        "rbblnsur",
        "pipe_spl_sur",
        "pipesur",
        "sss_blend_spl_sur",
        "sssblndsur",
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_full_rolling_ball_smbh(name))),
                &DecodeOptions::default(),
            )
            .expect("rolling-ball decode");
        let ProceduralSurfaceDefinition::Blend {
            native: Some(native),
            ..
        } = &result.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected complete rolling-ball graph")
        };
        assert_eq!(native.definition_index, 22507);
        assert_eq!(
            native.sides[0].support_kind,
            cadmpeg_ir::geometry::VariableBlendSupportKind::Surface
        );
        assert_eq!(
            native.sides[1].support_kind,
            cadmpeg_ir::geometry::VariableBlendSupportKind::Curve
        );
        assert_eq!(
            native.sides[0].location,
            cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0)
        );
        assert!(native.sides.iter().all(|side| side.surface.is_some()));
        assert!(native.sides.iter().all(|side| side.pcurve.is_some()));
        assert_eq!(native.sides[0].extension, Some(3));
        assert_eq!(native.sides[1].extension, Some(4));
        assert_eq!(native.offsets, [-3.0, -6.0]);
        assert_eq!(native.radius_selector, RollingBallRadiusSelector::None);
        assert_eq!(native.u_range, [Some(-1.0), Some(2.0)]);
        assert_eq!(native.v_range, [None, None]);
        assert_eq!(native.shape_prefix, 1);
        assert_eq!(native.parameters, [0.1, 0.2]);
        assert_eq!(native.tail, 17);
        assert_eq!(native.tail_enum, 0);
        assert_eq!(native.tail_parameterization, None);
        assert_eq!(
            native.discontinuities,
            expected_revision_surface_tail_discontinuities()
        );
        assert!(!native.tail_flag);
        assert_eq!(native.tail_extensions, [11, 12, 13]);
        assert_eq!(native.third.is_some(), name.starts_with("sss"));
        if let Some(third) = &native.third {
            assert_eq!(third.label, "third");
            assert_eq!(third.extension, 23);
            assert!(third.secondary_pcurve.is_some());
            assert!(!third.flag);
        }

        let expected = native.clone();
        let side_curves = native
            .sides
            .iter()
            .map(|side| side.curve.clone())
            .collect::<Vec<_>>();
        let third_curve = native.third.as_ref().map(|third| third.curve.clone());
        let slice_curve = native.slice.clone();
        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        for (ordinal, side) in side_curves.iter().enumerate() {
            source_less
                .model
                .curves
                .iter_mut()
                .find(|curve| Some(&curve.id) == side.as_ref())
                .expect("rolling-ball side curve")
                .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
                origin: cadmpeg_ir::math::Point3::new(ordinal as f64, 3.0, -2.0),
                direction: cadmpeg_ir::math::Vector3::new(4.0, -1.0, 2.0),
            };
        }
        if let Some(third) = &third_curve {
            source_less
                .model
                .curves
                .iter_mut()
                .find(|curve| curve.id == *third)
                .expect("rolling-ball third-side curve")
                .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
                origin: cadmpeg_ir::math::Point3::new(-1.0, 2.0, 3.0),
                direction: cadmpeg_ir::math::Vector3::new(3.0, 4.0, -2.0),
            };
        }
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == slice_curve)
            .expect("rolling-ball slice curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(2.0, -3.0, 1.0),
            direction: cadmpeg_ir::math::Vector3::new(4.0, 2.0, -1.0),
        };
        let mut encoded = Vec::new();
        F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &source_less,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less rolling-ball encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less rolling-ball round trip");
        let ProceduralSurfaceDefinition::Blend {
            native: Some(actual),
            ..
        } = &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected complete round-trip rolling-ball graph")
        };
        assert_eq!(actual.as_ref(), expected.as_ref());
        for side in actual.sides.iter() {
            assert!(matches!(
                round_trip
                    .ir
                    .model
                    .curves
                    .iter()
                    .find(|curve| Some(&curve.id) == side.curve.as_ref())
                    .map(|curve| &curve.geometry),
                Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                    if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
            ));
        }
        if let Some(third) = &actual.third {
            assert!(matches!(
                round_trip
                    .ir
                    .model
                    .curves
                    .iter()
                    .find(|curve| curve.id == third.curve)
                    .map(|curve| &curve.geometry),
                Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                    if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
            ));
        }
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == actual.slice)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1 && curve.knots == [-1.0, -1.0, 2.0, 2.0]
        ));
    }
}

/// Tail form `2` stores no cache and no fit tolerance: it stores the U
/// parameter interval, the V parameter interval, and the U closure, V closure,
/// U singularity, and V singularity enums. The carrier fields that follow the
/// tail decode at their stored values only when the tail is framed exactly.
#[test]
fn parameterized_tail_form_decodes_in_every_blend_carrier() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_variable_blend_smbh_with_tail_form("var_blend_spl_sur", 2),
            )),
            &DecodeOptions::default(),
        )
        .expect("parameterized variable-blend decode");
    let procedural = &decoded.ir.model.procedural_surfaces[0];
    assert_eq!(procedural.cache_fit_tolerance, None);
    let ProceduralSurfaceDefinition::VariableBlend { construction } = &procedural.definition else {
        panic!("expected variable-blend construction")
    };
    assert_eq!(construction.tail_enum, 2);
    assert_eq!(
        construction.tail_parameterization,
        Some(expected_revision_surface_tail_parameterization())
    );
    // Fields after the tail; a misframed tail shifts every one of them.
    assert_eq!(construction.tail_extensions, [31, 32, 33]);
    assert_eq!(construction.post_range, [Some(0.0), Some(1.0)]);

    for name in ["rb_blend_spl_sur", "sss_blend_spl_sur"] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_full_rolling_ball_with_tail_smbh(
                    name, 2,
                ))),
                &DecodeOptions::default(),
            )
            .expect("parameterized rolling-ball decode");
        let procedural = &decoded.ir.model.procedural_surfaces[0];
        assert_eq!(procedural.cache_fit_tolerance, None);
        let ProceduralSurfaceDefinition::Blend {
            native: Some(native),
            ..
        } = &procedural.definition
        else {
            panic!("expected complete rolling-ball graph")
        };
        assert_eq!(native.tail_enum, 2);
        assert_eq!(
            native.tail_parameterization,
            Some(expected_revision_surface_tail_parameterization())
        );
        assert_eq!(
            native.discontinuities,
            expected_revision_surface_tail_discontinuities()
        );
        assert!(!native.tail_flag);
        assert_eq!(native.third.is_some(), name.starts_with("sss"));
        // Fields after the tail; a misframed tail shifts every one of them.
        assert_eq!(native.tail_extensions, [11, 12, 13]);
    }

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_versioned_cyl_spl_sur_with_tail_smbh(2),
            )),
            &DecodeOptions::default(),
        )
        .expect("parameterized extrusion decode");
    let procedural = &decoded.ir.model.procedural_surfaces[0];
    // Form `2` stores no cache and no fit tolerance. The surface block inside
    // the directrix scope is not this record's cache and its trailing scalar is
    // not this record's fit tolerance.
    assert_eq!(procedural.cache_fit_tolerance, None);
    let ProceduralSurfaceDefinition::Extrusion {
        parameter_interval: Some([0.25, 0.75]),
        revision_form: Some(form),
        ..
    } = &procedural.definition
    else {
        panic!("expected a parameterized revision-gated extrusion")
    };
    assert_eq!(form.tail_enum, 2);
    assert_eq!(
        form.tail_parameterization,
        Some(expected_revision_surface_tail_parameterization())
    );
    assert_eq!(
        form.discontinuities,
        expected_revision_surface_tail_discontinuities()
    );
    assert!(!form.tail_flag);
}

/// Tail form `2` stores no solved cache, so a blend record carrying it owns
/// every surface block it holds as a construction support and rests on its
/// procedural carrier. Source-less generation writes the construction back from
/// the stored parameterization rather than needing a cache it never had.
#[test]
fn parameterized_blend_tails_round_trip_source_less_generation() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};

    for stream in [
        synthetic_full_rolling_ball_with_tail_smbh("rb_blend_spl_sur", 2),
        synthetic_variable_blend_smbh_with_tail_form("var_blend_spl_sur", 2),
    ] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&stream)),
                &DecodeOptions::default(),
            )
            .expect("parameterized blend decode");
        let mut source_less = decoded.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let expected = source_less.model.procedural_surfaces[0].clone();
        assert_eq!(expected.cache_fit_tolerance, None);
        let carrier = source_less
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == expected.surface)
            .expect("blend surface carrier");
        assert!(matches!(
            carrier.geometry,
            SurfaceGeometry::Procedural { .. }
        ));

        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("parameterized blend encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("parameterized blend round trip");
        let actual = &round_trip.ir.model.procedural_surfaces[0];
        assert_eq!(actual.cache_fit_tolerance, None);
        let (enumeration, parameterization) = match &actual.definition {
            ProceduralSurfaceDefinition::Blend {
                native: Some(native),
                ..
            } => (native.tail_enum, native.tail_parameterization.clone()),
            ProceduralSurfaceDefinition::VariableBlend { construction } => (
                construction.tail_enum,
                construction.tail_parameterization.clone(),
            ),
            other => panic!("expected a parameterized blend construction: {other:?}"),
        };
        assert_eq!(enumeration, 2);
        assert_eq!(
            parameterization,
            Some(expected_revision_surface_tail_parameterization())
        );
    }
}

#[test]
fn variable_blend_second_interval_decodes_unbounded_upper_bound() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    // The tail's second interval carries a lower bound with an
    // unbounded-above marker: `(T lo, F)` decodes to `[Some(lo), None]`.
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh_with_selector(
                "srf_srf_v_bl_spl_sur",
                false,
                None,
                [Some(-0.5), None],
            ))),
            &DecodeOptions::default(),
        )
        .expect("half-bounded second-interval decode");
    let ProceduralSurfaceDefinition::VariableBlend { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected variable blend")
    };
    assert_eq!(construction.u_range, [Some(-1.0), Some(2.0)]);
    assert_eq!(construction.v_range, [Some(-0.5), None]);
    assert_eq!(construction.shape_prefix, 11);
    assert_eq!(construction.shape_length, 6.0);
}

#[test]
fn generated_interp_radius_law_leaves_the_cross_section_enum_unconsumed() {
    use cadmpeg_ir::geometry::{
        ProceduralSurfaceDefinition, VariableBlendCrossSection, VariableBlendValuePayload,
    };

    // An `interp` payload ends at its last radius point. The enum that follows
    // is the record's cross-section selector; reading it as a trailing flag
    // costs the cross-section while leaving the byte count intact, so the
    // decoded cross-section is what pins the boundary.
    for (selector, expected) in [
        (Some(0), Some(VariableBlendCrossSection::Circular)),
        (
            Some(7),
            Some(VariableBlendCrossSection::G2Round {
                parameters: [2.0, 2.0],
            }),
        ),
        (None, None),
    ] {
        let smbh =
            synthetic_variable_blend_smbh_with_interp_radius("srf_srf_v_bl_spl_sur", selector);
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("interp variable-blend decode");
        let ProceduralSurfaceDefinition::VariableBlend { construction } =
            &result.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected variable blend")
        };
        let VariableBlendValuePayload::Interpolated { points, .. } =
            &construction.first_value.payload
        else {
            panic!("expected interpolated radius law")
        };
        assert_eq!(points.len(), 1);
        assert_eq!(construction.cross_section, expected);

        assert_revision_surface_round_trip(smbh, "variable_blend");
    }
}

#[test]
fn generated_edge_offset_radius_law_reads_two_parameters_and_one_offset() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, VariableBlendValuePayload};

    // `edge_offset` without the leading sub-discriminator stores its law-domain
    // parameter range and one offset: the second field is a parameter, and only
    // the third is a length, so only the third takes the centimetre-to-
    // millimetre conversion.
    let smbh = synthetic_variable_blend_smbh_with_edge_offset_radius("srf_srf_v_bl_spl_sur");
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("edge-offset variable-blend decode");
    let ProceduralSurfaceDefinition::VariableBlend { construction } =
        &result.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected variable blend")
    };
    let VariableBlendValuePayload::EdgeOffset { scalars, lengths } =
        &construction.first_value.payload
    else {
        panic!("expected edge-offset radius law")
    };
    assert_eq!(scalars, &[0.25, 0.75]);
    assert_eq!(lengths, &[15.0]);

    assert_revision_surface_round_trip(smbh, "variable_blend");
}

#[test]
fn generated_variable_blends_decode_complete_single_radius_graphs() {
    use cadmpeg_ir::geometry::{
        ProceduralSurfaceDefinition, VariableBlendSurfaceSubtype, VariableBlendValuePayload,
    };

    for (name, subtype) in [
        (
            "var_blend_spl_sur",
            VariableBlendSurfaceSubtype::VariableBlend,
        ),
        ("varblendsplsur", VariableBlendSurfaceSubtype::VariableBlend),
        (
            "srf_srf_v_bl_spl_sur",
            VariableBlendSurfaceSubtype::SurfaceSurface,
        ),
        ("srfsrfblndsur", VariableBlendSurfaceSubtype::SurfaceSurface),
        (
            "crv_crv_v_bl_spl_sur",
            VariableBlendSurfaceSubtype::CurveCurve,
        ),
        ("crvcrvblndsur", VariableBlendSurfaceSubtype::CurveCurve),
        (
            "crv_srf_v_bl_spl_sur",
            VariableBlendSurfaceSubtype::CurveSurface,
        ),
        ("crvsrfblndsur", VariableBlendSurfaceSubtype::CurveSurface),
        (
            "sfcv_free_bl_spl_sur",
            VariableBlendSurfaceSubtype::SurfaceCurveFree,
        ),
        (
            "sfcvfreeblndsur",
            VariableBlendSurfaceSubtype::SurfaceCurveFree,
        ),
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh(name))),
                &DecodeOptions::default(),
            )
            .expect("variable-blend decode");
        let ProceduralSurfaceDefinition::VariableBlend { construction } =
            &result.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected variable blend")
        };
        assert_eq!(construction.subtype, subtype);
        assert_eq!(construction.revision, 23100);
        assert_eq!(
            construction.sides[0].support_kind,
            cadmpeg_ir::geometry::VariableBlendSupportKind::Surface
        );
        assert_eq!(
            construction.sides[1].support_kind,
            cadmpeg_ir::geometry::VariableBlendSupportKind::Curve
        );
        assert_eq!(construction.sides[0].extension, Some(0));
        assert_eq!(construction.sides[1].extension, Some(5));
        assert_eq!(
            construction.sides[0].location,
            cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0)
        );
        assert_eq!(construction.offsets, [-2.0, 4.0]);
        assert_eq!(
            construction.radius_kind,
            cadmpeg_ir::geometry::VariableBlendRadiusKind::SingleRadius
        );
        let VariableBlendValuePayload::TwoEnds { parameters, radii } =
            &construction.first_value.payload
        else {
            panic!("expected two-ends radius law")
        };
        assert!(construction.first_value.modern_flag);
        assert_eq!(construction.first_value.discriminator, 7);
        assert_eq!(construction.first_value.calibrated, 3);
        assert_eq!(*parameters, [0.25, 0.75]);
        assert_eq!(*radii, [15.0, 25.0]);
        assert_eq!(construction.slice_range, [None, None]);
        assert_eq!(construction.u_range, [Some(-1.0), Some(2.0)]);
        assert_eq!(construction.v_range, [None, None]);
        assert_eq!(construction.shape_prefix, 11);
        assert_eq!(construction.shape_length, 6.0);
        assert_eq!(construction.tail_enum, 0);
        assert_eq!(
            construction.discontinuities,
            [
                vec![0.125],
                vec![],
                vec![0.25, 0.375],
                vec![],
                vec![0.5],
                vec![]
            ]
        );
        assert!(construction.tail_flag);
        assert_eq!(construction.tail_extensions, [31, 32, 33]);
        assert!(construction.secondary_curve.is_some());
        assert_eq!(construction.secondary_range, [None, None]);
        assert_eq!(
            construction.convexity,
            cadmpeg_ir::geometry::VariableBlendConvexity::Convex
        );
        assert_eq!(
            construction.render_mode,
            cadmpeg_ir::geometry::VariableBlendRenderMode::RollingBallSnapshot
        );
        assert_eq!(construction.post_range, [Some(0.0), Some(1.0)]);
        assert!(construction.post_curve.is_some());
        assert!(construction.post_pcurve.is_none());
        assert!(construction.sides.iter().all(|side| side.pcurve.is_some()));

        let expected = construction.clone();
        let post_curve = construction.post_curve.clone().expect("post curve");
        let slice_curve = construction.slice.clone();
        let side_curves = construction
            .sides
            .iter()
            .map(|side| side.curve.clone().expect("side curve"))
            .collect::<Vec<_>>();
        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == post_curve)
            .expect("variable-blend post curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(-2.0, 1.0, 3.0),
            direction: cadmpeg_ir::math::Vector3::new(3.0, -4.0, 2.0),
        };
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == slice_curve)
            .expect("variable-blend slice curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(3.0, -2.0, 1.0),
            direction: cadmpeg_ir::math::Vector3::new(4.0, 2.0, -3.0),
        };
        for (ordinal, side) in side_curves.iter().enumerate() {
            source_less
                .model
                .curves
                .iter_mut()
                .find(|curve| curve.id == *side)
                .expect("variable-blend side curve")
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
            .expect("source-less variable-blend encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less variable-blend round trip");
        let ProceduralSurfaceDefinition::VariableBlend {
            construction: actual,
        } = &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected round-trip variable blend")
        };
        assert_eq!(actual.as_ref(), expected.as_ref());
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| Some(&curve.id) == actual.post_curve.as_ref())
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
        ));
        for side in actual.sides.iter() {
            assert!(matches!(
                round_trip
                    .ir
                    .model
                    .curves
                    .iter()
                    .find(|curve| Some(&curve.id) == side.curve.as_ref())
                    .map(|curve| &curve.geometry),
                Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                    if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
            ));
        }
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == actual.slice)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1 && curve.knots == [-1.0, -1.0, 2.0, 2.0]
        ));
    }
}

#[test]
fn generated_variable_blend_rejects_radius_cardinality_mismatch() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let mut decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh(
                "var_blend_spl_sur",
            ))),
            &DecodeOptions::default(),
        )
        .expect("variable-blend decode")
        .ir;
    decoded.source = None;
    decoded.set_native_unknowns("f3d", &[]).unwrap();
    let ProceduralSurfaceDefinition::VariableBlend { construction } =
        &mut decoded.model.procedural_surfaces[0].definition
    else {
        panic!("expected variable blend")
    };
    construction.second_value = Some(construction.first_value.clone());

    assert!(cadmpeg_ir::validate_neutral(&decoded, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "variable blend construction payload is invalid"));
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &decoded,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("single-radius variable blend carries two-radii payloads"));
}

#[test]
fn generated_two_radii_variable_blend_round_trips_rounded_chamfer() {
    use cadmpeg_ir::geometry::{
        ProceduralSurfaceDefinition, VariableBlendRadiusKind, VariableBlendValuePayload,
    };

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh_with_branch(
                "var_blend_spl_sur",
                true,
            ))),
            &DecodeOptions::default(),
        )
        .expect("two-radii variable-blend decode");
    let ProceduralSurfaceDefinition::VariableBlend { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected variable blend")
    };
    assert_eq!(construction.radius_kind, VariableBlendRadiusKind::TwoRadii);
    assert!(matches!(
        construction
            .second_value
            .as_ref()
            .map(|value| &value.payload),
        Some(VariableBlendValuePayload::TwoEnds {
            parameters: [0.1, 0.9],
            radii: [35.0, 45.0]
        })
    ));
    let Some(cadmpeg_ir::geometry::VariableBlendCrossSection::RoundedChamfer {
        radius: Some(radius),
    }) = &construction.cross_section
    else {
        panic!("expected rounded-chamfer cross section with a radius law")
    };
    assert!(matches!(
        &radius.payload,
        VariableBlendValuePayload::TwoEnds {
            parameters: [0.0, 1.0],
            radii: [55.0, 65.0]
        }
    ));

    let expected = construction.clone();
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
        .expect("two-radii variable-blend source-less encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("two-radii variable-blend round trip");
    assert!(matches!(
        &round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::VariableBlend { construction }
            if construction == &expected
    ));
}

#[test]
fn generated_two_radii_variable_blend_decodes_explicit_circular_cross_section() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, VariableBlendRadiusKind};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh_with_selector(
                "srf_srf_v_bl_spl_sur",
                true,
                Some(0),
                [None, None],
            ))),
            &DecodeOptions::default(),
        )
        .expect("two-radii selector-zero decode");
    let ProceduralSurfaceDefinition::VariableBlend { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected variable blend")
    };
    assert_eq!(construction.radius_kind, VariableBlendRadiusKind::TwoRadii);
    assert!(matches!(
        &construction.cross_section,
        Some(cadmpeg_ir::geometry::VariableBlendCrossSection::Circular)
    ));
    let expected = construction.clone();
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
        .expect("selector-zero source-less encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("selector-zero round trip");
    assert!(matches!(
        &round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::VariableBlend { construction }
            if construction == &expected
    ));
}

pub(super) fn push_optional_value_quartet(surface: &mut Vec<u8>) {
    for value in [1.0, 0.0, 1.0, 0.0] {
        surface.push(0x0a);
        t_dbl(surface, value);
    }
}

#[test]
fn generated_revision_exact_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("exact_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_optional_value_quartet(surface);
        push_tagged_i64(surface, 0x15, 0);
    });
    assert_revision_surface_round_trip(smbh, "exact");
}

/// The blend constructions' tail enum was serialized as `cache_selector`. A
/// document written under that name deserializes into the same construction.
#[test]
fn blend_tail_enum_deserializes_under_its_former_name() {
    for smbh in [
        synthetic_full_rolling_ball_smbh("rb_blend_spl_sur"),
        synthetic_variable_blend_smbh("var_blend_spl_sur"),
    ] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("blend decode");
        let json = serde_json::to_string(&decoded.ir).expect("IR JSON");
        assert_eq!(json.matches("\"tail_enum\"").count(), 1);
        let renamed = json.replace("\"tail_enum\"", "\"cache_selector\"");
        let restored: cadmpeg_ir::document::CadIr =
            serde_json::from_str(&renamed).expect("IR under the former field name");
        assert_eq!(restored, decoded.ir);
    }
}

/// Tail form `0` stores a solved cache and its fit tolerance together, so a
/// construction that reaches the writer with the cache and without the
/// tolerance is inconsistent. The writer names the carrier and refuses instead
/// of substituting a tolerance of its own, which the tail cannot be
/// distinguished from a stored zero.
#[test]
fn revision_gated_solved_tails_refuse_a_missing_fit_tolerance() {
    for (smbh, carrier) in [
        (
            synthetic_revision_surface_smbh("exact_spl_sur", |surface| {
                push_revision_surface_tail(surface);
                push_optional_value_quartet(surface);
                push_tagged_i64(surface, 0x15, 0);
            }),
            "exact spline surface",
        ),
        (synthetic_versioned_cyl_spl_sur_smbh(), "extrusion surface"),
    ] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{carrier} decode: {error}"));
        assert!(decoded.ir.model.procedural_surfaces[0]
            .cache_fit_tolerance
            .is_some());
        let mut source_less = decoded.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less.model.procedural_surfaces[0].cache_fit_tolerance = None;
        let mut encoded = Vec::new();
        let error = F3dCodec.encode(&source_less, &mut encoded).unwrap_err();
        assert!(
            error
                .to_string()
                .contains(&format!("{carrier} requires a native cache-fit tolerance")),
            "unexpected {carrier} error: {error}"
        );
        assert!(encoded.is_empty());
    }
}

#[test]
fn generated_revision_exact_surface_carries_two_unextended_intervals() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SplineSurfaceParameters};

    // Two distinct non-[0,1] unextended parameter intervals: U then V.
    let smbh = synthetic_revision_surface_smbh("exact_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        for value in [0.0, std::f64::consts::FRAC_PI_2, 0.5, 2.0] {
            surface.push(0x0a);
            t_dbl(surface, value);
        }
        push_tagged_i64(surface, 0x15, 0);
    });
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("revision exact decode");
    let procedural = result.ir.model.procedural_surfaces.first().unwrap();
    let ProceduralSurfaceDefinition::Exact { parameters, .. } = &procedural.definition else {
        panic!("expected exact definition");
    };
    assert_eq!(
        parameters,
        &SplineSurfaceParameters::RevisionRanges {
            intervals: [
                [Some(0.0), Some(std::f64::consts::FRAC_PI_2)],
                [Some(0.5), Some(2.0)],
            ],
        }
    );
    assert_revision_surface_round_trip(smbh, "exact");
}

#[test]
fn generated_revision_loft_surface_carries_one_nonempty_wrap_interval() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SplineSurfaceParameters};

    // First wrap interval non-empty [0,1]; second reversed [1,0] = empty.
    let smbh = synthetic_revision_surface_smbh("loft_spl_sur", |surface| {
        t_long(surface, 1);
        t_dbl(surface, 0.0);
        t_long(surface, 1);
        t_long(surface, 1);
        surface.extend_from_slice(&generated_curve_block());
        surface.extend_from_slice(&[0x0b, 0x0b]);
        t_ident(surface, "null_surface");
        t_ident(surface, "nullbs");
        surface.push(0x0b);
        t_long(surface, -1);
        t_long(surface, 213);
        t_long(surface, 1);
        t_long(surface, 1);
        for value in [0.0, 1.0, 0.25, 0.75, 0.5, 1.5] {
            t_dbl(surface, value);
        }
        surface.push(0x0b);
        t_ident(surface, "null_curve");
        t_long(surface, 0);
        t_long(surface, -1);
        t_long(surface, 0);
        for value in [0.0, 1.0, 1.0, 0.0] {
            surface.push(0x0a);
            t_dbl(surface, value);
        }
        surface.extend_from_slice(&[0x0b; 4]);
        t_long(surface, 0);
        t_long(surface, 0);
        push_revision_surface_tail(surface);
    });
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("revision loft decode");
    let procedural = result.ir.model.procedural_surfaces.first().unwrap();
    let ProceduralSurfaceDefinition::Loft { parameters, .. } = &procedural.definition else {
        panic!("expected loft definition");
    };
    assert_eq!(
        parameters,
        &SplineSurfaceParameters::RevisionRanges {
            intervals: [[Some(0.0), Some(1.0)], [Some(1.0), Some(0.0)]],
        }
    );
    assert_revision_surface_round_trip(smbh, "loft");
}

#[test]
fn generated_revision_sum_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("sum_spl_sur", |surface| {
        for (lower, upper) in [(0.0, 1.0), (-2.0, 2.0)] {
            surface.extend_from_slice(&generated_curve_block());
            surface.push(0x0a);
            t_dbl(surface, lower);
            surface.push(0x0a);
            t_dbl(surface, upper);
        }
        t_pos(surface, [1.0, 2.0, 3.0]);
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh, "sum");
}

#[test]
fn generated_revision_rot_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("rot_spl_sur", |surface| {
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, 0.0);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        t_pos(surface, [0.0, 0.0, 0.0]);
        t_vec(surface, [0.0, 0.0, 1.0]);
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh, "revolution");
}

#[test]
fn generated_revision_t_spline_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("t_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_optional_value_quartet(surface);
        push_tagged_i64(surface, 0x15, 0);
        surface.push(0x0f);
        t_ident(surface, "t_spl_subtrans_object");
        t_u16_string(
            surface,
            "degree 3\nunits mm\nv 1 0 0 0\nv 2 1 0 0\ne 1 1 2\n",
        );
        surface.push(0x0b);
        t_u16_string(surface, "100verts 1 2\n");
        surface.push(0x10);
        t_long(surface, 2);
    });
    assert_revision_surface_round_trip(smbh, "t_spline");
}

#[test]
fn generated_revision_g2_blend_round_trips() {
    let smbh = synthetic_revision_surface_smbh("g2_blend_spl_sur", |surface| {
        t_dbl(surface, 1.0);
        t_dbl(surface, 1.0);
        append_generated_variable_blend_side(surface, "left", 1.0);
        append_generated_variable_blend_side(surface, "right", 4.0);
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, -1.5);
        surface.push(0x0a);
        t_dbl(surface, 2.5);
        t_dbl(surface, 0.125);
        t_dbl(surface, 0.125);
        push_tagged_i64(surface, 0x15, -1);
        surface.extend_from_slice(&[0x0b; 4]);
        t_long(surface, 1);
        t_dbl(surface, 0.001);
        t_dbl(surface, 0.0001);
        t_long(surface, 1);
        push_revision_surface_tail(surface);
        for value in [0, 0, 0] {
            t_long(surface, value);
        }
    });
    assert_revision_surface_round_trip(smbh, "revision_g2_blend");
}

#[test]
fn generated_parameterized_revision_g2_blend_round_trips() {
    let smbh = synthetic_revision_surface_smbh("g2_blend_spl_sur", |surface| {
        t_dbl(surface, 1.0);
        t_dbl(surface, 1.0);
        append_generated_variable_blend_side(surface, "left", 1.0);
        append_generated_variable_blend_side(surface, "right", 4.0);
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, -1.5);
        surface.push(0x0a);
        t_dbl(surface, 2.5);
        t_dbl(surface, 0.125);
        t_dbl(surface, 0.125);
        push_tagged_i64(surface, 0x15, -1);
        surface.extend_from_slice(&[0x0b; 4]);
        t_long(surface, 1);
        t_dbl(surface, 0.001);
        t_dbl(surface, 0.0001);
        t_long(surface, 1);
        push_parameterized_revision_surface_tail(surface);
        for value in [0, 0, 0] {
            t_long(surface, value);
        }
    });
    assert_revision_surface_round_trip(smbh.clone(), "revision_g2_blend");

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("parameterized revision g2 blend decode");
    let procedural = &result.ir.model.procedural_surfaces[0];
    assert_eq!(procedural.cache_fit_tolerance, None);
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RevisionG2Blend { construction } =
        &procedural.definition
    else {
        panic!("expected a revision g2 blend construction")
    };
    assert_parameterized_tail(
        construction.tail_enum,
        construction.tail_parameterization.as_ref(),
    );
}

#[test]
fn generated_revision_vertex_blend_round_trips() {
    let smbh = synthetic_revision_surface_smbh("VBL_SURF", |surface| {
        t_long(surface, 2);

        t_ident(surface, "circle");
        surface.push(0x0a);
        t_vec(surface, [0.0, 0.0, 0.0]);
        surface.push(0x0b);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, 0.1);
        surface.push(0x0a);
        t_dbl(surface, 0.9);
        push_tagged_i64(surface, 0x15, 3);
        t_vec(surface, [0.0, 0.0, 0.5]);
        t_vec(surface, [0.5, 0.0, 0.0]);
        t_dbl(surface, 0.1);
        t_dbl(surface, 0.9);
        surface.push(0x0b);

        t_ident(surface, "pcurve");
        surface.push(0x0b);
        t_vec(surface, [0.0, 0.0, 0.0]);
        surface.push(0x0a);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        t_ident(surface, "plane");
        t_pos(surface, [0.0, 0.0, 0.0]);
        t_vec(surface, [0.0, 0.0, 1.0]);
        t_vec(surface, [1.0, 0.0, 0.0]);
        surface.push(0x0b);
        surface.extend_from_slice(&[0x0b; 4]);
        surface.extend_from_slice(&generated_pcurve_block());
        surface.push(0x0a);
        t_dbl(surface, 0.002);

        t_long(surface, 9);
        t_dbl(surface, 0.003);
    });
    assert_revision_surface_round_trip(smbh, "vertex_blend");
}

#[test]
fn generated_revision_offset_with_inline_untyped_support_decodes() {
    let smbh = synthetic_revision_surface_smbh("off_spl_sur", |surface| {
        t_ident(surface, "spline");
        surface.push(0x0b);
        surface.push(0x0f);
        t_ident(surface, "mystery_spl_sur");
        t_long(surface, 23100);
        surface.extend_from_slice(&generated_surface_block());
        surface.push(0x10);
        surface.extend_from_slice(&[0x0b; 4]);
        t_dbl(surface, 0.3);
        surface.extend_from_slice(&[0x0b; 4]);
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh, "offset");
}

#[test]
fn generated_single_radius_variable_blend_decodes_explicit_circular_cross_section() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh_with_selector(
                "srf_srf_v_bl_spl_sur",
                false,
                Some(0),
                [None, None],
            ))),
            &DecodeOptions::default(),
        )
        .expect("single-radius selector-zero decode");
    let ProceduralSurfaceDefinition::VariableBlend { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected variable blend")
    };
    assert!(matches!(
        &construction.cross_section,
        Some(cadmpeg_ir::geometry::VariableBlendCrossSection::Circular)
    ));
    let expected = construction.clone();
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
        .expect("selector-zero source-less encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("selector-zero round trip");
    assert!(matches!(
        &round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::VariableBlend { construction }
            if construction == &expected
    ));
}

#[test]
fn generated_variable_blend_round_trips_parameterized_cross_sections() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, VariableBlendCrossSection};

    for (selector, expected_cross_section) in [
        (
            1,
            VariableBlendCrossSection::Thumbweights {
                parameters: [2.0, 2.0],
            },
        ),
        (
            7,
            VariableBlendCrossSection::G2Round {
                parameters: [2.0, 2.0],
            },
        ),
    ] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh_with_selector(
                    "srf_srf_v_bl_spl_sur",
                    false,
                    Some(selector),
                    [None, None],
                ))),
                &DecodeOptions::default(),
            )
            .expect("parameterized cross-section decode");
        let ProceduralSurfaceDefinition::VariableBlend { construction } =
            &decoded.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected variable blend")
        };
        assert_eq!(
            construction.cross_section.as_ref(),
            Some(&expected_cross_section)
        );

        let expected = construction.clone();
        let mut source_less = decoded.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("parameterized cross-section source-less encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("parameterized cross-section round trip");
        assert!(matches!(
            &round_trip.ir.model.procedural_surfaces[0].definition,
            ProceduralSurfaceDefinition::VariableBlend { construction }
                if construction == &expected
        ));
    }
}

#[test]
fn generated_variable_blend_round_trips_unclassified_bare_cross_sections() {
    use cadmpeg_ir::geometry::{
        ProceduralSurfaceDefinition, VariableBlendBareCrossSection, VariableBlendCrossSection,
    };

    for (selector, expected) in [
        (2, VariableBlendBareCrossSection::Selector2),
        (4, VariableBlendBareCrossSection::Selector4),
        (5, VariableBlendBareCrossSection::Selector5),
        (6, VariableBlendBareCrossSection::Selector6),
    ] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh_with_selector(
                    "srf_srf_v_bl_spl_sur",
                    false,
                    Some(selector),
                    [None, None],
                ))),
                &DecodeOptions::default(),
            )
            .expect("bare cross-section decode");
        let ProceduralSurfaceDefinition::VariableBlend { construction } =
            &decoded.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected variable blend")
        };
        assert_eq!(
            construction.cross_section,
            Some(VariableBlendCrossSection::UnclassifiedBare { selector: expected })
        );

        let expected_construction = construction.clone();
        let mut source_less = decoded.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("bare cross-section source-less encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("bare cross-section round trip");
        assert!(matches!(
            &round_trip.ir.model.procedural_surfaces[0].definition,
            ProceduralSurfaceDefinition::VariableBlend { construction }
                if construction == &expected_construction
        ));
    }
}

pub(super) fn push_revision_cl_scale(surface: &mut Vec<u8>, with_path: bool) {
    // One member: type, curve, endpoints, support, pcurve, flags, subdata.
    t_long(surface, 1);
    t_long(surface, 1);
    surface.extend_from_slice(&generated_curve_block());
    surface.push(0x0a);
    t_dbl(surface, 0.0);
    surface.push(0x0a);
    t_dbl(surface, 1.0);
    t_ident(surface, "null_surface");
    t_ident(surface, "nullbs");
    surface.push(0x0b);
    t_long(surface, -1);
    // Subdata type 213 with one row and one column: leading pair plus
    // `column_count + 1` trailing pairs in the revision encoding.
    t_long(surface, 213);
    t_long(surface, 1);
    t_long(surface, 1);
    for value in [0.0, 1.0, -0.5, 0.25, 0.75, 0.75] {
        t_dbl(surface, value);
    }
    surface.push(0x0b);
    if with_path {
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, 0.0);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
    } else {
        t_ident(surface, "null_curve");
    }
    t_long(surface, 0);
    t_long(surface, -1);
}

#[test]
fn generated_revision_compound_loft_round_trips() {
    let smbh = synthetic_revision_surface_smbh("cl_loft_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_revision_cl_scale(surface, true);
        t_long(surface, 2);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 0.0);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        t_vec(surface, [0.0, 0.0, 1.0]);
        surface.push(0x0b);
        surface.push(0x0b);
    });
    assert_revision_surface_round_trip(smbh, "revision_compound_loft");
}

#[test]
fn generated_parameterized_revision_compound_loft_round_trips() {
    let smbh = synthetic_revision_surface_smbh("cl_loft_spl_sur", |surface| {
        push_parameterized_revision_surface_tail(surface);
        push_revision_cl_scale(surface, true);
        t_long(surface, 2);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 0.0);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        t_vec(surface, [0.0, 0.0, 1.0]);
        surface.push(0x0b);
        surface.push(0x0b);
    });
    assert_revision_surface_round_trip(smbh.clone(), "revision_compound_loft");

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("parameterized revision compound loft decode");
    let procedural = &result.ir.model.procedural_surfaces[0];
    assert_eq!(procedural.cache_fit_tolerance, None);
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RevisionCompoundLoft { construction } =
        &procedural.definition
    else {
        panic!("expected a revision compound loft construction")
    };
    assert_parameterized_tail(
        construction.tail_enum,
        construction.tail_parameterization.as_ref(),
    );
}

#[test]
fn generated_revision_compound_loft_trailing_curve_round_trips() {
    let smbh = synthetic_revision_surface_smbh("cl_loft_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_revision_cl_scale(surface, false);
        t_long(surface, 1);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        t_vec(surface, [0.0, 0.0, 1.0]);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        surface.push(0x0a);
        t_dbl(surface, 0.0);
        surface.extend_from_slice(&generated_curve_block());
    });
    assert_revision_surface_round_trip(smbh, "revision_compound_loft");
}

#[test]
fn generated_revision_compound_loft_rejects_present_parameters_without_a_curve() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    // The trailing curve is present exactly when both parameter values are
    // present, so a payload carrying two present values and closing straight
    // away is not a legal record. The decoder reads the curve on the parameter
    // pair alone; it does not look ahead for the subtype-close byte.
    let smbh = synthetic_revision_surface_smbh("cl_loft_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_revision_cl_scale(surface, false);
        t_long(surface, 1);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        t_vec(surface, [0.0, 0.0, 1.0]);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        surface.push(0x0a);
        t_dbl(surface, 0.0);
    });
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("decode retains the record as a native unknown");
    assert!(!decoded
        .ir
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            surface.definition,
            ProceduralSurfaceDefinition::RevisionCompoundLoft { .. }
        )));

    let legal = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_revision_surface_smbh(
                "cl_loft_spl_sur",
                |surface| {
                    push_revision_surface_tail(surface);
                    push_revision_cl_scale(surface, false);
                    t_long(surface, 1);
                    push_revision_cl_scale(surface, false);
                    t_dbl(surface, 1.0);
                    surface.push(0x0b);
                    surface.push(0x0b);
                    t_long(surface, 0);
                    surface.push(0x0b);
                    surface.push(0x0b);
                    t_long(surface, 0);
                    t_vec(surface, [0.0, 0.0, 1.0]);
                    surface.push(0x0a);
                    t_dbl(surface, 1.0);
                    surface.push(0x0a);
                    t_dbl(surface, 0.0);
                    surface.extend_from_slice(&generated_curve_block());
                },
            ))),
            &DecodeOptions::default(),
        )
        .expect("legal revision compound loft decode")
        .ir;
    let mut edited = legal.clone();
    edited.source = None;
    edited.set_native_unknowns("f3d", &[]).unwrap();
    let ProceduralSurfaceDefinition::RevisionCompoundLoft { construction } =
        &mut edited.model.procedural_surfaces[0].definition
    else {
        panic!("expected revision compound loft")
    };
    construction.trailing_curve = None;
    let error = F3dCodec.encode(&edited, &mut Vec::new()).unwrap_err();
    assert!(error
        .to_string()
        .contains("pairs its trailing curve with both parameter values"));
}

#[test]
fn decode_carries_the_document_modeling_length_unit_into_source_metadata() {
    // The `Custom` system's `modelingLengthName` is the document's display
    // length unit. It reaches `SourceMeta`, not `CadIr::units`: no stored
    // quantity depends on it, and model-space coordinates stay centimetres
    // under every value.
    let design = crate::design::decode::units::tests::stream([
        "centimeter",
        "millimeter",
        "meter",
        "inch",
        "foot",
        "inch",
    ]);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(&synthetic_geometry_smbh()).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&design).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let result = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("decode with a unit-systems design stream");
    assert_eq!(
        result
            .ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("modeling_length_unit"))
            .map(String::as_str),
        Some("inch")
    );
    // The IR stays millimetre-canonical regardless of the display unit.
    assert_eq!(result.ir.units, cadmpeg_ir::units::Units::default());
}

#[test]
fn record_level_surface_bounds_round_trip() {
    let smbh = synthetic_revision_surface_smbh("exact_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_optional_value_quartet(surface);
        push_tagged_i64(surface, 0x15, 0);
    });
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("exact revision decode");
    let mut source_less = decoded.ir;
    assert_eq!(source_less.model.procedural_surfaces[0].record_bounds, None);
    source_less.model.procedural_surfaces[0].record_bounds =
        Some([Some(0.1), None, Some(0.2), None]);
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("record-bounds encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("record-bounds round trip");
    assert_eq!(
        round_trip.ir.model.procedural_surfaces[0].record_bounds,
        Some([Some(0.1), None, Some(0.2), None])
    );
}

#[test]
fn generated_vertex_blends_decode_all_boundary_variants() {
    use cadmpeg_ir::geometry::{
        ProceduralSurfaceDefinition, SurfaceGeometry, VertexBlendBoundaryGeometry,
    };

    for name in ["VBL_SURF", "vertexblendsur"] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_vertex_blend_smbh(name))),
                &DecodeOptions::default(),
            )
            .expect("vertex-blend decode");
        let ProceduralSurfaceDefinition::VertexBlend { construction } =
            &result.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected vertex blend")
        };
        let owner = result
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == result.ir.model.procedural_surfaces[0].surface)
            .expect("vertex-blend owner");
        assert!(
            matches!(
                owner.geometry,
                SurfaceGeometry::Procedural { ref construction }
                    if *construction == result.ir.model.procedural_surfaces[0].id
            ),
            "unexpected vertex-blend carrier: {:?}",
            owner.geometry
        );
        assert_eq!(construction.boundaries.len(), 4);
        assert_eq!(construction.grid_size, 17);
        assert_eq!(construction.fit_tolerance, 0.03);
        let VertexBlendBoundaryGeometry::Circle {
            form,
            twists,
            parameters,
            sense,
            ..
        } = &construction.boundaries[0].geometry
        else {
            panic!("expected circle boundary")
        };
        assert_eq!(*form, 1);
        assert_eq!(twists, &[cadmpeg_ir::math::Point3::new(20.0, 30.0, 40.0)]);
        assert_eq!(*parameters, [0.1, 0.9]);
        assert!(!*sense);
        assert!(matches!(
            construction.boundaries[1].geometry,
            VertexBlendBoundaryGeometry::Degenerate { .. }
        ));
        assert!(matches!(
            construction.boundaries[2].geometry,
            VertexBlendBoundaryGeometry::Pcurve {
                pcurve: Some(_),
                ..
            }
        ));
        assert!(matches!(
            construction.boundaries[3].geometry,
            VertexBlendBoundaryGeometry::Plane { .. }
        ));
        let bounded_curves =
            [0usize, 3].map(|ordinal| match &construction.boundaries[ordinal].geometry {
                VertexBlendBoundaryGeometry::Circle {
                    curve, parameters, ..
                }
                | VertexBlendBoundaryGeometry::Plane {
                    curve, parameters, ..
                } => (curve.clone(), *parameters),
                _ => unreachable!(),
            });

        let expected = construction.clone();
        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        for (ordinal, (curve, _)) in bounded_curves.iter().enumerate() {
            source_less
                .model
                .curves
                .iter_mut()
                .find(|candidate| candidate.id == *curve)
                .expect("vertex-blend boundary curve")
                .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
                origin: cadmpeg_ir::math::Point3::new(ordinal as f64, 2.0, -3.0),
                direction: cadmpeg_ir::math::Vector3::new(2.0, -1.0, 4.0),
            };
        }
        let mut encoded = Vec::new();
        F3dCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &source_less,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less vertex-blend encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less vertex-blend round trip");
        let ProceduralSurfaceDefinition::VertexBlend {
            construction: actual,
        } = &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected round-trip vertex blend")
        };
        assert_eq!(actual.as_ref(), expected.as_ref());
        for (curve, range) in bounded_curves {
            assert!(matches!(
                round_trip
                    .ir
                    .model
                    .curves
                    .iter()
                    .find(|candidate| candidate.id == curve)
                    .map(|curve| &curve.geometry),
                Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                    if curve.degree == 1
                        && curve.knots == [range[0], range[0], range[1], range[1]]
            ));
        }
    }
}

#[test]
fn decode_retains_generated_translational_extrusion_and_fit_contract() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let f3d = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    let procedural = result.ir.model.procedural_surfaces.first().unwrap();
    assert_eq!(procedural.cache_fit_tolerance, Some(0.02));
    let ProceduralSurfaceDefinition::Extrusion {
        direction,
        directrix,
        parameter_interval,
        native_position,
        revision_form: None,
    } = &procedural.definition
    else {
        panic!("expected extrusion")
    };
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 20.0));
    assert_eq!(*parameter_interval, Some([0.25, 0.75]));
    assert_eq!(
        *native_position,
        Some(cadmpeg_ir::math::Point3::new(40.0, 50.0, 60.0))
    );
    let directrix = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *directrix)
        .expect("extrusion directrix carrier");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(directrix) = &directrix.geometry else {
        panic!("expected NURBS directrix")
    };
    assert_eq!(directrix.control_points.len(), 3);
}

#[test]
fn decode_retains_versioned_nested_translational_extrusion() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_versioned_cyl_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("versioned extrusion decode");
    let procedural = result.ir.model.procedural_surfaces.first().unwrap();
    assert_eq!(procedural.cache_fit_tolerance, Some(0.02));
    let ProceduralSurfaceDefinition::Extrusion {
        direction,
        parameter_interval,
        native_position,
        ..
    } = &procedural.definition
    else {
        panic!("expected versioned extrusion")
    };
    assert_eq!(*parameter_interval, Some([0.25, 0.75]));
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 20.0));
    assert_eq!(
        *native_position,
        Some(cadmpeg_ir::math::Point3::new(40.0, 50.0, 60.0))
    );
}

#[test]
fn generated_f3d_rewrites_translational_extrusion_header() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let source = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated extrusion decode");
    let mut edited = decoded.ir;
    let ProceduralSurfaceDefinition::Extrusion {
        parameter_interval,
        direction,
        native_position,
        ..
    } = &mut edited.model.procedural_surfaces[0].definition
    else {
        panic!("expected extrusion")
    };
    *parameter_interval = Some([-0.5, 1.25]);
    *direction = cadmpeg_ir::math::Vector3::new(5.0, -10.0, 30.0);
    *native_position = Some(cadmpeg_ir::math::Point3::new(-20.0, 70.0, 15.0));

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("extrusion-direction regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated extrusion decode");
    let ProceduralSurfaceDefinition::Extrusion {
        parameter_interval,
        direction,
        native_position,
        ..
    } = &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected round-trip extrusion")
    };
    assert_eq!(*parameter_interval, Some([-0.5, 1.25]));
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(5.0, -10.0, 30.0));
    assert_eq!(
        *native_position,
        Some(cadmpeg_ir::math::Point3::new(-20.0, 70.0, 15.0))
    );
}

#[test]
fn generated_f3d_rewrites_procedural_surface_fit_tolerance() {
    let source = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated procedural-surface decode");
    let mut edited = decoded.ir;
    edited.model.procedural_surfaces[0].cache_fit_tolerance = Some(0.075);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("procedural-surface fit regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated procedural-surface decode");
    assert_eq!(
        round_trip.ir.model.procedural_surfaces[0].cache_fit_tolerance,
        Some(0.075)
    );
}

#[test]
fn generated_f3d_rewrites_nurbs_surface_control_grid() {
    let source = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated NURBS surface decode");
    let mut edited = decoded.ir;
    let surface = edited
        .model
        .surfaces
        .iter_mut()
        .find(|surface| {
            matches!(
                surface.geometry,
                cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(_)
            )
        })
        .expect("generated NURBS surface");
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs) = &mut surface.geometry else {
        unreachable!()
    };
    nurbs.control_points[2].x = 17.5;
    nurbs.control_points[2].z = -3.25;
    nurbs.u_degree = 2;
    nurbs.v_degree = 2;
    nurbs.u_knots = vec![-1.0, -1.0, -1.0, 2.0, 2.0];
    nurbs.v_knots = vec![-0.5, -0.5, -0.5, 1.5, 1.5];
    nurbs.u_periodic = true;
    let expected = nurbs.clone();
    let surface_id = surface.id.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("NURBS surface regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated NURBS surface decode");
    let surface = round_trip
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == surface_id)
        .expect("round-trip NURBS surface");
    assert_eq!(
        surface.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(expected)
    );
}

#[test]
fn generated_f3d_rewrites_rational_nurbs_surface_weights() {
    let source = f3d_with_smbh(&synthetic_rational_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated rational surface decode");
    let mut edited = decoded.ir;
    let surface = edited
        .model
        .surfaces
        .iter_mut()
        .find(|surface| {
            matches!(
                &surface.geometry,
                cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs)
                    if nurbs.weights.is_some()
            )
        })
        .expect("generated rational surface");
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs) = &mut surface.geometry else {
        unreachable!()
    };
    nurbs.weights.as_mut().expect("rational weights")[1] = 0.65;
    let expected = nurbs.clone();
    let surface_id = surface.id.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("rational-weight regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated rational surface decode");
    let surface = round_trip
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == surface_id)
        .expect("round-trip rational surface");
    assert_eq!(
        surface.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(expected)
    );
}

#[test]
fn generated_f3d_rewrites_extrusion_directrix_control_points() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let source = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated extrusion decode");
    let mut edited = decoded.ir;
    let ProceduralSurfaceDefinition::Extrusion { directrix, .. } =
        &edited.model.procedural_surfaces[0].definition
    else {
        panic!("expected extrusion")
    };
    let directrix_id = directrix.clone();
    let curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == directrix_id)
        .expect("extrusion directrix");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &mut curve.geometry else {
        panic!("expected NURBS directrix")
    };
    nurbs.control_points[1].y = 12.5;
    nurbs.control_points[1].z = -2.0;
    nurbs.degree = 1;
    nurbs.knots = vec![-2.0, -2.0, 3.0, 3.0, 3.0];
    nurbs.periodic = true;
    let expected = nurbs.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("extrusion-directrix regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated extrusion decode");
    let curve = round_trip
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == directrix_id)
        .expect("round-trip directrix");
    assert_eq!(
        curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Nurbs(expected)
    );
}

#[test]
fn decode_resolves_generated_ref_translational_extrusion() {
    let f3d = f3d_with_smbh(&synthetic_ref_cyl_spl_sur_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.procedural_surfaces.len(), 1);
    assert_eq!(
        result.ir.model.procedural_surfaces[0].cache_fit_tolerance,
        Some(0.02)
    );
}

#[test]
fn decode_resolves_revision_extrusion_implicit_directrix_reference() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let f3d = f3d_with_smbh(&synthetic_revision_ref_directrix_cyl_spl_sur_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.procedural_surfaces.len(), 1);
    assert!(matches!(
        result.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::Extrusion { .. }
    ));
    assert!(!result.report.losses.iter().any(|loss| loss.code
        == cadmpeg_ir::report::LossKind::shared(cadmpeg_ir::LossTaxonomy::GeometryNotTransferred)));
}

#[test]
fn decode_retains_generated_rolling_ball_definition() {
    use cadmpeg_ir::geometry::{BlendCrossSection, BlendRadiusLaw, ProceduralSurfaceDefinition};

    let f3d = f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    let procedural = result.ir.model.procedural_surfaces.first().unwrap();
    assert_eq!(procedural.cache_fit_tolerance, Some(0.01));
    let ProceduralSurfaceDefinition::Blend {
        supports,
        spine,
        radius,
        cross_section,
        ..
    } = &procedural.definition
    else {
        panic!("expected rolling-ball blend")
    };
    assert!(supports.iter().all(Option::is_some));
    assert!(supports.iter().flatten().all(|support| result
        .ir
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id == support.surface)));
    let spine = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| Some(&curve.id) == spine.as_ref())
        .expect("blend spine carrier");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(spine) = &spine.geometry else {
        panic!("expected NURBS blend spine")
    };
    assert_eq!(spine.control_points.len(), 3);
    assert_eq!(cross_section, &BlendCrossSection::Circular);
    assert_eq!(
        radius,
        &BlendRadiusLaw::Constant {
            signed_radius: -3.0
        }
    );
}

#[test]
fn generated_solved_plane_plane_blend_decodes_as_analytic_cylinder() {
    use cadmpeg_ir::geometry::{
        BlendRadiusLaw, CurveGeometry, NurbsCurve, ProceduralSurfaceDefinition, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated rolling-ball decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let ProceduralSurfaceDefinition::Blend {
        supports,
        spine: Some(spine),
        radius,
        ..
    } = &mut source_less.model.procedural_surfaces[0].definition
    else {
        panic!("expected rolling-ball definition")
    };
    let support_ids = [
        supports[0].as_ref().expect("first support").surface.clone(),
        supports[1]
            .as_ref()
            .expect("second support")
            .surface
            .clone(),
    ];
    let spine_id = spine.clone();
    *radius = BlendRadiusLaw::Constant {
        signed_radius: -2.0,
    };
    let support_geometry = [
        SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(1.0, 0.0, 0.0),
            u_axis: Vector3::new(0.0, 1.0, 0.0),
        },
        SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
    ];
    for (id, geometry) in support_ids.into_iter().zip(support_geometry) {
        source_less
            .model
            .surfaces
            .iter_mut()
            .find(|surface| surface.id == id)
            .expect("rolling-ball support")
            .geometry = geometry;
    }
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine_id)
        .expect("rolling-ball spine")
        .geometry = CurveGeometry::Nurbs(NurbsCurve {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point3::new(2.0, 2.0, -4.0),
            Point3::new(2.0, 2.0, 0.0),
            Point3::new(2.0, 2.0, 7.0),
        ],
        weights: None,
        periodic: false,
    });

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less rolling-ball encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less rolling-ball round trip");
    let carrier_id = &round_trip.ir.model.procedural_surfaces[0].surface;
    assert!(matches!(
        round_trip
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| &surface.id == carrier_id)
            .expect("rolling-ball carrier")
            .geometry,
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        } if origin == Point3::new(2.0, 2.0, -4.0)
            && axis == Vector3::new(0.0, 0.0, 1.0)
            && radius == 2.0
    ));
}

#[test]
fn generated_rolling_ball_surface_aliases_decode_and_write_canonically() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    for name in ["rbblnsur", "pipe_spl_sur", "pipesur"] {
        let bytes =
            with_legacy_subtype(synthetic_rb_blend_spl_sur_smbh(), "rb_blend_spl_sur", name);
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&bytes)),
                &DecodeOptions::default(),
            )
            .expect("rolling-ball alias decode");
        assert!(matches!(
            result.ir.model.procedural_surfaces[0].definition,
            ProceduralSurfaceDefinition::Blend { .. }
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
            .expect("canonical rolling-ball encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("canonical rolling-ball round trip");
        assert!(matches!(
            round_trip.ir.model.procedural_surfaces[0].definition,
            ProceduralSurfaceDefinition::Blend { .. }
        ));
    }
}

#[test]
fn generated_f3d_rewrites_rolling_ball_radius_law() {
    use cadmpeg_ir::geometry::{BlendRadiusLaw, ProceduralSurfaceDefinition};

    let source = f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated rolling-ball decode");
    let mut edited = decoded.ir;
    let ProceduralSurfaceDefinition::Blend { radius, .. } =
        &mut edited.model.procedural_surfaces[0].definition
    else {
        panic!("expected rolling-ball blend")
    };
    *radius = BlendRadiusLaw::Linear {
        start: -2.0,
        end: -4.0,
    };

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("rolling-ball radius regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated rolling-ball decode");
    let ProceduralSurfaceDefinition::Blend { radius, .. } =
        &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected round-trip rolling-ball blend")
    };
    assert_eq!(
        radius,
        &BlendRadiusLaw::Linear {
            start: -2.0,
            end: -4.0,
        }
    );
}

#[test]
fn generated_f3d_rewrites_rolling_ball_spine_cache() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let source = f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated rolling-ball decode");
    let mut edited = decoded.ir;
    let ProceduralSurfaceDefinition::Blend {
        spine: Some(spine), ..
    } = &edited.model.procedural_surfaces[0].definition
    else {
        panic!("expected rolling-ball spine")
    };
    let spine_id = spine.clone();
    let curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine_id)
        .expect("blend spine curve");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &mut curve.geometry else {
        panic!("expected NURBS blend spine")
    };
    nurbs.control_points[1].x = 8.0;
    nurbs.control_points[1].y = -6.0;
    nurbs.degree = 1;
    nurbs.knots = vec![-1.0, -1.0, 2.0, 2.0, 2.0];
    let expected = curve.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("blend-spine regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated rolling-ball decode");
    assert!(round_trip
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve == &expected));
}

#[test]
fn generated_f3d_rewrites_rolling_ball_support_cache() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let source = f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated rolling-ball decode");
    let mut edited = decoded.ir;
    let ProceduralSurfaceDefinition::Blend { supports, .. } =
        &edited.model.procedural_surfaces[0].definition
    else {
        panic!("expected rolling-ball blend")
    };
    let support_id = supports[0]
        .as_ref()
        .expect("first blend support")
        .surface
        .clone();
    let surface = edited
        .model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == support_id)
        .expect("blend support surface");
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs) = &mut surface.geometry else {
        panic!("expected NURBS blend support")
    };
    nurbs.control_points[1].x = 6.0;
    nurbs.control_points[1].z = 4.0;
    nurbs.u_degree = 2;
    nurbs.u_knots = vec![-1.0, -1.0, -1.0, 2.0, 2.0];
    let expected = surface.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("blend-support regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated rolling-ball decode");
    assert!(round_trip
        .ir
        .model
        .surfaces
        .iter()
        .any(|surface| surface == &expected));
}

#[test]
fn decode_reports_generated_partial_rolling_ball_supports() {
    let f3d = f3d_with_smbh(&synthetic_partial_rb_blend_spl_sur_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    assert!(result.report.losses.iter().any(|loss| loss
        .message
        .contains("only one of two native supports resolved")));
}

#[test]
fn subtype_reference_resolves_surface_cache() {
    let mut target = Vec::new();
    target.extend_from_slice(b"\x0f\x0d\x07surface");
    // A payload byte equal to SUBTYPE_CLOSE must not terminate the span.
    target.push(0x06);
    target.extend_from_slice(&[0x10, 0, 0, 0, 0, 0, 0, 0]);
    target.extend_from_slice(&generated_surface_block());
    target.push(0x10);

    let mut source = Vec::new();
    source.extend_from_slice(b"\x0f\x0d\x03ref\x04");
    source.extend_from_slice(&0i64.to_le_bytes());
    source.push(0x10);

    let mut active = target;
    active.extend_from_slice(&source);
    let decoded = cadmpeg_asm::nurbs::core::surface_cache_resolving_refs(
        &cadmpeg_asm::nurbs::toks::lex_test_span(&source, 8),
        &cadmpeg_asm::nurbs::toks::test_table(&active, 8),
    )
    .expect("subtype-table reference resolves to its surface cache");
    assert_eq!((decoded.u_count, decoded.v_count), (2, 2));
}

/// A form-2 `par_int_cur` whose uv pcurve runs from `first` to `second`.
pub(super) fn generated_form_two_par_int_cur(first: [f64; 2], second: [f64; 2]) -> Vec<u8> {
    let mut scope = vec![0x0f];
    t_ident(&mut scope, "par_int_cur");
    push_tagged_i64(&mut scope, 0x04, 1);
    push_tagged_i64(&mut scope, 0x15, 2);
    for bound in [0.0, 1.0] {
        scope.push(0x0a);
        push_tagged_f64(&mut scope, bound);
    }
    push_tagged_i64(&mut scope, 0x15, 0);
    t_ident(&mut scope, "spline");
    scope.push(0x0b);
    scope.push(0x0f);
    t_ident(&mut scope, "exact_spl_sur");
    scope.extend_from_slice(&generated_surface_block());
    scope.push(0x10);
    scope.extend_from_slice(&[0x0b; 4]);
    t_ident(&mut scope, "null_surface");
    scope.extend_from_slice(b"\x0d\x04nubs");
    push_tagged_i64(&mut scope, 0x04, 1);
    push_tagged_i64(&mut scope, 0x15, 0);
    push_tagged_i64(&mut scope, 0x04, 2);
    for (knot, multiplicity) in [(0.0, 1i64), (1.0, 1)] {
        push_tagged_f64(&mut scope, knot);
        push_tagged_i64(&mut scope, 0x04, multiplicity);
    }
    for [u, v] in [first, second] {
        push_tagged_f64(&mut scope, u);
        push_tagged_f64(&mut scope, v);
    }
    t_ident(&mut scope, "nullbs");
    push_tagged_i64(&mut scope, 0x04, 0);
    scope.push(0x10);
    scope
}

#[test]
fn a_form_two_par_int_cur_decodes_as_its_support_isoline() {
    use cadmpeg_asm::nurbs::proc_curve::decode_par_int_cur_isoline;
    use cadmpeg_ir::math::Point3;

    // The support is the unit bilinear patch scaled to millimetres, so the
    // isoline at u = 1 is the patch's far edge.
    let scope = generated_form_two_par_int_cur([1.0, 0.0], [1.0, 1.0]);
    let curve = decode_par_int_cur_isoline(&scope, 8, None).expect("form-2 isoline");
    assert_eq!(curve.degree, 1);
    assert_eq!(curve.knots, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        curve.control_points,
        [Point3::new(10.0, 0.0, 0.0), Point3::new(10.0, 10.0, 0.0)]
    );

    // A pcurve that crosses the support holds neither parameter fixed, so no
    // NURBS curve reproduces it and the form is refused.
    let diagonal = generated_form_two_par_int_cur([0.0, 0.0], [1.0, 1.0]);
    assert!(decode_par_int_cur_isoline(&diagonal, 8, None).is_none());

    // A pcurve running only part of the support's domain would need a trim.
    let partial = generated_form_two_par_int_cur([1.0, 0.0], [1.0, 0.5]);
    assert!(decode_par_int_cur_isoline(&partial, 8, None).is_none());
}

#[test]
fn a_nested_construction_cache_is_not_the_enclosing_scope_cache() {
    use cadmpeg_asm::nurbs::core::{decode_curve_cache, decode_owned_curve_cache_at};

    // A `par_int_cur` whose cache slot is `nullbs` and whose support is an
    // intcurve construction carrying a curve block of its own.
    let mut scope = vec![0x0f];
    t_ident(&mut scope, "par_int_cur");
    scope.push(0x0f);
    t_ident(&mut scope, "exact_int_cur");
    scope.extend_from_slice(&generated_curve_block());
    scope.push(0x10);
    t_ident(&mut scope, "nullbs");
    scope.push(0x10);

    assert!(decode_curve_cache(&scope).is_some());
    assert!(decode_owned_curve_cache_at(&scope, 8).is_none());
}

#[test]
fn a_nested_construction_does_not_claim_its_enclosing_record() {
    use cadmpeg_asm::nurbs::proc_surface::{
        procedural_surface_resolving_refs, DecodedProceduralSurfaceDefinition,
    };

    let bytes = synthetic_cyl_spl_sur_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let record = &records[9];
    let owned = bytes[record.offset..record.offset + record.len].to_vec();
    let decoded = procedural_surface_resolving_refs(
        &record.tokens,
        &cadmpeg_asm::nurbs::toks::SubtypeTable::from_records(std::slice::from_ref(record)),
    )
    .expect("the record owns its extrusion");
    assert!(matches!(
        decoded.definition,
        DecodedProceduralSurfaceDefinition::Extrusion { .. }
    ));

    // The same extrusion nested inside a variable-blend scope is that blend's
    // support surface, not the record's own surface.
    let marker = b"\x0f\x0d\x0bcyl_spl_sur";
    let at = owned
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap();
    let mut nested = owned.clone();
    nested.splice(at..at, *b"\x0f\x0d\x14srf_srf_v_bl_spl_sur");
    let terminator = nested.len() - 1;
    nested.insert(terminator, 0x10);
    let nested_records = cadmpeg_asm::sab::frame(&nested, 0, nested.len(), 8).unwrap();
    assert!(procedural_surface_resolving_refs(
        &nested_records[0].tokens,
        &cadmpeg_asm::nurbs::toks::SubtypeTable::from_records(&nested_records),
    )
    .is_none());
}

#[test]
fn subtype_table_walks_wide_strings_at_the_stream_ref_width() {
    for ref_width in [4usize, 8] {
        // The last four payload bytes spell a definition opening. Only a walker
        // that consumes the length prefix at `ref_width` steps past them.
        let payload = [b'0', b'1', b'2', b'3', 0x0f, 0x0d, 0x01, b'x'];

        let mut active = Vec::new();
        t_ident(&mut active, "tspl");
        active.push(0x09);
        active.extend_from_slice(&payload.len().to_le_bytes()[..ref_width]);
        active.extend_from_slice(&payload);
        let definition = active.len();
        active.extend_from_slice(b"\x0f\x0d\x08real_def\x10");
        active.push(0x11);

        let tables = cadmpeg_asm::nurbs::subtypes::SubtypeTables::from_stream(&active);
        assert_eq!(tables.for_width(ref_width), [definition]);
    }
}

#[test]
fn rgb_attribute_chain_decodes_body_color() {
    use std::collections::HashMap;

    let mut bytes = Vec::new();
    t_ident(&mut bytes, "body");
    t_ref(&mut bytes, 1); // attrib-chain head
    t_end(&mut bytes);
    t_subident(&mut bytes, "rgb_color");
    t_subident(&mut bytes, "st");
    t_ident(&mut bytes, "attrib");
    t_ref(&mut bytes, -1); // next attrib
    t_dbl(&mut bytes, 0.1);
    t_dbl(&mut bytes, 0.2);
    t_dbl(&mut bytes, 0.3);
    t_end(&mut bytes);

    let records = cadmpeg_asm::sab::frame(&bytes, 0, bytes.len(), 8).unwrap();
    let by_index: HashMap<i64, _> = records.iter().map(|r| (r.index as i64, r)).collect();
    let color =
        cadmpeg_asm::brep::attributes::attribute_chain_color(&records[0], &by_index).unwrap();
    assert_eq!((color.r, color.g, color.b, color.a), (0.1, 0.2, 0.3, 1.0));
}

#[test]
fn truecolor_attribute_chain_decodes_by_color_as_opaque_rgb() {
    use std::collections::HashMap;

    let mut bytes = Vec::new();
    t_ident(&mut bytes, "face");
    t_ref(&mut bytes, 1);
    t_end(&mut bytes);
    t_subident(&mut bytes, "truecolor");
    t_subident(&mut bytes, "adesk");
    t_ident(&mut bytes, "attrib");
    t_ref(&mut bytes, -1);
    bytes.push(0x17);
    bytes.extend_from_slice(&(0xc240_80c0i64).to_le_bytes());
    t_end(&mut bytes);

    let records = cadmpeg_asm::sab::frame(&bytes, 0, bytes.len(), 8).unwrap();
    let by_index: HashMap<i64, _> = records.iter().map(|r| (r.index as i64, r)).collect();
    let color =
        cadmpeg_asm::brep::attributes::attribute_chain_color(&records[0], &by_index).unwrap();
    assert_eq!(
        (color.r, color.g, color.b, color.a),
        (64.0 / 255.0, 128.0 / 255.0, 192.0 / 255.0, 1.0)
    );
}

#[test]
fn bt_text_color_attribute_chain_decodes_rgb() {
    use std::collections::HashMap;

    let mut bytes = Vec::new();
    t_ident(&mut bytes, "face");
    t_ref(&mut bytes, 1);
    t_end(&mut bytes);
    t_subident(&mut bytes, "entatt_color");
    t_subident(&mut bytes, "bt");
    t_ident(&mut bytes, "attrib");
    t_ref(&mut bytes, -1);
    push_u8_string(&mut bytes, "4227264"); // 0x4080c0
    t_end(&mut bytes);

    let records = cadmpeg_asm::sab::frame(&bytes, 0, bytes.len(), 8).unwrap();
    let by_index: HashMap<i64, _> = records.iter().map(|r| (r.index as i64, r)).collect();
    let color =
        cadmpeg_asm::brep::attributes::attribute_chain_color(&records[0], &by_index).unwrap();
    assert_eq!(
        (color.r, color.g, color.b, color.a),
        (64.0 / 255.0, 128.0 / 255.0, 192.0 / 255.0, 1.0)
    );
}

#[test]
fn bt_text_color_rejects_non_decimal_and_overwide_values() {
    use std::collections::HashMap;

    for value in ["", "+4227264", "0x4080c0", "16777216"] {
        let mut bytes = Vec::new();
        t_ident(&mut bytes, "face");
        t_ref(&mut bytes, 1);
        t_end(&mut bytes);
        t_subident(&mut bytes, "entatt_color");
        t_subident(&mut bytes, "bt");
        t_ident(&mut bytes, "attrib");
        t_ref(&mut bytes, -1);
        push_u8_string(&mut bytes, value);
        t_end(&mut bytes);

        let records = cadmpeg_asm::sab::frame(&bytes, 0, bytes.len(), 8).unwrap();
        let by_index: HashMap<i64, _> = records.iter().map(|r| (r.index as i64, r)).collect();
        assert!(
            cadmpeg_asm::brep::attributes::attribute_chain_color(&records[0], &by_index).is_none()
        );
    }
}

#[test]
fn invalid_color_attribute_does_not_hide_later_chain_color() {
    use std::collections::HashMap;

    let mut bytes = Vec::new();
    t_ident(&mut bytes, "face");
    t_ref(&mut bytes, 1);
    t_end(&mut bytes);
    t_subident(&mut bytes, "entatt_color");
    t_subident(&mut bytes, "bt");
    t_ident(&mut bytes, "attrib");
    t_ref(&mut bytes, 2);
    push_u8_string(&mut bytes, "not-a-color");
    t_end(&mut bytes);
    t_subident(&mut bytes, "rgb_color");
    t_subident(&mut bytes, "st");
    t_ident(&mut bytes, "attrib");
    t_ref(&mut bytes, -1);
    t_dbl(&mut bytes, 0.1);
    t_dbl(&mut bytes, 0.2);
    t_dbl(&mut bytes, 0.3);
    t_end(&mut bytes);

    let records = cadmpeg_asm::sab::frame(&bytes, 0, bytes.len(), 8).unwrap();
    let by_index: HashMap<i64, _> = records.iter().map(|r| (r.index as i64, r)).collect();
    let color =
        cadmpeg_asm::brep::attributes::attribute_chain_color(&records[0], &by_index).unwrap();
    assert_eq!((color.r, color.g, color.b, color.a), (0.1, 0.2, 0.3, 1.0));
}
