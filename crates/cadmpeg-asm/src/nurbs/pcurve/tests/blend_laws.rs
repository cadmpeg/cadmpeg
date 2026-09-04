// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn variable_blend_side_integer_extension_decodes_at_both_integer_widths() {
    use cadmpeg_ir::geometry::VariableBlendSupportKind;

    for int_width in [4usize, 8] {
        for (name, kind) in [
            (
                "blend_support_cos_curve",
                VariableBlendSupportKind::CosineCurve,
            ),
            ("blendsupcos", VariableBlendSupportKind::CosineCurve),
            ("blend_support_curve", VariableBlendSupportKind::Curve),
            ("blendsupcur", VariableBlendSupportKind::Curve),
            (
                "blend_support_point_curve",
                VariableBlendSupportKind::PointCurve,
            ),
            ("blendsuppnt", VariableBlendSupportKind::PointCurve),
            ("blend_support_surface", VariableBlendSupportKind::Surface),
            ("blendsupsur", VariableBlendSupportKind::Surface),
            (
                "blend_support_zero_curve",
                VariableBlendSupportKind::ZeroCurve,
            ),
            ("blendsupzro", VariableBlendSupportKind::ZeroCurve),
        ] {
            for expected in [None, Some(0), Some(3)] {
                let bytes = variable_blend_side(int_width, name, expected);
                let mut position = 0;
                let side = decode_rolling_ball_side(&bytes, &mut position, int_width, None)
                    .unwrap_or_else(|| {
                        panic!(
                            "variable-blend support side {name} width {int_width} extension {expected:?}"
                        )
                    });
                assert_eq!(position, bytes.len() - 1);
                assert_eq!(side.support_kind, kind);
                assert_eq!(side.extension, expected);
                assert_eq!(side.location, Point3::new(10.0, 20.0, 30.0));
                assert!(side.surface.is_none());
                assert!(side.curve.is_none());
                assert!(side.secondary_pcurve.is_none());
                assert!(side.tertiary_pcurve.is_none());
            }
        }
    }
}

#[test]
fn fixed_arity_law_operators_decode_at_both_integer_widths() {
    for int_width in [4usize, 8] {
        let mut bytes = Vec::new();
        push_string(&mut bytes, "SET");
        push_f64(&mut bytes, -2.0);
        push_string(&mut bytes, "ROTATE");
        push_vector(&mut bytes, [1.0, 2.0, 3.0]);
        push_string(&mut bytes, "TRANS");
        for scalar in 0..13 {
            push_f64(&mut bytes, f64::from(scalar));
        }
        for value in [4, 5, 6] {
            push_int(&mut bytes, 0x15, value, int_width);
        }
        push_string(&mut bytes, "TERM");
        push_vector(&mut bytes, [7.0, 8.0, 9.0]);
        push_int(&mut bytes, 0x04, 1, int_width);

        let toks = lex_test_span(&bytes, int_width);
        let mut cur = crate::nurbs::toks::Cur::at(&toks, 0);
        let set = crate::nurbs::proc_surface::law_expression(&mut cur, 0).unwrap();
        let rotate = crate::nurbs::proc_surface::law_expression(&mut cur, 0).unwrap();
        let term = crate::nurbs::proc_surface::law_expression(&mut cur, 0).unwrap();
        assert_eq!(cur.pos(), toks.len());
        assert!(matches!(
            set,
            EmbeddedLawExpression::Algebraic { operator, operands }
                if operator == "SET" && operands.len() == 1
        ));
        assert!(matches!(
            rotate,
            EmbeddedLawExpression::Algebraic { operator, operands }
                if operator == "ROTATE" && operands.len() == 2
        ));
        assert!(matches!(
            term,
            EmbeddedLawExpression::Algebraic { operator, operands }
                if operator == "TERM" && operands.len() == 2
        ));
    }
}

