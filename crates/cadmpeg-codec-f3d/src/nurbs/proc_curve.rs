// SPDX-License-Identifier: Apache-2.0
//! Procedural curve embedded types, decoders, resolving-ref walkers, and writer-facing patch layouts.

use crate::nurbs::blend::{decode_rolling_ball_side, decode_surface_ranges};
use crate::nurbs::core::{
    decode_curve_block, decode_curve_cache_resolving_refs, decode_surface_block,
    decode_surface_cache_resolving_refs,
};
use crate::nurbs::pcurve::{decode_pcurve_block_with_end, NurbsPcurve};
use crate::nurbs::proc_surface::{
    decode_law_formula, decode_nullable_embedded_pcurve, ellipse_to_nurbs, EmbeddedLawFormula,
};
use crate::nurbs::reader::{
    marker_positions, normalized, take_bool, take_double_payload, take_f64, take_float_array,
    take_float_array_payloads, take_native_ident, take_native_string, take_native_vec3,
    take_optional_range_value, take_range_value, take_tagged_int, Nullable, INT_WIDTHS, LEN_TO_MM,
};
use crate::nurbs::subtypes::{
    find_intcurve_subtype, find_subtype_marker, first_construction_subtype, subtype_refs,
    subtype_span, SubtypeTables,
};
use cadmpeg_ir::geometry::{NurbsCurve, SurfaceGeometry};
use cadmpeg_ir::le::{f64_at as read_f64, int_at as read_int};
use cadmpeg_ir::math::{Point3, Vector3};

/// Source curve and tail fields decoded from an `offset_int_cur` construction.
pub(crate) type VectorOffsetDefinition = (NurbsCurve, [f64; 2], Vector3, [String; 2], [i64; 2]);

/// Parent curve and retained range decoded from a `subset_int_cur` construction.
pub(crate) type SubsetDefinition = (NurbsCurve, [f64; 2]);

/// Parameter arrays and child curves decoded from a `comp_int_cur` construction.
pub(crate) type CompoundDefinition = (Vec<f64>, Vec<f64>, Vec<NurbsCurve>);

/// Embedded freeform support carriers and tail fields of an `off_int_cur`.
pub(crate) struct EmbeddedTwoSidedOffset {
    /// Two ordered embedded support surfaces.
    pub(crate) surfaces: [Option<SurfaceGeometry>; 2],
    /// Two ordered embedded NURBS parameter curves.
    pub(crate) pcurves: [Option<NurbsPcurve>; 2],
    /// Shared native parameter interval.
    pub(crate) parameter_range: [f64; 2],
    /// Three discontinuity arrays.
    pub(crate) discontinuities: [Vec<f64>; 3],
    pub(crate) discontinuity_flag: bool,
    /// Signed side offsets in document length units.
    pub(crate) offsets: [f64; 2],
}

/// Embedded support carriers and shared fields of an `int_int_cur`.
pub(crate) struct EmbeddedIntersection {
    pub(crate) surfaces: [Option<SurfaceGeometry>; 2],
    pub(crate) pcurves: [Option<NurbsPcurve>; 2],
    pub(crate) parameter_range: [f64; 2],
    pub(crate) discontinuities: [Vec<f64>; 3],
}

/// Three ordered support carriers and selector of an `sss_int_cur`.
pub(crate) struct EmbeddedThreeSurfaceIntersection {
    pub(crate) surfaces: [SurfaceGeometry; 3],
    pub(crate) pcurves: [NurbsPcurve; 3],
    pub(crate) parameter_range: [f64; 2],
    pub(crate) discontinuities: [Vec<f64>; 3],
    pub(crate) selector: i64,
}

/// Embedded support context, source curve, and tail of a `proj_int_cur`.
pub(crate) struct EmbeddedProjection {
    pub(crate) surfaces: [SurfaceGeometry; 2],
    pub(crate) pcurves: [NurbsPcurve; 2],
    pub(crate) parameter_range: [f64; 2],
    pub(crate) discontinuities: [Vec<f64>; 3],
    pub(crate) discontinuity_flag: bool,
    pub(crate) source: NurbsCurve,
    pub(crate) tail: cadmpeg_ir::geometry::ProjectionTail,
}

/// Shared context and tail fields of a silhouette intcurve.
pub(crate) struct EmbeddedSilhouette {
    pub(crate) context: EmbeddedIntersection,
    pub(crate) silhouette: cadmpeg_ir::geometry::SilhouetteKind,
    pub(crate) cast_surface: SurfaceGeometry,
    pub(crate) light_direction: Vector3,
}

/// Shared context and tail fields of an `off_surf_int_cur`.
pub(crate) struct EmbeddedSurfaceOffset {
    pub(crate) context: EmbeddedIntersection,
    pub(crate) discontinuity_flag: bool,
    pub(crate) base_u_range: [f64; 2],
    pub(crate) base_v_range: [f64; 2],
    pub(crate) base: NurbsCurve,
    pub(crate) base_range: [f64; 2],
    pub(crate) base_endpoints: [Option<f64>; 2],
    pub(crate) cache_first: Option<cadmpeg_ir::geometry::CacheFirstCurveForm>,
    pub(crate) distance: f64,
    pub(crate) shift: f64,
    pub(crate) scale: f64,
}

/// Spring support context, conditional null-carrier ranges, and direction enum.
pub(crate) struct EmbeddedSpring {
    pub(crate) surfaces: [Option<SurfaceGeometry>; 2],
    pub(crate) pcurves: [Option<NurbsPcurve>; 2],
    pub(crate) surface_parameter_ranges: [Option<[[f64; 2]; 2]>; 2],
    pub(crate) first_pcurve_parameter_range: Option<[f64; 2]>,
    pub(crate) parameter_range: [f64; 2],
    pub(crate) discontinuities: [Vec<f64>; 3],
    pub(crate) discontinuity_flag: bool,
    pub(crate) cache_first: Option<cadmpeg_ir::geometry::CacheFirstCurveForm>,
    pub(crate) direction: i64,
}

pub(crate) struct EmbeddedLawCurve {
    pub(crate) context: EmbeddedIntersection,
    pub(crate) extension: i64,
    pub(crate) primary: EmbeddedLawFormula,
    pub(crate) additional: Vec<EmbeddedLawFormula>,
}

pub(crate) enum EmbeddedDeformableData {
    VectorField {
        vectors: [Vector3; 4],
        parameter_pairs: Vec<[f64; 2]>,
    },
    Surface(SurfaceGeometry),
}

pub(crate) struct EmbeddedDeformable {
    pub(crate) extension: i64,
    pub(crate) bend: NurbsCurve,
    pub(crate) data: EmbeddedDeformableData,
}

/// A procedural curve cache together with its native subtype and fit contract.
pub struct DecodedProceduralCurve {
    /// The cached B-spline curve (control points scaled centimetre→
    /// millimetre; knots and weights unscaled).
    pub curve: NurbsCurve,
    /// The `intcurve` subtype record name (`exact_int_cur`, `off_int_cur`,
    /// `proj_int_cur`, `int_int_cur`, `helix_int_cur`, `sss_int_cur`, ...).
    pub native_kind: String,
    /// Neutral construction fields decoded from the subtype tail.
    pub definition: Option<cadmpeg_ir::geometry::ProceduralCurveDefinition>,
    /// Source curve and tail fields of an `offset_int_cur` construction.
    pub vector_offset: Option<VectorOffsetDefinition>,
    /// Parent curve and retained range of a `subset_int_cur` construction.
    pub subset: Option<SubsetDefinition>,
    /// Parameter arrays and ordered child curves of a `comp_int_cur` construction.
    pub compound: Option<CompoundDefinition>,
    /// Non-null embedded NURBS support carriers of an `off_int_cur`.
    pub(crate) embedded_two_sided_offset: Option<EmbeddedTwoSidedOffset>,
    /// Embedded support context of an `int_int_cur`.
    pub(crate) embedded_intersection: Option<(EmbeddedIntersection, bool)>,
    /// Three embedded support pairs of an `sss_int_cur`.
    pub(crate) embedded_three_surface_intersection: Option<EmbeddedThreeSurfaceIntersection>,
    /// Prefix-only surface-curve family and support context.
    pub(crate) embedded_surface_curve: Option<(
        cadmpeg_ir::geometry::SurfaceCurveFamily,
        EmbeddedIntersection,
        Option<cadmpeg_ir::geometry::SurfaceCurveTail>,
    )>,
    /// Embedded silhouette support, cast surface, and light vector.
    pub(crate) embedded_silhouette: Option<EmbeddedSilhouette>,
    /// Embedded support context and base curve of an `off_surf_int_cur`.
    pub(crate) embedded_surface_offset: Option<EmbeddedSurfaceOffset>,
    /// Modern non-null `spring_int_cur` construction.
    pub(crate) embedded_spring: Option<EmbeddedSpring>,
    /// Embedded bend curve and discriminator payload of a `defm_int_cur`.
    pub(crate) embedded_deformable: Option<EmbeddedDeformable>,
    /// Embedded support context and source of a `proj_int_cur`.
    pub(crate) embedded_projection: Option<EmbeddedProjection>,
    /// Embedded support context and recursive formulas of a `law_int_cur`.
    pub(crate) embedded_law: Option<EmbeddedLawCurve>,
    /// `surface_fit_tolerance` of the cached B-spline block, if present
    /// ([spec §7.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#75-nubsnurbs-blocks-b-spline-curves-and-surfaces)).
    pub cache_fit_tolerance: Option<f64>,
}

/// Decode a procedural 3D curve cache while following subtype-table references.
pub fn decode_procedural_curve_resolving_refs(
    record_bytes: &[u8],
    active_bytes: &[u8],
    tables: &SubtypeTables,
) -> Option<DecodedProceduralCurve> {
    INT_WIDTHS.into_iter().find_map(|int_width| {
        decode_procedural_curve_recursive(
            record_bytes,
            active_bytes,
            tables,
            &mut Vec::new(),
            int_width,
        )
    })
}

