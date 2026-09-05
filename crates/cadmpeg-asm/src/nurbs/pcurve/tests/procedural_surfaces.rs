// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::kernel_header::RefWidth;

#[test]
fn offset_surface_uses_direct_support_fields_then_cache() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        for name in ["off_spl_sur", "offsur"] {
            let mut bytes = vec![0x0f];
            push_ident(&mut bytes, name);
            push_ident(&mut bytes, "plane");
            push_position(&mut bytes, [0.5, 1.0, 1.5]);
            push_vector(&mut bytes, [0.0, 0.0, 1.0]);
            push_vector(&mut bytes, [1.0, 0.0, 0.0]);
            bytes.push(0x0b);
            push_f64(&mut bytes, -0.25);
            push_int(&mut bytes, 0x15, 2, int_width);
            push_int(&mut bytes, 0x15, 3, int_width);
            if name == "off_spl_sur" {
                bytes.push(0x0b);
            }
            bytes.extend_from_slice(&surface_block(int_width));
            push_f64(&mut bytes, 0.001);
            bytes.push(0x10);

            let tokens = lex_test_span(&bytes, int_width);
            let decoded = crate::nurbs::proc_surface::procedural_surface_resolving_refs(
                &tokens,
                &test_table(&bytes, int_width),
            )
            .unwrap_or_else(|| panic!("offset surface {name} at width {int_width}"));
            let fit_tolerance = decoded.cache_fit_tolerance.expect("fit tolerance");
            let DecodedProceduralSurfaceDefinition::Offset {
                support,
                distance,
                u_sense,
                v_sense,
                extension,
            } = decoded.definition
            else {
                panic!("expected legacy offset surface");
            };

            assert!(matches!(
                support,
                SurfaceGeometry::Plane { origin, .. }
                    if (origin.x - 5.0).abs() < f64::EPSILON
                        && (origin.y - 10.0).abs() < f64::EPSILON
                        && (origin.z - 15.0).abs() < f64::EPSILON
            ));
            assert!((distance - -2.5).abs() < f64::EPSILON);
            assert_eq!(u_sense, Some(2));
            assert_eq!(v_sense, Some(3));
            let cadmpeg_ir::geometry::OffsetExtension::Legacy(flags) = extension else {
                panic!("expected legacy offset extension")
            };
            assert_eq!(
                flags.wire_values(),
                if name == "off_spl_sur" {
                    vec![false]
                } else {
                    vec![]
                }
            );
            assert!((fit_tolerance - 0.01).abs() < f64::EPSILON * 10.0);
        }
    }
}

#[test]
fn offset_surface_rejects_nested_cache_substitution() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let mut bytes = vec![0x0f];
        push_ident(&mut bytes, "offsur");
        push_ident(&mut bytes, "plane");
        push_position(&mut bytes, [0.0, 0.0, 0.0]);
        push_vector(&mut bytes, [0.0, 0.0, 1.0]);
        push_vector(&mut bytes, [1.0, 0.0, 0.0]);
        bytes.push(0x0b);
        push_f64(&mut bytes, 0.25);
        push_int(&mut bytes, 0x15, 0, int_width);
        push_int(&mut bytes, 0x15, 0, int_width);
        bytes.push(0x0f);
        push_ident(&mut bytes, "support");
        bytes.extend_from_slice(&surface_block(int_width));
        bytes.push(0x10);
        push_f64(&mut bytes, 0.001);
        bytes.push(0x10);

        let tokens = lex_test_span(&bytes, int_width);

        assert!(
            crate::nurbs::proc_surface::procedural_surface_resolving_refs(
                &tokens,
                &test_table(&bytes, int_width),
            )
            .is_none()
        );
    }
}

#[test]
fn revision_deformable_surface_mode3_preserves_its_distinct_frame() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let mut bytes = vec![0x0f];
        push_ident(&mut bytes, "defm_spl_sur");
        push_int(&mut bytes, 0x04, 22_506, int_width);
        push_ident(&mut bytes, "cone");
        push_position(&mut bytes, [0.0, 0.0, 0.0]);
        push_vector(&mut bytes, [0.0, 0.0, 1.0]);
        push_vector(&mut bytes, [2.0, 0.0, 0.0]);
        push_f64(&mut bytes, 1.2);
        bytes.push(0x0b);
        bytes.push(0x0b);
        push_f64(&mut bytes, 0.5);
        push_f64(&mut bytes, 0.866_025_403_784_438_6);
        push_f64(&mut bytes, 0.25);
        bytes.push(0x0a);
        for bound in [0.0, 1.0, 0.0, 1.0] {
            bytes.push(0x0a);
            push_f64(&mut bytes, bound);
        }
        push_int(&mut bytes, 0x04, 3, int_width);
        for vector in [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
        ] {
            push_vector(&mut bytes, vector);
        }
        push_f64(&mut bytes, 2.5);
        bytes.extend_from_slice(&[0x0a, 0x0b, 0x0a]);
        push_position(&mut bytes, [1.0, 2.0, 3.0]);
        push_vector(&mut bytes, [2.0, 0.0, 0.0]);
        push_vector(&mut bytes, [0.0, 2.0, 0.0]);
        push_f64(&mut bytes, 3.5);
        bytes.extend_from_slice(&[0x0b, 0x0a]);
        for value in [4.5, 5.5, 6.5] {
            push_f64(&mut bytes, value);
        }
        bytes.extend_from_slice(&[0x0a, 0x0b, 0x0a, 0x0b, 0x0a]);
        push_f64(&mut bytes, 7.5);
        push_int(&mut bytes, 0x04, 19, int_width);
        push_int(&mut bytes, 0x15, 0, int_width);
        bytes.extend_from_slice(&surface_block(int_width));
        push_f64(&mut bytes, 0.001);
        for _ in 0..6 {
            push_int(&mut bytes, 0x04, 0, int_width);
        }
        bytes.push(0x0b);
        bytes.push(0x10);

        let tokens = lex_test_span(&bytes, int_width);
        let decoded = crate::nurbs::proc_surface::procedural_surface_resolving_refs(
            &tokens,
            &test_table(&bytes, int_width),
        )
        .unwrap_or_else(|| panic!("revision deformable surface at width {int_width}"));
        assert!(
            (decoded.cache_fit_tolerance.expect("fit tolerance") - 0.01).abs()
                < f64::EPSILON * 10.0
        );
        let DecodedProceduralSurfaceDefinition::Deformable(construction) = decoded.definition
        else {
            panic!("expected deformable surface");
        };
        let Some(revision_form) = construction.revision_form else {
            panic!("expected revision form");
        };
        assert_eq!(revision_form.revision, 22_506);
        assert_eq!(revision_form.cache.selector(), 0);
        assert_eq!(revision_form.cache.fit_tolerance(), Some(0.01));
        assert_eq!(
            revision_form.support_bounds,
            [Some(0.0), Some(1.0), Some(0.0), Some(1.0)]
        );
        let crate::nurbs::proc_surface::EmbeddedDeformableSurfaceData::Resolved(data) =
            construction.data
        else {
            panic!("expected resolved revision mode-3 data")
        };
        let cadmpeg_ir::geometry::DeformableSurfaceData::RevisionMode3 {
            leading_parameter,
            trailing_point,
            trailing_vectors,
            frame_parameter,
            parameters,
            trailing_parameter,
            trailing_value,
            ..
        } = data
        else {
            panic!("expected revision mode-3 data");
        };
        assert_eq!(leading_parameter, 2.5);
        assert_eq!(trailing_point, Point3::new(10.0, 20.0, 30.0));
        assert_eq!(trailing_vectors[1], Vector3::new(0.0, 2.0, 0.0));
        assert_eq!(frame_parameter, 3.5);
        assert_eq!(parameters, [4.5, 5.5, 6.5]);
        assert_eq!(trailing_parameter, 7.5);
        assert_eq!(trailing_value, 19);
    }
}

