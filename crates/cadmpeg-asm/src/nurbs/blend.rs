// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::items_after_test_module)]
//! Blend spline-surface decoders (cylindrical, rolling-ball, variable, vertex, and rb blends).

use crate::nurbs::core::{
    curve_block, decode_curve_block, decode_owned_curve_cache_at,
    decode_owned_curve_cache_resolving_refs_at, decode_owned_surface_cache_at,
    decode_owned_surface_cache_resolving_refs_at, decode_surface_block, surface_block,
};
use crate::nurbs::pcurve::pcurve_block_with_end;
use crate::nurbs::proc_curve::{
    decode_embedded_surface_with_ranges, decode_par_int_cur_isoline,
    embedded_base_curve_resolving_refs, embedded_surface, embedded_surface_with_ranges,
    optional_embedded_surface_with_bounds, par_int_cur_isoline,
};
use crate::nurbs::proc_surface::{
    decode_nullable_embedded_pcurve, nullable_embedded_pcurve, revision_surface_tail,
    DecodedProceduralSurface, DecodedProceduralSurfaceDefinition, EmbeddedRollingBall,
    EmbeddedRollingBallRadiusSelector, EmbeddedRollingBallSide, EmbeddedRollingBallThirdSide,
    EmbeddedVariableBlend, EmbeddedVertexBlend, EmbeddedVertexBlendBoundary,
    EmbeddedVertexBlendBoundaryGeometry, RevisionSurfaceTail,
};
use crate::nurbs::reader::{
    marker_at, take_bool, take_f64, take_native_ident, take_native_string, take_native_vec3,
    take_optional_range_value, take_tagged_int, unit_vector, LEN_TO_MM,
};
use crate::nurbs::subtypes::{subtype_span, SubtypeTables};
use crate::nurbs::toks::{self, Cur, SubtypeTable};
use crate::sab::Token;
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point3, Vector3};

/// Decode an inline `cyl_spl_sur` translational-extrusion definition.
pub(crate) fn cyl_spl_sur(
    toks: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    let names = ["cyl_spl_sur", "cylsur"];
    let (start, _) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    // The revision-gated layout stores the directrix as a nested intcurve scope
    // and ends with the shared revision-gated surface tail, so its cache is
    // located by parsing that tail. The compact layout has no tail: its optional
    // final surface cache is the last surface block in the scope.
    let (
        directrix,
        parameter_interval,
        direction,
        native_position,
        cache_fit_tolerance,
        revision_form,
    ) = if matches!(cur.peek(), Some(Token::Long(_))) {
        let revision = cur.take_long()?;
        // Sense flag of the embedded directrix curve. It is the carrier's
        // whole boolean run, so it travels in the revision form's `flags`.
        let directrix_start = cur.pos();
        let directrix_sense = if matches!(cur.peek(), Some(Token::True | Token::False)) {
            matches!(cur.peek(), Some(Token::True))
        } else {
            (cur.take_ident()? == "intcurve").then_some(())?;
            let sense = cur.take_bool()?;
            cur.set_pos(directrix_start);
            sense
        };
        let table = resolver?;
        let directrix = embedded_base_curve_resolving_refs(&mut cur, table)?;
        let start = cur.take_optional_range_value()?;
        let end = cur.take_optional_range_value()?;
        let interval = [start?, end?];
        let direction = cur.take_vector3()?;
        let native_position = cur.take_position()?;
        let RevisionSurfaceTail {
            enumeration: tail_enum,
            fit_tolerance,
            solved_cache_domains: _,
            parameterization,
            discontinuities,
            tail_flag,
        } = revision_surface_tail(&mut cur)?;
        cur.at_scope_end().then_some(())?;
        (
            directrix,
            interval,
            direction,
            native_position,
            fit_tolerance,
            Some(cadmpeg_ir::geometry::RevisionSurfaceForm {
                revision,
                support_bounds: [None; 4],
                reference_endpoints: [None; 2],
                second_endpoints: [None; 2],
                flags: vec![directrix_sense],
                tail_enum,
                tail_parameterization: parameterization,
                discontinuities,
                tail_flag,
                trailing_flags: Vec::new(),
            }),
        )
    } else {
        let directrix = crate::nurbs::core::curve_cache(span)?;
        let interval = [cur.take_f64()?, cur.take_f64()?];
        let direction = cur.take_vector3()?;
        let native_position = cur.take_position()?;
        let cache_fit_tolerance = toks::owned_marker_positions(span)
            .into_iter()
            .filter_map(|at| surface_block(span, at))
            .next_back()
            .and_then(|(_, cache_end)| match span.get(cache_end) {
                Some(Token::Double(value)) => Some(*value * LEN_TO_MM),
                _ => None,
            });
        (
            directrix,
            interval,
            direction,
            native_position,
            cache_fit_tolerance,
            None,
        )
    };

    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Extrusion {
            directrix,
            parameter_interval,
            direction: Vector3::new(
                direction[0] * LEN_TO_MM,
                direction[1] * LEN_TO_MM,
                direction[2] * LEN_TO_MM,
            ),
            native_position: Point3::new(
                native_position[0] * LEN_TO_MM,
                native_position[1] * LEN_TO_MM,
                native_position[2] * LEN_TO_MM,
            ),
            revision_form,
        },
        cache_fit_tolerance,
    })
}

pub(crate) fn decode_rolling_ball_side(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
    reference_context: Option<(&[u8], &SubtypeTables)>,
) -> Option<EmbeddedRollingBallSide> {
    use cadmpeg_ir::geometry::VariableBlendSupportKind;
    let support_kind = match take_native_string(bytes, position, int_width)?.as_str() {
        "blend_support_cos_curve" | "blendsupcos" => VariableBlendSupportKind::CosineCurve,
        "blend_support_curve" | "blendsupcur" => VariableBlendSupportKind::Curve,
        "blend_support_point_curve" | "blendsuppnt" => VariableBlendSupportKind::PointCurve,
        "blend_support_surface" | "blendsupsur" => VariableBlendSupportKind::Surface,
        "blend_support_zero_curve" | "blendsupzro" => VariableBlendSupportKind::ZeroCurve,
        _ => return None,
    };
    let (surface, surface_ranges) =
        decode_optional_rolling_ball_surface(bytes, position, int_width, reference_context)?;
    let saved = *position;
    let (curve, curve_range) =
        if take_native_ident(bytes, position).as_deref() == Some("null_curve") {
            (None, [None, None])
        } else {
            *position = saved;
            let curve = decode_rolling_ball_curve(bytes, position, int_width, reference_context)?;
            (Some(curve.geometry), curve.parameter_range)
        };
    let pcurve = decode_nullable_embedded_pcurve(bytes, position, int_width)?;
    let location = take_native_vec3(bytes, position, 0x13)?;
    let secondary_pcurve = decode_nullable_embedded_pcurve(bytes, position, int_width)?;
    let extension_start = *position;
    let extension_fields = (|| {
        let extension = take_tagged_int(bytes, position, 0x04, int_width)?;
        let tertiary = decode_nullable_embedded_pcurve(bytes, position, int_width)?;
        Some((extension, tertiary))
    })();
    let (extension, tertiary_pcurve) = match extension_fields {
        Some((extension, tertiary)) => (Some(extension), tertiary),
        None => {
            *position = extension_start;
            (None, None)
        }
    };
    Some(EmbeddedRollingBallSide {
        support_kind,
        surface,
        surface_ranges,
        curve,
        curve_range,
        pcurve,
        location: Point3::new(
            location[0] * LEN_TO_MM,
            location[1] * LEN_TO_MM,
            location[2] * LEN_TO_MM,
        ),
        secondary_pcurve,
        extension,
        tertiary_pcurve,
    })
}

