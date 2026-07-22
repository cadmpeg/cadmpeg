// SPDX-License-Identifier: Apache-2.0
//! Procedural spline-surface embedded types and their `_spl_sur` decoders.

use crate::nurbs::blend::{
    decode_cyl_spl_sur_at, decode_full_rb_blend_spl_sur, decode_rb_blend_spl_sur_fallback,
    decode_rolling_ball_side, decode_var_blend_spl_sur, decode_vertex_blend_spl_sur,
};
use crate::nurbs::core::{decode_curve_block, decode_surface_block};
use crate::nurbs::pcurve::{decode_pcurve_block_with_end, NurbsPcurve};
use crate::nurbs::proc_curve::{
    decode_embedded_base_curve_resolving_refs, decode_embedded_surface,
    decode_optional_embedded_surface_with_bounds, take_optional_helix_revision,
};
use crate::nurbs::reader::{
    marker_positions, normalized, take_bool, take_f64, take_float_array, take_native_ident,
    take_native_string, take_native_vec3, take_optional_range_value, take_range_value,
    take_tagged_int, INT_WIDTHS, LEN_TO_MM, NUBS_MARKER,
};
use crate::nurbs::subtypes::{
    find_subtype_marker, first_construction_subtype, subtype_refs, subtype_span, SubtypeTables,
};
use cadmpeg_ir::cursor::bounded_len;
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, NurbsCurve, NurbsSurface, SurfaceGeometry,
};
use cadmpeg_ir::le::f64_at as read_f64;
use cadmpeg_ir::math::{Point3, Vector3};

/// A decoded native procedural definition and the fit contract of its solved cache.
pub struct DecodedProceduralSurface {
    /// The native procedural surface construction (blend, sweep, loft, or
    /// taper family) decoded from its subtype-dispatched inline fields.
    pub definition: DecodedProceduralSurfaceDefinition,
    /// `surface_fit_tolerance` of the cached B-spline block, if present.
    /// `0.0` indicates fidelity to the procedural surface rather than
    /// identity with a primitive ([spec §7.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#75-nubsnurbs-blocks-b-spline-curves-and-surfaces)).
    pub cache_fit_tolerance: Option<f64>,
}

/// Source-native procedural semantics before embedded geometry is assigned IR ids.
pub enum DecodedProceduralSurfaceDefinition {
    /// Exact NURBS construction and retained native parameter fields.
    Exact {
        /// Legacy ordered ranges or revision-native scalar values.
        parameters: cadmpeg_ir::geometry::SplineSurfaceParameters,
        /// Native ASM extension integer.
        extension: i64,
        /// Revision-gated form fields.
        revision_form: Option<cadmpeg_ir::geometry::RevisionSurfaceForm>,
    },
    /// Native compound surface with ordered scalar/component pairs.
    Compound {
        /// Ordered native parameters.
        parameters: Vec<f64>,
        /// Ordered embedded component surfaces.
        components: Vec<SurfaceGeometry>,
    },
    /// Exact rectangular restriction of an embedded support surface.
    SubSurface {
        /// Embedded support surface.
        support: SurfaceGeometry,
        /// Ordered U and V parameter intervals.
        parameter_ranges: [[f64; 2]; 2],
    },
    /// Native taper family with shared carriers and subtype tail.
    Taper {
        /// Embedded base surface.
        support: SurfaceGeometry,
        /// Embedded reference curve.
        reference: NurbsCurve,
        /// Embedded UV curve, absent for `nullbs`.
        pcurve: Option<NurbsPcurve>,
        /// Native taper parameter.
        parameter: f64,
        /// Subtype-specific tail.
        taper: cadmpeg_ir::geometry::TaperSurfaceKind,
        /// Revision-gated form fields.
        revision_form: Option<cadmpeg_ir::geometry::RevisionSurfaceForm>,
    },
    /// Native loft construction graph with embedded carriers.
    Loft(EmbeddedLoft),
    /// Native compound-loft graph with embedded carriers.
    CompoundLoft(Box<EmbeddedCompoundLoft>),
    /// Revision-gated compound-loft graph with embedded carriers.
    RevisionCompoundLoft(Box<EmbeddedRevisionCompoundLoft>),
    /// Native scaled compound-loft graph with embedded carriers.
    ScaledCompoundLoft(Box<EmbeddedScaledCompoundLoft>),
    /// Native skinned surface graph with embedded carriers.
    Skin(Box<EmbeddedSkinSurface>),
    /// Native recursive law-surface graph.
    Law(Box<EmbeddedLawSurface>),
    /// Native curve-network surface graph with embedded carriers.
    Net(Box<EmbeddedNetSurface>),
    /// Native sweep surface graph with embedded carriers.
    Sweep(Box<EmbeddedSweepSurface>),
    /// Native T-spline wrapper and subtransform program.
    TSpline(Box<cadmpeg_ir::geometry::TSplineSurfaceConstruction>),
    /// Native circular or linear helix surface.
    Helix(Box<cadmpeg_ir::geometry::HelixSurfaceConstruction>),
    /// Native deformable surface with embedded support.
    Deformable(Box<EmbeddedDeformableSurface>),
    /// Native G2 blend construction with embedded carriers.
    G2Blend(Box<EmbeddedG2Blend>),
    /// Revision-gated G2 blend in the variable-blend side layout.
    RevisionG2Blend(Box<EmbeddedRevisionG2Blend>),
    /// Ruled interpolation between two ordered profile curves.
    Ruled {
        /// First embedded profile.
        first: NurbsCurve,
        /// Second embedded profile.
        second: NurbsCurve,
    },
    /// Translational sum of two curves around a stored origin.
    Sum {
        /// First embedded curve.
        first: CurveGeometry,
        /// Second embedded curve.
        second: CurveGeometry,
        /// Native model-space origin.
        basepoint: Vector3,
        /// Revision-gated form fields.
        revision_form: Option<cadmpeg_ir::geometry::RevisionSurfaceForm>,
    },
    /// Revolution of an embedded profile around an axis.
    Revolution {
        /// Embedded profile curve.
        directrix: CurveGeometry,
        /// Point on the axis in model space.
        axis_origin: Point3,
        /// Unit axis direction.
        axis_direction: Vector3,
        /// Angular interval from the solved surface cache.
        angular_interval: [f64; 2],
        /// Native profile parameter interval.
        parameter_interval: [f64; 2],
        /// Revision-gated form fields.
        revision_form: Option<cadmpeg_ir::geometry::RevisionSurfaceForm>,
    },
    /// Signed offset from an embedded support surface.
    Offset {
        /// Embedded support surface.
        support: SurfaceGeometry,
        /// Signed model-space distance.
        distance: f64,
        /// Native U sense enum.
        u_sense: i64,
        /// Native V sense enum.
        v_sense: i64,
        /// Ordered conditional ASM flags.
        extension_flags: Vec<bool>,
        /// Revision-gated form fields.
        revision_form: Option<cadmpeg_ir::geometry::RevisionSurfaceForm>,
    },
    /// Translation of an embedded directrix along a length-bearing direction.
    Extrusion {
        /// Embedded directrix cache.
        directrix: NurbsCurve,
        /// Stored directrix parameter interval.
        parameter_interval: [f64; 2],
        /// Length-bearing sweep direction.
        direction: Vector3,
        /// Native model-space position following the direction.
        native_position: Point3,
    },
    /// Rolling-ball blend with embedded support and spine caches.
    Blend {
        /// Embedded support caches in side order.
        supports: Box<[Option<SurfaceGeometry>; 2]>,
        /// Embedded center/spine curve.
        spine: Option<NurbsCurve>,
        /// Signed radius law.
        radius: BlendRadiusLaw,
        /// Blend cross-section family.
        cross_section: BlendCrossSection,
        /// Complete native construction graph when the full layout decoded.
        native: Option<Box<EmbeddedRollingBall>>,
    },
    /// Variable-radius blend with a complete embedded construction graph.
    VariableBlend(Box<EmbeddedVariableBlend>),
    /// Vertex-blend patch with complete embedded boundary graphs.
    VertexBlend(Box<EmbeddedVertexBlend>),
}

pub(crate) struct EmbeddedRollingBallSide {
    pub(crate) support_kind: cadmpeg_ir::geometry::VariableBlendSupportKind,
    pub(crate) surface: Option<SurfaceGeometry>,
    pub(crate) surface_ranges: [[Option<f64>; 2]; 2],
    pub(crate) curve: Option<CurveGeometry>,
    pub(crate) curve_range: [Option<f64>; 2],
    pub(crate) pcurve: Option<NurbsPcurve>,
    pub(crate) location: Point3,
    pub(crate) secondary_pcurve: Option<NurbsPcurve>,
    pub(crate) extension: Option<i64>,
    pub(crate) tertiary_pcurve: Option<NurbsPcurve>,
}

/// Embedded revision-gated G2 blend before stable IR ids are assigned.
pub struct EmbeddedRevisionG2Blend {
    pub(crate) revision: i64,
    pub(crate) leading_parameters: [f64; 2],
    pub(crate) sides: Box<[EmbeddedRollingBallSide; 2]>,
    pub(crate) center: CurveGeometry,
    pub(crate) center_range: [Option<f64>; 2],
    pub(crate) radii: [f64; 2],
    pub(crate) radius_selector: i64,
    pub(crate) u_range: [Option<f64>; 2],
    pub(crate) v_range: [Option<f64>; 2],
    pub(crate) shape_prefix: i64,
    pub(crate) shape_parameter: f64,
    pub(crate) shape_length: f64,
    pub(crate) shape_tail: i64,
    pub(crate) tail_enum: i64,
    pub(crate) discontinuities: [Vec<f64>; 6],
    pub(crate) tail_flag: bool,
    pub(crate) tail_extensions: [i64; 3],
}

pub(crate) struct EmbeddedRollingBallThirdSide {
    pub(crate) label: String,
    pub(crate) surface: SurfaceGeometry,
    pub(crate) curve: NurbsCurve,
    pub(crate) pcurve: Option<NurbsPcurve>,
    pub(crate) direction: Vector3,
    pub(crate) secondary_pcurve: Option<NurbsPcurve>,
    pub(crate) extension: i64,
    pub(crate) tertiary_pcurve: Option<NurbsPcurve>,
    pub(crate) flag: bool,
}

/// Embedded native variable blend before stable IR ids are assigned.
pub struct EmbeddedVariableBlend {
    pub(crate) revision: i64,
    pub(crate) sides: Box<[EmbeddedRollingBallSide; 2]>,
    pub(crate) slice: CurveGeometry,
    pub(crate) slice_range: [Option<f64>; 2],
    pub(crate) offsets: [f64; 2],
    pub(crate) radius_kind: cadmpeg_ir::geometry::VariableBlendRadiusKind,
    pub(crate) first_value: cadmpeg_ir::geometry::VariableBlendValue,
    pub(crate) second_value: Option<cadmpeg_ir::geometry::VariableBlendValue>,
    pub(crate) chamfer_selector: Option<i64>,
    pub(crate) chamfer: Option<Box<cadmpeg_ir::geometry::VariableBlendChamfer>>,
    pub(crate) single_radius_selector: Option<i64>,
    pub(crate) single_radius_tail: Option<cadmpeg_ir::geometry::VariableBlendSingleRadiusTail>,
    pub(crate) u_range: [Option<f64>; 2],
    pub(crate) v_range: [Option<f64>; 2],
    pub(crate) shape_prefix: i64,
    pub(crate) shape_parameter: f64,
    pub(crate) shape_length: f64,
    pub(crate) shape_tail: i64,
    pub(crate) cache_selector: i64,
    pub(crate) discontinuities: [Vec<f64>; 6],
    pub(crate) tail_flag: bool,
    pub(crate) tail_extensions: [i64; 3],
    pub(crate) secondary_curve: Option<CurveGeometry>,
    pub(crate) secondary_range: [Option<f64>; 2],
    pub(crate) convexity: cadmpeg_ir::geometry::VariableBlendConvexity,
    pub(crate) render_mode: cadmpeg_ir::geometry::VariableBlendRenderMode,
    pub(crate) post_range: [Option<f64>; 2],
    pub(crate) post_curve: Option<NurbsCurve>,
    pub(crate) post_pcurve: Option<NurbsPcurve>,
}

pub(crate) enum EmbeddedVertexBlendBoundaryGeometry {
    Circle {
        curve: CurveGeometry,
        curve_endpoints: [Option<f64>; 2],
        form: i64,
        twists: Vec<Point3>,
        parameters: [f64; 2],
        sense: i64,
    },
    Degenerate {
        location: Point3,
        normals: [Vector3; 2],
    },
    Pcurve {
        surface: SurfaceGeometry,
        support_bounds: [Option<f64>; 4],
        pcurve: Option<NurbsPcurve>,
        sense: i64,
        fit_tolerance: f64,
    },
    Plane {
        normal: Vector3,
        parameters: [f64; 2],
        curve: CurveGeometry,
        curve_endpoints: [Option<f64>; 2],
    },
}

pub(crate) struct EmbeddedVertexBlendBoundary {
    pub(crate) boundary_type: i64,
    pub(crate) magic: Point3,
    pub(crate) u_smoothing: i64,
    pub(crate) v_smoothing: i64,
    pub(crate) fullness: f64,
    pub(crate) geometry: EmbeddedVertexBlendBoundaryGeometry,
}

/// Embedded native vertex blend before stable IR ids are assigned.
pub struct EmbeddedVertexBlend {
    pub(crate) revision: Option<i64>,
    pub(crate) boundaries: Vec<EmbeddedVertexBlendBoundary>,
    pub(crate) grid_size: i64,
    pub(crate) fit_tolerance: f64,
}

pub(crate) enum EmbeddedRollingBallRadiusSelector {
    None,
    Value(f64),
}

/// Embedded native rolling-ball graph before stable IR ids are assigned.
pub struct EmbeddedRollingBall {
    pub(crate) definition_index: i64,
    pub(crate) sides: Box<[EmbeddedRollingBallSide; 2]>,
    pub(crate) slice: CurveGeometry,
    pub(crate) slice_range: [Option<f64>; 2],
    pub(crate) offsets: [f64; 2],
    pub(crate) radius_selector: EmbeddedRollingBallRadiusSelector,
    pub(crate) u_range: [Option<f64>; 2],
    pub(crate) v_range: [Option<f64>; 2],
    pub(crate) shape_prefix: i64,
    pub(crate) parameters: [f64; 2],
    pub(crate) tail: i64,
    pub(crate) cache_selector: i64,
    pub(crate) discontinuities: [Vec<f64>; 3],
    pub(crate) third: Option<Box<EmbeddedRollingBallThirdSide>>,
}

pub(crate) struct EmbeddedG2Side {
    pub(crate) label: String,
    pub(crate) surface: SurfaceGeometry,
    pub(crate) curve: NurbsCurve,
    pub(crate) pcurves: [Option<NurbsPcurve>; 2],
    pub(crate) direction: Vector3,
}

pub(crate) enum EmbeddedG2FirstShape {
    Full {
        surface: Option<NurbsSurface>,
        tolerance: Option<f64>,
    },
    None {
        coefficients: [f64; 9],
        tolerance: f64,
        extension: Option<cadmpeg_ir::geometry::LoftBridgeToken>,
        pcurve: Option<NurbsPcurve>,
    },
}

/// Embedded native G2 blend graph before stable IR ids are assigned.
pub struct EmbeddedG2Blend {
    pub(crate) first: EmbeddedG2Side,
    pub(crate) singularity: i64,
    pub(crate) first_shape: EmbeddedG2FirstShape,
    pub(crate) second: EmbeddedG2Side,
    pub(crate) second_exact_surface: NurbsSurface,
    pub(crate) center_curve: NurbsCurve,
    pub(crate) center_parameters: [f64; 2],
    pub(crate) center_flag: i64,
    pub(crate) parameter_ranges: [[f64; 2]; 2],
    pub(crate) trailing_parameters: [f64; 4],
    pub(crate) discontinuities: [Vec<f64>; 3],
}