#[test]
fn taper_surface_uses_direct_construction_cache_then_variant_tail() {
    let variants = [
        ("taper_spl_sur", 0u8),
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
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        for (name, kind) in variants {
            let mut bytes = vec![0x0f];
            push_ident(&mut bytes, name);
            push_ident(&mut bytes, "plane");
            push_position(&mut bytes, [0.0, 0.0, 0.0]);
            push_vector(&mut bytes, [0.0, 0.0, 1.0]);
            push_vector(&mut bytes, [1.0, 0.0, 0.0]);
            bytes.push(0x0b);
            bytes.extend_from_slice(&curve_block_with_endpoint(int_width, [4.0, 0.0, 0.0]));
            push_ident(&mut bytes, "nullbs");
            push_f64(&mut bytes, 0.25);
            bytes.extend_from_slice(&surface_block(int_width));
            push_f64(&mut bytes, 0.001);
            if kind == 1 {
                bytes.push(0x0a);
            }
            if kind >= 2 {
                push_vector(&mut bytes, [1.0, 2.0, 3.0]);
            }
            if matches!(kind, 3..=5) {
                push_f64(&mut bytes, 0.6);
                push_f64(&mut bytes, 0.8);
            }
            if kind == 4 {
                push_f64(&mut bytes, 1.5);
            }
            bytes.push(0x10);

            let tokens = lex_test_span(&bytes, int_width);
            let decoded = crate::nurbs::proc_surface::procedural_surface_resolving_refs(
                &tokens,
                &test_table(&bytes, int_width),
            )
            .unwrap_or_else(|| panic!("taper surface {name} at width {int_width}"));
            let fit_tolerance = decoded.cache_fit_tolerance.expect("fit tolerance");
            let DecodedProceduralSurfaceDefinition::Taper {
                reference,
                pcurve,
                parameter,
                taper,
                revision_form: None,
                ..
            } = decoded.definition
            else {
                panic!("expected legacy taper surface");
            };

            assert!((reference.control_points()[1].x - 40.0).abs() < f64::EPSILON);
            assert!(pcurve.is_none());
            assert!((parameter - 0.25).abs() < f64::EPSILON);
            assert!((fit_tolerance - 0.01).abs() < f64::EPSILON * 10.0);
            assert!(matches!(
                (kind, taper),
                (0, cadmpeg_ir::geometry::TaperSurfaceKind::Standard)
                    | (
                        1,
                        cadmpeg_ir::geometry::TaperSurfaceKind::Orthogonal { sense: true }
                    )
                    | (2, cadmpeg_ir::geometry::TaperSurfaceKind::Edge { .. })
                    | (3, cadmpeg_ir::geometry::TaperSurfaceKind::Shadow { .. })
                    | (4, cadmpeg_ir::geometry::TaperSurfaceKind::Ruled { .. })
                    | (5, cadmpeg_ir::geometry::TaperSurfaceKind::Swept { .. })
            ));
        }
    }
}

#[test]
fn taper_surface_rejects_nested_cache_substitution() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let mut bytes = vec![0x0f];
        push_ident(&mut bytes, "taper_spl_sur");
        push_ident(&mut bytes, "plane");
        push_position(&mut bytes, [0.0, 0.0, 0.0]);
        push_vector(&mut bytes, [0.0, 0.0, 1.0]);
        push_vector(&mut bytes, [1.0, 0.0, 0.0]);
        bytes.push(0x0b);
        bytes.extend_from_slice(&curve_block(int_width));
        push_ident(&mut bytes, "nullbs");
        push_f64(&mut bytes, 0.25);
        bytes.push(0x0f);
        push_ident(&mut bytes, "support");
        bytes.extend_from_slice(&surface_block(int_width));
        bytes.push(0x10);
        push_f64(&mut bytes, 0.001);
        bytes.push(0x10);

        let tokens = lex_test_span(&bytes, int_width);

        assert!(
            crate::nurbs::proc_surface::procedural_surface_resolving_refs(
                &tokens,
                &test_table(&bytes, int_width),
            )
            .is_none()
        );
    }
}

#[test]
fn compound_surface_uses_leading_cache_then_parameterized_components() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let mut bytes = vec![0x0f];
        push_ident(&mut bytes, "comp_spl_sur");
        bytes.extend_from_slice(&surface_block_with_x_offset(int_width, 5.0));
        push_f64(&mut bytes, 0.001);
        push_int(&mut bytes, 0x04, 2, int_width);
        push_f64(&mut bytes, 0.25);
        push_f64(&mut bytes, 0.75);
        for origin in [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]] {
            push_ident(&mut bytes, "plane");
            push_position(&mut bytes, origin);
            push_vector(&mut bytes, [0.0, 0.0, 1.0]);
            push_vector(&mut bytes, [1.0, 0.0, 0.0]);
            bytes.push(0x0b);
        }
        bytes.push(0x10);

        let tokens = lex_test_span(&bytes, int_width);
        let decoded = crate::nurbs::proc_surface::procedural_surface_resolving_refs(
            &tokens,
            &test_table(&bytes, int_width),
        )
        .unwrap_or_else(|| panic!("compound surface at width {int_width}"));
        let fit_tolerance = decoded.cache_fit_tolerance.expect("fit tolerance");
        let DecodedProceduralSurfaceDefinition::Compound {
            parameters,
            components,
        } = decoded.definition
        else {
            panic!("expected compound surface");
        };

        assert!((parameters[0] - 0.25).abs() < f64::EPSILON);
        assert!((parameters[1] - 0.75).abs() < f64::EPSILON);
        assert_eq!(components.len(), 2);
        assert!(matches!(
            components[0],
            SurfaceGeometry::Plane { origin, .. }
                if (origin.x - 10.0).abs() < f64::EPSILON
                    && (origin.y - 20.0).abs() < f64::EPSILON
                    && (origin.z - 30.0).abs() < f64::EPSILON
        ));
        assert!(matches!(
            components[1],
            SurfaceGeometry::Plane { origin, .. }
                if (origin.x - 40.0).abs() < f64::EPSILON
                    && (origin.y - 50.0).abs() < f64::EPSILON
                    && (origin.z - 60.0).abs() < f64::EPSILON
        ));
        assert!((fit_tolerance - 0.01).abs() < f64::EPSILON * 10.0);
    }
}

