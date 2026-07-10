// SPDX-License-Identifier: Apache-2.0
//! B-spline (`nubs`/`nurbs`) block decode for spline surfaces and procedural
//! curves.
//!
//! Spline surface (`spline`) and procedural curve (`intcurve`) records are
//! subtype-dispatched constructions this codec does not evaluate. They do,
//! however, carry a cached B-spline block — the surface's final face cache and
//! the curve's 3D cache — in a fixed inline grammar (spec §5), and that cache is
//! the exact geometry a downstream kernel or STEP writer consumes. This module
//! locates and decodes those blocks directly from the record's bytes.
//!
//! A block is introduced by a `0x0d`-tagged `nubs` (non-rational) or `nurbs`
//! (rational) marker. A **surface** block carries two degrees, four
//! periodic/singularity enums, U and V knot tables, and a control grid; a
//! **curve** block carries one degree, a closure enum, one knot table, and a
//! control polygon. The two are told apart by the token immediately after the
//! first degree (a second `0x04` degree for a surface, a `0x15` closure enum for
//! a curve), so scanning a record for the right kind never confuses a surface
//! cache with a 2D pcurve or a spine curve.
//!
//! Endpoint knot multiplicities are stored as `degree` (not `degree + 1`); the
//! clamped knot vector is recovered by adding one at each end. Control-point
//! x/y/z are model-space lengths converted centimetre→millimetre (×10); knots
//! and rational weights are not scaled. Surface control grids are stored
//! v-major (v outer, u inner) and are transposed to the IR's u-major order.

use cadmpeg_ir::geometry::{BlendCrossSection, BlendRadiusLaw, NurbsCurve, NurbsSurface};
use cadmpeg_ir::math::{Point2, Point3, Vector3};

/// Millimetres per ASM model-space length unit (centimetres).
const LEN_TO_MM: f64 = 10.0;

const NUBS_MARKER: &[u8] = b"\x0d\x04nubs";
const NURBS_MARKER: &[u8] = b"\x0d\x05nurbs";

fn read_i64(b: &[u8], p: usize) -> Option<i64> {
    b.get(p..p + 8).map(|s| {
        i64::from_le_bytes(
            s.try_into()
                .expect("invariant: b.get(p..p+8) is an 8-byte slice"),
        )
    })
}

fn read_f64(b: &[u8], p: usize) -> Option<f64> {
    b.get(p..p + 8).map(|s| {
        f64::from_le_bytes(
            s.try_into()
                .expect("invariant: b.get(p..p+8) is an 8-byte slice"),
        )
    })
}

/// Consume a `tag`-prefixed i64 at `*pos`, advancing past it.
fn take_tagged_i64(b: &[u8], pos: &mut usize, tag: u8) -> Option<i64> {
    if *b.get(*pos)? != tag {
        return None;
    }
    let v = read_i64(b, *pos + 1)?;
    *pos += 9;
    Some(v)
}

/// The B-spline marker at `pos`, if any: `(control-point dimension, byte length
/// of the marker, rational?)`.
fn marker_at(b: &[u8], pos: usize) -> Option<(usize, usize, bool)> {
    if b[pos..].starts_with(NUBS_MARKER) {
        Some((3, NUBS_MARKER.len(), false))
    } else if b[pos..].starts_with(NURBS_MARKER) {
        Some((4, NURBS_MARKER.len(), true))
    } else {
        None
    }
}

/// Positions of every `nubs`/`nurbs` marker in `b`, in order.
fn marker_positions(b: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    if b.len() < NUBS_MARKER.len() {
        return out;
    }
    for pos in 0..=b.len() - NUBS_MARKER.len() {
        if marker_at(b, pos).is_some() {
            out.push(pos);
        }
    }
    out
}

