// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::items_after_test_module)]
//! Blend spline-surface decoders (cylindrical, rolling-ball, variable, vertex, and rb blends).

use crate::kernel_header::RefWidth;
use crate::nurbs::core::{
    curve_block, decode_curve_block, decode_owned_curve_cache_at, decode_owned_surface_cache_at,
    decode_surface_block, surface_block,
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
    EmbeddedRollingBallThirdSide, EmbeddedVariableBlend, EmbeddedVertexBlend,
    EmbeddedVertexBlendBoundary, EmbeddedVertexBlendBoundaryGeometry, RevisionSurfaceTail,
};
use crate::nurbs::reader::{
    marker_at, take_bool, take_f64, take_native_ident, take_native_string, take_native_vec3,
    take_optional_range_value, take_tagged_int, Nullable, LEN_TO_MM,
};
use crate::nurbs::subtypes::subtype_span;
use crate::nurbs::toks::{self, Cur, SubtypeTable};
use crate::sab::Token;
use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::geometry::{
    pcurve::{PcurveGeometry, PcurveNurbs},
    BlendCrossSection, CurveGeometry, RollingBallSide, RollingBallSideExtension,
    RollingBallSupportCurve, RollingBallSupportSurface, SolvedCurveGeometry, SolvedSurfaceGeometry,
    SurfaceGeometry, VariableBlendCache,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::PositiveI64;

const UNSET_VARIABLE_BLEND_TANGENT: f64 = 1.0e37;

macro_rules! propagate_resource {
    ($result:expr) => {
        match $result {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        }
    };
}

/// Decode an inline `cyl_spl_sur` translational-extrusion definition.
pub(super) fn cyl_spl_sur(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<Result<DecodedProceduralSurface, cadmpeg_core::CodecError>> {
    let names = ["cyl_spl_sur", "cylsur"];
    let (start, _) = propagate_resource!(toks::find_owned_subtype_marker(ctx, toks, &names)?);
    let scope = toks::subtype_span(toks, start)?;
    let span = scope.tokens();
    let mut cur = Cur::at(span, 2);
    // The revision-gated layout stores the directrix as a nested intcurve scope
    // and ends with the shared revision-gated surface tail, so its cache is
    // located by parsing that tail. The compact layout has no tail: its optional
    // final surface cache is the last surface block in the scope.
    if matches!(cur.peek(), Some(Token::Long(_))) {
        let revision = PositiveI64::new(cur.take_long()?)?;
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
        let directrix =
            propagate_resource!(embedded_base_curve_resolving_refs(ctx, &mut cur, table)?);
        let start = cur.take_optional_range_value()?.value();
        let end = cur.take_optional_range_value()?.value();
        let interval = [start?, end?];
        let direction = cur.take_vector3()?;
        let native_position = cur.take_position()?;
        let RevisionSurfaceTail {
            cache,
            discontinuities,
            tail_flag,
        } = propagate_resource!(revision_surface_tail(ctx, &mut cur)?);
        cur.at_scope_end().then_some(())?;
        Some(Ok(DecodedProceduralSurface::revision(
            DecodedProceduralSurfaceDefinition::Extrusion {
                directrix,
                parameter_interval: interval,
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
                revision_form: Some(cadmpeg_ir::geometry::RevisionSurfaceForm {
                    revision,
                    support_bounds: [None; 4],
                    reference_endpoints: [None; 2],
                    second_endpoints: [None; 2],
                    flags: vec![directrix_sense],
                    cache: cache.into_form()?,
                    discontinuities,
                    tail_flag,
                    trailing_flags: Vec::new(),
                }),
            },
        )))
    } else {
        let directrix = propagate_resource!(crate::nurbs::core::curve_cache(ctx, span)?);
        let interval = [cur.take_f64()?, cur.take_f64()?];
        let direction = cur.take_vector3()?;
        let native_position = cur.take_position()?;
        let cache = propagate_resource!(scope.owned_marker_positions(ctx))
            .into_iter()
            .rev()
            .find_map(|at| surface_block(ctx, span, at));
        let cache_fit_tolerance = match cache {
            Some(Ok((_, cache_end))) => match span.get(cache_end) {
                Some(Token::Double(value)) => Some(*value * LEN_TO_MM),
                _ => None,
            },
            Some(Err(error)) => return Some(Err(error)),
            None => None,
        };
        Some(Ok(DecodedProceduralSurface::legacy(
            DecodedProceduralSurfaceDefinition::Extrusion {
                directrix,
                parameter_interval: interval,
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
                revision_form: None,
            },
            cache_fit_tolerance,
        )))
    }
}

pub(super) fn decode_rolling_ball_side(
    bytes: &[u8],
    position: &mut usize,
    int_width: RefWidth,
) -> Option<
    Result<
        RollingBallSide<SurfaceGeometry, CurveGeometry, PcurveNurbs>,
        cadmpeg_core::decode::ResourceLimit,
    >,
> {
    use cadmpeg_ir::geometry::VariableBlendSupportKind;
    let support_kind = match take_native_string(bytes, position, int_width)?.as_str() {
        "blend_support_cos_curve" | "blendsupcos" => VariableBlendSupportKind::CosineCurve,
        "blend_support_curve" | "blendsupcur" => VariableBlendSupportKind::Curve,
        "blend_support_point_curve" | "blendsuppnt" => VariableBlendSupportKind::PointCurve,
        "blend_support_surface" | "blendsupsur" => VariableBlendSupportKind::Surface,
        "blend_support_zero_curve" | "blendsupzro" => VariableBlendSupportKind::ZeroCurve,
        _ => return None,
    };
    let surface = decode_optional_rolling_ball_surface(bytes, position, int_width)?.value();
    let saved = *position;
    let curve = if take_native_ident(bytes, position).as_deref() == Some("null_curve") {
        None
    } else {
        *position = saved;
        Some(decode_rolling_ball_curve(bytes, position, int_width)?)
    };
    let curve = match curve {
        Some(Ok(curve)) => Some(curve),
        Some(Err(limit)) => return Some(Err(limit)),
        None => None,
    };
    let pcurve = decode_nullable_embedded_pcurve(bytes, position, int_width)?.value();
    let location = take_native_vec3(bytes, position, 0x13)?;
    let secondary_pcurve = decode_nullable_embedded_pcurve(bytes, position, int_width)?.value();
    let extension_start = *position;
    let extension_fields = (|| {
        let extension = take_tagged_int(bytes, position, 0x04, int_width)?;
        let tertiary = decode_nullable_embedded_pcurve(bytes, position, int_width)?.value();
        Some(RollingBallSideExtension {
            value: extension,
            pcurve: tertiary,
        })
    })();
    let extension = match extension_fields {
        Some(extension) => Some(extension),
        None => {
            *position = extension_start;
            None
        }
    };
    Some(Ok(RollingBallSide {
        support_kind,
        surface,
        curve,
        pcurve,
        location: Point3::new(
            location[0] * LEN_TO_MM,
            location[1] * LEN_TO_MM,
            location[2] * LEN_TO_MM,
        ),
        secondary_pcurve,
        extension,
    }))
}

/// A support-surface slot: the `null_surface` ident, or an embedded surface and
/// its parameter bounds.
pub(super) fn decode_optional_rolling_ball_surface(
    bytes: &[u8],
    position: &mut usize,
    int_width: RefWidth,
) -> Option<Nullable<RollingBallSupportSurface<SurfaceGeometry>>> {
    let saved = *position;
    if take_native_ident(bytes, position).as_deref() == Some("null_surface") {
        return Some(Nullable::Null);
    }
    *position = saved;
    decode_rolling_ball_surface(bytes, position, int_width).map(|(surface, parameter_ranges)| {
        Nullable::Value(RollingBallSupportSurface {
            surface,
            parameter_ranges,
        })
    })
}

pub(super) fn decode_rolling_ball_surface(
    bytes: &[u8],
    position: &mut usize,
    int_width: RefWidth,
) -> Option<(SurfaceGeometry, [[Option<f64>; 2]; 2])> {
    let saved = *position;
    let kind = take_native_ident(bytes, position)?;
    if kind == "spline" {
        if marker_at(bytes, *position).is_some() {
            let surface = decode_surface_block(bytes, *position, int_width)?;
            *position = surface.end();
            let ranges = decode_surface_ranges(bytes, position)?;
            return Some((
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface.surface)),
                ranges,
            ));
        }
        take_bool(bytes, position)?;
        let scope = subtype_span(bytes, *position, int_width)?;
        let surface = decode_owned_surface_cache_at(scope, int_width)?;
        *position += scope.bytes().len();
        let ranges = decode_surface_ranges(bytes, position)?;
        return Some((
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)),
            ranges,
        ));
    }
    *position = saved;
    decode_embedded_surface_with_ranges(bytes, position, int_width)
}

