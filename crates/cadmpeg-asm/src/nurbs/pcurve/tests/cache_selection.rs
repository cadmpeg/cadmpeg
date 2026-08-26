// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn curve_cache_decodes_in_both_integer_widths() {
    for int_width in [4usize, 8] {
        let block = curve_block(int_width);
        let curve = decode_curve_cache(&block)
            .unwrap_or_else(|| panic!("curve cache at width {int_width}"));
        assert_eq!(curve.degree, 1);
        assert_eq!(curve.control_points.len(), 2);
        assert_eq!(curve.control_points[1].x, 10.0); // cm→mm ×10
        assert_eq!(curve.knots, vec![0.0, 0.0, 1.0, 1.0]);
        assert!(first_curve_patch_layout(&block, int_width).is_some());
        assert!(final_curve_patch_layout(&block, int_width).is_some());
        let other_width = if int_width == 4 { 8 } else { 4 };
        assert!(first_curve_patch_layout(&block, other_width).is_none());
        assert!(final_curve_patch_layout(&block, other_width).is_none());
    }
}

#[test]
fn generic_curve_and_pcurve_caches_withhold_multiple_candidates() {
    for int_width in [4usize, 8] {
        let mut curves = curve_block(int_width);
        curves.extend_from_slice(&curve_block(int_width));
        assert!(decode_curve_cache(&curves).is_none());

        let mut pcurves = pcurve_block(int_width);
        pcurves.extend_from_slice(&pcurve_block(int_width));
        assert!(super::decode_pcurve_cache(&pcurves).is_none());

        let block = pcurve_block(int_width);
        assert!(super::final_pcurve_patch_layout(&block, int_width).is_some());
        let other_width = if int_width == 4 { 8 } else { 4 };
        assert!(super::final_pcurve_patch_layout(&block, other_width).is_none());

        let mut surfaces = b"comp_spl_sur".to_vec();
        surfaces.extend_from_slice(&surface_block(int_width));
        surfaces.extend_from_slice(&surface_block(int_width));
        assert!(decode_surface_cache(&surfaces).is_none());
    }
}

#[test]
fn wrapper_directrix_fields_reject_nested_curve_substitution() {
    for int_width in [4usize, 8] {
        let mut subset = vec![0x0f];
        push_ident(&mut subset, "subset_int_cur");
        subset.push(0x0f);
        push_ident(&mut subset, "support");
        subset.extend_from_slice(&curve_block(int_width));
        subset.push(0x10);
        push_f64(&mut subset, -1.0);
        push_f64(&mut subset, 2.0);
        subset.extend_from_slice(&curve_block(int_width));
        push_f64(&mut subset, 0.001);
        subset.push(0x10);
        let subset_tokens = lex_test_span(&subset, int_width);
        let subset_decoded =
            procedural_curve_resolving_refs(&subset_tokens, &test_table(&subset, int_width))
                .unwrap_or_else(|| panic!("nested subset source at width {int_width}"));
        assert!(subset_decoded.subset.is_none());

        let mut offset = vec![0x0f];
        push_ident(&mut offset, "offset_int_cur");
        offset.push(0x0b);
        offset.push(0x0f);
        push_ident(&mut offset, "support");
        offset.extend_from_slice(&curve_block(int_width));
        offset.push(0x10);
        push_f64(&mut offset, -1.0);
        push_f64(&mut offset, 2.0);
        push_vector(&mut offset, [1.0, 2.0, 3.0]);
        push_string(&mut offset, "first");
        push_int(&mut offset, 0x04, 4, int_width);
        push_string(&mut offset, "second");
        push_int(&mut offset, 0x04, 5, int_width);
        offset.extend_from_slice(&curve_block(int_width));
        push_f64(&mut offset, 0.001);
        offset.push(0x10);
        let offset_tokens = lex_test_span(&offset, int_width);
        let offset_decoded =
            procedural_curve_resolving_refs(&offset_tokens, &test_table(&offset, int_width))
                .unwrap_or_else(|| panic!("nested vector-offset source at width {int_width}"));
        assert!(offset_decoded.vector_offset.is_none());
    }
}