/// Read a knot table of `n` `(knot, multiplicity)` pairs, returning the expanded
/// (clamped) knot vector — each unique knot repeated by its stored multiplicity,
/// with one extra at the first and last to clamp the endpoints — and the pole
/// count `sum(mult) - (degree - 1)`.
fn read_knots(b: &[u8], pos: &mut usize, n: usize, degree: i64) -> Option<(Vec<f64>, usize)> {
    let mut knots = Vec::new();
    let mut mults = Vec::new();
    for _ in 0..n {
        if *b.get(*pos)? != 0x06 {
            return None;
        }
        knots.push(read_f64(b, *pos + 1)?);
        *pos += 9;
        mults.push(take_tagged_i64(b, pos, 0x04)?);
    }
    let sum: i64 = mults.iter().sum();
    let n_poles = sum - (degree - 1);
    if !(2..=100_000).contains(&n_poles) {
        return None;
    }
    let mut expanded = Vec::new();
    for (i, (kv, m)) in knots.iter().zip(&mults).enumerate() {
        let extra = i64::from(i == 0 || i == n - 1);
        for _ in 0..(*m + extra).max(0) {
            expanded.push(*kv);
        }
    }
    Some((expanded, n_poles as usize))
}

/// Read `count` control points of `cp_dims` doubles each at `*pos`. Returns the
/// scaled `(x, y, z)` positions and, for rational blocks, the weights.
fn read_control_points(
    b: &[u8],
    pos: &mut usize,
    count: usize,
    cp_dims: usize,
) -> Option<(Vec<Point3>, Option<Vec<f64>>)> {
    let mut points = Vec::with_capacity(count);
    let mut weights = if cp_dims == 4 {
        Some(Vec::with_capacity(count))
    } else {
        None
    };
    for _ in 0..count {
        let mut comps = [0.0f64; 4];
        for comp in comps.iter_mut().take(cp_dims) {
            if *b.get(*pos)? != 0x06 {
                return None;
            }
            *comp = read_f64(b, *pos + 1)?;
            *pos += 9;
        }
        points.push(Point3::new(
            comps[0] * LEN_TO_MM,
            comps[1] * LEN_TO_MM,
            comps[2] * LEN_TO_MM,
        ));
        if let Some(w) = weights.as_mut() {
            w.push(comps[3]);
        }
    }
    Some((points, weights))
}

/// CLOSURE enum value `2` denotes a periodic parametric direction.
fn is_periodic(enum_val: i64) -> bool {
    enum_val == 2
}

/// Decode a surface `nubs`/`nurbs` block at `marker_pos`, or `None` if the bytes
/// there are not a well-formed surface block.
struct DecodedSurfaceBlock {
    surface: NurbsSurface,
    end: usize,
}

fn decode_surface_block(b: &[u8], marker_pos: usize) -> Option<DecodedSurfaceBlock> {
    let (cp_dims, marker_len, _rational) = marker_at(b, marker_pos)?;
    let mut pos = marker_pos + marker_len;

    let degree_u = take_tagged_i64(b, &mut pos, 0x04)?;
    let degree_v = take_tagged_i64(b, &mut pos, 0x04)?;
    if !(1..=20).contains(&degree_u) || !(1..=20).contains(&degree_v) {
        return None;
    }
    // Some caches carry an optional scope identifier (`u`/`v`/`both`) before the
    // enum block; skip it so knot counts stay aligned.
    if b.get(pos) == Some(&0x0d) {
        let len = *b.get(pos + 1)? as usize;
        pos += 2 + len;
    }
    let mut enums = [0i64; 4];
    for e in &mut enums {
        *e = take_tagged_i64(b, &mut pos, 0x15)?;
    }
    let n_uniq_u = take_tagged_i64(b, &mut pos, 0x04)?;
    let n_uniq_v = take_tagged_i64(b, &mut pos, 0x04)?;
    if !(1..=1000).contains(&n_uniq_u) || !(1..=1000).contains(&n_uniq_v) {
        return None;
    }

    let (u_knots, n_poles_u) = read_knots(b, &mut pos, n_uniq_u as usize, degree_u)?;
    let (v_knots, n_poles_v) = read_knots(b, &mut pos, n_uniq_v as usize, degree_v)?;
    if n_poles_u.checked_mul(n_poles_v).is_none_or(|n| n > 200_000) {
        return None;
    }

    // Grid is stored v-major (v outer, u inner); transpose to the IR's u-major
    // order where index `u * v_count + v` is pole `(u, v)`.
    let (flat, flat_w) = read_control_points(b, &mut pos, n_poles_u * n_poles_v, cp_dims)?;
    let mut control_points = vec![Point3::new(0.0, 0.0, 0.0); n_poles_u * n_poles_v];
    let mut weights = flat_w.as_ref().map(|_| vec![0.0f64; n_poles_u * n_poles_v]);
    for v in 0..n_poles_v {
        for u in 0..n_poles_u {
            let file_idx = v * n_poles_u + u;
            let ir_idx = u * n_poles_v + v;
            control_points[ir_idx] = flat[file_idx];
            if let (Some(w), Some(fw)) = (weights.as_mut(), flat_w.as_ref()) {
                w[ir_idx] = fw[file_idx];
            }
        }
    }

    Some(DecodedSurfaceBlock {
        surface: NurbsSurface {
            u_degree: degree_u as u32,
            v_degree: degree_v as u32,
            u_knots,
            v_knots,
            u_count: n_poles_u as u32,
            v_count: n_poles_v as u32,
            control_points,
            weights,
            u_periodic: is_periodic(enums[0]),
            v_periodic: is_periodic(enums[1]),
        },
        end: pos,
    })
}