#[allow(clippy::option_option)] // Outer None is parse failure; inner None is native nullbs.
pub(crate) fn decode_nullable_embedded_pcurve(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<Option<NurbsPcurve>> {
    let saved = *position;
    if take_native_ident(bytes, position).as_deref() == Some("nullbs") {
        return Some(None);
    }
    *position = saved;
    let (pcurve, end) = decode_pcurve_block_with_end(bytes, *position, int_width)?;
    *position = end;
    Some(Some(pcurve))
}

fn decode_g2_side(bytes: &[u8], position: &mut usize, int_width: usize) -> Option<EmbeddedG2Side> {
    let label = take_native_string(bytes, position)?;
    let surface = decode_embedded_surface(bytes, position, int_width)?;
    let curve = decode_curve_block(bytes, *position, int_width)?;
    *position = curve.end;
    let first = decode_nullable_embedded_pcurve(bytes, position, int_width)?;
    let direction = take_native_vec3(bytes, position, 0x14)?;
    let second = decode_nullable_embedded_pcurve(bytes, position, int_width)?;
    Some(EmbeddedG2Side {
        label,
        surface,
        curve: curve.curve,
        pcurves: [first, second],
        direction: Vector3::new(direction[0], direction[1], direction[2]),
    })
}

fn take_bridge_token(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<cadmpeg_ir::geometry::LoftBridgeToken> {
    use cadmpeg_ir::geometry::LoftBridgeToken;
    match *bytes.get(*position)? {
        0x0a | 0x0b => Some(LoftBridgeToken::Boolean(take_bool(bytes, position)?)),
        0x04 => Some(LoftBridgeToken::Integer(take_tagged_int(
            bytes, position, 0x04, int_width,
        )?)),
        0x06 => Some(LoftBridgeToken::Double(take_f64(bytes, position)?)),
        0x15 => Some(LoftBridgeToken::Enum(take_tagged_int(
            bytes, position, 0x15, int_width,
        )?)),
        0x07..=0x09 => Some(LoftBridgeToken::Text(take_native_string(bytes, position)?)),
        _ => None,
    }
}

fn decode_g2_blend_spl_sur(
    record_bytes: &[u8],
    int_width: usize,
    resolver: Option<(&[u8], &SubtypeTables)>,
) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 2] = [b"g2_blend_spl_sur", b"g2blnsur"];
    let (start, name_len) =
        find_subtype_marker(record_bytes, &names).map(|(start, name)| (start, name.len()))?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name_len + 3;
    if span.get(position) == Some(&0x04) {
        // Revision-gated layout: revision integer, two scalars, two sides in
        // the variable-blend side layout, center curve with endpoints, two
        // radii, radius selector, optional U/V bounds, shape prologue,
        // shared tail, and three trailing integers.
        (first_construction_subtype(record_bytes).as_deref() == Some("g2_blend_spl_sur"))
            .then_some(())?;
        let revision = take_tagged_int(span, &mut position, 0x04, int_width)?;
        (revision > 0).then_some(())?;
        let leading_parameters = [
            take_f64(span, &mut position)?,
            take_f64(span, &mut position)?,
        ];
        let sides = Box::new([
            decode_rolling_ball_side(span, &mut position, int_width, resolver)?,
            decode_rolling_ball_side(span, &mut position, int_width, resolver)?,
        ]);
        let (active_bytes, tables) = resolver?;
        let center = decode_embedded_base_curve_resolving_refs(
            span,
            &mut position,
            int_width,
            active_bytes,
            tables,
        )?;
        let center_range = [
            take_optional_range_value(span, &mut position)?,
            take_optional_range_value(span, &mut position)?,
        ];
        let radii = [
            take_f64(span, &mut position)? * LEN_TO_MM,
            take_f64(span, &mut position)? * LEN_TO_MM,
        ];
        let radius_selector = take_tagged_int(span, &mut position, 0x15, int_width)?;
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
        let (tail_enum, fit_tolerance, discontinuities, tail_flag) =
            decode_revision_surface_tail(span, &mut position, int_width)?;
        let tail_extensions = [
            take_tagged_int(span, &mut position, 0x04, int_width)?,
            take_tagged_int(span, &mut position, 0x04, int_width)?,
            take_tagged_int(span, &mut position, 0x04, int_width)?,
        ];
        return Some(DecodedProceduralSurface {
            definition: DecodedProceduralSurfaceDefinition::RevisionG2Blend(Box::new(
                EmbeddedRevisionG2Blend {
                    revision,
                    leading_parameters,
                    sides,
                    center: CurveGeometry::Nurbs(center),
                    center_range,
                    radii,
                    radius_selector,
                    u_range,
                    v_range,
                    shape_prefix,
                    shape_parameter,
                    shape_length,
                    shape_tail,
                    tail_enum,
                    discontinuities,
                    tail_flag,
                    tail_extensions,
                },
            )),
            cache_fit_tolerance: Some(fit_tolerance),
        });
    }
    let first = decode_g2_side(span, &mut position, int_width)?;
    let singularity = take_tagged_int(span, &mut position, 0x15, int_width)?;
    let first_shape = if matches!(span.get(position), Some(0x0d | 0x0e)) {
        let saved = position;
        if take_native_ident(span, &mut position).as_deref() == Some("nullbs") {
            EmbeddedG2FirstShape::Full {
                surface: None,
                tolerance: None,
            }
        } else {
            position = saved;
            let surface = decode_surface_block(span, position, int_width)?;
            position = surface.end;
            EmbeddedG2FirstShape::Full {
                surface: Some(surface.surface),
                tolerance: Some(take_f64(span, &mut position)? * LEN_TO_MM),
            }
        }
    } else {
        let mut coefficients = [0.0; 9];
        for coefficient in &mut coefficients {
            *coefficient = take_f64(span, &mut position)?;
        }
        let tolerance = take_f64(span, &mut position)? * LEN_TO_MM;
        let extension = (!matches!(span.get(position), Some(0x07..=0x09 | 0x0d | 0x0e)))
            .then(|| take_bridge_token(span, &mut position, int_width))
            .flatten();
        let pcurve = decode_nullable_embedded_pcurve(span, &mut position, int_width)?;
        EmbeddedG2FirstShape::None {
            coefficients,
            tolerance,
            extension,
            pcurve,
        }
    };
    let second = decode_g2_side(span, &mut position, int_width)?;
    let second_exact = decode_surface_block(span, position, int_width)?;
    position = second_exact.end;
    let center = decode_curve_block(span, position, int_width)?;
    position = center.end;
    let center_parameters = [
        take_f64(span, &mut position)?,
        take_f64(span, &mut position)?,
    ];
    let center_flag = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let parameter_ranges = [
        [
            take_f64(span, &mut position)?,
            take_f64(span, &mut position)?,
        ],
        [
            take_f64(span, &mut position)?,
            take_f64(span, &mut position)?,
        ],
    ];
    let mut trailing_parameters = [0.0; 4];
    for parameter in &mut trailing_parameters {
        *parameter = take_f64(span, &mut position)?;
    }
    let cache = decode_surface_block(span, position, int_width)?;
    let cache_fit_tolerance = (span.get(cache.end) == Some(&0x06))
        .then(|| read_f64(span, cache.end + 1).map(|value| value * LEN_TO_MM))
        .flatten();
    position = cache.end + usize::from(cache_fit_tolerance.is_some()) * 9;
    let discontinuities = [
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
    ];
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::G2Blend(Box::new(EmbeddedG2Blend {
            first,
            singularity,
            first_shape,
            second,
            second_exact_surface: second_exact.surface,
            center_curve: center.curve,
            center_parameters,
            center_flag,
            parameter_ranges,
            trailing_parameters,
            discontinuities,
        })),
        cache_fit_tolerance,
    })
}

pub(crate) struct EmbeddedLoftProfileData {
    pub(crate) surface: Option<SurfaceGeometry>,
    pub(crate) support_bounds: [Option<f64>; 4],
    pub(crate) pcurve: Option<NurbsPcurve>,
    pub(crate) first_flag: bool,
    pub(crate) asm_extension: i64,
    pub(crate) subdata: cadmpeg_ir::geometry::LoftSubdata,
    pub(crate) direction: Option<Vector3>,
}

pub(crate) struct EmbeddedLoftProfileMember {
    pub(crate) type_code: i64,
    pub(crate) curve: NurbsCurve,
    pub(crate) endpoints: Option<[Option<f64>; 2]>,
    pub(crate) data: EmbeddedLoftProfileData,
}

pub(crate) struct EmbeddedLoftPath {
    pub(crate) curve: Option<NurbsCurve>,
    pub(crate) endpoints: Option<[Option<f64>; 2]>,
    pub(crate) auxiliaries: Vec<NurbsCurve>,
    pub(crate) flag: i64,
}

/// Embedded revision-gated compound loft before stable IR ids are assigned.
pub struct EmbeddedRevisionCompoundLoft {
    pub(crate) revision: i64,
    pub(crate) tail_enum: i64,
    pub(crate) discontinuities: [Vec<f64>; 6],
    pub(crate) tail_flag: bool,
    pub(crate) base_profile: Vec<EmbeddedLoftProfileMember>,
    pub(crate) base_path: EmbeddedLoftPath,
    pub(crate) entries: Vec<EmbeddedLoftSectionEntry>,
    pub(crate) flags: [bool; 2],
    pub(crate) kind: i64,
    pub(crate) kind_flags: [bool; 2],
    pub(crate) selector: i64,
    pub(crate) direction: Option<Vector3>,
    pub(crate) direction_curve: Option<NurbsCurve>,
    pub(crate) interval: [Option<f64>; 2],
    pub(crate) trailing_curve: Option<NurbsCurve>,
}

pub(crate) struct EmbeddedLoftSectionEntry {
    pub(crate) parameter: f64,
    pub(crate) profile: Vec<EmbeddedLoftProfileMember>,
    pub(crate) path: EmbeddedLoftPath,
}

/// Embedded native loft graph before its carriers receive stable IR ids.
pub struct EmbeddedLoft {
    pub(crate) sections: [Vec<EmbeddedLoftSectionEntry>; 2],
    pub(crate) revision_form: Option<cadmpeg_ir::geometry::LoftRevisionForm>,
    pub(crate) parameters: cadmpeg_ir::geometry::SplineSurfaceParameters,
    pub(crate) closures: [i64; 2],
    pub(crate) singularities: [i64; 2],
    pub(crate) mode: i64,
    pub(crate) bridge: Vec<cadmpeg_ir::geometry::LoftBridgeToken>,
}

pub(crate) struct EmbeddedCompoundLoftScale {
    pub(crate) members: Vec<EmbeddedLoftProfileMember>,
    pub(crate) path: NurbsCurve,
    pub(crate) auxiliaries: Vec<NurbsCurve>,
    pub(crate) tail: [i64; 2],
}

pub(crate) enum EmbeddedCompoundLoftDirection {
    Vector(Vector3),
    Curve(NurbsCurve),
}

pub(crate) enum EmbeddedCompoundLoftTail {
    Six {
        flags: [bool; 2],
        scale: Box<EmbeddedCompoundLoftScale>,
        selector: i64,
        direction: Vector3,
        parameter_range: [f64; 2],
        curve: NurbsCurve,
    },
    Seven {
        first_flag: bool,
        first_scale: Option<Box<EmbeddedCompoundLoftScale>>,
        second_flag: bool,
        second_scale: Box<EmbeddedCompoundLoftScale>,
        selector: i64,
        direction: Vector3,
        trailing_flags: [bool; 2],
    },
    Zero {
        flags: [bool; 2],
        selector: i64,
        direction: EmbeddedCompoundLoftDirection,
        trailing_flags: [bool; 2],
    },
}

/// Embedded native compound loft before stable IR ids are assigned.
pub struct EmbeddedCompoundLoft {
    pub(crate) scales: Box<[Option<EmbeddedCompoundLoftScale>; 4]>,
    pub(crate) fifth_scale: Option<Box<EmbeddedCompoundLoftScale>>,
    pub(crate) flags: [bool; 2],
    pub(crate) tail: EmbeddedCompoundLoftTail,
}

pub(crate) enum EmbeddedScaledCompoundLoftShape {
    Full,
    None {
        parameter_ranges: [[f64; 2]; 2],
        parameters: [Vec<f64>; 2],
    },
}

pub(crate) enum EmbeddedScaledCompoundLoftBranch {
    ExtendedVector {
        first_scale: Option<Box<EmbeddedCompoundLoftScale>>,
        second_scale: Box<EmbeddedCompoundLoftScale>,
        selector: i64,
        direction: Vector3,
    },
    ExtendedCurve {
        scale: Option<Box<EmbeddedCompoundLoftScale>>,
        flag: bool,
        singularity: i64,
        curve: NurbsCurve,
    },
    Direct {
        flag: bool,
        selector: i64,
        direction: EmbeddedCompoundLoftDirection,
    },
}

/// Embedded native scaled compound loft before stable IR ids are assigned.
pub struct EmbeddedScaledCompoundLoft {
    pub(crate) singularity: i64,
    pub(crate) shape: EmbeddedScaledCompoundLoftShape,
    pub(crate) discontinuities: [Vec<f64>; 6],
    pub(crate) discontinuity_flag: bool,
    pub(crate) scales: Box<[Option<EmbeddedCompoundLoftScale>; 3]>,
    pub(crate) flags: [bool; 2],
    pub(crate) selector: i64,
    pub(crate) branch: EmbeddedScaledCompoundLoftBranch,
    pub(crate) trailing_flags: [bool; 2],
    pub(crate) tail_kind: i64,
    pub(crate) tail_directions: [Vector3; 2],
    pub(crate) tail_singularity: i64,
    pub(crate) tail_curve: NurbsCurve,
}

pub(crate) enum EmbeddedLawExpression {
    Null,
    Integer(i64),
    Double(f64),
    Point(Point3),
    Vector(Vector3),
    Transform {
        scalars: [f64; 13],
        enums: [i64; 3],
    },
    Edge {
        curve: NurbsCurve,
        endpoints: Option<[Option<f64>; 2]>,
        parameters: [f64; 2],
    },
    Spline {
        native_id: i64,
        knots: Vec<f64>,
        controls: Vec<f64>,
        point: Point3,
    },
    Algebraic {
        operator: String,
        operands: Vec<EmbeddedLawExpression>,
    },
}

pub(crate) struct EmbeddedLawFormula {
    pub(crate) name: String,
    pub(crate) variables: Vec<EmbeddedLawExpression>,
}

/// Embedded native law surface before stable IR ids are assigned.
pub struct EmbeddedLawSurface {
    pub(crate) parameter_ranges: Option<[[f64; 2]; 2]>,
    pub(crate) primary: EmbeddedLawFormula,
    pub(crate) additional: Vec<EmbeddedLawFormula>,
    pub(crate) tail: cadmpeg_ir::geometry::LawSurfaceTail,
    pub(crate) discontinuities: [Vec<f64>; 6],
}

pub(crate) enum EmbeddedSkinSurfaceLayout {
    Profiles {
        profiles: Vec<EmbeddedLoftProfileMember>,
        path: NurbsCurve,
        tail: [i64; 2],
    },
    Compact {
        curve: NurbsCurve,
        subdata: cadmpeg_ir::geometry::LoftSubdata,
        first_tail: i64,
        secondary_curve: NurbsCurve,
        second_tail: i64,
    },
}

/// Embedded native skin surface before stable IR ids are assigned.
pub struct EmbeddedSkinSurface {
    pub(crate) surface_boolean: i64,
    pub(crate) surface_normal: i64,
    pub(crate) surface_direction: i64,
    pub(crate) count: i64,
    pub(crate) parameter: f64,
    pub(crate) inner_count: i64,
    pub(crate) layout: EmbeddedSkinSurfaceLayout,
    pub(crate) direction: Vector3,
    pub(crate) trailing_parameter: f64,
    pub(crate) formula: EmbeddedLawFormula,
    pub(crate) parameter_curve: NurbsCurve,
    pub(crate) discontinuities: [Vec<f64>; 6],
    pub(crate) discontinuity_flag: bool,
}

/// Embedded native net surface before stable IR ids are assigned.
pub struct EmbeddedNetSurface {
    pub(crate) sections: Box<[Vec<EmbeddedLoftSectionEntry>; 2]>,
    pub(crate) frame_parameters: [f64; 12],
    pub(crate) flag: i64,
    pub(crate) directions: [Vector3; 4],
    pub(crate) formulas: Box<[EmbeddedLawFormula; 4]>,
    pub(crate) discontinuities: [Vec<f64>; 6],
    pub(crate) discontinuity_flag: bool,
}

pub(crate) enum EmbeddedSweepSurfaceLayout {
    ProfileFirst {
        profile: NurbsCurve,
        spine: NurbsCurve,
        secondary_kind: i64,
        directions: [Vector3; 5],
        origin: Point3,
        parameters: [f64; 4],
        formulas: Box<[EmbeddedLawFormula; 3]>,
    },
    ExplicitFormula {
        profile: NurbsCurve,
        mode: i64,
        profile_range: [f64; 2],
        profile_frame: Option<(Point3, Vector3)>,
        origin: Point3,
        directions: [Vector3; 3],
        trajectory_flag: bool,
        path: NurbsCurve,
        path_range: [f64; 2],
        path_parameter: f64,
        formula_flag: bool,
        formula: EmbeddedLawFormula,
        trailing_flag: bool,
    },
    ExplicitGuide {
        profile: NurbsCurve,
        mode: i64,
        profile_range: [f64; 2],
        profile_frame: Option<(Point3, Vector3)>,
        origin: Point3,
        directions: [Vector3; 3],
        trajectory_flag: bool,
        path: NurbsCurve,
        path_range: [f64; 2],
        path_parameter: f64,
        guide_flags: [bool; 2],
        guide_curve: NurbsCurve,
        guide_range: [f64; 2],
        guide_modes: [i64; 2],
        guide_parameters: [f64; 6],
        trailing_flags: [bool; 3],
    },
    ExplicitSurface {
        profile: NurbsCurve,
        mode: i64,
        profile_range: [f64; 2],
        profile_frame: Option<(Point3, Vector3)>,
        origin: Point3,
        directions: [Vector3; 3],
        trajectory_flag: bool,
        path: NurbsCurve,
        path_range: [f64; 2],
        path_parameter: f64,
        singularity: i64,
        support_surface: SurfaceGeometry,
        auxiliary_curve: Option<NurbsCurve>,
        support_flag: bool,
        legacy_flag: Option<bool>,
    },
    LawDriven {
        profile: NurbsCurve,
        mode: i64,
        profile_range: [f64; 2],
        profile_frame: Option<(Point3, Vector3)>,
        origin: Point3,
        directions: [Vector3; 3],
        first_law: EmbeddedLawExpression,
        first_mode: i64,
        first_range: [f64; 2],
        law_direction: Vector3,
        path_mode: i64,
        path_flag: bool,
        path: NurbsCurve,
        path_range: [f64; 2],
        path_parameter: f64,
        second_law_flag: bool,
        second_law: EmbeddedLawExpression,
        formula_mode: i64,
        formula: EmbeddedLawFormula,
        trailing_flag: bool,
    },
}