pub(super) fn decode_surface_ranges(
    bytes: &[u8],
    position: &mut usize,
) -> Option<[[Option<f64>; 2]; 2]> {
    Some([
        [
            take_optional_range_value(bytes, position)?.value(),
            take_optional_range_value(bytes, position)?.value(),
        ],
        [
            take_optional_range_value(bytes, position)?.value(),
            take_optional_range_value(bytes, position)?.value(),
        ],
    ])
}

pub(super) fn decode_rolling_ball_curve(
    bytes: &[u8],
    position: &mut usize,
    int_width: RefWidth,
) -> Option<Result<RollingBallSupportCurve<CurveGeometry>, cadmpeg_core::decode::ResourceLimit>> {
    if marker_at(bytes, *position).is_some() {
        let curve = decode_curve_block(bytes, *position, int_width)?;
        *position = curve.end();
        let parameter_range = [
            take_optional_range_value(bytes, position)?.value(),
            take_optional_range_value(bytes, position)?.value(),
        ];
        return Some(Ok(RollingBallSupportCurve {
            curve: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve.curve)),
            parameter_range,
        }));
    }
    let kind = take_native_ident(bytes, position)?;
    if kind == "intcurve" {
        take_bool(bytes, position)?;
        let scope = subtype_span(bytes, *position, int_width)?;
        let curve = decode_owned_curve_cache_at(scope, int_width)
            .map(Ok)
            .or_else(|| decode_par_int_cur_isoline(scope.bytes(), int_width))?;
        *position += scope.bytes().len();
        let parameter_range = [
            take_optional_range_value(bytes, position)?.value(),
            take_optional_range_value(bytes, position)?.value(),
        ];
        return Some(curve.map(|curve| RollingBallSupportCurve {
            curve: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
            parameter_range,
        }));
    }
    let geometry = match kind.as_str() {
        "straight" => {
            let origin = take_native_vec3(bytes, position, 0x13)?;
            let direction = take_native_vec3(bytes, position, 0x14)?;
            CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    Point3::new(
                        origin[0] * LEN_TO_MM,
                        origin[1] * LEN_TO_MM,
                        origin[2] * LEN_TO_MM,
                    ),
                    FiniteVector3::new(Vector3::new(direction[0], direction[1], direction[2]))?
                        .unit_nonzero()?,
                )
                .ok()?,
            ))
        }
        "ellipse" => {
            let center = take_native_vec3(bytes, position, 0x13)?;
            let axis = take_native_vec3(bytes, position, 0x14)?;
            let reference = take_native_vec3(bytes, position, 0x14)?;
            let ratio = take_f64(bytes, position)?;
            let reference = Vector3::new(reference[0], reference[1], reference[2]);
            let major_radius = reference.norm() * LEN_TO_MM;
            if (ratio.abs() - 1.0).abs() <= f64::EPSILON {
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        Point3::new(
                            center[0] * LEN_TO_MM,
                            center[1] * LEN_TO_MM,
                            center[2] * LEN_TO_MM,
                        ),
                        FiniteVector3::new(Vector3::new(axis[0], axis[1], axis[2]))?
                            .unit_nonzero()?,
                        FiniteVector3::new(reference)?.unit_nonzero()?,
                        major_radius,
                    )
                    .ok()?,
                ))
            } else {
                CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
                    cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
                        Point3::new(
                            center[0] * LEN_TO_MM,
                            center[1] * LEN_TO_MM,
                            center[2] * LEN_TO_MM,
                        ),
                        FiniteVector3::new(Vector3::new(axis[0], axis[1], axis[2]))?
                            .unit_nonzero()?,
                        FiniteVector3::new(reference)?.unit_nonzero()?,
                        major_radius,
                        major_radius * ratio.abs(),
                    )
                    .ok()?,
                ))
            }
        }
        "degenerate_curve" => {
            let point = take_native_vec3(bytes, position, 0x13)?;
            CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(
                cadmpeg_ir::geometry::analytic::DegenerateCurve::try_new(Point3::new(
                    point[0] * LEN_TO_MM,
                    point[1] * LEN_TO_MM,
                    point[2] * LEN_TO_MM,
                ))
                .ok()?,
            ))
        }
        _ => return None,
    };
    let parameter_range = [
        take_optional_range_value(bytes, position)?.value(),
        take_optional_range_value(bytes, position)?.value(),
    ];
    Some(Ok(RollingBallSupportCurve {
        curve: geometry,
        parameter_range,
    }))
}

/// Decode one rolling-ball support side. Token-space counterpart of
/// [`decode_rolling_ball_side`].
pub(super) fn rolling_ball_side(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    cur: &mut Cur<'_>,
    reference_context: Option<&SubtypeTable>,
) -> Option<
    Result<RollingBallSide<SurfaceGeometry, CurveGeometry, PcurveNurbs>, cadmpeg_core::CodecError>,