/// Decode a curve `nubs`/`nurbs` block at `marker_pos`, or `None` if the bytes
/// there are not a well-formed 3D curve block.
struct DecodedCurveBlock {
    curve: NurbsCurve,
    end: usize,
}

fn decode_curve_block(b: &[u8], marker_pos: usize) -> Option<DecodedCurveBlock> {
    let (cp_dims, marker_len, _rational) = marker_at(b, marker_pos)?;
    let mut pos = marker_pos + marker_len;

    let degree = take_tagged_i64(b, &mut pos, 0x04)?;
    if !(1..=20).contains(&degree) {
        return None;
    }
    let closure = take_tagged_i64(b, &mut pos, 0x15)?;
    let n_uniq = take_tagged_i64(b, &mut pos, 0x04)?;
    if !(1..=1000).contains(&n_uniq) {
        return None;
    }
    let (knots, n_poles) = read_knots(b, &mut pos, n_uniq as usize, degree)?;
    let (control_points, weights) = read_control_points(b, &mut pos, n_poles, cp_dims)?;

    Some(DecodedCurveBlock {
        curve: NurbsCurve {
            degree: degree as u32,
            knots,
            control_points,
            weights,
            periodic: is_periodic(closure),
        },
        end: pos,
    })
}

/// The decoded payload of a 2D `nubs` pcurve block.
pub struct NurbsPcurve {
    /// Curve degree.
    pub degree: u32,
    /// Full clamped knot vector.
    pub knots: Vec<f64>,
    /// UV control points. These are surface parameters, not model-space
    /// lengths, and are deliberately never scaled.
    pub control_points: Vec<Point2>,
    /// Whether the parameter curve is periodic.
    pub periodic: bool,
}

fn decode_pcurve_block(b: &[u8], marker_pos: usize) -> Option<NurbsPcurve> {
    let (_cp_dims, marker_len, rational) = marker_at(b, marker_pos)?;
    if rational {
        return None;
    }
    let mut pos = marker_pos + marker_len;
    let degree = take_tagged_i64(b, &mut pos, 0x04)?;
    if !(1..=20).contains(&degree) {
        return None;
    }
    let closure = take_tagged_i64(b, &mut pos, 0x15)?;
    let n_uniq = take_tagged_i64(b, &mut pos, 0x04)?;
    if !(1..=1000).contains(&n_uniq) {
        return None;
    }
    let (knots, n_poles) = read_knots(b, &mut pos, n_uniq as usize, degree)?;
    let mut control_points = Vec::with_capacity(n_poles);
    for _ in 0..n_poles {
        if *b.get(pos)? != 0x06 {
            return None;
        }
        let u = read_f64(b, pos + 1)?;
        pos += 9;
        if *b.get(pos)? != 0x06 {
            return None;
        }
        let v = read_f64(b, pos + 1)?;
        pos += 9;
        control_points.push(Point2::new(u, v));
    }
    Some(NurbsPcurve {
        degree: degree as u32,
        knots,
        control_points,
        periodic: is_periodic(closure),
    })
}