#[test]
fn pcurve_fit_tolerance_withholds_nested_only_cache() {
    for int_width in [4usize, 8] {
        let mut bytes = vec![0x0f];
        push_ident(&mut bytes, "exp_par_cur");
        bytes.push(0x0f);
        push_ident(&mut bytes, "support");
        bytes.extend_from_slice(&pcurve_block(int_width));
        push_f64(&mut bytes, 0.9);
        bytes.push(0x10);
        bytes.push(0x10);

        let tokens = lex_test_span(&bytes, int_width);
        assert!(super::pcurve_fit_tolerance(&tokens).is_none());
    }
}

#[test]
fn patch_layout_roles_exclude_nested_construction_caches() {
    for int_width in [4usize, 8] {
        let mut surfaces = Vec::new();
        push_ident(&mut surfaces, "spline");
        surfaces.push(0x0f);
        push_ident(&mut surfaces, "auxiliary");
        surfaces.push(0x10);
        surfaces.push(0x0f);
        push_ident(&mut surfaces, "carrier");
        surfaces.extend_from_slice(&surface_block(int_width));
        let surface_end = surfaces.len();
        surfaces.push(0x0f);
        push_ident(&mut surfaces, "support");
        surfaces.extend_from_slice(&surface_block_with_x_offset(int_width, 9.0));
        surfaces.extend_from_slice(&[0x10, 0x10]);
        assert_eq!(
            final_surface_patch_layout(&surfaces, int_width)
                .expect("owned surface layout")
                .end,
            surface_end
        );
        assert!(surface_patch_layout_at(&surfaces, 1, int_width).is_none());
        let mut ambiguous_surfaces = surfaces.clone();
        ambiguous_surfaces.push(0x0f);
        push_ident(&mut ambiguous_surfaces, "competing");
        ambiguous_surfaces.extend_from_slice(&surface_block(int_width));
        ambiguous_surfaces.push(0x10);
        assert!(final_surface_patch_layout(&ambiguous_surfaces, int_width).is_none());

        let mut curves = Vec::new();
        push_ident(&mut curves, "intcurve");
        curves.push(0x0f);
        push_ident(&mut curves, "auxiliary");
        curves.push(0x10);
        curves.push(0x0f);
        push_ident(&mut curves, "carrier");
        curves.extend_from_slice(&curve_block(int_width));
        let curve_end = curves.len();
        curves.push(0x0f);
        push_ident(&mut curves, "support");
        curves.extend_from_slice(&curve_block_with_endpoint(int_width, [9.0, 0.0, 0.0]));
        curves.extend_from_slice(&[0x10, 0x10]);
        assert_eq!(
            first_curve_patch_layout(&curves, int_width)
                .expect("first owned curve layout")
                .end,
            curve_end
        );
        assert_eq!(
            final_curve_patch_layout(&curves, int_width)
                .expect("final owned curve layout")
                .end,
            curve_end
        );

        let mut pcurves = Vec::new();
        push_ident(&mut pcurves, "pcurve");
        pcurves.push(0x0f);
        push_ident(&mut pcurves, "auxiliary");
        pcurves.push(0x10);
        pcurves.push(0x0f);
        push_ident(&mut pcurves, "carrier");
        pcurves.extend_from_slice(&pcurve_block(int_width));
        let pcurve_end = pcurves.len();
        pcurves.push(0x0f);
        push_ident(&mut pcurves, "support");
        pcurves.extend_from_slice(&pcurve_block(int_width));
        pcurves.extend_from_slice(&[0x10, 0x10]);
        assert_eq!(
            super::final_pcurve_patch_layout(&pcurves, int_width)
                .expect("final owned pcurve layout")
                .control_end,
            pcurve_end
        );
    }
}