> {
    use cadmpeg_ir::geometry::VariableBlendSupportKind;
    let support_kind = match cur.take_str()? {
        "blend_support_cos_curve" | "blendsupcos" => VariableBlendSupportKind::CosineCurve,
        "blend_support_curve" | "blendsupcur" => VariableBlendSupportKind::Curve,
        "blend_support_point_curve" | "blendsuppnt" => VariableBlendSupportKind::PointCurve,
        "blend_support_surface" | "blendsupsur" => VariableBlendSupportKind::Surface,
        "blend_support_zero_curve" | "blendsupzro" => VariableBlendSupportKind::ZeroCurve,
        _ => return None,
    };
    let surface =
        propagate_resource!(optional_rolling_ball_surface(ctx, cur, reference_context)?).value();
    let saved = cur.pos();
    let curve = if cur.take_ident() == Some("null_curve") {
        None
    } else {
        cur.set_pos(saved);
        Some(propagate_resource!(rolling_ball_curve(
            ctx,
            cur,
            reference_context
        )?))
    };
    let pcurve = propagate_resource!(nullable_embedded_pcurve(ctx, cur)?).value();
    let location = cur.take_position()?;
    let secondary_pcurve = propagate_resource!(nullable_embedded_pcurve(ctx, cur)?).value();
    let extension_start = cur.pos();
    let extension = if let Some(value) = cur.take_long() {
        match nullable_embedded_pcurve(ctx, cur) {
            Some(Ok(tertiary)) => Some(RollingBallSideExtension {
                value,
                pcurve: tertiary.value(),
            }),
            Some(Err(error)) => return Some(Err(error)),
            None => {
                cur.set_pos(extension_start);
                None
            }
        }
    } else {
        cur.set_pos(extension_start);
        None
    };
    Some(Ok(RollingBallSide {
        support_kind,
        surface,
        curve,
        pcurve,
        location: Point3::new(
            location[0] * LEN_TO_MM,
            location[1] * LEN_TO_MM,
            location[2] * LEN_TO_MM,
        ),
        secondary_pcurve,
        extension,
    }))
}

/// A support-surface slot: the `null_surface` ident, or an embedded surface and
/// its parameter bounds. Token-space counterpart of
/// [`decode_optional_rolling_ball_surface`].
pub(super) fn optional_rolling_ball_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    cur: &mut Cur<'_>,
    reference_context: Option<&SubtypeTable>,
) -> Option<Result<Nullable<RollingBallSupportSurface<SurfaceGeometry>>, cadmpeg_core::CodecError>>
{
    let saved = cur.pos();
    if cur.take_ident() == Some("null_surface") {
        return Some(Ok(Nullable::Null));
    }
    cur.set_pos(saved);
    rolling_ball_surface(ctx, cur, reference_context).map(|result| result.map(Nullable::Value))
}

/// Decode one rolling-ball support surface. Token-space counterpart of
/// [`decode_rolling_ball_surface`].
fn rolling_ball_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    cur: &mut Cur<'_>,
    reference_context: Option<&SubtypeTable>,
) -> Option<Result<RollingBallSupportSurface<SurfaceGeometry>, cadmpeg_core::CodecError>> {
    let toks = cur.toks();
    let saved = cur.pos();
    let kind = cur.take_ident()?;
    if kind == "spline" {
        if toks::marker_at(toks, cur.pos()).is_some() {
            let (surface, surface_end) = propagate_resource!(surface_block(ctx, toks, cur.pos())?);
            cur.set_pos(surface_end);
            let ranges = surface_ranges(cur)?;
            return Some(Ok(RollingBallSupportSurface {
                surface: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)),
                parameter_ranges: ranges,
            }));
        }
        cur.take_bool()?;
        let scope = toks::subtype_span(toks, cur.pos())?;
        let surface = propagate_resource!(reference_context
            .and_then(
                |table| crate::nurbs::core::owned_surface_cache_resolving_refs(ctx, scope, table)
            )
            .or_else(|| crate::nurbs::core::owned_surface_cache(ctx, scope))?);
        cur.set_pos(cur.pos() + scope.tokens().len());
        let ranges = surface_ranges(cur)?;
        return Some(Ok(RollingBallSupportSurface {
            surface: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)),
            parameter_ranges: ranges,
        }));
    }
    cur.set_pos(saved);
    embedded_surface_with_ranges(ctx, cur).map(|result| {
        result.map(|surface| RollingBallSupportSurface {
            surface: surface.surface,
            parameter_ranges: surface.ranges,
        })
    })
}

/// Four optional U/V range bounds. Token-space counterpart of
/// [`decode_surface_ranges`].
pub(super) fn surface_ranges(cur: &mut Cur<'_>) -> Option<[[Option<f64>; 2]; 2]> {
    Some([
        [
            cur.take_optional_range_value()?.value(),
            cur.take_optional_range_value()?.value(),
        ],
        [
            cur.take_optional_range_value()?.value(),
            cur.take_optional_range_value()?.value(),
        ],
    ])
}

/// Decode one rolling-ball curve slot. Token-space counterpart of
/// [`decode_rolling_ball_curve`].
pub(super) fn rolling_ball_curve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    cur: &mut Cur<'_>,
    reference_context: Option<&SubtypeTable>,
) -> Option<Result<RollingBallSupportCurve<CurveGeometry>, cadmpeg_core::CodecError>> {
    let toks = cur.toks();
    if toks::marker_at(toks, cur.pos()).is_some() {
        let (curve, curve_end) = propagate_resource!(curve_block(ctx, toks, cur.pos())?);
        cur.set_pos(curve_end);
        let parameter_range = [
            cur.take_optional_range_value()?.value(),
            cur.take_optional_range_value()?.value(),
        ];
        return Some(Ok(RollingBallSupportCurve {
            curve: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
            parameter_range,
        }));
    }
    let kind = cur.take_ident()?;
    if kind == "intcurve" {
        cur.take_bool()?;
        let scope = toks::subtype_span(toks, cur.pos())?;
        let curve = reference_context
            .and_then(|table| {
                crate::nurbs::core::owned_curve_cache_resolving_refs(ctx, scope, table)
            })
            .or_else(|| crate::nurbs::core::owned_curve_cache(ctx, scope))
            .or_else(|| par_int_cur_isoline(ctx, scope.tokens(), reference_context))?;
        cur.set_pos(cur.pos() + scope.tokens().len());
        let parameter_range = [
            cur.take_optional_range_value()?.value(),
            cur.take_optional_range_value()?.value(),
        ];
        return Some(curve.map(|curve| RollingBallSupportCurve {
            curve: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
            parameter_range,
        }));
    }
    let geometry = match kind {
        "straight" => {
            let origin = cur.take_position()?;
            let direction = cur.take_vector3()?;
            CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    Point3::new(
                        origin[0] * LEN_TO_MM,
                        origin[1] * LEN_TO_MM,
                        origin[2] * LEN_TO_MM,
                    ),
                    FiniteVector3::new(Vector3::new(direction[0], direction[1], direction[2]))?
                        .unit_nonzero()?,
                )
                .ok()?,
            ))
        }
        "ellipse" => {
            let center = cur.take_position()?;
            let axis = cur.take_vector3()?;
            let reference = cur.take_vector3()?;
            let ratio = cur.take_f64()?;
            let reference = Vector3::new(reference[0], reference[1], reference[2]);
            let major_radius = reference.norm() * LEN_TO_MM;
            if (ratio.abs() - 1.0).abs() <= f64::EPSILON {
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        Point3::new(
                            center[0] * LEN_TO_MM,
                            center[1] * LEN_TO_MM,
                            center[2] * LEN_TO_MM,
                        ),
                        FiniteVector3::new(Vector3::new(axis[0], axis[1], axis[2]))?
                            .unit_nonzero()?,
                        FiniteVector3::new(reference)?.unit_nonzero()?,
                        major_radius,
                    )
                    .ok()?,
                ))
            } else {
                CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
                    cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
                        Point3::new(
                            center[0] * LEN_TO_MM,
                            center[1] * LEN_TO_MM,
                            center[2] * LEN_TO_MM,
                        ),
                        FiniteVector3::new(Vector3::new(axis[0], axis[1], axis[2]))?
                            .unit_nonzero()?,
                        FiniteVector3::new(reference)?.unit_nonzero()?,
                        major_radius,
                        major_radius * ratio.abs(),
                    )
                    .ok()?,
                ))
            }
        }
        "degenerate_curve" => {
            let point = cur.take_position()?;
            CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(
                cadmpeg_ir::geometry::analytic::DegenerateCurve::try_new(Point3::new(
                    point[0] * LEN_TO_MM,
                    point[1] * LEN_TO_MM,
                    point[2] * LEN_TO_MM,
                ))
                .ok()?,
            ))
        }
        _ => return None,
    };
    let parameter_range = [
        cur.take_optional_range_value()?.value(),
        cur.take_optional_range_value()?.value(),
    ];
    Some(Ok(RollingBallSupportCurve {
        curve: geometry,
        parameter_range,
    }))
}