/// Decode the face-surface cache of a spline surface record: the LAST valid
/// surface block in the record (the final `setSurfaceShape` cache; earlier
/// blocks are support surfaces or 2D pcurves). Returns `None` when no surface
/// block is present or parseable.
pub fn decode_surface_cache(record_bytes: &[u8]) -> Option<NurbsSurface> {
    marker_positions(record_bytes)
        .into_iter()
        .filter_map(|pos| decode_surface_block(record_bytes, pos))
        .map(|decoded| decoded.surface)
        .next_back()
}

/// A decoded native procedural definition and the fit contract of its solved cache.
pub struct DecodedProceduralSurface {
    /// The native procedural surface construction (blend, sweep, loft, or
    /// taper family) decoded from its subtype-dispatched inline fields.
    pub definition: DecodedProceduralSurfaceDefinition,
    /// `surface_fit_tolerance` of the cached B-spline block, if present.
    /// `0.0` indicates fidelity to the procedural surface rather than
    /// identity with a primitive (spec §7.5).
    pub cache_fit_tolerance: Option<f64>,
}

/// Source-native procedural semantics before embedded geometry is assigned IR ids.
pub enum DecodedProceduralSurfaceDefinition {
    /// Translation of an embedded directrix along a length-bearing direction.
    Extrusion {
        /// Embedded directrix cache.
        directrix: NurbsCurve,
        /// Length-bearing sweep direction.
        direction: Vector3,
    },
    /// Rolling-ball blend with embedded support and spine caches.
    Blend {
        /// Embedded support caches in side order.
        supports: Box<[Option<NurbsSurface>; 2]>,
        /// Embedded center/spine curve.
        spine: Option<NurbsCurve>,
        /// Signed radius law.
        radius: BlendRadiusLaw,
        /// Blend cross-section family.
        cross_section: BlendCrossSection,
    },
}

/// Decode an inline `cyl_spl_sur` translational-extrusion definition.
pub fn decode_cyl_spl_sur(record_bytes: &[u8]) -> Option<DecodedProceduralSurface> {
    let marker = b"\x0f\x0d\x0bcyl_spl_sur";
    let start = record_bytes
        .windows(marker.len())
        .position(|w| w == marker)?;
    let span = subtype_span(record_bytes, start)?;
    let directrix = decode_curve_cache(span)?;

    let mut doubles = Vec::new();
    let mut direction = None;
    let mut pos = marker.len();
    while pos < span.len() {
        match span[pos] {
            0x06 if direction.is_none() => doubles.push(read_f64(span, pos + 1)?),
            0x14 if direction.is_none() => {
                direction = Some(Vector3::new(
                    read_f64(span, pos + 1)? * LEN_TO_MM,
                    read_f64(span, pos + 9)? * LEN_TO_MM,
                    read_f64(span, pos + 17)? * LEN_TO_MM,
                ));
            }
            _ => {}
        }
        pos = next_token(span, pos)?;
    }
    let _u_range = [*doubles.first()?, *doubles.get(1)?];
    let decoded_cache = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at))
        .next_back()?;
    let _v_range = [
        *decoded_cache.surface.v_knots.first()?,
        *decoded_cache.surface.v_knots.last()?,
    ];
    let cache_fit_tolerance = (span.get(decoded_cache.end) == Some(&0x06))
        .then(|| read_f64(span, decoded_cache.end + 1).map(|v| v * LEN_TO_MM))
        .flatten();

    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Extrusion {
            directrix,
            direction: direction?,
        },
        cache_fit_tolerance,
    })
}