#[test]
fn law_surface_layout_decodes_at_both_integer_widths() {
    for int_width in [4usize, 8] {
        let mut bytes = vec![0x0f];
        push_ident(&mut bytes, "law_spl_sur");
        push_string(&mut bytes, "primary-law");
        push_int(&mut bytes, 0x04, 1, int_width);
        push_string(&mut bytes, "SET");
        push_f64(&mut bytes, -2.5);
        push_int(&mut bytes, 0x04, 1, int_width);
        push_string(&mut bytes, "aux-law");
        push_int(&mut bytes, 0x04, 1, int_width);
        push_string(&mut bytes, "TERM");
        push_vector(&mut bytes, [1.0, 2.0, 3.0]);
        push_int(&mut bytes, 0x04, 1, int_width);
        push_int(&mut bytes, 0x15, 0, int_width);
        bytes.extend_from_slice(&surface_block(int_width));
        push_f64(&mut bytes, 0.007);
        for values in [
            &[0.1][..],
            &[0.2, 0.3][..],
            &[][..],
            &[][..],
            &[][..],
            &[][..],
        ] {
            push_int(&mut bytes, 0x04, values.len() as i64, int_width);
            for value in values {
                push_f64(&mut bytes, *value);
            }
        }
        bytes.push(0x10);

        let decoded = crate::nurbs::proc_surface::law_spl_sur(&lex_test_span(&bytes, int_width))
            .unwrap_or_else(|| panic!("law surface at width {int_width}"));
        let DecodedProceduralSurfaceDefinition::Law(construction) = decoded.definition else {
            panic!("expected law surface at width {int_width}")
        };
        assert_eq!(construction.parameter_ranges, None);
        assert_eq!(construction.primary.name(), "primary-law");
        assert_eq!(construction.additional.len(), 1);
        assert_eq!(construction.discontinuities[1], [0.2, 0.3]);
        assert_eq!(decoded.cache_fit_tolerance, Some(0.07));
    }
}

#[test]
fn legacy_law_surface_uses_implicit_full_tail_at_both_integer_widths() {
    for int_width in [4usize, 8] {
        let mut bytes = vec![0x0f];
        push_ident(&mut bytes, "lawsur");
        for value in [-1.0, 2.0, -3.0, 4.0] {
            push_f64(&mut bytes, value);
        }
        push_string(&mut bytes, "null_law");
        push_int(&mut bytes, 0x04, 0, int_width);
        bytes.extend_from_slice(&surface_block(int_width));
        push_f64(&mut bytes, 0.007);
        for _ in 0..6 {
            push_int(&mut bytes, 0x04, 0, int_width);
        }
        bytes.push(0x10);

        let decoded = crate::nurbs::proc_surface::law_spl_sur(&lex_test_span(&bytes, int_width))
            .unwrap_or_else(|| panic!("legacy law surface at width {int_width}"));
        let DecodedProceduralSurfaceDefinition::Law(construction) = decoded.definition else {
            panic!("expected legacy law surface")
        };
        assert_eq!(
            construction.parameter_ranges,
            Some([[-1.0, 2.0], [-3.0, 4.0]])
        );
        assert!(matches!(
            construction.tail,
            cadmpeg_ir::geometry::LawSurfaceTail::Full
        ));
        assert_eq!(decoded.cache_fit_tolerance, Some(0.07));
    }
}

#[test]
fn cacheless_law_surface_tails_decode_at_both_integer_widths() {
    for int_width in [4usize, 8] {
        for selector in 1..=4 {
            let mut bytes = vec![0x0f];
            push_ident(&mut bytes, "law_spl_sur");
            push_string(&mut bytes, "null_law");
            push_int(&mut bytes, 0x04, 0, int_width);
            push_int(&mut bytes, 0x15, selector, int_width);
            match selector {
                1 => {
                    for values in [&[0.0, 1.0][..], &[-1.0, 2.0][..]] {
                        push_int(&mut bytes, 0x04, values.len() as i64, int_width);
                        for value in values {
                            push_f64(&mut bytes, *value);
                        }
                    }
                    push_f64(&mut bytes, 0.008);
                    for value in [0, 2, 1, 3] {
                        push_int(&mut bytes, 0x15, value, int_width);
                    }
                }
                2 => {
                    for value in [-0.5, 1.5, -2.0, 2.0] {
                        push_f64(&mut bytes, value);
                    }
                    for value in [1, 2, 0, 4] {
                        push_int(&mut bytes, 0x15, value, int_width);
                    }
                }
                3 | 4 => {}
                _ => unreachable!(),
            }
            for _ in 0..6 {
                push_int(&mut bytes, 0x04, 0, int_width);
            }
            bytes.push(0x10);

            let decoded =
                crate::nurbs::proc_surface::law_spl_sur(&lex_test_span(&bytes, int_width))
                    .unwrap_or_else(|| panic!("law tail {selector} at integer width {int_width}"));
            let DecodedProceduralSurfaceDefinition::Law(construction) = decoded.definition else {
                panic!("expected law surface")
            };
            assert_eq!(decoded.cache_fit_tolerance, None);
            assert!(matches!(
                (&construction.tail, selector),
                (cadmpeg_ir::geometry::LawSurfaceTail::Summary { .. }, 1)
                    | (cadmpeg_ir::geometry::LawSurfaceTail::None { .. }, 2)
                    | (cadmpeg_ir::geometry::LawSurfaceTail::Historical, 3)
                    | (cadmpeg_ir::geometry::LawSurfaceTail::Optimal, 4)
            ));
        }
    }
}

