// SPDX-License-Identifier: Apache-2.0

use super::{decode_pcurve_cache, final_pcurve_patch_layout, pcurve_fit_tolerance};
use crate::kernel_header::RefWidth;
use crate::nurbs::blend::{
    decode_rolling_ball_curve, decode_rolling_ball_side, decode_rolling_ball_surface,
    DecodedRollingBallCurve,
};
use crate::nurbs::core::{
    curve_cache, decode_curve_cache, decode_surface_cache, final_curve_patch_layout,
    final_surface_patch_layout, first_curve_patch_layout, surface_cache, surface_patch_layout_at,
};
use crate::nurbs::proc_curve::{
    compound_patch_layout, extrusion_patch_layout, helix_patch_layout, intersection_patch_layout,
    procedural_curve_resolving_refs, projection_patch_layout, rolling_ball_patch_layout,
    silhouette_patch_layout, spring_patch_layout, subset_patch_layout, surface_curve_patch_layout,
    surface_offset_patch_layout, three_surface_patch_layout, vector_offset_patch_layout,
    EmbeddedSurfaceCurve, ProjectionTailPatchLayout,
};
use crate::nurbs::proc_surface::{DecodedProceduralSurfaceDefinition, EmbeddedLawExpression};
use crate::nurbs::reader::NUBS_MARKER;
use crate::nurbs::subtypes::SubtypeTables;
use crate::nurbs::toks::{lex_test_span, test_table};
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

fn push_int(out: &mut Vec<u8>, tag: u8, value: i64, int_width: RefWidth) {
    out.push(tag);
    if int_width == RefWidth::Four {
        out.extend_from_slice(
            &i32::try_from(value)
                .expect("test value fits i32")
                .to_le_bytes(),
        );
    } else {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

fn push_f64(out: &mut Vec<u8>, value: f64) {
    out.push(0x06);
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_ident(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&[0x0d, value.len() as u8]);
    out.extend_from_slice(value.as_bytes());
}

fn push_string(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&[0x07, value.len() as u8]);
    out.extend_from_slice(value.as_bytes());
}

fn push_position(out: &mut Vec<u8>, values: [f64; 3]) {
    out.push(0x13);
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

fn push_vector(out: &mut Vec<u8>, values: [f64; 3]) {
    out.push(0x14);
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

/// A degree-1 two-pole 3D `nubs` curve block over `[0, 1]`.
fn curve_block_with_endpoint(int_width: RefWidth, endpoint: [f64; 3]) -> Vec<u8> {
    let mut b = NUBS_MARKER.to_vec();
    push_int(&mut b, 0x04, 1, int_width); // degree
    push_int(&mut b, 0x15, 0, int_width); // open closure
    push_int(&mut b, 0x04, 2, int_width); // unique knot count
    push_f64(&mut b, 0.0);
    push_int(&mut b, 0x04, 1, int_width);
    push_f64(&mut b, 1.0);
    push_int(&mut b, 0x04, 1, int_width);
    for component in [0.0, 0.0, 0.0, endpoint[0], endpoint[1], endpoint[2]] {
        push_f64(&mut b, component);
    }
    b
}

fn curve_block(int_width: RefWidth) -> Vec<u8> {
    curve_block_with_endpoint(int_width, [1.0, 2.0, 3.0])
}

/// A degree-1 2×2-pole `nubs` surface block over `[0, 1]²`.
fn surface_block_with_x_offset(int_width: RefWidth, x_offset: f64) -> Vec<u8> {
    let mut b = NUBS_MARKER.to_vec();
    push_int(&mut b, 0x04, 1, int_width); // u degree
    push_int(&mut b, 0x04, 1, int_width); // v degree
    for _ in 0..4 {
        push_int(&mut b, 0x15, 0, int_width); // periodic/singularity enums
    }
    push_int(&mut b, 0x04, 2, int_width); // unique u knots
    push_int(&mut b, 0x04, 2, int_width); // unique v knots
    for _ in 0..2 {
        push_f64(&mut b, 0.0);
        push_int(&mut b, 0x04, 1, int_width);
        push_f64(&mut b, 1.0);
        push_int(&mut b, 0x04, 1, int_width);
    }
    for pole in 0..4 {
        push_f64(&mut b, x_offset + f64::from(pole));
        push_f64(&mut b, 0.0);
        push_f64(&mut b, 0.0);
    }
    b
}

fn surface_block(int_width: RefWidth) -> Vec<u8> {
    surface_block_with_x_offset(int_width, 0.0)
}

fn pcurve_block(int_width: RefWidth) -> Vec<u8> {
    let mut b = NUBS_MARKER.to_vec();
    push_int(&mut b, 0x04, 1, int_width);
    push_int(&mut b, 0x15, 0, int_width);
    push_int(&mut b, 0x04, 2, int_width);
    for knot in [0.0, 1.0] {
        push_f64(&mut b, knot);
        push_int(&mut b, 0x04, 1, int_width);
    }
    for component in [0.0, 0.0, 1.0, 1.0] {
        push_f64(&mut b, component);
    }
    b
}

/// A revision-gated exact curve with one plane support and one paired BS2
/// pcurve in the shared cache-first context.
fn exact_cache_first_curve(int_width: RefWidth) -> Vec<u8> {
    let mut bytes = vec![0x0f];
    push_ident(&mut bytes, "exact_int_cur");
    push_int(&mut bytes, 0x04, 23_100, int_width);
    push_int(&mut bytes, 0x15, 0, int_width);
    bytes.extend_from_slice(&curve_block(int_width));
    push_f64(&mut bytes, 0.0);

    // Slot 1 is an inline plane support with one paired UV curve.
    push_ident(&mut bytes, "plane");
    push_position(&mut bytes, [0.0, 0.0, 0.0]);
    push_vector(&mut bytes, [0.0, 0.0, 1.0]);
    push_vector(&mut bytes, [1.0, 0.0, 0.0]);
    bytes.extend_from_slice(&[0x0b; 5]);
    push_ident(&mut bytes, "null_surface");

    // Slot 1 has one paired UV curve; slot 2 has pcurve absence.
    bytes.extend_from_slice(&pcurve_block(int_width));
    push_ident(&mut bytes, "nullbs");
    bytes.extend_from_slice(&[0x0b; 2]);
    for _ in 0..3 {
        push_int(&mut bytes, 0x04, 0, int_width);
    }
    push_int(&mut bytes, 0x04, 0, int_width);

    // Exact-curve fields after the shared cache-first context.
    bytes.push(0x0a);
    push_f64(&mut bytes, 1.0);
    bytes.push(0x0a);
    push_f64(&mut bytes, 0.0);
    push_int(&mut bytes, 0x15, 0, int_width);
    push_int(&mut bytes, 0x15, 0, int_width);
    bytes.push(0x10);
    bytes
}

#[test]
fn intcurve_selector_uses_the_serialized_direct_slot() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let mut bytes = curve_block(int_width);
        bytes.extend_from_slice(&pcurve_block(int_width));
        let toks = crate::nurbs::toks::lex_test_span(&bytes, int_width);
        let table = crate::nurbs::toks::test_table(&bytes, int_width);

        assert!(
            crate::nurbs::proc_curve::pcurve_for_selector_resolving_refs(&toks, 2, &table)
                .is_some()
        );
        assert!(
            crate::nurbs::proc_curve::pcurve_for_selector_resolving_refs(&toks, -2, &table)
                .is_some()
        );
        assert!(
            crate::nurbs::proc_curve::pcurve_for_selector_resolving_refs(&toks, 1, &table)
                .is_none()
        );
    }
}

#[test]
fn exact_curve_selector_uses_its_cache_first_support_slot() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let bytes = exact_cache_first_curve(int_width);

        let toks = crate::nurbs::toks::lex_test_span(&bytes, int_width);
        let table = crate::nurbs::toks::test_table(&bytes, int_width);
        let decoded = crate::nurbs::proc_curve::procedural_curve_resolving_refs(&toks, &table)
            .unwrap_or_else(|| panic!("exact curve at width {int_width}"));
        assert!(matches!(
            decoded.construction,
            crate::nurbs::proc_curve::ProceduralCurveConstruction::Exact
        ));
        let pcurve = crate::nurbs::proc_curve::pcurve_for_selector_resolving_refs(&toks, 1, &table)
            .unwrap_or_else(|| panic!("exact curve pcurve at width {int_width}"));
        assert_eq!(
            pcurve.control_points()[1],
            cadmpeg_ir::math::Point2::new(10.0, -10.0)
        );
        assert!(
            crate::nurbs::proc_curve::pcurve_for_selector_resolving_refs(&toks, -1, &table)
                .is_some()
        );
        assert!(
            crate::nurbs::proc_curve::pcurve_for_selector_resolving_refs(&toks, 2, &table)
                .is_none()
        );
    }
}

