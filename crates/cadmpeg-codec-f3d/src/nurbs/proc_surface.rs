// SPDX-License-Identifier: Apache-2.0
//! Procedural spline-surface embedded types and their `_spl_sur` decoders.

use crate::nurbs::blend::{
    cyl_spl_sur, full_rb_blend_spl_sur, rb_blend_spl_sur_fallback, rolling_ball_side,
    var_blend_spl_sur, vertex_blend_spl_sur,
};
use crate::nurbs::core::{curve_block, surface_block};
use crate::nurbs::pcurve::{decode_pcurve_block_with_end, pcurve_block_with_end, NurbsPcurve};
use crate::nurbs::proc_curve::{
    embedded_base_curve_resolving_refs, embedded_surface, optional_embedded_surface_with_bounds,
    optional_helix_revision,
};
use crate::nurbs::reader::{normalized, take_native_ident, LEN_TO_MM};
use crate::nurbs::toks::{self, Cur, SubtypeTable};
use crate::sab::Token;
use cadmpeg_codec_core::cursor::bounded_len;
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, NurbsCurve, NurbsSurface, SurfaceGeometry,
};
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
        /// Native U sense enum, absent from the revision-gated layout.
        u_sense: Option<i64>,
        /// Native V sense enum, absent from the revision-gated layout.
        v_sense: Option<i64>,
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
        /// Revision-gated form fields.
        revision_form: Option<cadmpeg_ir::geometry::RevisionSurfaceForm>,
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
    /// Enum opening the shared revision-gated surface tail.
    pub(crate) tail_enum: i64,
    /// Parameterization stored by tail-enum form `2` in place of a solved cache.
    pub(crate) tail_parameterization: Option<cadmpeg_ir::geometry::RevisionSurfaceParameterization>,
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
    pub(crate) subtype: cadmpeg_ir::geometry::VariableBlendSurfaceSubtype,
    pub(crate) revision: i64,
    pub(crate) sides: Box<[EmbeddedRollingBallSide; 2]>,
    pub(crate) slice: CurveGeometry,
    pub(crate) slice_range: [Option<f64>; 2],
    pub(crate) offsets: [f64; 2],
    pub(crate) radius_kind: cadmpeg_ir::geometry::VariableBlendRadiusKind,
    pub(crate) first_value: cadmpeg_ir::geometry::VariableBlendValue,
    pub(crate) second_value: Option<cadmpeg_ir::geometry::VariableBlendValue>,
    pub(crate) cross_section: Option<cadmpeg_ir::geometry::VariableBlendCrossSection>,
    /// Support-side parameter interval `(T0, T1)`.
    pub(crate) u_range: [Option<f64>; 2],
    /// Second interval `(T lo, F)`: a lower bound with an unbounded-above
    /// marker decoding to `[Some(lo), None]`.
    pub(crate) v_range: [Option<f64>; 2],
    /// Approximation-current flag (`1` when the cache is current).
    pub(crate) shape_prefix: i64,
    /// Requested fit tolerance.
    pub(crate) shape_parameter: f64,
    /// Achieved fit tolerance, at or below `shape_parameter`.
    pub(crate) shape_length: f64,
    /// Signed integer immediately before the shared tail's enum, taking the
    /// values `-1` and `1`.
    pub(crate) shape_tail: i64,
    /// Enum opening the shared revision-gated surface tail.
    pub(crate) tail_enum: i64,
    /// Parameterization stored by tail-enum form `2` in place of a solved cache.
    pub(crate) tail_parameterization: Option<cadmpeg_ir::geometry::RevisionSurfaceParameterization>,
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
        sense: bool,
    },
    Degenerate {
        location: Point3,
        normals: [Vector3; 2],
    },
    Pcurve {
        surface: SurfaceGeometry,
        support_bounds: [Option<f64>; 4],
        pcurve: Option<NurbsPcurve>,
        sense: bool,
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
    pub(crate) boundary_type: bool,
    pub(crate) magic: Vector3,
    pub(crate) u_smoothing: bool,
    pub(crate) v_smoothing: bool,
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
    /// Enum opening the shared revision-gated surface tail.
    pub(crate) tail_enum: i64,
    /// Parameterization stored by tail-enum form `2` in place of a solved cache.
    pub(crate) tail_parameterization: Option<cadmpeg_ir::geometry::RevisionSurfaceParameterization>,
    pub(crate) discontinuities: [Vec<f64>; 6],
    pub(crate) tail_flag: bool,
    pub(crate) third: Option<Box<EmbeddedRollingBallThirdSide>>,
    pub(crate) tail_extensions: [i64; 3],
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

/// Decode a `nullbs`-or-2D-block pcurve slot. Token-space counterpart of
/// [`decode_nullable_embedded_pcurve`].
#[allow(clippy::option_option)] // Outer None is parse failure; inner None is native nullbs.
pub(crate) fn nullable_embedded_pcurve(cur: &mut Cur<'_>) -> Option<Option<NurbsPcurve>> {
    let saved = cur.pos();
    if cur.take_ident() == Some("nullbs") {
        return Some(None);
    }
    cur.set_pos(saved);
    let (pcurve, end) = pcurve_block_with_end(cur.toks(), cur.pos())?;
    cur.set_pos(end);
    Some(Some(pcurve))
}

fn g2_side(cur: &mut Cur<'_>) -> Option<EmbeddedG2Side> {
    let label = cur.take_str()?.to_string();
    let surface = embedded_surface(cur)?;
    let (curve, curve_end) = curve_block(cur.toks(), cur.pos())?;
    cur.set_pos(curve_end);
    let first = nullable_embedded_pcurve(cur)?;
    let direction = cur.take_vector3()?;
    let second = nullable_embedded_pcurve(cur)?;
    Some(EmbeddedG2Side {
        label,
        surface,
        curve,
        pcurves: [first, second],
        direction: Vector3::new(direction[0], direction[1], direction[2]),
    })
}

fn bridge_token(cur: &mut Cur<'_>) -> Option<cadmpeg_ir::geometry::LoftBridgeToken> {
    use cadmpeg_ir::geometry::LoftBridgeToken;
    match cur.peek()? {
        Token::True | Token::False => Some(LoftBridgeToken::Boolean(cur.take_bool()?)),
        Token::Long(_) => Some(LoftBridgeToken::Integer(cur.take_long()?)),
        Token::Double(_) => Some(LoftBridgeToken::Double(cur.take_f64()?)),
        Token::Enum(_) => Some(LoftBridgeToken::Enum(cur.take_enum()?)),
        Token::Str(_) => Some(LoftBridgeToken::Text(cur.take_str()?.to_string())),
        _ => None,
    }
}

fn g2_blend_spl_sur(
    toks: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    let names = ["g2_blend_spl_sur", "g2blnsur"];
    let (start, name) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    if matches!(cur.peek(), Some(Token::Long(_))) {
        // Revision-gated layout: revision integer, two scalars, two sides in
        // the variable-blend side layout, center curve with endpoints, two
        // radii, radius selector, optional U/V bounds, shape prologue,
        // shared tail, and three trailing integers. Only the modern name
        // stores it.
        (name == "g2_blend_spl_sur").then_some(())?;
        let revision = cur.take_long()?;
        (revision > 0).then_some(())?;
        let leading_parameters = [cur.take_f64()?, cur.take_f64()?];
        let sides = Box::new([
            rolling_ball_side(&mut cur, resolver)?,
            rolling_ball_side(&mut cur, resolver)?,
        ]);
        let table = resolver?;
        let center = embedded_base_curve_resolving_refs(&mut cur, table)?;
        let center_range = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        let radii = [cur.take_f64()? * LEN_TO_MM, cur.take_f64()? * LEN_TO_MM];
        let radius_selector = cur.take_enum()?;
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
            fit_tolerance,
            parameterization,
            discontinuities,
            tail_flag,
        } = revision_surface_tail(&mut cur)?;
        let tail_extensions = [cur.take_long()?, cur.take_long()?, cur.take_long()?];
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
                    tail_parameterization: parameterization,
                    discontinuities,
                    tail_flag,
                    tail_extensions,
                },
            )),
            cache_fit_tolerance: fit_tolerance,
        });
    }
    let first = g2_side(&mut cur)?;
    let singularity = cur.take_enum()?;
    let first_shape = if cur.peek().is_some_and(Token::is_payload_ident) {
        let saved = cur.pos();
        if cur.take_ident() == Some("nullbs") {
            EmbeddedG2FirstShape::Full {
                surface: None,
                tolerance: None,
            }
        } else {
            cur.set_pos(saved);
            let (surface, surface_end) = surface_block(span, cur.pos())?;
            cur.set_pos(surface_end);
            EmbeddedG2FirstShape::Full {
                surface: Some(surface),
                tolerance: Some(cur.take_f64()? * LEN_TO_MM),
            }
        }
    } else {
        let mut coefficients = [0.0; 9];
        for coefficient in &mut coefficients {
            *coefficient = cur.take_f64()?;
        }
        let tolerance = cur.take_f64()? * LEN_TO_MM;
        let extension = (!matches!(cur.peek(), Some(token)
            if matches!(token, Token::Str(_)) || token.is_payload_ident()))
        .then(|| bridge_token(&mut cur))
        .flatten();
        let pcurve = nullable_embedded_pcurve(&mut cur)?;
        EmbeddedG2FirstShape::None {
            coefficients,
            tolerance,
            extension,
            pcurve,
        }
    };
    let second = g2_side(&mut cur)?;
    let (second_exact, second_exact_end) = surface_block(span, cur.pos())?;
    cur.set_pos(second_exact_end);
    let (center, center_end) = curve_block(span, cur.pos())?;
    cur.set_pos(center_end);
    let center_parameters = [cur.take_f64()?, cur.take_f64()?];
    let center_flag = cur.take_long()?;
    let parameter_ranges = [
        [cur.take_f64()?, cur.take_f64()?],
        [cur.take_f64()?, cur.take_f64()?],
    ];
    let mut trailing_parameters = [0.0; 4];
    for parameter in &mut trailing_parameters {
        *parameter = cur.take_f64()?;
    }
    let (_, cache_end) = surface_block(span, cur.pos())?;
    let cache_fit_tolerance = match span.get(cache_end) {
        Some(Token::Double(value)) => Some(*value * LEN_TO_MM),
        _ => None,
    };
    cur.set_pos(cache_end + usize::from(cache_fit_tolerance.is_some()));
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::G2Blend(Box::new(EmbeddedG2Blend {
            first,
            singularity,
            first_shape,
            second,
            second_exact_surface: second_exact,
            center_curve: center,
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
    pub(crate) secondary_pcurve: Option<NurbsPcurve>,
    pub(crate) first_flag: Option<bool>,
    pub(crate) asm_extension: Option<i64>,
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
    /// Enum opening the shared revision-gated surface tail.
    pub(crate) tail_enum: i64,
    /// Parameterization stored by tail-enum form `2` in place of a solved cache.
    pub(crate) tail_parameterization: Option<cadmpeg_ir::geometry::RevisionSurfaceParameterization>,
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
    TransformVec {
        vectors: [Vector3; 4],
        scale: f64,
        flags: [bool; 3],
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
fn compound_loft_scale(cur: &mut Cur<'_>) -> Option<Option<EmbeddedCompoundLoftScale>> {
    if matches!(cur.peek(), Some(Token::True | Token::False)) {
        return Some(None);
    }
    let count = usize::try_from(cur.take_long()?).ok()?;
    if count > 100_000 {
        return None;
    }
    let mut members = Vec::with_capacity(count);
    for _ in 0..count {
        let type_code = cur.take_long()?;
        let (curve, curve_end) = curve_block(cur.toks(), cur.pos())?;
        cur.set_pos(curve_end);
        let data = loft_profile_data(cur)?;
        members.push(EmbeddedLoftProfileMember {
            type_code,
            curve,
            endpoints: None,
            data,
        });
    }
    let (path, path_end) = curve_block(cur.toks(), cur.pos())?;
    cur.set_pos(path_end);
    let auxiliary_count = usize::try_from(cur.take_long()?).ok()?;
    if auxiliary_count > 100_000 {
        return None;
    }
    let mut auxiliaries = Vec::with_capacity(auxiliary_count);
    for _ in 0..auxiliary_count {
        let (curve, curve_end) = curve_block(cur.toks(), cur.pos())?;
        cur.set_pos(curve_end);
        auxiliaries.push(curve);
    }
    let tail = [cur.take_long()?, cur.take_long()?];
    Some(Some(EmbeddedCompoundLoftScale {
        members,
        path,
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

/// Payload form of a revision-gated loft profile member, selected by the
/// member's type integer.
#[derive(Clone, Copy)]
enum RevisionLoftMemberForm {
    /// Nonzero type: bounded support surface, one nullable BS2 pcurve, and the
    /// first flag.
    Support,
    /// Zero type: two nullable BS2 pcurve slots and no first flag.
    PcurvePair,
}

impl RevisionLoftMemberForm {
    /// The form the member's type integer selects.
    fn of(type_code: i64) -> Self {
        if type_code == 0 {
            Self::PcurvePair
        } else {
            Self::Support
        }
    }
}

/// The highest stream save format version whose revision-gated loft profile
/// members omit the ASM integer between the member payload and its constraint
/// subdata. Save format 22300 through 22600 streams omit it; save format 23200
/// streams carry it.
const LOFT_ASM_EXTENSION_ABSENT_THROUGH: u32 = 22600;

/// Whether a revision-gated loft profile member in this stream carries the ASM
/// integer. The gate keys on the stream save format, not on the record's own
/// serializer revision stamp: one revision stamp takes the integer in a later
/// stream and omits it in an earlier one.
fn revision_loft_carries_asm_extension(table: &SubtypeTable) -> bool {
    table
        .save_format_version()
        .is_none_or(|version| version > LOFT_ASM_EXTENSION_ABSENT_THROUGH)
}

/// Revision-gated loft profile data: the type-selected member payload, an
/// optional ASM integer, and constraint subdata with trailing row pairs.
fn revision_loft_profile_data(
    cur: &mut Cur<'_>,
    table: &SubtypeTable,
    form: RevisionLoftMemberForm,
    asm_extension_present: bool,
) -> Option<EmbeddedLoftProfileData> {
    let (surface, support_bounds, pcurve, secondary_pcurve, first_flag) = match form {
        RevisionLoftMemberForm::Support => {
            let (surface, support_bounds) = optional_embedded_surface_with_bounds(cur, table)?;
            let pcurve = nullable_embedded_pcurve(cur)?;
            let first_flag = cur.take_bool()?;
            (surface, support_bounds, pcurve, None, Some(first_flag))
        }
        RevisionLoftMemberForm::PcurvePair => {
            let pcurve = nullable_embedded_pcurve(cur)?;
            let secondary_pcurve = nullable_embedded_pcurve(cur)?;
            (None, [None; 4], pcurve, secondary_pcurve, None)
        }
    };
    let asm_extension = if asm_extension_present {
        Some(cur.take_long()?)
    } else {
        None
    };
    let subdata = loft_subdata_form(cur, true)?;
    let direction = if cur.take_bool()? {
        let value = cur.take_vector3()?;
        Some(Vector3::new(value[0], value[1], value[2]))
    } else {
        None
    };
    Some(EmbeddedLoftProfileData {
        surface,
        support_bounds,
        pcurve,
        secondary_pcurve,
        first_flag,
        asm_extension,
        subdata,
        direction,
    })
}

fn revision_loft_section(
    cur: &mut Cur<'_>,
    table: &SubtypeTable,
    asm_extension_present: bool,
) -> Option<Vec<EmbeddedLoftSectionEntry>> {
    let count = usize::try_from(cur.take_long()?).ok()?;
    // Each entry consumes at least one double token for its parameter.
    let count = bounded_len(count as u64, 1, cur.rest().len())?;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let parameter = cur.take_f64()?;
        let member_count = usize::try_from(cur.take_long()?).ok()?;
        // Each member consumes at least its type-code token.
        let member_count = bounded_len(member_count as u64, 1, cur.rest().len())?;
        let mut profile = Vec::with_capacity(member_count);
        for _ in 0..member_count {
            let type_code = cur.take_long()?;
            let curve = embedded_base_curve_resolving_refs(cur, table)?;
            let endpoints = [
                cur.take_optional_range_value()?,
                cur.take_optional_range_value()?,
            ];
            let data = revision_loft_profile_data(
                cur,
                table,
                RevisionLoftMemberForm::of(type_code),
                asm_extension_present,
            )?;
            profile.push(EmbeddedLoftProfileMember {
                type_code,
                curve,
                endpoints: Some(endpoints),
                data,
            });
        }
        let saved = cur.pos();
        let (path_curve, path_endpoints) = if cur.take_ident() == Some("null_curve") {
            (None, None)
        } else {
            cur.set_pos(saved);
            let curve = embedded_base_curve_resolving_refs(cur, table)?;
            let endpoints = [
                cur.take_optional_range_value()?,
                cur.take_optional_range_value()?,
            ];
            (Some(curve), Some(endpoints))
        };
        let auxiliary_count = usize::try_from(cur.take_long()?).ok()?;
        // Each auxiliary consumes at least its curve-block marker token.
        let auxiliary_count = bounded_len(auxiliary_count as u64, 1, cur.rest().len())?;
        let mut auxiliaries = Vec::with_capacity(auxiliary_count);
        for _ in 0..auxiliary_count {
            let (auxiliary, auxiliary_end) = curve_block(cur.toks(), cur.pos())?;
            cur.set_pos(auxiliary_end);
            auxiliaries.push(auxiliary);
        }
        let flag = cur.take_long()?;
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

fn loft_subdata(cur: &mut Cur<'_>) -> Option<cadmpeg_ir::geometry::LoftSubdata> {
    loft_subdata_form(cur, false)
}

fn loft_subdata_form(
    cur: &mut Cur<'_>,
    revision: bool,
) -> Option<cadmpeg_ir::geometry::LoftSubdata> {
    use cadmpeg_ir::geometry::{LoftSubdata, LoftSubdataRow};
    let type_code = cur.take_long()?;
    let row_count = cur.take_long()?;
    let column_count = cur.take_long()?;
    let rows_to_read = if type_code == 211 {
        1
    } else {
        usize::try_from(row_count).ok()?
    };
    let columns_to_read = usize::try_from(column_count).ok()?;
    // Each row consumes two double tokens for its parameters.
    let rows_to_read = bounded_len(rows_to_read as u64, 2, cur.rest().len())?;
    let mut rows = Vec::with_capacity(rows_to_read);
    for _ in 0..rows_to_read {
        let parameters = [cur.take_f64()?, cur.take_f64()?];
        let mut columns = Vec::new();
        if type_code != 211 {
            columns.reserve(columns_to_read);
            for _ in 0..columns_to_read {
                columns.push([cur.take_f64()?, cur.take_f64()?]);
            }
        }
        let extra = if revision && type_code != 211 {
            Some([cur.take_f64()?, cur.take_f64()?])
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

fn loft_profile_data(cur: &mut Cur<'_>) -> Option<EmbeddedLoftProfileData> {
    let surface = embedded_surface(cur)?;
    let saved = cur.pos();
    let pcurve = if cur.take_ident() == Some("nullbs") {
        None
    } else {
        cur.set_pos(saved);
        let (pcurve, end) = pcurve_block_with_end(cur.toks(), cur.pos())?;
        cur.set_pos(end);
        Some(pcurve)
    };
    let first_flag = cur.take_bool()?;
    let asm_extension = cur.take_long()?;
    let subdata = loft_subdata(cur)?;
    let direction = if cur.take_bool()? {
        let value = cur.take_vector3()?;
        Some(Vector3::new(value[0], value[1], value[2]))
    } else {
        None
    };
    Some(EmbeddedLoftProfileData {
        surface: Some(surface),
        support_bounds: [None; 4],
        pcurve,
        secondary_pcurve: None,
        first_flag: Some(first_flag),
        asm_extension: Some(asm_extension),
        subdata,
        direction,
    })
}

fn loft_section(cur: &mut Cur<'_>) -> Option<Vec<EmbeddedLoftSectionEntry>> {
    let count = usize::try_from(cur.take_long()?).ok()?;
    // Each entry consumes at least one double token for its parameter.
    let count = bounded_len(count as u64, 1, cur.rest().len())?;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let parameter = cur.take_f64()?;
        let member_count = usize::try_from(cur.take_long()?).ok()?;
        // Each member consumes at least its type-code token.
        let member_count = bounded_len(member_count as u64, 1, cur.rest().len())?;
        let mut profile = Vec::with_capacity(member_count);
        for _ in 0..member_count {
            let type_code = cur.take_long()?;
            let (curve, curve_end) = curve_block(cur.toks(), cur.pos())?;
            cur.set_pos(curve_end);
            let data = loft_profile_data(cur)?;
            profile.push(EmbeddedLoftProfileMember {
                type_code,
                curve,
                endpoints: None,
                data,
            });
        }
        let (curve, curve_end) = curve_block(cur.toks(), cur.pos())?;
        cur.set_pos(curve_end);
        let auxiliary_count = usize::try_from(cur.take_long()?).ok()?;
        // Each auxiliary consumes at least its curve-block marker token.
        let auxiliary_count = bounded_len(auxiliary_count as u64, 1, cur.rest().len())?;
        let mut auxiliaries = Vec::with_capacity(auxiliary_count);
        for _ in 0..auxiliary_count {
            let (auxiliary, auxiliary_end) = curve_block(cur.toks(), cur.pos())?;
            cur.set_pos(auxiliary_end);
            auxiliaries.push(auxiliary);
        }
        let flag = cur.take_long()?;
        entries.push(EmbeddedLoftSectionEntry {
            parameter,
            profile,
            path: EmbeddedLoftPath {
                curve: Some(curve),
                endpoints: None,
                auxiliaries,
                flag,
            },
        });
    }
    Some(entries)
}

fn revision_loft(
    span: &[Token],
    position: usize,
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    let table = resolver?;
    let mut cur = Cur::at(span, position);
    let revision = cur.take_long()?;
    (revision > 0).then_some(())?;
    let asm_extension_present = revision_loft_carries_asm_extension(table);
    let sections = [
        revision_loft_section(&mut cur, table, asm_extension_present)?,
        revision_loft_section(&mut cur, table, asm_extension_present)?,
    ];
    let wrap_ranges = [
        [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ],
        [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ],
    ];
    let mut flags = [false; 4];
    for flag in &mut flags {
        *flag = cur.take_bool()?;
    }
    let ints = [cur.take_long()?, cur.take_long()?];
    let RevisionSurfaceTail {
        enumeration: tail_enum,
        fit_tolerance,
        parameterization,
        discontinuities,
        tail_flag,
    } = revision_surface_tail(&mut cur)?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Loft(EmbeddedLoft {
            sections,
            revision_form: Some(cadmpeg_ir::geometry::LoftRevisionForm {
                revision,
                flags,
                ints,
                tail_enum,
                tail_parameterization: parameterization,
                discontinuities,
                tail_flag,
            }),
            parameters: cadmpeg_ir::geometry::SplineSurfaceParameters::RevisionRanges {
                intervals: wrap_ranges,
            },
            closures: [0, 0],
            singularities: [0, 0],
            mode: 0,
            bridge: Vec::new(),
        }),
        cache_fit_tolerance: fit_tolerance,
    })
}

fn loft_spl_sur(
    toks: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    use cadmpeg_ir::geometry::LoftBridgeToken;
    let names = ["loft_spl_sur", "loftsur"];
    let (start, name) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    // Only the modern name stores the revision-gated layout.
    if matches!(cur.peek(), Some(Token::Long(_))) && name == "loft_spl_sur" {
        if let Some(decoded) = revision_loft(span, cur.pos(), resolver) {
            return Some(decoded);
        }
    }
    let sections = [loft_section(&mut cur)?, loft_section(&mut cur)?];
    let parameter_ranges = [
        [cur.take_f64()?, cur.take_f64()?],
        [cur.take_f64()?, cur.take_f64()?],
    ];
    let closures = [cur.take_enum()?, cur.take_enum()?];
    let singularities = [cur.take_enum()?, cur.take_enum()?];
    let mode = cur.take_long()?;
    let (cache_at, cache_end) = toks::marker_positions(span)
        .into_iter()
        .filter_map(|at| surface_block(span, at).map(|(_, end)| (at, end)))
        .next_back()?;
    let mut bridge = Vec::new();
    while cur.pos() < cache_at {
        match cur.peek()? {
            Token::True | Token::False => {
                bridge.push(LoftBridgeToken::Boolean(cur.take_bool()?));
            }
            Token::Long(_) => bridge.push(LoftBridgeToken::Integer(cur.take_long()?)),
            Token::Double(_) => bridge.push(LoftBridgeToken::Double(cur.take_f64()?)),
            Token::Enum(_) => bridge.push(LoftBridgeToken::Enum(cur.take_enum()?)),
            Token::Str(_) => bridge.push(LoftBridgeToken::Text(cur.take_str()?.to_string())),
            _ => return None,
        }
    }
    let cache_fit_tolerance = match span.get(cache_end) {
        Some(Token::Double(value)) => Some(*value * LEN_TO_MM),
        _ => None,
    };
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
fn revision_cl_scale(
    cur: &mut Cur<'_>,
    table: &SubtypeTable,
    asm_extension_present: bool,
) -> Option<(Vec<EmbeddedLoftProfileMember>, EmbeddedLoftPath)> {
    let member_count = usize::try_from(cur.take_long()?).ok()?;
    // Each member consumes at least its type-code token.
    let member_count = bounded_len(member_count as u64, 1, cur.rest().len())?;
    let mut profile = Vec::with_capacity(member_count);
    for _ in 0..member_count {
        let type_code = cur.take_long()?;
        let curve = embedded_base_curve_resolving_refs(cur, table)?;
        let endpoints = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        let data = revision_loft_profile_data(
            cur,
            table,
            RevisionLoftMemberForm::of(type_code),
            asm_extension_present,
        )?;
        profile.push(EmbeddedLoftProfileMember {
            type_code,
            curve,
            endpoints: Some(endpoints),
            data,
        });
    }
    let saved = cur.pos();
    let (path_curve, path_endpoints) = if cur.take_ident() == Some("null_curve") {
        (None, None)
    } else {
        cur.set_pos(saved);
        let curve = embedded_base_curve_resolving_refs(cur, table)?;
        let endpoints = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        (Some(curve), Some(endpoints))
    };
    let auxiliary_count = usize::try_from(cur.take_long()?).ok()?;
    // Each auxiliary consumes at least its curve-block marker token.
    let auxiliary_count = bounded_len(auxiliary_count as u64, 1, cur.rest().len())?;
    let mut auxiliaries = Vec::with_capacity(auxiliary_count);
    for _ in 0..auxiliary_count {
        let (auxiliary, auxiliary_end) = curve_block(cur.toks(), cur.pos())?;
        cur.set_pos(auxiliary_end);
        auxiliaries.push(auxiliary);
    }
    let flag = cur.take_long()?;
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

fn revision_compound_loft(
    span: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    let table = resolver?;
    let mut cur = Cur::at(span, 2);
    let revision = cur.take_long()?;
    (revision > 0).then_some(())?;
    let RevisionSurfaceTail {
        enumeration: tail_enum,
        fit_tolerance,
        parameterization,
        discontinuities,
        tail_flag,
    } = revision_surface_tail(&mut cur)?;
    let asm_extension_present = revision_loft_carries_asm_extension(table);
    let (base_profile, base_path) = revision_cl_scale(&mut cur, table, asm_extension_present)?;
    let entry_count = usize::try_from(cur.take_long()?).ok()?;
    // Each entry consumes at least its member-count token.
    let entry_count = bounded_len(entry_count as u64, 1, cur.rest().len())?;
    let mut entries = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        let (profile, path) = revision_cl_scale(&mut cur, table, asm_extension_present)?;
        let parameter = cur.take_f64()?;
        entries.push(EmbeddedLoftSectionEntry {
            parameter,
            profile,
            path,
        });
    }
    let flags = [cur.take_bool()?, cur.take_bool()?];
    let kind = cur.take_long()?;
    // Only the kind-zero payload is defined for the revision layout.
    (kind == 0).then_some(())?;
    let kind_flags = [cur.take_bool()?, cur.take_bool()?];
    let selector = cur.take_long()?;
    let (direction, direction_curve) = if selector == 0 {
        let value = cur.take_vector3()?;
        (Some(Vector3::new(value[0], value[1], value[2])), None)
    } else {
        let (curve, curve_end) = curve_block(span, cur.pos())?;
        cur.set_pos(curve_end);
        (None, Some(curve))
    };
    let interval = [
        cur.take_optional_range_value()?,
        cur.take_optional_range_value()?,
    ];
    // The trailing curve is present exactly when both parameter values are
    // present. Nothing in the stream marks its absence; the parameter pair
    // is what selects it.
    let trailing_curve = if interval.iter().all(Option::is_some) {
        let (curve, curve_end) = curve_block(span, cur.pos())?;
        cur.set_pos(curve_end);
        Some(curve)
    } else {
        None
    };
    matches!(span.get(cur.pos()), Some(Token::SubtypeClose)).then_some(())?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::RevisionCompoundLoft(Box::new(
            EmbeddedRevisionCompoundLoft {
                revision,
                tail_enum,
                tail_parameterization: parameterization,
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
        cache_fit_tolerance: fit_tolerance,
    })
}

fn compound_loft_spl_sur(
    toks: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    let (start, _) = toks::find_owned_subtype_marker(toks, &["cl_loft_spl_sur"])?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    if matches!(cur.peek(), Some(Token::Long(_))) {
        return revision_compound_loft(span, resolver);
    }
    let (_, cache_end) = surface_block(span, cur.pos())?;
    cur.set_pos(cache_end);
    let cache_fit_tolerance = Some(cur.take_f64()? * LEN_TO_MM);
    let scales = Box::new([
        compound_loft_scale(&mut cur)?,
        compound_loft_scale(&mut cur)?,
        compound_loft_scale(&mut cur)?,
        compound_loft_scale(&mut cur)?,
    ]);
    let fifth_scale = if matches!(cur.peek(), Some(Token::Long(_))) {
        compound_loft_scale(&mut cur)?.map(Box::new)
    } else {
        None
    };
    let flags = [cur.take_bool()?, cur.take_bool()?];
    let kind = cur.take_long()?;
    let tail = match kind {
        6 => {
            let tail_flags = [cur.take_bool()?, cur.take_bool()?];
            let scale = Box::new(compound_loft_scale(&mut cur)??);
            let selector = cur.take_long()?;
            let direction = cur.take_vector3()?;
            let parameter_range = [cur.take_range_value()?, cur.take_range_value()?];
            let (curve, _) = curve_block(span, cur.pos())?;
            EmbeddedCompoundLoftTail::Six {
                flags: tail_flags,
                scale,
                selector,
                direction: Vector3::new(direction[0], direction[1], direction[2]),
                parameter_range,
                curve,
            }
        }
        7 => {
            let first_flag = cur.take_bool()?;
            let first_scale = compound_loft_scale(&mut cur)?.map(Box::new);
            let second_flag = cur.take_bool()?;
            let second_scale = Box::new(compound_loft_scale(&mut cur)??);
            let selector = cur.take_long()?;
            let direction = cur.take_vector3()?;
            let trailing_flags = [cur.take_bool()?, cur.take_bool()?];
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
            let tail_flags = [cur.take_bool()?, cur.take_bool()?];
            let selector = cur.take_long()?;
            let direction = if selector == 0 {
                let value = cur.take_vector3()?;
                EmbeddedCompoundLoftDirection::Vector(Vector3::new(value[0], value[1], value[2]))
            } else {
                let (curve, curve_end) = curve_block(span, cur.pos())?;
                cur.set_pos(curve_end);
                EmbeddedCompoundLoftDirection::Curve(curve)
            };
            let trailing_flags = [cur.take_bool()?, cur.take_bool()?];
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

fn scaled_compound_loft_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    let names = ["scaled_cloft_spl_sur", "sclclftsur"];
    let (start, _) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let singularity = cur.take_enum()?;
    let (shape, cache_fit_tolerance) = if cur.peek().is_some_and(Token::is_payload_ident) {
        let (_, cache_end) = surface_block(span, cur.pos())?;
        cur.set_pos(cache_end);
        let tolerance = cur.take_f64()? * LEN_TO_MM;
        (EmbeddedScaledCompoundLoftShape::Full, Some(tolerance))
    } else {
        let parameter_ranges = [
            [cur.take_range_value()?, cur.take_range_value()?],
            [cur.take_range_value()?, cur.take_range_value()?],
        ];
        let parameters = [cur.take_float_array()?, cur.take_float_array()?];
        (
            EmbeddedScaledCompoundLoftShape::None {
                parameter_ranges,
                parameters,
            },
            None,
        )
    };
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let discontinuity_flag = cur.take_bool()?;
    let scales = Box::new([
        compound_loft_scale(&mut cur)?,
        compound_loft_scale(&mut cur)?,
        compound_loft_scale(&mut cur)?,
    ]);
    let flags = [cur.take_bool()?, cur.take_bool()?];
    let selector = cur.take_long()?;
    let extended = cur.take_bool()?;
    let branch = if extended {
        let first_scale = compound_loft_scale(&mut cur)?.map(Box::new);
        if cur.take_bool()? {
            let second_scale = Box::new(compound_loft_scale(&mut cur)??);
            let selector = cur.take_long()?;
            let direction = cur.take_vector3()?;
            EmbeddedScaledCompoundLoftBranch::ExtendedVector {
                first_scale,
                second_scale,
                selector,
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            }
        } else {
            let flag = cur.take_bool()?;
            let singularity = cur.take_enum()?;
            let (curve, curve_end) = curve_block(span, cur.pos())?;
            cur.set_pos(curve_end);
            EmbeddedScaledCompoundLoftBranch::ExtendedCurve {
                scale: first_scale,
                flag,
                singularity,
                curve,
            }
        }
    } else {
        let flag = cur.take_bool()?;
        let selector = cur.take_long()?;
        let direction = if selector == 0 {
            let direction = cur.take_vector3()?;
            EmbeddedCompoundLoftDirection::Vector(Vector3::new(
                direction[0],
                direction[1],
                direction[2],
            ))
        } else {
            let (curve, curve_end) = curve_block(span, cur.pos())?;
            cur.set_pos(curve_end);
            EmbeddedCompoundLoftDirection::Curve(curve)
        };
        EmbeddedScaledCompoundLoftBranch::Direct {
            flag,
            selector,
            direction,
        }
    };
    let trailing_flags = [cur.take_bool()?, cur.take_bool()?];
    let tail_kind = cur.take_long()?;
    let first = cur.take_vector3()?;
    let second = cur.take_vector3()?;
    let tail_singularity = cur.take_enum()?;
    let (tail_curve, _) = curve_block(span, cur.pos())?;
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
                tail_curve,
            },
        )),
        cache_fit_tolerance,
    })
}

/// Decode one recursive law expression.
pub(crate) fn law_expression(cur: &mut Cur<'_>, depth: usize) -> Option<EmbeddedLawExpression> {
    law_expression_resolving(cur, depth, None)
}

fn law_expression_resolving(
    cur: &mut Cur<'_>,
    depth: usize,
    resolver: Option<&SubtypeTable>,
) -> Option<EmbeddedLawExpression> {
    if depth > 64 {
        return None;
    }
    match cur.peek()? {
        Token::Long(_) => {
            return Some(EmbeddedLawExpression::Integer(cur.take_long()?));
        }
        Token::Double(_) => return Some(EmbeddedLawExpression::Double(cur.take_f64()?)),
        Token::Position(_) => {
            let value = cur.take_position()?;
            return Some(EmbeddedLawExpression::Point(Point3::new(
                value[0] * LEN_TO_MM,
                value[1] * LEN_TO_MM,
                value[2] * LEN_TO_MM,
            )));
        }
        Token::Vector3(_) => {
            let value = cur.take_vector3()?;
            return Some(EmbeddedLawExpression::Vector(Vector3::new(
                value[0], value[1], value[2],
            )));
        }
        _ => {}
    }
    let operator = cur.take_str()?.to_string();
    match operator.as_str() {
        "null_law" => Some(EmbeddedLawExpression::Null),
        "TRANS" => {
            if matches!(cur.peek(), Some(Token::Vector3(_))) {
                let mut vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
                for vector in &mut vectors {
                    let value = cur.take_vector3()?;
                    *vector = Vector3::new(value[0], value[1], value[2]);
                }
                let scale = cur.take_f64()?;
                let flags = [cur.take_bool()?, cur.take_bool()?, cur.take_bool()?];
                return Some(EmbeddedLawExpression::TransformVec {
                    vectors,
                    scale,
                    flags,
                });
            }
            let mut scalars = [0.0; 13];
            for scalar in &mut scalars {
                *scalar = cur.take_f64()?;
            }
            let enums = [cur.take_enum()?, cur.take_enum()?, cur.take_enum()?];
            Some(EmbeddedLawExpression::Transform { scalars, enums })
        }
        "EDGE" => {
            let (curve, endpoints) = if let Some((curve, end)) = curve_block(cur.toks(), cur.pos())
            {
                cur.set_pos(end);
                let endpoints = matches!(cur.peek(), Some(Token::True | Token::False))
                    .then(|| {
                        Some([
                            cur.take_optional_range_value()?,
                            cur.take_optional_range_value()?,
                        ])
                    })
                    .flatten();
                (curve, endpoints)
            } else {
                let table = resolver?;
                let curve = embedded_base_curve_resolving_refs(cur, table)?;
                let endpoints = Some([
                    cur.take_optional_range_value()?,
                    cur.take_optional_range_value()?,
                ]);
                (curve, endpoints)
            };
            let parameters = [cur.take_f64()?, cur.take_f64()?];
            Some(EmbeddedLawExpression::Edge {
                curve,
                endpoints,
                parameters,
            })
        }
        "SPLINE_LAW" => {
            let native_id = cur.take_long()?;
            let knots = cur.take_float_array()?;
            let controls = cur.take_float_array()?;
            let point = cur.take_position()?;
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
                .map(|_| law_expression_resolving(cur, depth + 1, resolver))
                .collect::<Option<Vec<_>>>()?;
            Some(EmbeddedLawExpression::Algebraic { operator, operands })
        }
    }
}

/// Decode one named law formula and its counted variables.
pub(crate) fn law_formula(cur: &mut Cur<'_>) -> Option<EmbeddedLawFormula> {
    law_formula_resolving(cur, None)
}

fn law_formula_resolving(
    cur: &mut Cur<'_>,
    resolver: Option<&SubtypeTable>,
) -> Option<EmbeddedLawFormula> {
    let name = cur.take_str()?.to_string();
    if name == "null_law" {
        return Some(EmbeddedLawFormula {
            name,
            variables: Vec::new(),
        });
    }
    let count = usize::try_from(cur.take_long()?).ok()?;
    if count > 100_000 {
        return None;
    }
    let variables = (0..count)
        .map(|_| law_expression_resolving(cur, 0, resolver))
        .collect::<Option<Vec<_>>>()?;
    Some(EmbeddedLawFormula { name, variables })
}

fn skin_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    let names = ["skin_spl_sur", "skinsur"];
    let (start, _) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let surface_boolean = cur.take_enum()?;
    let surface_normal = cur.take_enum()?;
    let surface_direction = cur.take_enum()?;
    let count = cur.take_long()?;
    let parameter = cur.take_f64()?;
    let inner_count = cur.take_long()?;
    let layout = if cur.peek().is_some_and(Token::is_payload_ident) {
        let (curve, curve_end) = curve_block(span, cur.pos())?;
        cur.set_pos(curve_end);
        let subdata = loft_subdata(&mut cur)?;
        let first_tail = cur.take_long()?;
        let (secondary_curve, secondary_end) = curve_block(span, cur.pos())?;
        cur.set_pos(secondary_end);
        let second_tail = cur.take_long()?;
        EmbeddedSkinSurfaceLayout::Compact {
            curve,
            subdata,
            first_tail,
            secondary_curve,
            second_tail,
        }
    } else {
        let profile_count = usize::try_from(inner_count).ok()?;
        if profile_count > 100_000 {
            return None;
        }
        let mut profiles = Vec::with_capacity(profile_count);
        for _ in 0..profile_count {
            let type_code = cur.take_long()?;
            let (curve, curve_end) = curve_block(span, cur.pos())?;
            cur.set_pos(curve_end);
            let data = loft_profile_data(&mut cur)?;
            profiles.push(EmbeddedLoftProfileMember {
                type_code,
                curve,
                endpoints: None,
                data,
            });
        }
        let (path, path_end) = curve_block(span, cur.pos())?;
        cur.set_pos(path_end);
        let tail = [cur.take_long()?, cur.take_long()?];
        EmbeddedSkinSurfaceLayout::Profiles {
            profiles,
            path,
            tail,
        }
    };
    let direction = cur.take_vector3()?;
    let trailing_parameter = cur.take_f64()?;
    let formula = law_formula(&mut cur)?;
    let (parameter_curve, parameter_curve_end) = curve_block(span, cur.pos())?;
    cur.set_pos(parameter_curve_end);
    let (_, cache_end) = surface_block(span, cur.pos())?;
    cur.set_pos(cache_end);
    let cache_fit_tolerance = Some(cur.take_f64()? * LEN_TO_MM);
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let discontinuity_flag = cur.take_bool()?;
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
            parameter_curve,
            discontinuities,
            discontinuity_flag,
        })),
        cache_fit_tolerance,
    })
}

pub(crate) fn law_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    let names = ["law_spl_sur", "lawsur"];
    let (start, _) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let parameter_ranges = if matches!(cur.peek(), Some(Token::Double(_))) {
        Some([
            [cur.take_f64()?, cur.take_f64()?],
            [cur.take_f64()?, cur.take_f64()?],
        ])
    } else {
        None
    };
    let primary = law_formula(&mut cur)?;
    let count = usize::try_from(cur.take_long()?).ok()?;
    if count > 100_000 {
        return None;
    }
    let additional = (0..count)
        .map(|_| law_formula(&mut cur))
        .collect::<Option<Vec<_>>>()?;
    let selector = if parameter_ranges.is_some()
        && toks::marker_at(span, cur.pos()) == Some(toks::BsplineMarker::Nubs)
    {
        0
    } else {
        cur.take_enum()?
    };
    let (tail, cache_fit_tolerance) = match selector {
        0 => {
            let (_, cache_end) = surface_block(span, cur.pos())?;
            cur.set_pos(cache_end);
            (
                cadmpeg_ir::geometry::LawSurfaceTail::Full,
                Some(cur.take_f64()? * LEN_TO_MM),
            )
        }
        1 => {
            let parameters = [cur.take_float_array()?, cur.take_float_array()?];
            let fit_tolerance = cur.take_f64()? * LEN_TO_MM;
            let closures = [cur.take_enum()?, cur.take_enum()?];
            let singularities = [cur.take_enum()?, cur.take_enum()?];
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
                [cur.take_f64()?, cur.take_f64()?],
                [cur.take_f64()?, cur.take_f64()?],
            ];
            let closures = [cur.take_enum()?, cur.take_enum()?];
            let singularities = [cur.take_enum()?, cur.take_enum()?];
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
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
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

pub(crate) fn sub_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    let names = ["sub_spl_sur", "subsur"];
    let (start, _) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let parameter_ranges = [
        [cur.take_f64()?, cur.take_f64()?],
        [cur.take_f64()?, cur.take_f64()?],
    ];
    let support = embedded_surface(&mut cur)?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::SubSurface {
            support,
            parameter_ranges,
        },
        cache_fit_tolerance: None,
    })
}

