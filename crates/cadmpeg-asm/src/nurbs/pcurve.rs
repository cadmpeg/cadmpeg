// SPDX-License-Identifier: Apache-2.0
//! Cached parameter-space curve (pcurve) block decoding, patch layouts, and cache entry points.

use crate::nurbs::core::KnotPatchLayout;
use crate::nurbs::reader::{
    is_periodic, marker_at, marker_positions, read_knots, take_tagged_int, INT_WIDTHS,
};
use crate::nurbs::toks::{self, Cur};
use crate::sab::Token;
use cadmpeg_core::decode::View;
use cadmpeg_ir::math::Point2;

/// The decoded payload of a 2D `nubs` or `nurbs` pcurve block.
#[derive(Clone)]
pub struct NurbsPcurve {
    /// Curve degree.
    pub degree: u32,
    /// Full clamped knot vector.
    pub knots: Vec<f64>,
    /// UV control points in surface-parameter space, without length scaling.
    pub control_points: Vec<Point2>,
    /// Per-pole homogeneous weights; absent for a `nubs` block.
    pub weights: Option<Vec<f64>>,
    /// Whether the parameter curve is periodic.
    pub periodic: bool,
}

/// Writable value offsets for one 2D pcurve cache.
pub struct PcurvePatchLayout {
    /// Payload width of integer and enum fields.
    pub int_width: usize,
    /// Tagged-integer payload offset for the curve degree.
    pub degree_value_offset: usize,
    /// Tagged-double payload offsets in `(u, v)` pole order.
    pub control_value_offsets: Vec<usize>,
    /// Tagged-double payload offsets for homogeneous weights.
    pub weight_value_offsets: Vec<usize>,
    /// Number of UV control points.
    pub control_count: usize,
    /// Native unique-knot payloads and expanded run lengths.
    pub knots: KnotPatchLayout,
    /// Payload offset for the closure enum.
    pub periodic_value_offset: usize,
    /// Offset immediately after the final UV control component.
    pub control_end: usize,
}

/// Locate the final valid non-rational 2D pcurve block in a carrier record.
pub fn final_pcurve_patch_layout(record: &[u8]) -> Option<PcurvePatchLayout> {
    INT_WIDTHS
        .into_iter()
        .find_map(|int_width| final_pcurve_patch_layout_at(record, int_width))
}

fn final_pcurve_patch_layout_at(record: &[u8], int_width: usize) -> Option<PcurvePatchLayout> {
    marker_positions(record)
        .into_iter()
        .filter_map(|marker_pos| {
            let (_cp_dims, marker_len, rational) = marker_at(record, marker_pos)?;
            let mut pos = marker_pos + marker_len;
            let degree_value_offset = pos + 1;
            let degree = take_tagged_int(record, &mut pos, 0x04, int_width)?;
            if !(1..=20).contains(&degree) {
                return None;
            }
            let periodic_value_offset = pos + 1;
            let _closure = take_tagged_int(record, &mut pos, 0x15, int_width)?;
            let unique = take_tagged_int(record, &mut pos, 0x04, int_width)?;
            if !(1..=1000).contains(&unique) {
                return None;
            }
            let (_knots, control_count, knot_layout) =
                read_knots(record, &mut pos, unique as usize, degree, int_width)?;
            let mut offsets = Vec::with_capacity(control_count * 2);
            let mut weight_offsets = Vec::with_capacity(control_count * usize::from(rational));
            for _ in 0..control_count * 2 {
                if record.get(pos) != Some(&0x06) {
                    return None;
                }
                offsets.push(pos + 1);
                pos += 9;
                if rational && offsets.len() % 2 == 0 {
                    if record.get(pos) != Some(&0x06) {
                        return None;
                    }
                    weight_offsets.push(pos + 1);
                    pos += 9;
                }
            }
            Some(PcurvePatchLayout {
                int_width,
                degree_value_offset,
                control_value_offsets: offsets,
                weight_value_offsets: weight_offsets,
                control_count,
                knots: knot_layout.into(),
                periodic_value_offset,
                control_end: pos,
            })
        })
        .next_back()
}

fn decode_pcurve_block(b: &[u8], marker_pos: usize, int_width: usize) -> Option<NurbsPcurve> {
    decode_pcurve_block_with_end(b, marker_pos, int_width).map(|(pcurve, _)| pcurve)
}