#[test]
fn compound_surface_rejects_nonleading_cache_and_trailing_fields() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        for malformed in [0u8, 1] {
            let mut bytes = vec![0x0f];
            push_ident(&mut bytes, "comp_spl_sur");
            if malformed == 0 {
                bytes.push(0x0f);
                push_ident(&mut bytes, "support");
            }
            bytes.extend_from_slice(&surface_block(int_width));
            if malformed == 0 {
                bytes.push(0x10);
            }
            push_f64(&mut bytes, 0.001);
            push_int(&mut bytes, 0x04, 0, int_width);
            if malformed == 1 {
                bytes.push(0x0b);
            }
            bytes.push(0x10);

            let tokens = lex_test_span(&bytes, int_width);

            assert!(
                crate::nurbs::proc_surface::procedural_surface_resolving_refs(
                    &tokens,
                    &test_table(&bytes, int_width),
                )
                .is_none()
            );
        }
    }
}

#[test]
fn loft_surface_walks_bridge_to_direct_cache() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        for name in ["loft_spl_sur", "loftsur"] {
            let mut bytes = vec![0x0f];
            push_ident(&mut bytes, name);
            push_int(&mut bytes, 0x04, 0, int_width);
            push_int(&mut bytes, 0x04, 0, int_width);
            for value in [-1.0, 2.0, -3.0, 4.0] {
                push_f64(&mut bytes, value);
            }
            for value in [1, 2, 3, 4] {
                push_int(&mut bytes, 0x15, value, int_width);
            }
            push_int(&mut bytes, 0x04, 7, int_width);
            bytes.push(0x0a);
            push_int(&mut bytes, 0x04, 11, int_width);
            push_f64(&mut bytes, 0.25);
            push_int(&mut bytes, 0x15, 12, int_width);
            push_string(&mut bytes, "bridge");
            bytes.extend_from_slice(&surface_block(int_width));
            push_f64(&mut bytes, 0.001);
            bytes.push(0x10);

            let tokens = lex_test_span(&bytes, int_width);
            let decoded = crate::nurbs::proc_surface::procedural_surface_resolving_refs(
                &tokens,
                &test_table(&bytes, int_width),
            )
            .unwrap_or_else(|| panic!("loft surface {name} at width {int_width}"));
            let fit_tolerance = decoded.cache_fit_tolerance.expect("fit tolerance");
            let DecodedProceduralSurfaceDefinition::Loft(loft) = decoded.definition else {
                panic!("expected legacy loft surface");
            };

            assert!(loft.sections.iter().all(Vec::is_empty));
            assert_eq!(loft.closures, [1, 2]);
            assert_eq!(loft.singularities, [3, 4]);
            assert_eq!(loft.mode, 7);
            assert_eq!(loft.bridge.len(), 5);
            assert!(matches!(
                loft.bridge.as_slice(),
                [
                    cadmpeg_ir::geometry::LoftBridgeToken::Boolean(true),
                    cadmpeg_ir::geometry::LoftBridgeToken::Integer(11),
                    cadmpeg_ir::geometry::LoftBridgeToken::Double(value),
                    cadmpeg_ir::geometry::LoftBridgeToken::Enum(12),
                    cadmpeg_ir::geometry::LoftBridgeToken::Text(text),
                ] if (*value - 0.25).abs() < f64::EPSILON && text == "bridge"
            ));
            assert!((fit_tolerance - 0.01).abs() < f64::EPSILON * 10.0);
        }
    }
}

#[test]
fn loft_surface_rejects_nested_cache_substitution() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let mut bytes = vec![0x0f];
        push_ident(&mut bytes, "loftsur");
        push_int(&mut bytes, 0x04, 0, int_width);
        push_int(&mut bytes, 0x04, 0, int_width);
        for value in [-1.0, 2.0, -3.0, 4.0] {
            push_f64(&mut bytes, value);
        }
        for value in [0, 0, 0, 0] {
            push_int(&mut bytes, 0x15, value, int_width);
        }
        push_int(&mut bytes, 0x04, 0, int_width);
        bytes.push(0x0f);
        push_ident(&mut bytes, "support");
        bytes.extend_from_slice(&surface_block(int_width));
        bytes.push(0x10);
        push_f64(&mut bytes, 0.001);
        bytes.push(0x10);

        let tokens = lex_test_span(&bytes, int_width);

        assert!(
            crate::nurbs::proc_surface::procedural_surface_resolving_refs(
                &tokens,
                &test_table(&bytes, int_width),
            )
            .is_none()
        );
    }
}

#[test]
fn exact_surface_uses_leading_cache_ranges_then_extension() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        for name in ["exact_spl_sur", "exactsur"] {
            let mut bytes = vec![0x0f];
            push_ident(&mut bytes, name);
            bytes.extend_from_slice(&surface_block(int_width));
            push_f64(&mut bytes, 0.001);
            for value in [-1.0, 2.0, -3.0, 4.0] {
                push_f64(&mut bytes, value);
            }
            push_int(&mut bytes, 0x04, 9, int_width);
            bytes.push(0x10);

            let tokens = lex_test_span(&bytes, int_width);
            let decoded = crate::nurbs::proc_surface::procedural_surface_resolving_refs(
                &tokens,
                &test_table(&bytes, int_width),
            )
            .unwrap_or_else(|| panic!("exact surface {name} at width {int_width}"));
            let fit_tolerance = decoded.cache_fit_tolerance.expect("fit tolerance");
            let DecodedProceduralSurfaceDefinition::Exact {
                spline: cadmpeg_ir::geometry::ExactSpline::Legacy { ranges, extension },
            } = decoded.definition
            else {
                panic!("expected legacy exact surface");
            };

            assert!((ranges[0][0] - -1.0).abs() < f64::EPSILON);
            assert!((ranges[0][1] - 2.0).abs() < f64::EPSILON);
            assert!((ranges[1][0] - -3.0).abs() < f64::EPSILON);
            assert!((ranges[1][1] - 4.0).abs() < f64::EPSILON);
            assert_eq!(extension, 9);
            assert!((fit_tolerance - 0.01).abs() < f64::EPSILON * 10.0);
        }
    }
}

#[test]
fn exact_surface_rejects_nonleading_cache_and_trailing_fields() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        for malformed in [0u8, 1] {
            let mut bytes = vec![0x0f];
            push_ident(&mut bytes, "exactsur");
            if malformed == 0 {
                bytes.push(0x0f);
                push_ident(&mut bytes, "support");
            }
            bytes.extend_from_slice(&surface_block(int_width));
            if malformed == 0 {
                bytes.push(0x10);
            }
            push_f64(&mut bytes, 0.001);
            for value in [-1.0, 2.0, -3.0, 4.0] {
                push_f64(&mut bytes, value);
            }
            push_int(&mut bytes, 0x04, 9, int_width);
            if malformed == 1 {
                bytes.push(0x0b);
            }
            bytes.push(0x10);

            let tokens = lex_test_span(&bytes, int_width);

            assert!(
                crate::nurbs::proc_surface::procedural_surface_resolving_refs(
                    &tokens,
                    &test_table(&bytes, int_width),
                )
                .is_none()
            );
        }
    }
}