fn net_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    let names = ["net_spl_sur", "netsur"];
    let (start, _) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let sections = Box::new([loft_section(&mut cur)?, loft_section(&mut cur)?]);
    let mut frame_parameters = [0.0; 12];
    for parameter in &mut frame_parameters {
        *parameter = cur.take_f64()?;
    }
    let flag = cur.take_long()?;
    let mut directions = [Vector3::new(0.0, 0.0, 0.0); 4];
    for direction in &mut directions {
        let value = cur.take_vector3()?;
        *direction = Vector3::new(value[0], value[1], value[2]);
    }
    let formulas = (0..4)
        .map(|_| law_formula(&mut cur))
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    let (_, cache_end) = surface_block(span, cur.pos())?;
    cur.set_pos(cache_end);
    let cache_fit_tolerance = Some(cur.take_f64()? * LEN_TO_MM);
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let discontinuity_flag = cur.take_bool()?;
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

fn sweep_spl_sur(
    toks: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    let names = ["sweep_spl_sur", "sweep_sur", "sweepsur"];
    let (start, name) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    if matches!(cur.peek(), Some(Token::Long(_))) {
        // Only `sweep_sur` stores the revision-gated layout.
        (name == "sweep_sur").then_some(())?;
        return revision_sweep_sur(span, cur.pos(), resolver?);
    }
    let primary_kind = cur.take_enum()?;
    let layout = if cur.peek().is_some_and(Token::is_payload_ident) {
        let (profile, profile_end) = curve_block(span, cur.pos())?;
        cur.set_pos(profile_end);
        let (spine, spine_end) = curve_block(span, cur.pos())?;
        cur.set_pos(spine_end);
        let secondary_kind = cur.take_enum()?;
        let mut directions = [Vector3::new(0.0, 0.0, 0.0); 5];
        for direction in &mut directions {
            let value = cur.take_vector3()?;
            *direction = Vector3::new(value[0], value[1], value[2]);
        }
        let origin = cur.take_position()?;
        let mut parameters = [0.0; 4];
        for parameter in &mut parameters {
            *parameter = cur.take_f64()?;
        }
        let formulas = (0..3)
            .map(|_| law_formula(&mut cur))
            .collect::<Option<Vec<_>>>()?
            .try_into()
            .ok()?;
        EmbeddedSweepSurfaceLayout::ProfileFirst {
            profile,
            spine,
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
        let mode = cur.take_long()?;
        let (profile, profile_end) = curve_block(span, cur.pos())?;
        cur.set_pos(profile_end);
        let profile_range = [cur.take_f64()?, cur.take_f64()?];
        let profile_frame = if cur.take_bool()? {
            let point = cur.take_position()?;
            let vector = cur.take_vector3()?;
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
        let point = cur.take_position()?;
        let origin = Point3::new(
            point[0] * LEN_TO_MM,
            point[1] * LEN_TO_MM,
            point[2] * LEN_TO_MM,
        );
        let mut directions = [Vector3::new(0.0, 0.0, 0.0); 3];
        for direction in &mut directions {
            let value = cur.take_vector3()?;
            *direction = Vector3::new(value[0], value[1], value[2]);
        }
        if matches!(cur.peek(), Some(Token::Long(_))) {
            let branch = cur.take_long()?;
            let trajectory_flag = cur.take_bool()?;
            let (path, path_end) = curve_block(span, cur.pos())?;
            cur.set_pos(path_end);
            let path_range = [cur.take_f64()? * LEN_TO_MM, cur.take_f64()? * LEN_TO_MM];
            let path_parameter = cur.take_f64()?;
            match branch {
                1 => {
                    let formula_flag = cur.take_bool()?;
                    let formula = law_formula(&mut cur)?;
                    let trailing_flag = cur.take_bool()?;
                    EmbeddedSweepSurfaceLayout::ExplicitFormula {
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
                    }
                }
                2 => {
                    let guide_flags = [cur.take_bool()?, cur.take_bool()?];
                    let (guide_curve, guide_end) = curve_block(span, cur.pos())?;
                    cur.set_pos(guide_end);
                    let guide_range = [cur.take_f64()?, cur.take_f64()?];
                    let guide_modes = [cur.take_long()?, cur.take_long()?];
                    let mut guide_parameters = [0.0; 6];
                    for parameter in &mut guide_parameters {
                        *parameter = cur.take_f64()?;
                    }
                    let trailing_flags = [cur.take_bool()?, cur.take_bool()?, cur.take_bool()?];
                    EmbeddedSweepSurfaceLayout::ExplicitGuide {
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
                        guide_flags,
                        guide_curve,
                        guide_range,
                        guide_modes,
                        guide_parameters,
                        trailing_flags,
                    }
                }
                3 => {
                    let singularity = cur.take_enum()?;
                    let support_surface = embedded_surface(&mut cur)?;
                    let auxiliary_curve = if cur.take_bool()? {
                        let (curve, curve_end) = curve_block(span, cur.pos())?;
                        cur.set_pos(curve_end);
                        Some(curve)
                    } else {
                        None
                    };
                    let support_flag = cur.take_bool()?;
                    let legacy_flag = matches!(cur.peek(), Some(Token::True | Token::False))
                        .then(|| cur.take_bool())
                        .flatten();
                    EmbeddedSweepSurfaceLayout::ExplicitSurface {
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
            let first_law = law_expression(&mut cur, 0)?;
            let first_mode = cur.take_long()?;
            let first_range = [cur.take_f64()?, cur.take_f64()?];
            let vector = cur.take_vector3()?;
            let law_direction = Vector3::new(vector[0], vector[1], vector[2]);
            let path_mode = cur.take_long()?;
            let path_flag = cur.take_bool()?;
            let (path, path_end) = curve_block(span, cur.pos())?;
            cur.set_pos(path_end);
            let path_range = [cur.take_f64()?, cur.take_f64()?];
            let path_parameter = cur.take_f64()?;
            let second_law_flag = cur.take_bool()?;
            let second_law = law_expression(&mut cur, 0)?;
            let formula_mode = cur.take_long()?;
            let formula = law_formula(&mut cur)?;
            let trailing_flag = cur.take_bool()?;
            EmbeddedSweepSurfaceLayout::LawDriven {
                profile,
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
                path,
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
    let (_, cache_end) = surface_block(span, cur.pos())?;
    cur.set_pos(cache_end);
    let cache_fit_tolerance = Some(cur.take_f64()? * LEN_TO_MM);
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let discontinuity_flag = cur.take_bool()?;
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
fn revision_sweep_sur(
    span: &[Token],
    position: usize,
    table: &SubtypeTable,
) -> Option<DecodedProceduralSurface> {
    let mut cur = Cur::at(span, position);
    let revision = cur.take_long()?;
    (revision > 0).then_some(())?;
    let primary_flag = cur.take_bool()?;
    let mode = cur.take_long()?;
    let profile = embedded_base_curve_resolving_refs(&mut cur, table)?;
    let profile_endpoints = [
        cur.take_optional_range_value()?,
        cur.take_optional_range_value()?,
    ];
    let profile_range = [
        cur.take_optional_range_value()??,
        cur.take_optional_range_value()??,
    ];
    let profile_frame = if cur.take_bool()? {
        let point = cur.take_position()?;
        let vector = cur.take_vector3()?;
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
    let point = cur.take_position()?;
    let origin = Point3::new(
        point[0] * LEN_TO_MM,
        point[1] * LEN_TO_MM,
        point[2] * LEN_TO_MM,
    );
    let mut directions = [Vector3::new(0.0, 0.0, 0.0); 3];
    for direction in &mut directions {
        let value = cur.take_vector3()?;
        *direction = Vector3::new(value[0], value[1], value[2]);
    }
    (cur.take_long()? == 1).then_some(())?;
    let trajectory_flag = cur.take_bool()?;
    let path = embedded_base_curve_resolving_refs(&mut cur, table)?;
    let path_endpoints = [
        cur.take_optional_range_value()?,
        cur.take_optional_range_value()?,
    ];
    let path_range = [
        cur.take_optional_range_value()?? * LEN_TO_MM,
        cur.take_optional_range_value()?? * LEN_TO_MM,
    ];
    let path_parameter = cur.take_f64()?;
    let formula_flag = cur.take_bool()?;
    let formula = law_formula_resolving(&mut cur, Some(table))?;
    let trailing_flag = cur.take_bool()?;
    let tail_enum = cur.take_enum()?;
    let (_, cache_end) = surface_block(span, cur.pos())?;
    cur.set_pos(cache_end);
    let cache_fit_tolerance = Some(cur.take_f64()? * LEN_TO_MM);
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let discontinuity_flag = cur.take_bool()?;
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

fn taper_spl_sur(
    toks: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    use cadmpeg_ir::geometry::TaperSurfaceKind;
    let names: &[(&str, u8)] = &[
        ("taper_spl_sur", 0),
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
    let candidates: Vec<&str> = names.iter().map(|(name, _)| *name).collect();
    let (start, name) = toks::find_owned_subtype_marker(toks, &candidates)?;
    let kind = names
        .iter()
        .find_map(|(candidate, kind)| (*candidate == name).then_some(*kind))?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    if matches!(cur.peek(), Some(Token::Long(_))) {
        // Revision-gated form, stored by the orthogonal subtype's modern name.
        (name == "ortho_spl_sur").then_some(())?;
        let table = resolver?;
        let revision = cur.take_long()?;
        (revision > 0).then_some(())?;
        let (support, support_bounds) = optional_embedded_surface_with_bounds(&mut cur, table)?;
        let support = support?;
        let reference = embedded_base_curve_resolving_refs(&mut cur, table)?;
        let reference_endpoints = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        let pcurve = nullable_embedded_pcurve(&mut cur)?;
        let parameter = cur.take_f64()?;
        let RevisionSurfaceTail {
            enumeration: tail_enum,
            fit_tolerance,
            parameterization,
            discontinuities,
            tail_flag,
        } = revision_surface_tail(&mut cur)?;
        // The single trailing logical after the shared tail is the record's own
        // orthogonal-sense field, positionally matching the text form's single
        // boolean. `tail_flag` above is the shared-tail illegal-region flag.
        let sense = cur.take_bool()?;
        return Some(DecodedProceduralSurface {
            definition: DecodedProceduralSurfaceDefinition::Taper {
                support,
                reference,
                pcurve,
                parameter,
                taper: TaperSurfaceKind::Orthogonal { sense },
                revision_form: Some(cadmpeg_ir::geometry::RevisionSurfaceForm {
                    revision,
                    support_bounds,
                    reference_endpoints,
                    second_endpoints: [None; 2],
                    flags: Vec::new(),
                    tail_enum,
                    tail_parameterization: parameterization,
                    discontinuities,
                    tail_flag,
                    trailing_flags: Vec::new(),
                }),
            },
            cache_fit_tolerance: fit_tolerance,
        });
    }
    let support = embedded_surface(&mut cur)?;
    let (reference, reference_end) = curve_block(span, cur.pos())?;
    cur.set_pos(reference_end);
    let saved = cur.pos();
    let pcurve = if cur.take_ident() == Some("nullbs") {
        None
    } else {
        cur.set_pos(saved);
        let (pcurve, end) = pcurve_block_with_end(span, cur.pos())?;
        cur.set_pos(end);
        Some(pcurve)
    };
    let parameter = cur.take_f64()?;
    let (_, cache_end) = toks::marker_positions(span)
        .into_iter()
        .filter_map(|at| surface_block(span, at))
        .next_back()?;
    let cache_fit_tolerance = match span.get(cache_end) {
        Some(Token::Double(value)) => Some(*value * LEN_TO_MM),
        _ => None,
    };
    cur.set_pos(cache_end + usize::from(cache_fit_tolerance.is_some()));
    let take_draft = |cur: &mut Cur<'_>| {
        let draft = cur.take_vector3()?;
        Some(Vector3::new(draft[0], draft[1], draft[2]))
    };
    let taper = match kind {
        0 => TaperSurfaceKind::Standard,
        1 => TaperSurfaceKind::Orthogonal {
            sense: cur.take_bool()?,
        },
        2 => TaperSurfaceKind::Edge {
            draft: take_draft(&mut cur)?,
        },
        3 => TaperSurfaceKind::Shadow {
            draft: take_draft(&mut cur)?,
            sine: cur.take_f64()?,
            cosine: cur.take_f64()?,
        },
        4 => TaperSurfaceKind::Ruled {
            draft: take_draft(&mut cur)?,
            sine: cur.take_f64()?,
            cosine: cur.take_f64()?,
            factor: cur.take_f64()?,
        },
        5 => TaperSurfaceKind::Swept {
            draft: take_draft(&mut cur)?,
            sine: cur.take_f64()?,
            cosine: cur.take_f64()?,
        },
        _ => return None,
    };
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Taper {
            support,
            reference,
            pcurve,
            parameter,
            taper,
            revision_form: None,
        },
        cache_fit_tolerance,
    })
}

fn comp_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    let (start, _) = toks::find_owned_subtype_marker(toks, &["comp_spl_sur"])?;
    let span = toks::subtype_span(toks, start)?;
    let (_, cache_end) = toks::marker_positions(span)
        .into_iter()
        .find_map(|at| surface_block(span, at))?;
    let cache_fit_tolerance = match span.get(cache_end) {
        Some(Token::Double(value)) => Some(*value * LEN_TO_MM),
        _ => None,
    };
    let mut cur = Cur::at(span, cache_end + usize::from(cache_fit_tolerance.is_some()));
    let parameters = cur.take_float_array()?;
    let mut components = Vec::with_capacity(parameters.len());
    for _ in 0..parameters.len() {
        components.push(embedded_surface(&mut cur)?);
    }
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Compound {
            parameters,
            components,
        },
        cache_fit_tolerance,
    })
}

/// The shared revision-gated surface tail, decoded.
pub(crate) struct RevisionSurfaceTail {
    /// Enum opening the tail, selecting the approximation-cache form.
    pub(crate) enumeration: i64,
    /// Fit tolerance of the solved cache. Carried by form `0` only.
    pub(crate) fit_tolerance: Option<f64>,
    /// Parameter intervals and closure/singularity enums. Carried by form `2`
    /// only.
    pub(crate) parameterization: Option<cadmpeg_ir::geometry::RevisionSurfaceParameterization>,
    /// Six ordered discontinuity arrays.
    pub(crate) discontinuities: [Vec<f64>; 6],
    /// Boolean terminating the tail.
    pub(crate) tail_flag: bool,
}

/// Parse the shared revision-gated surface tail (GC-08). It opens with an enum
/// selecting the approximation-cache form: `0` stores the solved NURBS surface
/// followed by its fit tolerance; `2` stores no cache and no fit tolerance, and
/// instead stores the U parameter interval and the V parameter interval in the
/// optional bool-gated encoding followed by four enums holding U closure, V
/// closure, U singularity, and V singularity. Both forms then continue into six
/// counted discontinuity arrays and one boolean. Other values have no defined
/// grammar; they fail so the containing record is retained verbatim through the
/// native-preservation path rather than misparsed.
pub(crate) fn revision_surface_tail(cur: &mut Cur<'_>) -> Option<RevisionSurfaceTail> {
    let enumeration = cur.take_enum()?;
    let (fit_tolerance, parameterization) = match enumeration {
        0 => {
            let (_, cache_end) = surface_block(cur.toks(), cur.pos())?;
            cur.set_pos(cache_end);
            (Some(cur.take_f64()? * LEN_TO_MM), None)
        }
        2 => {
            let u_interval = [
                cur.take_optional_range_value()?,
                cur.take_optional_range_value()?,
            ];
            let v_interval = [
                cur.take_optional_range_value()?,
                cur.take_optional_range_value()?,
            ];
            (
                None,
                Some(cadmpeg_ir::geometry::RevisionSurfaceParameterization {
                    u_interval,
                    v_interval,
                    u_closure: cur.take_enum()?,
                    v_closure: cur.take_enum()?,
                    u_singularity: cur.take_enum()?,
                    v_singularity: cur.take_enum()?,
                }),
            )
        }
        _ => return None,
    };
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let tail_flag = cur.take_bool()?;
    Some(RevisionSurfaceTail {
        enumeration,
        fit_tolerance,
        parameterization,
        discontinuities,
        tail_flag,
    })
}

fn off_spl_sur(
    toks: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    let names = ["off_spl_sur", "offsur"];
    let (start, name) = toks::find_owned_subtype_marker(toks, &names)?;
    let modern = name == "off_spl_sur";
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    if matches!(cur.peek(), Some(Token::Long(_))) {
        // Only the modern name stores the revision-gated layout.
        modern.then_some(())?;
        let table = resolver?;
        let revision = cur.take_long()?;
        (revision > 0).then_some(())?;
        let (support, support_bounds) = optional_embedded_surface_with_bounds(&mut cur, table)?;
        let support = support?;
        let distance = cur.take_f64()? * LEN_TO_MM;
        // One four-boolean carrier run: the leading pair carrying record-level
        // progenitor orientation state, then the two-boolean ASM extension
        // prefix. The first boolean repeats the support reference's sense flag
        // and orients the offset displacement; the second leaves the point set
        // unchanged. This run occupies the byte positions the pre-revision
        // layout reads as the U/V sense enums but shares no grammar with them,
        // so it travels in the revision form rather than those IR slots.
        let mut flags = Vec::with_capacity(4);
        for _ in 0..4 {
            flags.push(cur.take_bool()?);
        }
        let RevisionSurfaceTail {
            enumeration: tail_enum,
            fit_tolerance,
            parameterization,
            discontinuities,
            tail_flag,
        } = revision_surface_tail(&mut cur)?;
        return Some(DecodedProceduralSurface {
            definition: DecodedProceduralSurfaceDefinition::Offset {
                support,
                distance,
                u_sense: None,
                v_sense: None,
                extension_flags: Vec::new(),
                revision_form: Some(cadmpeg_ir::geometry::RevisionSurfaceForm {
                    revision,
                    support_bounds,
                    reference_endpoints: [None; 2],
                    second_endpoints: [None; 2],
                    flags,
                    tail_enum,
                    tail_parameterization: parameterization,
                    discontinuities,
                    tail_flag,
                    trailing_flags: Vec::new(),
                }),
            },
            cache_fit_tolerance: fit_tolerance,
        });
    }
    let support = embedded_surface(&mut cur)?;
    let distance = cur.take_f64()? * LEN_TO_MM;
    let u_sense = Some(cur.take_enum()?);
    let v_sense = Some(cur.take_enum()?);
    let mut extension_flags = Vec::new();
    if modern {
        let first = cur.take_bool()?;
        extension_flags.push(first);
        if first {
            extension_flags.push(cur.take_bool()?);
            if matches!(cur.peek(), Some(Token::True | Token::False)) {
                extension_flags.push(cur.take_bool()?);
            }
        }
    }
    let (_, cache_end) = toks::marker_positions(span)
        .into_iter()
        .filter_map(|at| surface_block(span, at))
        .next_back()?;
    let cache_fit_tolerance = match span.get(cache_end) {
        Some(Token::Double(value)) => Some(*value * LEN_TO_MM),
        _ => None,
    };
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

fn rot_spl_sur(
    toks: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    let names = ["rot_spl_sur", "rotsur"];
    let (start, name) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    if matches!(cur.peek(), Some(Token::Long(_))) {
        // Revision-gated layout: revision integer, profile curve with two
        // optional endpoints, axis origin and direction, shared tail. Only the
        // modern name stores it.
        (name == "rot_spl_sur").then_some(())?;
        let revision = cur.take_long()?;
        (revision > 0).then_some(())?;
        let table = resolver?;
        let profile = embedded_base_curve_resolving_refs(&mut cur, table)?;
        let profile_endpoints = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        let origin = cur.take_position()?;
        let axis = cur.take_vector3()?;
        let RevisionSurfaceTail {
            enumeration: tail_enum,
            fit_tolerance,
            parameterization,
            discontinuities,
            tail_flag,
        } = revision_surface_tail(&mut cur)?;
        let cache = toks::marker_positions(span)
            .into_iter()
            .filter_map(|at| surface_block(span, at).map(|(surface, _)| surface))
            .next_back()?;
        let angular_interval = [*cache.v_knots.first()?, *cache.v_knots.last()?];
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
                    tail_parameterization: parameterization,
                    discontinuities,
                    tail_flag,
                    trailing_flags: Vec::new(),
                }),
            },
            cache_fit_tolerance: fit_tolerance,
        });
    }
    let (directrix, directrix_end) = toks::marker_positions(span)
        .into_iter()
        .find_map(|at| curve_block(span, at))?;
    let parameter_interval = [*directrix.knots.first()?, *directrix.knots.last()?];
    let mut cur = Cur::at(span, directrix_end);
    let origin = cur.take_position()?;
    let axis_origin = Point3::new(
        origin[0] * LEN_TO_MM,
        origin[1] * LEN_TO_MM,
        origin[2] * LEN_TO_MM,
    );
    let axis = cur.take_vector3()?;
    let axis_direction = normalized(axis)?;
    let (cache, cache_end) = toks::marker_positions(span)
        .into_iter()
        .filter_map(|at| surface_block(span, at))
        .next_back()?;
    let angular_interval = [*cache.v_knots.first()?, *cache.v_knots.last()?];
    let cache_fit_tolerance = match span.get(cache_end) {
        Some(Token::Double(value)) => Some(*value * LEN_TO_MM),
        _ => None,
    };
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Revolution {
            directrix: CurveGeometry::Nurbs(directrix),
            axis_origin,
            axis_direction,
            angular_interval,
            parameter_interval,
            revision_form: None,
        },
        cache_fit_tolerance,
    })
}