/// Embedded native sweep surface before stable IR ids are assigned.
pub struct EmbeddedSweepSurface {
    pub(crate) primary_kind: i64,
    pub(crate) revision_form: Option<cadmpeg_ir::geometry::SweepRevisionForm>,
    pub(crate) layout: EmbeddedSweepSurfaceLayout,
    pub(crate) discontinuities: [Vec<f64>; 6],
    pub(crate) discontinuity_flag: bool,
}

/// Embedded native deformable surface before stable support ids are assigned.
pub struct EmbeddedDeformableSurface {
    pub(crate) support: SurfaceGeometry,
    pub(crate) data: EmbeddedDeformableSurfaceData,
    pub(crate) discontinuities: [Vec<f64>; 6],
    pub(crate) discontinuity_flag: bool,
}

pub(crate) enum EmbeddedDeformableSurfaceData {
    Resolved(cadmpeg_ir::geometry::DeformableSurfaceData),
    SurfaceCurve {
        surface: SurfaceGeometry,
        native_id: i64,
        flag: bool,
        first_parameter: f64,
        selector: i64,
        second_parameter: f64,
        curve: NurbsCurve,
        vectors: [Vector3; 4],
        frame_parameter: f64,
        flags: [bool; 3],
        parameter_triples: Vec<[f64; 3]>,
    },
    Full {
        leading_vectors: [Vector3; 4],
        leading_parameter: f64,
        leading_flags: [bool; 3],
        selector: i64,
        surface: SurfaceGeometry,
        native_id: i64,
        flag: bool,
        first_parameter: f64,
        version_value: Option<i64>,
        second_parameter: f64,
        curve: NurbsCurve,
        frames: Box<[cadmpeg_ir::geometry::DeformableVectorFrame; 2]>,
        trailing_value: i64,
    },
}

#[allow(clippy::option_option)] // Outer None is parse failure; inner None is an absent scale slot.
fn decode_compound_loft_scale(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<Option<EmbeddedCompoundLoftScale>> {
    if matches!(bytes.get(*position), Some(0x0a | 0x0b)) {
        return Some(None);
    }
    let count = usize::try_from(take_tagged_int(bytes, position, 0x04, int_width)?).ok()?;
    if count > 100_000 {
        return None;
    }
    let mut members = Vec::with_capacity(count);
    for _ in 0..count {
        let type_code = take_tagged_int(bytes, position, 0x04, int_width)?;
        let curve = decode_curve_block(bytes, *position, int_width)?;
        *position = curve.end;
        let data = decode_loft_profile_data(bytes, position, int_width)?;
        members.push(EmbeddedLoftProfileMember {
            type_code,
            curve: curve.curve,
            endpoints: None,
            data,
        });
    }
    let path = decode_curve_block(bytes, *position, int_width)?;
    *position = path.end;
    let auxiliary_count =
        usize::try_from(take_tagged_int(bytes, position, 0x04, int_width)?).ok()?;
    if auxiliary_count > 100_000 {
        return None;
    }
    let mut auxiliaries = Vec::with_capacity(auxiliary_count);
    for _ in 0..auxiliary_count {
        let curve = decode_curve_block(bytes, *position, int_width)?;
        *position = curve.end;
        auxiliaries.push(curve.curve);
    }
    let tail = [
        take_tagged_int(bytes, position, 0x04, int_width)?,
        take_tagged_int(bytes, position, 0x04, int_width)?,
    ];
    Some(Some(EmbeddedCompoundLoftScale {
        members,
        path: path.curve,
        auxiliaries,
        tail,
    }))
}

/// Exact rational quadratic NURBS of a full native ellipse.
pub(crate) fn ellipse_to_nurbs(
    center: [f64; 3],
    normal: [f64; 3],
    major: [f64; 3],
    ratio: f64,
) -> Option<NurbsCurve> {
    let length = (major[0] * major[0] + major[1] * major[1] + major[2] * major[2]).sqrt();
    (length.is_finite() && length > 0.0).then_some(())?;
    let minor_direction = [
        normal[1] * major[2] - normal[2] * major[1],
        normal[2] * major[0] - normal[0] * major[2],
        normal[0] * major[1] - normal[1] * major[0],
    ];
    let minor_length = (minor_direction[0] * minor_direction[0]
        + minor_direction[1] * minor_direction[1]
        + minor_direction[2] * minor_direction[2])
        .sqrt();
    (minor_length.is_finite() && minor_length > 0.0).then_some(())?;
    let minor_scale = ratio * length / minor_length;
    let minor = [
        minor_direction[0] * minor_scale,
        minor_direction[1] * minor_scale,
        minor_direction[2] * minor_scale,
    ];
    let at = |mj: f64, mn: f64| {
        Point3::new(
            (center[0] + mj * major[0] + mn * minor[0]) * LEN_TO_MM,
            (center[1] + mj * major[1] + mn * minor[1]) * LEN_TO_MM,
            (center[2] + mj * major[2] + mn * minor[2]) * LEN_TO_MM,
        )
    };
    let w = std::f64::consts::FRAC_1_SQRT_2;
    Some(NurbsCurve {
        degree: 2,
        knots: vec![
            0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
        ],
        control_points: vec![
            at(1.0, 0.0),
            at(1.0, 1.0),
            at(0.0, 1.0),
            at(-1.0, 1.0),
            at(-1.0, 0.0),
            at(-1.0, -1.0),
            at(0.0, -1.0),
            at(1.0, -1.0),
            at(1.0, 0.0),
        ],
        weights: Some(vec![1.0, w, 1.0, w, 1.0, w, 1.0, w, 1.0]),
        periodic: false,
    })
}

/// Revision-gated loft profile data: bounded support, nullable pcurve, flags,
/// and constraint subdata with trailing row pairs.
fn decode_revision_loft_profile_data(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
    active_bytes: &[u8],
    tables: &SubtypeTables,
) -> Option<EmbeddedLoftProfileData> {
    let (surface, support_bounds) = decode_optional_embedded_surface_with_bounds(
        bytes,
        position,
        int_width,
        active_bytes,
        tables,
    )?;
    let pcurve = decode_nullable_embedded_pcurve(bytes, position, int_width)?;
    let first_flag = take_bool(bytes, position)?;
    let asm_extension = take_tagged_int(bytes, position, 0x04, int_width)?;
    let subdata = decode_loft_subdata_form(bytes, position, int_width, true)?;
    let direction = if take_bool(bytes, position)? {
        let value = take_native_vec3(bytes, position, 0x14)?;
        Some(Vector3::new(value[0], value[1], value[2]))
    } else {
        None
    };
    Some(EmbeddedLoftProfileData {
        surface,
        support_bounds,
        pcurve,
        first_flag,
        asm_extension,
        subdata,
        direction,
    })
}

fn decode_revision_loft_section(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
    active_bytes: &[u8],
    tables: &SubtypeTables,
) -> Option<Vec<EmbeddedLoftSectionEntry>> {
    let count = usize::try_from(take_tagged_int(bytes, position, 0x04, int_width)?).ok()?;
    let count = bounded_len(count as u64, 9, bytes.len().saturating_sub(*position))?;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let parameter = take_f64(bytes, position)?;
        let member_count =
            usize::try_from(take_tagged_int(bytes, position, 0x04, int_width)?).ok()?;
        let member_count = bounded_len(
            member_count as u64,
            1 + int_width,
            bytes.len().saturating_sub(*position),
        )?;
        let mut profile = Vec::with_capacity(member_count);
        for _ in 0..member_count {
            let type_code = take_tagged_int(bytes, position, 0x04, int_width)?;
            let curve = decode_embedded_base_curve_resolving_refs(
                bytes,
                position,
                int_width,
                active_bytes,
                tables,
            )?;
            let endpoints = [
                take_optional_range_value(bytes, position)?,
                take_optional_range_value(bytes, position)?,
            ];
            let data = decode_revision_loft_profile_data(
                bytes,
                position,
                int_width,
                active_bytes,
                tables,
            )?;
            profile.push(EmbeddedLoftProfileMember {
                type_code,
                curve,
                endpoints: Some(endpoints),
                data,
            });
        }
        let saved = *position;
        let (path_curve, path_endpoints) =
            if take_native_ident(bytes, position).as_deref() == Some("null_curve") {
                (None, None)
            } else {
                *position = saved;
                let curve = decode_embedded_base_curve_resolving_refs(
                    bytes,
                    position,
                    int_width,
                    active_bytes,
                    tables,
                )?;
                let endpoints = [
                    take_optional_range_value(bytes, position)?,
                    take_optional_range_value(bytes, position)?,
                ];
                (Some(curve), Some(endpoints))
            };
        let auxiliary_count =
            usize::try_from(take_tagged_int(bytes, position, 0x04, int_width)?).ok()?;
        let auxiliary_count = bounded_len(
            auxiliary_count as u64,
            6,
            bytes.len().saturating_sub(*position),
        )?;
        let mut auxiliaries = Vec::with_capacity(auxiliary_count);
        for _ in 0..auxiliary_count {
            let auxiliary = decode_curve_block(bytes, *position, int_width)?;
            *position = auxiliary.end;
            auxiliaries.push(auxiliary.curve);
        }
        let flag = take_tagged_int(bytes, position, 0x04, int_width)?;
        entries.push(EmbeddedLoftSectionEntry {
            parameter,
            profile,
            path: EmbeddedLoftPath {
                curve: path_curve,
                endpoints: path_endpoints,
                auxiliaries,
                flag,
            },
        });
    }
    Some(entries)
}

fn decode_loft_subdata(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<cadmpeg_ir::geometry::LoftSubdata> {
    decode_loft_subdata_form(bytes, position, int_width, false)
}

fn decode_loft_subdata_form(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
    revision: bool,
) -> Option<cadmpeg_ir::geometry::LoftSubdata> {
    use cadmpeg_ir::geometry::{LoftSubdata, LoftSubdataRow};
    let type_code = take_tagged_int(bytes, position, 0x04, int_width)?;
    let row_count = take_tagged_int(bytes, position, 0x04, int_width)?;
    let column_count = take_tagged_int(bytes, position, 0x04, int_width)?;
    let rows_to_read = if type_code == 211 {
        1
    } else {
        usize::try_from(row_count).ok()?
    };
    let columns_to_read = usize::try_from(column_count).ok()?;
    // Each row consumes two tagged doubles (2 * 9 bytes) for its parameters.
    let rows_to_read = bounded_len(
        rows_to_read as u64,
        18,
        bytes.len().saturating_sub(*position),
    )?;
    let mut rows = Vec::with_capacity(rows_to_read);
    for _ in 0..rows_to_read {
        let parameters = [take_f64(bytes, position)?, take_f64(bytes, position)?];
        let mut columns = Vec::new();
        if type_code != 211 {
            columns.reserve(columns_to_read);
            for _ in 0..columns_to_read {
                columns.push([take_f64(bytes, position)?, take_f64(bytes, position)?]);
            }
        }
        let extra = if revision && type_code != 211 {
            Some([take_f64(bytes, position)?, take_f64(bytes, position)?])
        } else {
            None
        };
        rows.push(LoftSubdataRow {
            parameters,
            columns,
            extra,
        });
    }
    Some(LoftSubdata {
        type_code,
        row_count,
        column_count,
        rows,
    })
}

fn decode_loft_profile_data(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<EmbeddedLoftProfileData> {
    let surface = decode_embedded_surface(bytes, position, int_width)?;
    let saved = *position;
    let pcurve = if take_native_ident(bytes, position).as_deref() == Some("nullbs") {
        None
    } else {
        *position = saved;
        let (pcurve, end) = decode_pcurve_block_with_end(bytes, *position, int_width)?;
        *position = end;
        Some(pcurve)
    };
    let first_flag = take_bool(bytes, position)?;
    let asm_extension = take_tagged_int(bytes, position, 0x04, int_width)?;
    let subdata = decode_loft_subdata(bytes, position, int_width)?;
    let direction = if take_bool(bytes, position)? {
        let value = take_native_vec3(bytes, position, 0x14)?;
        Some(Vector3::new(value[0], value[1], value[2]))
    } else {
        None
    };
    Some(EmbeddedLoftProfileData {
        surface: Some(surface),
        support_bounds: [None; 4],
        pcurve,
        first_flag,
        asm_extension,
        subdata,
        direction,
    })
}

fn decode_loft_section(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<Vec<EmbeddedLoftSectionEntry>> {
    let count = usize::try_from(take_tagged_int(bytes, position, 0x04, int_width)?).ok()?;
    // Each entry consumes at least one tagged double (9 bytes) for its parameter.
    let count = bounded_len(count as u64, 9, bytes.len().saturating_sub(*position))?;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let parameter = take_f64(bytes, position)?;
        let member_count =
            usize::try_from(take_tagged_int(bytes, position, 0x04, int_width)?).ok()?;
        // Each member consumes at least a tagged type code (1 + int_width bytes).
        let member_count = bounded_len(
            member_count as u64,
            1 + int_width,
            bytes.len().saturating_sub(*position),
        )?;
        let mut profile = Vec::with_capacity(member_count);
        for _ in 0..member_count {
            let type_code = take_tagged_int(bytes, position, 0x04, int_width)?;
            let curve = decode_curve_block(bytes, *position, int_width)?;
            *position = curve.end;
            let data = decode_loft_profile_data(bytes, position, int_width)?;
            profile.push(EmbeddedLoftProfileMember {
                type_code,
                curve: curve.curve,
                endpoints: None,
                data,
            });
        }
        let curve = decode_curve_block(bytes, *position, int_width)?;
        *position = curve.end;
        let auxiliary_count =
            usize::try_from(take_tagged_int(bytes, position, 0x04, int_width)?).ok()?;
        // Each auxiliary consumes at least a curve-block marker (6 bytes).
        let auxiliary_count = bounded_len(
            auxiliary_count as u64,
            6,
            bytes.len().saturating_sub(*position),
        )?;
        let mut auxiliaries = Vec::with_capacity(auxiliary_count);
        for _ in 0..auxiliary_count {
            let auxiliary = decode_curve_block(bytes, *position, int_width)?;
            *position = auxiliary.end;
            auxiliaries.push(auxiliary.curve);
        }
        let flag = take_tagged_int(bytes, position, 0x04, int_width)?;
        entries.push(EmbeddedLoftSectionEntry {
            parameter,
            profile,
            path: EmbeddedLoftPath {
                curve: Some(curve.curve),
                endpoints: None,
                auxiliaries,
                flag,
            },
        });
    }
    Some(entries)
}

fn decode_revision_loft(
    span: &[u8],
    mut position: usize,
    int_width: usize,
    resolver: Option<(&[u8], &SubtypeTables)>,
) -> Option<DecodedProceduralSurface> {
    let (active_bytes, tables) = resolver?;
    let revision = take_tagged_int(span, &mut position, 0x04, int_width)?;
    (revision > 0).then_some(())?;
    let sections = [
        decode_revision_loft_section(span, &mut position, int_width, active_bytes, tables)?,
        decode_revision_loft_section(span, &mut position, int_width, active_bytes, tables)?,
    ];
    let mut parameter_values = [None; 4];
    for value in &mut parameter_values {
        *value = take_optional_range_value(span, &mut position)?;
    }
    let mut flags = [false; 4];
    for flag in &mut flags {
        *flag = take_bool(span, &mut position)?;
    }
    let ints = [
        take_tagged_int(span, &mut position, 0x04, int_width)?,
        take_tagged_int(span, &mut position, 0x04, int_width)?,
    ];
    let (tail_enum, fit_tolerance, discontinuities, tail_flag) =
        decode_revision_surface_tail(span, &mut position, int_width)?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Loft(EmbeddedLoft {
            sections,
            revision_form: Some(cadmpeg_ir::geometry::LoftRevisionForm {
                revision,
                flags,
                ints,
                tail_enum,
                discontinuities,
                tail_flag,
            }),
            parameters: cadmpeg_ir::geometry::SplineSurfaceParameters::RevisionValues {
                values: parameter_values,
            },
            closures: [0, 0],
            singularities: [0, 0],
            mode: 0,
            bridge: Vec::new(),
        }),
        cache_fit_tolerance: Some(fit_tolerance),
    })
}