#[test]
fn ruled_surface_uses_two_direct_profiles_then_cache() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let mut bytes = vec![0x0f];
        push_ident(&mut bytes, "rule_sur");
        bytes.extend_from_slice(&curve_block_with_endpoint(int_width, [1.0, 0.0, 0.0]));
        bytes.extend_from_slice(&curve_block_with_endpoint(int_width, [4.0, 0.0, 0.0]));
        bytes.extend_from_slice(&surface_block(int_width));
        push_f64(&mut bytes, 0.001);
        bytes.push(0x10);

        let tokens = lex_test_span(&bytes, int_width);
        let decoded = crate::nurbs::proc_surface::procedural_surface_resolving_refs(
            &tokens,
            &test_table(&bytes, int_width),
        )
        .unwrap_or_else(|| panic!("ruled surface at width {int_width}"));
        let DecodedProceduralSurfaceDefinition::Ruled { first, second } = decoded.definition else {
            panic!("expected ruled surface");
        };

        assert!((first.control_points()[1].x - 10.0).abs() < f64::EPSILON);
        assert!((second.control_points()[1].x - 40.0).abs() < f64::EPSILON);
        assert!(
            (decoded.cache_fit_tolerance.expect("fit tolerance") - 0.01).abs()
                < f64::EPSILON * 10.0
        );
    }
}

#[test]
fn ruled_surface_rejects_nested_profile_substitution() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let mut bytes = vec![0x0f];
        push_ident(&mut bytes, "rule_sur");
        bytes.push(0x0f);
        push_ident(&mut bytes, "support");
        bytes.extend_from_slice(&curve_block(int_width));
        bytes.push(0x10);
        bytes.extend_from_slice(&curve_block(int_width));
        bytes.extend_from_slice(&curve_block(int_width));
        bytes.extend_from_slice(&surface_block(int_width));
        push_f64(&mut bytes, 0.001);
        bytes.push(0x10);

        let tokens = lex_test_span(&bytes, int_width);

        assert!(
            crate::nurbs::proc_surface::procedural_surface_resolving_refs(
                &tokens,
                &test_table(&bytes, int_width),
            )
            .is_none()
        );
    }
}

#[test]
fn sum_surface_uses_two_direct_curves_origin_then_cache() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let mut bytes = vec![0x0f];
        push_ident(&mut bytes, "sum_spl_sur");
        bytes.extend_from_slice(&curve_block_with_endpoint(int_width, [1.0, 0.0, 0.0]));
        bytes.extend_from_slice(&curve_block_with_endpoint(int_width, [4.0, 0.0, 0.0]));
        push_position(&mut bytes, [0.5, 1.0, 1.5]);
        bytes.extend_from_slice(&surface_block(int_width));
        push_f64(&mut bytes, 0.001);
        bytes.push(0x10);

        let tokens = lex_test_span(&bytes, int_width);
        let decoded = crate::nurbs::proc_surface::procedural_surface_resolving_refs(
            &tokens,
            &test_table(&bytes, int_width),
        )
        .unwrap_or_else(|| panic!("sum surface at width {int_width}"));
        let fit_tolerance = decoded.cache_fit_tolerance.expect("fit tolerance");
        let DecodedProceduralSurfaceDefinition::Sum {
            first,
            second,
            basepoint,
            revision_form: None,
        } = decoded.definition
        else {
            panic!("expected legacy sum surface");
        };
        let CurveGeometry::Nurbs(first) = first else {
            panic!("expected first NURBS curve");
        };
        let CurveGeometry::Nurbs(second) = second else {
            panic!("expected second NURBS curve");
        };

        assert!((first.control_points()[1].x - 10.0).abs() < f64::EPSILON);
        assert!((second.control_points()[1].x - 40.0).abs() < f64::EPSILON);
        assert!((basepoint.x - 5.0).abs() < f64::EPSILON);
        assert!((basepoint.y - 10.0).abs() < f64::EPSILON);
        assert!((basepoint.z - 15.0).abs() < f64::EPSILON);
        assert!((fit_tolerance - 0.01).abs() < f64::EPSILON * 10.0);
    }
}

#[test]
fn sum_surface_rejects_nested_curve_substitution() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let mut bytes = vec![0x0f];
        push_ident(&mut bytes, "sum_spl_sur");
        bytes.push(0x0f);
        push_ident(&mut bytes, "support");
        bytes.extend_from_slice(&curve_block(int_width));
        bytes.push(0x10);
        bytes.extend_from_slice(&curve_block(int_width));
        bytes.extend_from_slice(&curve_block(int_width));
        push_position(&mut bytes, [0.0, 0.0, 0.0]);
        bytes.extend_from_slice(&surface_block(int_width));
        push_f64(&mut bytes, 0.001);
        bytes.push(0x10);

        let tokens = lex_test_span(&bytes, int_width);

        assert!(
            crate::nurbs::proc_surface::procedural_surface_resolving_refs(
                &tokens,
                &test_table(&bytes, int_width),
            )
            .is_none()
        );
    }
}

#[test]
fn revolution_surface_uses_direct_profile_axis_then_cache() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let mut bytes = vec![0x0f];
        push_ident(&mut bytes, "rot_spl_sur");
        bytes.extend_from_slice(&curve_block_with_endpoint(int_width, [4.0, 0.0, 0.0]));
        push_position(&mut bytes, [0.5, 1.0, 1.5]);
        push_vector(&mut bytes, [0.0, 0.0, 2.0]);
        bytes.extend_from_slice(&surface_block(int_width));
        push_f64(&mut bytes, 0.001);
        bytes.push(0x10);

        let tokens = lex_test_span(&bytes, int_width);
        let decoded = crate::nurbs::proc_surface::procedural_surface_resolving_refs(
            &tokens,
            &test_table(&bytes, int_width),
        )
        .unwrap_or_else(|| panic!("revolution surface at width {int_width}"));
        let fit_tolerance = decoded.cache_fit_tolerance.expect("fit tolerance");
        let DecodedProceduralSurfaceDefinition::Revolution {
            directrix,
            axis_origin,
            axis_direction,
            angular_interval,
            parameter_interval,
            revision_form: None,
        } = decoded.definition
        else {
            panic!("expected legacy revolution surface");
        };
        let CurveGeometry::Nurbs(directrix) = directrix else {
            panic!("expected NURBS profile");
        };

        assert!((directrix.control_points()[1].x - 40.0).abs() < f64::EPSILON);
        assert!((axis_origin.x - 5.0).abs() < f64::EPSILON);
        assert!((axis_origin.y - 10.0).abs() < f64::EPSILON);
        assert!((axis_origin.z - 15.0).abs() < f64::EPSILON);
        assert!((axis_direction.x - 0.0).abs() < f64::EPSILON);
        assert!((axis_direction.y - 0.0).abs() < f64::EPSILON);
        assert!((axis_direction.z - 1.0).abs() < f64::EPSILON);
        assert!((angular_interval[0] - 0.0).abs() < f64::EPSILON);
        assert!((angular_interval[1] - 1.0).abs() < f64::EPSILON);
        assert!((parameter_interval[0] - 0.0).abs() < f64::EPSILON);
        assert!((parameter_interval[1] - 1.0).abs() < f64::EPSILON);
        assert!((fit_tolerance - 0.01).abs() < f64::EPSILON * 10.0);
    }
}