fn decode_rb_blend_spl_sur(record_bytes: &[u8]) -> Option<DecodedProceduralSurface> {
    let marker = b"\x0f\x0d\x10rb_blend_spl_sur";
    let start = record_bytes
        .windows(marker.len())
        .position(|w| w == marker)?;
    let span = subtype_span(record_bytes, start)?;
    let cache = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at))
        .next_back()?;

    let mut support_count = 0usize;
    let mut radius_boundary = None;
    let mut pos = marker.len();
    while pos < cache.end {
        match span[pos] {
            0x0d | 0x0e => {
                let len = usize::from(*span.get(pos + 1)?);
                let name = span.get(pos + 2..pos + 2 + len)?;
                if [b"plane".as_slice(), b"sphere", b"cone", b"torus"].contains(&name) {
                    support_count += 1;
                }
            }
            0x15 if read_i64(span, pos + 1) == Some(-1) => radius_boundary = Some(pos),
            _ => {}
        }
        pos = next_token(span, pos)?;
    }
    let boundary = radius_boundary?;
    let mut radius_values = Vec::new();
    let mut pos = marker.len();
    while pos < boundary {
        if span[pos] == 0x06 {
            radius_values.push(read_f64(span, pos + 1)?);
        }
        pos = next_token(span, pos)?;
    }
    let end = *radius_values.last()? * LEN_TO_MM;
    let start = *radius_values.get(radius_values.len().checked_sub(2)?)? * LEN_TO_MM;
    let radius = if start == end {
        BlendRadiusLaw::Constant {
            signed_radius: start,
        }
    } else {
        BlendRadiusLaw::Linear { start, end }
    };
    let center_curve = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_curve_block(span, at))
        .map(|decoded| decoded.curve)
        .next_back();
    let mut support_caches = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at))
        .filter(|decoded| decoded.end < cache.end)
        .map(|decoded| decoded.surface);
    let supports = [
        (support_count > 0).then(|| support_caches.next()).flatten(),
        (support_count > 1).then(|| support_caches.next()).flatten(),
    ];
    let cache_fit_tolerance = (span.get(cache.end) == Some(&0x06))
        .then(|| read_f64(span, cache.end + 1).map(|v| v * LEN_TO_MM))
        .flatten();

    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Blend {
            supports: Box::new(supports),
            spine: center_curve,
            radius,
            cross_section: BlendCrossSection::Circular,
        },
        cache_fit_tolerance,
    })
}

/// Decode a native procedural definition, following nested subtype-table references.
pub fn decode_procedural_surface_resolving_refs(
    record_bytes: &[u8],
    active_bytes: &[u8],
) -> Option<DecodedProceduralSurface> {
    decode_procedural_resolving_refs(record_bytes, active_bytes, &mut Vec::new())
}

fn decode_procedural_resolving_refs(
    bytes: &[u8],
    active_bytes: &[u8],
    seen: &mut Vec<usize>,
) -> Option<DecodedProceduralSurface> {
    if let Some(decoded) = decode_cyl_spl_sur(bytes).or_else(|| decode_rb_blend_spl_sur(bytes)) {
        return Some(decoded);
    }
    let table = subtype_table(active_bytes);
    for index in subtype_refs(bytes) {
        if seen.contains(&index) {
            continue;
        }
        let target = *table.get(index)?;
        seen.push(index);
        if let Some(decoded) = decode_procedural_resolving_refs(
            subtype_span(active_bytes, target)?,
            active_bytes,
            seen,
        ) {
            return Some(decoded);
        }
    }
    None
}

/// Decode a surface cache from a carrier record, following the ASM subtype
/// table when the record stores a nested `ref N` instead of an inline cache.
/// `active_bytes` is the full active SAB slice; `N` indexes its non-`ref`
/// subtype openings in byte order.
pub fn decode_surface_cache_resolving_refs(
    record_bytes: &[u8],
    active_bytes: &[u8],
) -> Option<NurbsSurface> {
    decode_cache_resolving_refs(
        record_bytes,
        active_bytes,
        &mut Vec::new(),
        decode_surface_cache,
    )
}

/// Decode the 3D curve cache of a procedural curve record: the FIRST valid curve
/// block (surface and 2D pcurve blocks in the record are skipped because they do
/// not parse as a 3D curve block). Returns `None` when none is present.
pub fn decode_curve_cache(record_bytes: &[u8]) -> Option<NurbsCurve> {
    marker_positions(record_bytes)
        .into_iter()
        .find_map(|pos| decode_curve_block(record_bytes, pos).map(|decoded| decoded.curve))
}