#[test]
fn surface_cache_decodes_in_both_integer_widths() {
    for int_width in [4usize, 8] {
        let block = surface_block(int_width);
        let surface = decode_surface_cache(&block)
            .unwrap_or_else(|| panic!("surface cache at width {int_width}"));
        assert_eq!((surface.u_degree, surface.v_degree), (1, 1));
        assert_eq!((surface.u_count, surface.v_count), (2, 2));
        assert!(final_surface_patch_layout(&block, int_width).is_some());
        let other_width = if int_width == 4 { 8 } else { 4 };
        assert!(final_surface_patch_layout(&block, other_width).is_none());
    }
}

#[test]
fn token_curve_cache_ignores_nested_support_scope() {
    for int_width in [4usize, 8] {
        let mut bytes = vec![0x0f];
        push_ident(&mut bytes, "exact_int_cur");
        bytes.push(0x0f);
        push_ident(&mut bytes, "support");
        bytes.extend_from_slice(&curve_block_with_endpoint(int_width, [2.0, 0.0, 0.0]));
        bytes.push(0x10);
        bytes.extend_from_slice(&curve_block_with_endpoint(int_width, [7.0, 0.0, 0.0]));
        bytes.push(0x10);

        let tokens = lex_test_span(&bytes, int_width);
        let curve = curve_cache(&tokens)
            .unwrap_or_else(|| panic!("owned curve cache at width {int_width}"));

        assert!((curve.control_points[1].x - 70.0).abs() < f64::EPSILON);
    }
}

#[test]
fn procedural_curve_cache_ignores_nested_support_scope() {
    for int_width in [4usize, 8] {
        let mut bytes = vec![0x0f];
        push_ident(&mut bytes, "spring_int_cur");
        bytes.push(0x0f);
        push_ident(&mut bytes, "support");
        bytes.extend_from_slice(&curve_block_with_endpoint(int_width, [2.0, 0.0, 0.0]));
        bytes.push(0x10);
        bytes.extend_from_slice(&curve_block_with_endpoint(int_width, [7.0, 0.0, 0.0]));
        bytes.push(0x10);

        let tokens = lex_test_span(&bytes, int_width);
        let decoded = procedural_curve_resolving_refs(&tokens, &test_table(&bytes, int_width))
            .unwrap_or_else(|| panic!("procedural curve at width {int_width}"));

        assert!((decoded.curve.control_points[1].x - 70.0).abs() < f64::EPSILON);
    }
}

#[test]
fn procedural_curve_with_only_nested_cache_is_withheld() {
    for int_width in [4usize, 8] {
        let mut bytes = vec![0x0f];
        push_ident(&mut bytes, "spring_int_cur");
        bytes.push(0x0f);
        push_ident(&mut bytes, "support");
        bytes.extend_from_slice(&curve_block(int_width));
        bytes.push(0x10);
        bytes.push(0x10);

        let tokens = lex_test_span(&bytes, int_width);

        assert!(procedural_curve_resolving_refs(&tokens, &test_table(&bytes, int_width)).is_none());
    }
}

#[test]
fn token_surface_cache_ignores_later_nested_support_scope() {
    for int_width in [4usize, 8] {
        let mut bytes = vec![0x0f];
        push_ident(&mut bytes, "off_spl_sur");
        bytes.extend_from_slice(&surface_block_with_x_offset(int_width, 5.0));
        bytes.push(0x0f);
        push_ident(&mut bytes, "support");
        bytes.extend_from_slice(&surface_block_with_x_offset(int_width, 9.0));
        bytes.push(0x10);
        bytes.push(0x10);

        let tokens = lex_test_span(&bytes, int_width);
        let surface = surface_cache(&tokens)
            .unwrap_or_else(|| panic!("owned surface cache at width {int_width}"));

        assert!((surface.control_points[0].x - 50.0).abs() < f64::EPSILON);
    }
}