fn rolling_ball_third_side(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    cur: &mut Cur<'_>,
) -> Option<Result<EmbeddedRollingBallThirdSide, cadmpeg_core::CodecError>> {
    let label = propagate_resource!(crate::decode_alloc::copy_string(
        ctx,
        cur.take_str()?,
        "ASM rolling ball third-side label"
    ));
    let surface = propagate_resource!(embedded_surface(ctx, cur)?);
    let (curve, curve_end) = propagate_resource!(curve_block(ctx, cur.toks(), cur.pos())?);
    cur.set_pos(curve_end);
    let pcurve = propagate_resource!(nullable_embedded_pcurve(ctx, cur)?).value();
    let direction = cur.take_vector3()?;
    let secondary_pcurve = propagate_resource!(nullable_embedded_pcurve(ctx, cur)?).value();
    let extension = cur.take_long()?;
    let tertiary_pcurve = propagate_resource!(nullable_embedded_pcurve(ctx, cur)?).value();
    let flag = cur.take_bool()?;
    Some(Ok(EmbeddedRollingBallThirdSide {
        label,
        surface,
        curve,
        pcurve,
        direction: Vector3::new(direction[0], direction[1], direction[2]),
        secondary_pcurve,
        extension,
        tertiary_pcurve,
        flag,
    }))
}

fn blend_value_name<'a>(cur: &mut Cur<'a>) -> Option<&'a str> {
    let saved = cur.pos();
    if let Some(value) = cur.take_str() {
        return Some(value);
    }
    cur.set_pos(saved);
    cur.take_ident()
}

fn radius_function_geometry(mut function: PcurveNurbs) -> Option<PcurveGeometry> {
    function
        .edit_control_points(|point| {
            point.u *= LEN_TO_MM;
            Ok(())
        })
        .ok()?;
    Some(PcurveGeometry::Nurbs { nurbs: function })
}

fn variable_blend_value(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    cur: &mut Cur<'_>,
    depth: usize,
) -> Option<Result<cadmpeg_ir::geometry::VariableBlendValue, cadmpeg_core::CodecError>> {
    use cadmpeg_ir::geometry::{
        EdgeOffsetDiscriminator, VariableBlendInterpolationPoint, VariableBlendTerminal,
        VariableBlendValue, VariableBlendValuePayload,
    };
    let _depth_guard = propagate_resource!(ctx.enter_nested("decode ASM variable blend value"));
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
    let modern_flag = cur.take_bool()?;
    let payload = match name {
        "fixed_width" => VariableBlendValuePayload::FixedWidth {
            discriminator,
            parameters: [cur.take_f64()?, cur.take_f64()?],
            width: cur.take_f64()?,
        },
        "two_ends" => VariableBlendValuePayload::TwoEnds {
            discriminator,
            parameters: [cur.take_f64()?, cur.take_f64()?],
            radii: [cur.take_f64()? * LEN_TO_MM, cur.take_f64()? * LEN_TO_MM],
        },
        // The payload is the law-domain parameter range and one offset, so the
        // second field is a parameter and only the third is a length. The
        // sub-discriminator selects no layout here; it is still read and written
        // as the format stores it, and no value outside `0` and `1` is defined.
        "edge_offset" => VariableBlendValuePayload::EdgeOffset {
            discriminator: EdgeOffsetDiscriminator::from_code(discriminator)?,
            scalars: [cur.take_f64()?, cur.take_f64()?],
            lengths: [cur.take_f64()? * LEN_TO_MM],
        },
        "functional" => {
            let parameter = cur.take_f64()?;
            let radius = cur.take_f64()? * LEN_TO_MM;
            let (function, end) =
                propagate_resource!(pcurve_block_with_end(ctx, cur.toks(), cur.pos())?);
            cur.set_pos(end);
            let terminal = if matches!(cur.peek(), Some(Token::Double(_))) {
                VariableBlendTerminal::Double(cur.take_f64()?)
            } else {
                VariableBlendTerminal::Text(propagate_resource!(crate::decode_alloc::copy_string(
                    ctx,
                    blend_value_name(cur)?,
                    "ASM variable blend terminal text",
                )))
            };
            VariableBlendValuePayload::Functional {
                discriminator,
                parameter,
                radius,
                function: radius_function_geometry(function)?,
                terminal,
            }
        }
        "const" => VariableBlendValuePayload::Constant {
            discriminator,
            parameters: [cur.take_f64()?, cur.take_f64()?],
            radius: cur.take_f64()? * LEN_TO_MM,
            variable_chamfer: cur.take_enum()?,
            chamfer_type: cur.take_enum()?,
            nested: Box::new(match variable_blend_value(ctx, cur, depth + 1)? {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            }),
        },
        "interp" => {
            let parameter = cur.take_f64()?;
            let radius = cur.take_f64()? * LEN_TO_MM;
            let (function, end) =
                propagate_resource!(pcurve_block_with_end(ctx, cur.toks(), cur.pos())?);
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
            if let Err(error) = ctx
                .charge_collection_items(count as u64, "decode variable blend interpolation points")
            {
                return Some(Err(error));
            }
            let mut points = Vec::new();
            if points.try_reserve(count).is_err() {
                return Some(Err(ctx.refuse_codec_limit(
                    "reserve variable blend interpolation points",
                    count as u64,
                    count as u64,
                )));
            }
            for _ in 0..count {
                let parameter = cur.take_f64()?;
                let radius = cur.take_f64()? * LEN_TO_MM;
                let tangents = [cur.take_f64()?, cur.take_f64()?]
                    .map(|value| (value != UNSET_VARIABLE_BLEND_TANGENT).then_some(value));
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
                discriminator,
                parameter,
                radius,
                function: radius_function_geometry(function)?,
                enum_count,
                enum_tagged,
                points,
            }
        }
        _ => return None,
    };
    Some(Ok(VariableBlendValue {
        modern_flag,
        calibrated,
        payload,
    }))
}