#[test]
fn revolution_surface_rejects_nested_profile_substitution() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let mut bytes = vec![0x0f];
        push_ident(&mut bytes, "rot_spl_sur");
        bytes.push(0x0f);
        push_ident(&mut bytes, "support");
        bytes.extend_from_slice(&curve_block(int_width));
        bytes.push(0x10);
        bytes.extend_from_slice(&curve_block(int_width));
        push_position(&mut bytes, [0.0, 0.0, 0.0]);
        push_vector(&mut bytes, [0.0, 0.0, 1.0]);
        bytes.extend_from_slice(&surface_block(int_width));
        push_f64(&mut bytes, 0.001);
        bytes.push(0x10);

        let tokens = lex_test_span(&bytes, int_width);

        assert!(
            crate::nurbs::proc_surface::procedural_surface_resolving_refs(
                &tokens,
                &test_table(&bytes, int_width),
            )
            .is_none()
        );
    }
}

#[test]
fn revision_revolution_uses_the_shared_tails_solved_cache_domain() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let mut bytes = vec![0x0f];
        push_ident(&mut bytes, "rot_spl_sur");
        push_int(&mut bytes, 0x04, 23_100, int_width);
        bytes.extend_from_slice(&curve_block_with_endpoint(int_width, [4.0, 0.0, 0.0]));
        bytes.extend_from_slice(&[0x0b, 0x0b]);
        push_position(&mut bytes, [0.5, 1.0, 1.5]);
        push_vector(&mut bytes, [0.0, 0.0, 2.0]);
        push_int(&mut bytes, 0x15, 0, int_width);
        bytes.extend_from_slice(&surface_block(int_width));
        push_f64(&mut bytes, 0.001);
        for _ in 0..6 {
            push_int(&mut bytes, 0x04, 0, int_width);
        }
        bytes.push(0x0b);
        bytes.push(0x10);

        let tokens = lex_test_span(&bytes, int_width);
        let decoded = crate::nurbs::proc_surface::procedural_surface_resolving_refs(
            &tokens,
            &test_table(&bytes, int_width),
        )
        .unwrap_or_else(|| panic!("revision revolution surface at width {int_width}"));
        let DecodedProceduralSurfaceDefinition::Revolution {
            angular_interval,
            parameter_interval,
            revision_form: Some(_),
            ..
        } = decoded.definition
        else {
            panic!("expected revision revolution surface");
        };

        assert!((angular_interval[0] - 0.0).abs() < f64::EPSILON);
        assert!((angular_interval[1] - 1.0).abs() < f64::EPSILON);
        assert!((parameter_interval[0] - 0.0).abs() < f64::EPSILON);
        assert!((parameter_interval[1] - 1.0).abs() < f64::EPSILON);
    }
}

#[test]
fn surface_cache_resolves_width4_subtype_ref() {
    // Active slice: one named subtype span holding the surface cache.
    let mut active = vec![0x0f, 0x0d, 0x07];
    active.extend_from_slice(b"spl_sur");
    active.extend_from_slice(&surface_block(RefWidth::Four));
    active.push(0x10);
    // Record: `ref 0` into the subtype table, 4-byte index payload.
    let mut record = vec![0x0f, 0x0d, 0x03];
    record.extend_from_slice(b"ref");
    push_int(&mut record, 0x04, 0, RefWidth::Four);
    record.push(0x10);
    let surface = crate::nurbs::core::surface_cache_resolving_refs(
        &crate::nurbs::toks::lex_test_span(&record, RefWidth::Four),
        &crate::nurbs::toks::test_table(&active, RefWidth::Four),
    )
    .expect("resolved width-4 ref");
    assert_eq!((surface.u_count(), surface.v_count()), (2, 2));
}

#[test]
fn surface_cache_resolves_compact_subtype_refs_at_both_widths() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let mut active = vec![0x0f];
        push_ident(&mut active, "spl_sur");
        active.extend_from_slice(&surface_block(int_width));
        active.push(0x10);
        let mut record = vec![0x0f];
        push_int(&mut record, 0x04, 0, int_width);
        record.push(0x10);
        let surface = crate::nurbs::core::surface_cache_resolving_refs(
            &crate::nurbs::toks::lex_test_span(&record, int_width),
            &crate::nurbs::toks::test_table(&active, int_width),
        )
        .unwrap_or_else(|| panic!("compact subtype ref at width {int_width}"));
        assert_eq!((surface.u_count(), surface.v_count()), (2, 2));
    }
}

#[test]
fn spring_layout_walks_both_integer_widths() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let mut bytes = vec![0x0f, 0x0d, 0x0e];
        bytes.extend_from_slice(b"spring_int_cur");
        for _ in 0..2 {
            bytes.extend_from_slice(&[0x0d, 0x0c]);
            bytes.extend_from_slice(b"null_surface");
            for value in [0.0, 1.0, 2.0, 3.0] {
                push_f64(&mut bytes, value);
            }
        }
        bytes.extend_from_slice(&[0x0d, 0x06]);
        bytes.extend_from_slice(b"nullbs");
        push_f64(&mut bytes, -1.0);
        push_f64(&mut bytes, 1.0);
        bytes.extend_from_slice(&[0x0d, 0x06]);
        bytes.extend_from_slice(b"nullbs");
        push_f64(&mut bytes, -2.0);
        push_f64(&mut bytes, 2.0);
        for values in [&[0.25][..], &[][..], &[0.5, 0.75][..]] {
            push_int(&mut bytes, 0x04, values.len() as i64, int_width);
            for value in values {
                push_f64(&mut bytes, *value);
            }
        }
        bytes.push(0x0a);
        let direction = bytes.len();
        push_int(&mut bytes, 0x15, -3, int_width);

        let layout = spring_patch_layout(&bytes, int_width)
            .unwrap_or_else(|| panic!("spring layout at width {int_width}"));
        assert_eq!(layout.direction, direction);
        assert_eq!(
            layout.discontinuities.iter().map(Vec::len).sum::<usize>(),
            3
        );
        assert_eq!(layout.discontinuity_flag + 1, layout.direction);
    }
}