/// A decoded support-surface slot: the surface, absent when the slot holds
/// `null_surface`, and its `[[u0, u1], [v0, v1]]` parameter bounds.
pub(crate) type OptionalSupportSurface = (Option<SurfaceGeometry>, [[Option<f64>; 2]; 2]);

/// A support-surface slot: the `null_surface` ident, or an embedded surface and
/// its parameter bounds.
pub(crate) fn decode_optional_rolling_ball_surface(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
    reference_context: Option<(&[u8], &SubtypeTables)>,
) -> Option<OptionalSupportSurface> {
    let saved = *position;
    if take_native_ident(bytes, position).as_deref() == Some("null_surface") {
        return Some((None, [[None, None], [None, None]]));
    }
    *position = saved;
    decode_rolling_ball_surface(bytes, position, int_width, reference_context)
        .map(|(surface, ranges)| (Some(surface), ranges))
}

pub(crate) fn decode_rolling_ball_surface(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
    reference_context: Option<(&[u8], &SubtypeTables)>,
) -> Option<(SurfaceGeometry, [[Option<f64>; 2]; 2])> {
    let saved = *position;
    let kind = take_native_ident(bytes, position)?;
    if kind == "spline" {
        if marker_at(bytes, *position).is_some() {
            let surface = decode_surface_block(bytes, *position, int_width)?;
            *position = surface.end;
            let ranges = decode_surface_ranges(bytes, position)?;
            return Some((SurfaceGeometry::Nurbs(surface.surface), ranges));
        }
        take_bool(bytes, position)?;
        let scope = subtype_span(bytes, *position, int_width)?;
        let surface = reference_context
            .and_then(|(active_bytes, tables)| {
                decode_owned_surface_cache_resolving_refs_at(scope, active_bytes, tables, int_width)
            })
            .or_else(|| decode_owned_surface_cache_at(scope, int_width))?;
        *position += scope.len();
        let ranges = decode_surface_ranges(bytes, position)?;
        return Some((SurfaceGeometry::Nurbs(surface), ranges));
    }
    *position = saved;
    decode_embedded_surface_with_ranges(bytes, position, int_width)
}

pub(crate) fn decode_surface_ranges(
    bytes: &[u8],
    position: &mut usize,
) -> Option<[[Option<f64>; 2]; 2]> {
    Some([
        [
            take_optional_range_value(bytes, position)?,
            take_optional_range_value(bytes, position)?,
        ],
        [
            take_optional_range_value(bytes, position)?,
            take_optional_range_value(bytes, position)?,
        ],
    ])
}

pub(crate) struct DecodedRollingBallCurve {
    pub(crate) geometry: CurveGeometry,
    pub(crate) parameter_range: [Option<f64>; 2],
}

pub(crate) fn decode_rolling_ball_curve(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
    reference_context: Option<(&[u8], &SubtypeTables)>,
) -> Option<DecodedRollingBallCurve> {
    if marker_at(bytes, *position).is_some() {
        let curve = decode_curve_block(bytes, *position, int_width)?;
        *position = curve.end;
        let parameter_range = [
            take_optional_range_value(bytes, position)?,
            take_optional_range_value(bytes, position)?,
        ];
        return Some(DecodedRollingBallCurve {
            geometry: CurveGeometry::Nurbs(curve.curve),
            parameter_range,
        });
    }
    let kind = take_native_ident(bytes, position)?;
    if kind == "intcurve" {
        take_bool(bytes, position)?;
        let scope = subtype_span(bytes, *position, int_width)?;
        let curve = reference_context
            .and_then(|(active_bytes, tables)| {
                decode_owned_curve_cache_resolving_refs_at(scope, active_bytes, tables, int_width)
            })
            .or_else(|| decode_owned_curve_cache_at(scope, int_width))
            .or_else(|| decode_par_int_cur_isoline(scope, int_width, reference_context))?;
        *position += scope.len();
        let parameter_range = [
            take_optional_range_value(bytes, position)?,
            take_optional_range_value(bytes, position)?,
        ];
        return Some(DecodedRollingBallCurve {
            geometry: CurveGeometry::Nurbs(curve),
            parameter_range,
        });
    }
    let geometry = match kind.as_str() {
        "straight" => {
            let origin = take_native_vec3(bytes, position, 0x13)?;
            let direction = take_native_vec3(bytes, position, 0x14)?;
            CurveGeometry::Line {
                origin: Point3::new(
                    origin[0] * LEN_TO_MM,
                    origin[1] * LEN_TO_MM,
                    origin[2] * LEN_TO_MM,
                ),
                direction: unit_vector(Vector3::new(direction[0], direction[1], direction[2]))?,
            }
        }
        "ellipse" => {
            let center = take_native_vec3(bytes, position, 0x13)?;
            let axis = take_native_vec3(bytes, position, 0x14)?;
            let reference = take_native_vec3(bytes, position, 0x14)?;
            let ratio = take_f64(bytes, position)?;
            let reference = Vector3::new(reference[0], reference[1], reference[2]);
            let major_radius = reference.norm() * LEN_TO_MM;
            if (ratio.abs() - 1.0).abs() <= f64::EPSILON {
                CurveGeometry::Circle {
                    center: Point3::new(
                        center[0] * LEN_TO_MM,
                        center[1] * LEN_TO_MM,
                        center[2] * LEN_TO_MM,
                    ),
                    axis: unit_vector(Vector3::new(axis[0], axis[1], axis[2]))?,
                    ref_direction: unit_vector(reference)?,
                    radius: major_radius,
                }
            } else {
                CurveGeometry::Ellipse {
                    center: Point3::new(
                        center[0] * LEN_TO_MM,
                        center[1] * LEN_TO_MM,
                        center[2] * LEN_TO_MM,
                    ),
                    axis: unit_vector(Vector3::new(axis[0], axis[1], axis[2]))?,
                    major_direction: unit_vector(reference)?,
                    major_radius,
                    minor_radius: major_radius * ratio.abs(),
                }
            }
        }
        "degenerate_curve" => {
            let point = take_native_vec3(bytes, position, 0x13)?;
            CurveGeometry::Degenerate {
                point: Point3::new(
                    point[0] * LEN_TO_MM,
                    point[1] * LEN_TO_MM,
                    point[2] * LEN_TO_MM,
                ),
            }
        }
        _ => return None,
    };
    let parameter_range = [
        take_optional_range_value(bytes, position)?,
        take_optional_range_value(bytes, position)?,
    ];
    Some(DecodedRollingBallCurve {
        geometry,
        parameter_range,
    })
}