pub(crate) fn decode_pcurve_block_with_end(
    b: &[u8],
    marker_pos: usize,
    int_width: usize,
) -> Option<(NurbsPcurve, usize)> {
    let (_cp_dims, marker_len, rational) = marker_at(b, marker_pos)?;
    let mut pos = marker_pos + marker_len;
    let degree = take_tagged_int(b, &mut pos, 0x04, int_width)?;
    if !(1..=20).contains(&degree) {
        return None;
    }
    let closure = take_tagged_int(b, &mut pos, 0x15, int_width)?;
    let n_uniq = take_tagged_int(b, &mut pos, 0x04, int_width)?;
    if !(1..=1000).contains(&n_uniq) {
        return None;
    }
    let (knots, n_poles, _knot_layout) =
        read_knots(b, &mut pos, n_uniq as usize, degree, int_width)?;
    let mut control_points = Vec::with_capacity(n_poles);
    let mut weights = rational.then(|| Vec::with_capacity(n_poles));
    for _ in 0..n_poles {
        if *b.get(pos)? != 0x06 {
            return None;
        }
        let u = View::f64_le_at(b, pos + 1)?;
        pos += 9;
        if *b.get(pos)? != 0x06 {
            return None;
        }
        let v = View::f64_le_at(b, pos + 1)?;
        pos += 9;
        control_points.push(Point2::new(u, v));
        if let Some(weights) = weights.as_mut() {
            if *b.get(pos)? != 0x06 {
                return None;
            }
            weights.push(View::f64_le_at(b, pos + 1)?);
            pos += 9;
        }
    }
    Some((
        NurbsPcurve {
            degree: degree as u32,
            knots,
            control_points,
            weights,
            periodic: is_periodic(closure),
        },
        pos,
    ))
}

/// Decode the first well-formed 2D `nubs` block in a pcurve record.
pub fn decode_pcurve_cache(record_bytes: &[u8]) -> Option<NurbsPcurve> {
    INT_WIDTHS
        .into_iter()
        .find_map(|int_width| decode_pcurve_cache_at(record_bytes, int_width))
}

fn decode_pcurve_cache_at(record_bytes: &[u8], int_width: usize) -> Option<NurbsPcurve> {
    marker_positions(record_bytes)
        .into_iter()
        .find_map(|pos| decode_pcurve_block(record_bytes, pos, int_width))
}

/// Decode a 2D `nubs`/`nurbs` pcurve block at token `marker_pos`, returning
/// the pcurve and the token index just past the block. Token-space counterpart
/// of [`decode_pcurve_block_with_end`].
pub(crate) fn pcurve_block_with_end(
    toks: &[Token],
    marker_pos: usize,
) -> Option<(NurbsPcurve, usize)> {
    let rational = toks::marker_at(toks, marker_pos)?.rational();
    let mut cur = Cur::at(toks, marker_pos + 1);
    let degree = cur.take_long()?;
    if !(1..=20).contains(&degree) {
        return None;
    }
    let closure = cur.take_enum()?;
    let n_uniq = cur.take_long()?;
    if !(1..=1000).contains(&n_uniq) {
        return None;
    }
    let (knots, n_poles) = toks::take_knot_table(&mut cur, n_uniq as usize, degree)?;
    let mut control_points = Vec::new();
    let mut weights = rational.then(Vec::new);
    for _ in 0..n_poles {
        let u = cur.take_f64()?;
        let v = cur.take_f64()?;
        control_points.push(Point2::new(u, v));
        if let Some(weights) = weights.as_mut() {
            weights.push(cur.take_f64()?);
        }
    }
    Some((
        NurbsPcurve {
            degree: degree as u32,
            knots,
            control_points,
            weights,
            periodic: is_periodic(closure),
        },
        cur.pos(),
    ))
}

fn pcurve_block(toks: &[Token], marker_pos: usize) -> Option<NurbsPcurve> {
    pcurve_block_with_end(toks, marker_pos).map(|(pcurve, _)| pcurve)
}

