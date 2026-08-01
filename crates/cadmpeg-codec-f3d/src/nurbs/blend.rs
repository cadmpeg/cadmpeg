// SPDX-License-Identifier: Apache-2.0
//! Blend spline-surface decoders (cylindrical, rolling-ball, variable, vertex, and rb blends).

use crate::nurbs::core::{
    decode_curve_block, decode_curve_cache_at, decode_owned_curve_cache_at,
    decode_owned_curve_cache_resolving_refs_at, decode_owned_surface_cache_at,
    decode_owned_surface_cache_resolving_refs_at, decode_surface_block,
};
use crate::nurbs::pcurve::decode_pcurve_block_with_end;
use crate::nurbs::proc_curve::{
    decode_embedded_base_curve_resolving_refs, decode_embedded_surface,
    decode_embedded_surface_with_ranges, decode_optional_embedded_surface_with_bounds,
    decode_par_int_cur_isoline,
};
use crate::nurbs::proc_surface::{
    decode_nullable_embedded_pcurve, decode_revision_surface_tail, DecodedProceduralSurface,
    DecodedProceduralSurfaceDefinition, EmbeddedRollingBall, EmbeddedRollingBallRadiusSelector,
    EmbeddedRollingBallSide, EmbeddedRollingBallThirdSide, EmbeddedVariableBlend,
    EmbeddedVertexBlend, EmbeddedVertexBlendBoundary, EmbeddedVertexBlendBoundaryGeometry,
    RevisionSurfaceTail,
};
use crate::nurbs::reader::{
    marker_at, marker_positions, owned_marker_positions, take_bool, take_f64, take_native_ident,
    take_native_string, take_native_vec3, take_optional_range_value, take_tagged_int, unit_vector,
    LEN_TO_MM,
};
use crate::nurbs::subtypes::{find_owned_subtype_marker, next_token, subtype_span, SubtypeTables};
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry, SurfaceGeometry,
};
use cadmpeg_ir::le::{f64_at as read_f64, int_at as read_int};
use cadmpeg_ir::math::{Point3, Vector3};