#[test]
fn three_surface_layout_walks_both_integer_widths() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let mut bytes = vec![0x0f, 0x0d, 0x0b];
        bytes.extend_from_slice(b"sss_int_cur");
        for _ in 0..2 {
            bytes.extend_from_slice(&[0x0d, 0x06]);
            bytes.extend_from_slice(b"spline");
            bytes.extend_from_slice(&surface_block(int_width));
        }
        bytes.extend_from_slice(&pcurve_block(int_width));
        bytes.extend_from_slice(&pcurve_block(int_width));
        push_f64(&mut bytes, -2.0);
        push_f64(&mut bytes, 3.0);
        for values in [&[0.25][..], &[][..], &[0.5, 0.75][..]] {
            push_int(&mut bytes, 0x04, values.len() as i64, int_width);
            for value in values {
                push_f64(&mut bytes, *value);
            }
        }
        let selector = bytes.len();
        push_int(&mut bytes, 0x04, 7, int_width);
        bytes.extend_from_slice(&[0x0d, 0x06]);
        bytes.extend_from_slice(b"spline");
        bytes.extend_from_slice(&surface_block(int_width));
        bytes.extend_from_slice(&pcurve_block(int_width));

        let layout = three_surface_patch_layout(&bytes, int_width)
            .unwrap_or_else(|| panic!("three-surface layout at width {int_width}"));
        assert_eq!(layout.selector, selector);
        assert_eq!(
            layout.discontinuities.iter().map(Vec::len).sum::<usize>(),
            3
        );
    }
}

#[test]
fn surface_curve_layout_walks_each_family_at_both_widths() {
    use cadmpeg_ir::geometry::SurfaceCurveFamilyKind;
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        for (name, family) in [
            ("blend_int_cur", SurfaceCurveFamilyKind::Blend),
            ("surf_int_cur", SurfaceCurveFamilyKind::SurfaceConstrained),
            ("par_int_cur", SurfaceCurveFamilyKind::Parametric),
            ("skin_int_cur", SurfaceCurveFamilyKind::Skin),
        ] {
            let mut bytes = vec![0x0f, 0x0d, name.len() as u8];
            bytes.extend_from_slice(name.as_bytes());
            for _ in 0..2 {
                bytes.extend_from_slice(&[0x0d, 0x06]);
                bytes.extend_from_slice(b"spline");
                bytes.extend_from_slice(&surface_block(int_width));
            }
            bytes.extend_from_slice(&pcurve_block(int_width));
            bytes.extend_from_slice(&pcurve_block(int_width));
            push_f64(&mut bytes, -2.0);
            push_f64(&mut bytes, 3.0);
            for values in [&[0.25][..], &[][..], &[0.5, 0.75][..]] {
                push_int(&mut bytes, 0x04, values.len() as i64, int_width);
                for value in values {
                    push_f64(&mut bytes, *value);
                }
            }

            let layout = surface_curve_patch_layout(&bytes, int_width, family)
                .unwrap_or_else(|| panic!("{name} layout at width {int_width}"));
            assert_eq!(
                layout.discontinuities.iter().map(Vec::len).sum::<usize>(),
                3
            );
        }
    }
}

#[test]
fn intersection_layout_walks_modern_and_legacy_names_at_both_widths() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        for name in ["int_int_cur", "surf_surf_int_cur", "surfintcur"] {
            let mut bytes = vec![0x0f, 0x0d, name.len() as u8];
            bytes.extend_from_slice(name.as_bytes());
            for _ in 0..2 {
                bytes.extend_from_slice(&[0x0d, 0x06]);
                bytes.extend_from_slice(b"spline");
                bytes.extend_from_slice(&surface_block(int_width));
            }
            bytes.extend_from_slice(&pcurve_block(int_width));
            bytes.extend_from_slice(&pcurve_block(int_width));
            push_f64(&mut bytes, -2.0);
            push_f64(&mut bytes, 3.0);
            for values in [&[0.25][..], &[][..], &[0.5, 0.75][..]] {
                push_int(&mut bytes, 0x04, values.len() as i64, int_width);
                for value in values {
                    push_f64(&mut bytes, *value);
                }
            }
            let flag = bytes.len();
            bytes.push(0x0a);

            let layout = intersection_patch_layout(&bytes, int_width)
                .unwrap_or_else(|| panic!("{name} layout at width {int_width}"));
            assert_eq!(layout.discontinuity_flag, flag);
            assert_eq!(
                layout.discontinuities.iter().map(Vec::len).sum::<usize>(),
                3
            );
        }
    }
}

#[test]
fn cache_first_intersection_resolves_support_ref_and_nullable_pcurve() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let mut support = vec![0x0f];
        push_ident(&mut support, "intersection_support");
        support.extend_from_slice(&surface_block(int_width));
        support.push(0x10);

        let mut record = vec![0x0f];
        push_ident(&mut record, "int_int_cur");
        push_int(&mut record, 0x04, 22_507, int_width);
        push_int(&mut record, 0x15, 0, int_width);
        record.extend_from_slice(&curve_block(int_width));
        push_f64(&mut record, 1.0e-6);
        push_ident(&mut record, "plane");
        push_position(&mut record, [0.0, 0.0, 0.0]);
        push_vector(&mut record, [0.0, 0.0, 1.0]);
        push_vector(&mut record, [1.0, 0.0, 0.0]);
        record.push(0x0b);
        record.extend_from_slice(&[0x0b; 4]);
        push_ident(&mut record, "spline");
        record.push(0x0b);
        record.push(0x0f);
        push_ident(&mut record, "ref");
        push_int(&mut record, 0x04, 0, int_width);
        record.push(0x10);
        for value in [-2.0, 2.0, -3.0, 3.0] {
            record.push(0x0a);
            push_f64(&mut record, value);
        }
        push_ident(&mut record, "nullbs");
        record.extend_from_slice(&pcurve_block(int_width));
        record.extend_from_slice(&[0x0b, 0x0b]);
        for _ in 0..4 {
            push_int(&mut record, 0x04, 0, int_width);
        }
        record.push(0x10);

        let mut active = support;
        active.extend_from_slice(&record);
        let decoded = crate::nurbs::proc_curve::procedural_curve_resolving_refs(
            &lex_test_span(&record, int_width),
            &test_table(&active, int_width),
        )
        .unwrap_or_else(|| panic!("cache-first intersection at width {int_width}"));
        let (context, flag) = decoded
            .embedded_intersection
            .expect("typed intersection context");
        assert!(!flag);
        assert_eq!(context.parameter_range, [0.0, 1.0]);
        assert!(matches!(
            context.surfaces[0],
            crate::nurbs::proc_curve::SupportSlot::Surface(SurfaceGeometry::Plane { .. })
        ));
        assert!(matches!(
            context.surfaces[1],
            crate::nurbs::proc_curve::SupportSlot::Surface(SurfaceGeometry::Nurbs(_))
        ));
        assert!(context.pcurves[0].is_none());
        assert!(context.pcurves[1].is_some());
        assert!(context.discontinuities.iter().all(Vec::is_empty));
    }
}