fn sum_spl_sur(
    toks: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    let names = ["sum_spl_sur", "sumsur"];
    let (start, name) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    if matches!(cur.peek(), Some(Token::Long(_))) {
        // Revision-gated layout: revision integer, two curves each with two
        // optional endpoints, model-space origin, shared tail. Only the modern
        // name stores it.
        (name == "sum_spl_sur").then_some(())?;
        let revision = cur.take_long()?;
        (revision > 0).then_some(())?;
        let table = resolver?;
        let first = embedded_base_curve_resolving_refs(&mut cur, table)?;
        let first_endpoints = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        let second = embedded_base_curve_resolving_refs(&mut cur, table)?;
        let second_endpoints = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        let origin = cur.take_position()?;
        let RevisionSurfaceTail {
            enumeration: tail_enum,
            fit_tolerance,
            parameterization,
            discontinuities,
            tail_flag,
        } = revision_surface_tail(&mut cur)?;
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
                    tail_parameterization: parameterization,
                    discontinuities,
                    tail_flag,
                    trailing_flags: Vec::new(),
                }),
            },
            cache_fit_tolerance: fit_tolerance,
        });
    }
    let mut decoded_curves = toks::marker_positions(span)
        .into_iter()
        .filter_map(|at| curve_block(span, at));
    let first = decoded_curves.next()?;
    let (second, second_end) = decoded_curves.next()?;
    let mut cur = Cur::at(span, second_end);
    let origin = cur.take_position()?;
    let basepoint = Vector3::new(
        origin[0] * LEN_TO_MM,
        origin[1] * LEN_TO_MM,
        origin[2] * LEN_TO_MM,
    );
    let cache = toks::marker_positions(span)
        .into_iter()
        .filter_map(|at| surface_block(span, at))
        .next_back();
    let cache_fit_tolerance = cache.and_then(|(_, cache_end)| match span.get(cache_end) {
        Some(Token::Double(value)) => Some(*value * LEN_TO_MM),
        _ => None,
    });
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Sum {
            first: CurveGeometry::Nurbs(first.0),
            second: CurveGeometry::Nurbs(second),
            basepoint,
            revision_form: None,
        },
        cache_fit_tolerance,
    })
}