#[cfg(test)]
mod variable_blend_value_tests {
    use super::{rolling_ball_third_side, variable_blend_value, UNSET_VARIABLE_BLEND_TANGENT};
    use crate::kernel_header::RefWidth;
    use crate::nurbs::toks::Cur;
    use crate::sab::Token;
    use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
    use cadmpeg_ir::geometry::VariableBlendValuePayload;

    #[test]
    fn rolling_ball_third_side_label_refuses_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let tokens = [Token::Str("label".into())];
        let mut cur = Cur::at(&tokens, 0);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 4;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty test input fits input limit");
        let Some(Err(CodecError::ResourceLimit(refusal))) = rolling_ball_third_side(&ctx, &mut cur)
        else {
            panic!("label copy must refuse retained limit");
        };
        assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(refusal.operation, "ASM rolling ball third-side label");
    }

    #[test]
    fn variable_blend_terminal_text_refuses_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let tokens = [
            Token::Str("functional".into()),
            Token::Enum(0),
            Token::True,
            Token::Double(0.0),
            Token::Double(1.0),
            Token::Ident("nubs".into()),
            Token::Long(1),
            Token::Enum(0),
            Token::Long(2),
            Token::Double(0.0),
            Token::Long(1),
            Token::Double(1.0),
            Token::Long(1),
            Token::Double(0.0),
            Token::Double(0.0),
            Token::Double(1.0),
            Token::Double(0.0),
            Token::Str("terminal".into()),
        ];
        let mut cur = Cur::at(&tokens, 0);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 7;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty test input fits input limit");
        let Some(Err(CodecError::ResourceLimit(refusal))) = variable_blend_value(&ctx, &mut cur, 0)
        else {
            panic!("terminal text copy must refuse retained limit");
        };
        assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(refusal.operation, "ASM variable blend terminal text");
    }

    #[test]
    fn variable_blend_value_refuses_recursion_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let tokens = [
            Token::Str("fixed_width".into()),
            Token::Enum(0),
            Token::True,
            Token::Double(0.0),
            Token::Double(1.0),
            Token::Double(2.0),
        ];
        let mut cur = Cur::at(&tokens, 0);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty test input fits input limit");
        let Some(Err(CodecError::ResourceLimit(refusal))) = variable_blend_value(&ctx, &mut cur, 0)
        else {
            panic!("variable blend depth must refuse");
        };
        assert_eq!(refusal.dimension, ResourceDimension::RecursionDepth);
        assert_eq!(refusal.operation, "decode ASM variable blend value");
    }

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
        let asm_decode_arena = cadmpeg_core::decode::DecodeArena::new();
        let (asm_decode_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &asm_decode_arena,
            &cadmpeg_core::decode::DecodePolicy::default(),
        )
        .expect("test decode context");
        let mut direct = Vec::new();
        two_ends(&mut direct);
        let toks = crate::nurbs::toks::lex_test_span(&direct, RefWidth::Eight)
            .expect("valid single-record byte fixture");
        let mut cur = Cur::at(&toks, 0);
        let decoded = variable_blend_value(&asm_decode_ctx, &mut cur, 0)
            .expect("generated two-ends value")
            .expect("resource allocation did not fail");
        assert_eq!(cur.pos(), toks.len());
        assert!(decoded.modern_flag);
        assert_eq!(decoded.payload.discriminator(), 7);
        let VariableBlendValuePayload::TwoEnds {
            parameters, radii, ..
        } = decoded.payload
        else {
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
        let toks = crate::nurbs::toks::lex_test_span(&recursive, RefWidth::Eight)
            .expect("valid single-record byte fixture");
        let mut cur = Cur::at(&toks, 0);
        let decoded = variable_blend_value(&asm_decode_ctx, &mut cur, 0)
            .expect("generated recursive const value")
            .expect("resource allocation did not fail");
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
        let asm_decode_arena = cadmpeg_core::decode::DecodeArena::new();
        let (asm_decode_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &asm_decode_arena,
            &cadmpeg_core::decode::DecodePolicy::default(),
        )
        .expect("test decode context");
        let mut bytes = Vec::new();
        text(&mut bytes, "fixed_width");
        integer(&mut bytes, 0x15, 0);
        bytes.push(0x0a);
        // Distinct parameter-range bounds and a distinct chamfer width.
        for value in [0.5, 3.5, 0.1905] {
            double(&mut bytes, value);
        }
        let toks = crate::nurbs::toks::lex_test_span(&bytes, RefWidth::Eight)
            .expect("valid single-record byte fixture");
        let mut cur = Cur::at(&toks, 0);
        let decoded = variable_blend_value(&asm_decode_ctx, &mut cur, 0)
            .expect("generated fixed-width value")
            .expect("resource allocation did not fail");
        assert_eq!(cur.pos(), toks.len());
        let VariableBlendValuePayload::FixedWidth {
            parameters, width, ..
        } = decoded.payload
        else {
            panic!("expected fixed-width payload")
        };
        assert_eq!(parameters, [0.5, 3.5]);
        assert_eq!(width, 0.1905);
    }

    #[test]
    fn decodes_generated_enum_tagged_interp_counts() {
        let asm_decode_arena = cadmpeg_core::decode::DecodeArena::new();
        let (asm_decode_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &asm_decode_arena,
            &cadmpeg_core::decode::DecodePolicy::default(),
        )
        .expect("test decode context");
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
        let toks = crate::nurbs::toks::lex_test_span(&bytes, RefWidth::Eight)
            .expect("valid single-record byte fixture");
        let mut cur = Cur::at(&toks, 0);
        let decoded = variable_blend_value(&asm_decode_ctx, &mut cur, 0)
            .expect("generated enum-tagged interp value")
            .expect("resource allocation did not fail");
        assert_eq!(cur.pos(), toks.len() - 1);
        let VariableBlendValuePayload::Interpolated {
            enum_count,
            enum_tagged,
            function,
            points,
            ..
        } = decoded.payload
        else {
            panic!("expected interpolated payload")
        };
        assert_eq!(enum_count, 2);
        assert!(enum_tagged);
        assert_eq!(points.len(), 1);
        let PcurveGeometry::Nurbs { nurbs } = function else {
            panic!("expected NURBS radius function")
        };
        assert_eq!(
            nurbs.control_points()[0],
            cadmpeg_ir::math::Point2::new(0.0, 0.0)
        );
        assert_eq!(
            nurbs.control_points()[1],
            cadmpeg_ir::math::Point2::new(10.0, 1.0)
        );
    }

    #[test]
    fn variable_blend_interpolation_points_refuse_collection_limit() {
        let mut bytes = Vec::new();
        text(&mut bytes, "interp");
        integer(&mut bytes, 0x15, 0);
        bytes.push(0x0a);
        double(&mut bytes, 0.0);
        double(&mut bytes, 1.0);
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
        integer(&mut bytes, 0x15, 2);
        integer(&mut bytes, 0x04, 1);
        double(&mut bytes, 0.5);
        double(&mut bytes, 1.5);
        double(&mut bytes, 0.0);
        double(&mut bytes, 1.0);
        bytes.push(0x13);
        for value in [1.0_f64, 2.0, 3.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x14);
        for value in [0.0_f64, 0.0, 1.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let tokens = crate::nurbs::toks::lex_test_span(&bytes, RefWidth::Eight)
            .expect("valid interpolation value");
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("input is within the root byte limit");
        let mut cur = Cur::at(&tokens, 0);
        let result = variable_blend_value(&ctx, &mut cur, 0)
            .expect("interpolation grammar reaches its point collection");
        assert!(matches!(
            result,
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
        ));
    }

    #[test]
    fn decodes_interp_point_with_unset_derivatives() {
        let asm_decode_arena = cadmpeg_core::decode::DecodeArena::new();
        let (asm_decode_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &asm_decode_arena,
            &cadmpeg_core::decode::DecodePolicy::default(),
        )
        .expect("test decode context");
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
        double(&mut bytes, UNSET_VARIABLE_BLEND_TANGENT);
        double(&mut bytes, UNSET_VARIABLE_BLEND_TANGENT);
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
        let toks = crate::nurbs::toks::lex_test_span(&bytes, RefWidth::Eight)
            .expect("valid single-record byte fixture");
        let mut cur = Cur::at(&toks, 0);
        let decoded = variable_blend_value(&asm_decode_ctx, &mut cur, 0)
            .expect("generated interp value with unset derivatives")
            .expect("resource allocation did not fail");
        assert_eq!(cur.pos(), toks.len() - 1);
        let VariableBlendValuePayload::Interpolated { points, .. } = decoded.payload else {
            panic!("expected interpolated payload")
        };
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].tangents, [None, None]);
    }
}