/// Decode an exact procedural curve construction that has no solved cache.
pub(crate) fn decode_cacheless_procedural_curve_resolving_refs(
    record_bytes: &[u8],
    active_bytes: &[u8],
    tables: &SubtypeTables,
) -> Option<(String, cadmpeg_ir::geometry::ProceduralCurveDefinition)> {
    INT_WIDTHS.into_iter().find_map(|int_width| {
        decode_cacheless_procedural_curve_recursive(
            record_bytes,
            active_bytes,
            tables,
            &mut Vec::new(),
            int_width,
        )
    })
}

fn decode_cacheless_procedural_curve_recursive(
    bytes: &[u8],
    active_bytes: &[u8],
    tables: &SubtypeTables,
    seen: &mut Vec<usize>,
    int_width: usize,
) -> Option<(String, cadmpeg_ir::geometry::ProceduralCurveDefinition)> {
    if let Some(definition) = decode_helix_definition(bytes, int_width) {
        return Some(("helix_int_cur".into(), definition));
    }
    let table = tables.for_width(int_width);
    for index in subtype_refs(bytes, int_width) {
        if seen.contains(&index) {
            continue;
        }
        seen.push(index);
        let target = *table.get(index)?;
        if let Some(decoded) = decode_cacheless_procedural_curve_recursive(
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

fn decode_procedural_curve_recursive(
    bytes: &[u8],
    active_bytes: &[u8],
    tables: &SubtypeTables,
    seen: &mut Vec<usize>,
    int_width: usize,
) -> Option<DecodedProceduralCurve> {
    let vector_offset = decode_vector_offset_definition(bytes, int_width);
    let subset = decode_subset_definition(bytes, int_width);
    let compound = decode_compound_definition(bytes, int_width);
    // Wrapper constructions serialize their source curves before the record's
    // own cache, so the cache is the last decodable curve block. Every other
    // intcurve opens with its cache — the first block, followed by the fit
    // tolerance; later blocks belong to nested construction machinery
    // (support surfaces, blend spines, progenitors) and are not the carrier.
    let positions = marker_positions(bytes);
    let solved = if vector_offset.is_some() || subset.is_some() || compound.is_some() {
        positions
            .into_iter()
            .rev()
            .find_map(|position| decode_curve_block(bytes, position, int_width))
    } else {
        positions
            .into_iter()
            .find_map(|position| decode_curve_block(bytes, position, int_width))
    };
    if let Some(decoded) = solved {
        let cache_fit_tolerance = (bytes.get(decoded.end) == Some(&0x06))
            .then(|| read_f64(bytes, decoded.end + 1).map(|value| value * LEN_TO_MM))
            .flatten();
        let native_kind =
            first_construction_subtype(bytes).unwrap_or_else(|| "intcurve".to_string());
        let definition = if native_kind == "exact_int_cur" {
            Some(cadmpeg_ir::geometry::ProceduralCurveDefinition::Exact)
        } else {
            decode_helix_definition(bytes, int_width)
                .or_else(|| decode_two_sided_offset(bytes, int_width))
        };
        let embedded_intersection =
            decode_embedded_intersection(bytes, int_width, &decoded.curve, active_bytes, tables);
        let embedded_surface_curve =
            decode_embedded_surface_curve(bytes, int_width, &decoded.curve, active_bytes, tables);
        let embedded_surface_offset =
            decode_embedded_surface_offset(bytes, int_width, &decoded.curve, active_bytes, tables);
        let embedded_spring =
            decode_embedded_spring(bytes, int_width, &decoded.curve, active_bytes, tables);
        return Some(DecodedProceduralCurve {
            curve: decoded.curve,
            native_kind,
            definition,
            vector_offset,
            subset,
            compound,
            embedded_two_sided_offset: decode_embedded_two_sided_offset(bytes, int_width),
            embedded_intersection,
            embedded_three_surface_intersection: decode_embedded_three_surface_intersection(
                bytes, int_width,
            ),
            embedded_surface_curve,
            embedded_silhouette: decode_embedded_silhouette(bytes, int_width),
            embedded_surface_offset,
            embedded_spring,
            embedded_deformable: decode_embedded_deformable(bytes, int_width),
            embedded_projection: decode_embedded_projection(bytes, int_width),
            embedded_law: decode_embedded_law_curve(bytes, int_width),
            cache_fit_tolerance,
        });
    }
    let table = tables.for_width(int_width);
    for index in subtype_refs(bytes, int_width) {
        if seen.contains(&index) {
            continue;
        }
        let target = *table.get(index)?;
        seen.push(index);
        if let Some(decoded) = decode_procedural_curve_recursive(
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

fn decode_embedded_deformable(bytes: &[u8], int_width: usize) -> Option<EmbeddedDeformable> {
    let name = b"defm_int_cur";
    let marker = find_subtype_marker(bytes, &[name]).map(|(marker, _)| marker)?;
    let mut position = marker + name.len() + 3;
    let extension = take_tagged_int(bytes, &mut position, 0x04, int_width)?;
    let bend = decode_curve_block(bytes, position, int_width)?;
    position = bend.end;
    let mode = take_tagged_int(bytes, &mut position, 0x04, int_width)?;
    let data = match mode {
        8 => {
            let mut vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
            for vector in &mut vectors {
                let value = take_native_vec3(bytes, &mut position, 0x14)?;
                *vector = Vector3::new(value[0], value[1], value[2]);
            }
            let count = take_tagged_int(bytes, &mut position, 0x04, int_width)?;
            let count = usize::try_from(count).ok()?;
            let mut parameter_pairs = Vec::with_capacity(count);
            for _ in 0..count {
                parameter_pairs.push([
                    take_f64(bytes, &mut position)?,
                    take_f64(bytes, &mut position)?,
                ]);
            }
            EmbeddedDeformableData::VectorField {
                vectors,
                parameter_pairs,
            }
        }
        5 => EmbeddedDeformableData::Surface(decode_embedded_surface(
            bytes,
            &mut position,
            int_width,
        )?),
        _ => return None,
    };
    Some(EmbeddedDeformable {
        extension,
        bend: bend.curve,
        data,
    })
}

fn decode_embedded_law_curve(bytes: &[u8], int_width: usize) -> Option<EmbeddedLawCurve> {
    let (marker, name_len) = find_intcurve_subtype(bytes, b"law_int_cur")?;
    let mut position = marker + name_len + 3;
    let solved = decode_curve_block(bytes, position, int_width)?;
    position = solved.end;
    take_f64(bytes, &mut position)?;
    let surfaces = [
        decode_embedded_surface(bytes, &mut position, int_width)?,
        decode_embedded_surface(bytes, &mut position, int_width)?,
    ];
    let (first_pcurve, first_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = first_end;
    let (second_pcurve, second_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = second_end;
    let parameter_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
    ];
    let extension = take_tagged_int(bytes, &mut position, 0x04, int_width)?;
    let primary = decode_law_formula(bytes, &mut position, int_width)?;
    let count = usize::try_from(take_tagged_int(bytes, &mut position, 0x04, int_width)?).ok()?;
    if count > 100_000 {
        return None;
    }
    let additional = (0..count)
        .map(|_| decode_law_formula(bytes, &mut position, int_width))
        .collect::<Option<Vec<_>>>()?;
    Some(EmbeddedLawCurve {
        context: EmbeddedIntersection {
            surfaces: surfaces.map(Some),
            pcurves: [Some(first_pcurve), Some(second_pcurve)],
            parameter_range,
            discontinuities,
        },
        extension,
        primary,
        additional,
    })
}

fn decode_embedded_spring(
    bytes: &[u8],
    int_width: usize,
    solved: &NurbsCurve,
    active_bytes: &[u8],
    tables: &SubtypeTables,
) -> Option<EmbeddedSpring> {
    let name = b"spring_int_cur";
    let (marker, name_len) = find_intcurve_subtype(bytes, name)?;
    let mut position = marker + name_len + 3;
    if bytes.get(position) == Some(&0x04) {
        let context = decode_cache_first_curve_context(
            bytes,
            &mut position,
            int_width,
            solved,
            active_bytes,
            tables,
        )?;
        let direction = take_tagged_int(bytes, &mut position, 0x15, int_width)?;
        return Some(EmbeddedSpring {
            surfaces: context.surfaces,
            pcurves: context.pcurves,
            surface_parameter_ranges: [None, None],
            first_pcurve_parameter_range: None,
            parameter_range: context.parameter_range,
            discontinuities: context.discontinuities,
            discontinuity_flag: false,
            cache_first: Some(context.form),
            direction,
        });
    }
    let mut surfaces = [None, None];
    let mut surface_parameter_ranges = [None, None];
    for side in 0..2 {
        let saved = position;
        if take_native_ident(bytes, &mut position).as_deref() == Some("null_surface") {
            surface_parameter_ranges[side] = Some([
                [
                    take_range_value(bytes, &mut position)?,
                    take_range_value(bytes, &mut position)?,
                ],
                [
                    take_range_value(bytes, &mut position)?,
                    take_range_value(bytes, &mut position)?,
                ],
            ]);
        } else {
            position = saved;
            surfaces[side] = Some(decode_embedded_surface(bytes, &mut position, int_width)?);
        }
    }
    let first_pcurve;
    let first_pcurve_parameter_range;
    let saved = position;
    if take_native_ident(bytes, &mut position).as_deref() == Some("nullbs") {
        first_pcurve = None;
        first_pcurve_parameter_range = Some([
            take_range_value(bytes, &mut position)?,
            take_range_value(bytes, &mut position)?,
        ]);
    } else {
        position = saved;
        let (pcurve, end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
        position = end;
        first_pcurve = Some(pcurve);
        first_pcurve_parameter_range = None;
    }
    let saved = position;
    let second_pcurve = if take_native_ident(bytes, &mut position).as_deref() == Some("nullbs") {
        None
    } else {
        position = saved;
        let (pcurve, end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
        position = end;
        Some(pcurve)
    };
    let parameter_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
    ];
    let discontinuity_flag = take_bool(bytes, &mut position)?;
    let direction = take_tagged_int(bytes, &mut position, 0x15, int_width)?;
    Some(EmbeddedSpring {
        surfaces,
        pcurves: [first_pcurve, second_pcurve],
        surface_parameter_ranges,
        first_pcurve_parameter_range,
        parameter_range,
        discontinuities,
        discontinuity_flag,
        cache_first: None,
        direction,
    })
}

/// Writable fields in the shared context tail of a `spring_int_cur` subtype.
pub(crate) struct SpringPatchLayout {
    pub(crate) parameter_range: [usize; 2],
    pub(crate) discontinuities: [Vec<usize>; 3],
    pub(crate) discontinuity_flag: usize,
    pub(crate) direction: usize,
}

/// Locate spring context fields by walking the subtype grammar at `int_width`.
pub(crate) fn spring_patch_layout(bytes: &[u8], int_width: usize) -> Option<SpringPatchLayout> {
    let (marker, name_len) = find_intcurve_subtype(bytes, b"spring_int_cur")?;
    let mut position = marker + name_len + 3;
    for _ in 0..2 {
        let saved = position;
        if take_native_ident(bytes, &mut position).as_deref() == Some("null_surface") {
            for _ in 0..4 {
                take_double_payload(bytes, &mut position)?;
            }
        } else {
            position = saved;
            decode_embedded_surface(bytes, &mut position, int_width)?;
        }
    }
    let saved = position;
    if take_native_ident(bytes, &mut position).as_deref() == Some("nullbs") {
        take_double_payload(bytes, &mut position)?;
        take_double_payload(bytes, &mut position)?;
    } else {
        position = decode_pcurve_block_with_end(bytes, saved, int_width)?.1;
    }
    let saved = position;
    if take_native_ident(bytes, &mut position).as_deref() != Some("nullbs") {
        position = decode_pcurve_block_with_end(bytes, saved, int_width)?.1;
    }
    let parameter_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
    ];
    let discontinuity_flag = position;
    take_bool(bytes, &mut position)?;
    let direction = position;
    take_tagged_int(bytes, &mut position, 0x15, int_width)?;
    Some(SpringPatchLayout {
        parameter_range,
        discontinuities,
        discontinuity_flag,
        direction,
    })
}

/// Writable radius-law payloads in a rolling-ball blend surface subtype.
pub(crate) struct RollingBallPatchLayout {
    pub(crate) radii: [usize; 2],
}

/// Writable leading fields in a translational-extrusion surface subtype.
pub(crate) struct ExtrusionPatchLayout {
    pub(crate) parameter_interval: [usize; 2],
    pub(crate) direction: usize,
    pub(crate) native_position: usize,
}

/// Writable construction fields in a `helix_int_cur` subtype.
pub(crate) struct HelixPatchLayout {
    pub(crate) angle_range: [usize; 2],
    pub(crate) frame_vectors: [usize; 4],
    pub(crate) apex_factor: usize,
    pub(crate) axis: usize,
}

/// Writable fields following the source cache in an `offset_int_cur` subtype.
pub(crate) struct VectorOffsetPatchLayout {
    pub(crate) parameter_range: [usize; 2],
    pub(crate) offset: usize,
}

/// Writable parameter range following the parent curve in `subset_int_cur`.
pub(crate) struct SubsetPatchLayout {
    pub(crate) parameter_range: [usize; 2],
}

/// Writable parameter arrays in a `comp_int_cur` subtype.
pub(crate) struct CompoundPatchLayout {
    pub(crate) parameters: Vec<usize>,
    pub(crate) component_parameters: Vec<usize>,
}

/// Locate both compound parameter arrays from their native counts.
pub(crate) fn compound_patch_layout(bytes: &[u8], int_width: usize) -> Option<CompoundPatchLayout> {
    let name = b"comp_int_cur";
    let marker = find_subtype_marker(bytes, &[name]).map(|(marker, _)| marker)?;
    subtype_span(bytes, marker, int_width)?;
    let mut position = marker + name.len() + 3;
    let parameters = take_float_array_payloads(bytes, &mut position, int_width)?;
    let component_count =
        usize::try_from(take_tagged_int(bytes, &mut position, 0x04, int_width)?).ok()?;
    if component_count == 0 {
        return None;
    }
    let mut component_parameters = Vec::with_capacity(component_count);
    for _ in 0..component_count {
        component_parameters.push(take_double_payload(bytes, &mut position)?);
    }
    Some(CompoundPatchLayout {
        parameters,
        component_parameters,
    })
}

/// Locate the subset range by consuming the subtype-owned parent curve.
pub(crate) fn subset_patch_layout(bytes: &[u8], int_width: usize) -> Option<SubsetPatchLayout> {
    let name = b"subset_int_cur";
    let marker = find_subtype_marker(bytes, &[name]).map(|(marker, _)| marker)?;
    subtype_span(bytes, marker, int_width)?;
    let mut position = marker + name.len() + 3;
    position = decode_curve_block(bytes, position, int_width)?.end;
    let parameter_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    Some(SubsetPatchLayout { parameter_range })
}

/// Locate vector-offset fields by consuming the wrapper flag and source curve.
pub(crate) fn vector_offset_patch_layout(
    bytes: &[u8],
    int_width: usize,
) -> Option<VectorOffsetPatchLayout> {
    let name = b"offset_int_cur";
    let marker = find_subtype_marker(bytes, &[name]).map(|(marker, _)| marker)?;
    subtype_span(bytes, marker, int_width)?;
    let mut position = marker + name.len() + 3;
    take_bool(bytes, &mut position)?;
    position = decode_curve_block(bytes, position, int_width)?.end;
    let parameter_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let offset = position + 1;
    take_native_vec3(bytes, &mut position, 0x14)?;
    Some(VectorOffsetPatchLayout {
        parameter_range,
        offset,
    })
}

/// Locate helix fields by consuming the subtype prefix grammar.
pub(crate) fn helix_patch_layout(bytes: &[u8], int_width: usize) -> Option<HelixPatchLayout> {
    let name = b"helix_int_cur";
    let marker = find_subtype_marker(bytes, &[name]).map(|(marker, _)| marker)?;
    subtype_span(bytes, marker, int_width)?;
    let mut position = marker + name.len() + 3;
    let current_layout = take_optional_helix_revision(bytes, &mut position, int_width)?;
    let take_range_payload = |position: &mut usize| {
        if matches!(bytes.get(*position), Some(0x0a | 0x0b)) {
            *position += 1;
        }
        take_double_payload(bytes, position)
    };
    let angle_range = [
        take_range_payload(&mut position)?,
        take_range_payload(&mut position)?,
    ];
    let mut frame_vectors = [0usize; 4];
    let frame_tags = if current_layout {
        [0x13, 0x14, 0x14, 0x14]
    } else {
        [0x13; 4]
    };
    for (offset, tag) in frame_vectors.iter_mut().zip(frame_tags) {
        *offset = position + 1;
        take_native_vec3(bytes, &mut position, tag)?;
    }
    let apex_factor = take_double_payload(bytes, &mut position)?;
    let axis = position + 1;
    take_native_vec3(bytes, &mut position, 0x14)?;
    Some(HelixPatchLayout {
        angle_range,
        frame_vectors,
        apex_factor,
        axis,
    })
}

/// Locate extrusion fields from the `cyl_spl_sur` subtype header.
pub(crate) fn extrusion_patch_layout(
    bytes: &[u8],
    int_width: usize,
) -> Option<ExtrusionPatchLayout> {
    let names: [&[u8]; 2] = [b"cyl_spl_sur", b"cylsur"];
    let (start, name_len) =
        find_subtype_marker(bytes, &names).map(|(start, name)| (start, name.len()))?;
    subtype_span(bytes, start, int_width)?;
    let mut position = start + name_len + 3;
    let parameter_interval = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let direction = position + 1;
    take_native_vec3(bytes, &mut position, 0x14)?;
    let native_position = position + 1;
    take_native_vec3(bytes, &mut position, 0x13)?;
    Some(ExtrusionPatchLayout {
        parameter_interval,
        direction,
        native_position,
    })
}

/// Locate the rolling-ball radius pair by walking both supports and the slice curve.
pub(crate) fn rolling_ball_patch_layout(
    bytes: &[u8],
    int_width: usize,
) -> Option<RollingBallPatchLayout> {
    let names: [&[u8]; 6] = [
        b"rb_blend_spl_sur",
        b"rbblnsur",
        b"pipe_spl_sur",
        b"pipesur",
        b"sss_blend_spl_sur",
        b"sssblndsur",
    ];
    let (start, name_len) =
        find_subtype_marker(bytes, &names).map(|(start, name)| (start, name.len()))?;
    let span = subtype_span(bytes, start, int_width)?;
    let payload_start = name_len + 3;
    let radii = (|| {
        let mut position = payload_start;
        take_tagged_int(span, &mut position, 0x04, int_width)?;
        decode_rolling_ball_side(span, &mut position, int_width, None)?;
        decode_rolling_ball_side(span, &mut position, int_width, None)?;
        position = decode_curve_block(span, position, int_width)?.end;
        Some([
            start + take_double_payload(span, &mut position)?,
            start + take_double_payload(span, &mut position)?,
        ])
    })()
    .or_else(|| {
        let mut position = payload_start;
        for _ in 0..2 {
            take_native_string(span, &mut position)?;
            let support_kind = take_native_ident(span, &mut position)?;
            if !matches!(support_kind.as_str(), "plane" | "sphere" | "cone" | "torus") {
                return None;
            }
            position = decode_surface_block(span, position, int_width)?.end;
        }
        position = decode_curve_block(span, position, int_width)?.end;
        Some([
            start + take_double_payload(span, &mut position)?,
            start + take_double_payload(span, &mut position)?,
        ])
    })?;
    Some(RollingBallPatchLayout { radii })
}

/// Embedded cache-first base curve: a direct NURBS block, an analytic
/// `straight`, or a referenced `intcurve` resolved to its solved cache.
pub(crate) fn decode_embedded_base_curve_resolving_refs(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
    active_bytes: &[u8],
    tables: &SubtypeTables,
) -> Option<NurbsCurve> {
    if let Some(block) = decode_curve_block(bytes, *position, int_width) {
        *position = block.end;
        return Some(block.curve);
    }
    let saved = *position;
    match take_native_ident(bytes, position)?.as_str() {
        "straight" => {
            let origin = take_native_vec3(bytes, position, 0x13)?;
            let direction = take_native_vec3(bytes, position, 0x14)?;
            let start = Point3::new(
                origin[0] * LEN_TO_MM,
                origin[1] * LEN_TO_MM,
                origin[2] * LEN_TO_MM,
            );
            let end = Point3::new(
                (origin[0] + direction[0]) * LEN_TO_MM,
                (origin[1] + direction[1]) * LEN_TO_MM,
                (origin[2] + direction[2]) * LEN_TO_MM,
            );
            Some(NurbsCurve {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![start, end],
                weights: None,
                periodic: false,
            })
        }
        "ellipse" => {
            let center = take_native_vec3(bytes, position, 0x13)?;
            let normal = take_native_vec3(bytes, position, 0x14)?;
            let major = take_native_vec3(bytes, position, 0x14)?;
            let ratio = take_f64(bytes, position)?;
            ellipse_to_nurbs(center, normal, major, ratio)
        }
        "degenerate_curve" => {
            let point = take_native_vec3(bytes, position, 0x13)?;
            let at = Point3::new(
                point[0] * LEN_TO_MM,
                point[1] * LEN_TO_MM,
                point[2] * LEN_TO_MM,
            );
            Some(NurbsCurve {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![at, at],
                weights: None,
                periodic: false,
            })
        }
        "intcurve" => {
            take_bool(bytes, position)?;
            let reference = *position;
            let marker = b"\x0f\x0d\x03ref\x04";
            if !bytes.get(reference..)?.starts_with(marker) {
                if bytes.get(reference) == Some(&0x0f) {
                    // Inline subtype scope: resolve its solved curve cache.
                    let scope = subtype_span(bytes, reference, int_width)?;
                    let curve = decode_curve_cache_resolving_refs(scope, active_bytes, tables)?;
                    *position = reference + scope.len();
                    return Some(curve);
                }
                *position = saved;
                return None;
            }
            let index =
                usize::try_from(read_int(bytes, reference + marker.len(), int_width)?).ok()?;
            let reference_span = subtype_span(bytes, reference, int_width)?;
            *position = reference + reference_span.len();
            tables
                .for_width(int_width)
                .get(index)
                .and_then(|target| subtype_span(active_bytes, *target, int_width))
                .and_then(|target| decode_curve_cache_resolving_refs(target, active_bytes, tables))
        }
        _ => {
            *position = saved;
            None
        }
    }
}

fn decode_embedded_surface_offset(
    bytes: &[u8],
    int_width: usize,
    solved: &NurbsCurve,
    active_bytes: &[u8],
    tables: &SubtypeTables,
) -> Option<EmbeddedSurfaceOffset> {
    let name = b"off_surf_int_cur";
    let (marker, name_len) = find_intcurve_subtype(bytes, name)?;
    let mut position = marker + name_len + 3;
    if bytes.get(position) == Some(&0x04) {
        let context = decode_cache_first_curve_context(
            bytes,
            &mut position,
            int_width,
            solved,
            active_bytes,
            tables,
        )?;
        let base_u_range = [
            take_optional_range_value(bytes, &mut position)??,
            take_optional_range_value(bytes, &mut position)??,
        ];
        let base_v_range = [
            take_optional_range_value(bytes, &mut position)??,
            take_optional_range_value(bytes, &mut position)??,
        ];
        let base = decode_embedded_base_curve_resolving_refs(
            bytes,
            &mut position,
            int_width,
            active_bytes,
            tables,
        )?;
        let base_endpoints = [
            take_optional_range_value(bytes, &mut position)?,
            take_optional_range_value(bytes, &mut position)?,
        ];
        let base_range = [
            take_optional_range_value(bytes, &mut position)??,
            take_optional_range_value(bytes, &mut position)??,
        ];
        return Some(EmbeddedSurfaceOffset {
            context: EmbeddedIntersection {
                surfaces: context.surfaces,
                pcurves: context.pcurves,
                parameter_range: context.parameter_range,
                discontinuities: context.discontinuities,
            },
            discontinuity_flag: false,
            base_u_range,
            base_v_range,
            base,
            base_range,
            base_endpoints,
            cache_first: Some(context.form),
            distance: take_f64(bytes, &mut position)? * LEN_TO_MM,
            shift: take_f64(bytes, &mut position)?,
            scale: take_f64(bytes, &mut position)?,
        });
    }
    let surfaces = [
        decode_embedded_surface(bytes, &mut position, int_width)?,
        decode_embedded_surface(bytes, &mut position, int_width)?,
    ];
    let (first_pcurve, first_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = first_end;
    let (second_pcurve, second_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = second_end;
    let parameter_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
    ];
    let discontinuity_flag = take_bool(bytes, &mut position)?;
    let base_u_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let base_v_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let base = decode_curve_block(bytes, position, int_width)?;
    position = base.end;
    let base_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    Some(EmbeddedSurfaceOffset {
        context: EmbeddedIntersection {
            surfaces: surfaces.map(Some),
            pcurves: [Some(first_pcurve), Some(second_pcurve)],
            parameter_range,
            discontinuities,
        },
        discontinuity_flag,
        base_u_range,
        base_v_range,
        base: base.curve,
        base_range,
        base_endpoints: [None, None],
        cache_first: None,
        distance: take_f64(bytes, &mut position)? * LEN_TO_MM,
        shift: take_f64(bytes, &mut position)?,
        scale: take_f64(bytes, &mut position)?,
    })
}

/// Writable scalar fields in an `off_surf_int_cur` subtype.
pub(crate) struct SurfaceOffsetPatchLayout {
    pub(crate) parameter_range: [usize; 2],
    pub(crate) discontinuities: [Vec<usize>; 3],
    pub(crate) discontinuity_flag: usize,
    pub(crate) base_u_range: [usize; 2],
    pub(crate) base_v_range: [usize; 2],
    pub(crate) base_range: [usize; 2],
    pub(crate) distance: usize,
    pub(crate) shift: usize,
    pub(crate) scale: usize,
}

/// Locate surface-offset fields by walking supports and the base curve.
pub(crate) fn surface_offset_patch_layout(
    bytes: &[u8],
    int_width: usize,
) -> Option<SurfaceOffsetPatchLayout> {
    let (marker, name_len) = find_intcurve_subtype(bytes, b"off_surf_int_cur")?;
    let mut position = marker + name_len + 3;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    let parameter_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
    ];
    let discontinuity_flag = position;
    take_bool(bytes, &mut position)?;
    let base_u_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let base_v_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    position = decode_curve_block(bytes, position, int_width)?.end;
    let base_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let distance = take_double_payload(bytes, &mut position)?;
    let shift = take_double_payload(bytes, &mut position)?;
    let scale = take_double_payload(bytes, &mut position)?;
    Some(SurfaceOffsetPatchLayout {
        parameter_range,
        discontinuities,
        discontinuity_flag,
        base_u_range,
        base_v_range,
        base_range,
        distance,
        shift,
        scale,
    })
}

fn decode_embedded_silhouette(bytes: &[u8], int_width: usize) -> Option<EmbeddedSilhouette> {
    use cadmpeg_ir::geometry::SilhouetteKind;
    let names = [
        (b"silh_int_cur".as_slice(), SilhouetteKind::Standard),
        (b"para_silh_int_cur".as_slice(), SilhouetteKind::Parametric),
        (b"parasil".as_slice(), SilhouetteKind::Parametric),
        (
            b"taper_silh_int_cur".as_slice(),
            SilhouetteKind::Taper { draft_factor: 0.0 },
        ),
    ];
    let (marker, name, mut silhouette) = names.into_iter().find_map(|(name, silhouette)| {
        bytes
            .windows(name.len() + 3)
            .position(|window| {
                window[0] == 0x0f
                    && matches!(window[1], 0x0d | 0x0e)
                    && usize::from(window[2]) == name.len()
                    && &window[3..] == name
            })
            .map(|marker| (marker, name, silhouette))
    })?;
    let mut position = marker + name.len() + 3;
    let surfaces = [
        decode_embedded_surface(bytes, &mut position, int_width)?,
        decode_embedded_surface(bytes, &mut position, int_width)?,
    ];
    let (first_pcurve, first_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = first_end;
    let (second_pcurve, second_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = second_end;
    let parameter_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
    ];
    let cast_surface = decode_embedded_surface(bytes, &mut position, int_width)?;
    let light = take_native_vec3(bytes, &mut position, 0x14)?;
    let light_direction = normalized(light)?;
    if matches!(silhouette, SilhouetteKind::Taper { .. }) {
        silhouette = SilhouetteKind::Taper {
            draft_factor: take_f64(bytes, &mut position)?,
        };
    }
    Some(EmbeddedSilhouette {
        context: EmbeddedIntersection {
            surfaces: surfaces.map(Some),
            pcurves: [Some(first_pcurve), Some(second_pcurve)],
            parameter_range,
            discontinuities,
        },
        silhouette,
        cast_surface,
        light_direction,
    })
}

/// Writable light and optional taper fields in a silhouette subtype.
pub(crate) struct SilhouettePatchLayout {
    pub(crate) light_direction: usize,
    pub(crate) draft_factor: Option<usize>,
}

/// Locate silhouette fields by walking its context and cast surface.
pub(crate) fn silhouette_patch_layout(
    bytes: &[u8],
    int_width: usize,
    silhouette: &cadmpeg_ir::geometry::SilhouetteKind,
) -> Option<SilhouettePatchLayout> {
    use cadmpeg_ir::geometry::SilhouetteKind;
    let (names, tapered): (&[&[u8]], bool) = match silhouette {
        SilhouetteKind::Standard => (&[b"silh_int_cur"], false),
        SilhouetteKind::Parametric => (&[b"para_silh_int_cur", b"parasil"], false),
        SilhouetteKind::Taper { .. } => (&[b"taper_silh_int_cur"], true),
    };
    let (marker, name) = find_subtype_marker(bytes, names)?;
    let mut position = marker + name.len() + 3;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    take_double_payload(bytes, &mut position)?;
    take_double_payload(bytes, &mut position)?;
    for _ in 0..3 {
        take_float_array_payloads(bytes, &mut position, int_width)?;
    }
    decode_embedded_surface(bytes, &mut position, int_width)?;
    (*bytes.get(position)? == 0x14).then_some(())?;
    let light_direction = position + 1;
    bytes.get(light_direction..light_direction + 24)?;
    position = light_direction + 24;
    let draft_factor = if tapered {
        Some(take_double_payload(bytes, &mut position)?)
    } else {
        None
    };
    Some(SilhouettePatchLayout {
        light_direction,
        draft_factor,
    })
}

fn decode_embedded_surface_curve(
    bytes: &[u8],
    int_width: usize,
    solved: &NurbsCurve,
    active_bytes: &[u8],
    tables: &SubtypeTables,
) -> Option<(
    cadmpeg_ir::geometry::SurfaceCurveFamily,
    EmbeddedIntersection,
    Option<cadmpeg_ir::geometry::SurfaceCurveTail>,
)> {
    use cadmpeg_ir::geometry::SurfaceCurveFamily;
    let names = [
        (b"blend_int_cur".as_slice(), SurfaceCurveFamily::Blend),
        (b"bldcur".as_slice(), SurfaceCurveFamily::Blend),
        (
            b"surf_int_cur".as_slice(),
            SurfaceCurveFamily::SurfaceConstrained,
        ),
        (
            b"surfcur".as_slice(),
            SurfaceCurveFamily::SurfaceConstrained,
        ),
        (b"par_int_cur".as_slice(), SurfaceCurveFamily::Parametric),
        (b"parcur".as_slice(), SurfaceCurveFamily::Parametric),
        (b"skin_int_cur".as_slice(), SurfaceCurveFamily::Skin),
        (b"d5c2_cur".as_slice(), SurfaceCurveFamily::Skin),
    ];
    let (marker, name, family) = names.into_iter().find_map(|(name, family)| {
        bytes
            .windows(name.len() + 3)
            .position(|window| {
                window[0] == 0x0f
                    && matches!(window[1], 0x0d | 0x0e)
                    && usize::from(window[2]) == name.len()
                    && &window[3..] == name
            })
            .map(|marker| (marker, name, family))
    })?;
    let position = marker + name.len() + 3;
    decode_context_first_surface_curve(bytes, position, int_width, family.clone()).or_else(|| {
        decode_cache_first_surface_curve(
            bytes,
            position,
            int_width,
            family,
            solved,
            active_bytes,
            tables,
        )
    })
}

/// Shared cache-first intcurve context: revision, enum zero, solved cache and
/// fit tolerance, two bounded supports, two nullable pcurves, two optional
/// solved-interval endpoints, three discontinuity arrays, and one extension.
struct CacheFirstCurveContext {
    form: cadmpeg_ir::geometry::CacheFirstCurveForm,
    surfaces: [Option<SurfaceGeometry>; 2],
    pcurves: [Option<NurbsPcurve>; 2],
    parameter_range: [f64; 2],
    discontinuities: [Vec<f64>; 3],
}

fn decode_cache_first_curve_context(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
    solved: &NurbsCurve,
    active_bytes: &[u8],
    tables: &SubtypeTables,
) -> Option<CacheFirstCurveContext> {
    let revision = take_tagged_int(bytes, position, 0x04, int_width)?;
    (revision > 0).then_some(())?;
    (take_tagged_int(bytes, position, 0x15, int_width)? == 0).then_some(())?;
    *position = decode_curve_block(bytes, *position, int_width)?.end;
    take_f64(bytes, position)?;
    let (first_surface, first_bounds) = decode_optional_embedded_surface_with_bounds(
        bytes,
        position,
        int_width,
        active_bytes,
        tables,
    )?;
    let (second_surface, second_bounds) = decode_optional_embedded_surface_with_bounds(
        bytes,
        position,
        int_width,
        active_bytes,
        tables,
    )?;
    let pcurves = [
        decode_nullable_embedded_pcurve(bytes, position, int_width)?,
        decode_nullable_embedded_pcurve(bytes, position, int_width)?,
    ];
    let solved_range = [
        take_optional_range_value(bytes, position)?,
        take_optional_range_value(bytes, position)?,
    ];
    let domain = nurbs_curve_parameter_domain(solved)?;
    let parameter_range = [
        solved_range[0].unwrap_or(domain[0]),
        solved_range[1].unwrap_or(domain[1]),
    ];
    let discontinuities = [
        take_float_array(bytes, position, int_width)?,
        take_float_array(bytes, position, int_width)?,
        take_float_array(bytes, position, int_width)?,
    ];
    let extension = take_tagged_int(bytes, position, 0x04, int_width)?;
    Some(CacheFirstCurveContext {
        form: cadmpeg_ir::geometry::CacheFirstCurveForm {
            revision,
            support_bounds: [first_bounds, second_bounds],
            solved_range,
            extension,
        },
        surfaces: [first_surface, second_surface],
        pcurves,
        parameter_range,
        discontinuities,
    })
}

fn decode_context_first_surface_curve(
    bytes: &[u8],
    mut position: usize,
    int_width: usize,
    family: cadmpeg_ir::geometry::SurfaceCurveFamily,
) -> Option<(
    cadmpeg_ir::geometry::SurfaceCurveFamily,
    EmbeddedIntersection,
    Option<cadmpeg_ir::geometry::SurfaceCurveTail>,
)> {
    let surfaces = [
        decode_embedded_surface(bytes, &mut position, int_width)?,
        decode_embedded_surface(bytes, &mut position, int_width)?,
    ];
    let (first_pcurve, first_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = first_end;
    let (second_pcurve, second_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = second_end;
    let parameter_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
    ];
    Some((
        family,
        EmbeddedIntersection {
            surfaces: surfaces.map(Some),
            pcurves: [Some(first_pcurve), Some(second_pcurve)],
            parameter_range,
            discontinuities,
        },
        None,
    ))
}

fn decode_cache_first_surface_curve(
    bytes: &[u8],
    mut position: usize,
    int_width: usize,
    family: cadmpeg_ir::geometry::SurfaceCurveFamily,
    solved: &NurbsCurve,
    active_bytes: &[u8],
    tables: &SubtypeTables,
) -> Option<(
    cadmpeg_ir::geometry::SurfaceCurveFamily,
    EmbeddedIntersection,
    Option<cadmpeg_ir::geometry::SurfaceCurveTail>,
)> {
    let context = decode_cache_first_curve_context(
        bytes,
        &mut position,
        int_width,
        solved,
        active_bytes,
        tables,
    )?;
    let flag = take_bool(bytes, &mut position)?;
    let second_flag = matches!(bytes.get(position), Some(0x0a | 0x0b))
        .then(|| take_bool(bytes, &mut position))
        .flatten();
    Some((
        family,
        EmbeddedIntersection {
            surfaces: context.surfaces,
            pcurves: context.pcurves,
            parameter_range: context.parameter_range,
            discontinuities: context.discontinuities,
        },
        Some(cadmpeg_ir::geometry::SurfaceCurveTail {
            extension: context.form.extension,
            flag,
            second_flag,
            revision: context.form.revision,
            support_bounds: context.form.support_bounds,
            solved_range: context.form.solved_range,
        }),
    ))
}

/// Writable shared-context fields in a surface-related `intcurve` subtype.
pub(crate) struct SurfaceCurvePatchLayout {
    pub(crate) parameter_range: [usize; 2],
    pub(crate) discontinuities: [Vec<usize>; 3],
}

/// Locate a surface-curve context by walking its two ordered support pairs.
pub(crate) fn surface_curve_patch_layout(
    bytes: &[u8],
    int_width: usize,
    family: &cadmpeg_ir::geometry::SurfaceCurveFamily,
) -> Option<SurfaceCurvePatchLayout> {
    use cadmpeg_ir::geometry::SurfaceCurveFamily;
    let names: &[&[u8]] = match family {
        SurfaceCurveFamily::Blend => &[b"blend_int_cur", b"bldcur"],
        SurfaceCurveFamily::SurfaceConstrained => &[b"surf_int_cur", b"surfcur"],
        SurfaceCurveFamily::Parametric => &[b"par_int_cur", b"parcur"],
        SurfaceCurveFamily::Skin => &[b"skin_int_cur", b"d5c2_cur"],
    };
    let (marker, name) = find_subtype_marker(bytes, names)?;
    let mut position = marker + name.len() + 3;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    let parameter_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
    ];
    Some(SurfaceCurvePatchLayout {
        parameter_range,
        discontinuities,
    })
}

fn decode_embedded_three_surface_intersection(
    bytes: &[u8],
    int_width: usize,
) -> Option<EmbeddedThreeSurfaceIntersection> {
    let name = b"sss_int_cur";
    let marker = find_subtype_marker(bytes, &[name]).map(|(marker, _)| marker)?;
    let mut position = marker + name.len() + 3;
    let first = decode_embedded_surface(bytes, &mut position, int_width)?;
    let second = decode_embedded_surface(bytes, &mut position, int_width)?;
    let (first_pcurve, first_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = first_end;
    let (second_pcurve, second_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = second_end;
    let parameter_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
    ];
    let selector = take_tagged_int(bytes, &mut position, 0x04, int_width)?;
    let third = decode_embedded_surface(bytes, &mut position, int_width)?;
    let (third_pcurve, _) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    Some(EmbeddedThreeSurfaceIntersection {
        surfaces: [first, second, third],
        pcurves: [first_pcurve, second_pcurve, third_pcurve],
        parameter_range,
        discontinuities,
        selector,
    })
}

/// Writable context fields in an `sss_int_cur` subtype.
pub(crate) struct ThreeSurfacePatchLayout {
    pub(crate) parameter_range: [usize; 2],
    pub(crate) discontinuities: [Vec<usize>; 3],
    pub(crate) selector: usize,
}

/// Locate three-surface intersection fields by walking all three support pairs.
pub(crate) fn three_surface_patch_layout(
    bytes: &[u8],
    int_width: usize,
) -> Option<ThreeSurfacePatchLayout> {
    let (marker, name_len) = find_intcurve_subtype(bytes, b"sss_int_cur")?;
    let mut position = marker + name_len + 3;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    let parameter_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
    ];
    let selector = position;
    take_tagged_int(bytes, &mut position, 0x04, int_width)?;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    decode_pcurve_block_with_end(bytes, position, int_width)?;
    Some(ThreeSurfacePatchLayout {
        parameter_range,
        discontinuities,
        selector,
    })
}

fn decode_embedded_projection(bytes: &[u8], int_width: usize) -> Option<EmbeddedProjection> {
    let name = b"proj_int_cur";
    let (marker, name_len) = find_intcurve_subtype(bytes, name)?;
    let mut position = marker + name_len + 3;
    let surfaces = [
        decode_embedded_surface(bytes, &mut position, int_width)?,
        decode_embedded_surface(bytes, &mut position, int_width)?,
    ];
    let (first_pcurve, first_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = first_end;
    let (second_pcurve, second_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = second_end;
    let parameter_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
    ];
    let discontinuity_flag = take_bool(bytes, &mut position)?;
    let source = decode_curve_block(bytes, position, int_width)?;
    position = source.end;
    let flag = take_bool(bytes, &mut position)?;
    let tail = if bytes.get(position) == Some(&0x10) {
        cadmpeg_ir::geometry::ProjectionTail::EarlyClose { flag }
    } else {
        cadmpeg_ir::geometry::ProjectionTail::Ranged {
            flag,
            parameter_range: [
                take_range_value(bytes, &mut position)?,
                take_range_value(bytes, &mut position)?,
            ],
            role: take_native_string(bytes, &mut position)?,
        }
    };
    Some(EmbeddedProjection {
        surfaces,
        pcurves: [first_pcurve, second_pcurve],
        parameter_range,
        discontinuities,
        discontinuity_flag,
        source: source.curve,
        tail,
    })
}

/// Writable tail shape of a `proj_int_cur` subtype.
pub(crate) enum ProjectionTailPatchLayout {
    EarlyClose {
        flag: usize,
    },
    Ranged {
        flag: usize,
        parameter_range: [usize; 2],
        role: std::ops::Range<usize>,
    },
}

/// Writable shared-context and tail fields in a `proj_int_cur` subtype.
pub(crate) struct ProjectionPatchLayout {
    pub(crate) parameter_range: [usize; 2],
    pub(crate) discontinuities: [Vec<usize>; 3],
    pub(crate) discontinuity_flag: usize,
    pub(crate) tail: ProjectionTailPatchLayout,
}

/// Locate projection fields by walking supports, source curve, and selected tail.
pub(crate) fn projection_patch_layout(
    bytes: &[u8],
    int_width: usize,
) -> Option<ProjectionPatchLayout> {
    let (marker, name_len) = find_intcurve_subtype(bytes, b"proj_int_cur")?;
    let mut position = marker + name_len + 3;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    let parameter_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
    ];
    let discontinuity_flag = position;
    take_bool(bytes, &mut position)?;
    position = decode_curve_block(bytes, position, int_width)?.end;
    let tail_flag = position;
    take_bool(bytes, &mut position)?;
    let tail = if bytes.get(position) == Some(&0x10) {
        ProjectionTailPatchLayout::EarlyClose { flag: tail_flag }
    } else {
        let parameter_range = [
            take_double_payload(bytes, &mut position)?,
            take_double_payload(bytes, &mut position)?,
        ];
        (*bytes.get(position)? == 0x07).then_some(())?;
        let length = usize::from(*bytes.get(position + 1)?);
        let role = position + 2..position + 2 + length;
        bytes.get(role.clone())?;
        ProjectionTailPatchLayout::Ranged {
            flag: tail_flag,
            parameter_range,
            role,
        }
    };
    Some(ProjectionPatchLayout {
        parameter_range,
        discontinuities,
        discontinuity_flag,
        tail,
    })
}

fn decode_embedded_intersection(
    bytes: &[u8],
    int_width: usize,
    solved: &NurbsCurve,
    active_bytes: &[u8],
    tables: &SubtypeTables,
) -> Option<(EmbeddedIntersection, bool)> {
    let names: [&[u8]; 3] = [b"int_int_cur", b"surf_surf_int_cur", b"surfintcur"];
    let (marker, name) = find_subtype_marker(bytes, &names)?;
    let position = marker + name.len() + 3;
    decode_context_first_intersection(bytes, position, int_width).or_else(|| {
        decode_cache_first_intersection(bytes, position, int_width, solved, active_bytes, tables)
    })
}

fn decode_context_first_intersection(
    bytes: &[u8],
    mut position: usize,
    int_width: usize,
) -> Option<(EmbeddedIntersection, bool)> {
    let surfaces = [
        decode_embedded_surface(bytes, &mut position, int_width)?,
        decode_embedded_surface(bytes, &mut position, int_width)?,
    ];
    let (first_pcurve, first_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = first_end;
    let (second_pcurve, second_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = second_end;
    let parameter_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
    ];
    let discontinuity_flag = take_bool(bytes, &mut position)?;
    Some((
        EmbeddedIntersection {
            surfaces: surfaces.map(Some),
            pcurves: [Some(first_pcurve), Some(second_pcurve)],
            parameter_range,
            discontinuities,
        },
        discontinuity_flag,
    ))
}

fn decode_cache_first_intersection(
    bytes: &[u8],
    mut position: usize,
    int_width: usize,
    solved: &NurbsCurve,
    active_bytes: &[u8],
    tables: &SubtypeTables,
) -> Option<(EmbeddedIntersection, bool)> {
    (take_tagged_int(bytes, &mut position, 0x04, int_width)? > 0).then_some(())?;
    (take_tagged_int(bytes, &mut position, 0x15, int_width)? == 0).then_some(())?;
    position = decode_curve_block(bytes, position, int_width)?.end;
    take_f64(bytes, &mut position)?;
    let first_surface = decode_optional_embedded_surface_resolving_ref(
        bytes,
        &mut position,
        int_width,
        active_bytes,
        tables,
    )?;
    let second_surface = decode_optional_embedded_surface_resolving_ref(
        bytes,
        &mut position,
        int_width,
        active_bytes,
        tables,
    )?;
    let surfaces = [first_surface, second_surface];
    let pcurves = [
        decode_nullable_embedded_pcurve(bytes, &mut position, int_width)?,
        decode_nullable_embedded_pcurve(bytes, &mut position, int_width)?,
    ];
    let domain = nurbs_curve_parameter_domain(solved)?;
    let parameter_range = [
        take_optional_range_value(bytes, &mut position)?.unwrap_or(domain[0]),
        take_optional_range_value(bytes, &mut position)?.unwrap_or(domain[1]),
    ];
    let discontinuities = [
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
    ];
    let discontinuity_flag = take_tagged_int(bytes, &mut position, 0x04, int_width)? != 0;
    Some((
        EmbeddedIntersection {
            surfaces,
            pcurves,
            parameter_range,
            discontinuities,
        },
        discontinuity_flag,
    ))
}

/// Writable shared-context fields in an `int_int_cur` subtype.
pub(crate) struct IntersectionPatchLayout {
    pub(crate) parameter_range: [usize; 2],
    pub(crate) discontinuities: [Vec<usize>; 3],
    pub(crate) discontinuity_flag: usize,
}

/// Locate an intersection context by walking both ordered support pairs.
pub(crate) fn intersection_patch_layout(
    bytes: &[u8],
    int_width: usize,
) -> Option<IntersectionPatchLayout> {
    let names: [&[u8]; 3] = [b"int_int_cur", b"surf_surf_int_cur", b"surfintcur"];
    let (marker, name) = find_subtype_marker(bytes, &names)?;
    let mut position = marker + name.len() + 3;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    let parameter_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
    ];
    let discontinuity_flag = position;
    take_bool(bytes, &mut position)?;
    Some(IntersectionPatchLayout {
        parameter_range,
        discontinuities,
        discontinuity_flag,
    })
}

fn decode_embedded_two_sided_offset(
    bytes: &[u8],
    int_width: usize,
) -> Option<EmbeddedTwoSidedOffset> {
    let name = b"off_int_cur";
    let (marker, name_len) = find_intcurve_subtype(bytes, name)?;
    let mut position = marker + name_len + 3;
    let first_surface = decode_optional_embedded_surface(bytes, &mut position, int_width)?.value();
    let second_surface = decode_optional_embedded_surface(bytes, &mut position, int_width)?.value();
    let surfaces = [first_surface, second_surface];
    let first_pcurve = decode_optional_pcurve(bytes, &mut position, int_width)?.value();
    let second_pcurve = decode_optional_pcurve(bytes, &mut position, int_width)?.value();
    let pcurves = [first_pcurve, second_pcurve];
    let parameter_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
    ];
    let discontinuity_flag = take_bool(bytes, &mut position)?;
    let offsets = [
        take_range_value(bytes, &mut position)? * LEN_TO_MM,
        take_range_value(bytes, &mut position)? * LEN_TO_MM,
    ];
    Some(EmbeddedTwoSidedOffset {
        surfaces,
        pcurves,
        parameter_range,
        discontinuities,
        discontinuity_flag,
        offsets,
    })
}

fn decode_optional_embedded_surface(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<Nullable<SurfaceGeometry>> {
    let start = *position;
    if take_native_ident(bytes, position)?.as_str() == "null_surface" {
        return Some(Nullable::Null);
    }
    *position = start;
    decode_embedded_surface(bytes, position, int_width).map(Nullable::Value)
}

fn decode_optional_pcurve(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<Nullable<NurbsPcurve>> {
    let start = *position;
    if take_native_ident(bytes, position)?.as_str() == "nullbs" {
        return Some(Nullable::Null);
    }
    let (pcurve, end) = decode_pcurve_block_with_end(bytes, start, int_width)?;
    *position = end;
    Some(Nullable::Value(pcurve))
}

/// Writable scalar locations in a retained `off_int_cur` construction.
pub(crate) struct TwoSidedOffsetPatchLayout {
    pub(crate) parameter_range: [usize; 2],
    pub(crate) discontinuities: [Vec<usize>; 3],
    pub(crate) discontinuity_flag: usize,
    pub(crate) offsets: [usize; 2],
}

/// Locates the fixed-width scalar payloads after variable embedded supports.
pub(crate) fn two_sided_offset_patch_layout(
    bytes: &[u8],
    int_width: usize,
) -> Option<TwoSidedOffsetPatchLayout> {
    let name = b"off_int_cur";
    let (marker, name_len) = find_intcurve_subtype(bytes, name)?;
    let mut position = marker + name_len + 3;
    skip_offset_support_surface(bytes, &mut position, int_width)?;
    skip_offset_support_surface(bytes, &mut position, int_width)?;
    skip_offset_support_pcurve(bytes, &mut position, int_width)?;
    skip_offset_support_pcurve(bytes, &mut position, int_width)?;
    let parameter_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
    ];
    let discontinuity_flag = position;
    take_bool(bytes, &mut position)?;
    let offsets = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    Some(TwoSidedOffsetPatchLayout {
        parameter_range,
        discontinuities,
        discontinuity_flag,
        offsets,
    })
}

fn skip_offset_support_surface(bytes: &[u8], position: &mut usize, int_width: usize) -> Option<()> {
    let start = *position;
    if take_native_ident(bytes, position)?.as_str() == "null_surface" {
        return Some(());
    }
    *position = start;
    decode_embedded_surface(bytes, position, int_width)?;
    Some(())
}

fn skip_offset_support_pcurve(bytes: &[u8], position: &mut usize, int_width: usize) -> Option<()> {
    let start = *position;
    if take_native_ident(bytes, position)?.as_str() == "nullbs" {
        return Some(());
    }
    *position = decode_pcurve_block_with_end(bytes, start, int_width)?.1;
    Some(())
}

pub(crate) fn decode_embedded_surface(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<SurfaceGeometry> {
    decode_embedded_surface_fields(bytes, position, int_width, false).map(|(surface, _)| surface)
}

pub(crate) fn decode_embedded_surface_with_ranges(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<(SurfaceGeometry, [[Option<f64>; 2]; 2])> {
    decode_embedded_surface_fields(bytes, position, int_width, true)
}

fn decode_embedded_surface_fields(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
    preserve_ranges: bool,
) -> Option<(SurfaceGeometry, [[Option<f64>; 2]; 2])> {
    let no_ranges = [[None, None], [None, None]];
    let kind = take_native_ident(bytes, position)?;
    if kind == "spline" {
        let decoded = decode_surface_block(bytes, *position, int_width)?;
        *position = decoded.end;
        let ranges = if preserve_ranges {
            decode_surface_ranges(bytes, position)?
        } else {
            no_ranges
        };
        return Some((SurfaceGeometry::Nurbs(decoded.surface), ranges));
    }
    let point = take_native_vec3(bytes, position, 0x13)?;
    let point = Point3::new(
        point[0] * LEN_TO_MM,
        point[1] * LEN_TO_MM,
        point[2] * LEN_TO_MM,
    );
    match kind.as_str() {
        "plane" => {
            let normal = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            let u_axis = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            take_bool(bytes, position)?;
            let ranges = if preserve_ranges {
                decode_surface_ranges(bytes, position)?
            } else {
                no_ranges
            };
            Some((
                SurfaceGeometry::Plane {
                    origin: point,
                    normal,
                    u_axis,
                },
                ranges,
            ))
        }
        "cone" => {
            let native_axis = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            let major = take_native_vec3(bytes, position, 0x14)?;
            let radius = (major[0] * major[0] + major[1] * major[1] + major[2] * major[2]).sqrt()
                * LEN_TO_MM;
            let ref_direction = normalized(major)?;
            let ratio = take_f64(bytes, position)?;
            take_bool(bytes, position)?;
            take_bool(bytes, position)?;
            let sine = take_f64(bytes, position)?;
            let cosine = take_f64(bytes, position)?;
            take_f64(bytes, position)?;
            take_bool(bytes, position)?;
            let ranges = if preserve_ranges {
                decode_surface_ranges(bytes, position)?
            } else {
                for _ in 0..4 {
                    take_bool(bytes, position)?;
                }
                no_ranges
            };
            let surface = if sine.abs() <= f64::EPSILON && ratio == 1.0 {
                SurfaceGeometry::Cylinder {
                    origin: point,
                    axis: native_axis,
                    ref_direction,
                    radius,
                }
            } else {
                let axis = if sine * cosine < 0.0 {
                    Vector3::new(-native_axis.x, -native_axis.y, -native_axis.z)
                } else {
                    native_axis
                };
                SurfaceGeometry::Cone {
                    origin: point,
                    axis,
                    ref_direction,
                    radius,
                    ratio,
                    half_angle: sine.abs().asin(),
                }
            };
            Some((surface, ranges))
        }
        "sphere" => {
            let radius = take_f64(bytes, position)? * LEN_TO_MM;
            let ref_direction = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            let axis = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            take_bool(bytes, position)?;
            let ranges = if preserve_ranges {
                decode_surface_ranges(bytes, position)?
            } else {
                for _ in 0..4 {
                    take_bool(bytes, position)?;
                }
                no_ranges
            };
            Some((
                SurfaceGeometry::Sphere {
                    center: point,
                    axis,
                    ref_direction,
                    radius,
                },
                ranges,
            ))
        }
        "torus" => {
            let axis = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            let major_radius = take_f64(bytes, position)? * LEN_TO_MM;
            let minor_radius = take_f64(bytes, position)? * LEN_TO_MM;
            let ref_direction = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            take_bool(bytes, position)?;
            let ranges = if preserve_ranges {
                decode_surface_ranges(bytes, position)?
            } else {
                for _ in 0..4 {
                    take_bool(bytes, position)?;
                }
                no_ranges
            };
            Some((
                SurfaceGeometry::Torus {
                    center: point,
                    axis,
                    ref_direction,
                    major_radius,
                    minor_radius,
                },
                ranges,
            ))
        }
        _ => None,
    }
}

#[allow(clippy::option_option)] // Outer None is parse failure; inner None is an unresolved ref.
fn decode_optional_embedded_surface_resolving_ref(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
    active_bytes: &[u8],
    tables: &SubtypeTables,
) -> Option<Option<SurfaceGeometry>> {
    decode_optional_embedded_surface_with_bounds(bytes, position, int_width, active_bytes, tables)
        .map(|(surface, _)| surface)
}

/// Optional embedded support surface plus its four optional U/V bound fields.
#[allow(clippy::type_complexity)]
pub(crate) fn decode_optional_embedded_surface_with_bounds(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
    active_bytes: &[u8],
    tables: &SubtypeTables,
) -> Option<(Option<SurfaceGeometry>, [Option<f64>; 4])> {
    let saved = *position;
    let kind = take_native_ident(bytes, position);
    if kind.as_deref() == Some("null_surface") {
        return Some((None, [None; 4]));
    }
    if kind.as_deref() == Some("spline") {
        if matches!(bytes.get(*position), Some(0x0a | 0x0b)) {
            take_bool(bytes, position)?;
        }
        let reference = *position;
        let marker = b"\x0f\x0d\x03ref\x04";
        if bytes.get(reference..)?.starts_with(marker) {
            let index =
                usize::try_from(read_int(bytes, reference + marker.len(), int_width)?).ok()?;
            let reference_span = subtype_span(bytes, reference, int_width)?;
            *position = reference + reference_span.len();
            let surface = tables
                .for_width(int_width)
                .get(index)
                .and_then(|target| subtype_span(active_bytes, *target, int_width))
                .and_then(|target| {
                    decode_surface_cache_resolving_refs(target, active_bytes, tables)
                })
                .map(SurfaceGeometry::Nurbs);
            let mut bounds = [None; 4];
            for bound in &mut bounds {
                *bound = take_optional_range_value(bytes, position)?;
            }
            return Some((surface, bounds));
        }
    }
    *position = saved;
    if let Some(surface) = decode_embedded_surface(bytes, position, int_width) {
        let mut bounds = [None; 4];
        if kind.as_deref() == Some("plane") || kind.as_deref() == Some("spline") {
            for bound in &mut bounds {
                *bound = take_optional_range_value(bytes, position)?;
            }
        }
        return Some((Some(surface), bounds));
    }
    // Inline `spline { <subtype> }` support scope whose construction grammar
    // the embedded decoder does not type, including nested revision-gated
    // subtypes: resolve the scope's solved surface cache (following nested
    // subtype-table references) and consume the scope, then read the four
    // optional bound fields.
    *position = saved;
    if kind.as_deref() == Some("spline") {
        take_native_ident(bytes, position)?;
        if matches!(bytes.get(*position), Some(0x0a | 0x0b)) {
            take_bool(bytes, position)?;
        }
        if bytes.get(*position) == Some(&0x0f) {
            let scope = subtype_span(bytes, *position, int_width)?;
            let surface = decode_surface_cache_resolving_refs(scope, active_bytes, tables)?;
            *position += scope.len();
            let mut bounds = [None; 4];
            for bound in &mut bounds {
                *bound = take_optional_range_value(bytes, position)?;
            }
            return Some((Some(SurfaceGeometry::Nurbs(surface)), bounds));
        }
    }
    *position = saved;
    None
}

fn decode_two_sided_offset(
    bytes: &[u8],
    int_width: usize,
) -> Option<cadmpeg_ir::geometry::ProceduralCurveDefinition> {
    use cadmpeg_ir::geometry::{
        IntcurveSupportContext, IntcurveSupportSide, ProceduralCurveDefinition,
    };

    let name = b"off_int_cur";
    let (marker, name_len) = find_intcurve_subtype(bytes, name)?;
    let mut position = marker + name_len + 3;
    for expected in ["null_surface", "null_surface", "nullbs", "nullbs"] {
        if take_native_ident(bytes, &mut position)?.as_str() != expected {
            return None;
        }
    }
    let parameter_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
    ];
    let discontinuity_flag = take_bool(bytes, &mut position)?;
    let offsets = [
        take_range_value(bytes, &mut position)? * LEN_TO_MM,
        take_range_value(bytes, &mut position)? * LEN_TO_MM,
    ];
    Some(ProceduralCurveDefinition::TwoSidedOffset {
        context: IntcurveSupportContext {
            sides: [
                IntcurveSupportSide {
                    surface: None,
                    pcurve: None,
                },
                IntcurveSupportSide {
                    surface: None,
                    pcurve: None,
                },
            ],
            parameter_range,
            discontinuities,
        },
        discontinuity_flag,
        offsets,
    })
}

fn decode_compound_definition(bytes: &[u8], int_width: usize) -> Option<CompoundDefinition> {
    let name = b"comp_int_cur";
    let marker = find_subtype_marker(bytes, &[name]).map(|(marker, _)| marker)?;
    let mut position = marker + name.len() + 3;
    let parameters = take_float_array(bytes, &mut position, int_width)?;
    let count = usize::try_from(take_tagged_int(bytes, &mut position, 0x04, int_width)?).ok()?;
    if count == 0 {
        return None;
    }
    let mut component_parameters = Vec::with_capacity(count);
    for _ in 0..count {
        if bytes.get(position) != Some(&0x06) {
            return None;
        }
        component_parameters.push(read_f64(bytes, position + 1)?);
        position += 9;
    }
    if !matches!(bytes.get(position), Some(0x0a | 0x0b)) {
        return None;
    }
    position += 1;
    let mut components = Vec::with_capacity(count);
    for _ in 0..count {
        let relative = marker_positions(bytes.get(position..)?)
            .into_iter()
            .next()?;
        let decoded = decode_curve_block(bytes, position + relative, int_width)?;
        components.push(decoded.curve);
        position = decoded.end;
    }
    Some((parameters, component_parameters, components))
}

fn decode_subset_definition(bytes: &[u8], int_width: usize) -> Option<SubsetDefinition> {
    let name = b"subset_int_cur";
    let (marker, name_len) = find_intcurve_subtype(bytes, name)?;
    let start = marker + name_len + 3;
    let source_marker = marker_positions(&bytes[start..]).into_iter().next()? + start;
    let source = decode_curve_block(bytes, source_marker, int_width)?;
    let mut position = source.end;
    let range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    Some((source.curve, range))
}

fn decode_vector_offset_definition(
    bytes: &[u8],
    int_width: usize,
) -> Option<VectorOffsetDefinition> {
    let name = b"offset_int_cur";
    let (marker, name_len) = find_intcurve_subtype(bytes, name)?;
    let start = marker + name_len + 3;
    let source_marker = marker_positions(&bytes[start..]).into_iter().next()? + start;
    let source = decode_curve_block(bytes, source_marker, int_width)?;
    let mut position = source.end;
    if bytes.get(position) != Some(&0x06) || bytes.get(position + 9) != Some(&0x06) {
        return None;
    }
    let parameter_range = [
        read_f64(bytes, position + 1)?,
        read_f64(bytes, position + 10)?,
    ];
    position += 18;
    let offset = take_native_vec3(bytes, &mut position, 0x14)?;
    let first_label = take_native_string(bytes, &mut position)?;
    let first_code = take_tagged_int(bytes, &mut position, 0x04, int_width)?;
    let second_label = take_native_string(bytes, &mut position)?;
    let second_code = take_tagged_int(bytes, &mut position, 0x04, int_width)?;
    Some((
        source.curve,
        parameter_range,
        Vector3::new(
            offset[0] * LEN_TO_MM,
            offset[1] * LEN_TO_MM,
            offset[2] * LEN_TO_MM,
        ),
        [first_label, second_label],
        [first_code, second_code],
    ))
}

pub(crate) fn decode_helix_definition(
    bytes: &[u8],
    int_width: usize,
) -> Option<cadmpeg_ir::geometry::ProceduralCurveDefinition> {
    let name = b"helix_int_cur";
    let marker = find_subtype_marker(bytes, &[name]).map(|(marker, _)| marker)?;
    let mut position = marker + name.len() + 3;
    let current_layout = take_optional_helix_revision(bytes, &mut position, int_width)?;
    let lower = take_range_value(bytes, &mut position)?;
    let upper = take_range_value(bytes, &mut position)?;
    let center = take_native_vec3(bytes, &mut position, 0x13)?;
    let vector_tag = if current_layout { 0x14 } else { 0x13 };
    let major = take_native_vec3(bytes, &mut position, vector_tag)?;
    let minor = take_native_vec3(bytes, &mut position, vector_tag)?;
    let pitch = take_native_vec3(bytes, &mut position, vector_tag)?;
    if bytes.get(position) != Some(&0x06) {
        return None;
    }
    let apex_factor = read_f64(bytes, position + 1)?;
    position += 9;
    let axis = take_native_vec3(bytes, &mut position, 0x14)?;
    Some(cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
        angle_range: [lower, upper],
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
        axis: Vector3::new(axis[0], axis[1], axis[2]),
    })
}

/// Consume the current helix subtype's ASM release word when present. The
/// earlier form begins directly with an optional range-bound flag or double.
pub(crate) fn take_optional_helix_revision(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<bool> {
    if bytes.get(*position) != Some(&0x04) {
        return Some(false);
    }
    let revision = take_tagged_int(bytes, position, 0x04, int_width)?;
    (20_000..=99_999).contains(&revision).then_some(true)
}

/// Four optional U/V parameter bounds following a surface record's first
/// top-level subtype scope, or `None` when the record stores no bound
/// fields. `record_bytes` starts at the record's name chain.
pub fn record_trailing_surface_bounds(record_bytes: &[u8]) -> Option<[Option<f64>; 4]> {
    INT_WIDTHS
        .into_iter()
        .find_map(|int_width| record_trailing_surface_bounds_at(record_bytes, int_width))
}

fn record_trailing_surface_bounds_at(
    record_bytes: &[u8],
    int_width: usize,
) -> Option<[Option<f64>; 4]> {
    // Walk the fixed spline-record header: name tokens, attrib ref, history
    // int, geometry ref, sense boolean, then the subtype scope.
    let mut position = 0usize;
    while matches!(record_bytes.get(position), Some(0x0d | 0x0e)) {
        position += 2 + usize::from(*record_bytes.get(position + 1)?);
    }
    for tag in [0x0c, 0x04, 0x0c] {
        if record_bytes.get(position) != Some(&tag) {
            return None;
        }
        position += 1 + int_width;
    }
    if !matches!(record_bytes.get(position), Some(0x0a | 0x0b)) {
        return None;
    }
    position += 1;
    if record_bytes.get(position) != Some(&0x0f) {
        return None;
    }
    let scope = subtype_span(record_bytes, position, int_width)?;
    position += scope.len();
    if !matches!(record_bytes.get(position), Some(0x0a | 0x0b)) {
        return None;
    }
    let mut bounds = [None; 4];
    for bound in &mut bounds {
        *bound = take_optional_range_value(record_bytes, &mut position)?;
    }
    Some(bounds)
}

fn nurbs_curve_parameter_domain(curve: &NurbsCurve) -> Option<[f64; 2]> {
    let degree = usize::try_from(curve.degree).ok()?;
    Some([
        *curve.knots.get(degree)?,
        *curve.knots.get(curve.control_points.len())?,
    ])
}