/// Decode one rolling-ball support side. Token-space counterpart of
/// [`decode_rolling_ball_side`].
pub(crate) fn rolling_ball_side(
    cur: &mut Cur<'_>,
    reference_context: Option<&SubtypeTable>,
) -> Option<EmbeddedRollingBallSide> {
    use cadmpeg_ir::geometry::VariableBlendSupportKind;
    let support_kind = match cur.take_str()? {
        "blend_support_cos_curve" | "blendsupcos" => VariableBlendSupportKind::CosineCurve,
        "blend_support_curve" | "blendsupcur" => VariableBlendSupportKind::Curve,
        "blend_support_point_curve" | "blendsuppnt" => VariableBlendSupportKind::PointCurve,
        "blend_support_surface" | "blendsupsur" => VariableBlendSupportKind::Surface,
        "blend_support_zero_curve" | "blendsupzro" => VariableBlendSupportKind::ZeroCurve,
        _ => return None,
    };
    let (surface, surface_ranges) = optional_rolling_ball_surface(cur, reference_context)?;
    let saved = cur.pos();
    let (curve, curve_range) = if cur.take_ident() == Some("null_curve") {
        (None, [None, None])
    } else {
        cur.set_pos(saved);
        let curve = rolling_ball_curve(cur, reference_context)?;
        (Some(curve.geometry), curve.parameter_range)
    };
    let pcurve = nullable_embedded_pcurve(cur)?;
    let location = cur.take_position()?;
    let secondary_pcurve = nullable_embedded_pcurve(cur)?;
    let extension_start = cur.pos();
    let extension_fields = (|| {
        let extension = cur.take_long()?;
        let tertiary = nullable_embedded_pcurve(cur)?;
        Some((extension, tertiary))
    })();
    let (extension, tertiary_pcurve) = match extension_fields {
        Some((extension, tertiary)) => (Some(extension), tertiary),
        None => {
            cur.set_pos(extension_start);
            (None, None)
        }
    };
    Some(EmbeddedRollingBallSide {
        support_kind,
        surface,
        surface_ranges,
        curve,
        curve_range,
        pcurve,
        location: Point3::new(
            location[0] * LEN_TO_MM,
            location[1] * LEN_TO_MM,
            location[2] * LEN_TO_MM,
        ),
        secondary_pcurve,
        extension,
        tertiary_pcurve,
    })
}

/// A support-surface slot: the `null_surface` ident, or an embedded surface and
/// its parameter bounds. Token-space counterpart of
/// [`decode_optional_rolling_ball_surface`].
pub(crate) fn optional_rolling_ball_surface(
    cur: &mut Cur<'_>,
    reference_context: Option<&SubtypeTable>,
) -> Option<OptionalSupportSurface> {
    let saved = cur.pos();
    if cur.take_ident() == Some("null_surface") {
        return Some((None, [[None, None], [None, None]]));
    }
    cur.set_pos(saved);
    rolling_ball_surface(cur, reference_context).map(|(surface, ranges)| (Some(surface), ranges))
}

/// Decode one rolling-ball support surface. Token-space counterpart of
/// [`decode_rolling_ball_surface`].
pub(crate) fn rolling_ball_surface(
    cur: &mut Cur<'_>,
    reference_context: Option<&SubtypeTable>,
) -> Option<(SurfaceGeometry, [[Option<f64>; 2]; 2])> {
    let toks = cur.toks();
    let saved = cur.pos();
    let kind = cur.take_ident()?;
    if kind == "spline" {
        if toks::marker_at(toks, cur.pos()).is_some() {
            let (surface, surface_end) = surface_block(toks, cur.pos())?;
            cur.set_pos(surface_end);
            let ranges = surface_ranges(cur)?;
            return Some((SurfaceGeometry::Nurbs(surface), ranges));
        }
        cur.take_bool()?;
        let scope = toks::subtype_span(toks, cur.pos())?;
        let surface = reference_context
            .and_then(|table| crate::nurbs::core::owned_surface_cache_resolving_refs(scope, table))
            .or_else(|| crate::nurbs::core::owned_surface_cache(scope))?;
        cur.set_pos(cur.pos() + scope.len());
        let ranges = surface_ranges(cur)?;
        return Some((SurfaceGeometry::Nurbs(surface), ranges));
    }
    cur.set_pos(saved);
    embedded_surface_with_ranges(cur)
}

/// Four optional U/V range bounds. Token-space counterpart of
/// [`decode_surface_ranges`].
pub(crate) fn surface_ranges(cur: &mut Cur<'_>) -> Option<[[Option<f64>; 2]; 2]> {
    Some([
        [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ],
        [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ],
    ])
}

/// Decode one rolling-ball curve slot. Token-space counterpart of
/// [`decode_rolling_ball_curve`].
pub(crate) fn rolling_ball_curve(
    cur: &mut Cur<'_>,
    reference_context: Option<&SubtypeTable>,
) -> Option<DecodedRollingBallCurve> {
    let toks = cur.toks();
    if toks::marker_at(toks, cur.pos()).is_some() {
        let (curve, curve_end) = curve_block(toks, cur.pos())?;
        cur.set_pos(curve_end);
        let parameter_range = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        return Some(DecodedRollingBallCurve {
            geometry: CurveGeometry::Nurbs(curve),
            parameter_range,
        });
    }
    let kind = cur.take_ident()?;
    if kind == "intcurve" {
        cur.take_bool()?;
        let scope = toks::subtype_span(toks, cur.pos())?;
        let curve = reference_context
            .and_then(|table| crate::nurbs::core::owned_curve_cache_resolving_refs(scope, table))
            .or_else(|| crate::nurbs::core::owned_curve_cache(scope))
            .or_else(|| par_int_cur_isoline(scope, reference_context))?;
        cur.set_pos(cur.pos() + scope.len());
        let parameter_range = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        return Some(DecodedRollingBallCurve {
            geometry: CurveGeometry::Nurbs(curve),
            parameter_range,
        });
    }
    let geometry = match kind {
        "straight" => {
            let origin = cur.take_position()?;
            let direction = cur.take_vector3()?;
            CurveGeometry::Line {
                origin: Point3::new(
                    origin[0] * LEN_TO_MM,
                    origin[1] * LEN_TO_MM,
                    origin[2] * LEN_TO_MM,
                ),
                direction: unit_vector(Vector3::new(direction[0], direction[1], direction[2]))?,
            }
        }
        "ellipse" => {
            let center = cur.take_position()?;
            let axis = cur.take_vector3()?;
            let reference = cur.take_vector3()?;
            let ratio = cur.take_f64()?;
            let reference = Vector3::new(reference[0], reference[1], reference[2]);
            let major_radius = reference.norm() * LEN_TO_MM;
            if (ratio.abs() - 1.0).abs() <= f64::EPSILON {
                CurveGeometry::Circle {
                    center: Point3::new(
                        center[0] * LEN_TO_MM,
                        center[1] * LEN_TO_MM,
                        center[2] * LEN_TO_MM,
                    ),
                    axis: unit_vector(Vector3::new(axis[0], axis[1], axis[2]))?,
                    ref_direction: unit_vector(reference)?,
                    radius: major_radius,
                }
            } else {
                CurveGeometry::Ellipse {
                    center: Point3::new(
                        center[0] * LEN_TO_MM,
                        center[1] * LEN_TO_MM,
                        center[2] * LEN_TO_MM,
                    ),
                    axis: unit_vector(Vector3::new(axis[0], axis[1], axis[2]))?,
                    major_direction: unit_vector(reference)?,
                    major_radius,
                    minor_radius: major_radius * ratio.abs(),
                }
            }
        }
        "degenerate_curve" => {
            let point = cur.take_position()?;
            CurveGeometry::Degenerate {
                point: Point3::new(
                    point[0] * LEN_TO_MM,
                    point[1] * LEN_TO_MM,
                    point[2] * LEN_TO_MM,
                ),
            }
        }
        _ => return None,
    };
    let parameter_range = [
        cur.take_optional_range_value()?,
        cur.take_optional_range_value()?,
    ];
    Some(DecodedRollingBallCurve {
        geometry,
        parameter_range,
    })
}