#[test]
fn exact_curve_selector_follows_subtype_reference() {
    for int_width in [RefWidth::Four, RefWidth::Eight] {
        let active = exact_cache_first_curve(int_width);
        for named in [false, true] {
            let mut wrapper = vec![0x0f];
            if named {
                push_ident(&mut wrapper, "ref");
            }
            push_int(&mut wrapper, 0x04, 0, int_width);
            wrapper.push(0x10);
            let toks = crate::nurbs::toks::lex_test_span(&wrapper, int_width);
            let table = crate::nurbs::toks::test_table(&active, int_width);
            assert!(
                crate::nurbs::proc_curve::pcurve_for_selector_resolving_refs(&toks, -1, &table)
                    .is_some()
            );
            assert!(
                crate::nurbs::proc_curve::pcurve_for_selector_resolving_refs(&toks, 2, &table)
                    .is_none()
            );
        }
    }
}

fn rolling_ball_side(int_width: RefWidth, label: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_string(
        &mut bytes,
        if label == "left" {
            "blend_support_surface"
        } else {
            "blend_support_curve"
        },
    );
    push_ident(&mut bytes, "null_surface");
    bytes.extend_from_slice(&curve_block(int_width));
    bytes.extend_from_slice(&[0x0b, 0x0b]);
    bytes.extend_from_slice(&pcurve_block(int_width));
    push_position(&mut bytes, [7.0, 8.0, 9.0]);
    push_ident(&mut bytes, "nullbs");
    push_int(&mut bytes, 0x04, 0, int_width);
    push_ident(&mut bytes, "nullbs");
    bytes
}

fn variable_blend_side(int_width: RefWidth, name: &str, extension: Option<i64>) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_string(&mut bytes, name);
    push_ident(&mut bytes, "null_surface");
    push_ident(&mut bytes, "null_curve");
    bytes.extend_from_slice(&pcurve_block(int_width));
    push_position(&mut bytes, [1.0, 2.0, 3.0]);
    push_ident(&mut bytes, "nullbs");
    if let Some(extension) = extension {
        push_int(&mut bytes, 0x04, extension, int_width);
        push_ident(&mut bytes, "nullbs");
    }
    bytes.push(0x10);
    bytes
}

mod blend_laws;
mod cache_selection;
mod procedural_curves;
mod procedural_surfaces;