/// Decode the BS2 field owned directly by an `exp_par_cur` scope.
///
/// The scope grammar makes its first owned B-spline block the pcurve. Nested
/// support references are not searched because they belong to other fields.
pub fn explicit_pcurve_cache(toks: &[Token]) -> Option<NurbsPcurve> {
    let position = toks::owned_marker_positions(toks).into_iter().next()?;
    pcurve_block(toks, position)
}

/// Resolve an explicit pcurve through one subtype-table reference.
pub fn explicit_pcurve_cache_from_subtype_ref(
    index: i64,
    table: &toks::SubtypeTable,
) -> Option<NurbsPcurve> {
    let index = usize::try_from(index).ok()?;
    explicit_pcurve_cache(table.span(index)?)
}

/// The parameter-space fit tolerance immediately following the final valid 2D
/// pcurve block in `toks`. Token-space counterpart of
/// [`decode_pcurve_fit_tolerance`].
pub fn pcurve_fit_tolerance(toks: &[Token]) -> Option<f64> {
    let (_, end) = toks::marker_positions(toks)
        .into_iter()
        .filter_map(|pos| pcurve_block_with_end(toks, pos))
        .next_back()?;
    match toks.get(end) {
        Some(Token::Double(value)) => Some(*value),
        _ => None,
    }
}