fn rolling_ball_third_side(cur: &mut Cur<'_>) -> Option<EmbeddedRollingBallThirdSide> {
    let label = cur.take_str()?.to_string();
    let surface = embedded_surface(cur)?;
    let (curve, curve_end) = curve_block(cur.toks(), cur.pos())?;
    cur.set_pos(curve_end);
    let pcurve = nullable_embedded_pcurve(cur)?;
    let direction = cur.take_vector3()?;
    let secondary_pcurve = nullable_embedded_pcurve(cur)?;
    let extension = cur.take_long()?;
    let tertiary_pcurve = nullable_embedded_pcurve(cur)?;
    let flag = cur.take_bool()?;
    Some(EmbeddedRollingBallThirdSide {
        label,
        surface,
        curve,
        pcurve,
        direction: Vector3::new(direction[0], direction[1], direction[2]),
        secondary_pcurve,
        extension,
        tertiary_pcurve,
        flag,
    })
}

fn blend_value_name(cur: &mut Cur<'_>) -> Option<String> {
    let saved = cur.pos();
    if let Some(value) = cur.take_str() {
        return Some(value.to_string());
    }
    cur.set_pos(saved);
    cur.take_ident().map(str::to_string)
}

fn variable_blend_value(
    cur: &mut Cur<'_>,
    modern: bool,
    depth: usize,
) -> Option<cadmpeg_ir::geometry::VariableBlendValue> {
    use cadmpeg_ir::geometry::{
        LoftBridgeToken, VariableBlendInterpolationPoint, VariableBlendValue,
        VariableBlendValuePayload,
    };
    if depth > 32 {
        return None;
    }
    let name = blend_value_name(cur)?;
    let discriminator = if matches!(cur.peek(), Some(Token::Long(_))) {
        cur.take_long()?
    } else {
        1
    };
    let calibrated = cur.take_enum()?;
    let modern_flag = if modern { cur.take_bool()? } else { false };
    let payload = match name.as_str() {
        "fixed_width" => VariableBlendValuePayload::FixedWidth {
            parameters: [cur.take_f64()?, cur.take_f64()?],
            width: cur.take_f64()?,
        },
        "two_ends" => VariableBlendValuePayload::TwoEnds {
            parameters: [cur.take_f64()?, cur.take_f64()?],
            radii: [cur.take_f64()? * LEN_TO_MM, cur.take_f64()? * LEN_TO_MM],
        },
        // The payload is the law-domain parameter range and one offset, so the
        // second field is a parameter and only the third is a length. The
        // sub-discriminator selects no layout here; it is still read and written
        // as the format stores it, and no value outside `0` and `1` is defined.
        "edge_offset" if matches!(discriminator, 0 | 1) => VariableBlendValuePayload::EdgeOffset {
            scalars: vec![cur.take_f64()?, cur.take_f64()?],
            lengths: vec![cur.take_f64()? * LEN_TO_MM],
        },
        "functional" => {
            let parameter = cur.take_f64()?;
            let radius = cur.take_f64()? * LEN_TO_MM;
            let (function, end) = pcurve_block_with_end(cur.toks(), cur.pos())?;
            cur.set_pos(end);
            let terminal = if matches!(cur.peek(), Some(Token::Double(_))) {
                LoftBridgeToken::Double(cur.take_f64()?)
            } else {
                LoftBridgeToken::Text(blend_value_name(cur)?)
            };
            VariableBlendValuePayload::Functional {
                parameter,
                radius,
                function: PcurveGeometry::Nurbs {
                    degree: function.degree,
                    knots: function.knots,
                    control_points: function.control_points,
                    weights: function.weights,
                    periodic: function.periodic,
                },
                terminal,
            }
        }
        "const" => VariableBlendValuePayload::Constant {
            parameters: [cur.take_f64()?, cur.take_f64()?],
            radius: cur.take_f64()? * LEN_TO_MM,
            variable_chamfer: cur.take_enum()?,
            chamfer_type: cur.take_enum()?,
            nested: Box::new(variable_blend_value(cur, modern, depth + 1)?),
        },
        "interp" => {
            let parameter = cur.take_f64()?;
            let radius = cur.take_f64()? * LEN_TO_MM;
            let (function, end) = pcurve_block_with_end(cur.toks(), cur.pos())?;
            cur.set_pos(end);
            // The extension enum precedes the radius-point count and gates
            // nothing. Revision-gated streams store it as a 0x15 enum token;
            // pre-revision streams use a 0x04 integer.
            let enum_tagged = matches!(cur.peek(), Some(Token::Enum(_)));
            let enum_count = if enum_tagged {
                cur.take_enum()?
            } else {
                cur.take_long()?
            };
            let count = usize::try_from(cur.take_long()?).ok()?;
            if count > 100_000 {
                return None;
            }
            let mut points = Vec::with_capacity(count);
            for _ in 0..count {
                let parameter = cur.take_f64()?;
                let radius = cur.take_f64()? * LEN_TO_MM;
                let tangents = [cur.take_f64()?, cur.take_f64()?];
                let location = cur.take_position()?;
                let normal = cur.take_vector3()?;
                points.push(VariableBlendInterpolationPoint {
                    parameter,
                    radius,
                    tangents,
                    location: Point3::new(
                        location[0] * LEN_TO_MM,
                        location[1] * LEN_TO_MM,
                        location[2] * LEN_TO_MM,
                    ),
                    normal: Vector3::new(normal[0], normal[1], normal[2]),
                });
            }
            // The payload ends at the last radius point. The enum that follows
            // is the enclosing record's cross-section selector, not a tail flag.
            VariableBlendValuePayload::Interpolated {
                parameter,
                radius,
                function: PcurveGeometry::Nurbs {
                    degree: function.degree,
                    knots: function.knots,
                    control_points: function.control_points,
                    weights: function.weights,
                    periodic: function.periodic,
                },
                enum_count,
                enum_tagged,
                points,
            }
        }
        _ => return None,
    };
    Some(VariableBlendValue {
        name,
        modern_flag,
        discriminator,
        calibrated,
        payload,
    })
}