#[test]
fn intersection_selector_keeps_pcurve_for_cacheless_surface_support_in_both_forms() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let mut support = vec![0x0f];
        push_ident(&mut support, "helix_spl_line");
        push_int(&mut support, 0x04, 23_100, int_width);
        for value in [-0.5, 0.5, -2.0, 3.0, 0.0, std::f64::consts::TAU] {
            push_f64(&mut support, value);
        }
        push_position(&mut support, [1.0, 2.0, 3.0]);
        push_vector(&mut support, [2.0, 0.0, 0.0]);
        push_vector(&mut support, [0.0, 2.0, 0.0]);
        push_vector(&mut support, [0.0, 0.0, 4.0]);
        push_f64(&mut support, 0.25);
        push_vector(&mut support, [0.0, 0.0, 1.0]);
        for sentinel in ["null_surface", "null_surface", "nullbs", "nullbs"] {
            push_ident(&mut support, sentinel);
        }
        push_vector(&mut support, [5.0, 6.0, 7.0]);
        support.push(0x10);

        for inline in [false, true] {
            let form = if inline { "inline" } else { "reference" };
            let mut record = vec![0x0f];
            push_ident(&mut record, "int_int_cur");
            push_int(&mut record, 0x04, 22_507, int_width);
            push_int(&mut record, 0x15, 0, int_width);
            record.extend_from_slice(&curve_block(int_width));
            push_f64(&mut record, 1.0e-6);
            push_ident(&mut record, "spline");
            record.push(0x0b);
            if inline {
                record.extend_from_slice(&support);
            } else {
                record.extend_from_slice(&[0x0f]);
                push_ident(&mut record, "ref");
                push_int(&mut record, 0x04, 0, int_width);
                record.push(0x10);
            }
            record.extend_from_slice(&[0x0b; 4]);
            push_ident(&mut record, "null_surface");
            record.extend_from_slice(&pcurve_block(int_width));
            push_ident(&mut record, "nullbs");
            record.extend_from_slice(&[0x0b, 0x0b]);
            for _ in 0..4 {
                push_int(&mut record, 0x04, 0, int_width);
            }
            record.push(0x10);

            let mut active = support.clone();
            active.extend_from_slice(&record);
            let toks = lex_test_span(&record, int_width);
            let table = test_table(&active, int_width);
            let decoded = crate::nurbs::proc_curve::procedural_curve_resolving_refs(&toks, &table)
                .unwrap_or_else(|| panic!("cacheless support intersection at {int_width}: {form}"));
            let context = decoded
                .embedded_intersection
                .as_ref()
                .map(|(context, _)| context)
                .expect("typed intersection context");
            assert!(matches!(
                context.surfaces,
                [
                    crate::nurbs::proc_curve::SupportSlot::DeclaredOnly,
                    crate::nurbs::proc_curve::SupportSlot::Absent
                ]
            ));
            assert!(context.pcurves[0].is_some());
            assert!(
                crate::nurbs::proc_curve::pcurve_for_selector_resolving_refs(&toks, 1, &table)
                    .is_some()
            );
            assert!(
                crate::nurbs::proc_curve::pcurve_for_selector_resolving_refs(&toks, 2, &table)
                    .is_none()
            );
        }
    }
}

#[test]
fn cache_first_blend_curve_retains_nullable_supports_and_tail() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let mut support = vec![0x0f];
        push_ident(&mut support, "blend_support");
        support.extend_from_slice(&surface_block(int_width));
        support.push(0x10);

        let mut record = vec![0x0f];
        push_ident(&mut record, "blend_int_cur");
        push_int(&mut record, 0x04, 22_507, int_width);
        push_int(&mut record, 0x15, 0, int_width);
        record.extend_from_slice(&curve_block(int_width));
        push_f64(&mut record, 1.0e-6);
        push_ident(&mut record, "spline");
        record.push(0x0b);
        record.push(0x0f);
        push_ident(&mut record, "ref");
        push_int(&mut record, 0x04, 0, int_width);
        record.push(0x10);
        record.extend_from_slice(&[0x0b; 4]);
        push_ident(&mut record, "null_surface");
        record.extend_from_slice(&pcurve_block(int_width));
        push_ident(&mut record, "nullbs");
        record.extend_from_slice(&[0x0b, 0x0b]);
        for _ in 0..3 {
            push_int(&mut record, 0x04, 0, int_width);
        }
        push_int(&mut record, 0x04, 7, int_width);
        record.push(0x0a);
        record.push(0x10);

        let mut active = support;
        active.extend_from_slice(&record);
        let decoded = crate::nurbs::proc_curve::procedural_curve_resolving_refs(
            &lex_test_span(&record, int_width),
            &test_table(&active, int_width),
        )
        .unwrap_or_else(|| panic!("cache-first blend curve at width {int_width}"));
        let EmbeddedSurfaceCurve::Blend {
            context,
            tail: Some(tail),
        } = decoded.embedded_surface_curve.expect("typed blend context")
        else {
            panic!("blend surface-curve family")
        };
        assert_eq!(context.parameter_range, [0.0, 1.0]);
        assert!(matches!(
            context.surfaces[0],
            crate::nurbs::proc_curve::SupportSlot::Surface(SurfaceGeometry::Nurbs(_))
        ));
        assert!(matches!(
            context.surfaces[1],
            crate::nurbs::proc_curve::SupportSlot::Absent
                | crate::nurbs::proc_curve::SupportSlot::DeclaredOnly
        ));
        assert!(context.pcurves[0].is_some());
        assert!(context.pcurves[1].is_none());
        assert_eq!(tail.tail.extension, 7);
        assert!(tail.flags);
    }
}

#[test]
fn cache_first_par_curve_selects_mirrored_support_slot() {
    // `flag1 = F` mirrors the support onto the second serialized slot:
    // surface slot 1 and pcurve slot 1 are null, while slot 2 carries the
    // parametric support surface and its bs2 pcurve. `par_int_cur`
    // terminates on two booleans, so `flag2` is read after `flag1`.
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let mut support = vec![0x0f];
        push_ident(&mut support, "par_support");
        support.extend_from_slice(&surface_block(int_width));
        support.push(0x10);

        let mut record = vec![0x0f];
        push_ident(&mut record, "par_int_cur");
        push_int(&mut record, 0x04, 22_507, int_width);
        push_int(&mut record, 0x15, 0, int_width);
        record.extend_from_slice(&curve_block(int_width));
        push_f64(&mut record, 1.0e-6);
        // Surface slot 1: null; slot 2: reference into the support table.
        push_ident(&mut record, "null_surface");
        push_ident(&mut record, "spline");
        record.push(0x0b);
        record.push(0x0f);
        push_ident(&mut record, "ref");
        push_int(&mut record, 0x04, 0, int_width);
        record.push(0x10);
        record.extend_from_slice(&[0x0b; 4]);
        // Pcurve slot 1: null; slot 2: populated bs2 pcurve.
        push_ident(&mut record, "nullbs");
        record.extend_from_slice(&pcurve_block(int_width));
        record.extend_from_slice(&[0x0b, 0x0b]);
        for _ in 0..3 {
            push_int(&mut record, 0x04, 0, int_width);
        }
        push_int(&mut record, 0x04, 7, int_width);
        record.push(0x0b);
        record.push(0x0b);
        record.push(0x10);

        let mut active = support;
        active.extend_from_slice(&record);
        let decoded = crate::nurbs::proc_curve::procedural_curve_resolving_refs(
            &lex_test_span(&record, int_width),
            &test_table(&active, int_width),
        )
        .unwrap_or_else(|| panic!("cache-first par curve at width {int_width}"));
        let EmbeddedSurfaceCurve::Parametric {
            context,
            tail: Some(tail),
        } = decoded.embedded_surface_curve.expect("typed par context")
        else {
            panic!("parametric surface-curve family")
        };
        assert!(matches!(
            context.surfaces[0],
            crate::nurbs::proc_curve::SupportSlot::Absent
                | crate::nurbs::proc_curve::SupportSlot::DeclaredOnly
        ));
        assert!(matches!(
            context.surfaces[1],
            crate::nurbs::proc_curve::SupportSlot::Surface(SurfaceGeometry::Nurbs(_))
        ));
        assert!(context.pcurves[0].is_none());
        assert!(context.pcurves[1].is_some());
        assert_eq!(tail.tail.extension, 7);
        assert!(!tail.flags.flag);
        assert_eq!(tail.flags.second_flag, Some(false));
    }
}