fn decode_loft_spl_sur(
    record_bytes: &[u8],
    int_width: usize,
    resolver: Option<(&[u8], &SubtypeTables)>,
) -> Option<DecodedProceduralSurface> {
    use cadmpeg_ir::geometry::LoftBridgeToken;
    let names: [&[u8]; 2] = [b"loft_spl_sur", b"loftsur"];
    let (start, name_len) =
        find_subtype_marker(record_bytes, &names).map(|(start, name)| (start, name.len()))?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name_len + 3;
    if span.get(position) == Some(&0x04)
        && first_construction_subtype(record_bytes).as_deref() == Some("loft_spl_sur")
    {
        if let Some(decoded) = decode_revision_loft(span, position, int_width, resolver) {
            return Some(decoded);
        }
    }
    let sections = [
        decode_loft_section(span, &mut position, int_width)?,
        decode_loft_section(span, &mut position, int_width)?,
    ];
    let parameter_ranges = [
        [
            take_f64(span, &mut position)?,
            take_f64(span, &mut position)?,
        ],
        [
            take_f64(span, &mut position)?,
            take_f64(span, &mut position)?,
        ],
    ];
    let closures = [
        take_tagged_int(span, &mut position, 0x15, int_width)?,
        take_tagged_int(span, &mut position, 0x15, int_width)?,
    ];
    let singularities = [
        take_tagged_int(span, &mut position, 0x15, int_width)?,
        take_tagged_int(span, &mut position, 0x15, int_width)?,
    ];
    let mode = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let (cache_at, cache) = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at, int_width).map(|cache| (at, cache)))
        .next_back()?;
    let mut bridge = Vec::new();
    while position < cache_at {
        match *span.get(position)? {
            0x0a | 0x0b => bridge.push(LoftBridgeToken::Boolean(take_bool(span, &mut position)?)),
            0x04 => bridge.push(LoftBridgeToken::Integer(take_tagged_int(
                span,
                &mut position,
                0x04,
                int_width,
            )?)),
            0x06 => bridge.push(LoftBridgeToken::Double(take_f64(span, &mut position)?)),
            0x15 => bridge.push(LoftBridgeToken::Enum(take_tagged_int(
                span,
                &mut position,
                0x15,
                int_width,
            )?)),
            0x07..=0x09 => {
                bridge.push(LoftBridgeToken::Text(take_native_string(
                    span,
                    &mut position,
                )?));
            }
            _ => return None,
        }
    }
    let cache_fit_tolerance = (span.get(cache.end) == Some(&0x06))
        .then(|| read_f64(span, cache.end + 1).map(|value| value * LEN_TO_MM))
        .flatten();
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Loft(EmbeddedLoft {
            sections,
            revision_form: None,
            parameters: cadmpeg_ir::geometry::SplineSurfaceParameters::OrderedRanges {
                ranges: parameter_ranges,
            },
            closures,
            singularities,
            mode,
            bridge,
        }),
        cache_fit_tolerance,
    })
}

/// One revision-gated compound-loft scale block: counted profile members,
/// nullable path curve with optional endpoints, counted auxiliary BS3
/// curves, and one tail integer.
fn decode_revision_cl_scale(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
    active_bytes: &[u8],
    tables: &SubtypeTables,
) -> Option<(Vec<EmbeddedLoftProfileMember>, EmbeddedLoftPath)> {
    let member_count = usize::try_from(take_tagged_int(bytes, position, 0x04, int_width)?).ok()?;
    let member_count = bounded_len(
        member_count as u64,
        1 + int_width,
        bytes.len().saturating_sub(*position),
    )?;
    let mut profile = Vec::with_capacity(member_count);
    for _ in 0..member_count {
        let type_code = take_tagged_int(bytes, position, 0x04, int_width)?;
        let curve = decode_embedded_base_curve_resolving_refs(
            bytes,
            position,
            int_width,
            active_bytes,
            tables,
        )?;
        let endpoints = [
            take_optional_range_value(bytes, position)?,
            take_optional_range_value(bytes, position)?,
        ];
        let data =
            decode_revision_loft_profile_data(bytes, position, int_width, active_bytes, tables)?;
        profile.push(EmbeddedLoftProfileMember {
            type_code,
            curve,
            endpoints: Some(endpoints),
            data,
        });
    }
    let saved = *position;
    let (path_curve, path_endpoints) =
        if take_native_ident(bytes, position).as_deref() == Some("null_curve") {
            (None, None)
        } else {
            *position = saved;
            let curve = decode_embedded_base_curve_resolving_refs(
                bytes,
                position,
                int_width,
                active_bytes,
                tables,
            )?;
            let endpoints = [
                take_optional_range_value(bytes, position)?,
                take_optional_range_value(bytes, position)?,
            ];
            (Some(curve), Some(endpoints))
        };
    let auxiliary_count =
        usize::try_from(take_tagged_int(bytes, position, 0x04, int_width)?).ok()?;
    let auxiliary_count = bounded_len(
        auxiliary_count as u64,
        6,
        bytes.len().saturating_sub(*position),
    )?;
    let mut auxiliaries = Vec::with_capacity(auxiliary_count);
    for _ in 0..auxiliary_count {
        let auxiliary = decode_curve_block(bytes, *position, int_width)?;
        *position = auxiliary.end;
        auxiliaries.push(auxiliary.curve);
    }
    let flag = take_tagged_int(bytes, position, 0x04, int_width)?;
    Some((
        profile,
        EmbeddedLoftPath {
            curve: path_curve,
            endpoints: path_endpoints,
            auxiliaries,
            flag,
        },
    ))
}

fn decode_revision_compound_loft(
    span: &[u8],
    int_width: usize,
    resolver: Option<(&[u8], &SubtypeTables)>,
) -> Option<DecodedProceduralSurface> {
    let (active_bytes, tables) = resolver?;
    let name = b"cl_loft_spl_sur";
    let mut position = name.len() + 3;
    let revision = take_tagged_int(span, &mut position, 0x04, int_width)?;
    (revision > 0).then_some(())?;
    let (tail_enum, fit_tolerance, discontinuities, tail_flag) =
        decode_revision_surface_tail(span, &mut position, int_width)?;
    let (base_profile, base_path) =
        decode_revision_cl_scale(span, &mut position, int_width, active_bytes, tables)?;
    let entry_count =
        usize::try_from(take_tagged_int(span, &mut position, 0x04, int_width)?).ok()?;
    let entry_count = bounded_len(
        entry_count as u64,
        1 + int_width,
        span.len().saturating_sub(position),
    )?;
    let mut entries = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        let (profile, path) =
            decode_revision_cl_scale(span, &mut position, int_width, active_bytes, tables)?;
        let parameter = take_f64(span, &mut position)?;
        entries.push(EmbeddedLoftSectionEntry {
            parameter,
            profile,
            path,
        });
    }
    let flags = [
        take_bool(span, &mut position)?,
        take_bool(span, &mut position)?,
    ];
    let kind = take_tagged_int(span, &mut position, 0x04, int_width)?;
    // Only the kind-zero payload is defined for the revision layout.
    (kind == 0).then_some(())?;
    let kind_flags = [
        take_bool(span, &mut position)?,
        take_bool(span, &mut position)?,
    ];
    let selector = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let (direction, direction_curve) = if selector == 0 {
        let value = take_native_vec3(span, &mut position, 0x14)?;
        (Some(Vector3::new(value[0], value[1], value[2])), None)
    } else {
        let curve = decode_curve_block(span, position, int_width)?;
        position = curve.end;
        (None, Some(curve.curve))
    };
    let interval = [
        take_optional_range_value(span, &mut position)?,
        take_optional_range_value(span, &mut position)?,
    ];
    let trailing_curve = if span.get(position) == Some(&0x10) {
        None
    } else {
        let curve = decode_curve_block(span, position, int_width)?;
        position = curve.end;
        Some(curve.curve)
    };
    (span.get(position) == Some(&0x10)).then_some(())?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::RevisionCompoundLoft(Box::new(
            EmbeddedRevisionCompoundLoft {
                revision,
                tail_enum,
                discontinuities,
                tail_flag,
                base_profile,
                base_path,
                entries,
                flags,
                kind,
                kind_flags,
                selector,
                direction,
                direction_curve,
                interval,
                trailing_curve,
            },
        )),
        cache_fit_tolerance: Some(fit_tolerance),
    })
}

fn decode_compound_loft_spl_sur(
    record_bytes: &[u8],
    int_width: usize,
    resolver: Option<(&[u8], &SubtypeTables)>,
) -> Option<DecodedProceduralSurface> {
    let name: &[u8] = b"cl_loft_spl_sur";
    let (start, _) = find_subtype_marker(record_bytes, &[name])?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name.len() + 3;
    if span.get(position) == Some(&0x04) {
        (first_construction_subtype(record_bytes).as_deref() == Some("cl_loft_spl_sur"))
            .then_some(())?;
        return decode_revision_compound_loft(span, int_width, resolver);
    }
    let cache = decode_surface_block(span, position, int_width)?;
    position = cache.end;
    let cache_fit_tolerance = Some(take_f64(span, &mut position)? * LEN_TO_MM);
    let scales = Box::new([
        decode_compound_loft_scale(span, &mut position, int_width)?,
        decode_compound_loft_scale(span, &mut position, int_width)?,
        decode_compound_loft_scale(span, &mut position, int_width)?,
        decode_compound_loft_scale(span, &mut position, int_width)?,
    ]);
    let fifth_scale = if span.get(position) == Some(&0x04) {
        decode_compound_loft_scale(span, &mut position, int_width)?.map(Box::new)
    } else {
        None
    };
    let flags = [
        take_bool(span, &mut position)?,
        take_bool(span, &mut position)?,
    ];
    let kind = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let tail = match kind {
        6 => {
            let tail_flags = [
                take_bool(span, &mut position)?,
                take_bool(span, &mut position)?,
            ];
            let scale = Box::new(decode_compound_loft_scale(span, &mut position, int_width)??);
            let selector = take_tagged_int(span, &mut position, 0x04, int_width)?;
            let direction = take_native_vec3(span, &mut position, 0x14)?;
            let parameter_range = [
                take_range_value(span, &mut position)?,
                take_range_value(span, &mut position)?,
            ];
            let curve = decode_curve_block(span, position, int_width)?;
            EmbeddedCompoundLoftTail::Six {
                flags: tail_flags,
                scale,
                selector,
                direction: Vector3::new(direction[0], direction[1], direction[2]),
                parameter_range,
                curve: curve.curve,
            }
        }
        7 => {
            let first_flag = take_bool(span, &mut position)?;
            let first_scale =
                decode_compound_loft_scale(span, &mut position, int_width)?.map(Box::new);
            let second_flag = take_bool(span, &mut position)?;
            let second_scale =
                Box::new(decode_compound_loft_scale(span, &mut position, int_width)??);
            let selector = take_tagged_int(span, &mut position, 0x04, int_width)?;
            let direction = take_native_vec3(span, &mut position, 0x14)?;
            let trailing_flags = [
                take_bool(span, &mut position)?,
                take_bool(span, &mut position)?,
            ];
            EmbeddedCompoundLoftTail::Seven {
                first_flag,
                first_scale,
                second_flag,
                second_scale,
                selector,
                direction: Vector3::new(direction[0], direction[1], direction[2]),
                trailing_flags,
            }
        }
        0 => {
            let tail_flags = [
                take_bool(span, &mut position)?,
                take_bool(span, &mut position)?,
            ];
            let selector = take_tagged_int(span, &mut position, 0x04, int_width)?;
            let direction = if selector == 0 {
                let value = take_native_vec3(span, &mut position, 0x14)?;
                EmbeddedCompoundLoftDirection::Vector(Vector3::new(value[0], value[1], value[2]))
            } else {
                let curve = decode_curve_block(span, position, int_width)?;
                position = curve.end;
                EmbeddedCompoundLoftDirection::Curve(curve.curve)
            };
            let trailing_flags = [
                take_bool(span, &mut position)?,
                take_bool(span, &mut position)?,
            ];
            EmbeddedCompoundLoftTail::Zero {
                flags: tail_flags,
                selector,
                direction,
                trailing_flags,
            }
        }
        _ => return None,
    };
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::CompoundLoft(Box::new(
            EmbeddedCompoundLoft {
                scales,
                fifth_scale,
                flags,
                tail,
            },
        )),
        cache_fit_tolerance,
    })
}

fn decode_scaled_compound_loft_spl_sur(
    record_bytes: &[u8],
    int_width: usize,
) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 2] = [b"scaled_cloft_spl_sur", b"sclclftsur"];
    let (start, name) = find_subtype_marker(record_bytes, &names)?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name.len() + 3;
    let singularity = take_tagged_int(span, &mut position, 0x15, int_width)?;
    let (shape, cache_fit_tolerance) = if matches!(span.get(position), Some(0x0d | 0x0e)) {
        let cache = decode_surface_block(span, position, int_width)?;
        position = cache.end;
        let tolerance = take_f64(span, &mut position)? * LEN_TO_MM;
        (EmbeddedScaledCompoundLoftShape::Full, Some(tolerance))
    } else {
        let parameter_ranges = [
            [
                take_range_value(span, &mut position)?,
                take_range_value(span, &mut position)?,
            ],
            [
                take_range_value(span, &mut position)?,
                take_range_value(span, &mut position)?,
            ],
        ];
        let parameters = [
            take_float_array(span, &mut position, int_width)?,
            take_float_array(span, &mut position, int_width)?,
        ];
        (
            EmbeddedScaledCompoundLoftShape::None {
                parameter_ranges,
                parameters,
            },
            None,
        )
    };
    let discontinuities = [
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
    ];
    let discontinuity_flag = take_bool(span, &mut position)?;
    let scales = Box::new([
        decode_compound_loft_scale(span, &mut position, int_width)?,
        decode_compound_loft_scale(span, &mut position, int_width)?,
        decode_compound_loft_scale(span, &mut position, int_width)?,
    ]);
    let flags = [
        take_bool(span, &mut position)?,
        take_bool(span, &mut position)?,
    ];
    let selector = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let extended = take_bool(span, &mut position)?;
    let branch = if extended {
        let first_scale = decode_compound_loft_scale(span, &mut position, int_width)?.map(Box::new);
        if take_bool(span, &mut position)? {
            let second_scale =
                Box::new(decode_compound_loft_scale(span, &mut position, int_width)??);
            let selector = take_tagged_int(span, &mut position, 0x04, int_width)?;
            let direction = take_native_vec3(span, &mut position, 0x14)?;
            EmbeddedScaledCompoundLoftBranch::ExtendedVector {
                first_scale,
                second_scale,
                selector,
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            }
        } else {
            let flag = take_bool(span, &mut position)?;
            let singularity = take_tagged_int(span, &mut position, 0x15, int_width)?;
            let curve = decode_curve_block(span, position, int_width)?;
            position = curve.end;
            EmbeddedScaledCompoundLoftBranch::ExtendedCurve {
                scale: first_scale,
                flag,
                singularity,
                curve: curve.curve,
            }
        }
    } else {
        let flag = take_bool(span, &mut position)?;
        let selector = take_tagged_int(span, &mut position, 0x04, int_width)?;
        let direction = if selector == 0 {
            let direction = take_native_vec3(span, &mut position, 0x14)?;
            EmbeddedCompoundLoftDirection::Vector(Vector3::new(
                direction[0],
                direction[1],
                direction[2],
            ))
        } else {
            let curve = decode_curve_block(span, position, int_width)?;
            position = curve.end;
            EmbeddedCompoundLoftDirection::Curve(curve.curve)
        };
        EmbeddedScaledCompoundLoftBranch::Direct {
            flag,
            selector,
            direction,
        }
    };
    let trailing_flags = [
        take_bool(span, &mut position)?,
        take_bool(span, &mut position)?,
    ];
    let tail_kind = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let first = take_native_vec3(span, &mut position, 0x14)?;
    let second = take_native_vec3(span, &mut position, 0x14)?;
    let tail_singularity = take_tagged_int(span, &mut position, 0x15, int_width)?;
    let tail_curve = decode_curve_block(span, position, int_width)?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::ScaledCompoundLoft(Box::new(
            EmbeddedScaledCompoundLoft {
                singularity,
                shape,
                discontinuities,
                discontinuity_flag,
                scales,
                flags,
                selector,
                branch,
                trailing_flags,
                tail_kind,
                tail_directions: [
                    Vector3::new(first[0], first[1], first[2]),
                    Vector3::new(second[0], second[1], second[2]),
                ],
                tail_singularity,
                tail_curve: tail_curve.curve,
            },
        )),
        cache_fit_tolerance,
    })
}

pub(crate) fn decode_law_expression(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
    depth: usize,
) -> Option<EmbeddedLawExpression> {
    decode_law_expression_resolving(bytes, position, int_width, depth, None)
}