#[test]
fn sub_surface_layout_decodes_at_both_integer_widths() {
    for int_width in [4usize, 8] {
        for name in ["sub_spl_sur", "subsur"] {
            let mut bytes = vec![0x0f];
            push_ident(&mut bytes, name);
            for value in [-1.0, 2.0, -3.0, 4.0] {
                push_f64(&mut bytes, value);
            }
            push_ident(&mut bytes, "plane");
            push_position(&mut bytes, [0.1, -0.2, 0.3]);
            push_vector(&mut bytes, [0.0, 0.0, 1.0]);
            push_vector(&mut bytes, [1.0, 0.0, 0.0]);
            bytes.push(0x0b);
            bytes.push(0x10);

            let decoded =
                crate::nurbs::proc_surface::sub_spl_sur(&lex_test_span(&bytes, int_width))
                    .unwrap_or_else(|| panic!("{name} at integer width {int_width}"));
            let DecodedProceduralSurfaceDefinition::SubSurface {
                support,
                parameter_ranges,
            } = decoded.definition
            else {
                panic!("expected sub-surface")
            };
            assert_eq!(parameter_ranges, [[-1.0, 2.0], [-3.0, 4.0]]);
            assert!(matches!(
                support,
                SurfaceGeometry::Plane { origin, .. }
                    if origin == Point3::new(1.0, -2.0, 3.0)
            ));
            assert_eq!(decoded.cache_fit_tolerance, None);
        }
    }
}

#[test]
fn rolling_ball_layout_walks_both_integer_widths() {
    for int_width in [4usize, 8] {
        let mut bytes = vec![0x0f];
        push_ident(&mut bytes, "rb_blend_spl_sur");
        push_int(&mut bytes, 0x04, 22507, int_width);
        bytes.extend_from_slice(&rolling_ball_side(int_width, "left"));
        bytes.extend_from_slice(&rolling_ball_side(int_width, "right"));
        bytes.extend_from_slice(&curve_block(int_width));
        push_f64(&mut bytes, -0.3);
        push_f64(&mut bytes, -0.6);
        push_int(&mut bytes, 0x15, -1, int_width);
        bytes.push(0x10);

        let layout = rolling_ball_patch_layout(&bytes, int_width)
            .unwrap_or_else(|| panic!("rolling-ball layout at width {int_width}"));
        let values = layout
            .radii
            .map(|offset| f64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()));
        assert_eq!(values, [-0.3, -0.6]);

        let mut compact = vec![0x0f];
        push_ident(&mut compact, "pipe_spl_sur");
        for (label, kind) in [("left", "plane"), ("right", "sphere")] {
            push_string(&mut compact, label);
            push_ident(&mut compact, kind);
            compact.extend_from_slice(&surface_block(int_width));
        }
        compact.extend_from_slice(&curve_block(int_width));
        push_f64(&mut compact, -1.5);
        push_f64(&mut compact, -2.5);
        push_int(&mut compact, 0x15, -1, int_width);
        compact.push(0x10);
        let layout = rolling_ball_patch_layout(&compact, int_width)
            .unwrap_or_else(|| panic!("compact rolling-ball layout at width {int_width}"));
        let values = layout
            .radii
            .map(|offset| f64::from_le_bytes(compact[offset..offset + 8].try_into().unwrap()));
        assert_eq!(values, [-1.5, -2.5]);
    }
}