fn ruled_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    let names = ["rule_sur", "rulesur"];
    let (start, _) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut curves = toks::marker_positions(span)
        .into_iter()
        .filter_map(|at| curve_block(span, at).map(|(curve, _)| curve));
    let first = curves.next()?;
    let second = curves.next()?;
    let cache = toks::marker_positions(span)
        .into_iter()
        .filter_map(|at| surface_block(span, at))
        .next_back();
    let cache_fit_tolerance = cache.and_then(|(_, cache_end)| match span.get(cache_end) {
        Some(Token::Double(value)) => Some(*value * LEN_TO_MM),
        _ => None,
    });
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Ruled { first, second },
        cache_fit_tolerance,
    })
}

fn exact_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    let names = ["exact_spl_sur", "exactsur"];
    let (start, name) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    if matches!(cur.peek(), Some(Token::Long(_))) {
        // Revision-gated layout: revision integer, shared tail, four optional
        // parameter values, and the extension as an enum. Only the modern name
        // stores it.
        (name == "exact_spl_sur").then_some(())?;
        let revision = cur.take_long()?;
        (revision > 0).then_some(())?;
        let RevisionSurfaceTail {
            enumeration: tail_enum,
            fit_tolerance,
            parameterization,
            discontinuities,
            tail_flag,
        } = revision_surface_tail(&mut cur)?;
        // The two unextended parameter intervals, each an ordered [lo, hi] pair
        // of optional bounds. This subtype serializes them U-then-V; the loft
        // wrap ranges sharing `RevisionRanges` serialize V-then-U, so the order
        // is per subtype and is not a property of that type. Stored positionally
        // here and labelled only by the specification.
        let unextended_ranges = [
            [
                cur.take_optional_range_value()?,
                cur.take_optional_range_value()?,
            ],
            [
                cur.take_optional_range_value()?,
                cur.take_optional_range_value()?,
            ],
        ];
        let extension = cur.take_enum()?;
        return Some(DecodedProceduralSurface {
            definition: DecodedProceduralSurfaceDefinition::Exact {
                parameters: cadmpeg_ir::geometry::SplineSurfaceParameters::RevisionRanges {
                    intervals: unextended_ranges,
                },
                extension,
                revision_form: Some(cadmpeg_ir::geometry::RevisionSurfaceForm {
                    revision,
                    support_bounds: [None; 4],
                    reference_endpoints: [None; 2],
                    second_endpoints: [None; 2],
                    flags: Vec::new(),
                    tail_enum,
                    tail_parameterization: parameterization,
                    discontinuities,
                    tail_flag,
                    trailing_flags: Vec::new(),
                }),
            },
            cache_fit_tolerance: fit_tolerance,
        });
    }
    let (_, cache_end) = toks::marker_positions(span)
        .into_iter()
        .filter_map(|at| surface_block(span, at))
        .next_back()?;
    let cache_fit_tolerance = match span.get(cache_end) {
        Some(Token::Double(value)) => Some(*value * LEN_TO_MM),
        _ => None,
    };
    cur.set_pos(cache_end + usize::from(cache_fit_tolerance.is_some()));
    let parameter_ranges = [
        [cur.take_range_value()?, cur.take_range_value()?],
        [cur.take_range_value()?, cur.take_range_value()?],
    ];
    let extension = cur.take_long()?;
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