fn decode_law_expression_resolving(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
    depth: usize,
    resolver: Option<(&[u8], &SubtypeTables)>,
) -> Option<EmbeddedLawExpression> {
    if depth > 64 {
        return None;
    }
    match *bytes.get(*position)? {
        0x04 => {
            return Some(EmbeddedLawExpression::Integer(take_tagged_int(
                bytes, position, 0x04, int_width,
            )?));
        }
        0x06 => return Some(EmbeddedLawExpression::Double(take_f64(bytes, position)?)),
        0x13 => {
            let value = take_native_vec3(bytes, position, 0x13)?;
            return Some(EmbeddedLawExpression::Point(Point3::new(
                value[0] * LEN_TO_MM,
                value[1] * LEN_TO_MM,
                value[2] * LEN_TO_MM,
            )));
        }
        0x14 => {
            let value = take_native_vec3(bytes, position, 0x14)?;
            return Some(EmbeddedLawExpression::Vector(Vector3::new(
                value[0], value[1], value[2],
            )));
        }
        _ => {}
    }
    let operator = take_native_string(bytes, position)?;
    match operator.as_str() {
        "null_law" => Some(EmbeddedLawExpression::Null),
        "TRANS" => {
            let mut scalars = [0.0; 13];
            for scalar in &mut scalars {
                *scalar = take_f64(bytes, position)?;
            }
            let enums = [
                take_tagged_int(bytes, position, 0x15, int_width)?,
                take_tagged_int(bytes, position, 0x15, int_width)?,
                take_tagged_int(bytes, position, 0x15, int_width)?,
            ];
            Some(EmbeddedLawExpression::Transform { scalars, enums })
        }
        "EDGE" => {
            let (curve, endpoints) =
                if let Some(block) = decode_curve_block(bytes, *position, int_width) {
                    *position = block.end;
                    let endpoints = matches!(bytes.get(*position), Some(0x0a | 0x0b))
                        .then(|| {
                            Some([
                                take_optional_range_value(bytes, position)?,
                                take_optional_range_value(bytes, position)?,
                            ])
                        })
                        .flatten();
                    (block.curve, endpoints)
                } else {
                    let (active_bytes, tables) = resolver?;
                    let curve = decode_embedded_base_curve_resolving_refs(
                        bytes,
                        position,
                        int_width,
                        active_bytes,
                        tables,
                    )?;
                    let endpoints = Some([
                        take_optional_range_value(bytes, position)?,
                        take_optional_range_value(bytes, position)?,
                    ]);
                    (curve, endpoints)
                };
            let parameters = [take_f64(bytes, position)?, take_f64(bytes, position)?];
            Some(EmbeddedLawExpression::Edge {
                curve,
                endpoints,
                parameters,
            })
        }
        "SPLINE_LAW" => {
            let native_id = take_tagged_int(bytes, position, 0x04, int_width)?;
            let knots = take_float_array(bytes, position, int_width)?;
            let controls = take_float_array(bytes, position, int_width)?;
            let point = take_native_vec3(bytes, position, 0x13)?;
            Some(EmbeddedLawExpression::Spline {
                native_id,
                knots,
                controls,
                point: Point3::new(
                    point[0] * LEN_TO_MM,
                    point[1] * LEN_TO_MM,
                    point[2] * LEN_TO_MM,
                ),
            })
        }
        _ => {
            let arity = match operator.as_str() {
                "COS" | "SIN" | "TAN" | "COT" | "SEC" | "CSC" | "COSH" | "SINH" | "TANH"
                | "COTH" | "SECH" | "CSCH" | "ARCCOS" | "ARCSIN" | "ARCTAN" | "ARCOT"
                | "ARCSEC" | "ARCCSC" | "ARCCOSH" | "ARCSINH" | "ARCTANH" | "ARCOTH"
                | "ARCSECH" | "ARCCSCH" | "ABS" | "EXP" | "LN" | "LOG" | "SIGN" | "SIZE"
                | "SET" | "SQRT" | "NORM" | "NOT" => 1,
                "CROSS" | "DOT" | "DCUR" | "ROTATE" | "TERM" => 2,
                "VEC" | "DSURF" => 3,
                _ => return None,
            };
            let operands = (0..arity)
                .map(|_| {
                    decode_law_expression_resolving(bytes, position, int_width, depth + 1, resolver)
                })
                .collect::<Option<Vec<_>>>()?;
            Some(EmbeddedLawExpression::Algebraic { operator, operands })
        }
    }
}

pub(crate) fn decode_law_formula(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<EmbeddedLawFormula> {
    decode_law_formula_resolving(bytes, position, int_width, None)
}

fn decode_law_formula_resolving(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
    resolver: Option<(&[u8], &SubtypeTables)>,
) -> Option<EmbeddedLawFormula> {
    let name = take_native_string(bytes, position)?;
    if name == "null_law" {
        return Some(EmbeddedLawFormula {
            name,
            variables: Vec::new(),
        });
    }
    let count = usize::try_from(take_tagged_int(bytes, position, 0x04, int_width)?).ok()?;
    if count > 100_000 {
        return None;
    }
    let variables = (0..count)
        .map(|_| decode_law_expression_resolving(bytes, position, int_width, 0, resolver))
        .collect::<Option<Vec<_>>>()?;
    Some(EmbeddedLawFormula { name, variables })
}

fn decode_skin_spl_sur(record_bytes: &[u8], int_width: usize) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 2] = [b"skin_spl_sur", b"skinsur"];
    let (start, name) = find_subtype_marker(record_bytes, &names)?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name.len() + 3;
    let surface_boolean = take_tagged_int(span, &mut position, 0x15, int_width)?;
    let surface_normal = take_tagged_int(span, &mut position, 0x15, int_width)?;
    let surface_direction = take_tagged_int(span, &mut position, 0x15, int_width)?;
    let count = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let parameter = take_f64(span, &mut position)?;
    let inner_count = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let layout = if matches!(span.get(position), Some(0x0d | 0x0e)) {
        let curve = decode_curve_block(span, position, int_width)?;
        position = curve.end;
        let subdata = decode_loft_subdata(span, &mut position, int_width)?;
        let first_tail = take_tagged_int(span, &mut position, 0x04, int_width)?;
        let secondary_curve = decode_curve_block(span, position, int_width)?;
        position = secondary_curve.end;
        let second_tail = take_tagged_int(span, &mut position, 0x04, int_width)?;
        EmbeddedSkinSurfaceLayout::Compact {
            curve: curve.curve,
            subdata,
            first_tail,
            secondary_curve: secondary_curve.curve,
            second_tail,
        }
    } else {
        let profile_count = usize::try_from(inner_count).ok()?;
        if profile_count > 100_000 {
            return None;
        }
        let mut profiles = Vec::with_capacity(profile_count);
        for _ in 0..profile_count {
            let type_code = take_tagged_int(span, &mut position, 0x04, int_width)?;
            let curve = decode_curve_block(span, position, int_width)?;
            position = curve.end;
            let data = decode_loft_profile_data(span, &mut position, int_width)?;
            profiles.push(EmbeddedLoftProfileMember {
                type_code,
                curve: curve.curve,
                endpoints: None,
                data,
            });
        }
        let path = decode_curve_block(span, position, int_width)?;
        position = path.end;
        let tail = [
            take_tagged_int(span, &mut position, 0x04, int_width)?,
            take_tagged_int(span, &mut position, 0x04, int_width)?,
        ];
        EmbeddedSkinSurfaceLayout::Profiles {
            profiles,
            path: path.curve,
            tail,
        }
    };
    let direction = take_native_vec3(span, &mut position, 0x14)?;
    let trailing_parameter = take_f64(span, &mut position)?;
    let formula = decode_law_formula(span, &mut position, int_width)?;
    let parameter_curve = decode_curve_block(span, position, int_width)?;
    position = parameter_curve.end;
    let cache = decode_surface_block(span, position, int_width)?;
    position = cache.end;
    let cache_fit_tolerance = Some(take_f64(span, &mut position)? * LEN_TO_MM);
    let discontinuities = [
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
    ];
    let discontinuity_flag = take_bool(span, &mut position)?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Skin(Box::new(EmbeddedSkinSurface {
            surface_boolean,
            surface_normal,
            surface_direction,
            count,
            parameter,
            inner_count,
            layout,
            direction: Vector3::new(direction[0], direction[1], direction[2]),
            trailing_parameter,
            formula,
            parameter_curve: parameter_curve.curve,
            discontinuities,
            discontinuity_flag,
        })),
        cache_fit_tolerance,
    })
}

pub(crate) fn decode_law_spl_sur(
    record_bytes: &[u8],
    int_width: usize,
) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 2] = [b"law_spl_sur", b"lawsur"];
    let (start, name) = find_subtype_marker(record_bytes, &names)?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name.len() + 3;
    let parameter_ranges = if span.get(position) == Some(&0x06) {
        Some([
            [
                take_f64(span, &mut position)?,
                take_f64(span, &mut position)?,
            ],
            [
                take_f64(span, &mut position)?,
                take_f64(span, &mut position)?,
            ],
        ])
    } else {
        None
    };
    let primary = decode_law_formula(span, &mut position, int_width)?;
    let count = usize::try_from(take_tagged_int(span, &mut position, 0x04, int_width)?).ok()?;
    if count > 100_000 {
        return None;
    }
    let additional = (0..count)
        .map(|_| decode_law_formula(span, &mut position, int_width))
        .collect::<Option<Vec<_>>>()?;
    let selector = if parameter_ranges.is_some() && span.get(position..)?.starts_with(NUBS_MARKER) {
        0
    } else {
        take_tagged_int(span, &mut position, 0x15, int_width)?
    };
    let (tail, cache_fit_tolerance) = match selector {
        0 => {
            let cache = decode_surface_block(span, position, int_width)?;
            position = cache.end;
            (
                cadmpeg_ir::geometry::LawSurfaceTail::Full,
                Some(take_f64(span, &mut position)? * LEN_TO_MM),
            )
        }
        1 => {
            let parameters = [
                take_float_array(span, &mut position, int_width)?,
                take_float_array(span, &mut position, int_width)?,
            ];
            let fit_tolerance = take_f64(span, &mut position)? * LEN_TO_MM;
            let closures = [
                take_tagged_int(span, &mut position, 0x15, int_width)?,
                take_tagged_int(span, &mut position, 0x15, int_width)?,
            ];
            let singularities = [
                take_tagged_int(span, &mut position, 0x15, int_width)?,
                take_tagged_int(span, &mut position, 0x15, int_width)?,
            ];
            (
                cadmpeg_ir::geometry::LawSurfaceTail::Summary {
                    parameters,
                    fit_tolerance,
                    closures,
                    singularities,
                },
                None,
            )
        }
        2 => {
            let parameter_ranges = [
                [
                    take_f64(span, &mut position)?,
                    take_f64(span, &mut position)?,
                ],
                [
                    take_f64(span, &mut position)?,
                    take_f64(span, &mut position)?,
                ],
            ];
            let closures = [
                take_tagged_int(span, &mut position, 0x15, int_width)?,
                take_tagged_int(span, &mut position, 0x15, int_width)?,
            ];
            let singularities = [
                take_tagged_int(span, &mut position, 0x15, int_width)?,
                take_tagged_int(span, &mut position, 0x15, int_width)?,
            ];
            (
                cadmpeg_ir::geometry::LawSurfaceTail::None {
                    parameter_ranges,
                    closures,
                    singularities,
                },
                None,
            )
        }
        3 => (cadmpeg_ir::geometry::LawSurfaceTail::Historical, None),
        4 => (cadmpeg_ir::geometry::LawSurfaceTail::Optimal, None),
        _ => return None,
    };
    let discontinuities = [
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
    ];
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Law(Box::new(EmbeddedLawSurface {
            parameter_ranges,
            primary,
            additional,
            tail,
            discontinuities,
        })),
        cache_fit_tolerance,
    })
}

pub(crate) fn decode_sub_spl_sur(
    record_bytes: &[u8],
    int_width: usize,
) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 2] = [b"sub_spl_sur", b"subsur"];
    let (start, name) = find_subtype_marker(record_bytes, &names)?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name.len() + 3;
    let parameter_ranges = [
        [
            take_f64(span, &mut position)?,
            take_f64(span, &mut position)?,
        ],
        [
            take_f64(span, &mut position)?,
            take_f64(span, &mut position)?,
        ],
    ];
    let support = decode_embedded_surface(span, &mut position, int_width)?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::SubSurface {
            support,
            parameter_ranges,
        },
        cache_fit_tolerance: None,
    })
}

fn decode_net_spl_sur(record_bytes: &[u8], int_width: usize) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 2] = [b"net_spl_sur", b"netsur"];
    let (start, name) = find_subtype_marker(record_bytes, &names)?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name.len() + 3;
    let sections = Box::new([
        decode_loft_section(span, &mut position, int_width)?,
        decode_loft_section(span, &mut position, int_width)?,
    ]);
    let mut frame_parameters = [0.0; 12];
    for parameter in &mut frame_parameters {
        *parameter = take_f64(span, &mut position)?;
    }
    let flag = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let mut directions = [Vector3::new(0.0, 0.0, 0.0); 4];
    for direction in &mut directions {
        let value = take_native_vec3(span, &mut position, 0x14)?;
        *direction = Vector3::new(value[0], value[1], value[2]);
    }
    let formulas = (0..4)
        .map(|_| decode_law_formula(span, &mut position, int_width))
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    let cache = decode_surface_block(span, position, int_width)?;
    position = cache.end;
    let cache_fit_tolerance = Some(take_f64(span, &mut position)? * LEN_TO_MM);
    let discontinuities = [
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
    ];
    let discontinuity_flag = take_bool(span, &mut position)?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Net(Box::new(EmbeddedNetSurface {
            sections,
            frame_parameters,
            flag,
            directions,
            formulas: Box::new(formulas),
            discontinuities,
            discontinuity_flag,
        })),
        cache_fit_tolerance,
    })
}