#[test]
fn rolling_ball_curves_decode_analytic_and_nested_intcurve_forms() {
    for int_width in [4usize, 8] {
        let mut straight = Vec::new();
        push_ident(&mut straight, "straight");
        push_position(&mut straight, [1.0, 2.0, 3.0]);
        push_vector(&mut straight, [0.0, 2.0, 0.0]);
        straight.push(0x0a);
        push_f64(&mut straight, -2.0);
        straight.push(0x0a);
        push_f64(&mut straight, 3.0);
        let mut position = 0;
        assert!(matches!(
            decode_rolling_ball_curve(&straight, &mut position, int_width, None),
            Some(DecodedRollingBallCurve {
                geometry: CurveGeometry::Line { origin, direction },
                parameter_range: [Some(-2.0), Some(3.0)],
            })
                if origin == Point3::new(10.0, 20.0, 30.0)
                    && direction == Vector3::new(0.0, 1.0, 0.0)
        ));
        assert_eq!(position, straight.len());

        let mut intcurve = Vec::new();
        push_ident(&mut intcurve, "intcurve");
        intcurve.push(0x0b);
        intcurve.push(0x0f);
        push_ident(&mut intcurve, "exact_int_cur");
        intcurve.extend_from_slice(&curve_block(int_width));
        intcurve.push(0x10);
        intcurve.extend_from_slice(&[0x0b, 0x0b]);
        let mut position = 0;
        assert!(matches!(
            decode_rolling_ball_curve(&intcurve, &mut position, int_width, None),
            Some(DecodedRollingBallCurve {
                geometry: CurveGeometry::Nurbs(curve),
                parameter_range: [None, None],
            }) if curve.degree == 1
        ));
        assert_eq!(position, intcurve.len());

        let mut active = vec![0x0f];
        push_ident(&mut active, "exact_int_cur");
        active.extend_from_slice(&curve_block(int_width));
        active.push(0x10);
        let mut reference = vec![0x0f];
        push_ident(&mut reference, "holder");
        reference.push(0x0f);
        push_ident(&mut reference, "ref");
        push_int(&mut reference, 0x04, 0, int_width);
        reference.push(0x10);
        reference.push(0x10);
        active.extend_from_slice(&reference);
        let tables = SubtypeTables::from_stream(&active);
        let mut intcurve = Vec::new();
        push_ident(&mut intcurve, "intcurve");
        intcurve.push(0x0b);
        intcurve.extend_from_slice(&reference);
        intcurve.extend_from_slice(&[0x0b, 0x0b]);
        let mut position = 0;
        assert!(matches!(
            decode_rolling_ball_curve(
                &intcurve,
                &mut position,
                int_width,
                Some((&active, &tables)),
            ),
            Some(DecodedRollingBallCurve {
                geometry: CurveGeometry::Nurbs(curve),
                parameter_range: [None, None],
            }) if curve.degree == 1
        ));
        assert_eq!(position, intcurve.len());
    }
}

#[test]
fn rolling_ball_surfaces_decode_framed_spline_supports() {
    for int_width in [4usize, 8] {
        let mut bytes = Vec::new();
        push_ident(&mut bytes, "spline");
        bytes.push(0x0b);
        bytes.push(0x0f);
        push_ident(&mut bytes, "exact_spl_sur");
        bytes.extend_from_slice(&surface_block(int_width));
        bytes.push(0x10);
        for value in [-1.0, 2.0, -3.0, 4.0] {
            bytes.push(0x0a);
            push_f64(&mut bytes, value);
        }
        let mut position = 0;
        assert!(matches!(
            decode_rolling_ball_surface(&bytes, &mut position, int_width, None),
            Some((
                SurfaceGeometry::Nurbs(surface),
                [[Some(-1.0), Some(2.0)], [Some(-3.0), Some(4.0)]],
            ))
                if surface.u_degree == 1 && surface.v_degree == 1
        ));
        assert_eq!(position, bytes.len());
    }
}