pub(super) fn var_blend_spl_sur(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &[Token],
    reference_context: Option<&SubtypeTable>,
) -> Option<Result<DecodedProceduralSurface, cadmpeg_core::CodecError>> {
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
    let (start, name) = propagate_resource!(toks::find_owned_subtype_marker(ctx, toks, &names)?);
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
    let span = toks::subtype_span(toks, start)?.tokens();
    let mut cur = Cur::at(span, 2);
    let revision = PositiveI64::new(cur.take_long()?)?;
    let first = match rolling_ball_side(ctx, &mut cur, reference_context)? {
        Ok(side) => side,
        Err(limit) => return Some(Err(limit)),
    };
    let second = match rolling_ball_side(ctx, &mut cur, reference_context)? {
        Ok(side) => side,
        Err(limit) => return Some(Err(limit)),
    };
    let sides = Box::new([first, second]);
    let slice = match rolling_ball_curve(ctx, &mut cur, reference_context)? {
        Ok(curve) => curve,
        Err(limit) => return Some(Err(limit)),
    };
    let offsets = [cur.take_f64()? * LEN_TO_MM, cur.take_f64()? * LEN_TO_MM];
    let two_radii = match cur.take_enum()? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let first_value = match variable_blend_value(ctx, &mut cur, 0)? {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    let radii = if two_radii {
        cadmpeg_ir::geometry::VariableBlendRadii::Two {
            first: first_value,
            second: match variable_blend_value(ctx, &mut cur, 0)? {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            },
        }
    } else {
        cadmpeg_ir::geometry::VariableBlendRadii::Single { value: first_value }
    };
    // The cross-section clause follows the complete one- or two-radius law
    // sequence. An absent enum is the elided circular default.
    let cross_section = if matches!(cur.peek(), Some(Token::Enum(_))) {
        let selector = cur.take_enum()?;
        match selector {
            0 => Some(VariableBlendCrossSection::Circular {}),
            1 => Some(VariableBlendCrossSection::Thumbweights {
                parameters: [cur.take_f64()?, cur.take_f64()?],
            }),
            selector @ (2 | 4 | 5 | 6) => Some(VariableBlendCrossSection::UnclassifiedBare {
                selector: selector.try_into().ok()?,
            }),
            3 => {
                let radius = if cur.take_bool()? {
                    Some(Box::new(match variable_blend_value(ctx, &mut cur, 0)? {
                        Ok(value) => value,
                        Err(error) => return Some(Err(error)),
                    }))
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
        cur.take_optional_range_value()?.value(),
        cur.take_optional_range_value()?.value(),
    ];
    let [Some(u_lower), Some(u_upper)] = u_range else {
        return None;
    };
    let v_range = [
        cur.take_optional_range_value()?.value(),
        cur.take_optional_range_value()?.value(),
    ];
    let [v_lower, None] = v_range else {
        return None;
    };
    let shape_prefix = cur.take_long()?;
    let shape_parameter = cur.take_f64()?;
    let shape_length = cur.take_f64()? * LEN_TO_MM;
    let shape_tail = cur.take_long()?;
    let RevisionSurfaceTail {
        cache,
        discontinuities,
        tail_flag,
    } = propagate_resource!(revision_surface_tail(ctx, &mut cur)?);
    let tail_extensions = [cur.take_long()?, cur.take_long()?, cur.take_long()?];
    let saved = cur.pos();
    let secondary_curve = if cur.take_ident() == Some("null_curve") {
        None
    } else {
        cur.set_pos(saved);
        match rolling_ball_curve(ctx, &mut cur, reference_context)? {
            Ok(curve) => Some(curve),
            Err(limit) => return Some(Err(limit)),
        }
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
        cur.take_optional_range_value()?.value(),
        cur.take_optional_range_value()?.value(),
    ];
    let saved = cur.pos();
    let post_curve = if cur.take_ident() == Some("nullbs") {
        None
    } else {
        cur.set_pos(saved);
        let (post, post_end) = propagate_resource!(curve_block(ctx, span, cur.pos())?);
        cur.set_pos(post_end);
        Some(post)
    };
    let post_pcurve = propagate_resource!(nullable_embedded_pcurve(ctx, &mut cur)?).value();
    cur.at_scope_end().then_some(())?;
    Some(Ok(DecodedProceduralSurface::revision(
        DecodedProceduralSurfaceDefinition::VariableBlend(Box::new(EmbeddedVariableBlend {
            subtype,
            revision,
            sides,
            slice: slice.curve,
            slice_range: slice.parameter_range,
            offsets,
            radii,
            cross_section,
            u_range: [u_lower, u_upper],
            v_lower,
            shape_parameter,
            shape_length,
            shape_tail,
            cache: match cache.into_form()? {
                cadmpeg_ir::geometry::RevisionCacheForm::SolvedCache { fit_tolerance } => {
                    match std::num::NonZeroI64::new(shape_prefix) {
                        Some(shape_prefix) => VariableBlendCache::Current {
                            shape_prefix,
                            fit_tolerance,
                        },
                        None => VariableBlendCache::Stale {},
                    }
                }
                cadmpeg_ir::geometry::RevisionCacheForm::Parameterization(parameterization) => {
                    VariableBlendCache::Parameterization {
                        shape_prefix,
                        parameterization,
                    }
                }
            },
            discontinuities,
            tail_flag,
            tail_extensions,
            secondary_curve,
            convexity,
            render_mode,
            post_range,
            post_curve,
            post_pcurve,
        })),
    )))
}

fn vertex_blend_boundary(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    cur: &mut Cur<'_>,
) -> Option<Result<EmbeddedVertexBlendBoundary, cadmpeg_core::CodecError>> {
    let kind = cur.take_str()?;
    let boundary_type = cur.take_bool()?;
    let magic = cur.take_position()?;
    let u_smoothing = cur.take_bool()?;
    let v_smoothing = cur.take_bool()?;
    let fullness = cur.take_f64()?;
    let geometry = match kind {
        "circle" => {
            let (curve, curve_end) = propagate_resource!(curve_block(ctx, cur.toks(), cur.pos())?);
            cur.set_pos(curve_end);
            let form = cur.take_enum()?;
            let mut read_twist = || {
                let twist = cur.take_position()?;
                Some(Point3::new(
                    twist[0] * LEN_TO_MM,
                    twist[1] * LEN_TO_MM,
                    twist[2] * LEN_TO_MM,
                ))
            };
            let twists = match form {
                0 => cadmpeg_ir::geometry::VertexBlendTwists::None {},
                1 => cadmpeg_ir::geometry::VertexBlendTwists::One {
                    twist: read_twist()?,
                },
                3 => cadmpeg_ir::geometry::VertexBlendTwists::Two {
                    twists: [read_twist()?, read_twist()?],
                },
                _ => return None,
            };
            let parameters = [cur.take_f64()?, cur.take_f64()?];
            let sense = cur.take_bool()?;
            EmbeddedVertexBlendBoundaryGeometry::Circle {
                curve: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                curve_endpoints: [None; 2],
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
            let surface = propagate_resource!(embedded_surface(ctx, cur)?);
            let pcurve = propagate_resource!(nullable_embedded_pcurve(ctx, cur)?).value();
            let sense = cur.take_bool()?;
            let fit_tolerance =
                cadmpeg_ir::geometry::FitTolerance::try_new(cur.take_f64()?).ok()?;
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
            let (curve, curve_end) = propagate_resource!(curve_block(ctx, cur.toks(), cur.pos())?);
            cur.set_pos(curve_end);
            EmbeddedVertexBlendBoundaryGeometry::Plane {
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                parameters,
                curve: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                curve_endpoints: [None; 2],
            }
        }
        _ => return None,
    };
    Some(Ok(EmbeddedVertexBlendBoundary {
        boundary_type,
        // The magic item is a unit direction or the zero vector, not a
        // length-bearing location, so it takes no unit conversion.
        magic: Vector3::new(magic[0], magic[1], magic[2]),
        u_smoothing,
        v_smoothing,
        fullness,
        geometry,
    }))
}

/// Decode one revision-gated vertex-blend boundary: ident-token type name,
/// cross boolean, magic vector, smoothing booleans, fullness, and the
/// type-selected payload with bound-carrying supports and endpoint-carrying
/// curves.
fn revision_vertex_blend_boundary(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    cur: &mut Cur<'_>,
    resolver: Option<&SubtypeTable>,
) -> Option<Result<EmbeddedVertexBlendBoundary, cadmpeg_core::CodecError>> {
    let table = resolver?;
    let kind = cur.take_ident()?;
    let boundary_type = cur.take_bool()?;
    let magic = cur.take_vector3()?;
    let u_smoothing = cur.take_bool()?;
    let v_smoothing = cur.take_bool()?;
    let fullness = cur.take_f64()?;
    let geometry = match kind {
        "circle" => {
            let curve = propagate_resource!(embedded_base_curve_resolving_refs(ctx, cur, table)?);
            let curve_endpoints = [
                cur.take_optional_range_value()?.value(),
                cur.take_optional_range_value()?.value(),
            ];
            let form = cur.take_enum()?;
            let mut read_twist = || {
                let twist = cur.take_vector3()?;
                Some(Point3::new(
                    twist[0] * LEN_TO_MM,
                    twist[1] * LEN_TO_MM,
                    twist[2] * LEN_TO_MM,
                ))
            };
            let twists = match form {
                0 => cadmpeg_ir::geometry::VertexBlendTwists::None {},
                1 => cadmpeg_ir::geometry::VertexBlendTwists::One {
                    twist: read_twist()?,
                },
                3 => cadmpeg_ir::geometry::VertexBlendTwists::Two {
                    twists: [read_twist()?, read_twist()?],
                },
                _ => return None,
            };
            let parameters = [cur.take_f64()?, cur.take_f64()?];
            let sense = cur.take_bool()?;
            EmbeddedVertexBlendBoundaryGeometry::Circle {
                curve: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                curve_endpoints,
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
            let (surface, support_bounds) =
                match optional_embedded_surface_with_bounds(ctx, cur, table)? {
                    Ok(surface) => surface,
                    Err(error) => return Some(Err(error)),
                };
            let pcurve = propagate_resource!(nullable_embedded_pcurve(ctx, cur)?).value();
            let sense = cur.take_bool()?;
            let fit_tolerance =
                cadmpeg_ir::geometry::FitTolerance::try_new(cur.take_f64()?).ok()?;
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
            let curve = propagate_resource!(embedded_base_curve_resolving_refs(ctx, cur, table)?);
            let curve_endpoints = [
                cur.take_optional_range_value()?.value(),
                cur.take_optional_range_value()?.value(),
            ];
            EmbeddedVertexBlendBoundaryGeometry::Plane {
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                parameters,
                curve: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                curve_endpoints,
            }
        }
        _ => return None,
    };
    Some(Ok(EmbeddedVertexBlendBoundary {
        boundary_type,
        // The magic item is a unit direction or the zero vector, not a
        // length-bearing location, so it takes no unit conversion.
        magic: Vector3::new(magic[0], magic[1], magic[2]),
        u_smoothing,
        v_smoothing,
        fullness,
        geometry,
    }))
}

pub(super) fn vertex_blend_spl_sur(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<Result<DecodedProceduralSurface, cadmpeg_core::CodecError>> {
    let names = ["VBL_SURF", "vertexblendsur"];
    let (start, name) = propagate_resource!(toks::find_owned_subtype_marker(ctx, toks, &names)?);
    let span = toks::subtype_span(toks, start)?.tokens();
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
        let revision = PositiveI64::new(cur.take_long()?)?;
        Some(revision)
    } else {
        None
    };
    let count = usize::try_from(cur.take_long()?).ok()?;
    if count > 100_000 {
        return None;
    }
    let mut boundaries =
        match crate::decode_alloc::counted_vec(ctx, count, "ASM vertex blend boundaries") {
            Ok(boundaries) => boundaries,
            Err(error) => return Some(Err(error)),
        };
    for _ in 0..count {
        boundaries.push(if revision.is_some() {
            match revision_vertex_blend_boundary(ctx, &mut cur, resolver)? {
                Ok(boundary) => boundary,
                Err(error) => return Some(Err(error)),
            }
        } else {
            propagate_resource!(vertex_blend_boundary(ctx, &mut cur)?)
        });
    }
    let grid_size = cur.take_long()?;
    let fit_tolerance =
        cadmpeg_ir::geometry::FitTolerance::try_new(cur.take_f64()? * LEN_TO_MM).ok()?;
    Some(Ok(DecodedProceduralSurface::legacy(
        DecodedProceduralSurfaceDefinition::VertexBlend(Box::new(EmbeddedVertexBlend {
            revision,
            boundaries,
            grid_size,
            fit_tolerance,
        })),
        None,
    )))
}

pub(super) fn full_rb_blend_spl_sur(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &[Token],
    table: &SubtypeTable,
) -> Option<Result<DecodedProceduralSurface, cadmpeg_core::CodecError>> {
    let names = [
        "rb_blend_spl_sur",
        "rbblnsur",
        "pipe_spl_sur",
        "pipesur",
        "sss_blend_spl_sur",
        "sssblndsur",
    ];
    let (start, name) = propagate_resource!(toks::find_owned_subtype_marker(ctx, toks, &names)?);
    let has_third = name == "sss_blend_spl_sur" || name == "sssblndsur";
    let span = toks::subtype_span(toks, start)?.tokens();
    let mut cur = Cur::at(span, 2);
    let revision = PositiveI64::new(cur.take_long()?)?;
    let first = match rolling_ball_side(ctx, &mut cur, Some(table))? {
        Ok(side) => side,
        Err(limit) => return Some(Err(limit)),
    };
    let second = match rolling_ball_side(ctx, &mut cur, Some(table))? {
        Ok(side) => side,
        Err(limit) => return Some(Err(limit)),
    };
    let sides = Box::new([first, second]);
    let slice = match rolling_ball_curve(ctx, &mut cur, Some(table))? {
        Ok(curve) => curve,
        Err(limit) => return Some(Err(limit)),
    };
    let offsets = [cur.take_f64()? * LEN_TO_MM, cur.take_f64()? * LEN_TO_MM];
    let radius_selector = match cur.peek()? {
        Token::Enum(_) => {
            if cur.take_enum()? != -1 {
                return None;
            }
            None
        }
        Token::Double(_) => Some(cur.take_f64()?),
        _ => return None,
    };
    let u_range = [
        cur.take_optional_range_value()?.value(),
        cur.take_optional_range_value()?.value(),
    ];
    let v_range = [
        cur.take_optional_range_value()?.value(),
        cur.take_optional_range_value()?.value(),
    ];
    let shape_prefix = cur.take_long()?;
    let parameters = [cur.take_f64()?, cur.take_f64()?];
    let tail = cur.take_long()?;
    let RevisionSurfaceTail {
        cache,
        discontinuities,
        tail_flag,
    } = propagate_resource!(revision_surface_tail(ctx, &mut cur)?);
    let third = if has_third {
        Some(Box::new(propagate_resource!(rolling_ball_third_side(
            ctx, &mut cur
        )?)))
    } else {
        None
    };
    let tail_extensions = [cur.take_long()?, cur.take_long()?, cur.take_long()?];
    cur.at_scope_end().then_some(())?;
    Some(Ok(DecodedProceduralSurface::revision(
        DecodedProceduralSurfaceDefinition::Blend {
            supports: Box::new([None, None]),
            spine: match &slice.curve {
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) => Some(
                    propagate_resource!(curve.try_clone_for_decode(ctx, "ASM rolling ball spine")),
                ),
                _ => None,
            },
            radius_offsets: offsets,
            cross_section: BlendCrossSection::Circular,
            native: Some(Box::new(EmbeddedRollingBall {
                revision,
                sides,
                slice: slice.curve,
                slice_range: slice.parameter_range,
                offsets,
                radius_selector,
                u_range,
                v_range,
                shape_prefix,
                parameters,
                tail,
                cache: cache.into_form()?,
                discontinuities,
                tail_flag,
                third,
                tail_extensions,
            })),
        },
    )))
}

/// Decode the compact rolling-ball carrier emitted without the native side
/// graph. Every field is positional; nested construction members are not
/// searched by token kind.
pub(super) fn compact_rb_blend_spl_sur(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &[Token],
) -> Option<Result<DecodedProceduralSurface, cadmpeg_core::CodecError>> {
    let names = ["rb_blend_spl_sur", "rbblnsur", "pipe_spl_sur", "pipesur"];
    let (start, _) = propagate_resource!(toks::find_owned_subtype_marker(ctx, toks, &names)?);
    let span = toks::subtype_span(toks, start)?.tokens();
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
            let (_, end) = propagate_resource!(surface_block(ctx, span, cur.pos())?);
            cur.set_pos(end);
            None
        } else if let Some(surface) = embedded_surface(ctx, &mut cur) {
            Some(propagate_resource!(surface))
        } else {
            cur.set_pos(payload_start);
            let (surface, end) = propagate_resource!(surface_block(ctx, span, cur.pos())?);
            cur.set_pos(end);
            Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                surface,
            )))
        };
        supports[support_count] = support;
        support_count += 1;
    }
    let (spine, spine_end) = propagate_resource!(curve_block(ctx, span, cur.pos())?);
    cur.set_pos(spine_end);
    let offsets = [cur.take_f64()? * LEN_TO_MM, cur.take_f64()? * LEN_TO_MM];
    (cur.take_enum()? == -1).then_some(())?;
    let (_, cache_end) = propagate_resource!(surface_block(ctx, span, cur.pos())?);
    cur.set_pos(cache_end);
    let cache_fit_tolerance = if matches!(cur.peek(), Some(Token::Double(_))) {
        Some(cur.take_f64()? * LEN_TO_MM)
    } else {
        None
    };
    cur.at_scope_end().then_some(())?;

    Some(Ok(DecodedProceduralSurface::legacy(
        DecodedProceduralSurfaceDefinition::Blend {
            supports: Box::new(supports),
            spine: Some(spine),
            radius_offsets: offsets,
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance,
    )))
}