fn decode_sweep_spl_sur(
    record_bytes: &[u8],
    int_width: usize,
    resolver: Option<(&[u8], &SubtypeTables)>,
) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 3] = [b"sweep_spl_sur", b"sweep_sur", b"sweepsur"];
    let (start, name_len) =
        find_subtype_marker(record_bytes, &names).map(|(start, name)| (start, name.len()))?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name_len + 3;
    if span.get(position) == Some(&0x04) {
        (first_construction_subtype(record_bytes).as_deref() == Some("sweep_sur")).then_some(())?;
        return decode_revision_sweep_sur(span, position, int_width, resolver?);
    }
    let primary_kind = take_tagged_int(span, &mut position, 0x15, int_width)?;
    let layout = if matches!(span.get(position), Some(0x0d | 0x0e)) {
        let profile = decode_curve_block(span, position, int_width)?;
        position = profile.end;
        let spine = decode_curve_block(span, position, int_width)?;
        position = spine.end;
        let secondary_kind = take_tagged_int(span, &mut position, 0x15, int_width)?;
        let mut directions = [Vector3::new(0.0, 0.0, 0.0); 5];
        for direction in &mut directions {
            let value = take_native_vec3(span, &mut position, 0x14)?;
            *direction = Vector3::new(value[0], value[1], value[2]);
        }
        let origin = take_native_vec3(span, &mut position, 0x13)?;
        let mut parameters = [0.0; 4];
        for parameter in &mut parameters {
            *parameter = take_f64(span, &mut position)?;
        }
        let formulas = (0..3)
            .map(|_| decode_law_formula(span, &mut position, int_width))
            .collect::<Option<Vec<_>>>()?
            .try_into()
            .ok()?;
        EmbeddedSweepSurfaceLayout::ProfileFirst {
            profile: profile.curve,
            spine: spine.curve,
            secondary_kind,
            directions,
            origin: Point3::new(
                origin[0] * LEN_TO_MM,
                origin[1] * LEN_TO_MM,
                origin[2] * LEN_TO_MM,
            ),
            parameters,
            formulas: Box::new(formulas),
        }
    } else {
        let mode = take_tagged_int(span, &mut position, 0x04, int_width)?;
        let profile = decode_curve_block(span, position, int_width)?;
        position = profile.end;
        let profile_range = [
            take_f64(span, &mut position)?,
            take_f64(span, &mut position)?,
        ];
        let profile_frame = if take_bool(span, &mut position)? {
            let point = take_native_vec3(span, &mut position, 0x13)?;
            let vector = take_native_vec3(span, &mut position, 0x14)?;
            Some((
                Point3::new(
                    point[0] * LEN_TO_MM,
                    point[1] * LEN_TO_MM,
                    point[2] * LEN_TO_MM,
                ),
                Vector3::new(vector[0], vector[1], vector[2]),
            ))
        } else {
            None
        };
        let point = take_native_vec3(span, &mut position, 0x13)?;
        let origin = Point3::new(
            point[0] * LEN_TO_MM,
            point[1] * LEN_TO_MM,
            point[2] * LEN_TO_MM,
        );
        let mut directions = [Vector3::new(0.0, 0.0, 0.0); 3];
        for direction in &mut directions {
            let value = take_native_vec3(span, &mut position, 0x14)?;
            *direction = Vector3::new(value[0], value[1], value[2]);
        }
        if span.get(position) == Some(&0x04) {
            let branch = take_tagged_int(span, &mut position, 0x04, int_width)?;
            let trajectory_flag = take_bool(span, &mut position)?;
            let path = decode_curve_block(span, position, int_width)?;
            position = path.end;
            let path_range = [
                take_f64(span, &mut position)? * LEN_TO_MM,
                take_f64(span, &mut position)? * LEN_TO_MM,
            ];
            let path_parameter = take_f64(span, &mut position)?;
            match branch {
                1 => {
                    let formula_flag = take_bool(span, &mut position)?;
                    let formula = decode_law_formula(span, &mut position, int_width)?;
                    let trailing_flag = take_bool(span, &mut position)?;
                    EmbeddedSweepSurfaceLayout::ExplicitFormula {
                        profile: profile.curve,
                        mode,
                        profile_range,
                        profile_frame,
                        origin,
                        directions,
                        trajectory_flag,
                        path: path.curve,
                        path_range,
                        path_parameter,
                        formula_flag,
                        formula,
                        trailing_flag,
                    }
                }
                2 => {
                    let guide_flags = [
                        take_bool(span, &mut position)?,
                        take_bool(span, &mut position)?,
                    ];
                    let guide_curve = decode_curve_block(span, position, int_width)?;
                    position = guide_curve.end;
                    let guide_range = [
                        take_f64(span, &mut position)?,
                        take_f64(span, &mut position)?,
                    ];
                    let guide_modes = [
                        take_tagged_int(span, &mut position, 0x04, int_width)?,
                        take_tagged_int(span, &mut position, 0x04, int_width)?,
                    ];
                    let mut guide_parameters = [0.0; 6];
                    for parameter in &mut guide_parameters {
                        *parameter = take_f64(span, &mut position)?;
                    }
                    let trailing_flags = [
                        take_bool(span, &mut position)?,
                        take_bool(span, &mut position)?,
                        take_bool(span, &mut position)?,
                    ];
                    EmbeddedSweepSurfaceLayout::ExplicitGuide {
                        profile: profile.curve,
                        mode,
                        profile_range,
                        profile_frame,
                        origin,
                        directions,
                        trajectory_flag,
                        path: path.curve,
                        path_range,
                        path_parameter,
                        guide_flags,
                        guide_curve: guide_curve.curve,
                        guide_range,
                        guide_modes,
                        guide_parameters,
                        trailing_flags,
                    }
                }
                3 => {
                    let singularity = take_tagged_int(span, &mut position, 0x15, int_width)?;
                    let support_surface = decode_embedded_surface(span, &mut position, int_width)?;
                    let auxiliary_curve = if take_bool(span, &mut position)? {
                        let curve = decode_curve_block(span, position, int_width)?;
                        position = curve.end;
                        Some(curve.curve)
                    } else {
                        None
                    };
                    let support_flag = take_bool(span, &mut position)?;
                    let legacy_flag = matches!(span.get(position), Some(0x0a | 0x0b))
                        .then(|| take_bool(span, &mut position))
                        .flatten();
                    EmbeddedSweepSurfaceLayout::ExplicitSurface {
                        profile: profile.curve,
                        mode,
                        profile_range,
                        profile_frame,
                        origin,
                        directions,
                        trajectory_flag,
                        path: path.curve,
                        path_range,
                        path_parameter,
                        singularity,
                        support_surface,
                        auxiliary_curve,
                        support_flag,
                        legacy_flag,
                    }
                }
                _ => return None,
            }
        } else {
            let first_law = decode_law_expression(span, &mut position, int_width, 0)?;
            let first_mode = take_tagged_int(span, &mut position, 0x04, int_width)?;
            let first_range = [
                take_f64(span, &mut position)?,
                take_f64(span, &mut position)?,
            ];
            let vector = take_native_vec3(span, &mut position, 0x14)?;
            let law_direction = Vector3::new(vector[0], vector[1], vector[2]);
            let path_mode = take_tagged_int(span, &mut position, 0x04, int_width)?;
            let path_flag = take_bool(span, &mut position)?;
            let path = decode_curve_block(span, position, int_width)?;
            position = path.end;
            let path_range = [
                take_f64(span, &mut position)?,
                take_f64(span, &mut position)?,
            ];
            let path_parameter = take_f64(span, &mut position)?;
            let second_law_flag = take_bool(span, &mut position)?;
            let second_law = decode_law_expression(span, &mut position, int_width, 0)?;
            let formula_mode = take_tagged_int(span, &mut position, 0x04, int_width)?;
            let formula = decode_law_formula(span, &mut position, int_width)?;
            let trailing_flag = take_bool(span, &mut position)?;
            EmbeddedSweepSurfaceLayout::LawDriven {
                profile: profile.curve,
                mode,
                profile_range,
                profile_frame,
                origin,
                directions,
                first_law,
                first_mode,
                first_range,
                law_direction,
                path_mode,
                path_flag,
                path: path.curve,
                path_range,
                path_parameter,
                second_law_flag,
                second_law,
                formula_mode,
                formula,
                trailing_flag,
            }
        }
    };
    let cache = decode_surface_block(span, position, int_width)?;
    position = cache.end;
    let cache_fit_tolerance = Some(take_f64(span, &mut position)? * LEN_TO_MM);
    let discontinuities = [
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
    ];
    let discontinuity_flag = take_bool(span, &mut position)?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Sweep(Box::new(EmbeddedSweepSurface {
            primary_kind,
            revision_form: None,
            layout,
            discontinuities,
            discontinuity_flag,
        })),
        cache_fit_tolerance,
    })
}

/// Revision-gated `sweep_sur` explicit-formula layout.
fn decode_revision_sweep_sur(
    span: &[u8],
    mut position: usize,
    int_width: usize,
    (active_bytes, tables): (&[u8], &SubtypeTables),
) -> Option<DecodedProceduralSurface> {
    let revision = take_tagged_int(span, &mut position, 0x04, int_width)?;
    (revision > 0).then_some(())?;
    let primary_flag = take_bool(span, &mut position)?;
    let mode = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let profile = decode_embedded_base_curve_resolving_refs(
        span,
        &mut position,
        int_width,
        active_bytes,
        tables,
    )?;
    let profile_endpoints = [
        take_optional_range_value(span, &mut position)?,
        take_optional_range_value(span, &mut position)?,
    ];
    let profile_range = [
        take_optional_range_value(span, &mut position)??,
        take_optional_range_value(span, &mut position)??,
    ];
    let profile_frame = if take_bool(span, &mut position)? {
        let point = take_native_vec3(span, &mut position, 0x13)?;
        let vector = take_native_vec3(span, &mut position, 0x14)?;
        Some((
            Point3::new(
                point[0] * LEN_TO_MM,
                point[1] * LEN_TO_MM,
                point[2] * LEN_TO_MM,
            ),
            Vector3::new(vector[0], vector[1], vector[2]),
        ))
    } else {
        None
    };
    let point = take_native_vec3(span, &mut position, 0x13)?;
    let origin = Point3::new(
        point[0] * LEN_TO_MM,
        point[1] * LEN_TO_MM,
        point[2] * LEN_TO_MM,
    );
    let mut directions = [Vector3::new(0.0, 0.0, 0.0); 3];
    for direction in &mut directions {
        let value = take_native_vec3(span, &mut position, 0x14)?;
        *direction = Vector3::new(value[0], value[1], value[2]);
    }
    (take_tagged_int(span, &mut position, 0x04, int_width)? == 1).then_some(())?;
    let trajectory_flag = take_bool(span, &mut position)?;
    let path = decode_embedded_base_curve_resolving_refs(
        span,
        &mut position,
        int_width,
        active_bytes,
        tables,
    )?;
    let path_endpoints = [
        take_optional_range_value(span, &mut position)?,
        take_optional_range_value(span, &mut position)?,
    ];
    let path_range = [
        take_optional_range_value(span, &mut position)?? * LEN_TO_MM,
        take_optional_range_value(span, &mut position)?? * LEN_TO_MM,
    ];
    let path_parameter = take_f64(span, &mut position)?;
    let formula_flag = take_bool(span, &mut position)?;
    let formula =
        decode_law_formula_resolving(span, &mut position, int_width, Some((active_bytes, tables)))?;
    let trailing_flag = take_bool(span, &mut position)?;
    let tail_enum = take_tagged_int(span, &mut position, 0x15, int_width)?;
    let cache = decode_surface_block(span, position, int_width)?;
    position = cache.end;
    let cache_fit_tolerance = Some(take_f64(span, &mut position)? * LEN_TO_MM);
    let discontinuities = [
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
    ];
    let discontinuity_flag = take_bool(span, &mut position)?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Sweep(Box::new(EmbeddedSweepSurface {
            primary_kind: 0,
            revision_form: Some(cadmpeg_ir::geometry::SweepRevisionForm {
                revision,
                primary_flag,
                profile_endpoints,
                path_endpoints,
                tail_enum,
            }),
            layout: EmbeddedSweepSurfaceLayout::ExplicitFormula {
                profile,
                mode,
                profile_range,
                profile_frame,
                origin,
                directions,
                trajectory_flag,
                path,
                path_range,
                path_parameter,
                formula_flag,
                formula,
                trailing_flag,
            },
            discontinuities,
            discontinuity_flag,
        })),
        cache_fit_tolerance,
    })
}

fn decode_taper_spl_sur(
    record_bytes: &[u8],
    int_width: usize,
    resolver: Option<(&[u8], &SubtypeTables)>,
) -> Option<DecodedProceduralSurface> {
    use cadmpeg_ir::geometry::TaperSurfaceKind;
    let names: &[(&[u8], u8)] = &[
        (b"taper_spl_sur", 0),
        (b"ortho_spl_sur", 1),
        (b"orthosur", 1),
        (b"edge_tpr_spl_sur", 2),
        (b"shadow_tpr_spl_sur", 3),
        (b"shadowtapersur", 3),
        (b"ruled_tpr_spl_sur", 4),
        (b"ruledtapersur", 4),
        (b"swept_tpr_spl_sur", 5),
        (b"swepttapersur", 5),
    ];
    let (start, name_len, kind) = names.iter().find_map(|(name, kind)| {
        record_bytes
            .windows(name.len() + 3)
            .position(|window| {
                window[0] == 0x0f
                    && matches!(window[1], 0x0d | 0x0e)
                    && usize::from(window[2]) == name.len()
                    && &window[3..] == *name
            })
            .map(|start| (start, name.len(), *kind))
    })?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name_len + 3;
    if span.get(position) == Some(&0x04) {
        // Revision-gated form, stored by the orthogonal subtype.
        (kind == 1).then_some(())?;
        (first_construction_subtype(record_bytes).as_deref() == Some("ortho_spl_sur"))
            .then_some(())?;
        let (active_bytes, tables) = resolver?;
        let revision = take_tagged_int(span, &mut position, 0x04, int_width)?;
        (revision > 0).then_some(())?;
        let (support, support_bounds) = decode_optional_embedded_surface_with_bounds(
            span,
            &mut position,
            int_width,
            active_bytes,
            tables,
        )?;
        let support = support?;
        let reference = decode_embedded_base_curve_resolving_refs(
            span,
            &mut position,
            int_width,
            active_bytes,
            tables,
        )?;
        let reference_endpoints = [
            take_optional_range_value(span, &mut position)?,
            take_optional_range_value(span, &mut position)?,
        ];
        let pcurve = decode_nullable_embedded_pcurve(span, &mut position, int_width)?;
        let parameter = take_f64(span, &mut position)?;
        let (tail_enum, fit_tolerance, discontinuities, tail_flag) =
            decode_revision_surface_tail(span, &mut position, int_width)?;
        let trailing_flags = vec![take_bool(span, &mut position)?];
        return Some(DecodedProceduralSurface {
            definition: DecodedProceduralSurfaceDefinition::Taper {
                support,
                reference,
                pcurve,
                parameter,
                taper: TaperSurfaceKind::Orthogonal { sense: false },
                revision_form: Some(cadmpeg_ir::geometry::RevisionSurfaceForm {
                    revision,
                    support_bounds,
                    reference_endpoints,
                    second_endpoints: [None; 2],
                    flags: Vec::new(),
                    tail_enum,
                    discontinuities,
                    tail_flag,
                    trailing_flags,
                }),
            },
            cache_fit_tolerance: Some(fit_tolerance),
        });
    }
    let support = decode_embedded_surface(span, &mut position, int_width)?;
    let reference = decode_curve_block(span, position, int_width)?;
    position = reference.end;
    let saved = position;
    let pcurve = if take_native_ident(span, &mut position).as_deref() == Some("nullbs") {
        None
    } else {
        position = saved;
        let (pcurve, end) = decode_pcurve_block_with_end(span, position, int_width)?;
        position = end;
        Some(pcurve)
    };
    let parameter = take_f64(span, &mut position)?;
    let cache = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at, int_width))
        .next_back()?;
    let cache_fit_tolerance = (span.get(cache.end) == Some(&0x06))
        .then(|| read_f64(span, cache.end + 1).map(|value| value * LEN_TO_MM))
        .flatten();
    position = cache.end + usize::from(cache_fit_tolerance.is_some()) * 9;
    let take_draft = |position: &mut usize| {
        let draft = take_native_vec3(span, position, 0x14)?;
        Some(Vector3::new(draft[0], draft[1], draft[2]))
    };
    let taper = match kind {
        0 => TaperSurfaceKind::Standard,
        1 => TaperSurfaceKind::Orthogonal {
            sense: take_bool(span, &mut position)?,
        },
        2 => TaperSurfaceKind::Edge {
            draft: take_draft(&mut position)?,
        },
        3 => TaperSurfaceKind::Shadow {
            draft: take_draft(&mut position)?,
            sine: take_f64(span, &mut position)?,
            cosine: take_f64(span, &mut position)?,
        },
        4 => TaperSurfaceKind::Ruled {
            draft: take_draft(&mut position)?,
            sine: take_f64(span, &mut position)?,
            cosine: take_f64(span, &mut position)?,
            factor: take_f64(span, &mut position)?,
        },
        5 => TaperSurfaceKind::Swept {
            draft: take_draft(&mut position)?,
            sine: take_f64(span, &mut position)?,
            cosine: take_f64(span, &mut position)?,
        },
        _ => return None,
    };
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Taper {
            support,
            reference: reference.curve,
            pcurve,
            parameter,
            taper,
            revision_form: None,
        },
        cache_fit_tolerance,
    })
}

fn decode_comp_spl_sur(record_bytes: &[u8], int_width: usize) -> Option<DecodedProceduralSurface> {
    let (start, _) = find_subtype_marker(record_bytes, &[b"comp_spl_sur".as_slice()])?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let cache = marker_positions(span)
        .into_iter()
        .find_map(|at| decode_surface_block(span, at, int_width))?;
    let cache_fit_tolerance = (span.get(cache.end) == Some(&0x06))
        .then(|| read_f64(span, cache.end + 1).map(|value| value * LEN_TO_MM))
        .flatten();
    let mut position = cache.end + usize::from(cache_fit_tolerance.is_some()) * 9;
    let parameters = take_float_array(span, &mut position, int_width)?;
    let mut components = Vec::with_capacity(parameters.len());
    for _ in 0..parameters.len() {
        components.push(decode_embedded_surface(span, &mut position, int_width)?);
    }
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Compound {
            parameters,
            components,
        },
        cache_fit_tolerance,
    })
}

/// Parse the shared revision-gated surface tail: enum, solved cache, fit
/// tolerance, six discontinuity arrays, and one boolean.
fn decode_revision_surface_tail(
    span: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<(i64, f64, [Vec<f64>; 6], bool)> {
    let tail_enum = take_tagged_int(span, position, 0x15, int_width)?;
    let cache = decode_surface_block(span, *position, int_width)?;
    *position = cache.end;
    let fit_tolerance = take_f64(span, position)? * LEN_TO_MM;
    let discontinuities = [
        take_float_array(span, position, int_width)?,
        take_float_array(span, position, int_width)?,
        take_float_array(span, position, int_width)?,
        take_float_array(span, position, int_width)?,
        take_float_array(span, position, int_width)?,
        take_float_array(span, position, int_width)?,
    ];
    let tail_flag = take_bool(span, position)?;
    Some((tail_enum, fit_tolerance, discontinuities, tail_flag))
}

fn decode_off_spl_sur(
    record_bytes: &[u8],
    int_width: usize,
    resolver: Option<(&[u8], &SubtypeTables)>,
) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 2] = [b"off_spl_sur", b"offsur"];
    let (start, name) = find_subtype_marker(record_bytes, &names)?;
    let name_len = name.len();
    let modern = name == b"off_spl_sur";
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name_len + 3;
    if span.get(position) == Some(&0x04) {
        (first_construction_subtype(record_bytes).as_deref() == Some("off_spl_sur"))
            .then_some(())?;
        let (active_bytes, tables) = resolver?;
        let revision = take_tagged_int(span, &mut position, 0x04, int_width)?;
        (revision > 0).then_some(())?;
        let (support, support_bounds) = decode_optional_embedded_surface_with_bounds(
            span,
            &mut position,
            int_width,
            active_bytes,
            tables,
        )?;
        let support = support?;
        let distance = take_f64(span, &mut position)? * LEN_TO_MM;
        let mut flags = Vec::with_capacity(4);
        for _ in 0..4 {
            flags.push(take_bool(span, &mut position)?);
        }
        let (tail_enum, fit_tolerance, discontinuities, tail_flag) =
            decode_revision_surface_tail(span, &mut position, int_width)?;
        return Some(DecodedProceduralSurface {
            definition: DecodedProceduralSurfaceDefinition::Offset {
                support,
                distance,
                u_sense: 0,
                v_sense: 0,
                extension_flags: Vec::new(),
                revision_form: Some(cadmpeg_ir::geometry::RevisionSurfaceForm {
                    revision,
                    support_bounds,
                    reference_endpoints: [None; 2],
                    second_endpoints: [None; 2],
                    flags,
                    tail_enum,
                    discontinuities,
                    tail_flag,
                    trailing_flags: Vec::new(),
                }),
            },
            cache_fit_tolerance: Some(fit_tolerance),
        });
    }
    let support = decode_embedded_surface(span, &mut position, int_width)?;
    let distance = take_f64(span, &mut position)? * LEN_TO_MM;
    let u_sense = take_tagged_int(span, &mut position, 0x15, int_width)?;
    let v_sense = take_tagged_int(span, &mut position, 0x15, int_width)?;
    let mut extension_flags = Vec::new();
    if modern {
        let first = take_bool(span, &mut position)?;
        extension_flags.push(first);
        if first {
            extension_flags.push(take_bool(span, &mut position)?);
            if matches!(span.get(position), Some(0x0a | 0x0b)) {
                extension_flags.push(take_bool(span, &mut position)?);
            }
        }
    }
    let cache = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at, int_width))
        .next_back()?;
    let cache_fit_tolerance = (span.get(cache.end) == Some(&0x06))
        .then(|| read_f64(span, cache.end + 1).map(|value| value * LEN_TO_MM))
        .flatten();
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Offset {
            support,
            distance,
            u_sense,
            v_sense,
            extension_flags,
            revision_form: None,
        },
        cache_fit_tolerance,
    })
}