fn t_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    use cadmpeg_ir::geometry::{TSplineSubtransform, TSplineSurfaceConstruction};

    let (start, _) = toks::find_owned_subtype_marker(toks, &["t_spl_sur"])?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let (
        cache_fit_tolerance,
        discontinuities,
        discontinuity_flag,
        parameter_ranges,
        type_code,
        revision_form,
    );
    if matches!(cur.peek(), Some(Token::Long(_))) {
        // Revision-gated layout: revision integer, shared tail, four optional
        // parameter values, the type code as an enum, then the nested
        // subtransform scope and trailing integer.
        let revision = cur.take_long()?;
        (revision > 0).then_some(())?;
        let RevisionSurfaceTail {
            enumeration: tail_enum,
            fit_tolerance,
            parameterization,
            discontinuities: tail_discontinuities,
            tail_flag,
        } = revision_surface_tail(&mut cur)?;
        let mut bounds = [None; 4];
        for bound in &mut bounds {
            *bound = cur.take_optional_range_value()?;
        }
        cache_fit_tolerance = fit_tolerance;
        discontinuities = tail_discontinuities.clone();
        discontinuity_flag = tail_flag;
        parameter_ranges = [
            [bounds[0].unwrap_or(0.0), bounds[1].unwrap_or(0.0)],
            [bounds[2].unwrap_or(0.0), bounds[3].unwrap_or(0.0)],
        ];
        type_code = cur.take_enum()?;
        revision_form = Some(cadmpeg_ir::geometry::RevisionSurfaceForm {
            revision,
            support_bounds: bounds,
            reference_endpoints: [None; 2],
            second_endpoints: [None; 2],
            flags: Vec::new(),
            tail_enum,
            tail_parameterization: parameterization,
            discontinuities: tail_discontinuities,
            tail_flag,
            trailing_flags: Vec::new(),
        });
    } else {
        let (_, cache_end) = surface_block(span, cur.pos())?;
        cur.set_pos(cache_end);
        cache_fit_tolerance = Some(cur.take_f64()? * LEN_TO_MM);
        discontinuities = [
            cur.take_float_array()?,
            cur.take_float_array()?,
            cur.take_float_array()?,
            cur.take_float_array()?,
            cur.take_float_array()?,
            cur.take_float_array()?,
        ];
        discontinuity_flag = cur.take_bool()?;
        parameter_ranges = [
            [cur.take_f64()? * LEN_TO_MM, cur.take_f64()? * LEN_TO_MM],
            [cur.take_f64()? * LEN_TO_MM, cur.take_f64()? * LEN_TO_MM],
        ];
        type_code = cur.take_long()?;
        revision_form = None;
    }
    if !matches!(cur.peek(), Some(Token::SubtypeOpen)) {
        return None;
    }
    cur.bump();
    let source_kind = cur.take_ident()?;
    let subtransform = match source_kind {
        "t_spl_subtrans_object" => {
            let program = cur.take_str()?.to_string();
            let separator = if matches!(cur.peek(), Some(Token::Str(_))) {
                None
            } else {
                Some(cur.take_bool()?)
            };
            let values = cur.take_str()?.to_string();
            TSplineSubtransform::Inline {
                program,
                separator,
                values,
            }
        }
        "ref" => TSplineSubtransform::Reference {
            index: cur.take_long()?,
            resolved: None,
        },
        _ => return None,
    };
    if !matches!(cur.peek(), Some(Token::SubtypeClose)) {
        return None;
    }
    cur.bump();
    let trailing_value = cur.take_long()?;
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