#[cfg(test)]
mod variable_blend_value_tests {
    use super::*;
    use cadmpeg_ir::geometry::VariableBlendValuePayload;

    fn text(bytes: &mut Vec<u8>, value: &str) {
        bytes.push(0x07);
        bytes.push(u8::try_from(value.len()).expect("generated text length"));
        bytes.extend_from_slice(value.as_bytes());
    }

    fn integer(bytes: &mut Vec<u8>, tag: u8, value: i64) {
        bytes.push(tag);
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn double(bytes: &mut Vec<u8>, value: f64) {
        bytes.push(0x06);
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn two_ends(bytes: &mut Vec<u8>) {
        text(bytes, "two_ends");
        integer(bytes, 0x04, 7);
        integer(bytes, 0x15, 3);
        bytes.push(0x0a);
        for value in [0.25, 0.75, 1.5, 2.5] {
            double(bytes, value);
        }
    }

    #[test]
    fn decodes_generated_two_ends_and_recursive_const_values() {
        let mut direct = Vec::new();
        two_ends(&mut direct);
        let toks = crate::nurbs::toks::lex_test_span(&direct, 8);
        let mut cur = Cur::at(&toks, 0);
        let decoded = variable_blend_value(&mut cur, true, 0).expect("generated two-ends value");
        assert_eq!(cur.pos(), toks.len());
        assert!(decoded.modern_flag);
        assert_eq!(decoded.discriminator, 7);
        let VariableBlendValuePayload::TwoEnds { parameters, radii } = decoded.payload else {
            panic!("expected two-ends payload")
        };
        assert_eq!(parameters, [0.25, 0.75]);
        assert_eq!(radii, [15.0, 25.0]);

        let mut recursive = Vec::new();
        text(&mut recursive, "const");
        integer(&mut recursive, 0x15, 4);
        recursive.push(0x0b);
        for value in [0.1, 0.9, 3.0] {
            double(&mut recursive, value);
        }
        integer(&mut recursive, 0x15, 3);
        integer(&mut recursive, 0x15, 2);
        two_ends(&mut recursive);
        let toks = crate::nurbs::toks::lex_test_span(&recursive, 8);
        let mut cur = Cur::at(&toks, 0);
        let decoded =
            variable_blend_value(&mut cur, true, 0).expect("generated recursive const value");
        assert_eq!(cur.pos(), toks.len());
        let VariableBlendValuePayload::Constant { radius, nested, .. } = decoded.payload else {
            panic!("expected constant payload")
        };
        assert_eq!(radius, 30.0);
        assert!(matches!(
            nested.payload,
            VariableBlendValuePayload::TwoEnds { .. }
        ));
    }

    #[test]
    fn decodes_generated_fixed_width_value() {
        let mut bytes = Vec::new();
        text(&mut bytes, "fixed_width");
        integer(&mut bytes, 0x15, 0);
        bytes.push(0x0a);
        // Distinct parameter-range bounds and a distinct chamfer width.
        for value in [0.5, 3.5, 0.1905] {
            double(&mut bytes, value);
        }
        let toks = crate::nurbs::toks::lex_test_span(&bytes, 8);
        let mut cur = Cur::at(&toks, 0);
        let decoded = variable_blend_value(&mut cur, true, 0).expect("generated fixed-width value");
        assert_eq!(cur.pos(), toks.len());
        let VariableBlendValuePayload::FixedWidth { parameters, width } = decoded.payload else {
            panic!("expected fixed-width payload")
        };
        assert_eq!(parameters, [0.5, 3.5]);
        assert_eq!(width, 0.1905);
    }

    #[test]
    fn decodes_generated_enum_tagged_interp_counts() {
        let mut bytes = Vec::new();
        text(&mut bytes, "interp");
        integer(&mut bytes, 0x15, 0);
        bytes.push(0x0a);
        double(&mut bytes, 0.0);
        double(&mut bytes, 1.0);
        // Minimal degree-1 BS2 function block.
        bytes.push(0x0d);
        bytes.push(4);
        bytes.extend_from_slice(b"nubs");
        integer(&mut bytes, 0x04, 1);
        integer(&mut bytes, 0x15, 0);
        integer(&mut bytes, 0x04, 2);
        double(&mut bytes, 0.0);
        integer(&mut bytes, 0x04, 1);
        double(&mut bytes, 1.0);
        integer(&mut bytes, 0x04, 1);
        for value in [0.0, 0.0, 1.0, 1.0] {
            double(&mut bytes, value);
        }
        // Enum-tagged extension enum, then the radius-point count.
        integer(&mut bytes, 0x15, 2);
        integer(&mut bytes, 0x04, 1);
        double(&mut bytes, 0.5);
        double(&mut bytes, 1.5);
        double(&mut bytes, 0.0);
        double(&mut bytes, 1.0);
        bytes.push(0x13);
        for value in [1.0f64, 2.0, 3.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x14);
        for value in [0.0f64, 0.0, 1.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        // The value ends at the last radius point. A following enum belongs to
        // the enclosing record's cross-section clause, so it must be left
        // unconsumed.
        integer(&mut bytes, 0x15, 0);
        let toks = crate::nurbs::toks::lex_test_span(&bytes, 8);
        let mut cur = Cur::at(&toks, 0);
        let decoded =
            variable_blend_value(&mut cur, true, 0).expect("generated enum-tagged interp value");
        assert_eq!(cur.pos(), toks.len() - 1);
        let VariableBlendValuePayload::Interpolated {
            enum_count,
            enum_tagged,
            points,
            ..
        } = decoded.payload
        else {
            panic!("expected interpolated payload")
        };
        assert_eq!(enum_count, 2);
        assert!(enum_tagged);
        assert_eq!(points.len(), 1);
    }

    #[test]
    fn decodes_interp_point_with_unset_derivatives() {
        // Sentinel value marking an unset first/second derivative.
        const UNSET: f64 = 1e37;
        let mut bytes = Vec::new();
        text(&mut bytes, "interp");
        integer(&mut bytes, 0x15, 0);
        bytes.push(0x0a);
        double(&mut bytes, 0.0);
        double(&mut bytes, 1.0);
        // Minimal degree-1 BS2 function block.
        bytes.push(0x0d);
        bytes.push(4);
        bytes.extend_from_slice(b"nubs");
        integer(&mut bytes, 0x04, 1);
        integer(&mut bytes, 0x15, 0);
        integer(&mut bytes, 0x04, 2);
        double(&mut bytes, 0.0);
        integer(&mut bytes, 0x04, 1);
        double(&mut bytes, 1.0);
        integer(&mut bytes, 0x04, 1);
        for value in [0.0, 0.0, 1.0, 1.0] {
            double(&mut bytes, value);
        }
        // One interpolation control whose two derivatives are unset.
        integer(&mut bytes, 0x15, 1);
        integer(&mut bytes, 0x04, 1);
        double(&mut bytes, 0.5);
        double(&mut bytes, 1.5);
        double(&mut bytes, UNSET);
        double(&mut bytes, UNSET);
        bytes.push(0x13);
        for value in [1.0f64, 2.0, 3.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x14);
        for value in [0.0f64, 0.0, 1.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        // The enclosing record's cross-section enum, left unconsumed.
        integer(&mut bytes, 0x15, 0);
        let toks = crate::nurbs::toks::lex_test_span(&bytes, 8);
        let mut cur = Cur::at(&toks, 0);
        let decoded = variable_blend_value(&mut cur, true, 0)
            .expect("generated interp value with unset derivatives");
        assert_eq!(cur.pos(), toks.len() - 1);
        let VariableBlendValuePayload::Interpolated { points, .. } = decoded.payload else {
            panic!("expected interpolated payload")
        };
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].tangents, [UNSET, UNSET]);
    }
}

pub(crate) fn var_blend_spl_sur(
    toks: &[Token],
    reference_context: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    use cadmpeg_ir::geometry::VariableBlendCrossSection;
    let names = [
        "var_blend_spl_sur",
        "varblendsplsur",
        "srf_srf_v_bl_spl_sur",
        "srfsrfblndsur",
        "crv_crv_v_bl_spl_sur",
        "crvcrvblndsur",
        "crv_srf_v_bl_spl_sur",
        "crvsrfblndsur",
        "sfcv_free_bl_spl_sur",
        "sfcvfreeblndsur",
    ];
    let (start, name) = toks::find_owned_subtype_marker(toks, &names)?;
    let subtype = match name {
        "var_blend_spl_sur" | "varblendsplsur" => {
            cadmpeg_ir::geometry::VariableBlendSurfaceSubtype::VariableBlend
        }
        "srf_srf_v_bl_spl_sur" | "srfsrfblndsur" => {
            cadmpeg_ir::geometry::VariableBlendSurfaceSubtype::SurfaceSurface
        }
        "crv_crv_v_bl_spl_sur" | "crvcrvblndsur" => {
            cadmpeg_ir::geometry::VariableBlendSurfaceSubtype::CurveCurve
        }
        "crv_srf_v_bl_spl_sur" | "crvsrfblndsur" => {
            cadmpeg_ir::geometry::VariableBlendSurfaceSubtype::CurveSurface
        }
        "sfcv_free_bl_spl_sur" | "sfcvfreeblndsur" => {
            cadmpeg_ir::geometry::VariableBlendSurfaceSubtype::SurfaceCurveFree
        }
        _ => return None,
    };
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let revision = cur.take_long()?;
    let sides = Box::new([
        rolling_ball_side(&mut cur, reference_context)?,
        rolling_ball_side(&mut cur, reference_context)?,
    ]);
    let slice = rolling_ball_curve(&mut cur, reference_context)?;
    let offsets = [cur.take_f64()? * LEN_TO_MM, cur.take_f64()? * LEN_TO_MM];
    let radius_kind = match cur.take_enum()? {
        0 => cadmpeg_ir::geometry::VariableBlendRadiusKind::SingleRadius,
        1 => cadmpeg_ir::geometry::VariableBlendRadiusKind::TwoRadii,
        _ => return None,
    };
    let first_value = variable_blend_value(&mut cur, true, 0)?;
    let second_value = if matches!(
        radius_kind,
        cadmpeg_ir::geometry::VariableBlendRadiusKind::TwoRadii
    ) {
        Some(variable_blend_value(&mut cur, true, 0)?)
    } else {
        None
    };
    // The cross-section clause follows the complete one- or two-radius law
    // sequence. An absent enum is the elided circular default.
    let cross_section = if matches!(cur.peek(), Some(Token::Enum(_))) {
        let selector = cur.take_enum()?;
        match selector {
            0 => Some(VariableBlendCrossSection::Circular),
            1 => Some(VariableBlendCrossSection::Thumbweights {
                parameters: [cur.take_f64()?, cur.take_f64()?],
            }),
            selector @ (2 | 4 | 5 | 6) => Some(VariableBlendCrossSection::UnclassifiedBare {
                selector: selector.try_into().ok()?,
            }),
            3 => {
                let radius = if cur.take_bool()? {
                    Some(Box::new(variable_blend_value(&mut cur, true, 0)?))
                } else {
                    None
                };
                Some(VariableBlendCrossSection::RoundedChamfer { radius })
            }
            7 => Some(VariableBlendCrossSection::G2Round {
                parameters: [cur.take_f64()?, cur.take_f64()?],
            }),
            _ => return None,
        }
    } else {
        None
    };
    let u_range = [
        cur.take_optional_range_value()?,
        cur.take_optional_range_value()?,
    ];
    let v_range = [
        cur.take_optional_range_value()?,
        cur.take_optional_range_value()?,
    ];
    let shape_prefix = cur.take_long()?;
    let shape_parameter = cur.take_f64()?;
    let shape_length = cur.take_f64()? * LEN_TO_MM;
    let shape_tail = cur.take_long()?;
    let RevisionSurfaceTail {
        enumeration: tail_enum,
        fit_tolerance: cache_fit_tolerance,
        solved_cache_domains: _,
        parameterization: tail_parameterization,
        discontinuities,
        tail_flag,
    } = revision_surface_tail(&mut cur)?;
    let tail_extensions = [cur.take_long()?, cur.take_long()?, cur.take_long()?];
    let saved = cur.pos();
    let (secondary_curve, secondary_range) = if cur.take_ident() == Some("null_curve") {
        (None, [None, None])
    } else {
        cur.set_pos(saved);
        let secondary = rolling_ball_curve(&mut cur, reference_context)?;
        (Some(secondary.geometry), secondary.parameter_range)
    };
    let convexity = if cur.take_bool()? {
        cadmpeg_ir::geometry::VariableBlendConvexity::Convex
    } else {
        cadmpeg_ir::geometry::VariableBlendConvexity::Concave
    };
    let render_mode = if cur.take_bool()? {
        cadmpeg_ir::geometry::VariableBlendRenderMode::RollingBallEnvelope
    } else {
        cadmpeg_ir::geometry::VariableBlendRenderMode::RollingBallSnapshot
    };
    let post_range = [
        cur.take_optional_range_value()?,
        cur.take_optional_range_value()?,
    ];
    let saved = cur.pos();
    let post_curve = if cur.take_ident() == Some("nullbs") {
        None
    } else {
        cur.set_pos(saved);
        let (post, post_end) = curve_block(span, cur.pos())?;
        cur.set_pos(post_end);
        Some(post)
    };
    let post_pcurve = nullable_embedded_pcurve(&mut cur)?;
    cur.at_scope_end().then_some(())?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::VariableBlend(Box::new(
            EmbeddedVariableBlend {
                subtype,
                revision,
                sides,
                slice: slice.geometry,
                slice_range: slice.parameter_range,
                offsets,
                radius_kind,
                first_value,
                second_value,
                cross_section,
                u_range,
                v_range,
                shape_prefix,
                shape_parameter,
                shape_length,
                shape_tail,
                tail_enum,
                tail_parameterization,
                discontinuities,
                tail_flag,
                tail_extensions,
                secondary_curve,
                secondary_range,
                convexity,
                render_mode,
                post_range,
                post_curve,
                post_pcurve,
            },
        )),
        cache_fit_tolerance,
    })
}