#[cfg(test)]
mod width_tests {
    use crate::nurbs::blend::{
        decode_rolling_ball_curve, decode_rolling_ball_side, decode_rolling_ball_surface,
        DecodedRollingBallCurve,
    };
    use crate::nurbs::core::{
        curve_cache, decode_curve_cache, decode_surface_cache, surface_cache,
    };
    use crate::nurbs::proc_curve::{
        compound_patch_layout, extrusion_patch_layout, helix_patch_layout,
        intersection_patch_layout, procedural_curve_resolving_refs, projection_patch_layout,
        rolling_ball_patch_layout, silhouette_patch_layout, spring_patch_layout,
        subset_patch_layout, surface_curve_patch_layout, surface_offset_patch_layout,
        three_surface_patch_layout, vector_offset_patch_layout, ProjectionTailPatchLayout,
    };
    use crate::nurbs::proc_surface::{DecodedProceduralSurfaceDefinition, EmbeddedLawExpression};
    use crate::nurbs::reader::NUBS_MARKER;
    use crate::nurbs::subtypes::SubtypeTables;
    use crate::nurbs::toks::{lex_test_span, test_table};
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};

    fn push_int(out: &mut Vec<u8>, tag: u8, value: i64, int_width: usize) {
        out.push(tag);
        if int_width == 4 {
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
    fn curve_block_with_endpoint(int_width: usize, endpoint: [f64; 3]) -> Vec<u8> {
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

    fn curve_block(int_width: usize) -> Vec<u8> {
        curve_block_with_endpoint(int_width, [1.0, 2.0, 3.0])
    }

    /// A degree-1 2×2-pole `nubs` surface block over `[0, 1]²`.
    fn surface_block_with_x_offset(int_width: usize, x_offset: f64) -> Vec<u8> {
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

    fn surface_block(int_width: usize) -> Vec<u8> {
        surface_block_with_x_offset(int_width, 0.0)
    }

    fn pcurve_block(int_width: usize) -> Vec<u8> {
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
    fn exact_cache_first_curve(int_width: usize) -> Vec<u8> {
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
        for int_width in [4usize, 8] {
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
        for int_width in [4usize, 8] {
            let bytes = exact_cache_first_curve(int_width);

            let toks = crate::nurbs::toks::lex_test_span(&bytes, int_width);
            let table = crate::nurbs::toks::test_table(&bytes, int_width);
            let decoded = crate::nurbs::proc_curve::procedural_curve_resolving_refs(&toks, &table)
                .unwrap_or_else(|| panic!("exact curve at width {int_width}"));
            assert_eq!(decoded.native_kind, "exact_int_cur");
            let pcurve =
                crate::nurbs::proc_curve::pcurve_for_selector_resolving_refs(&toks, 1, &table)
                    .unwrap_or_else(|| panic!("exact curve pcurve at width {int_width}"));
            assert_eq!(
                pcurve.control_points[1],
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
        for int_width in [4usize, 8] {
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

    fn rolling_ball_side(int_width: usize, label: &str) -> Vec<u8> {
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

    fn variable_blend_side(int_width: usize, name: &str, extension: Option<i64>) -> Vec<u8> {
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

            let decoded =
                crate::nurbs::proc_surface::law_spl_sur(&lex_test_span(&bytes, int_width))
                    .unwrap_or_else(|| panic!("law surface at width {int_width}"));
            let DecodedProceduralSurfaceDefinition::Law(construction) = decoded.definition else {
                panic!("expected law surface at width {int_width}")
            };
            assert_eq!(construction.parameter_ranges, None);
            assert_eq!(construction.primary.name, "primary-law");
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

            let decoded =
                crate::nurbs::proc_surface::law_spl_sur(&lex_test_span(&bytes, int_width))
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
                        .unwrap_or_else(|| {
                            panic!("law tail {selector} at integer width {int_width}")
                        });
                let DecodedProceduralSurfaceDefinition::Law(construction) = decoded.definition
                else {
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

    #[test]
    fn extrusion_layout_walks_modern_and_legacy_names_at_both_widths() {
        for int_width in [4usize, 8] {
            for name in ["cyl_spl_sur", "cylsur"] {
                let mut bytes = Vec::new();
                push_f64(&mut bytes, 99.0);
                push_vector(&mut bytes, [90.0, 91.0, 92.0]);
                push_position(&mut bytes, [93.0, 94.0, 95.0]);
                bytes.push(0x0f);
                push_ident(&mut bytes, name);
                push_f64(&mut bytes, -2.0);
                push_f64(&mut bytes, 3.0);
                push_vector(&mut bytes, [4.0, 5.0, 6.0]);
                push_position(&mut bytes, [7.0, 8.0, 9.0]);
                bytes.extend_from_slice(&curve_block(int_width));
                bytes.extend_from_slice(&surface_block(int_width));
                bytes.push(0x10);

                let layout = extrusion_patch_layout(&bytes, int_width)
                    .unwrap_or_else(|| panic!("extrusion layout {name} at width {int_width}"));
                let interval = layout.parameter_interval.map(|offset| {
                    f64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
                });
                assert_eq!(interval, [-2.0, 3.0]);
                assert_eq!(
                    f64::from_le_bytes(
                        bytes[layout.direction..layout.direction + 8]
                            .try_into()
                            .unwrap()
                    ),
                    4.0
                );
                assert_eq!(
                    f64::from_le_bytes(
                        bytes[layout.native_position..layout.native_position + 8]
                            .try_into()
                            .unwrap()
                    ),
                    7.0
                );
            }
        }
    }

    #[test]
    fn extrusion_definition_decodes_without_a_solved_surface_cache() {
        for int_width in [4usize, 8] {
            let mut bytes = vec![0x0f];
            push_ident(&mut bytes, "cyl_spl_sur");
            push_f64(&mut bytes, -2.0);
            push_f64(&mut bytes, 3.0);
            push_vector(&mut bytes, [4.0, 5.0, 6.0]);
            push_position(&mut bytes, [7.0, 8.0, 9.0]);
            bytes.extend_from_slice(&curve_block(int_width));
            bytes.push(0x10);

            let decoded = crate::nurbs::blend::cyl_spl_sur(&lex_test_span(&bytes, int_width), None)
                .unwrap_or_else(|| panic!("cache-less extrusion at width {int_width}"));
            assert_eq!(decoded.cache_fit_tolerance, None);
            let DecodedProceduralSurfaceDefinition::Extrusion {
                parameter_interval,
                direction,
                native_position,
                ..
            } = decoded.definition
            else {
                panic!("expected extrusion definition")
            };
            assert_eq!(parameter_interval, [-2.0, 3.0]);
            assert_eq!(direction, Vector3::new(40.0, 50.0, 60.0));
            assert_eq!(native_position, Point3::new(70.0, 80.0, 90.0));
        }
    }

    #[test]
    fn helix_layout_walks_optional_range_flags_at_both_widths() {
        for int_width in [4usize, 8] {
            let mut bytes = Vec::new();
            push_f64(&mut bytes, 99.0);
            push_position(&mut bytes, [90.0, 91.0, 92.0]);
            push_vector(&mut bytes, [93.0, 94.0, 95.0]);
            bytes.push(0x0f);
            push_ident(&mut bytes, "helix_int_cur");
            push_int(&mut bytes, 0x04, 23_100, int_width);
            bytes.push(0x0b);
            push_f64(&mut bytes, -1.0);
            push_f64(&mut bytes, 2.0);
            push_position(&mut bytes, [3.0, 4.0, 5.0]);
            push_vector(&mut bytes, [6.0, 7.0, 8.0]);
            push_vector(&mut bytes, [9.0, 10.0, 11.0]);
            push_vector(&mut bytes, [12.0, 13.0, 14.0]);
            push_f64(&mut bytes, 15.0);
            push_vector(&mut bytes, [16.0, 17.0, 18.0]);
            bytes.extend_from_slice(&curve_block(int_width));
            bytes.push(0x10);

            assert!(
                crate::nurbs::proc_curve::helix_definition(&lex_test_span(&bytes, int_width))
                    .is_some()
            );
            let layout = helix_patch_layout(&bytes, int_width)
                .unwrap_or_else(|| panic!("helix layout at width {int_width}"));
            let range = layout
                .angle_range
                .map(|offset| f64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()));
            assert_eq!(range, [-1.0, 2.0]);
            assert_eq!(
                f64::from_le_bytes(
                    bytes[layout.frame_vectors[0]..layout.frame_vectors[0] + 8]
                        .try_into()
                        .unwrap()
                ),
                3.0
            );
            assert_eq!(
                f64::from_le_bytes(
                    bytes[layout.apex_factor..layout.apex_factor + 8]
                        .try_into()
                        .unwrap()
                ),
                15.0
            );
            assert_eq!(
                f64::from_le_bytes(bytes[layout.axis..layout.axis + 8].try_into().unwrap()),
                16.0
            );
        }
    }

    #[test]
    fn decodes_current_cacheless_helix_record() {
        let hex = "0e08696e7463757276650d0563757276650cffffffff04ffffffff0cffffffff0b0f0d0d68656c69785f696e745f637572043c5a00000a067701e4b803dd04400a0605738860695607401338aee5545e6a7e3cbfab714dc0c45b3c13b8e608728f9dbf14930e205da081e83ffbd1d341709ad73f000000000000000014fbd1d341709ad73f930e205da081e8bf00000000000000001400000000000000000000000000000000cdccccccccccf43f0600000000000000001400000000000000000000000000000000000000000000f03f0d0c6e756c6c5f737572666163650d0c6e756c6c5f737572666163650d066e756c6c62730d066e756c6c6273100b0b11";
        let bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|digits| u8::from_str_radix(std::str::from_utf8(digits).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();

        let definition = crate::nurbs::proc_curve::helix_definition(&lex_test_span(&bytes, 4))
            .expect("current cache-less helix definition");
        let cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
            angle_range,
            pitch,
            apex_factor,
            axis,
            ..
        } = definition
        else {
            panic!("expected helix definition")
        };
        assert!(angle_range[0] < angle_range[1]);
        assert_eq!(pitch, Vector3::new(0.0, 0.0, 13.0));
        assert_eq!(apex_factor, 0.0);
        assert_eq!(axis, Vector3::new(0.0, 0.0, 1.0));
    }

    #[test]
    fn decodes_current_cacheless_helix_surface_at_both_widths() {
        for int_width in [4usize, 8] {
            let mut bytes = vec![0x0f];
            push_ident(&mut bytes, "helix_spl_line");
            push_int(&mut bytes, 0x04, 23_100, int_width);
            for value in [-0.5, 0.5, -2.0, 3.0, 0.0, std::f64::consts::TAU] {
                bytes.push(0x0a);
                push_f64(&mut bytes, value);
            }
            push_position(&mut bytes, [1.0, 2.0, 3.0]);
            push_vector(&mut bytes, [2.0, 0.0, 0.0]);
            push_vector(&mut bytes, [0.0, 2.0, 0.0]);
            push_vector(&mut bytes, [0.0, 0.0, 4.0]);
            push_f64(&mut bytes, 0.25);
            push_vector(&mut bytes, [0.0, 0.0, 1.0]);
            for sentinel in ["null_surface", "null_surface", "nullbs", "nullbs"] {
                push_ident(&mut bytes, sentinel);
            }
            push_vector(&mut bytes, [5.0, 6.0, 7.0]);
            bytes.push(0x10);

            let decoded =
                crate::nurbs::proc_surface::helix_spl_sur(&lex_test_span(&bytes, int_width))
                    .unwrap_or_else(|| panic!("current helix surface at width {int_width}"));
            let DecodedProceduralSurfaceDefinition::Helix(construction) = decoded.definition else {
                panic!("expected helix surface definition")
            };
            assert_eq!(construction.path.pitch, Vector3::new(0.0, 0.0, 40.0));
            assert_eq!(
                construction.profile,
                cadmpeg_ir::geometry::HelixSurfaceProfile::Line {
                    direction: Vector3::new(50.0, 60.0, 70.0),
                }
            );
        }
    }

    #[test]
    fn vector_offset_layout_ignores_outer_vectors_at_both_widths() {
        for int_width in [4usize, 8] {
            let mut bytes = Vec::new();
            push_f64(&mut bytes, 99.0);
            push_vector(&mut bytes, [90.0, 91.0, 92.0]);
            bytes.push(0x0f);
            push_ident(&mut bytes, "offset_int_cur");
            bytes.push(0x0b);
            bytes.extend_from_slice(&curve_block(int_width));
            push_f64(&mut bytes, -2.0);
            push_f64(&mut bytes, 5.0);
            push_vector(&mut bytes, [0.5, -1.0, 2.0]);
            push_string(&mut bytes, "source");
            push_int(&mut bytes, 0x04, 7, int_width);
            push_string(&mut bytes, "offset");
            push_int(&mut bytes, 0x04, 9, int_width);
            bytes.extend_from_slice(&curve_block(int_width));
            bytes.push(0x10);

            let layout = vector_offset_patch_layout(&bytes, int_width)
                .unwrap_or_else(|| panic!("vector-offset layout at width {int_width}"));
            let range = layout
                .parameter_range
                .map(|offset| f64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()));
            assert_eq!(range, [-2.0, 5.0]);
            assert_eq!(
                f64::from_le_bytes(bytes[layout.offset..layout.offset + 8].try_into().unwrap()),
                0.5
            );
        }
    }

    #[test]
    fn subset_layout_ignores_outer_curve_cache_at_both_widths() {
        for int_width in [4usize, 8] {
            let mut bytes = curve_block(int_width);
            push_f64(&mut bytes, 99.0);
            bytes.push(0x0f);
            push_ident(&mut bytes, "subset_int_cur");
            bytes.extend_from_slice(&curve_block(int_width));
            push_f64(&mut bytes, -1.5);
            push_f64(&mut bytes, 3.5);
            bytes.extend_from_slice(&curve_block(int_width));
            bytes.push(0x10);

            let layout = subset_patch_layout(&bytes, int_width)
                .unwrap_or_else(|| panic!("subset layout at width {int_width}"));
            let range = layout
                .parameter_range
                .map(|offset| f64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()));
            assert_eq!(range, [-1.5, 3.5]);
        }
    }

    #[test]
    fn compound_layout_requires_framed_subtype_at_both_widths() {
        for int_width in [4usize, 8] {
            let mut bytes = Vec::new();
            push_string(&mut bytes, "comp_int_cur");
            push_int(&mut bytes, 0x04, 1, int_width);
            push_f64(&mut bytes, 99.0);
            bytes.push(0x0f);
            push_ident(&mut bytes, "comp_int_cur");
            push_int(&mut bytes, 0x04, 3, int_width);
            for value in [0.0, 0.5, 1.0] {
                push_f64(&mut bytes, value);
            }
            push_int(&mut bytes, 0x04, 2, int_width);
            for value in [-2.0, 4.0] {
                push_f64(&mut bytes, value);
            }
            bytes.push(0x0b);
            bytes.extend_from_slice(&curve_block(int_width));
            bytes.extend_from_slice(&curve_block(int_width));
            bytes.push(0x10);

            let layout = compound_patch_layout(&bytes, int_width)
                .unwrap_or_else(|| panic!("compound layout at width {int_width}"));
            let parameters = layout
                .parameters
                .iter()
                .map(|offset| f64::from_le_bytes(bytes[*offset..*offset + 8].try_into().unwrap()))
                .collect::<Vec<_>>();
            let component_parameters = layout
                .component_parameters
                .iter()
                .map(|offset| f64::from_le_bytes(bytes[*offset..*offset + 8].try_into().unwrap()))
                .collect::<Vec<_>>();
            assert_eq!(parameters, [0.0, 0.5, 1.0]);
            assert_eq!(component_parameters, [-2.0, 4.0]);
        }
    }

    #[test]
    fn curve_cache_decodes_in_both_integer_widths() {
        for int_width in [4usize, 8] {
            let curve = decode_curve_cache(&curve_block(int_width))
                .unwrap_or_else(|| panic!("curve cache at width {int_width}"));
            assert_eq!(curve.degree, 1);
            assert_eq!(curve.control_points.len(), 2);
            assert_eq!(curve.control_points[1].x, 10.0); // cm→mm ×10
            assert_eq!(curve.knots, vec![0.0, 0.0, 1.0, 1.0]);
        }
    }

    #[test]
    fn surface_cache_decodes_in_both_integer_widths() {
        for int_width in [4usize, 8] {
            let surface = decode_surface_cache(&surface_block(int_width))
                .unwrap_or_else(|| panic!("surface cache at width {int_width}"));
            assert_eq!((surface.u_degree, surface.v_degree), (1, 1));
            assert_eq!((surface.u_count, surface.v_count), (2, 2));
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

            assert!(
                procedural_curve_resolving_refs(&tokens, &test_table(&bytes, int_width)).is_none()
            );
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

    #[test]
    fn offset_surface_uses_direct_support_fields_then_cache() {
        for int_width in [4usize, 8] {
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
                    extension_flags,
                    revision_form: None,
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
                assert_eq!(
                    extension_flags,
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
        for int_width in [4usize, 8] {
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
    fn ruled_surface_uses_two_direct_profiles_then_cache() {
        for int_width in [4usize, 8] {
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
            let DecodedProceduralSurfaceDefinition::Ruled { first, second } = decoded.definition
            else {
                panic!("expected ruled surface");
            };

            assert!((first.control_points[1].x - 10.0).abs() < f64::EPSILON);
            assert!((second.control_points[1].x - 40.0).abs() < f64::EPSILON);
            assert!(
                (decoded.cache_fit_tolerance.expect("fit tolerance") - 0.01).abs()
                    < f64::EPSILON * 10.0
            );
        }
    }

    #[test]
    fn ruled_surface_rejects_nested_profile_substitution() {
        for int_width in [4usize, 8] {
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
        for int_width in [4usize, 8] {
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

            assert!((first.control_points[1].x - 10.0).abs() < f64::EPSILON);
            assert!((second.control_points[1].x - 40.0).abs() < f64::EPSILON);
            assert!((basepoint.x - 5.0).abs() < f64::EPSILON);
            assert!((basepoint.y - 10.0).abs() < f64::EPSILON);
            assert!((basepoint.z - 15.0).abs() < f64::EPSILON);
            assert!((fit_tolerance - 0.01).abs() < f64::EPSILON * 10.0);
        }
    }

    #[test]
    fn sum_surface_rejects_nested_curve_substitution() {
        for int_width in [4usize, 8] {
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
        for int_width in [4usize, 8] {
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

            assert!((directrix.control_points[1].x - 40.0).abs() < f64::EPSILON);
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
        for int_width in [4usize, 8] {
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
    fn surface_cache_resolves_width4_subtype_ref() {
        // Active slice: one named subtype span holding the surface cache.
        let mut active = vec![0x0f, 0x0d, 0x07];
        active.extend_from_slice(b"spl_sur");
        active.extend_from_slice(&surface_block(4));
        active.push(0x10);
        // Record: `ref 0` into the subtype table, 4-byte index payload.
        let mut record = vec![0x0f, 0x0d, 0x03];
        record.extend_from_slice(b"ref");
        push_int(&mut record, 0x04, 0, 4);
        record.push(0x10);
        let surface = crate::nurbs::core::surface_cache_resolving_refs(
            &crate::nurbs::toks::lex_test_span(&record, 4),
            &crate::nurbs::toks::test_table(&active, 4),
        )
        .expect("resolved width-4 ref");
        assert_eq!((surface.u_count, surface.v_count), (2, 2));
    }

    #[test]
    fn surface_cache_resolves_compact_subtype_refs_at_both_widths() {
        for int_width in [4usize, 8] {
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
            assert_eq!((surface.u_count, surface.v_count), (2, 2));
        }
    }

    #[test]
    fn spring_layout_walks_both_integer_widths() {
        for int_width in [4usize, 8] {
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
        for int_width in [4usize, 8] {
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
        use cadmpeg_ir::geometry::SurfaceCurveFamily;
        for int_width in [4usize, 8] {
            for (name, family) in [
                ("blend_int_cur", SurfaceCurveFamily::Blend),
                ("surf_int_cur", SurfaceCurveFamily::SurfaceConstrained),
                ("par_int_cur", SurfaceCurveFamily::Parametric),
                ("skin_int_cur", SurfaceCurveFamily::Skin),
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

                let layout = surface_curve_patch_layout(&bytes, int_width, &family)
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
        for int_width in [4usize, 8] {
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
        for int_width in [4usize, 8] {
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
                Some(SurfaceGeometry::Plane { .. })
            ));
            assert!(matches!(
                context.surfaces[1],
                Some(SurfaceGeometry::Nurbs(_))
            ));
            assert!(context.pcurves[0].is_none());
            assert!(context.pcurves[1].is_some());
            assert!(context.discontinuities.iter().all(Vec::is_empty));
        }
    }

    #[test]
    fn intersection_selector_keeps_pcurve_for_cacheless_surface_support_in_both_forms() {
        for int_width in [4usize, 8] {
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
                let decoded =
                    crate::nurbs::proc_curve::procedural_curve_resolving_refs(&toks, &table)
                        .unwrap_or_else(|| {
                            panic!("cacheless support intersection at {int_width}: {form}")
                        });
                let context = decoded
                    .embedded_intersection
                    .as_ref()
                    .map(|(context, _)| context)
                    .expect("typed intersection context");
                assert_eq!(context.support_present, [true, false]);
                assert!(context.surfaces[0].is_none());
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
        use cadmpeg_ir::geometry::SurfaceCurveFamily;

        for int_width in [4usize, 8] {
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
            let (family, context, tail) =
                decoded.embedded_surface_curve.expect("typed blend context");
            assert_eq!(family, SurfaceCurveFamily::Blend);
            assert_eq!(context.parameter_range, [0.0, 1.0]);
            assert!(matches!(
                context.surfaces[0],
                Some(SurfaceGeometry::Nurbs(_))
            ));
            assert!(context.surfaces[1].is_none());
            assert!(context.pcurves[0].is_some());
            assert!(context.pcurves[1].is_none());
            let tail = tail.expect("cache-first tail");
            assert_eq!(tail.extension, 7);
            assert!(tail.flag);
        }
    }

    #[test]
    fn cache_first_par_curve_selects_mirrored_support_slot() {
        use cadmpeg_ir::geometry::SurfaceCurveFamily;

        // `flag1 = F` mirrors the support onto the second serialized slot:
        // surface slot 1 and pcurve slot 1 are null, while slot 2 carries the
        // parametric support surface and its bs2 pcurve. `par_int_cur`
        // terminates on two booleans, so `flag2` is read after `flag1`.
        for int_width in [4usize, 8] {
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
            let (family, context, tail) =
                decoded.embedded_surface_curve.expect("typed par context");
            assert_eq!(family, SurfaceCurveFamily::Parametric);
            assert!(context.surfaces[0].is_none());
            assert!(matches!(
                context.surfaces[1],
                Some(SurfaceGeometry::Nurbs(_))
            ));
            assert!(context.pcurves[0].is_none());
            assert!(context.pcurves[1].is_some());
            let tail = tail.expect("cache-first tail");
            assert_eq!(tail.extension, 7);
            assert!(!tail.flag);
            assert_eq!(tail.second_flag, Some(false));
        }
    }

    #[test]
    fn projection_layout_walks_both_tail_forms_at_both_widths() {
        for int_width in [4usize, 8] {
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
        for int_width in [4usize, 8] {
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
        for int_width in [4usize, 8] {
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
}