/// Decode a curve cache from a carrier record, resolving nested ASM subtype
/// references through the active slice's subtype table.
pub fn decode_curve_cache_resolving_refs(
    record_bytes: &[u8],
    active_bytes: &[u8],
) -> Option<NurbsCurve> {
    decode_cache_resolving_refs(
        record_bytes,
        active_bytes,
        &mut Vec::new(),
        decode_curve_cache,
    )
}

/// A procedural curve cache together with its native subtype and fit contract.
pub struct DecodedProceduralCurve {
    /// The cached B-spline curve (control points scaled centimetre→
    /// millimetre; knots and weights unscaled).
    pub curve: NurbsCurve,
    /// The `intcurve` subtype record name (`exact_int_cur`, `off_int_cur`,
    /// `proj_int_cur`, `int_int_cur`, `helix_int_cur`, `sss_int_cur`, ...).
    pub native_kind: String,
    /// `surface_fit_tolerance` of the cached B-spline block, if present
    /// (spec §7.5).
    pub cache_fit_tolerance: Option<f64>,
}

/// Decode a procedural 3D curve cache while following subtype-table references.
pub fn decode_procedural_curve_resolving_refs(
    record_bytes: &[u8],
    active_bytes: &[u8],
) -> Option<DecodedProceduralCurve> {
    decode_procedural_curve_recursive(record_bytes, active_bytes, &mut Vec::new())
}

fn decode_procedural_curve_recursive(
    bytes: &[u8],
    active_bytes: &[u8],
    seen: &mut Vec<usize>,
) -> Option<DecodedProceduralCurve> {
    for position in marker_positions(bytes) {
        if let Some(decoded) = decode_curve_block(bytes, position) {
            let cache_fit_tolerance = (bytes.get(decoded.end) == Some(&0x06))
                .then(|| read_f64(bytes, decoded.end + 1).map(|value| value * LEN_TO_MM))
                .flatten();
            return Some(DecodedProceduralCurve {
                curve: decoded.curve,
                native_kind: first_construction_subtype(bytes)
                    .unwrap_or_else(|| "intcurve".to_string()),
                cache_fit_tolerance,
            });
        }
    }
    let table = subtype_table(active_bytes);
    for index in subtype_refs(bytes) {
        if seen.contains(&index) {
            continue;
        }
        let target = *table.get(index)?;
        seen.push(index);
        if let Some(decoded) = decode_procedural_curve_recursive(
            subtype_span(active_bytes, target)?,
            active_bytes,
            seen,
        ) {
            return Some(decoded);
        }
    }
    None
}

fn first_construction_subtype(bytes: &[u8]) -> Option<String> {
    for pos in 0..bytes.len().saturating_sub(3) {
        if bytes[pos] != 0x0f || !matches!(bytes.get(pos + 1), Some(0x0d | 0x0e)) {
            continue;
        }
        let len = usize::from(*bytes.get(pos + 2)?);
        let name = bytes.get(pos + 3..pos + 3 + len)?;
        if name != b"ref" {
            return Some(String::from_utf8_lossy(name).into_owned());
        }
    }
    None
}

fn decode_cache_resolving_refs<T>(
    bytes: &[u8],
    active_bytes: &[u8],
    seen: &mut Vec<usize>,
    decode_inline: fn(&[u8]) -> Option<T>,
) -> Option<T> {
    if let Some(decoded) = decode_inline(bytes) {
        return Some(decoded);
    }
    let table = subtype_table(active_bytes);
    for index in subtype_refs(bytes) {
        if seen.contains(&index) {
            continue;
        }
        let target = *table.get(index)?;
        seen.push(index);
        if let Some(decoded) = decode_cache_resolving_refs(
            subtype_span(active_bytes, target)?,
            active_bytes,
            seen,
            decode_inline,
        ) {
            return Some(decoded);
        }
    }
    None
}

fn subtype_table(bytes: &[u8]) -> Vec<usize> {
    let mut table = Vec::new();
    for pos in 0..bytes.len().saturating_sub(4) {
        if bytes[pos] != 0x0f || !matches!(bytes.get(pos + 1), Some(0x0d | 0x0e)) {
            continue;
        }
        let len = *bytes.get(pos + 2).unwrap_or(&0) as usize;
        let Some(name) = bytes.get(pos + 3..pos + 3 + len) else {
            continue;
        };
        if name != b"ref" && name.iter().all(|b| (0x21..=0x7e).contains(b)) {
            table.push(pos);
        }
    }
    table
}