fn vertex_blend_boundary(cur: &mut Cur<'_>) -> Option<EmbeddedVertexBlendBoundary> {
    let kind = cur.take_str()?.to_string();
    let boundary_type = cur.take_bool()?;
    let magic = cur.take_position()?;
    let u_smoothing = cur.take_bool()?;
    let v_smoothing = cur.take_bool()?;
    let fullness = cur.take_f64()?;
    let geometry = match kind.as_str() {
        "circle" => {
            let (curve, curve_end) = curve_block(cur.toks(), cur.pos())?;
            cur.set_pos(curve_end);
            let form = cur.take_enum()?;
            let twist_count = match form {
                0 => 0,
                1 => 1,
                3 => 2,
                _ => return None,
            };
            let mut twists = Vec::with_capacity(twist_count);
            for _ in 0..twist_count {
                let twist = cur.take_position()?;
                twists.push(Point3::new(
                    twist[0] * LEN_TO_MM,
                    twist[1] * LEN_TO_MM,
                    twist[2] * LEN_TO_MM,
                ));
            }
            let parameters = [cur.take_f64()?, cur.take_f64()?];
            let sense = cur.take_bool()?;
            EmbeddedVertexBlendBoundaryGeometry::Circle {
                curve: CurveGeometry::Nurbs(curve),
                curve_endpoints: [None; 2],
                form,
                twists,
                parameters,
                sense,
            }
        }
        "deg" => {
            let location = cur.take_position()?;
            let first = cur.take_vector3()?;
            let second = cur.take_vector3()?;
            EmbeddedVertexBlendBoundaryGeometry::Degenerate {
                location: Point3::new(
                    location[0] * LEN_TO_MM,
                    location[1] * LEN_TO_MM,
                    location[2] * LEN_TO_MM,
                ),
                normals: [
                    Vector3::new(first[0], first[1], first[2]),
                    Vector3::new(second[0], second[1], second[2]),
                ],
            }
        }
        "pcurve" => {
            let surface = embedded_surface(cur)?;
            let pcurve = nullable_embedded_pcurve(cur)?;
            let sense = cur.take_bool()?;
            let fit_tolerance = cur.take_f64()?;
            EmbeddedVertexBlendBoundaryGeometry::Pcurve {
                surface,
                support_bounds: [None; 4],
                pcurve,
                sense,
                fit_tolerance,
            }
        }
        "plane" => {
            let normal = cur.take_vector3()?;
            let parameters = [cur.take_f64()?, cur.take_f64()?];
            let (curve, curve_end) = curve_block(cur.toks(), cur.pos())?;
            cur.set_pos(curve_end);
            EmbeddedVertexBlendBoundaryGeometry::Plane {
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                parameters,
                curve: CurveGeometry::Nurbs(curve),
                curve_endpoints: [None; 2],
            }
        }
        _ => return None,
    };
    Some(EmbeddedVertexBlendBoundary {
        boundary_type,
        // The magic item is a unit direction or the zero vector, not a
        // length-bearing location, so it takes no unit conversion.
        magic: Vector3::new(magic[0], magic[1], magic[2]),
        u_smoothing,
        v_smoothing,
        fullness,
        geometry,
    })
}