#[test]
fn projection_layout_walks_both_tail_forms_at_both_widths() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        for early_close in [false, true] {
            let mut bytes = vec![0x0f, 0x0d, 0x0c];
            bytes.extend_from_slice(b"proj_int_cur");
            for _ in 0..2 {
                bytes.extend_from_slice(&[0x0d, 0x06]);
                bytes.extend_from_slice(b"spline");
                bytes.extend_from_slice(&surface_block(int_width));
            }
            bytes.extend_from_slice(&pcurve_block(int_width));
            bytes.extend_from_slice(&pcurve_block(int_width));
            push_f64(&mut bytes, -2.0);
            push_f64(&mut bytes, 3.0);
            for values in [&[0.25][..], &[][..], &[0.5, 0.75][..]] {
                push_int(&mut bytes, 0x04, values.len() as i64, int_width);
                for value in values {
                    push_f64(&mut bytes, *value);
                }
            }
            let context_flag = bytes.len();
            bytes.push(0x0a);
            bytes.extend_from_slice(&curve_block(int_width));
            let tail_flag = bytes.len();
            bytes.push(0x0b);
            if early_close {
                bytes.push(0x10);
            } else {
                push_f64(&mut bytes, -1.0);
                push_f64(&mut bytes, 1.0);
                bytes.extend_from_slice(&[0x07, 0x05]);
                bytes.extend_from_slice(b"surf1");
            }

            let layout = projection_patch_layout(&bytes, int_width)
                .unwrap_or_else(|| panic!("projection layout at width {int_width}"));
            assert_eq!(layout.discontinuity_flag, context_flag);
            match layout.tail {
                ProjectionTailPatchLayout::EarlyClose { flag } => {
                    assert!(early_close);
                    assert_eq!(flag, tail_flag);
                }
                ProjectionTailPatchLayout::Ranged { flag, role, .. } => {
                    assert!(!early_close);
                    assert_eq!(flag, tail_flag);
                    assert_eq!(&bytes[role], b"surf1");
                }
            }
        }
    }
}

#[test]
fn silhouette_layout_walks_each_family_at_both_widths() {
    use cadmpeg_ir::geometry::SilhouetteKind;
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        for (name, kind) in [
            ("silh_int_cur", SilhouetteKind::Standard),
            ("para_silh_int_cur", SilhouetteKind::Parametric),
            (
                "taper_silh_int_cur",
                SilhouetteKind::Taper { draft_factor: 0.5 },
            ),
        ] {
            let mut bytes = vec![0x0f, 0x0d, name.len() as u8];
            bytes.extend_from_slice(name.as_bytes());
            for _ in 0..2 {
                bytes.extend_from_slice(&[0x0d, 0x06]);
                bytes.extend_from_slice(b"spline");
                bytes.extend_from_slice(&surface_block(int_width));
            }
            bytes.extend_from_slice(&pcurve_block(int_width));
            bytes.extend_from_slice(&pcurve_block(int_width));
            push_f64(&mut bytes, -2.0);
            push_f64(&mut bytes, 3.0);
            for values in [&[0.25][..], &[][..], &[0.5, 0.75][..]] {
                push_int(&mut bytes, 0x04, values.len() as i64, int_width);
                for value in values {
                    push_f64(&mut bytes, *value);
                }
            }
            bytes.extend_from_slice(&[0x0d, 0x06]);
            bytes.extend_from_slice(b"spline");
            bytes.extend_from_slice(&surface_block(int_width));
            bytes.push(0x14);
            let light = bytes.len();
            for value in [0.0f64, -1.0, 0.0] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            if matches!(kind, SilhouetteKind::Taper { .. }) {
                push_f64(&mut bytes, 0.5);
            }

            let layout = silhouette_patch_layout(&bytes, int_width, &kind)
                .unwrap_or_else(|| panic!("{name} layout at width {int_width}"));
            assert_eq!(layout.light_direction, light);
            assert_eq!(layout.draft_factor.is_some(), name.starts_with("taper"));
        }
    }
}

#[test]
fn surface_offset_layout_walks_both_integer_widths() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let name = "off_surf_int_cur";
        let mut bytes = vec![0x0f, 0x0d, name.len() as u8];
        bytes.extend_from_slice(name.as_bytes());
        for _ in 0..2 {
            bytes.extend_from_slice(&[0x0d, 0x06]);
            bytes.extend_from_slice(b"spline");
            bytes.extend_from_slice(&surface_block(int_width));
        }
        bytes.extend_from_slice(&pcurve_block(int_width));
        bytes.extend_from_slice(&pcurve_block(int_width));
        push_f64(&mut bytes, -2.0);
        push_f64(&mut bytes, 3.0);
        for values in [&[0.25][..], &[][..], &[0.5, 0.75][..]] {
            push_int(&mut bytes, 0x04, values.len() as i64, int_width);
            for value in values {
                push_f64(&mut bytes, *value);
            }
        }
        let flag = bytes.len();
        bytes.push(0x0a);
        for value in [-1.0, 1.0, -2.0, 2.0] {
            push_f64(&mut bytes, value);
        }
        bytes.extend_from_slice(&curve_block(int_width));
        for value in [-3.0, 3.0, 0.5, 0.25, 1.5] {
            push_f64(&mut bytes, value);
        }

        let layout = surface_offset_patch_layout(&bytes, int_width)
            .unwrap_or_else(|| panic!("surface-offset layout at width {int_width}"));
        assert_eq!(layout.discontinuity_flag, flag);
        assert_eq!(
            layout.discontinuities.iter().map(Vec::len).sum::<usize>(),
            3
        );
        assert!(layout.distance < layout.shift && layout.shift < layout.scale);
    }
}