/// Decode an inline `cyl_spl_sur` translational-extrusion definition.
pub(crate) fn decode_cyl_spl_sur_at(
    record_bytes: &[u8],
    int_width: usize,
) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 2] = [b"cyl_spl_sur", b"cylsur"];
    let (start, name) = find_owned_subtype_marker(record_bytes, &names, int_width)?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let directrix = decode_curve_cache_at(span, int_width)?;

    let mut position = name.len() + 3;
    // The revision-gated layout stores the directrix as a nested intcurve scope
    // and ends with the shared revision-gated surface tail, so its cache is
    // located by parsing that tail. The compact layout has no tail: its optional
    // final surface cache is the last surface block in the scope.
    let (parameter_interval, direction, native_position, cache_fit_tolerance, revision_form) =
        if span.get(position) == Some(&0x04) {
            let revision = take_tagged_int(span, &mut position, 0x04, int_width)?;
            (take_native_ident(span, &mut position)? == "intcurve").then_some(())?;
            // Sense flag of the embedded directrix curve. It is the carrier's
            // whole boolean run, so it travels in the revision form's `flags`.
            let directrix_sense = take_bool(span, &mut position)?;
            let directrix_scope = subtype_span(span, position, int_width)?;
            position += directrix_scope.len();
            let start = take_optional_range_value(span, &mut position)?;
            let end = take_optional_range_value(span, &mut position)?;
            let interval = [start?, end?];
            let direction = take_native_vec3(span, &mut position, 0x14)?;
            let native_position = take_native_vec3(span, &mut position, 0x13)?;
            let RevisionSurfaceTail {
                enumeration: tail_enum,
                fit_tolerance,
                parameterization,
                discontinuities,
                tail_flag,
            } = decode_revision_surface_tail(span, &mut position, int_width)?;
            (
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
            let interval = [
                take_f64(span, &mut position)?,
                take_f64(span, &mut position)?,
            ];
            let direction = take_native_vec3(span, &mut position, 0x14)?;
            let native_position = take_native_vec3(span, &mut position, 0x13)?;
            let cache_fit_tolerance = owned_marker_positions(span, int_width)
                .into_iter()
                .filter_map(|at| decode_surface_block(span, at, int_width))
                .next_back()
                .filter(|cache| span.get(cache.end) == Some(&0x06))
                .and_then(|cache| read_f64(span, cache.end + 1).map(|v| v * LEN_TO_MM));
            (
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
    let support_kind = match take_native_string(bytes, position)?.as_str() {
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

fn decode_rolling_ball_third_side(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<EmbeddedRollingBallThirdSide> {
    let label = take_native_string(bytes, position)?;
    let surface = decode_embedded_surface(bytes, position, int_width)?;
    let curve = decode_curve_block(bytes, *position, int_width)?;
    *position = curve.end;
    let pcurve = decode_nullable_embedded_pcurve(bytes, position, int_width)?;
    let direction = take_native_vec3(bytes, position, 0x14)?;
    let secondary_pcurve = decode_nullable_embedded_pcurve(bytes, position, int_width)?;
    let extension = take_tagged_int(bytes, position, 0x04, int_width)?;
    let tertiary_pcurve = decode_nullable_embedded_pcurve(bytes, position, int_width)?;
    let flag = take_bool(bytes, position)?;
    Some(EmbeddedRollingBallThirdSide {
        label,
        surface,
        curve: curve.curve,
        pcurve,
        direction: Vector3::new(direction[0], direction[1], direction[2]),
        secondary_pcurve,
        extension,
        tertiary_pcurve,
        flag,
    })
}

fn take_blend_value_name(bytes: &[u8], position: &mut usize) -> Option<String> {
    let saved = *position;
    if let Some(value) = take_native_string(bytes, position) {
        return Some(value);
    }
    *position = saved;
    take_native_ident(bytes, position)
}

fn decode_variable_blend_value(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
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
    let name = take_blend_value_name(bytes, position)?;
    let discriminator = if bytes.get(*position) == Some(&0x04) {
        take_tagged_int(bytes, position, 0x04, int_width)?
    } else {
        1
    };
    let calibrated = take_tagged_int(bytes, position, 0x15, int_width)?;
    let modern_flag = if modern {
        take_bool(bytes, position)?
    } else {
        false
    };
    let payload = match name.as_str() {
        "fixed_width" => VariableBlendValuePayload::FixedWidth {
            parameters: [take_f64(bytes, position)?, take_f64(bytes, position)?],
            width: take_f64(bytes, position)?,
        },
        "two_ends" => VariableBlendValuePayload::TwoEnds {
            parameters: [take_f64(bytes, position)?, take_f64(bytes, position)?],
            radii: [
                take_f64(bytes, position)? * LEN_TO_MM,
                take_f64(bytes, position)? * LEN_TO_MM,
            ],
        },
        // The payload is the law-domain parameter range and one offset, so the
        // second field is a parameter and only the third is a length. The
        // sub-discriminator selects no layout here; it is still read and written
        // as the format stores it, and no value outside `0` and `1` is defined.
        "edge_offset" if matches!(discriminator, 0 | 1) => VariableBlendValuePayload::EdgeOffset {
            scalars: vec![take_f64(bytes, position)?, take_f64(bytes, position)?],
            lengths: vec![take_f64(bytes, position)? * LEN_TO_MM],
        },
        "functional" => {
            let parameter = take_f64(bytes, position)?;
            let radius = take_f64(bytes, position)? * LEN_TO_MM;
            let (function, end) = decode_pcurve_block_with_end(bytes, *position, int_width)?;
            *position = end;
            let terminal = if bytes.get(*position) == Some(&0x06) {
                LoftBridgeToken::Double(take_f64(bytes, position)?)
            } else {
                LoftBridgeToken::Text(take_blend_value_name(bytes, position)?)
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
            parameters: [take_f64(bytes, position)?, take_f64(bytes, position)?],
            radius: take_f64(bytes, position)? * LEN_TO_MM,
            variable_chamfer: take_tagged_int(bytes, position, 0x15, int_width)?,
            chamfer_type: take_tagged_int(bytes, position, 0x15, int_width)?,
            nested: Box::new(decode_variable_blend_value(
                bytes,
                position,
                int_width,
                modern,
                depth + 1,
            )?),
        },
        "interp" => {
            let parameter = take_f64(bytes, position)?;
            let radius = take_f64(bytes, position)? * LEN_TO_MM;
            let (function, end) = decode_pcurve_block_with_end(bytes, *position, int_width)?;
            *position = end;
            // The extension enum precedes the radius-point count and gates
            // nothing. Revision-gated streams store it as a 0x15 enum token;
            // pre-revision streams use a 0x04 integer.
            let enum_tagged = bytes.get(*position) == Some(&0x15);
            let count_tag = if enum_tagged { 0x15 } else { 0x04 };
            let enum_count = take_tagged_int(bytes, position, count_tag, int_width)?;
            let count = usize::try_from(take_tagged_int(bytes, position, 0x04, int_width)?).ok()?;
            if count > 100_000 {
                return None;
            }
            let mut points = Vec::with_capacity(count);
            for _ in 0..count {
                let parameter = take_f64(bytes, position)?;
                let radius = take_f64(bytes, position)? * LEN_TO_MM;
                let tangents = [take_f64(bytes, position)?, take_f64(bytes, position)?];
                let location = take_native_vec3(bytes, position, 0x13)?;
                let normal = take_native_vec3(bytes, position, 0x14)?;
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
        let mut position = 0;
        let decoded = decode_variable_blend_value(&direct, &mut position, 8, true, 0)
            .expect("generated two-ends value");
        assert_eq!(position, direct.len());
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
        let mut position = 0;
        let decoded = decode_variable_blend_value(&recursive, &mut position, 8, true, 0)
            .expect("generated recursive const value");
        assert_eq!(position, recursive.len());
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
        let mut position = 0;
        let decoded = decode_variable_blend_value(&bytes, &mut position, 8, true, 0)
            .expect("generated fixed-width value");
        assert_eq!(position, bytes.len());
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
        let payload_end = bytes.len();
        integer(&mut bytes, 0x15, 0);
        let mut position = 0;
        let decoded = decode_variable_blend_value(&bytes, &mut position, 8, true, 0)
            .expect("generated enum-tagged interp value");
        assert_eq!(position, payload_end);
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
        const UNSET: f64 = 9.9999999999999995e+36;
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
        let payload_end = bytes.len();
        integer(&mut bytes, 0x15, 0);
        let mut position = 0;
        let decoded = decode_variable_blend_value(&bytes, &mut position, 8, true, 0)
            .expect("generated interp value with unset derivatives");
        assert_eq!(position, payload_end);
        let VariableBlendValuePayload::Interpolated { points, .. } = decoded.payload else {
            panic!("expected interpolated payload")
        };
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].tangents, [UNSET, UNSET]);
    }
}

pub(crate) fn decode_var_blend_spl_sur(
    record_bytes: &[u8],
    int_width: usize,
    reference_context: Option<(&[u8], &SubtypeTables)>,
) -> Option<DecodedProceduralSurface> {
    use cadmpeg_ir::geometry::VariableBlendCrossSection;
    let names: [&[u8]; 10] = [
        b"var_blend_spl_sur",
        b"varblendsplsur",
        b"srf_srf_v_bl_spl_sur",
        b"srfsrfblndsur",
        b"crv_crv_v_bl_spl_sur",
        b"crvcrvblndsur",
        b"crv_srf_v_bl_spl_sur",
        b"crvsrfblndsur",
        b"sfcv_free_bl_spl_sur",
        b"sfcvfreeblndsur",
    ];
    let (start, name) = find_owned_subtype_marker(record_bytes, &names, int_width)?;
    let subtype = match name {
        b"var_blend_spl_sur" | b"varblendsplsur" => {
            cadmpeg_ir::geometry::VariableBlendSurfaceSubtype::VariableBlend
        }
        b"srf_srf_v_bl_spl_sur" | b"srfsrfblndsur" => {
            cadmpeg_ir::geometry::VariableBlendSurfaceSubtype::SurfaceSurface
        }
        b"crv_crv_v_bl_spl_sur" | b"crvcrvblndsur" => {
            cadmpeg_ir::geometry::VariableBlendSurfaceSubtype::CurveCurve
        }
        b"crv_srf_v_bl_spl_sur" | b"crvsrfblndsur" => {
            cadmpeg_ir::geometry::VariableBlendSurfaceSubtype::CurveSurface
        }
        b"sfcv_free_bl_spl_sur" | b"sfcvfreeblndsur" => {
            cadmpeg_ir::geometry::VariableBlendSurfaceSubtype::SurfaceCurveFree
        }
        _ => return None,
    };
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name.len() + 3;
    let revision = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let sides = Box::new([
        decode_rolling_ball_side(span, &mut position, int_width, reference_context)?,
        decode_rolling_ball_side(span, &mut position, int_width, reference_context)?,
    ]);
    let slice = decode_rolling_ball_curve(span, &mut position, int_width, reference_context)?;
    let offsets = [
        take_f64(span, &mut position)? * LEN_TO_MM,
        take_f64(span, &mut position)? * LEN_TO_MM,
    ];
    let radius_kind = match take_tagged_int(span, &mut position, 0x15, int_width)? {
        0 => cadmpeg_ir::geometry::VariableBlendRadiusKind::SingleRadius,
        1 => cadmpeg_ir::geometry::VariableBlendRadiusKind::TwoRadii,
        _ => return None,
    };
    let first_value = decode_variable_blend_value(span, &mut position, int_width, true, 0)?;
    let second_value = if matches!(
        radius_kind,
        cadmpeg_ir::geometry::VariableBlendRadiusKind::TwoRadii
    ) {
        Some(decode_variable_blend_value(
            span,
            &mut position,
            int_width,
            true,
            0,
        )?)
    } else {
        None
    };
    // The cross-section clause follows the complete one- or two-radius law
    // sequence. An absent enum is the elided circular default.
    let cross_section = if span.get(position) == Some(&0x15) {
        let selector = take_tagged_int(span, &mut position, 0x15, int_width)?;
        match selector {
            0 => Some(VariableBlendCrossSection::Circular),
            1 => Some(VariableBlendCrossSection::Thumbweights {
                parameters: [
                    take_f64(span, &mut position)?,
                    take_f64(span, &mut position)?,
                ],
            }),
            3 => {
                let radius = if take_bool(span, &mut position)? {
                    Some(Box::new(decode_variable_blend_value(
                        span,
                        &mut position,
                        int_width,
                        true,
                        0,
                    )?))
                } else {
                    None
                };
                Some(VariableBlendCrossSection::RoundedChamfer { radius })
            }
            7 => Some(VariableBlendCrossSection::G2Round {
                parameters: [
                    take_f64(span, &mut position)?,
                    take_f64(span, &mut position)?,
                ],
            }),
            _ => return None,
        }
    } else {
        None
    };
    let u_range = [
        take_optional_range_value(span, &mut position)?,
        take_optional_range_value(span, &mut position)?,
    ];
    let v_range = [
        take_optional_range_value(span, &mut position)?,
        take_optional_range_value(span, &mut position)?,
    ];
    let shape_prefix = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let shape_parameter = take_f64(span, &mut position)?;
    let shape_length = take_f64(span, &mut position)? * LEN_TO_MM;
    let shape_tail = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let RevisionSurfaceTail {
        enumeration: tail_enum,
        fit_tolerance: cache_fit_tolerance,
        parameterization: tail_parameterization,
        discontinuities,
        tail_flag,
    } = decode_revision_surface_tail(span, &mut position, int_width)?;
    let tail_extensions = [
        take_tagged_int(span, &mut position, 0x04, int_width)?,
        take_tagged_int(span, &mut position, 0x04, int_width)?,
        take_tagged_int(span, &mut position, 0x04, int_width)?,
    ];
    let saved = position;
    let (secondary_curve, secondary_range) =
        if take_native_ident(span, &mut position).as_deref() == Some("null_curve") {
            (None, [None, None])
        } else {
            position = saved;
            let secondary =
                decode_rolling_ball_curve(span, &mut position, int_width, reference_context)?;
            (Some(secondary.geometry), secondary.parameter_range)
        };
    let convexity = if take_bool(span, &mut position)? {
        cadmpeg_ir::geometry::VariableBlendConvexity::Convex
    } else {
        cadmpeg_ir::geometry::VariableBlendConvexity::Concave
    };
    let render_mode = if take_bool(span, &mut position)? {
        cadmpeg_ir::geometry::VariableBlendRenderMode::RollingBallEnvelope
    } else {
        cadmpeg_ir::geometry::VariableBlendRenderMode::RollingBallSnapshot
    };
    let post_range = [
        take_optional_range_value(span, &mut position)?,
        take_optional_range_value(span, &mut position)?,
    ];
    let saved = position;
    let post_curve = if take_native_ident(span, &mut position).as_deref() == Some("nullbs") {
        None
    } else {
        position = saved;
        let post = decode_curve_block(span, position, int_width)?;
        position = post.end;
        Some(post.curve)
    };
    let post_pcurve = decode_nullable_embedded_pcurve(span, &mut position, int_width)?;
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

fn decode_vertex_blend_boundary(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<EmbeddedVertexBlendBoundary> {
    let kind = take_native_string(bytes, position)?;
    let boundary_type = take_bool(bytes, position)?;
    let magic = take_native_vec3(bytes, position, 0x13)?;
    let u_smoothing = take_bool(bytes, position)?;
    let v_smoothing = take_bool(bytes, position)?;
    let fullness = take_f64(bytes, position)?;
    let geometry = match kind.as_str() {
        "circle" => {
            let curve = decode_curve_block(bytes, *position, int_width)?;
            *position = curve.end;
            let form = take_tagged_int(bytes, position, 0x15, int_width)?;
            let twist_count = match form {
                0 => 0,
                1 => 1,
                3 => 2,
                _ => return None,
            };
            let mut twists = Vec::with_capacity(twist_count);
            for _ in 0..twist_count {
                let twist = take_native_vec3(bytes, position, 0x13)?;
                twists.push(Point3::new(
                    twist[0] * LEN_TO_MM,
                    twist[1] * LEN_TO_MM,
                    twist[2] * LEN_TO_MM,
                ));
            }
            let parameters = [take_f64(bytes, position)?, take_f64(bytes, position)?];
            let sense = take_bool(bytes, position)?;
            EmbeddedVertexBlendBoundaryGeometry::Circle {
                curve: CurveGeometry::Nurbs(curve.curve),
                curve_endpoints: [None; 2],
                form,
                twists,
                parameters,
                sense,
            }
        }
        "deg" => {
            let location = take_native_vec3(bytes, position, 0x13)?;
            let first = take_native_vec3(bytes, position, 0x14)?;
            let second = take_native_vec3(bytes, position, 0x14)?;
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
            let surface = decode_embedded_surface(bytes, position, int_width)?;
            let pcurve = decode_nullable_embedded_pcurve(bytes, position, int_width)?;
            let sense = take_bool(bytes, position)?;
            let fit_tolerance = take_f64(bytes, position)?;
            EmbeddedVertexBlendBoundaryGeometry::Pcurve {
                surface,
                support_bounds: [None; 4],
                pcurve,
                sense,
                fit_tolerance,
            }
        }
        "plane" => {
            let normal = take_native_vec3(bytes, position, 0x14)?;
            let parameters = [take_f64(bytes, position)?, take_f64(bytes, position)?];
            let curve = decode_curve_block(bytes, *position, int_width)?;
            *position = curve.end;
            EmbeddedVertexBlendBoundaryGeometry::Plane {
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                parameters,
                curve: CurveGeometry::Nurbs(curve.curve),
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
fn decode_revision_vertex_blend_boundary(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
    resolver: Option<(&[u8], &SubtypeTables)>,
) -> Option<EmbeddedVertexBlendBoundary> {
    let (active_bytes, tables) = resolver?;
    let kind = take_native_ident(bytes, position)?;
    let boundary_type = take_bool(bytes, position)?;
    let magic = take_native_vec3(bytes, position, 0x14)?;
    let u_smoothing = take_bool(bytes, position)?;
    let v_smoothing = take_bool(bytes, position)?;
    let fullness = take_f64(bytes, position)?;
    let geometry = match kind.as_str() {
        "circle" => {
            let curve = decode_embedded_base_curve_resolving_refs(
                bytes,
                position,
                int_width,
                active_bytes,
                tables,
            )?;
            let curve_endpoints = [
                take_optional_range_value(bytes, position)?,
                take_optional_range_value(bytes, position)?,
            ];
            let form = take_tagged_int(bytes, position, 0x15, int_width)?;
            let twist_count = match form {
                0 => 0,
                1 => 1,
                3 => 2,
                _ => return None,
            };
            let mut twists = Vec::with_capacity(twist_count);
            for _ in 0..twist_count {
                let twist = take_native_vec3(bytes, position, 0x14)?;
                twists.push(Point3::new(
                    twist[0] * LEN_TO_MM,
                    twist[1] * LEN_TO_MM,
                    twist[2] * LEN_TO_MM,
                ));
            }
            let parameters = [take_f64(bytes, position)?, take_f64(bytes, position)?];
            let sense = take_bool(bytes, position)?;
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
            let location = take_native_vec3(bytes, position, 0x13)?;
            let first = take_native_vec3(bytes, position, 0x14)?;
            let second = take_native_vec3(bytes, position, 0x14)?;
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
            let (surface, support_bounds) = decode_optional_embedded_surface_with_bounds(
                bytes,
                position,
                int_width,
                active_bytes,
                tables,
            )?;
            let pcurve = decode_nullable_embedded_pcurve(bytes, position, int_width)?;
            let sense = take_bool(bytes, position)?;
            let fit_tolerance = take_f64(bytes, position)?;
            EmbeddedVertexBlendBoundaryGeometry::Pcurve {
                surface: surface?,
                support_bounds,
                pcurve,
                sense,
                fit_tolerance,
            }
        }
        "plane" => {
            let normal = take_native_vec3(bytes, position, 0x14)?;
            let parameters = [take_f64(bytes, position)?, take_f64(bytes, position)?];
            let curve = decode_embedded_base_curve_resolving_refs(
                bytes,
                position,
                int_width,
                active_bytes,
                tables,
            )?;
            let curve_endpoints = [
                take_optional_range_value(bytes, position)?,
                take_optional_range_value(bytes, position)?,
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

pub(crate) fn decode_vertex_blend_spl_sur(
    record_bytes: &[u8],
    int_width: usize,
    resolver: Option<(&[u8], &SubtypeTables)>,
) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 2] = [b"VBL_SURF", b"vertexblendsur"];
    let (start, name) = find_owned_subtype_marker(record_bytes, &names, int_width)?;
    let name_len = name.len();
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name_len + 3;
    // The revision-gated layout stores the revision integer before the
    // boundary count; boundary names are ident tokens and boundary payloads
    // carry optional bounds and endpoints. The count is a `0x04` integer in
    // both layouts, so the revision layout is recognized by the second
    // `0x04` token: a legacy count is directly followed by a boundary type
    // string, a revision integer by the count integer. Only the modern name
    // stores the revision layout.
    let revision =
        if span.get(position) == Some(&0x04) && span.get(position + 1 + int_width) == Some(&0x04) {
            (name == b"VBL_SURF".as_slice()).then_some(())?;
            let revision = take_tagged_int(span, &mut position, 0x04, int_width)?;
            (revision > 0).then_some(())?;
            Some(revision)
        } else {
            None
        };
    let count = usize::try_from(take_tagged_int(span, &mut position, 0x04, int_width)?).ok()?;
    if count > 100_000 {
        return None;
    }
    let mut boundaries = Vec::with_capacity(count);
    for _ in 0..count {
        boundaries.push(if revision.is_some() {
            decode_revision_vertex_blend_boundary(span, &mut position, int_width, resolver)?
        } else {
            decode_vertex_blend_boundary(span, &mut position, int_width)?
        });
    }
    let grid_size = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let fit_tolerance = take_f64(span, &mut position)? * LEN_TO_MM;
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

pub(crate) fn decode_full_rb_blend_spl_sur(
    record_bytes: &[u8],
    int_width: usize,
    active_bytes: &[u8],
    tables: &SubtypeTables,
) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 6] = [
        b"rb_blend_spl_sur",
        b"rbblnsur",
        b"pipe_spl_sur",
        b"pipesur",
        b"sss_blend_spl_sur",
        b"sssblndsur",
    ];
    let (start, name) = find_owned_subtype_marker(record_bytes, &names, int_width)?;
    let name_len = name.len();
    let has_third = name == b"sss_blend_spl_sur" || name == b"sssblndsur";
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name_len + 3;
    let definition_index = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let sides = Box::new([
        decode_rolling_ball_side(span, &mut position, int_width, Some((active_bytes, tables)))?,
        decode_rolling_ball_side(span, &mut position, int_width, Some((active_bytes, tables)))?,
    ]);
    let slice =
        decode_rolling_ball_curve(span, &mut position, int_width, Some((active_bytes, tables)))?;
    let offsets = [
        take_f64(span, &mut position)? * LEN_TO_MM,
        take_f64(span, &mut position)? * LEN_TO_MM,
    ];
    let radius_selector = match span.get(position)? {
        0x15 => {
            if take_tagged_int(span, &mut position, 0x15, int_width)? != -1 {
                return None;
            }
            EmbeddedRollingBallRadiusSelector::None
        }
        0x06 => EmbeddedRollingBallRadiusSelector::Value(take_f64(span, &mut position)?),
        _ => return None,
    };
    let u_range = [
        take_optional_range_value(span, &mut position)?,
        take_optional_range_value(span, &mut position)?,
    ];
    let v_range = [
        take_optional_range_value(span, &mut position)?,
        take_optional_range_value(span, &mut position)?,
    ];
    let shape_prefix = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let parameters = [
        take_f64(span, &mut position)?,
        take_f64(span, &mut position)?,
    ];
    let tail = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let RevisionSurfaceTail {
        enumeration: tail_enum,
        fit_tolerance: cache_fit_tolerance,
        parameterization: tail_parameterization,
        discontinuities,
        tail_flag,
    } = decode_revision_surface_tail(span, &mut position, int_width)?;
    let third = if has_third {
        Some(Box::new(decode_rolling_ball_third_side(
            span,
            &mut position,
            int_width,
        )?))
    } else {
        None
    };
    let tail_extensions = [
        take_tagged_int(span, &mut position, 0x04, int_width)?,
        take_tagged_int(span, &mut position, 0x04, int_width)?,
        take_tagged_int(span, &mut position, 0x04, int_width)?,
    ];
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

pub(crate) fn decode_rb_blend_spl_sur_fallback(
    record_bytes: &[u8],
    int_width: usize,
) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 4] = [
        b"rb_blend_spl_sur",
        b"rbblnsur",
        b"pipe_spl_sur",
        b"pipesur",
    ];
    let (start, header_len) = find_owned_subtype_marker(record_bytes, &names, int_width)
        .map(|(start, name)| (start, name.len() + 3))?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let cache = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at, int_width))
        .next_back()?;

    let mut support_geometries = Vec::new();
    let mut radius_boundary = None;
    let mut pos = header_len;
    while pos < cache.end {
        match span[pos] {
            0x0e => {
                let len = usize::from(*span.get(pos + 1)?);
                let name = span.get(pos + 2..pos + 2 + len)?;
                if [b"plane".as_slice(), b"sphere", b"cone", b"torus"].contains(&name) {
                    let at = next_token(span, pos, int_width)?;
                    let mut end = at;
                    let geometry =
                        decode_embedded_surface(span, &mut end, int_width).or_else(|| {
                            decode_surface_block(span, at, int_width)
                                .map(|decoded| SurfaceGeometry::Nurbs(decoded.surface))
                        });
                    support_geometries.push(geometry);
                }
            }
            0x15 if read_int(span, pos + 1, int_width) == Some(-1) => radius_boundary = Some(pos),
            _ => {}
        }
        pos = next_token(span, pos, int_width)?;
    }
    let boundary = radius_boundary?;
    let mut radius_values = Vec::new();
    let mut pos = header_len;
    while pos < boundary {
        if span[pos] == 0x06 {
            radius_values.push(read_f64(span, pos + 1)?);
        }
        pos = next_token(span, pos, int_width)?;
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
        .filter_map(|at| decode_curve_block(span, at, int_width))
        .map(|decoded| decoded.curve)
        .next_back();
    let supports: [Option<SurfaceGeometry>; 2] = support_geometries
        .into_iter()
        .chain(std::iter::repeat(None))
        .take(2)
        .collect::<Vec<_>>()
        .try_into()
        .expect("two support slots collected");
    let cache_fit_tolerance = (span.get(cache.end) == Some(&0x06))
        .then(|| read_f64(span, cache.end + 1).map(|v| v * LEN_TO_MM))
        .flatten();

    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Blend {
            supports: Box::new(supports),
            spine: center_curve,
            radius,
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance,
    })
}