/// Decode one revision-gated vertex-blend boundary: ident-token type name,
/// cross boolean, magic vector, smoothing booleans, fullness, and the
/// type-selected payload with bound-carrying supports and endpoint-carrying
/// curves.
fn revision_vertex_blend_boundary(
    cur: &mut Cur<'_>,
    resolver: Option<&SubtypeTable>,
) -> Option<EmbeddedVertexBlendBoundary> {
    let table = resolver?;
    let kind = cur.take_ident()?.to_string();
    let boundary_type = cur.take_bool()?;
    let magic = cur.take_vector3()?;
    let u_smoothing = cur.take_bool()?;
    let v_smoothing = cur.take_bool()?;
    let fullness = cur.take_f64()?;
    let geometry = match kind.as_str() {
        "circle" => {
            let curve = embedded_base_curve_resolving_refs(cur, table)?;
            let curve_endpoints = [
                cur.take_optional_range_value()?,
                cur.take_optional_range_value()?,
            ];
            let form = cur.take_enum()?;
            let twist_count = match form {
                0 => 0,
                1 => 1,
                3 => 2,
                _ => return None,
            };
            let mut twists = Vec::with_capacity(twist_count);
            for _ in 0..twist_count {
                let twist = cur.take_vector3()?;
                twists.push(Point3::new(
                    twist[0] * LEN_TO_MM,
                    twist[1] * LEN_TO_MM,
                    twist[2] * LEN_TO_MM,
                ));
            }
            let parameters = [cur.take_f64()?, cur.take_f64()?];
            let sense = cur.take_bool()?;
            EmbeddedVertexBlendBoundaryGeometry::Circle {
                curve: CurveGeometry::Nurbs(curve),
                curve_endpoints,
                form,
                twists,
                parameters,
                sense,
            }
        }
        "deg" => {
            let location = cur.take_position()?;
            let first = cur.take_vector3()?;
            let second = cur.take_vector3()?;
            EmbeddedVertexBlendBoundaryGeometry::Degenerate {
                location: Point3::new(
                    location[0] * LEN_TO_MM,
                    location[1] * LEN_TO_MM,
                    location[2] * LEN_TO_MM,
                ),
                normals: [
                    Vector3::new(first[0], first[1], first[2]),
                    Vector3::new(second[0], second[1], second[2]),
                ],
            }
        }
        "pcurve" => {
            let (surface, support_bounds) = optional_embedded_surface_with_bounds(cur, table)?;
            let pcurve = nullable_embedded_pcurve(cur)?;
            let sense = cur.take_bool()?;
            let fit_tolerance = cur.take_f64()?;
            EmbeddedVertexBlendBoundaryGeometry::Pcurve {
                surface: surface?,
                support_bounds,
                pcurve,
                sense,
                fit_tolerance,
            }
        }
        "plane" => {
            let normal = cur.take_vector3()?;
            let parameters = [cur.take_f64()?, cur.take_f64()?];
            let curve = embedded_base_curve_resolving_refs(cur, table)?;
            let curve_endpoints = [
                cur.take_optional_range_value()?,
                cur.take_optional_range_value()?,
            ];
            EmbeddedVertexBlendBoundaryGeometry::Plane {
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                parameters,
                curve: CurveGeometry::Nurbs(curve),
                curve_endpoints,
            }
        }
        _ => return None,
    };
    Some(EmbeddedVertexBlendBoundary {
        boundary_type,
        // The magic item is a unit direction or the zero vector, not a
        // length-bearing location, so it takes no unit conversion.
        magic: Vector3::new(magic[0], magic[1], magic[2]),
        u_smoothing,
        v_smoothing,
        fullness,
        geometry,
    })
}