fn decode_rot_spl_sur(
    record_bytes: &[u8],
    int_width: usize,
    resolver: Option<(&[u8], &SubtypeTables)>,
) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 2] = [b"rot_spl_sur", b"rotsur"];
    let (start, name_len) =
        find_subtype_marker(record_bytes, &names).map(|(start, name)| (start, name.len()))?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name_len + 3;
    if span.get(position) == Some(&0x04) {
        // Revision-gated layout: revision integer, profile curve with two
        // optional endpoints, axis origin and direction, shared tail.
        (first_construction_subtype(record_bytes).as_deref() == Some("rot_spl_sur"))
            .then_some(())?;
        let revision = take_tagged_int(span, &mut position, 0x04, int_width)?;
        (revision > 0).then_some(())?;
        let (active_bytes, tables) = resolver?;
        let profile = decode_embedded_base_curve_resolving_refs(
            span,
            &mut position,
            int_width,
            active_bytes,
            tables,
        )?;
        let profile_endpoints = [
            take_optional_range_value(span, &mut position)?,
            take_optional_range_value(span, &mut position)?,
        ];
        let origin = take_native_vec3(span, &mut position, 0x13)?;
        let axis = take_native_vec3(span, &mut position, 0x14)?;
        let (tail_enum, fit_tolerance, discontinuities, tail_flag) =
            decode_revision_surface_tail(span, &mut position, int_width)?;
        let cache = marker_positions(span)
            .into_iter()
            .filter_map(|at| decode_surface_block(span, at, int_width))
            .next_back()?;
        let angular_interval = [
            *cache.surface.v_knots.first()?,
            *cache.surface.v_knots.last()?,
        ];
        let parameter_interval = [
            profile_endpoints[0].unwrap_or(*profile.knots.first()?),
            profile_endpoints[1].unwrap_or(*profile.knots.last()?),
        ];
        return Some(DecodedProceduralSurface {
            definition: DecodedProceduralSurfaceDefinition::Revolution {
                directrix: CurveGeometry::Nurbs(profile),
                axis_origin: Point3::new(
                    origin[0] * LEN_TO_MM,
                    origin[1] * LEN_TO_MM,
                    origin[2] * LEN_TO_MM,
                ),
                axis_direction: normalized(axis)?,
                angular_interval,
                parameter_interval,
                revision_form: Some(cadmpeg_ir::geometry::RevisionSurfaceForm {
                    revision,
                    support_bounds: [None; 4],
                    reference_endpoints: profile_endpoints,
                    second_endpoints: [None; 2],
                    flags: Vec::new(),
                    tail_enum,
                    discontinuities,
                    tail_flag,
                    trailing_flags: Vec::new(),
                }),
            },
            cache_fit_tolerance: Some(fit_tolerance),
        });
    }
    let directrix = marker_positions(span)
        .into_iter()
        .find_map(|at| decode_curve_block(span, at, int_width))?;
    let parameter_interval = [
        *directrix.curve.knots.first()?,
        *directrix.curve.knots.last()?,
    ];
    let mut position = directrix.end;
    let origin = take_native_vec3(span, &mut position, 0x13)?;
    let axis_origin = Point3::new(
        origin[0] * LEN_TO_MM,
        origin[1] * LEN_TO_MM,
        origin[2] * LEN_TO_MM,
    );
    let axis = take_native_vec3(span, &mut position, 0x14)?;
    let axis_direction = normalized(axis)?;
    let cache = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at, int_width))
        .next_back()?;
    let angular_interval = [
        *cache.surface.v_knots.first()?,
        *cache.surface.v_knots.last()?,
    ];
    let cache_fit_tolerance = (span.get(cache.end) == Some(&0x06))
        .then(|| read_f64(span, cache.end + 1).map(|value| value * LEN_TO_MM))
        .flatten();
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Revolution {
            directrix: CurveGeometry::Nurbs(directrix.curve),
            axis_origin,
            axis_direction,
            angular_interval,
            parameter_interval,
            revision_form: None,
        },
        cache_fit_tolerance,
    })
}

fn decode_sum_spl_sur(
    record_bytes: &[u8],
    int_width: usize,
    resolver: Option<(&[u8], &SubtypeTables)>,
) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 2] = [b"sum_spl_sur", b"sumsur"];
    let (start, name_len) =
        find_subtype_marker(record_bytes, &names).map(|(start, name)| (start, name.len()))?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name_len + 3;
    if span.get(position) == Some(&0x04) {
        // Revision-gated layout: revision integer, two curves each with two
        // optional endpoints, model-space origin, shared tail.
        (first_construction_subtype(record_bytes).as_deref() == Some("sum_spl_sur"))
            .then_some(())?;
        let revision = take_tagged_int(span, &mut position, 0x04, int_width)?;
        (revision > 0).then_some(())?;
        let (active_bytes, tables) = resolver?;
        let first = decode_embedded_base_curve_resolving_refs(
            span,
            &mut position,
            int_width,
            active_bytes,
            tables,
        )?;
        let first_endpoints = [
            take_optional_range_value(span, &mut position)?,
            take_optional_range_value(span, &mut position)?,
        ];
        let second = decode_embedded_base_curve_resolving_refs(
            span,
            &mut position,
            int_width,
            active_bytes,
            tables,
        )?;
        let second_endpoints = [
            take_optional_range_value(span, &mut position)?,
            take_optional_range_value(span, &mut position)?,
        ];
        let origin = take_native_vec3(span, &mut position, 0x13)?;
        let (tail_enum, fit_tolerance, discontinuities, tail_flag) =
            decode_revision_surface_tail(span, &mut position, int_width)?;
        return Some(DecodedProceduralSurface {
            definition: DecodedProceduralSurfaceDefinition::Sum {
                first: CurveGeometry::Nurbs(first),
                second: CurveGeometry::Nurbs(second),
                basepoint: Vector3::new(
                    origin[0] * LEN_TO_MM,
                    origin[1] * LEN_TO_MM,
                    origin[2] * LEN_TO_MM,
                ),
                revision_form: Some(cadmpeg_ir::geometry::RevisionSurfaceForm {
                    revision,
                    support_bounds: [None; 4],
                    reference_endpoints: first_endpoints,
                    second_endpoints,
                    flags: Vec::new(),
                    tail_enum,
                    discontinuities,
                    tail_flag,
                    trailing_flags: Vec::new(),
                }),
            },
            cache_fit_tolerance: Some(fit_tolerance),
        });
    }
    let mut decoded_curves = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_curve_block(span, at, int_width));
    let first = decoded_curves.next()?;
    let second = decoded_curves.next()?;
    let mut position = second.end;
    let origin = take_native_vec3(span, &mut position, 0x13)?;
    let basepoint = Vector3::new(
        origin[0] * LEN_TO_MM,
        origin[1] * LEN_TO_MM,
        origin[2] * LEN_TO_MM,
    );
    let cache = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at, int_width))
        .next_back();
    let cache_fit_tolerance = cache.and_then(|cache| {
        (span.get(cache.end) == Some(&0x06))
            .then(|| read_f64(span, cache.end + 1).map(|value| value * LEN_TO_MM))
            .flatten()
    });
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Sum {
            first: CurveGeometry::Nurbs(first.curve),
            second: CurveGeometry::Nurbs(second.curve),
            basepoint,
            revision_form: None,
        },
        cache_fit_tolerance,
    })
}

fn decode_ruled_spl_sur(record_bytes: &[u8], int_width: usize) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 2] = [b"rule_sur", b"rulesur"];
    let (start, _) = find_subtype_marker(record_bytes, &names)?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut curves = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_curve_block(span, at, int_width).map(|decoded| decoded.curve));
    let first = curves.next()?;
    let second = curves.next()?;
    let cache = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at, int_width))
        .next_back();
    let cache_fit_tolerance = cache.and_then(|cache| {
        (span.get(cache.end) == Some(&0x06))
            .then(|| read_f64(span, cache.end + 1).map(|value| value * LEN_TO_MM))
            .flatten()
    });
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Ruled { first, second },
        cache_fit_tolerance,
    })
}

fn decode_exact_spl_sur(record_bytes: &[u8], int_width: usize) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 2] = [b"exact_spl_sur", b"exactsur"];
    let (start, name) = find_subtype_marker(record_bytes, &names)?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name.len() + 3;
    if span.get(position) == Some(&0x04) {
        // Revision-gated layout: revision integer, shared tail, four optional
        // parameter values, and the extension as an enum.
        (first_construction_subtype(record_bytes).as_deref() == Some("exact_spl_sur"))
            .then_some(())?;
        let revision = take_tagged_int(span, &mut position, 0x04, int_width)?;
        (revision > 0).then_some(())?;
        let (tail_enum, fit_tolerance, discontinuities, tail_flag) =
            decode_revision_surface_tail(span, &mut position, int_width)?;
        let mut bounds = [None; 4];
        for bound in &mut bounds {
            *bound = take_optional_range_value(span, &mut position)?;
        }
        let extension = take_tagged_int(span, &mut position, 0x15, int_width)?;
        return Some(DecodedProceduralSurface {
            definition: DecodedProceduralSurfaceDefinition::Exact {
                parameters: cadmpeg_ir::geometry::SplineSurfaceParameters::RevisionValues {
                    values: bounds,
                },
                extension,
                revision_form: Some(cadmpeg_ir::geometry::RevisionSurfaceForm {
                    revision,
                    support_bounds: [None; 4],
                    reference_endpoints: [None; 2],
                    second_endpoints: [None; 2],
                    flags: Vec::new(),
                    tail_enum,
                    discontinuities,
                    tail_flag,
                    trailing_flags: Vec::new(),
                }),
            },
            cache_fit_tolerance: Some(fit_tolerance),
        });
    }
    let cache = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at, int_width))
        .next_back()?;
    let cache_fit_tolerance = (span.get(cache.end) == Some(&0x06))
        .then(|| read_f64(span, cache.end + 1).map(|value| value * LEN_TO_MM))
        .flatten();
    let mut position = cache.end + usize::from(cache_fit_tolerance.is_some()) * 9;
    let parameter_ranges = [
        [
            take_range_value(span, &mut position)?,
            take_range_value(span, &mut position)?,
        ],
        [
            take_range_value(span, &mut position)?,
            take_range_value(span, &mut position)?,
        ],
    ];
    let extension = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let _ = name;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Exact {
            parameters: cadmpeg_ir::geometry::SplineSurfaceParameters::OrderedRanges {
                ranges: parameter_ranges,
            },
            extension,
            revision_form: None,
        },
        cache_fit_tolerance,
    })
}

fn decode_t_spl_sur(record_bytes: &[u8], int_width: usize) -> Option<DecodedProceduralSurface> {
    use cadmpeg_ir::geometry::{TSplineSubtransform, TSplineSurfaceConstruction};

    let name: &[u8] = b"t_spl_sur";
    let (start, _) = find_subtype_marker(record_bytes, &[name])?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name.len() + 3;
    let (
        cache_fit_tolerance,
        discontinuities,
        discontinuity_flag,
        parameter_ranges,
        type_code,
        revision_form,
    );
    if span.get(position) == Some(&0x04) {
        // Revision-gated layout: revision integer, shared tail, four optional
        // parameter values, the type code as an enum, then the nested
        // subtransform scope and trailing integer.
        (first_construction_subtype(record_bytes).as_deref() == Some("t_spl_sur")).then_some(())?;
        let revision = take_tagged_int(span, &mut position, 0x04, int_width)?;
        (revision > 0).then_some(())?;
        let (tail_enum, fit_tolerance, tail_discontinuities, tail_flag) =
            decode_revision_surface_tail(span, &mut position, int_width)?;
        let mut bounds = [None; 4];
        for bound in &mut bounds {
            *bound = take_optional_range_value(span, &mut position)?;
        }
        cache_fit_tolerance = Some(fit_tolerance);
        discontinuities = tail_discontinuities.clone();
        discontinuity_flag = tail_flag;
        parameter_ranges = [
            [bounds[0].unwrap_or(0.0), bounds[1].unwrap_or(0.0)],
            [bounds[2].unwrap_or(0.0), bounds[3].unwrap_or(0.0)],
        ];
        type_code = take_tagged_int(span, &mut position, 0x15, int_width)?;
        revision_form = Some(cadmpeg_ir::geometry::RevisionSurfaceForm {
            revision,
            support_bounds: bounds,
            reference_endpoints: [None; 2],
            second_endpoints: [None; 2],
            flags: Vec::new(),
            tail_enum,
            discontinuities: tail_discontinuities,
            tail_flag,
            trailing_flags: Vec::new(),
        });
    } else {
        let cache = decode_surface_block(span, position, int_width)?;
        position = cache.end;
        cache_fit_tolerance = Some(take_f64(span, &mut position)? * LEN_TO_MM);
        discontinuities = [
            take_float_array(span, &mut position, int_width)?,
            take_float_array(span, &mut position, int_width)?,
            take_float_array(span, &mut position, int_width)?,
            take_float_array(span, &mut position, int_width)?,
            take_float_array(span, &mut position, int_width)?,
            take_float_array(span, &mut position, int_width)?,
        ];
        discontinuity_flag = take_bool(span, &mut position)?;
        parameter_ranges = [
            [
                take_f64(span, &mut position)? * LEN_TO_MM,
                take_f64(span, &mut position)? * LEN_TO_MM,
            ],
            [
                take_f64(span, &mut position)? * LEN_TO_MM,
                take_f64(span, &mut position)? * LEN_TO_MM,
            ],
        ];
        type_code = take_tagged_int(span, &mut position, 0x04, int_width)?;
        revision_form = None;
    }
    if span.get(position) != Some(&0x0f) {
        return None;
    }
    position += 1;
    let source_kind = take_native_ident(span, &mut position)?;
    let subtransform = match source_kind.as_str() {
        "t_spl_subtrans_object" => {
            let program = take_native_string(span, &mut position)?;
            let separator = if span.get(position) == Some(&0x08) {
                None
            } else {
                Some(take_bool(span, &mut position)?)
            };
            let values = take_native_string(span, &mut position)?;
            TSplineSubtransform::Inline {
                program,
                separator,
                values,
            }
        }
        "ref" => TSplineSubtransform::Reference {
            index: take_tagged_int(span, &mut position, 0x04, int_width)?,
            resolved: None,
        },
        _ => return None,
    };
    if span.get(position) != Some(&0x10) {
        return None;
    }
    position += 1;
    let trailing_value = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let program_graph = match &subtransform {
        TSplineSubtransform::Inline { program, .. } => {
            Some(cadmpeg_ir::geometry::TSplineProgram::parse(program))
        }
        TSplineSubtransform::Reference { .. } => None,
    };
    let values_graph = match &subtransform {
        TSplineSubtransform::Inline { values, .. } => {
            Some(cadmpeg_ir::geometry::TSplineProgram::parse(values))
        }
        TSplineSubtransform::Reference { .. } => None,
    };
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::TSpline(Box::new(
            TSplineSurfaceConstruction {
                parameter_ranges,
                type_code,
                subtransform,
                program_graph,
                values_graph,
                trailing_value,
                discontinuities,
                discontinuity_flag,
                revision_form,
            },
        )),
        cache_fit_tolerance,
    })
}