fn subtype_refs(bytes: &[u8]) -> Vec<usize> {
    let mut refs = Vec::new();
    let marker = b"\x0f\x0d\x03ref\x04";
    for pos in 0..=bytes.len().saturating_sub(marker.len() + 8) {
        if bytes[pos..].starts_with(marker) {
            if let Some(index) = read_i64(bytes, pos + marker.len()) {
                if index >= 0 {
                    refs.push(index as usize);
                }
            }
        }
    }
    refs
}

fn subtype_span(bytes: &[u8], start: usize) -> Option<&[u8]> {
    let mut depth = 0usize;
    let mut pos = start;
    while pos < bytes.len() {
        match bytes[pos] {
            0x0f => depth += 1,
            0x10 => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return bytes.get(start..=pos);
                }
            }
            _ => {}
        }
        pos = next_token(bytes, pos)?;
    }
    None
}

fn next_token(bytes: &[u8], pos: usize) -> Option<usize> {
    let tag = *bytes.get(pos)?;
    let fixed = match tag {
        0x02 => 2,
        0x03 => 3,
        0x04 | 0x06 | 0x0c | 0x15 | 0x17 => 9,
        0x05 => 5,
        0x0a | 0x0b | 0x0f | 0x10 | 0x11 => 1,
        0x13 | 0x14 => 25,
        0x16 => 17,
        0x07 | 0x0d | 0x0e => 2 + usize::from(*bytes.get(pos + 1)?),
        0x08 => {
            3 + usize::from(u16::from_le_bytes(
                bytes.get(pos + 1..pos + 3)?.try_into().ok()?,
            ))
        }
        0x09 | 0x12 => {
            5 + usize::try_from(u32::from_le_bytes(
                bytes.get(pos + 1..pos + 5)?.try_into().ok()?,
            ))
            .ok()?
        }
        _ => return None,
    };
    let next = pos.checked_add(fixed)?;
    (next <= bytes.len()).then_some(next)
}

/// Decode the first well-formed 2D `nubs` block in a pcurve record.
pub fn decode_pcurve_cache(record_bytes: &[u8]) -> Option<NurbsPcurve> {
    marker_positions(record_bytes)
        .into_iter()
        .find_map(|pos| decode_pcurve_block(record_bytes, pos))
}

/// Decode a 2D pcurve cache, resolving a nested ASM subtype-table reference
/// when the pcurve record delegates its UV carrier to an `intcurve` block.
pub fn decode_pcurve_cache_resolving_refs(
    record_bytes: &[u8],
    active_bytes: &[u8],
) -> Option<NurbsPcurve> {
    decode_cache_resolving_refs(
        record_bytes,
        active_bytes,
        &mut Vec::new(),
        decode_pcurve_cache,
    )
}

/// Decode the UV cache carried by a ref-form pcurve's `intcurve` entity. The
/// first curve-shaped `nubs` block is the 3D edge carrier; the subsequent
/// well-formed 2D block is the pcurve.
pub fn decode_intcurve_pcurve_cache(record_bytes: &[u8]) -> Option<NurbsPcurve> {
    let mut saw_curve = false;
    for position in marker_positions(record_bytes) {
        if !saw_curve && decode_curve_block(record_bytes, position).is_some() {
            saw_curve = true;
            continue;
        }
        if saw_curve {
            if let Some(pcurve) = decode_pcurve_block(record_bytes, position) {
                return Some(pcurve);
            }
        }
    }
    None
}

/// Decode an intcurve-carried UV cache, following its construction subtype
/// reference when the caches live in the subtype table rather than inline.
pub fn decode_intcurve_pcurve_cache_resolving_refs(
    record_bytes: &[u8],
    active_bytes: &[u8],
) -> Option<NurbsPcurve> {
    decode_cache_resolving_refs(
        record_bytes,
        active_bytes,
        &mut Vec::new(),
        decode_intcurve_pcurve_cache,
    )
}