pub(crate) fn vertex_blend_spl_sur(
    toks: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    let names = ["VBL_SURF", "vertexblendsur"];
    let (start, name) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    // The revision-gated layout stores the revision integer before the
    // boundary count; boundary names are ident tokens and boundary payloads
    // carry optional bounds and endpoints. The count is an integer token in
    // both layouts, so the revision layout is recognized by the second
    // integer token: a legacy count is directly followed by a boundary type
    // string, a revision integer by the count integer. Only the modern name
    // stores the revision layout.
    let revision = if matches!(span.get(cur.pos()), Some(Token::Long(_)))
        && matches!(span.get(cur.pos() + 1), Some(Token::Long(_)))
    {
        (name == "VBL_SURF").then_some(())?;
        let revision = cur.take_long()?;
        (revision > 0).then_some(())?;
        Some(revision)
    } else {
        None
    };
    let count = usize::try_from(cur.take_long()?).ok()?;
    if count > 100_000 {
        return None;
    }
    let mut boundaries = Vec::with_capacity(count);
    for _ in 0..count {
        boundaries.push(if revision.is_some() {
            revision_vertex_blend_boundary(&mut cur, resolver)?
        } else {
            vertex_blend_boundary(&mut cur)?
        });
    }
    let grid_size = cur.take_long()?;
    let fit_tolerance = cur.take_f64()? * LEN_TO_MM;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::VertexBlend(Box::new(
            EmbeddedVertexBlend {
                revision,
                boundaries,
                grid_size,
                fit_tolerance,
            },
        )),
        cache_fit_tolerance: None,
    })
}

pub(crate) fn full_rb_blend_spl_sur(
    toks: &[Token],
    table: &SubtypeTable,
) -> Option<DecodedProceduralSurface> {
    let names = [
        "rb_blend_spl_sur",
        "rbblnsur",
        "pipe_spl_sur",
        "pipesur",
        "sss_blend_spl_sur",
        "sssblndsur",
    ];
    let (start, name) = toks::find_owned_subtype_marker(toks, &names)?;
    let has_third = name == "sss_blend_spl_sur" || name == "sssblndsur";
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let definition_index = cur.take_long()?;
    let sides = Box::new([
        rolling_ball_side(&mut cur, Some(table))?,
        rolling_ball_side(&mut cur, Some(table))?,
    ]);
    let slice = rolling_ball_curve(&mut cur, Some(table))?;
    let offsets = [cur.take_f64()? * LEN_TO_MM, cur.take_f64()? * LEN_TO_MM];
    let radius_selector = match cur.peek()? {
        Token::Enum(_) => {
            if cur.take_enum()? != -1 {
                return None;
            }
            EmbeddedRollingBallRadiusSelector::None
        }
        Token::Double(_) => EmbeddedRollingBallRadiusSelector::Value(cur.take_f64()?),
        _ => return None,
    };
    let u_range = [
        cur.take_optional_range_value()?,
        cur.take_optional_range_value()?,
    ];
    let v_range = [
        cur.take_optional_range_value()?,
        cur.take_optional_range_value()?,
    ];
    let shape_prefix = cur.take_long()?;
    let parameters = [cur.take_f64()?, cur.take_f64()?];
    let tail = cur.take_long()?;
    let RevisionSurfaceTail {
        enumeration: tail_enum,
        fit_tolerance: cache_fit_tolerance,
        solved_cache_domains: _,
        parameterization: tail_parameterization,
        discontinuities,
        tail_flag,
    } = revision_surface_tail(&mut cur)?;
    let third = if has_third {
        Some(Box::new(rolling_ball_third_side(&mut cur)?))
    } else {
        None
    };
    let tail_extensions = [cur.take_long()?, cur.take_long()?, cur.take_long()?];
    cur.at_scope_end().then_some(())?;
    let radius = if offsets[0] == offsets[1] {
        BlendRadiusLaw::Constant {
            signed_radius: offsets[0],
        }
    } else {
        BlendRadiusLaw::Linear {
            start: offsets[0],
            end: offsets[1],
        }
    };
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Blend {
            supports: Box::new([None, None]),
            spine: match &slice.geometry {
                CurveGeometry::Nurbs(curve) => Some(curve.clone()),
                _ => None,
            },
            radius,
            cross_section: BlendCrossSection::Circular,
            native: Some(Box::new(EmbeddedRollingBall {
                definition_index,
                sides,
                slice: slice.geometry,
                slice_range: slice.parameter_range,
                offsets,
                radius_selector,
                u_range,
                v_range,
                shape_prefix,
                parameters,
                tail,
                tail_enum,
                tail_parameterization,
                discontinuities,
                tail_flag,
                third,
                tail_extensions,
            })),
        },
        cache_fit_tolerance,
    })
}

/// Decode the compact rolling-ball carrier emitted without the native side
/// graph. Every field is positional; nested construction members are not
/// searched by token kind.
pub(crate) fn compact_rb_blend_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    let names = ["rb_blend_spl_sur", "rbblnsur", "pipe_spl_sur", "pipesur"];
    let (start, _) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let mut supports = [None, None];
    let mut support_count = 0usize;
    while matches!(cur.peek(), Some(Token::Str(label)) if label == "blend_support_surface") {
        if support_count == supports.len() {
            return None;
        }
        cur.take_str()?;
        let has_outer_kind = matches!(cur.peek(), Some(Token::Ident(name) | Token::SubIdent(name)) if name != "nubs" && name != "nurbs");
        if has_outer_kind {
            cur.take_ident()?;
        }
        let payload_start = cur.pos();
        let support = if !has_outer_kind {
            let (_, end) = surface_block(span, cur.pos())?;
            cur.set_pos(end);
            None
        } else if let Some(surface) = embedded_surface(&mut cur) {
            Some(surface)
        } else {
            cur.set_pos(payload_start);
            let (surface, end) = surface_block(span, cur.pos())?;
            cur.set_pos(end);
            Some(SurfaceGeometry::Nurbs(surface))
        };
        supports[support_count] = support;
        support_count += 1;
    }
    let (spine, spine_end) = curve_block(span, cur.pos())?;
    cur.set_pos(spine_end);
    let offsets = [cur.take_f64()? * LEN_TO_MM, cur.take_f64()? * LEN_TO_MM];
    (cur.take_enum()? == -1).then_some(())?;
    let (_, cache_end) = surface_block(span, cur.pos())?;
    cur.set_pos(cache_end);
    let cache_fit_tolerance = if matches!(cur.peek(), Some(Token::Double(_))) {
        Some(cur.take_f64()? * LEN_TO_MM)
    } else {
        None
    };
    cur.at_scope_end().then_some(())?;

    let radius = if offsets[0] == offsets[1] {
        BlendRadiusLaw::Constant {
            signed_radius: offsets[0],
        }
    } else {
        BlendRadiusLaw::Linear {
            start: offsets[0],
            end: offsets[1],
        }
    };
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Blend {
            supports: Box::new(supports),
            spine: Some(spine),
            radius,
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance,
    })
}