fn decode_deformable_surface_frame(
    bytes: &[u8],
    position: &mut usize,
) -> Option<cadmpeg_ir::geometry::DeformableSurfaceFrame> {
    let mut leading_vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
    for vector in &mut leading_vectors {
        let value = take_native_vec3(bytes, position, 0x14)?;
        *vector = Vector3::new(value[0], value[1], value[2]);
    }
    let leading_parameter = take_f64(bytes, position)?;
    let leading_flags = [
        take_bool(bytes, position)?,
        take_bool(bytes, position)?,
        take_bool(bytes, position)?,
    ];
    let mut secondary_vectors = [Vector3::new(0.0, 0.0, 0.0); 3];
    for vector in &mut secondary_vectors {
        let value = take_native_vec3(bytes, position, 0x14)?;
        *vector = Vector3::new(value[0], value[1], value[2]);
    }
    let secondary_parameter = take_f64(bytes, position)?;
    let secondary_flags = [take_bool(bytes, position)?, take_bool(bytes, position)?];
    let point = take_native_vec3(bytes, position, 0x13)?;
    let trailing_flags = [
        take_bool(bytes, position)?,
        take_bool(bytes, position)?,
        take_bool(bytes, position)?,
        take_bool(bytes, position)?,
        take_bool(bytes, position)?,
    ];
    Some(cadmpeg_ir::geometry::DeformableSurfaceFrame {
        leading_vectors,
        leading_parameter,
        leading_flags,
        secondary_vectors,
        secondary_parameter,
        secondary_flags,
        point: Point3::new(
            point[0] * LEN_TO_MM,
            point[1] * LEN_TO_MM,
            point[2] * LEN_TO_MM,
        ),
        trailing_flags,
    })
}

fn decode_deformable_vector_frame(
    bytes: &[u8],
    position: &mut usize,
) -> Option<cadmpeg_ir::geometry::DeformableVectorFrame> {
    let mut vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
    for vector in &mut vectors {
        let value = take_native_vec3(bytes, position, 0x14)?;
        *vector = Vector3::new(value[0], value[1], value[2]);
    }
    Some(cadmpeg_ir::geometry::DeformableVectorFrame {
        vectors,
        parameter: take_f64(bytes, position)?,
        flags: [
            take_bool(bytes, position)?,
            take_bool(bytes, position)?,
            take_bool(bytes, position)?,
        ],
    })
}

fn decode_defm_spl_sur(record_bytes: &[u8], int_width: usize) -> Option<DecodedProceduralSurface> {
    use cadmpeg_ir::geometry::DeformableSurfaceData;
    let names: [&[u8]; 2] = [b"defm_spl_sur", b"defmsur"];
    let (start, name_len) =
        find_subtype_marker(record_bytes, &names).map(|(start, name)| (start, name.len()))?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name_len + 3;
    let support = decode_embedded_surface(span, &mut position, int_width)?;
    let mode = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let data = match mode {
        1 => {
            let frame = Box::new(decode_deformable_surface_frame(span, &mut position)?);
            let count =
                usize::try_from(take_tagged_int(span, &mut position, 0x04, int_width)?).ok()?;
            let parameter_triples = (0..count)
                .map(|_| {
                    Some([
                        take_f64(span, &mut position)?,
                        take_f64(span, &mut position)?,
                        take_f64(span, &mut position)?,
                    ])
                })
                .collect::<Option<Vec<_>>>()?;
            EmbeddedDeformableSurfaceData::Resolved(DeformableSurfaceData::Plain {
                frame,
                parameter_triples,
            })
        }
        3 => EmbeddedDeformableSurfaceData::Resolved(DeformableSurfaceData::Guided {
            frame: Box::new(decode_deformable_surface_frame(span, &mut position)?),
            selector: take_tagged_int(span, &mut position, 0x04, int_width)?,
            guide_parameter: take_f64(span, &mut position)?,
        }),
        5 => {
            let surface = decode_embedded_surface(span, &mut position, int_width)?;
            let native_id = take_tagged_int(span, &mut position, 0x04, int_width)?;
            let flag = take_bool(span, &mut position)?;
            let first_parameter = take_f64(span, &mut position)?;
            let selector = take_tagged_int(span, &mut position, 0x04, int_width)?;
            let second_parameter = take_f64(span, &mut position)?;
            let curve = decode_curve_block(span, position, int_width)?;
            position = curve.end;
            let mut vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
            for vector in &mut vectors {
                let value = take_native_vec3(span, &mut position, 0x14)?;
                *vector = Vector3::new(value[0], value[1], value[2]);
            }
            let frame_parameter = take_f64(span, &mut position)?;
            let flags = [
                take_bool(span, &mut position)?,
                take_bool(span, &mut position)?,
                take_bool(span, &mut position)?,
            ];
            let count =
                usize::try_from(take_tagged_int(span, &mut position, 0x04, int_width)?).ok()?;
            let parameter_triples = (0..count)
                .map(|_| {
                    Some([
                        take_f64(span, &mut position)?,
                        take_f64(span, &mut position)?,
                        take_f64(span, &mut position)?,
                    ])
                })
                .collect::<Option<Vec<_>>>()?;
            EmbeddedDeformableSurfaceData::SurfaceCurve {
                surface,
                native_id,
                flag,
                first_parameter,
                selector,
                second_parameter,
                curve: curve.curve,
                vectors,
                frame_parameter,
                flags,
                parameter_triples,
            }
        }
        6 => {
            let mut leading_vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
            for vector in &mut leading_vectors {
                let value = take_native_vec3(span, &mut position, 0x14)?;
                *vector = Vector3::new(value[0], value[1], value[2]);
            }
            let leading_parameter = take_f64(span, &mut position)?;
            let leading_flags = [
                take_bool(span, &mut position)?,
                take_bool(span, &mut position)?,
                take_bool(span, &mut position)?,
            ];
            let selector = take_tagged_int(span, &mut position, 0x04, int_width)?;
            let surface = decode_embedded_surface(span, &mut position, int_width)?;
            let native_id = take_tagged_int(span, &mut position, 0x04, int_width)?;
            let flag = take_bool(span, &mut position)?;
            let first_parameter = take_f64(span, &mut position)?;
            let version_value = (span.get(position) == Some(&0x04))
                .then(|| take_tagged_int(span, &mut position, 0x04, int_width))
                .flatten();
            let second_parameter = take_f64(span, &mut position)?;
            let curve = decode_curve_block(span, position, int_width)?;
            position = curve.end;
            let frames = Box::new([
                decode_deformable_vector_frame(span, &mut position)?,
                decode_deformable_vector_frame(span, &mut position)?,
            ]);
            EmbeddedDeformableSurfaceData::Full {
                leading_vectors,
                leading_parameter,
                leading_flags,
                selector,
                surface,
                native_id,
                flag,
                first_parameter,
                version_value,
                second_parameter,
                curve: curve.curve,
                frames,
                trailing_value: take_tagged_int(span, &mut position, 0x04, int_width)?,
            }
        }
        8 => {
            let mut vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
            for vector in &mut vectors {
                let value = take_native_vec3(span, &mut position, 0x14)?;
                *vector = Vector3::new(value[0], value[1], value[2]);
            }
            EmbeddedDeformableSurfaceData::Resolved(DeformableSurfaceData::Minimal {
                vectors,
                selector: take_tagged_int(span, &mut position, 0x04, int_width)?,
            })
        }
        _ => return None,
    };
    let cache = decode_surface_block(span, position, int_width)?;
    position = cache.end;
    let cache_fit_tolerance = Some(take_f64(span, &mut position)? * LEN_TO_MM);
    let discontinuities = [
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
    ];
    let discontinuity_flag = take_bool(span, &mut position)?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Deformable(Box::new(
            EmbeddedDeformableSurface {
                support,
                data,
                discontinuities,
                discontinuity_flag,
            },
        )),
        cache_fit_tolerance,
    })
}

pub(crate) fn decode_helix_spl_sur(
    record_bytes: &[u8],
    int_width: usize,
) -> Option<DecodedProceduralSurface> {
    use cadmpeg_ir::geometry::{
        HelixPathConstruction, HelixSurfaceConstruction, HelixSurfaceProfile,
    };

    let names: [&[u8]; 2] = [b"helix_spl_circ", b"helix_spl_line"];
    let (start, name) = find_subtype_marker(record_bytes, &names)?;
    let name_len = name.len();
    let circular = name == b"helix_spl_circ";
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name_len + 3;
    let current_layout = take_optional_helix_revision(span, &mut position, int_width)?;
    let angle_range = [
        take_range_value(span, &mut position)?,
        take_range_value(span, &mut position)?,
    ];
    let dimension_scale = if circular { LEN_TO_MM } else { 1.0 };
    let dimension_range = [
        take_range_value(span, &mut position)? * dimension_scale,
        take_range_value(span, &mut position)? * dimension_scale,
    ];
    let length = circular
        .then(|| take_f64(span, &mut position).map(|v| v * LEN_TO_MM))
        .flatten();
    let path_angle_range = [
        take_range_value(span, &mut position)?,
        take_range_value(span, &mut position)?,
    ];
    let center = take_native_vec3(span, &mut position, 0x13)?;
    let vector_tag = if current_layout { 0x14 } else { 0x13 };
    let major = take_native_vec3(span, &mut position, vector_tag)?;
    let minor = take_native_vec3(span, &mut position, vector_tag)?;
    let pitch = take_native_vec3(span, &mut position, vector_tag)?;
    let apex_factor = take_f64(span, &mut position)?;
    let axis = normalized(take_native_vec3(span, &mut position, 0x14)?)?;
    for sentinel in ["null_surface", "null_surface", "nullbs", "nullbs"] {
        if take_native_ident(span, &mut position)?.as_str() != sentinel {
            return None;
        }
    }
    let path = HelixPathConstruction {
        angle_range: path_angle_range,
        center: Point3::new(
            center[0] * LEN_TO_MM,
            center[1] * LEN_TO_MM,
            center[2] * LEN_TO_MM,
        ),
        major: Vector3::new(
            major[0] * LEN_TO_MM,
            major[1] * LEN_TO_MM,
            major[2] * LEN_TO_MM,
        ),
        minor: Vector3::new(
            minor[0] * LEN_TO_MM,
            minor[1] * LEN_TO_MM,
            minor[2] * LEN_TO_MM,
        ),
        pitch: Vector3::new(
            pitch[0] * LEN_TO_MM,
            pitch[1] * LEN_TO_MM,
            pitch[2] * LEN_TO_MM,
        ),
        apex_factor,
        axis,
    };
    let profile = if let Some(length) = length {
        HelixSurfaceProfile::Circle {
            length,
            radius: take_f64(span, &mut position)? * LEN_TO_MM,
        }
    } else {
        let direction = take_native_vec3(
            span,
            &mut position,
            if current_layout { 0x14 } else { 0x13 },
        )?;
        HelixSurfaceProfile::Line {
            direction: Vector3::new(
                direction[0] * LEN_TO_MM,
                direction[1] * LEN_TO_MM,
                direction[2] * LEN_TO_MM,
            ),
        }
    };
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Helix(Box::new(HelixSurfaceConstruction {
            angle_range,
            dimension_range,
            path,
            profile,
        })),
        cache_fit_tolerance: None,
    })
}

fn decode_t_spline_subtransform(
    bytes: &[u8],
    int_width: usize,
) -> Option<cadmpeg_ir::geometry::TSplineSubtransform> {
    use cadmpeg_ir::geometry::TSplineSubtransform;

    let mut position = usize::from(bytes.first() == Some(&0x0f));
    match take_native_ident(bytes, &mut position)?.as_str() {
        "t_spl_subtrans_object" => {
            let program = take_native_string(bytes, &mut position)?;
            let separator = if bytes.get(position) == Some(&0x08) {
                None
            } else {
                Some(take_bool(bytes, &mut position)?)
            };
            let values = take_native_string(bytes, &mut position)?;
            Some(TSplineSubtransform::Inline {
                program,
                separator,
                values,
            })
        }
        "ref" => Some(TSplineSubtransform::Reference {
            index: take_tagged_int(bytes, &mut position, 0x04, int_width)?,
            resolved: None,
        }),
        _ => None,
    }
}

fn resolve_t_spline_subtransform(
    index: usize,
    active_bytes: &[u8],
    table: &[usize],
    seen: &mut Vec<usize>,
    int_width: usize,
) -> Option<cadmpeg_ir::geometry::TSplineSubtransform> {
    use cadmpeg_ir::geometry::TSplineSubtransform;

    if seen.contains(&index) {
        return None;
    }
    seen.push(index);
    let target = *table.get(index)?;
    let decoded =
        decode_t_spline_subtransform(subtype_span(active_bytes, target, int_width)?, int_width)?;
    match decoded {
        inline @ TSplineSubtransform::Inline { .. } => Some(inline),
        TSplineSubtransform::Reference { index, .. } => resolve_t_spline_subtransform(
            usize::try_from(index).ok()?,
            active_bytes,
            table,
            seen,
            int_width,
        ),
    }
}

/// Decode a native procedural definition, following nested subtype-table references.
pub fn decode_procedural_surface_resolving_refs(
    record_bytes: &[u8],
    active_bytes: &[u8],
    tables: &SubtypeTables,
) -> Option<DecodedProceduralSurface> {
    INT_WIDTHS.into_iter().find_map(|int_width| {
        decode_procedural_resolving_refs(
            record_bytes,
            active_bytes,
            tables,
            &mut Vec::new(),
            int_width,
        )
    })
}

fn decode_procedural_resolving_refs(
    bytes: &[u8],
    active_bytes: &[u8],
    tables: &SubtypeTables,
    seen: &mut Vec<usize>,
    int_width: usize,
) -> Option<DecodedProceduralSurface> {
    if let Some(mut decoded) = decode_defm_spl_sur(bytes, int_width)
        .or_else(|| decode_helix_spl_sur(bytes, int_width))
        .or_else(|| decode_t_spl_sur(bytes, int_width))
        .or_else(|| decode_exact_spl_sur(bytes, int_width))
        .or_else(|| decode_comp_spl_sur(bytes, int_width))
        .or_else(|| decode_taper_spl_sur(bytes, int_width, Some((active_bytes, tables))))
        .or_else(|| decode_loft_spl_sur(bytes, int_width, Some((active_bytes, tables))))
        .or_else(|| decode_compound_loft_spl_sur(bytes, int_width, Some((active_bytes, tables))))
        .or_else(|| decode_scaled_compound_loft_spl_sur(bytes, int_width))
        .or_else(|| decode_sub_spl_sur(bytes, int_width))
        .or_else(|| decode_law_spl_sur(bytes, int_width))
        .or_else(|| decode_skin_spl_sur(bytes, int_width))
        .or_else(|| decode_net_spl_sur(bytes, int_width))
        .or_else(|| decode_sweep_spl_sur(bytes, int_width, Some((active_bytes, tables))))
        .or_else(|| decode_g2_blend_spl_sur(bytes, int_width, Some((active_bytes, tables))))
        .or_else(|| decode_ruled_spl_sur(bytes, int_width))
        .or_else(|| decode_sum_spl_sur(bytes, int_width, Some((active_bytes, tables))))
        .or_else(|| decode_rot_spl_sur(bytes, int_width, Some((active_bytes, tables))))
        .or_else(|| decode_off_spl_sur(bytes, int_width, Some((active_bytes, tables))))
        .or_else(|| decode_cyl_spl_sur_at(bytes, int_width))
        .or_else(|| decode_var_blend_spl_sur(bytes, int_width, Some((active_bytes, tables))))
        .or_else(|| decode_vertex_blend_spl_sur(bytes, int_width, Some((active_bytes, tables))))
        .or_else(|| decode_full_rb_blend_spl_sur(bytes, int_width, active_bytes, tables))
        .or_else(|| decode_rb_blend_spl_sur_fallback(bytes, int_width))
    {
        if let DecodedProceduralSurfaceDefinition::TSpline(construction) = &mut decoded.definition {
            if let cadmpeg_ir::geometry::TSplineSubtransform::Reference { index, resolved } =
                &mut construction.subtransform
            {
                let inline = resolve_t_spline_subtransform(
                    usize::try_from(*index).ok()?,
                    active_bytes,
                    tables.for_width(int_width),
                    &mut Vec::new(),
                    int_width,
                )?;
                let program = match &inline {
                    cadmpeg_ir::geometry::TSplineSubtransform::Inline { program, .. } => program,
                    cadmpeg_ir::geometry::TSplineSubtransform::Reference { .. } => return None,
                };
                construction.program_graph =
                    Some(cadmpeg_ir::geometry::TSplineProgram::parse(program));
                let values = match &inline {
                    cadmpeg_ir::geometry::TSplineSubtransform::Inline { values, .. } => values,
                    cadmpeg_ir::geometry::TSplineSubtransform::Reference { .. } => return None,
                };
                construction.values_graph =
                    Some(cadmpeg_ir::geometry::TSplineProgram::parse(values));
                *resolved = Some(Box::new(inline));
            }
        }
        return Some(decoded);
    }
    let table = tables.for_width(int_width);
    for index in subtype_refs(bytes, int_width) {
        if seen.contains(&index) {
            continue;
        }
        let target = *table.get(index)?;
        seen.push(index);
        if let Some(decoded) = decode_procedural_resolving_refs(
            subtype_span(active_bytes, target, int_width)?,
            active_bytes,
            tables,
            seen,
            int_width,
        ) {
            return Some(decoded);
        }
    }
    None
}