fn deformable_surface_frame(
    cur: &mut Cur<'_>,
) -> Option<cadmpeg_ir::geometry::DeformableSurfaceFrame> {
    let mut leading_vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
    for vector in &mut leading_vectors {
        let value = cur.take_vector3()?;
        *vector = Vector3::new(value[0], value[1], value[2]);
    }
    let leading_parameter = cur.take_f64()?;
    let leading_flags = [cur.take_bool()?, cur.take_bool()?, cur.take_bool()?];
    let mut secondary_vectors = [Vector3::new(0.0, 0.0, 0.0); 3];
    for vector in &mut secondary_vectors {
        let value = cur.take_vector3()?;
        *vector = Vector3::new(value[0], value[1], value[2]);
    }
    let secondary_parameter = cur.take_f64()?;
    let secondary_flags = [cur.take_bool()?, cur.take_bool()?];
    let point = cur.take_position()?;
    let trailing_flags = [
        cur.take_bool()?,
        cur.take_bool()?,
        cur.take_bool()?,
        cur.take_bool()?,
        cur.take_bool()?,
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

fn deformable_vector_frame(
    cur: &mut Cur<'_>,
) -> Option<cadmpeg_ir::geometry::DeformableVectorFrame> {
    let mut vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
    for vector in &mut vectors {
        let value = cur.take_vector3()?;
        *vector = Vector3::new(value[0], value[1], value[2]);
    }
    Some(cadmpeg_ir::geometry::DeformableVectorFrame {
        vectors,
        parameter: cur.take_f64()?,
        flags: [cur.take_bool()?, cur.take_bool()?, cur.take_bool()?],
    })
}

fn defm_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    use cadmpeg_ir::geometry::DeformableSurfaceData;
    let names = ["defm_spl_sur", "defmsur"];
    let (start, _) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let support = embedded_surface(&mut cur)?;
    let mode = cur.take_long()?;
    let data = match mode {
        1 => {
            let frame = Box::new(deformable_surface_frame(&mut cur)?);
            let count = usize::try_from(cur.take_long()?).ok()?;
            let parameter_triples = (0..count)
                .map(|_| Some([cur.take_f64()?, cur.take_f64()?, cur.take_f64()?]))
                .collect::<Option<Vec<_>>>()?;
            EmbeddedDeformableSurfaceData::Resolved(DeformableSurfaceData::Plain {
                frame,
                parameter_triples,
            })
        }
        3 => EmbeddedDeformableSurfaceData::Resolved(DeformableSurfaceData::Guided {
            frame: Box::new(deformable_surface_frame(&mut cur)?),
            selector: cur.take_long()?,
            guide_parameter: cur.take_f64()?,
        }),
        5 => {
            let surface = embedded_surface(&mut cur)?;
            let native_id = cur.take_long()?;
            let flag = cur.take_bool()?;
            let first_parameter = cur.take_f64()?;
            let selector = cur.take_long()?;
            let second_parameter = cur.take_f64()?;
            let (curve, curve_end) = curve_block(span, cur.pos())?;
            cur.set_pos(curve_end);
            let mut vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
            for vector in &mut vectors {
                let value = cur.take_vector3()?;
                *vector = Vector3::new(value[0], value[1], value[2]);
            }
            let frame_parameter = cur.take_f64()?;
            let flags = [cur.take_bool()?, cur.take_bool()?, cur.take_bool()?];
            let count = usize::try_from(cur.take_long()?).ok()?;
            let parameter_triples = (0..count)
                .map(|_| Some([cur.take_f64()?, cur.take_f64()?, cur.take_f64()?]))
                .collect::<Option<Vec<_>>>()?;
            EmbeddedDeformableSurfaceData::SurfaceCurve {
                surface,
                native_id,
                flag,
                first_parameter,
                selector,
                second_parameter,
                curve,
                vectors,
                frame_parameter,
                flags,
                parameter_triples,
            }
        }
        6 => {
            let mut leading_vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
            for vector in &mut leading_vectors {
                let value = cur.take_vector3()?;
                *vector = Vector3::new(value[0], value[1], value[2]);
            }
            let leading_parameter = cur.take_f64()?;
            let leading_flags = [cur.take_bool()?, cur.take_bool()?, cur.take_bool()?];
            let selector = cur.take_long()?;
            let surface = embedded_surface(&mut cur)?;
            let native_id = cur.take_long()?;
            let flag = cur.take_bool()?;
            let first_parameter = cur.take_f64()?;
            let version_value = matches!(cur.peek(), Some(Token::Long(_)))
                .then(|| cur.take_long())
                .flatten();
            let second_parameter = cur.take_f64()?;
            let (curve, curve_end) = curve_block(span, cur.pos())?;
            cur.set_pos(curve_end);
            let frames = Box::new([
                deformable_vector_frame(&mut cur)?,
                deformable_vector_frame(&mut cur)?,
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
                curve,
                frames,
                trailing_value: cur.take_long()?,
            }
        }
        8 => {
            let mut vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
            for vector in &mut vectors {
                let value = cur.take_vector3()?;
                *vector = Vector3::new(value[0], value[1], value[2]);
            }
            EmbeddedDeformableSurfaceData::Resolved(DeformableSurfaceData::Minimal {
                vectors,
                selector: cur.take_long()?,
            })
        }
        _ => return None,
    };
    let (_, cache_end) = surface_block(span, cur.pos())?;
    cur.set_pos(cache_end);
    let cache_fit_tolerance = Some(cur.take_f64()? * LEN_TO_MM);
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let discontinuity_flag = cur.take_bool()?;
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

pub(crate) fn helix_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    use cadmpeg_ir::geometry::{
        HelixPathConstruction, HelixSurfaceConstruction, HelixSurfaceProfile,
    };

    let names = ["helix_spl_circ", "helix_spl_line"];
    let (start, name) = toks::find_owned_subtype_marker(toks, &names)?;
    let circular = name == "helix_spl_circ";
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let current_layout = optional_helix_revision(&mut cur)?;
    let angle_range = [cur.take_range_value()?, cur.take_range_value()?];
    let dimension_scale = if circular { LEN_TO_MM } else { 1.0 };
    let dimension_range = [
        cur.take_range_value()? * dimension_scale,
        cur.take_range_value()? * dimension_scale,
    ];
    let length = circular
        .then(|| cur.take_f64().map(|v| v * LEN_TO_MM))
        .flatten();
    let path_angle_range = [cur.take_range_value()?, cur.take_range_value()?];
    let center = cur.take_position()?;
    let take_frame_vector = |cur: &mut Cur<'_>| {
        if current_layout {
            cur.take_vector3()
        } else {
            cur.take_position()
        }
    };
    let major = take_frame_vector(&mut cur)?;
    let minor = take_frame_vector(&mut cur)?;
    let pitch = take_frame_vector(&mut cur)?;
    let apex_factor = cur.take_f64()?;
    let axis = normalized(cur.take_vector3()?)?;
    for sentinel in ["null_surface", "null_surface", "nullbs", "nullbs"] {
        if cur.take_ident()? != sentinel {
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
            radius: cur.take_f64()? * LEN_TO_MM,
        }
    } else {
        let direction = take_frame_vector(&mut cur)?;
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

fn t_spline_subtransform(span: &[Token]) -> Option<cadmpeg_ir::geometry::TSplineSubtransform> {
    use cadmpeg_ir::geometry::TSplineSubtransform;

    let start = usize::from(matches!(span.first(), Some(Token::SubtypeOpen)));
    let mut cur = Cur::at(span, start);
    match cur.take_ident()? {
        "t_spl_subtrans_object" => {
            let program = cur.take_str()?.to_string();
            let separator = if matches!(cur.peek(), Some(Token::Str(_))) {
                None
            } else {
                Some(cur.take_bool()?)
            };
            let values = cur.take_str()?.to_string();
            Some(TSplineSubtransform::Inline {
                program,
                separator,
                values,
            })
        }
        "ref" => Some(TSplineSubtransform::Reference {
            index: cur.take_long()?,
            resolved: None,
        }),
        _ => None,
    }
}

fn resolve_t_spline_subtransform(
    index: usize,
    table: &SubtypeTable,
    seen: &mut Vec<usize>,
) -> Option<cadmpeg_ir::geometry::TSplineSubtransform> {
    use cadmpeg_ir::geometry::TSplineSubtransform;

    if seen.contains(&index) {
        return None;
    }
    seen.push(index);
    let decoded = t_spline_subtransform(table.span(index)?)?;
    match decoded {
        inline @ TSplineSubtransform::Inline { .. } => Some(inline),
        TSplineSubtransform::Reference { index, .. } => {
            resolve_t_spline_subtransform(usize::try_from(index).ok()?, table, seen)
        }
    }
}

/// Decode a native procedural definition, following nested subtype-table references.
pub(crate) fn procedural_surface_resolving_refs(
    toks: &[Token],
    table: &SubtypeTable,
) -> Option<DecodedProceduralSurface> {
    procedural_resolving_refs(toks, table, &mut Vec::new())
}

fn procedural_resolving_refs(
    toks: &[Token],
    table: &SubtypeTable,
    seen: &mut Vec<usize>,
) -> Option<DecodedProceduralSurface> {
    if let Some(mut decoded) = defm_spl_sur(toks)
        .or_else(|| helix_spl_sur(toks))
        .or_else(|| t_spl_sur(toks))
        .or_else(|| exact_spl_sur(toks))
        .or_else(|| comp_spl_sur(toks))
        .or_else(|| taper_spl_sur(toks, Some(table)))
        .or_else(|| loft_spl_sur(toks, Some(table)))
        .or_else(|| compound_loft_spl_sur(toks, Some(table)))
        .or_else(|| scaled_compound_loft_spl_sur(toks))
        .or_else(|| sub_spl_sur(toks))
        .or_else(|| law_spl_sur(toks))
        .or_else(|| skin_spl_sur(toks))
        .or_else(|| net_spl_sur(toks))
        .or_else(|| sweep_spl_sur(toks, Some(table)))
        .or_else(|| g2_blend_spl_sur(toks, Some(table)))
        .or_else(|| ruled_spl_sur(toks))
        .or_else(|| sum_spl_sur(toks, Some(table)))
        .or_else(|| rot_spl_sur(toks, Some(table)))
        .or_else(|| off_spl_sur(toks, Some(table)))
        .or_else(|| cyl_spl_sur(toks, Some(table)))
        .or_else(|| var_blend_spl_sur(toks, Some(table)))
        .or_else(|| vertex_blend_spl_sur(toks, Some(table)))
        .or_else(|| full_rb_blend_spl_sur(toks, table))
        .or_else(|| rb_blend_spl_sur_fallback(toks))
    {
        if let DecodedProceduralSurfaceDefinition::TSpline(construction) = &mut decoded.definition {
            if let cadmpeg_ir::geometry::TSplineSubtransform::Reference { index, resolved } =
                &mut construction.subtransform
            {
                let inline = resolve_t_spline_subtransform(
                    usize::try_from(*index).ok()?,
                    table,
                    &mut Vec::new(),
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
    // References are followed only when the record carries no construction of
    // its own. A record that owns one but failed to decode it is a refusal:
    // its references are that construction's supports, and decoding one of them
    // would report a support as the record's own surface.
    if toks::owned_subtype_defs(toks)
        .iter()
        .any(|(_, name)| *name != "ref")
    {
        return None;
    }
    for index in toks::subtype_refs(toks) {
        if seen.contains(&index) {
            continue;
        }
        let target = table.span(index)?;
        seen.push(index);
        if let Some(decoded) = procedural_resolving_refs(target, table, seen) {
            return Some(decoded);
        }
    }
    None
}

#[cfg(test)]
mod tail_selector_tests {
    use super::*;

    /// A four-byte enum field.
    fn push_enum(span: &mut Vec<u8>, value: i32) {
        span.push(0x15);
        span.extend_from_slice(&value.to_le_bytes());
    }

    /// A four-byte counted float array of `count` zeros.
    fn push_float_array(span: &mut Vec<u8>, count: i32) {
        span.push(0x04);
        span.extend_from_slice(&count.to_le_bytes());
        for _ in 0..count {
            span.push(0x06);
            span.extend_from_slice(&0.0f64.to_le_bytes());
        }
    }

    /// A shared revision-gated surface tail whose opening enum selects a
    /// cache form with no defined grammar. The helper must reject it so the
    /// containing record is retained verbatim rather than misparsed.
    #[test]
    fn undefined_tail_form_is_rejected_for_verbatim_retention() {
        // Enum with value 1, followed by a value that could otherwise open a
        // solved cache block's fields.
        let mut span = Vec::new();
        push_enum(&mut span, 1);
        span.push(0x06);
        span.extend_from_slice(&0.0f64.to_le_bytes());
        let toks = toks::lex_test_span(&span, 4);
        let mut cur = Cur::at(&toks, 0);
        assert!(revision_surface_tail(&mut cur).is_none());
    }

    /// Tail form `2` stores no cache and no fit tolerance: the U parameter
    /// interval, the V parameter interval, then U closure, V closure, U
    /// singularity, and V singularity.
    #[test]
    fn parameterized_tail_form_decodes_intervals_then_closure_enums() {
        let mut span = Vec::new();
        push_enum(&mut span, 2);
        // U interval: present lower bound, absent upper bound.
        span.push(0x0a);
        span.push(0x06);
        span.extend_from_slice(&0.25f64.to_le_bytes());
        span.push(0x0b);
        // V interval: both bounds present.
        for value in [(-1.5f64), 3.5] {
            span.push(0x0a);
            span.push(0x06);
            span.extend_from_slice(&value.to_le_bytes());
        }
        for value in [1, 0, 2, 3] {
            push_enum(&mut span, value);
        }
        push_float_array(&mut span, 1);
        for _ in 0..5 {
            push_float_array(&mut span, 0);
        }
        span.push(0x0b);

        let toks = toks::lex_test_span(&span, 4);
        let mut cur = Cur::at(&toks, 0);
        let tail = revision_surface_tail(&mut cur).expect("parameterized tail");
        assert_eq!(cur.pos(), toks.len());
        assert_eq!(tail.enumeration, 2);
        assert_eq!(tail.fit_tolerance, None);
        let parameterization = tail.parameterization.expect("parameterization");
        assert_eq!(parameterization.u_interval, [Some(0.25), None]);
        assert_eq!(parameterization.v_interval, [Some(-1.5), Some(3.5)]);
        assert_eq!(parameterization.u_closure, 1);
        assert_eq!(parameterization.v_closure, 0);
        assert_eq!(parameterization.u_singularity, 2);
        assert_eq!(parameterization.v_singularity, 3);
        assert_eq!(tail.discontinuities[0], [0.0]);
        assert!(tail.discontinuities[1..].iter().all(Vec::is_empty));
        assert!(!tail.tail_flag);
    }
}
