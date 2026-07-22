// SPDX-License-Identifier: Apache-2.0
//! Native record writers for surfaces, curves, and pcurves.

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    BlendRadiusLaw, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry,
    ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point3, Vector3};

use super::native_bytes::{
    native_curve_base, native_enum, native_f64, native_i64, native_ident, native_point, native_ref,
    native_string, native_subident, native_surface_base, native_u16_string, native_vector,
};
use crate::nurbs::reader::LEN_TO_MM;
use crate::writer::primitives::{finite_point, finite_vector, native_bool, unique_knot_count};

pub(crate) fn native_smbh_header(target: &CadIr) -> Result<Vec<u8>, CodecError> {
    if !target.tolerances.linear.is_finite()
        || target.tolerances.linear <= 0.0
        || !target.tolerances.angular.is_finite()
        || target.tolerances.angular <= 0.0
    {
        return Err(CodecError::Malformed(
            "source-less F3D tolerances must be finite and positive".into(),
        ));
    }
    let mut bytes = b"ASM BinaryFile8".to_vec();
    // Release word matching the product string, the zero region, then the
    // entity-count and flags words (bit 0: history partition present).
    bytes.extend_from_slice(&23100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 12]);
    bytes.extend_from_slice(&7u64.to_le_bytes());
    bytes.extend_from_slice(&3u64.to_le_bytes());
    native_string(&mut bytes, "Autodesk Neutron")?;
    native_string(&mut bytes, "ASM 231.6.3.65535 OSX")?;
    native_string(&mut bytes, "Thu Jan  1 00:00:00 1970")?;
    native_f64(&mut bytes, 60.0);
    native_f64(&mut bytes, target.tolerances.linear);
    native_f64(&mut bytes, target.tolerances.angular);
    Ok(bytes)
}

pub(crate) fn native_nurbs_surface(
    bytes: &mut Vec<u8>,
    surface: &NurbsSurface,
) -> Result<(), CodecError> {
    let u_count = usize::try_from(surface.u_count)
        .map_err(|_| CodecError::NotImplemented("F3D NURBS u count exceeds usize".into()))?;
    let v_count = usize::try_from(surface.v_count)
        .map_err(|_| CodecError::NotImplemented("F3D NURBS v count exceeds usize".into()))?;
    if surface.control_points.len() != u_count.saturating_mul(v_count)
        || surface
            .weights
            .as_ref()
            .is_some_and(|weights| weights.len() != surface.control_points.len())
    {
        return Err(CodecError::Malformed(
            "source-less F3D NURBS surface has inconsistent control-grid cardinality".into(),
        ));
    }
    native_ident(
        bytes,
        if surface.weights.is_some() {
            "nurbs"
        } else {
            "nubs"
        },
    )?;
    native_i64(bytes, i64::from(surface.u_degree));
    native_i64(bytes, i64::from(surface.v_degree));
    native_enum(bytes, if surface.u_periodic { 2 } else { 0 });
    native_enum(bytes, if surface.v_periodic { 2 } else { 0 });
    native_enum(bytes, 0);
    native_enum(bytes, 0);
    native_nurbs_knot_counts(bytes, [&surface.u_knots, &surface.v_knots])?;
    native_nurbs_knots(bytes, &surface.u_knots)?;
    native_nurbs_knots(bytes, &surface.v_knots)?;
    for v in 0..v_count {
        for u in 0..u_count {
            let index = u * v_count + v;
            let point = surface.control_points[index];
            native_f64(bytes, point.x / LEN_TO_MM);
            native_f64(bytes, point.y / LEN_TO_MM);
            native_f64(bytes, point.z / LEN_TO_MM);
            if let Some(weights) = surface.weights.as_ref() {
                native_f64(bytes, weights[index]);
            }
        }
    }
    Ok(())
}

pub(crate) fn native_procedural_surface(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    solved_surface: &Surface,
    solved_cache: &NurbsSurface,
) -> Result<bool, CodecError> {
    let written =
        native_procedural_surface_definition(bytes, target, solved_surface, solved_cache)?;
    if written {
        native_record_bounds(bytes, target, &solved_surface.id);
    }
    Ok(written)
}

/// Emit the four optional record-level U/V bound fields retained on the
/// surface's procedural construction, after its subtype scope closes.
fn native_record_bounds(bytes: &mut Vec<u8>, target: &CadIr, surface: &cadmpeg_ir::ids::SurfaceId) {
    let bounds = target
        .model
        .procedural_surfaces
        .iter()
        .find(|procedural| procedural.surface == *surface)
        .and_then(|procedural| procedural.record_bounds);
    if let Some(bounds) = bounds {
        for bound in bounds {
            native_optional_f64(bytes, bound);
        }
    }
}

fn native_procedural_surface_definition(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    solved_surface: &Surface,
    solved_cache: &NurbsSurface,
) -> Result<bool, CodecError> {
    let mut definitions = target
        .model
        .procedural_surfaces
        .iter()
        .filter(|procedural| procedural.surface == solved_surface.id);
    let Some(procedural) = definitions.next() else {
        return Ok(false);
    };
    if definitions.next().is_some() {
        return Err(CodecError::Malformed(format!(
            "surface {} has multiple procedural constructions",
            solved_surface.id
        )));
    }
    match &procedural.definition {
        ProceduralSurfaceDefinition::Deformable { construction } => {
            use cadmpeg_ir::geometry::DeformableSurfaceData;
            let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
                CodecError::Malformed(
                    "deformable surface requires a native cache-fit tolerance".into(),
                )
            })?;
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "defm_spl_sur")?;
            let support = target
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == construction.support)
                .ok_or_else(|| CodecError::Malformed("deformable support is missing".into()))?;
            native_embedded_surface(bytes, &support.geometry)?;
            let write_frame =
                |bytes: &mut Vec<u8>, frame: &cadmpeg_ir::geometry::DeformableSurfaceFrame| {
                    for vector in frame.leading_vectors {
                        native_vector(bytes, [vector.x, vector.y, vector.z]);
                    }
                    native_f64(bytes, frame.leading_parameter);
                    for flag in frame.leading_flags {
                        bytes.push(native_bool(flag));
                    }
                    for vector in frame.secondary_vectors {
                        native_vector(bytes, [vector.x, vector.y, vector.z]);
                    }
                    native_f64(bytes, frame.secondary_parameter);
                    for flag in frame.secondary_flags {
                        bytes.push(native_bool(flag));
                    }
                    native_point(
                        bytes,
                        [
                            frame.point.x / LEN_TO_MM,
                            frame.point.y / LEN_TO_MM,
                            frame.point.z / LEN_TO_MM,
                        ],
                    );
                    for flag in frame.trailing_flags {
                        bytes.push(native_bool(flag));
                    }
                };
            match &construction.data {
                DeformableSurfaceData::Full {
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
                    trailing_value,
                } => {
                    native_i64(bytes, 6);
                    for vector in leading_vectors {
                        native_vector(bytes, [vector.x, vector.y, vector.z]);
                    }
                    native_f64(bytes, *leading_parameter);
                    for flag in leading_flags {
                        bytes.push(native_bool(*flag));
                    }
                    native_i64(bytes, *selector);
                    let secondary = target
                        .model
                        .surfaces
                        .iter()
                        .find(|candidate| candidate.id == *surface)
                        .ok_or_else(|| {
                            CodecError::Malformed("deformable secondary surface is missing".into())
                        })?;
                    native_embedded_surface(bytes, &secondary.geometry)?;
                    native_i64(bytes, *native_id);
                    bytes.push(native_bool(*flag));
                    native_f64(bytes, *first_parameter);
                    if let Some(value) = version_value {
                        native_i64(bytes, *value);
                    }
                    native_f64(bytes, *second_parameter);
                    let curve = native_loft_curve_in_range(
                        target,
                        curve,
                        Some([*first_parameter, *second_parameter]),
                    )?;
                    native_nurbs_curve(bytes, &curve)?;
                    for frame in frames.iter() {
                        for vector in frame.vectors {
                            native_vector(bytes, [vector.x, vector.y, vector.z]);
                        }
                        native_f64(bytes, frame.parameter);
                        for flag in frame.flags {
                            bytes.push(native_bool(flag));
                        }
                    }
                    native_i64(bytes, *trailing_value);
                }
                DeformableSurfaceData::SurfaceCurve {
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
                } => {
                    native_i64(bytes, 5);
                    let secondary = target
                        .model
                        .surfaces
                        .iter()
                        .find(|candidate| candidate.id == *surface)
                        .ok_or_else(|| {
                            CodecError::Malformed("deformable secondary surface is missing".into())
                        })?;
                    native_embedded_surface(bytes, &secondary.geometry)?;
                    native_i64(bytes, *native_id);
                    bytes.push(native_bool(*flag));
                    native_f64(bytes, *first_parameter);
                    native_i64(bytes, *selector);
                    native_f64(bytes, *second_parameter);
                    let curve = native_loft_curve_in_range(
                        target,
                        curve,
                        Some([*first_parameter, *second_parameter]),
                    )?;
                    native_nurbs_curve(bytes, &curve)?;
                    for vector in vectors {
                        native_vector(bytes, [vector.x, vector.y, vector.z]);
                    }
                    native_f64(bytes, *frame_parameter);
                    for flag in flags {
                        bytes.push(native_bool(*flag));
                    }
                    native_i64(
                        bytes,
                        i64::try_from(parameter_triples.len()).map_err(|_| {
                            CodecError::NotImplemented("deformable triple count exceeds i64".into())
                        })?,
                    );
                    for triple in parameter_triples {
                        for value in triple {
                            native_f64(bytes, *value);
                        }
                    }
                }
                DeformableSurfaceData::Plain {
                    frame,
                    parameter_triples,
                } => {
                    native_i64(bytes, 1);
                    write_frame(bytes, frame);
                    native_i64(
                        bytes,
                        i64::try_from(parameter_triples.len()).map_err(|_| {
                            CodecError::NotImplemented("deformable triple count exceeds i64".into())
                        })?,
                    );
                    for triple in parameter_triples {
                        for value in triple {
                            native_f64(bytes, *value);
                        }
                    }
                }
                DeformableSurfaceData::Guided {
                    frame,
                    selector,
                    guide_parameter,
                } => {
                    native_i64(bytes, 3);
                    write_frame(bytes, frame);
                    native_i64(bytes, *selector);
                    native_f64(bytes, *guide_parameter);
                }
                DeformableSurfaceData::Minimal { vectors, selector } => {
                    native_i64(bytes, 8);
                    for vector in vectors {
                        native_vector(bytes, [vector.x, vector.y, vector.z]);
                    }
                    native_i64(bytes, *selector);
                }
            }
            native_nurbs_surface(bytes, solved_cache)?;
            native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
            for values in &construction.discontinuities {
                native_compound_loft_float_array(bytes, values)?;
            }
            bytes.push(native_bool(construction.discontinuity_flag));
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::TSpline { construction } => {
            use cadmpeg_ir::geometry::TSplineSubtransform;
            let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
                CodecError::Malformed(
                    "T-spline surface requires a native cache-fit tolerance".into(),
                )
            })?;
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "t_spl_sur")?;
            if let Some(form) = &construction.revision_form {
                if form.revision <= 0 {
                    return Err(CodecError::Malformed(
                        "revision-gated t_spl_sur requires a positive revision".into(),
                    ));
                }
                native_i64(bytes, form.revision);
                native_revision_surface_tail(
                    bytes,
                    form,
                    solved_cache,
                    procedural.cache_fit_tolerance,
                )?;
                for bound in &form.support_bounds {
                    native_optional_f64(bytes, *bound);
                }
                native_enum(bytes, construction.type_code);
            } else {
                native_nurbs_surface(bytes, solved_cache)?;
                native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
                for values in &construction.discontinuities {
                    native_compound_loft_float_array(bytes, values)?;
                }
                bytes.push(native_bool(construction.discontinuity_flag));
                for range in &construction.parameter_ranges {
                    for value in range {
                        native_f64(bytes, *value / LEN_TO_MM);
                    }
                }
                native_i64(bytes, construction.type_code);
            }
            bytes.push(0x0f);
            match &construction.subtransform {
                TSplineSubtransform::Inline {
                    program,
                    separator,
                    values,
                } => {
                    let parsed = cadmpeg_ir::geometry::TSplineProgram::parse(program);
                    if construction.program_graph.as_ref() != Some(&parsed) {
                        return Err(CodecError::Malformed(
                            "T-spline parsed program graph diverges from its native program".into(),
                        ));
                    }
                    if construction.values_graph.as_ref()
                        != Some(&cadmpeg_ir::geometry::TSplineProgram::parse(values))
                    {
                        return Err(CodecError::Malformed(
                            "T-spline parsed values graph diverges from its native program".into(),
                        ));
                    }
                    native_ident(bytes, "t_spl_subtrans_object")?;
                    native_u16_string(bytes, program)?;
                    if let Some(separator) = separator {
                        bytes.push(native_bool(*separator));
                    }
                    native_u16_string(bytes, values)?;
                }
                TSplineSubtransform::Reference {
                    resolved: Some(resolved),
                    ..
                } => {
                    let TSplineSubtransform::Inline {
                        program,
                        separator,
                        values,
                    } = resolved.as_ref()
                    else {
                        return Err(CodecError::Malformed(
                            "resolved T-spline subtransform must be inline".into(),
                        ));
                    };
                    let parsed = cadmpeg_ir::geometry::TSplineProgram::parse(program);
                    if construction.program_graph.as_ref() != Some(&parsed) {
                        return Err(CodecError::Malformed(
                            "T-spline parsed program graph diverges from its resolved program"
                                .into(),
                        ));
                    }
                    if construction.values_graph.as_ref()
                        != Some(&cadmpeg_ir::geometry::TSplineProgram::parse(values))
                    {
                        return Err(CodecError::Malformed(
                            "T-spline parsed values graph diverges from its resolved program"
                                .into(),
                        ));
                    }
                    native_ident(bytes, "t_spl_subtrans_object")?;
                    native_u16_string(bytes, program)?;
                    if let Some(separator) = separator {
                        bytes.push(native_bool(*separator));
                    }
                    native_u16_string(bytes, values)?;
                }
                TSplineSubtransform::Reference { resolved: None, .. } => {
                    return Err(CodecError::NotImplemented(
                        "source-less referenced t_spl_subtrans_object has no resolved target"
                            .into(),
                    ));
                }
            }
            bytes.push(0x10);
            native_i64(bytes, construction.trailing_value);
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::Exact {
            parameters,
            extension,
            revision_form,
        } => {
            let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
                CodecError::Malformed(
                    "exact spline surface requires a native cache-fit tolerance".into(),
                )
            })?;
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "exact_spl_sur")?;
            if let (
                Some(form),
                cadmpeg_ir::geometry::SplineSurfaceParameters::RevisionValues { values },
            ) = (revision_form, parameters)
            {
                if form.revision <= 0 {
                    return Err(CodecError::Malformed(
                        "revision-gated exact_spl_sur requires a positive revision".into(),
                    ));
                }
                native_i64(bytes, form.revision);
                native_revision_surface_tail(
                    bytes,
                    form,
                    solved_cache,
                    procedural.cache_fit_tolerance,
                )?;
                for value in values {
                    native_optional_f64(bytes, *value);
                }
                native_enum(bytes, *extension);
                bytes.push(0x10);
                return Ok(true);
            }
            let cadmpeg_ir::geometry::SplineSurfaceParameters::OrderedRanges { ranges } =
                parameters
            else {
                return Err(CodecError::Malformed(
                    "exact spline parameter fields conflict with its revision form".into(),
                ));
            };
            if revision_form.is_some() {
                return Err(CodecError::Malformed(
                    "exact spline ordered ranges cannot carry a revision form".into(),
                ));
            }
            native_nurbs_surface(bytes, solved_cache)?;
            native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
            for range in ranges {
                for value in range {
                    native_f64(bytes, *value);
                }
            }
            native_i64(bytes, *extension);
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::Compound {
            parameters,
            components,
        } => {
            if parameters.len() != components.len() {
                return Err(CodecError::Malformed(
                    "comp_spl_sur requires one parameter per component surface".into(),
                ));
            }
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "comp_spl_sur")?;
            native_nurbs_surface(bytes, solved_cache)?;
            if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
                native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
            }
            native_i64(
                bytes,
                i64::try_from(parameters.len()).map_err(|_| {
                    CodecError::NotImplemented("compound surface count exceeds i64".into())
                })?,
            );
            for parameter in parameters {
                native_f64(bytes, *parameter);
            }
            for component in components {
                let component = target
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == *component)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "compound surface {} references missing component {component}",
                            procedural.id
                        ))
                    })?;
                native_embedded_surface(bytes, &component.geometry)?;
            }
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::SubSurface { .. } => {
            return Err(CodecError::Malformed(format!(
                "sub-surface {} must use its exact cacheless carrier",
                procedural.id
            )));
        }
        ProceduralSurfaceDefinition::Taper {
            support,
            reference,
            pcurve,
            parameter,
            taper,
            revision_form,
        } => {
            let support = target
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == *support)
                .ok_or_else(|| CodecError::Malformed("taper support surface is missing".into()))?;
            let reference = target
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *reference)
                .ok_or_else(|| CodecError::Malformed("taper reference curve is missing".into()))?;
            let reference = native_spline_field_curve(
                &reference.geometry,
                native_pcurve_knot_domain(pcurve.as_ref())?,
            )?;
            let subtype = match taper {
                cadmpeg_ir::geometry::TaperSurfaceKind::Standard => "taper_spl_sur",
                cadmpeg_ir::geometry::TaperSurfaceKind::Orthogonal { .. } => "ortho_spl_sur",
                cadmpeg_ir::geometry::TaperSurfaceKind::Edge { .. } => "edge_tpr_spl_sur",
                cadmpeg_ir::geometry::TaperSurfaceKind::Shadow { .. } => "shadow_tpr_spl_sur",
                cadmpeg_ir::geometry::TaperSurfaceKind::Ruled { .. } => "ruled_tpr_spl_sur",
                cadmpeg_ir::geometry::TaperSurfaceKind::Swept { .. } => "swept_tpr_spl_sur",
            };
            if let Some(form) = revision_form {
                if form.revision <= 0
                    || !matches!(
                        taper,
                        cadmpeg_ir::geometry::TaperSurfaceKind::Orthogonal { .. }
                    )
                {
                    return Err(CodecError::Malformed(
                        "revision-gated taper generation requires the orthogonal subtype".into(),
                    ));
                }
                native_surface_base(bytes, "spline")?;
                bytes.push(0x0f);
                native_ident(bytes, "ortho_spl_sur")?;
                native_i64(bytes, form.revision);
                native_embedded_surface_with_bounds(
                    bytes,
                    &support.geometry,
                    &form.support_bounds,
                )?;
                native_nurbs_curve(bytes, &reference)?;
                for value in form.reference_endpoints {
                    native_optional_f64(bytes, value);
                }
                if let Some(pcurve) = pcurve {
                    native_nurbs_pcurve_block(bytes, pcurve)?;
                } else {
                    native_ident(bytes, "nullbs")?;
                }
                native_f64(bytes, *parameter);
                native_revision_surface_tail(
                    bytes,
                    form,
                    solved_cache,
                    procedural.cache_fit_tolerance,
                )?;
                bytes.push(0x10);
                return Ok(true);
            }
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, subtype)?;
            native_embedded_surface(bytes, &support.geometry)?;
            native_nurbs_curve(bytes, &reference)?;
            if let Some(pcurve) = pcurve {
                native_nurbs_pcurve_block(bytes, pcurve)?;
            } else {
                native_ident(bytes, "nullbs")?;
            }
            native_f64(bytes, *parameter);
            native_nurbs_surface(bytes, solved_cache)?;
            if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
                native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
            }
            let write_draft = |bytes: &mut Vec<u8>, draft: Vector3| {
                native_vector(bytes, [draft.x, draft.y, draft.z]);
            };
            match taper {
                cadmpeg_ir::geometry::TaperSurfaceKind::Standard => {}
                cadmpeg_ir::geometry::TaperSurfaceKind::Orthogonal { sense } => {
                    bytes.push(native_bool(*sense));
                }
                cadmpeg_ir::geometry::TaperSurfaceKind::Edge { draft } => {
                    write_draft(bytes, *draft);
                }
                cadmpeg_ir::geometry::TaperSurfaceKind::Shadow {
                    draft,
                    sine,
                    cosine,
                }
                | cadmpeg_ir::geometry::TaperSurfaceKind::Swept {
                    draft,
                    sine,
                    cosine,
                } => {
                    write_draft(bytes, *draft);
                    native_f64(bytes, *sine);
                    native_f64(bytes, *cosine);
                }
                cadmpeg_ir::geometry::TaperSurfaceKind::Ruled {
                    draft,
                    sine,
                    cosine,
                    factor,
                } => {
                    write_draft(bytes, *draft);
                    native_f64(bytes, *sine);
                    native_f64(bytes, *cosine);
                    native_f64(bytes, *factor);
                }
            }
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::Loft {
            sections,
            revision_form,
            parameters,
            closures,
            singularities,
            mode,
            bridge,
        } => encode_native_loft(
            bytes,
            target,
            procedural,
            sections,
            revision_form.as_ref(),
            parameters,
            closures,
            singularities,
            *mode,
            bridge,
            solved_cache,
        )?,
        ProceduralSurfaceDefinition::CompoundLoft { construction } => {
            encode_native_compound_loft(bytes, target, procedural, construction, solved_cache)?;
        }
        ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } => {
            encode_native_scaled_compound_loft(
                bytes,
                target,
                procedural,
                construction,
                Some(solved_cache),
            )?;
        }
        ProceduralSurfaceDefinition::Skin { construction } => {
            encode_native_skin_surface(bytes, target, procedural, construction, solved_cache)?;
        }
        ProceduralSurfaceDefinition::Law { construction } => {
            encode_native_law_surface(bytes, target, procedural, construction, Some(solved_cache))?;
        }
        ProceduralSurfaceDefinition::Net { construction } => {
            encode_native_net_surface(bytes, target, procedural, construction, solved_cache)?;
        }
        ProceduralSurfaceDefinition::Sweep {
            profile,
            spine,
            native: Some(construction),
        } => encode_native_sweep_surface(
            bytes,
            target,
            procedural,
            profile,
            spine,
            construction,
            solved_cache,
        )?,
        ProceduralSurfaceDefinition::Sweep { native: None, .. } => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D sweep surface {} lacks its native construction graph",
                procedural.id
            )))
        }
        ProceduralSurfaceDefinition::G2Blend { construction } => {
            encode_native_g2_blend(bytes, target, procedural, construction, solved_cache)?;
        }
        ProceduralSurfaceDefinition::RevisionCompoundLoft { construction } => {
            encode_native_revision_compound_loft(
                bytes,
                target,
                procedural,
                construction,
                solved_cache,
            )?;
        }
        ProceduralSurfaceDefinition::RevisionG2Blend { construction } => {
            encode_native_revision_g2_blend(bytes, target, procedural, construction, solved_cache)?;
        }
        ProceduralSurfaceDefinition::VariableBlend { construction } => {
            encode_native_variable_blend(bytes, target, procedural, construction, solved_cache)?;
        }
        ProceduralSurfaceDefinition::VertexBlend { .. } => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D vertex blend {} must use its procedural carrier because VBL_SURF has no solved-cache field",
                procedural.id
            )));
        }
        ProceduralSurfaceDefinition::Ruled { first, second } => {
            let profiles = [first, second]
                .map(|id| {
                    target
                        .model
                        .curves
                        .iter()
                        .find(|curve| curve.id == *id)
                        .ok_or_else(|| {
                            CodecError::Malformed(format!(
                                "ruled surface {} references missing profile {id}",
                                procedural.id
                            ))
                        })
                })
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?;
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "rule_sur")?;
            let profile_range = [
                solved_cache.u_knots.first().copied().ok_or_else(|| {
                    CodecError::Malformed("ruled solved surface has no U knot domain".into())
                })?,
                solved_cache.u_knots.last().copied().ok_or_else(|| {
                    CodecError::Malformed("ruled solved surface has no U knot domain".into())
                })?,
            ];
            for profile in profiles {
                let profile = native_interval_curve(&profile.geometry, profile_range)?;
                native_nurbs_curve(bytes, &profile)?;
            }
            native_nurbs_surface(bytes, solved_cache)?;
            if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
                native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
            }
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::Sum {
            first,
            second,
            basepoint,
            revision_form,
        } => {
            if let Some(form) = revision_form {
                if form.revision <= 0 {
                    return Err(CodecError::Malformed(
                        "revision-gated sum_spl_sur requires a positive revision".into(),
                    ));
                }
                native_surface_base(bytes, "spline")?;
                bytes.push(0x0f);
                native_ident(bytes, "sum_spl_sur")?;
                native_i64(bytes, form.revision);
                for (curve, endpoints) in [
                    (first, &form.reference_endpoints),
                    (second, &form.second_endpoints),
                ] {
                    let range = match endpoints {
                        [Some(lower), Some(upper)] => Some([*lower, *upper]),
                        _ => None,
                    };
                    let curve = native_loft_curve_in_range(target, curve, range)?;
                    native_nurbs_curve(bytes, &curve)?;
                    for endpoint in endpoints {
                        native_optional_f64(bytes, *endpoint);
                    }
                }
                native_point(
                    bytes,
                    [
                        basepoint.x / LEN_TO_MM,
                        basepoint.y / LEN_TO_MM,
                        basepoint.z / LEN_TO_MM,
                    ],
                );
                native_revision_surface_tail(
                    bytes,
                    form,
                    solved_cache,
                    procedural.cache_fit_tolerance,
                )?;
                bytes.push(0x10);
                return Ok(true);
            }
            let curves = [first, second]
                .map(|id| {
                    target
                        .model
                        .curves
                        .iter()
                        .find(|curve| curve.id == *id)
                        .ok_or_else(|| {
                            CodecError::Malformed(format!(
                                "sum surface {} references missing curve {id}",
                                procedural.id
                            ))
                        })
                })
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?;
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "sum_spl_sur")?;
            let ranges = [&solved_cache.u_knots, &solved_cache.v_knots]
                .into_iter()
                .map(|knots| {
                    Ok::<_, CodecError>([
                        knots.first().copied().ok_or_else(|| {
                            CodecError::Malformed(
                                "sum solved surface has an empty knot domain".into(),
                            )
                        })?,
                        knots.last().copied().ok_or_else(|| {
                            CodecError::Malformed(
                                "sum solved surface has an empty knot domain".into(),
                            )
                        })?,
                    ])
                })
                .collect::<Result<Vec<_>, _>>()?;
            for (curve, range) in curves.into_iter().zip(ranges) {
                let curve = native_interval_curve(&curve.geometry, range)?;
                native_nurbs_curve(bytes, &curve)?;
            }
            native_point(
                bytes,
                [
                    basepoint.x / LEN_TO_MM,
                    basepoint.y / LEN_TO_MM,
                    basepoint.z / LEN_TO_MM,
                ],
            );
            native_nurbs_surface(bytes, solved_cache)?;
            if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
                native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
            }
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::Revolution {
            directrix,
            axis_origin,
            axis_direction,
            angular_interval,
            parameter_interval,
            transposed,
            revision_form,
        } => {
            if let Some(form) = revision_form {
                if form.revision <= 0 {
                    return Err(CodecError::Malformed(
                        "revision-gated rot_spl_sur requires a positive revision".into(),
                    ));
                }
                native_surface_base(bytes, "spline")?;
                bytes.push(0x0f);
                native_ident(bytes, "rot_spl_sur")?;
                native_i64(bytes, form.revision);
                let range = match form.reference_endpoints {
                    [Some(lower), Some(upper)] => Some([lower, upper]),
                    _ => None,
                };
                let profile = native_loft_curve_in_range(target, directrix, range)?;
                native_nurbs_curve(bytes, &profile)?;
                for endpoint in &form.reference_endpoints {
                    native_optional_f64(bytes, *endpoint);
                }
                native_point(
                    bytes,
                    [
                        axis_origin.x / LEN_TO_MM,
                        axis_origin.y / LEN_TO_MM,
                        axis_origin.z / LEN_TO_MM,
                    ],
                );
                native_vector(
                    bytes,
                    [axis_direction.x, axis_direction.y, axis_direction.z],
                );
                native_revision_surface_tail(
                    bytes,
                    form,
                    solved_cache,
                    procedural.cache_fit_tolerance,
                )?;
                bytes.push(0x10);
                return Ok(true);
            }
            let directrix = target
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *directrix)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "revolution surface {} references a missing directrix",
                        procedural.id
                    ))
                })?;
            let directrix = native_interval_curve(&directrix.geometry, *parameter_interval)?;
            let native_parameter_interval = [
                directrix.knots.first().copied().unwrap_or(0.0),
                directrix.knots.last().copied().unwrap_or(0.0),
            ];
            let native_angular_interval = [
                solved_cache.v_knots.first().copied().unwrap_or(0.0),
                solved_cache.v_knots.last().copied().unwrap_or(0.0),
            ];
            if *transposed
                || *parameter_interval != native_parameter_interval
                || *angular_interval != native_angular_interval
            {
                return Err(CodecError::NotImplemented(
                    "source-less F3D rot_spl_sur intervals must match its profile and solved cache and cannot be transposed".into(),
                ));
            }
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "rot_spl_sur")?;
            native_nurbs_curve(bytes, &directrix)?;
            native_point(
                bytes,
                [
                    axis_origin.x / LEN_TO_MM,
                    axis_origin.y / LEN_TO_MM,
                    axis_origin.z / LEN_TO_MM,
                ],
            );
            native_vector(
                bytes,
                [axis_direction.x, axis_direction.y, axis_direction.z],
            );
            native_nurbs_surface(bytes, solved_cache)?;
            if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
                native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
            }
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::Offset {
            support,
            distance,
            u_sense,
            v_sense,
            extension_flags,
            revision_form,
        } => {
            let support = target
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == *support)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "offset surface {} references a missing support",
                        procedural.id
                    ))
                })?;
            if let Some(form) = revision_form {
                if form.revision <= 0 || form.flags.len() != 4 {
                    return Err(CodecError::Malformed(
                        "revision-gated off_spl_sur requires a positive revision and four flags"
                            .into(),
                    ));
                }
                native_surface_base(bytes, "spline")?;
                bytes.push(0x0f);
                native_ident(bytes, "off_spl_sur")?;
                native_i64(bytes, form.revision);
                native_embedded_surface_with_bounds(
                    bytes,
                    &support.geometry,
                    &form.support_bounds,
                )?;
                native_f64(bytes, *distance / LEN_TO_MM);
                for flag in &form.flags {
                    bytes.push(native_bool(*flag));
                }
                native_revision_surface_tail(
                    bytes,
                    form,
                    solved_cache,
                    procedural.cache_fit_tolerance,
                )?;
                bytes.push(0x10);
                return Ok(true);
            }
            let valid_flags = matches!(
                extension_flags.as_slice(),
                [] | [false] | [true, _] | [true, _, _]
            );
            if !valid_flags {
                return Err(CodecError::Malformed(
                    "off_spl_sur ASM extension flags have an invalid conditional shape".into(),
                ));
            }
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(
                bytes,
                if extension_flags.is_empty() {
                    "offsur"
                } else {
                    "off_spl_sur"
                },
            )?;
            native_embedded_surface(bytes, &support.geometry)?;
            native_f64(bytes, *distance / LEN_TO_MM);
            native_enum(bytes, *u_sense);
            native_enum(bytes, *v_sense);
            for flag in extension_flags {
                bytes.push(native_bool(*flag));
            }
            native_nurbs_surface(bytes, solved_cache)?;
            if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
                native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
            }
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::Extrusion {
            directrix,
            parameter_interval,
            direction,
            native_position,
        } => encode_native_extrusion(
            bytes,
            target,
            procedural,
            directrix,
            parameter_interval.ok_or_else(|| {
                CodecError::Malformed("source-less F3D extrusion lacks its native interval".into())
            })?,
            *direction,
            native_position.ok_or_else(|| {
                CodecError::Malformed("source-less F3D extrusion lacks its native position".into())
            })?,
            Some(solved_cache),
        )?,
        ProceduralSurfaceDefinition::Blend {
            supports,
            spine,
            radius,
            cross_section,
            native,
        } => {
            if let Some(native) = native {
                encode_complete_native_rolling_ball(
                    bytes,
                    target,
                    procedural,
                    native,
                    solved_cache,
                )?;
            } else {
                encode_native_rolling_ball(
                    bytes,
                    target,
                    procedural,
                    supports,
                    spine.as_ref(),
                    radius,
                    cross_section,
                    solved_cache,
                )?;
            }
        }
        ProceduralSurfaceDefinition::Helix { .. } => {
            return Err(CodecError::Malformed(format!(
                "source-less F3D helix surface {} must use its cacheless native carrier",
                procedural.id
            )))
        }
        ProceduralSurfaceDefinition::Unknown { .. } => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D unknown procedural surface {} cannot be regenerated losslessly",
                procedural.id
            )))
        }
    }
    Ok(true)
}

fn native_bridge_token(
    bytes: &mut Vec<u8>,
    token: &cadmpeg_ir::geometry::LoftBridgeToken,
) -> Result<(), CodecError> {
    match token {
        cadmpeg_ir::geometry::LoftBridgeToken::Boolean(value) => {
            bytes.push(native_bool(*value));
        }
        cadmpeg_ir::geometry::LoftBridgeToken::Integer(value) => native_i64(bytes, *value),
        cadmpeg_ir::geometry::LoftBridgeToken::Double(value) => native_f64(bytes, *value),
        cadmpeg_ir::geometry::LoftBridgeToken::Text(value) => native_string(bytes, value)?,
        cadmpeg_ir::geometry::LoftBridgeToken::Enum(value) => native_enum(bytes, *value),
    }
    Ok(())
}

fn native_g2_pcurve(
    bytes: &mut Vec<u8>,
    pcurve: Option<&PcurveGeometry>,
) -> Result<(), CodecError> {
    if let Some(pcurve) = pcurve {
        native_nurbs_pcurve_block(bytes, pcurve)
    } else {
        native_ident(bytes, "nullbs")
    }
}

fn native_g2_side(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    side: &cadmpeg_ir::geometry::G2BlendSide,
) -> Result<(), CodecError> {
    native_string(bytes, &side.label)?;
    let surface = target
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == side.surface)
        .ok_or_else(|| CodecError::Malformed(format!("G2 support {} is missing", side.surface)))?;
    native_embedded_surface(bytes, &surface.geometry)?;
    let curve = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == side.curve)
        .ok_or_else(|| CodecError::Malformed(format!("G2 side curve {} is missing", side.curve)))?;
    let pcurve = side
        .pcurves
        .iter()
        .flatten()
        .find(|pcurve| matches!(pcurve, PcurveGeometry::Nurbs { .. }));
    let curve = native_spline_field_curve(&curve.geometry, native_pcurve_knot_domain(pcurve)?)?;
    native_nurbs_curve(bytes, &curve)?;
    native_g2_pcurve(bytes, side.pcurves[0].as_ref())?;
    native_vector(
        bytes,
        [side.direction.x, side.direction.y, side.direction.z],
    );
    native_g2_pcurve(bytes, side.pcurves[1].as_ref())?;
    Ok(())
}

fn encode_native_g2_blend(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::G2BlendConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "g2_blend_spl_sur")?;
    native_g2_side(bytes, target, &construction.first)?;
    native_enum(bytes, construction.singularity);
    match &construction.first_shape {
        cadmpeg_ir::geometry::G2BlendFirstShape::Full { surface, tolerance } => {
            match (surface, tolerance) {
                (None, None) => native_ident(bytes, "nullbs")?,
                (Some(surface), Some(tolerance)) => {
                    let surface = target
                        .model
                        .surfaces
                        .iter()
                        .find(|candidate| candidate.id == *surface)
                        .ok_or_else(|| {
                            CodecError::Malformed("G2 first exact surface is missing".into())
                        })?;
                    let SurfaceGeometry::Nurbs(surface) = &surface.geometry else {
                        return Err(CodecError::NotImplemented(
                            "source-less G2 full branch requires a NURBS exact surface".into(),
                        ));
                    };
                    native_nurbs_surface(bytes, surface)?;
                    native_f64(bytes, *tolerance / LEN_TO_MM);
                }
                _ => {
                    return Err(CodecError::Malformed(
                        "G2 full surface and tolerance must be paired".into(),
                    ));
                }
            }
        }
        cadmpeg_ir::geometry::G2BlendFirstShape::None {
            coefficients,
            tolerance,
            extension,
            pcurve,
        } => {
            for coefficient in coefficients {
                native_f64(bytes, *coefficient);
            }
            native_f64(bytes, *tolerance / LEN_TO_MM);
            if let Some(extension) = extension {
                native_bridge_token(bytes, extension)?;
            }
            native_g2_pcurve(bytes, pcurve.as_ref())?;
        }
    }
    native_g2_side(bytes, target, &construction.second)?;
    let second_exact = target
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == construction.second_exact_surface)
        .ok_or_else(|| CodecError::Malformed("G2 second exact surface is missing".into()))?;
    let SurfaceGeometry::Nurbs(second_exact) = &second_exact.geometry else {
        return Err(CodecError::NotImplemented(
            "source-less G2 second exact surface must be NURBS".into(),
        ));
    };
    native_nurbs_surface(bytes, second_exact)?;
    let center_curve = native_loft_curve_in_range(
        target,
        &construction.center_curve,
        Some(construction.center_parameters),
    )?;
    native_nurbs_curve(bytes, &center_curve)?;
    for value in construction.center_parameters {
        native_f64(bytes, value);
    }
    native_i64(bytes, construction.center_flag);
    for range in construction.parameter_ranges {
        native_f64(bytes, range[0]);
        native_f64(bytes, range[1]);
    }
    for value in construction.trailing_parameters {
        native_f64(bytes, value);
    }
    native_nurbs_surface(bytes, solved_cache)?;
    if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
        native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
    }
    for discontinuities in &construction.discontinuities {
        native_i64(
            bytes,
            i64::try_from(discontinuities.len()).map_err(|_| {
                CodecError::NotImplemented("G2 discontinuity count exceeds i64".into())
            })?,
        );
        for value in discontinuities {
            native_f64(bytes, *value);
        }
    }
    bytes.push(0x10);
    Ok(())
}

fn native_loft_curve(
    target: &CadIr,
    id: &cadmpeg_ir::ids::CurveId,
) -> Result<NurbsCurve, CodecError> {
    let curve = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *id)
        .ok_or_else(|| CodecError::Malformed(format!("loft references missing curve {id}")))?;
    native_spline_field_curve(&curve.geometry, None).map_err(|_| {
        CodecError::NotImplemented(format!(
            "source-less F3D loft requires a NURBS, circle, or ellipse curve {id}"
        ))
    })
}

fn native_loft_subdata(
    bytes: &mut Vec<u8>,
    subdata: &cadmpeg_ir::geometry::LoftSubdata,
) -> Result<(), CodecError> {
    let expected_rows = if subdata.type_code == 211 {
        1
    } else {
        usize::try_from(subdata.row_count)
            .map_err(|_| CodecError::Malformed("negative loft row count".into()))?
    };
    let expected_columns = usize::try_from(subdata.column_count)
        .map_err(|_| CodecError::Malformed("negative loft column count".into()))?;
    if subdata.rows.len() != expected_rows
        || (subdata.type_code != 211
            && subdata
                .rows
                .iter()
                .any(|row| row.columns.len() != expected_columns))
    {
        return Err(CodecError::Malformed(
            "loft subdata counts do not match their rows".into(),
        ));
    }
    native_i64(bytes, subdata.type_code);
    native_i64(bytes, subdata.row_count);
    native_i64(bytes, subdata.column_count);
    for row in &subdata.rows {
        for value in row.parameters {
            native_f64(bytes, value);
        }
        for column in &row.columns {
            native_f64(bytes, column[0]);
            native_f64(bytes, column[1]);
        }
        if let Some(extra) = row.extra {
            native_f64(bytes, extra[0]);
            native_f64(bytes, extra[1]);
        }
    }
    Ok(())
}

fn native_loft_section(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    section: &cadmpeg_ir::geometry::LoftSection,
    parameter_range: Option<[f64; 2]>,
) -> Result<(), CodecError> {
    native_i64(
        bytes,
        i64::try_from(section.entries.len())
            .map_err(|_| CodecError::NotImplemented("loft section count exceeds i64".into()))?,
    );
    for entry in &section.entries {
        native_f64(bytes, entry.parameter);
        native_i64(
            bytes,
            i64::try_from(entry.profile.len())
                .map_err(|_| CodecError::NotImplemented("loft profile count exceeds i64".into()))?,
        );
        for member in &entry.profile {
            native_i64(bytes, member.type_code);
            let curve = native_loft_curve_in_range(target, &member.curve, parameter_range)?;
            native_nurbs_curve(bytes, &curve)?;
            if let Some(endpoints) = member.endpoints {
                for value in endpoints {
                    native_optional_f64(bytes, value);
                }
            }
            if let Some(surface_id) = &member.data.surface {
                let surface = target
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == *surface_id)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "loft references missing surface {surface_id}"
                        ))
                    })?;
                if member.endpoints.is_some() {
                    native_embedded_surface_with_bounds(
                        bytes,
                        &surface.geometry,
                        &member.data.support_bounds,
                    )?;
                } else {
                    native_embedded_surface(bytes, &surface.geometry)?;
                }
            } else {
                native_ident(bytes, "null_surface")?;
            }
            if let Some(pcurve) = &member.data.pcurve {
                native_nurbs_pcurve_block(bytes, pcurve)?;
            } else {
                native_ident(bytes, "nullbs")?;
            }
            bytes.push(native_bool(member.data.first_flag));
            native_i64(bytes, member.data.asm_extension);
            native_loft_subdata(bytes, &member.data.subdata)?;
            bytes.push(native_bool(member.data.direction.is_some()));
            if let Some(direction) = member.data.direction {
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
        }
        if let Some(path_curve) = &entry.path.curve {
            let path = native_loft_curve_in_range(target, path_curve, parameter_range)?;
            native_nurbs_curve(bytes, &path)?;
            if let Some(endpoints) = entry.path.endpoints {
                for value in endpoints {
                    native_optional_f64(bytes, value);
                }
            }
        } else {
            native_ident(bytes, "null_curve")?;
        }
        native_i64(
            bytes,
            i64::try_from(entry.path.auxiliaries.len()).map_err(|_| {
                CodecError::NotImplemented("loft auxiliary count exceeds i64".into())
            })?,
        );
        for auxiliary in &entry.path.auxiliaries {
            let auxiliary = native_loft_curve_in_range(target, auxiliary, parameter_range)?;
            native_nurbs_curve(bytes, &auxiliary)?;
        }
        native_i64(bytes, entry.path.flag);
    }
    Ok(())
}

fn native_loft_curve_in_range(
    target: &CadIr,
    id: &cadmpeg_ir::ids::CurveId,
    parameter_range: Option<[f64; 2]>,
) -> Result<NurbsCurve, CodecError> {
    let curve = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *id)
        .ok_or_else(|| CodecError::Malformed(format!("loft references missing curve {id}")))?;
    native_spline_field_curve(&curve.geometry, parameter_range).map_err(|_| {
        CodecError::NotImplemented(format!(
            "source-less F3D loft requires NURBS curve {id} without a section domain"
        ))
    })
}

fn native_compound_loft_scale(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    scale: &cadmpeg_ir::geometry::CompoundLoftScale,
) -> Result<(), CodecError> {
    native_i64(
        bytes,
        i64::try_from(scale.members.len()).map_err(|_| {
            CodecError::NotImplemented("compound-loft member count exceeds i64".into())
        })?,
    );
    for member in &scale.members {
        native_i64(bytes, member.type_code);
        let curve = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == member.curve)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "compound loft references missing member curve {}",
                    member.curve
                ))
            })?;
        let curve = native_spline_field_curve(
            &curve.geometry,
            native_pcurve_knot_domain(member.data.pcurve.as_ref())?,
        )?;
        native_nurbs_curve(bytes, &curve)?;
        let surface_id = member.data.surface.as_ref().ok_or_else(|| {
            CodecError::Malformed("compound loft members require a support surface".into())
        })?;
        let surface = target
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *surface_id)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "compound loft references missing surface {surface_id}"
                ))
            })?;
        native_embedded_surface(bytes, &surface.geometry)?;
        native_optional_pcurve(bytes, member.data.pcurve.as_ref())?;
        bytes.push(native_bool(member.data.first_flag));
        native_i64(bytes, member.data.asm_extension);
        native_loft_subdata(bytes, &member.data.subdata)?;
        bytes.push(native_bool(member.data.direction.is_some()));
        if let Some(direction) = member.data.direction {
            native_vector(bytes, [direction.x, direction.y, direction.z]);
        }
    }
    native_nurbs_curve(bytes, &native_loft_curve(target, &scale.path)?)?;
    native_i64(
        bytes,
        i64::try_from(scale.auxiliaries.len()).map_err(|_| {
            CodecError::NotImplemented("compound-loft auxiliary count exceeds i64".into())
        })?,
    );
    for auxiliary in &scale.auxiliaries {
        native_nurbs_curve(bytes, &native_loft_curve(target, auxiliary)?)?;
    }
    for value in scale.tail {
        native_i64(bytes, value);
    }
    Ok(())
}

fn encode_native_compound_loft(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::CompoundLoftConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::{CompoundLoftDirection, CompoundLoftTail};

    let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
        CodecError::Malformed("compound-loft surface requires a native cache-fit tolerance".into())
    })?;

    let first_absent = construction.scales.iter().position(Option::is_none);
    if first_absent
        .is_some_and(|index| construction.scales[index + 1..].iter().any(Option::is_some))
    {
        return Err(CodecError::Malformed(
            "compound-loft leading scales must form a contiguous prefix".into(),
        ));
    }
    if construction.fifth_scale.is_some() && first_absent.is_some() {
        return Err(CodecError::Malformed(
            "compound-loft fifth scale requires all four leading scales".into(),
        ));
    }

    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "cl_loft_spl_sur")?;
    native_nurbs_surface(bytes, solved_cache)?;
    native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
    for scale in construction.scales.iter().flatten() {
        native_compound_loft_scale(bytes, target, scale)?;
    }
    if let Some(scale) = construction.fifth_scale.as_deref() {
        native_compound_loft_scale(bytes, target, scale)?;
    }
    for flag in construction.flags {
        bytes.push(native_bool(flag));
    }
    match &construction.tail {
        CompoundLoftTail::Six {
            flags,
            scale,
            selector,
            direction,
            parameter_range,
            curve,
        } => {
            native_i64(bytes, 6);
            for flag in flags {
                bytes.push(native_bool(*flag));
            }
            native_compound_loft_scale(bytes, target, scale)?;
            native_i64(bytes, *selector);
            native_vector(bytes, [direction.x, direction.y, direction.z]);
            for value in parameter_range {
                native_f64(bytes, *value);
            }
            let curve = native_loft_curve_in_range(target, curve, Some(*parameter_range))?;
            native_nurbs_curve(bytes, &curve)?;
        }
        CompoundLoftTail::Seven {
            first_flag,
            first_scale,
            second_flag,
            second_scale,
            selector,
            direction,
            trailing_flags,
        } => {
            native_i64(bytes, 7);
            bytes.push(native_bool(*first_flag));
            if let Some(scale) = first_scale.as_deref() {
                native_compound_loft_scale(bytes, target, scale)?;
            }
            bytes.push(native_bool(*second_flag));
            native_compound_loft_scale(bytes, target, second_scale)?;
            native_i64(bytes, *selector);
            native_vector(bytes, [direction.x, direction.y, direction.z]);
            for flag in trailing_flags {
                bytes.push(native_bool(*flag));
            }
        }
        CompoundLoftTail::Zero {
            flags,
            selector,
            direction,
            trailing_flags,
        } => {
            native_i64(bytes, 0);
            for flag in flags {
                bytes.push(native_bool(*flag));
            }
            native_i64(bytes, *selector);
            match direction {
                CompoundLoftDirection::Vector { value } if *selector == 0 => {
                    native_vector(bytes, [value.x, value.y, value.z]);
                }
                CompoundLoftDirection::Curve { curve } if *selector != 0 => {
                    native_nurbs_curve(bytes, &native_loft_curve(target, curve)?)?;
                }
                _ => {
                    return Err(CodecError::Malformed(
                        "compound-loft direction conflicts with its selector".into(),
                    ));
                }
            }
            for flag in trailing_flags {
                bytes.push(native_bool(*flag));
            }
        }
    }
    bytes.push(0x10);
    Ok(())
}

fn native_compound_loft_float_array(bytes: &mut Vec<u8>, values: &[f64]) -> Result<(), CodecError> {
    native_i64(
        bytes,
        i64::try_from(values.len()).map_err(|_| {
            CodecError::NotImplemented("compound-loft float-array count exceeds i64".into())
        })?,
    );
    for value in values {
        native_f64(bytes, *value);
    }
    Ok(())
}

fn encode_native_scaled_compound_loft(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::ScaledCompoundLoftConstruction,
    solved_cache: Option<&NurbsSurface>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::{
        CompoundLoftDirection, ScaledCompoundLoftBranch, ScaledCompoundLoftShape,
    };

    let first_absent = construction.scales.iter().position(Option::is_none);
    if first_absent
        .is_some_and(|index| construction.scales[index + 1..].iter().any(Option::is_some))
    {
        return Err(CodecError::Malformed(
            "scaled compound-loft scales must form a contiguous prefix".into(),
        ));
    }
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "scaled_cloft_spl_sur")?;
    native_enum(bytes, construction.singularity);
    match &construction.shape {
        ScaledCompoundLoftShape::Full => {
            let solved_cache = solved_cache.ok_or_else(|| {
                CodecError::Malformed(
                    "scaled compound-loft full shape requires a solved NURBS cache".into(),
                )
            })?;
            let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
                CodecError::Malformed(
                    "scaled compound-loft full shape requires a native cache-fit tolerance".into(),
                )
            })?;
            native_nurbs_surface(bytes, solved_cache)?;
            native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
        }
        ScaledCompoundLoftShape::None {
            parameter_ranges,
            parameters,
        } => {
            if procedural.cache_fit_tolerance.is_some() {
                return Err(CodecError::Malformed(
                    "scaled compound-loft none shape cannot carry a cache-fit tolerance".into(),
                ));
            }
            for range in parameter_ranges {
                for value in range {
                    native_f64(bytes, *value);
                }
            }
            for values in parameters {
                native_compound_loft_float_array(bytes, values)?;
            }
        }
    }
    for values in &construction.discontinuities {
        native_compound_loft_float_array(bytes, values)?;
    }
    bytes.push(native_bool(construction.discontinuity_flag));
    for scale in construction.scales.iter().flatten() {
        native_compound_loft_scale(bytes, target, scale)?;
    }
    for flag in construction.flags {
        bytes.push(native_bool(flag));
    }
    native_i64(bytes, construction.selector);
    match &construction.branch {
        ScaledCompoundLoftBranch::ExtendedVector {
            first_scale,
            second_scale,
            selector,
            direction,
        } => {
            bytes.push(native_bool(true));
            if let Some(scale) = first_scale.as_deref() {
                native_compound_loft_scale(bytes, target, scale)?;
            }
            bytes.push(native_bool(true));
            native_compound_loft_scale(bytes, target, second_scale)?;
            native_i64(bytes, *selector);
            native_vector(bytes, [direction.x, direction.y, direction.z]);
        }
        ScaledCompoundLoftBranch::ExtendedCurve {
            scale,
            flag,
            singularity,
            curve,
        } => {
            bytes.push(native_bool(true));
            if let Some(scale) = scale.as_deref() {
                native_compound_loft_scale(bytes, target, scale)?;
            }
            bytes.push(native_bool(false));
            bytes.push(native_bool(*flag));
            native_enum(bytes, *singularity);
            native_nurbs_curve(bytes, &native_loft_curve(target, curve)?)?;
        }
        ScaledCompoundLoftBranch::Direct {
            flag,
            selector,
            direction,
        } => {
            bytes.push(native_bool(false));
            bytes.push(native_bool(*flag));
            native_i64(bytes, *selector);
            match direction {
                CompoundLoftDirection::Vector { value } if *selector == 0 => {
                    native_vector(bytes, [value.x, value.y, value.z]);
                }
                CompoundLoftDirection::Curve { curve } if *selector != 0 => {
                    native_nurbs_curve(bytes, &native_loft_curve(target, curve)?)?;
                }
                _ => {
                    return Err(CodecError::Malformed(
                        "scaled compound-loft direction conflicts with its selector".into(),
                    ));
                }
            }
        }
    }
    for flag in construction.trailing_flags {
        bytes.push(native_bool(flag));
    }
    native_i64(bytes, construction.tail_kind);
    for direction in construction.tail_directions {
        native_vector(bytes, [direction.x, direction.y, direction.z]);
    }
    native_enum(bytes, construction.tail_singularity);
    native_nurbs_curve(bytes, &native_loft_curve(target, &construction.tail_curve)?)?;
    bytes.push(0x10);
    Ok(())
}

pub(crate) fn native_cacheless_procedural_surface(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    surface: &Surface,
) -> Result<bool, CodecError> {
    let written = native_cacheless_procedural_surface_definition(bytes, target, surface)?;
    if written {
        native_record_bounds(bytes, target, &surface.id);
    }
    Ok(written)
}

fn native_cacheless_procedural_surface_definition(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    surface: &Surface,
) -> Result<bool, CodecError> {
    let mut definitions = target
        .model
        .procedural_surfaces
        .iter()
        .filter(|procedural| procedural.surface == surface.id);
    let Some(procedural) = definitions.next() else {
        return Ok(false);
    };
    if definitions.next().is_some() {
        return Err(CodecError::Malformed(format!(
            "surface {} has multiple procedural constructions",
            surface.id
        )));
    }
    if let ProceduralSurfaceDefinition::Extrusion {
        directrix,
        parameter_interval,
        direction,
        native_position,
    } = &procedural.definition
    {
        encode_native_extrusion(
            bytes,
            target,
            procedural,
            directrix,
            parameter_interval.ok_or_else(|| {
                CodecError::Malformed("source-less F3D extrusion lacks its native interval".into())
            })?,
            *direction,
            native_position.ok_or_else(|| {
                CodecError::Malformed("source-less F3D extrusion lacks its native position".into())
            })?,
            None,
        )?;
        return Ok(true);
    }
    if let ProceduralSurfaceDefinition::Helix { construction } = &procedural.definition {
        use cadmpeg_ir::geometry::HelixSurfaceProfile;
        native_surface_base(bytes, "spline")?;
        bytes.push(0x0f);
        let circular = matches!(construction.profile, HelixSurfaceProfile::Circle { .. });
        native_ident(
            bytes,
            if circular {
                "helix_spl_circ"
            } else {
                "helix_spl_line"
            },
        )?;
        for value in construction.angle_range {
            native_f64(bytes, value);
        }
        for value in construction.dimension_range {
            native_f64(bytes, if circular { value / LEN_TO_MM } else { value });
        }
        if let HelixSurfaceProfile::Circle { length, .. } = construction.profile {
            native_f64(bytes, length / LEN_TO_MM);
        }
        for value in construction.path.angle_range {
            native_f64(bytes, value);
        }
        native_point(
            bytes,
            [
                construction.path.center.x / LEN_TO_MM,
                construction.path.center.y / LEN_TO_MM,
                construction.path.center.z / LEN_TO_MM,
            ],
        );
        for vector in [
            construction.path.major,
            construction.path.minor,
            construction.path.pitch,
        ] {
            native_point(
                bytes,
                [
                    vector.x / LEN_TO_MM,
                    vector.y / LEN_TO_MM,
                    vector.z / LEN_TO_MM,
                ],
            );
        }
        native_f64(bytes, construction.path.apex_factor);
        native_vector(
            bytes,
            [
                construction.path.axis.x,
                construction.path.axis.y,
                construction.path.axis.z,
            ],
        );
        for sentinel in ["null_surface", "null_surface", "nullbs", "nullbs"] {
            native_ident(bytes, sentinel)?;
        }
        match construction.profile {
            HelixSurfaceProfile::Circle { radius, .. } => native_f64(bytes, radius / LEN_TO_MM),
            HelixSurfaceProfile::Line { direction } => {
                native_point(
                    bytes,
                    [
                        direction.x / LEN_TO_MM,
                        direction.y / LEN_TO_MM,
                        direction.z / LEN_TO_MM,
                    ],
                );
            }
        }
        bytes.push(0x10);
        return Ok(true);
    }
    if let ProceduralSurfaceDefinition::Ruled { first, second } = &procedural.definition {
        if procedural.cache_fit_tolerance.is_some() {
            return Err(CodecError::Malformed(
                "cacheless ruled surface cannot carry a cache-fit tolerance".into(),
            ));
        }
        native_surface_base(bytes, "spline")?;
        bytes.push(0x0f);
        native_ident(bytes, "rule_sur")?;
        native_nurbs_curve(bytes, &native_loft_curve(target, first)?)?;
        native_nurbs_curve(bytes, &native_loft_curve(target, second)?)?;
        bytes.push(0x10);
        return Ok(true);
    }
    if let ProceduralSurfaceDefinition::Sum {
        first,
        second,
        basepoint,
        revision_form: None,
    } = &procedural.definition
    {
        if procedural.cache_fit_tolerance.is_some() {
            return Err(CodecError::Malformed(
                "cacheless sum surface cannot carry a cache-fit tolerance".into(),
            ));
        }
        native_surface_base(bytes, "spline")?;
        bytes.push(0x0f);
        native_ident(bytes, "sum_spl_sur")?;
        native_nurbs_curve(bytes, &native_loft_curve(target, first)?)?;
        native_nurbs_curve(bytes, &native_loft_curve(target, second)?)?;
        native_point(
            bytes,
            [
                basepoint.x / LEN_TO_MM,
                basepoint.y / LEN_TO_MM,
                basepoint.z / LEN_TO_MM,
            ],
        );
        bytes.push(0x10);
        return Ok(true);
    }
    if let ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } = &procedural.definition
    {
        if matches!(
            construction.shape,
            cadmpeg_ir::geometry::ScaledCompoundLoftShape::None { .. }
        ) {
            encode_native_scaled_compound_loft(bytes, target, procedural, construction, None)?;
            return Ok(true);
        }
    }
    if let ProceduralSurfaceDefinition::Law { construction } = &procedural.definition {
        if !matches!(
            construction.tail,
            cadmpeg_ir::geometry::LawSurfaceTail::Full
        ) {
            encode_native_law_surface(bytes, target, procedural, construction, None)?;
            return Ok(true);
        }
    }
    if let ProceduralSurfaceDefinition::SubSurface {
        support,
        parameter_ranges,
    } = &procedural.definition
    {
        let support = target
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *support)
            .ok_or_else(|| CodecError::Malformed("sub-surface support is missing".into()))?;
        native_surface_base(bytes, "spline")?;
        bytes.push(0x0f);
        native_ident(bytes, "sub_spl_sur")?;
        for value in parameter_ranges.iter().flatten() {
            native_f64(bytes, *value);
        }
        native_embedded_surface(bytes, &support.geometry)?;
        bytes.push(0x10);
        return Ok(true);
    }
    if let ProceduralSurfaceDefinition::VertexBlend { construction } = &procedural.definition {
        encode_native_vertex_blend(bytes, target, construction)?;
        return Ok(true);
    }
    Ok(false)
}

fn native_law_expression(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    expression: &cadmpeg_ir::geometry::LawExpression,
    depth: usize,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::LawExpression;
    if depth > 64 {
        return Err(CodecError::Malformed(
            "native law expression exceeds 64 recursive levels".into(),
        ));
    }
    match expression {
        LawExpression::Null => native_string(bytes, "null_law")?,
        LawExpression::Integer { value } => native_i64(bytes, *value),
        LawExpression::Double { value } => native_f64(bytes, *value),
        LawExpression::Point { value } => {
            native_point(
                bytes,
                [
                    value.x / LEN_TO_MM,
                    value.y / LEN_TO_MM,
                    value.z / LEN_TO_MM,
                ],
            );
        }
        LawExpression::Vector { value } => {
            native_vector(bytes, [value.x, value.y, value.z]);
        }
        LawExpression::Transform { scalars, enums } => {
            native_string(bytes, "TRANS")?;
            for scalar in scalars {
                native_f64(bytes, *scalar);
            }
            for value in enums {
                native_enum(bytes, *value);
            }
        }
        LawExpression::Edge {
            curve,
            endpoints,
            parameters,
        } => {
            native_string(bytes, "EDGE")?;
            let curve = target
                .model
                .curves
                .iter()
                .find(|candidate| candidate.id == *curve)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("law edge curve {curve} is missing"))
                })?;
            let curve = native_interval_curve(&curve.geometry, *parameters)?;
            native_nurbs_curve(bytes, &curve)?;
            if let Some(endpoints) = endpoints {
                for value in endpoints {
                    native_optional_f64(bytes, *value);
                }
            }
            for parameter in parameters {
                native_f64(bytes, *parameter);
            }
        }
        LawExpression::Spline {
            native_id,
            knots,
            controls,
            point,
        } => {
            native_string(bytes, "SPLINE_LAW")?;
            native_i64(bytes, *native_id);
            native_compound_loft_float_array(bytes, knots)?;
            native_compound_loft_float_array(bytes, controls)?;
            native_point(
                bytes,
                [
                    point.x / LEN_TO_MM,
                    point.y / LEN_TO_MM,
                    point.z / LEN_TO_MM,
                ],
            );
        }
        LawExpression::Algebraic { operator, operands } => {
            let arity = match operator.as_str() {
                "COS" | "SIN" | "TAN" | "COT" | "SEC" | "CSC" | "COSH" | "SINH" | "TANH"
                | "COTH" | "SECH" | "CSCH" | "ARCCOS" | "ARCSIN" | "ARCTAN" | "ARCOT"
                | "ARCSEC" | "ARCCSC" | "ARCCOSH" | "ARCSINH" | "ARCTANH" | "ARCOTH"
                | "ARCSECH" | "ARCCSCH" | "ABS" | "EXP" | "LN" | "LOG" | "SIGN" | "SIZE"
                | "SET" | "SQRT" | "NORM" | "NOT" => 1,
                "CROSS" | "DOT" | "DCUR" | "ROTATE" | "TERM" => 2,
                "VEC" | "DSURF" => 3,
                "MIN" | "MAX" | "STEP" => {
                    return Err(CodecError::NotImplemented(format!(
                        "source-less F3D law operator {operator} has unresolved variable arity"
                    )));
                }
                _ => {
                    return Err(CodecError::NotImplemented(format!(
                        "source-less F3D law operator {operator} has no defined byte grammar"
                    )));
                }
            };
            if operands.len() != arity {
                return Err(CodecError::Malformed(format!(
                    "F3D law operator {operator} requires {arity} operands, got {}",
                    operands.len()
                )));
            }
            native_string(bytes, operator)?;
            for operand in operands {
                native_law_expression(bytes, target, operand, depth + 1)?;
            }
        }
    }
    Ok(())
}

fn native_law_formula(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    formula: &cadmpeg_ir::geometry::LawFormula,
) -> Result<(), CodecError> {
    native_string(bytes, &formula.name)?;
    if formula.name == "null_law" {
        if !formula.variables.is_empty() {
            return Err(CodecError::Malformed(
                "null_law formula cannot carry variables".into(),
            ));
        }
        return Ok(());
    }
    native_i64(
        bytes,
        i64::try_from(formula.variables.len())
            .map_err(|_| CodecError::NotImplemented("law variable count exceeds i64".into()))?,
    );
    for variable in &formula.variables {
        native_law_expression(bytes, target, variable, 0)?;
    }
    Ok(())
}

fn encode_native_law_surface(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::LawSurfaceConstruction,
    solved_cache: Option<&NurbsSurface>,
) -> Result<(), CodecError> {
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "law_spl_sur")?;
    if let Some(parameter_ranges) = construction.parameter_ranges {
        for range in parameter_ranges {
            for parameter in range {
                native_f64(bytes, parameter);
            }
        }
    }
    native_law_formula(bytes, target, &construction.primary)?;
    native_i64(
        bytes,
        i64::try_from(construction.additional.len()).map_err(|_| {
            CodecError::NotImplemented("law surface formula count exceeds i64".into())
        })?,
    );
    for formula in &construction.additional {
        native_law_formula(bytes, target, formula)?;
    }
    match &construction.tail {
        cadmpeg_ir::geometry::LawSurfaceTail::Full => {
            let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
                CodecError::Malformed("full law surface requires a cache-fit tolerance".into())
            })?;
            if construction.parameter_ranges.is_none() {
                native_enum(bytes, 0);
            }
            native_nurbs_surface(
                bytes,
                solved_cache.ok_or_else(|| {
                    CodecError::Malformed("full law surface requires a solved cache".into())
                })?,
            )?;
            native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
        }
        cadmpeg_ir::geometry::LawSurfaceTail::Summary {
            parameters,
            fit_tolerance,
            closures,
            singularities,
        } => {
            native_enum(bytes, 1);
            for values in parameters {
                native_compound_loft_float_array(bytes, values)?;
            }
            native_f64(bytes, fit_tolerance / LEN_TO_MM);
            for value in closures.iter().chain(singularities) {
                native_enum(bytes, *value);
            }
        }
        cadmpeg_ir::geometry::LawSurfaceTail::None {
            parameter_ranges,
            closures,
            singularities,
        } => {
            native_enum(bytes, 2);
            for value in parameter_ranges.iter().flatten() {
                native_f64(bytes, *value);
            }
            for value in closures.iter().chain(singularities) {
                native_enum(bytes, *value);
            }
        }
        cadmpeg_ir::geometry::LawSurfaceTail::Historical => native_enum(bytes, 3),
        cadmpeg_ir::geometry::LawSurfaceTail::Optimal => native_enum(bytes, 4),
    }
    for values in &construction.discontinuities {
        native_compound_loft_float_array(bytes, values)?;
    }
    bytes.push(0x10);
    Ok(())
}

fn native_skin_profile_data(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    data: &cadmpeg_ir::geometry::LoftProfileData,
) -> Result<(), CodecError> {
    let surface_id = data
        .surface
        .as_ref()
        .ok_or_else(|| CodecError::Malformed("skin profiles require a support surface".into()))?;
    let surface = target
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == *surface_id)
        .ok_or_else(|| {
            CodecError::Malformed(format!("skin references missing surface {surface_id}"))
        })?;
    native_embedded_surface(bytes, &surface.geometry)?;
    native_optional_pcurve(bytes, data.pcurve.as_ref())?;
    bytes.push(native_bool(data.first_flag));
    native_i64(bytes, data.asm_extension);
    native_loft_subdata(bytes, &data.subdata)?;
    bytes.push(native_bool(data.direction.is_some()));
    if let Some(direction) = data.direction {
        native_vector(bytes, [direction.x, direction.y, direction.z]);
    }
    Ok(())
}

fn encode_native_skin_surface(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::SkinSurfaceConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::SkinSurfaceLayout;
    let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
        CodecError::Malformed("skin surface requires a native cache-fit tolerance".into())
    })?;
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "skin_spl_sur")?;
    native_enum(bytes, construction.surface_boolean);
    native_enum(bytes, construction.surface_normal);
    native_enum(bytes, construction.surface_direction);
    native_i64(bytes, construction.count);
    native_f64(bytes, construction.parameter);
    native_i64(bytes, construction.inner_count);
    match &construction.layout {
        SkinSurfaceLayout::Profiles {
            profiles,
            path,
            tail,
        } => {
            if usize::try_from(construction.inner_count).ok() != Some(profiles.len()) {
                return Err(CodecError::Malformed(
                    "skin profile count conflicts with its inner count".into(),
                ));
            }
            for profile in profiles {
                native_i64(bytes, profile.type_code);
                let curve = target
                    .model
                    .curves
                    .iter()
                    .find(|curve| curve.id == profile.curve)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "skin references missing profile curve {}",
                            profile.curve
                        ))
                    })?;
                let curve = native_spline_field_curve(
                    &curve.geometry,
                    native_pcurve_knot_domain(profile.data.pcurve.as_ref())?,
                )?;
                native_nurbs_curve(bytes, &curve)?;
                native_skin_profile_data(bytes, target, &profile.data)?;
            }
            native_nurbs_curve(bytes, &native_loft_curve(target, path)?)?;
            for value in tail {
                native_i64(bytes, *value);
            }
        }
        SkinSurfaceLayout::Compact {
            curve,
            subdata,
            first_tail,
            secondary_curve,
            second_tail,
        } => {
            native_nurbs_curve(bytes, &native_loft_curve(target, curve)?)?;
            native_loft_subdata(bytes, subdata)?;
            native_i64(bytes, *first_tail);
            native_nurbs_curve(bytes, &native_loft_curve(target, secondary_curve)?)?;
            native_i64(bytes, *second_tail);
        }
    }
    native_vector(
        bytes,
        [
            construction.direction.x,
            construction.direction.y,
            construction.direction.z,
        ],
    );
    native_f64(bytes, construction.trailing_parameter);
    native_law_formula(bytes, target, &construction.formula)?;
    native_nurbs_curve(
        bytes,
        &native_loft_curve(target, &construction.parameter_curve)?,
    )?;
    native_nurbs_surface(bytes, solved_cache)?;
    native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
    for values in &construction.discontinuities {
        native_compound_loft_float_array(bytes, values)?;
    }
    bytes.push(native_bool(construction.discontinuity_flag));
    bytes.push(0x10);
    Ok(())
}

fn encode_native_net_surface(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::NetSurfaceConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
        CodecError::Malformed("net surface requires a native cache-fit tolerance".into())
    })?;
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "net_spl_sur")?;
    for section in construction.sections.iter() {
        native_loft_section(bytes, target, section, None)?;
    }
    for parameter in construction.frame_parameters {
        native_f64(bytes, parameter);
    }
    native_i64(bytes, construction.flag);
    for direction in construction.directions {
        native_vector(bytes, [direction.x, direction.y, direction.z]);
    }
    for formula in construction.formulas.iter() {
        native_law_formula(bytes, target, formula)?;
    }
    native_nurbs_surface(bytes, solved_cache)?;
    native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
    for values in &construction.discontinuities {
        native_compound_loft_float_array(bytes, values)?;
    }
    bytes.push(native_bool(construction.discontinuity_flag));
    bytes.push(0x10);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn encode_native_sweep_surface(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    profile: &cadmpeg_ir::ids::CurveId,
    spine: &cadmpeg_ir::ids::CurveId,
    construction: &cadmpeg_ir::geometry::SweepSurfaceConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::SweepSurfaceLayout;
    let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
        CodecError::Malformed("sweep surface requires a native cache-fit tolerance".into())
    })?;
    if let Some(form) = &construction.revision_form {
        let SweepSurfaceLayout::ExplicitFormula {
            mode,
            profile_range,
            profile_frame,
            origin,
            directions,
            trajectory_flag,
            path_range,
            path_parameter,
            formula_flag,
            formula,
            trailing_flag,
        } = &construction.layout
        else {
            return Err(CodecError::Malformed(
                "revision-gated sweep generation requires the explicit formula layout".into(),
            ));
        };
        if form.revision <= 0 {
            return Err(CodecError::Malformed(
                "revision-gated sweep requires a positive serializer revision".into(),
            ));
        }
        native_surface_base(bytes, "spline")?;
        bytes.push(0x0f);
        native_ident(bytes, "sweep_sur")?;
        native_i64(bytes, form.revision);
        bytes.push(native_bool(form.primary_flag));
        native_i64(bytes, *mode);
        let profile = native_loft_curve_in_range(target, profile, Some(*profile_range))?;
        native_nurbs_curve(bytes, &profile)?;
        for value in form.profile_endpoints {
            native_optional_f64(bytes, value);
        }
        for value in profile_range {
            native_optional_f64(bytes, Some(*value));
        }
        bytes.push(native_bool(profile_frame.is_some()));
        if let Some((point, direction)) = profile_frame {
            native_point(
                bytes,
                [
                    point.x / LEN_TO_MM,
                    point.y / LEN_TO_MM,
                    point.z / LEN_TO_MM,
                ],
            );
            native_vector(bytes, [direction.x, direction.y, direction.z]);
        }
        native_point(
            bytes,
            [
                origin.x / LEN_TO_MM,
                origin.y / LEN_TO_MM,
                origin.z / LEN_TO_MM,
            ],
        );
        for direction in directions {
            native_vector(bytes, [direction.x, direction.y, direction.z]);
        }
        native_i64(bytes, 1);
        bytes.push(native_bool(*trajectory_flag));
        let native_path_range = [path_range[0] / LEN_TO_MM, path_range[1] / LEN_TO_MM];
        let spine = native_loft_curve_in_range(target, spine, Some(native_path_range))?;
        native_nurbs_curve(bytes, &spine)?;
        for value in form.path_endpoints {
            native_optional_f64(bytes, value);
        }
        for value in path_range {
            native_optional_f64(bytes, Some(*value / LEN_TO_MM));
        }
        native_f64(bytes, *path_parameter);
        bytes.push(native_bool(*formula_flag));
        native_law_formula(bytes, target, formula)?;
        bytes.push(native_bool(*trailing_flag));
        native_enum(bytes, form.tail_enum);
        native_nurbs_surface(bytes, solved_cache)?;
        native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
        for values in &construction.discontinuities {
            native_compound_loft_float_array(bytes, values)?;
        }
        bytes.push(native_bool(construction.discontinuity_flag));
        bytes.push(0x10);
        return Ok(());
    }
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "sweep_spl_sur")?;
    native_enum(bytes, construction.primary_kind);
    match &construction.layout {
        SweepSurfaceLayout::ProfileFirst {
            secondary_kind,
            directions,
            origin,
            parameters,
            formulas,
        } => {
            native_nurbs_curve(bytes, &native_loft_curve(target, profile)?)?;
            native_nurbs_curve(bytes, &native_loft_curve(target, spine)?)?;
            native_enum(bytes, *secondary_kind);
            for direction in directions {
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_point(
                bytes,
                [
                    origin.x / LEN_TO_MM,
                    origin.y / LEN_TO_MM,
                    origin.z / LEN_TO_MM,
                ],
            );
            for parameter in parameters {
                native_f64(bytes, *parameter);
            }
            for formula in formulas.iter() {
                native_law_formula(bytes, target, formula)?;
            }
        }
        SweepSurfaceLayout::ExplicitFormula {
            mode,
            profile_range,
            profile_frame,
            origin,
            directions,
            trajectory_flag,
            path_range,
            path_parameter,
            formula_flag,
            formula,
            trailing_flag,
        } => {
            native_i64(bytes, *mode);
            let profile = native_loft_curve_in_range(target, profile, Some(*profile_range))?;
            native_nurbs_curve(bytes, &profile)?;
            for value in profile_range {
                native_f64(bytes, *value);
            }
            bytes.push(native_bool(profile_frame.is_some()));
            if let Some((point, direction)) = profile_frame {
                native_point(
                    bytes,
                    [
                        point.x / LEN_TO_MM,
                        point.y / LEN_TO_MM,
                        point.z / LEN_TO_MM,
                    ],
                );
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_point(
                bytes,
                [
                    origin.x / LEN_TO_MM,
                    origin.y / LEN_TO_MM,
                    origin.z / LEN_TO_MM,
                ],
            );
            for direction in directions {
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_i64(bytes, 1);
            bytes.push(native_bool(*trajectory_flag));
            let native_path_range = [path_range[0] / LEN_TO_MM, path_range[1] / LEN_TO_MM];
            let spine = native_loft_curve_in_range(target, spine, Some(native_path_range))?;
            native_nurbs_curve(bytes, &spine)?;
            for value in path_range {
                native_f64(bytes, *value / LEN_TO_MM);
            }
            native_f64(bytes, *path_parameter);
            bytes.push(native_bool(*formula_flag));
            native_law_formula(bytes, target, formula)?;
            bytes.push(native_bool(*trailing_flag));
        }
        SweepSurfaceLayout::ExplicitGuide {
            mode,
            profile_range,
            profile_frame,
            origin,
            directions,
            trajectory_flag,
            path_range,
            path_parameter,
            guide_flags,
            guide_curve,
            guide_range,
            guide_modes,
            guide_parameters,
            trailing_flags,
        } => {
            native_i64(bytes, *mode);
            let profile = native_loft_curve_in_range(target, profile, Some(*profile_range))?;
            native_nurbs_curve(bytes, &profile)?;
            for value in profile_range {
                native_f64(bytes, *value);
            }
            bytes.push(native_bool(profile_frame.is_some()));
            if let Some((point, direction)) = profile_frame {
                native_point(
                    bytes,
                    [
                        point.x / LEN_TO_MM,
                        point.y / LEN_TO_MM,
                        point.z / LEN_TO_MM,
                    ],
                );
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_point(
                bytes,
                [
                    origin.x / LEN_TO_MM,
                    origin.y / LEN_TO_MM,
                    origin.z / LEN_TO_MM,
                ],
            );
            for direction in directions {
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_i64(bytes, 2);
            bytes.push(native_bool(*trajectory_flag));
            let native_path_range = [path_range[0] / LEN_TO_MM, path_range[1] / LEN_TO_MM];
            let spine = native_loft_curve_in_range(target, spine, Some(native_path_range))?;
            native_nurbs_curve(bytes, &spine)?;
            for value in path_range {
                native_f64(bytes, *value / LEN_TO_MM);
            }
            native_f64(bytes, *path_parameter);
            for flag in guide_flags {
                bytes.push(native_bool(*flag));
            }
            let guide_curve = native_loft_curve_in_range(target, guide_curve, Some(*guide_range))?;
            native_nurbs_curve(bytes, &guide_curve)?;
            for value in guide_range {
                native_f64(bytes, *value);
            }
            for mode in guide_modes {
                native_i64(bytes, *mode);
            }
            for parameter in guide_parameters {
                native_f64(bytes, *parameter);
            }
            for flag in trailing_flags {
                bytes.push(native_bool(*flag));
            }
        }
        SweepSurfaceLayout::ExplicitSurface {
            mode,
            profile_range,
            profile_frame,
            origin,
            directions,
            trajectory_flag,
            path_range,
            path_parameter,
            singularity,
            support_surface,
            auxiliary_curve,
            support_flag,
            legacy_flag,
        } => {
            native_i64(bytes, *mode);
            let profile = native_loft_curve_in_range(target, profile, Some(*profile_range))?;
            native_nurbs_curve(bytes, &profile)?;
            for value in profile_range {
                native_f64(bytes, *value);
            }
            bytes.push(native_bool(profile_frame.is_some()));
            if let Some((point, direction)) = profile_frame {
                native_point(
                    bytes,
                    [
                        point.x / LEN_TO_MM,
                        point.y / LEN_TO_MM,
                        point.z / LEN_TO_MM,
                    ],
                );
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_point(
                bytes,
                [
                    origin.x / LEN_TO_MM,
                    origin.y / LEN_TO_MM,
                    origin.z / LEN_TO_MM,
                ],
            );
            for direction in directions {
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_i64(bytes, 3);
            bytes.push(native_bool(*trajectory_flag));
            let native_path_range = [path_range[0] / LEN_TO_MM, path_range[1] / LEN_TO_MM];
            let spine = native_loft_curve_in_range(target, spine, Some(native_path_range))?;
            native_nurbs_curve(bytes, &spine)?;
            for value in path_range {
                native_f64(bytes, *value / LEN_TO_MM);
            }
            native_f64(bytes, *path_parameter);
            native_enum(bytes, *singularity);
            let support = target
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == *support_surface)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "sweep references missing support surface {support_surface}"
                    ))
                })?;
            native_embedded_surface(bytes, &support.geometry)?;
            bytes.push(native_bool(auxiliary_curve.is_some()));
            if let Some(curve) = auxiliary_curve {
                native_nurbs_curve(bytes, &native_loft_curve(target, curve)?)?;
            }
            bytes.push(native_bool(*support_flag));
            if let Some(flag) = legacy_flag {
                bytes.push(native_bool(*flag));
            }
        }
        SweepSurfaceLayout::LawDriven {
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
            path_range,
            path_parameter,
            second_law_flag,
            second_law,
            formula_mode,
            formula,
            trailing_flag,
        } => {
            native_i64(bytes, *mode);
            let profile = native_loft_curve_in_range(target, profile, Some(*profile_range))?;
            native_nurbs_curve(bytes, &profile)?;
            for value in profile_range {
                native_f64(bytes, *value);
            }
            bytes.push(native_bool(profile_frame.is_some()));
            if let Some((point, direction)) = profile_frame {
                native_point(
                    bytes,
                    [
                        point.x / LEN_TO_MM,
                        point.y / LEN_TO_MM,
                        point.z / LEN_TO_MM,
                    ],
                );
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_point(
                bytes,
                [
                    origin.x / LEN_TO_MM,
                    origin.y / LEN_TO_MM,
                    origin.z / LEN_TO_MM,
                ],
            );
            for direction in directions {
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_law_expression(bytes, target, first_law, 0)?;
            native_i64(bytes, *first_mode);
            for value in first_range {
                native_f64(bytes, *value);
            }
            native_vector(bytes, [law_direction.x, law_direction.y, law_direction.z]);
            native_i64(bytes, *path_mode);
            bytes.push(native_bool(*path_flag));
            let spine = native_loft_curve_in_range(target, spine, Some(*path_range))?;
            native_nurbs_curve(bytes, &spine)?;
            for value in path_range {
                native_f64(bytes, *value);
            }
            native_f64(bytes, *path_parameter);
            bytes.push(native_bool(*second_law_flag));
            native_law_expression(bytes, target, second_law, 0)?;
            native_i64(bytes, *formula_mode);
            native_law_formula(bytes, target, formula)?;
            bytes.push(native_bool(*trailing_flag));
        }
    }
    native_nurbs_surface(bytes, solved_cache)?;
    native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
    for values in &construction.discontinuities {
        native_compound_loft_float_array(bytes, values)?;
    }
    bytes.push(native_bool(construction.discontinuity_flag));
    bytes.push(0x10);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn encode_native_loft(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    sections: &[cadmpeg_ir::geometry::LoftSection; 2],
    revision_form: Option<&cadmpeg_ir::geometry::LoftRevisionForm>,
    parameters: &cadmpeg_ir::geometry::SplineSurfaceParameters,
    closures: &[i64; 2],
    singularities: &[i64; 2],
    mode: i64,
    bridge: &[cadmpeg_ir::geometry::LoftBridgeToken],
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    if let Some(form) = revision_form {
        let cadmpeg_ir::geometry::SplineSurfaceParameters::RevisionValues { values } = parameters
        else {
            return Err(CodecError::Malformed(
                "revision-gated loft requires revision-native parameter values".into(),
            ));
        };
        if form.revision <= 0 {
            return Err(CodecError::Malformed(
                "revision-gated loft requires a positive serializer revision".into(),
            ));
        }
        native_surface_base(bytes, "spline")?;
        bytes.push(0x0f);
        native_ident(bytes, "loft_spl_sur")?;
        native_i64(bytes, form.revision);
        for section in sections {
            native_loft_section(bytes, target, section, None)?;
        }
        for value in values {
            native_optional_f64(bytes, *value);
        }
        for flag in form.flags {
            bytes.push(native_bool(flag));
        }
        for value in form.ints {
            native_i64(bytes, value);
        }
        native_enum(bytes, form.tail_enum);
        native_nurbs_surface(bytes, solved_cache)?;
        native_f64(
            bytes,
            procedural.cache_fit_tolerance.unwrap_or(0.0) / LEN_TO_MM,
        );
        for discontinuities in &form.discontinuities {
            native_i64(
                bytes,
                i64::try_from(discontinuities.len()).map_err(|_| {
                    CodecError::NotImplemented("discontinuity count exceeds i64".into())
                })?,
            );
            for value in discontinuities {
                native_f64(bytes, *value);
            }
        }
        bytes.push(native_bool(form.tail_flag));
        bytes.push(0x10);
        return Ok(());
    }
    native_surface_base(bytes, "spline")?;
    let cadmpeg_ir::geometry::SplineSurfaceParameters::OrderedRanges { ranges } = parameters else {
        return Err(CodecError::Malformed(
            "legacy loft requires ordered parameter ranges".into(),
        ));
    };
    bytes.push(0x0f);
    native_ident(bytes, "loft_spl_sur")?;
    for (section, range) in sections.iter().zip(ranges) {
        native_loft_section(bytes, target, section, Some(*range))?;
    }
    for range in ranges {
        native_f64(bytes, range[0]);
        native_f64(bytes, range[1]);
    }
    for closure in closures {
        native_enum(bytes, *closure);
    }
    for singularity in singularities {
        native_enum(bytes, *singularity);
    }
    native_i64(bytes, mode);
    for token in bridge {
        match token {
            cadmpeg_ir::geometry::LoftBridgeToken::Boolean(value) => {
                bytes.push(native_bool(*value));
            }
            cadmpeg_ir::geometry::LoftBridgeToken::Integer(value) => native_i64(bytes, *value),
            cadmpeg_ir::geometry::LoftBridgeToken::Double(value) => native_f64(bytes, *value),
            cadmpeg_ir::geometry::LoftBridgeToken::Text(value) => native_string(bytes, value)?,
            cadmpeg_ir::geometry::LoftBridgeToken::Enum(value) => native_enum(bytes, *value),
        }
    }
    native_nurbs_surface(bytes, solved_cache)?;
    if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
        native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
    }
    bytes.push(0x10);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn encode_native_extrusion(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    directrix: &cadmpeg_ir::ids::CurveId,
    parameter_interval: [f64; 2],
    direction: Vector3,
    native_position: cadmpeg_ir::math::Point3,
    solved_cache: Option<&NurbsSurface>,
) -> Result<(), CodecError> {
    let directrix = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *directrix)
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "procedural surface {} references missing directrix {directrix}",
                procedural.id
            ))
        })?;
    let directrix_cache = native_interval_curve(&directrix.geometry, parameter_interval)?;
    if [
        parameter_interval[0],
        parameter_interval[1],
        direction.x,
        direction.y,
        direction.z,
        native_position.x,
        native_position.y,
        native_position.z,
    ]
    .into_iter()
    .any(|component| !component.is_finite())
    {
        return Err(CodecError::Malformed(
            "source-less extrusion fields must be finite".into(),
        ));
    }
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "cyl_spl_sur")?;
    native_f64(bytes, parameter_interval[0]);
    native_f64(bytes, parameter_interval[1]);
    native_vector(
        bytes,
        [
            direction.x / LEN_TO_MM,
            direction.y / LEN_TO_MM,
            direction.z / LEN_TO_MM,
        ],
    );
    native_point(
        bytes,
        [
            native_position.x / LEN_TO_MM,
            native_position.y / LEN_TO_MM,
            native_position.z / LEN_TO_MM,
        ],
    );
    native_nurbs_curve(bytes, &directrix_cache)?;
    if let Some(solved_cache) = solved_cache {
        native_nurbs_surface(bytes, solved_cache)?;
        if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
            native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
        }
    } else if procedural.cache_fit_tolerance.is_some() {
        return Err(CodecError::Malformed(
            "cache-less F3D extrusion cannot carry a cache-fit tolerance".into(),
        ));
    }
    bytes.push(0x10);
    Ok(())
}

fn native_optional_pcurve(
    bytes: &mut Vec<u8>,
    pcurve: Option<&PcurveGeometry>,
) -> Result<(), CodecError> {
    if let Some(pcurve) = pcurve {
        native_nurbs_pcurve_block(bytes, pcurve)
    } else {
        native_ident(bytes, "nullbs")
    }
}

fn native_variable_blend_value(
    bytes: &mut Vec<u8>,
    value: &cadmpeg_ir::geometry::VariableBlendValue,
    depth: usize,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::{LoftBridgeToken, VariableBlendValuePayload};
    if depth > 32 {
        return Err(CodecError::Malformed(
            "variable blend-value recursion exceeds 32 levels".into(),
        ));
    }
    native_string(bytes, &value.name)?;
    if value.discriminator != 1 {
        native_i64(bytes, value.discriminator);
    }
    native_enum(bytes, value.calibrated);
    bytes.push(native_bool(value.modern_flag));
    match &value.payload {
        VariableBlendValuePayload::TwoEnds { parameters, radii } => {
            for parameter in parameters {
                native_f64(bytes, *parameter);
            }
            for radius in radii {
                native_f64(bytes, *radius / LEN_TO_MM);
            }
        }
        VariableBlendValuePayload::FixedWidth { parameters, width } => {
            native_f64(bytes, parameters[0]);
            native_f64(bytes, parameters[1]);
            native_f64(bytes, *width);
        }
        VariableBlendValuePayload::EdgeOffset { scalars, lengths } => {
            let expected = if value.discriminator == 0 {
                (2, 1)
            } else {
                (1, 2)
            };
            if (scalars.len(), lengths.len()) != expected {
                return Err(CodecError::Malformed(
                    "variable edge-offset payload has inconsistent arity".into(),
                ));
            }
            for scalar in scalars {
                native_f64(bytes, *scalar);
            }
            for length in lengths {
                native_f64(bytes, *length / LEN_TO_MM);
            }
        }
        VariableBlendValuePayload::Functional {
            parameter,
            radius,
            function,
            terminal,
        } => {
            native_f64(bytes, *parameter);
            native_f64(bytes, *radius / LEN_TO_MM);
            native_nurbs_pcurve_block(bytes, function)?;
            match terminal {
                LoftBridgeToken::Double(value) => native_f64(bytes, *value),
                LoftBridgeToken::Text(value) => native_string(bytes, value)?,
                _ => {
                    return Err(CodecError::NotImplemented(
                        "functional variable-blend terminal must be double or text".into(),
                    ));
                }
            }
        }
        VariableBlendValuePayload::Constant {
            parameters,
            radius,
            variable_chamfer,
            chamfer_type,
            nested,
        } => {
            for parameter in parameters {
                native_f64(bytes, *parameter);
            }
            native_f64(bytes, *radius / LEN_TO_MM);
            native_enum(bytes, *variable_chamfer);
            native_enum(bytes, *chamfer_type);
            native_variable_blend_value(bytes, nested, depth + 1)?;
        }
        VariableBlendValuePayload::Interpolated {
            parameter,
            radius,
            function,
            enum_count,
            enum_tagged,
            points,
            tail,
        } => {
            native_f64(bytes, *parameter);
            native_f64(bytes, *radius / LEN_TO_MM);
            native_nurbs_pcurve_block(bytes, function)?;
            if *enum_tagged {
                native_enum(bytes, *enum_count);
            } else {
                native_i64(bytes, *enum_count);
            }
            native_i64(
                bytes,
                i64::try_from(points.len()).map_err(|_| {
                    CodecError::NotImplemented("variable blend point count exceeds i64".into())
                })?,
            );
            for point in points {
                native_f64(bytes, point.parameter);
                native_f64(bytes, point.radius / LEN_TO_MM);
                for tangent in point.tangents {
                    native_f64(bytes, tangent);
                }
                native_point(
                    bytes,
                    [
                        point.location.x / LEN_TO_MM,
                        point.location.y / LEN_TO_MM,
                        point.location.z / LEN_TO_MM,
                    ],
                );
                native_vector(bytes, [point.normal.x, point.normal.y, point.normal.z]);
            }
            if *enum_tagged {
                native_enum(bytes, i64::from(tail.is_some()));
            } else {
                native_i64(bytes, i64::from(tail.is_some()));
            }
            if let Some(tail) = tail {
                native_f64(bytes, tail[0]);
                native_f64(bytes, tail[1]);
            }
        }
    }
    Ok(())
}

fn native_vertex_blend_bool(bytes: &mut Vec<u8>, value: i64) -> Result<(), CodecError> {
    match value {
        0 => bytes.push(native_bool(false)),
        1 => bytes.push(native_bool(true)),
        _ => {
            return Err(CodecError::Malformed(
                "vertex-blend boolean enum must be 0 or 1".into(),
            ));
        }
    }
    Ok(())
}

fn native_vertex_blend_boundary(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    boundary: &cadmpeg_ir::geometry::VertexBlendBoundary,
    revision: bool,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::VertexBlendBoundaryGeometry;
    let kind = match &boundary.geometry {
        VertexBlendBoundaryGeometry::Circle { .. } => "circle",
        VertexBlendBoundaryGeometry::Degenerate { .. } => "deg",
        VertexBlendBoundaryGeometry::Pcurve { .. } => "pcurve",
        VertexBlendBoundaryGeometry::Plane { .. } => "plane",
    };
    if revision {
        native_ident(bytes, kind)?;
    } else {
        native_string(bytes, kind)?;
    }
    native_vertex_blend_bool(bytes, boundary.boundary_type)?;
    if revision {
        native_vector(
            bytes,
            [
                boundary.magic.x / LEN_TO_MM,
                boundary.magic.y / LEN_TO_MM,
                boundary.magic.z / LEN_TO_MM,
            ],
        );
    } else {
        native_point(
            bytes,
            [
                boundary.magic.x / LEN_TO_MM,
                boundary.magic.y / LEN_TO_MM,
                boundary.magic.z / LEN_TO_MM,
            ],
        );
    }
    native_vertex_blend_bool(bytes, boundary.u_smoothing)?;
    native_vertex_blend_bool(bytes, boundary.v_smoothing)?;
    native_f64(bytes, boundary.fullness);
    match &boundary.geometry {
        VertexBlendBoundaryGeometry::Circle {
            curve,
            curve_endpoints,
            form,
            twists,
            parameters,
            sense,
        } => {
            let expected_twists = match form {
                0 => 0,
                1 => 1,
                3 => 2,
                _ => {
                    return Err(CodecError::Malformed(
                        "vertex-blend circle form must be 0, 1, or 3".into(),
                    ));
                }
            };
            if twists.len() != expected_twists {
                return Err(CodecError::Malformed(
                    "vertex-blend circle twist count conflicts with its form".into(),
                ));
            }
            let range = if revision { None } else { Some(*parameters) };
            let curve = native_loft_curve_in_range(target, curve, range)?;
            native_nurbs_curve(bytes, &curve)?;
            if revision {
                for endpoint in curve_endpoints {
                    native_optional_f64(bytes, *endpoint);
                }
            }
            native_enum(bytes, *form);
            for twist in twists {
                if revision {
                    native_vector(
                        bytes,
                        [
                            twist.x / LEN_TO_MM,
                            twist.y / LEN_TO_MM,
                            twist.z / LEN_TO_MM,
                        ],
                    );
                } else {
                    native_point(
                        bytes,
                        [
                            twist.x / LEN_TO_MM,
                            twist.y / LEN_TO_MM,
                            twist.z / LEN_TO_MM,
                        ],
                    );
                }
            }
            native_f64(bytes, parameters[0]);
            native_f64(bytes, parameters[1]);
            native_vertex_blend_bool(bytes, *sense)?;
        }
        VertexBlendBoundaryGeometry::Degenerate { location, normals } => {
            native_point(
                bytes,
                [
                    location.x / LEN_TO_MM,
                    location.y / LEN_TO_MM,
                    location.z / LEN_TO_MM,
                ],
            );
            for normal in normals {
                native_vector(bytes, [normal.x, normal.y, normal.z]);
            }
        }
        VertexBlendBoundaryGeometry::Pcurve {
            surface,
            support_bounds,
            pcurve,
            sense,
            fit_tolerance,
        } => {
            let surface = target
                .model
                .surfaces
                .iter()
                .find(|candidate| candidate.id == *surface)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("vertex-blend support {surface} is missing"))
                })?;
            if revision {
                native_embedded_surface_with_bounds(bytes, &surface.geometry, support_bounds)?;
            } else {
                native_embedded_surface(bytes, &surface.geometry)?;
            }
            native_optional_pcurve(bytes, pcurve.as_ref())?;
            native_vertex_blend_bool(bytes, *sense)?;
            native_f64(bytes, *fit_tolerance);
        }
        VertexBlendBoundaryGeometry::Plane {
            normal,
            parameters,
            curve,
            curve_endpoints,
        } => {
            native_vector(bytes, [normal.x, normal.y, normal.z]);
            native_f64(bytes, parameters[0]);
            native_f64(bytes, parameters[1]);
            let range = if revision { None } else { Some(*parameters) };
            let curve = native_loft_curve_in_range(target, curve, range)?;
            native_nurbs_curve(bytes, &curve)?;
            if revision {
                for endpoint in curve_endpoints {
                    native_optional_f64(bytes, *endpoint);
                }
            }
        }
    }
    Ok(())
}

fn encode_native_vertex_blend(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    construction: &cadmpeg_ir::geometry::VertexBlendConstruction,
) -> Result<(), CodecError> {
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "VBL_SURF")?;
    if let Some(revision) = construction.revision {
        if revision <= 0 {
            return Err(CodecError::Malformed(
                "revision-gated VBL_SURF requires a positive revision".into(),
            ));
        }
        native_i64(bytes, revision);
    }
    native_i64(
        bytes,
        i64::try_from(construction.boundaries.len()).map_err(|_| {
            CodecError::NotImplemented("vertex-blend boundary count exceeds i64".into())
        })?,
    );
    for boundary in &construction.boundaries {
        native_vertex_blend_boundary(bytes, target, boundary, construction.revision.is_some())?;
    }
    native_i64(bytes, construction.grid_size);
    native_f64(bytes, construction.fit_tolerance / LEN_TO_MM);
    bytes.push(0x10);
    Ok(())
}

/// Emit one revision-gated compound-loft scale block: counted profile
/// members, nullable path curve with optional endpoints, counted auxiliary
/// curves, and the tail integer.
fn native_revision_cl_scale(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    profile: &[cadmpeg_ir::geometry::LoftProfileMember],
    path: &cadmpeg_ir::geometry::LoftPath,
) -> Result<(), CodecError> {
    native_i64(
        bytes,
        i64::try_from(profile.len()).map_err(|_| {
            CodecError::NotImplemented("compound-loft member count exceeds i64".into())
        })?,
    );
    for member in profile {
        native_i64(bytes, member.type_code);
        let curve = native_loft_curve_in_range(target, &member.curve, None)?;
        native_nurbs_curve(bytes, &curve)?;
        let endpoints = member.endpoints.ok_or_else(|| {
            CodecError::Malformed(
                "revision compound-loft members require optional endpoints".into(),
            )
        })?;
        for value in endpoints {
            native_optional_f64(bytes, value);
        }
        if let Some(surface_id) = &member.data.surface {
            let surface = target
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == *surface_id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "compound loft references missing surface {surface_id}"
                    ))
                })?;
            native_embedded_surface_with_bounds(
                bytes,
                &surface.geometry,
                &member.data.support_bounds,
            )?;
        } else {
            native_ident(bytes, "null_surface")?;
        }
        if let Some(pcurve) = &member.data.pcurve {
            native_nurbs_pcurve_block(bytes, pcurve)?;
        } else {
            native_ident(bytes, "nullbs")?;
        }
        bytes.push(native_bool(member.data.first_flag));
        native_i64(bytes, member.data.asm_extension);
        native_loft_subdata(bytes, &member.data.subdata)?;
        bytes.push(native_bool(member.data.direction.is_some()));
        if let Some(direction) = member.data.direction {
            native_vector(bytes, [direction.x, direction.y, direction.z]);
        }
    }
    if let Some(path_curve) = &path.curve {
        let curve = native_loft_curve_in_range(target, path_curve, None)?;
        native_nurbs_curve(bytes, &curve)?;
        if let Some(endpoints) = path.endpoints {
            for value in endpoints {
                native_optional_f64(bytes, value);
            }
        }
    } else {
        native_ident(bytes, "null_curve")?;
    }
    native_i64(
        bytes,
        i64::try_from(path.auxiliaries.len()).map_err(|_| {
            CodecError::NotImplemented("compound-loft auxiliary count exceeds i64".into())
        })?,
    );
    for auxiliary in &path.auxiliaries {
        let auxiliary = native_loft_curve_in_range(target, auxiliary, None)?;
        native_nurbs_curve(bytes, &auxiliary)?;
    }
    native_i64(bytes, path.flag);
    Ok(())
}

fn encode_native_revision_compound_loft(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::RevisionCompoundLoftConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    if construction.revision <= 0 {
        return Err(CodecError::Malformed(
            "revision-gated cl_loft_spl_sur requires a positive revision".into(),
        ));
    }
    if construction.kind != 0 {
        return Err(CodecError::NotImplemented(
            "revision-gated cl_loft_spl_sur defines only the kind-zero payload".into(),
        ));
    }
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "cl_loft_spl_sur")?;
    native_i64(bytes, construction.revision);
    native_enum(bytes, construction.tail_enum);
    native_nurbs_surface(bytes, solved_cache)?;
    native_f64(
        bytes,
        procedural.cache_fit_tolerance.unwrap_or(0.0) / LEN_TO_MM,
    );
    for discontinuities in &construction.discontinuities {
        native_i64(
            bytes,
            i64::try_from(discontinuities.len()).map_err(|_| {
                CodecError::NotImplemented("discontinuity count exceeds i64".into())
            })?,
        );
        for value in discontinuities {
            native_f64(bytes, *value);
        }
    }
    bytes.push(native_bool(construction.tail_flag));
    native_revision_cl_scale(
        bytes,
        target,
        &construction.base_profile,
        &construction.base_path,
    )?;
    native_i64(
        bytes,
        i64::try_from(construction.entries.len()).map_err(|_| {
            CodecError::NotImplemented("compound-loft entry count exceeds i64".into())
        })?,
    );
    for entry in &construction.entries {
        native_revision_cl_scale(bytes, target, &entry.profile, &entry.path)?;
        native_f64(bytes, entry.parameter);
    }
    for flag in construction.flags {
        bytes.push(native_bool(flag));
    }
    native_i64(bytes, construction.kind);
    for flag in construction.kind_flags {
        bytes.push(native_bool(flag));
    }
    native_i64(bytes, construction.selector);
    match (
        construction.selector,
        &construction.direction,
        &construction.direction_curve,
    ) {
        (0, Some(direction), None) => {
            native_vector(bytes, [direction.x, direction.y, direction.z]);
        }
        (selector, None, Some(curve)) if selector != 0 => {
            let curve = native_loft_curve_in_range(target, curve, None)?;
            native_nurbs_curve(bytes, &curve)?;
        }
        _ => {
            return Err(CodecError::Malformed(
                "compound-loft direction conflicts with its selector".into(),
            ));
        }
    }
    for value in construction.interval {
        native_optional_f64(bytes, value);
    }
    if let Some(curve) = &construction.trailing_curve {
        let curve = native_loft_curve_in_range(target, curve, None)?;
        native_nurbs_curve(bytes, &curve)?;
    }
    bytes.push(0x10);
    Ok(())
}

fn encode_native_revision_g2_blend(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::RevisionG2BlendConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    if construction.revision <= 0 {
        return Err(CodecError::Malformed(
            "revision-gated g2_blend_spl_sur requires a positive revision".into(),
        ));
    }
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "g2_blend_spl_sur")?;
    native_i64(bytes, construction.revision);
    for parameter in construction.leading_parameters {
        native_f64(bytes, parameter);
    }
    for side in construction.sides.iter() {
        native_rolling_ball_side(bytes, target, side)?;
    }
    let center_range = match construction.center_range {
        [Some(lower), Some(upper)] => Some([lower, upper]),
        _ => None,
    };
    let center = native_loft_curve_in_range(target, &construction.center, center_range)?;
    native_nurbs_curve(bytes, &center)?;
    for endpoint in construction.center_range {
        native_optional_f64(bytes, endpoint);
    }
    for radius in construction.radii {
        native_f64(bytes, radius / LEN_TO_MM);
    }
    native_enum(bytes, construction.radius_selector);
    for range in [construction.u_range, construction.v_range] {
        for endpoint in range {
            native_optional_f64(bytes, endpoint);
        }
    }
    native_i64(bytes, construction.shape_prefix);
    native_f64(bytes, construction.shape_parameter);
    native_f64(bytes, construction.shape_length / LEN_TO_MM);
    native_i64(bytes, construction.shape_tail);
    native_enum(bytes, construction.tail_enum);
    native_nurbs_surface(bytes, solved_cache)?;
    native_f64(
        bytes,
        procedural.cache_fit_tolerance.unwrap_or(0.0) / LEN_TO_MM,
    );
    for discontinuities in &construction.discontinuities {
        native_i64(
            bytes,
            i64::try_from(discontinuities.len()).map_err(|_| {
                CodecError::NotImplemented("discontinuity count exceeds i64".into())
            })?,
        );
        for value in discontinuities {
            native_f64(bytes, *value);
        }
    }
    bytes.push(native_bool(construction.tail_flag));
    for extension in construction.tail_extensions {
        native_i64(bytes, extension);
    }
    bytes.push(0x10);
    Ok(())
}

fn encode_native_variable_blend(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::VariableBlendConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::LoftBridgeToken;
    let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
        CodecError::Malformed("variable blend requires a native cache-fit tolerance".into())
    })?;
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "srf_srf_v_bl_spl_sur")?;
    native_i64(bytes, construction.revision);
    for side in construction.sides.iter() {
        native_rolling_ball_side(bytes, target, side)?;
    }
    let slice_range = match construction.slice_range {
        [Some(lower), Some(upper)] => Some([lower, upper]),
        _ => match construction.u_range {
            [Some(lower), Some(upper)] => Some([lower, upper]),
            _ => None,
        },
    };
    let slice = native_loft_curve_in_range(target, &construction.slice, slice_range)?;
    native_nurbs_curve(bytes, &slice)?;
    for endpoint in construction.slice_range {
        bytes.push(native_bool(endpoint.is_some()));
        if let Some(value) = endpoint {
            native_f64(bytes, value);
        }
    }
    for offset in construction.offsets {
        native_f64(bytes, offset / LEN_TO_MM);
    }
    native_enum(
        bytes,
        match construction.radius_kind {
            cadmpeg_ir::geometry::VariableBlendRadiusKind::SingleRadius => 0,
            cadmpeg_ir::geometry::VariableBlendRadiusKind::TwoRadii => 1,
        },
    );
    native_variable_blend_value(bytes, &construction.first_value, 0)?;
    if matches!(
        construction.radius_kind,
        cadmpeg_ir::geometry::VariableBlendRadiusKind::TwoRadii
    ) {
        if construction.single_radius_tail.is_some() {
            return Err(CodecError::Malformed(
                "two-radii variable blend carries a single-radius tail".into(),
            ));
        }
        let second = construction.second_value.as_ref().ok_or_else(|| {
            CodecError::Malformed("two-radii variable blend lacks its second value".into())
        })?;
        native_variable_blend_value(bytes, second, 0)?;
        match (construction.chamfer_selector, &construction.chamfer) {
            (Some(0), None) => native_enum(bytes, 0),
            (Some(3) | None, Some(chamfer)) => {
                native_enum(
                    bytes,
                    match chamfer.kind {
                        cadmpeg_ir::geometry::VariableBlendChamferKind::Rounded => 3,
                    },
                );
                native_enum(bytes, chamfer.chamfer_type);
                native_variable_blend_value(bytes, &chamfer.value, 0)?;
            }
            (None, None) => {}
            _ => {
                return Err(CodecError::Malformed(
                    "variable-blend chamfer selector conflicts with its chamfer payload".into(),
                ));
            }
        }
    } else {
        if construction.second_value.is_some() || construction.chamfer.is_some() {
            return Err(CodecError::Malformed(
                "single-radius variable blend carries two-radii payloads".into(),
            ));
        }
        match (
            construction.single_radius_selector,
            &construction.single_radius_tail,
        ) {
            (Some(0), None) => native_enum(bytes, 0),
            (selector, Some(tail)) => {
                let value = match &tail.selector {
                    LoftBridgeToken::Integer(value) => *value,
                    _ => {
                        return Err(CodecError::NotImplemented(
                            "variable single-radius selector must be an integer".into(),
                        ));
                    }
                };
                if selector.is_some_and(|stored| stored != value) {
                    return Err(CodecError::Malformed(
                        "variable-blend single-radius selector conflicts with its tail".into(),
                    ));
                }
                native_enum(bytes, value);
                for parameter in tail.parameters {
                    native_f64(bytes, parameter);
                }
            }
            (None, None) => {}
            _ => {
                return Err(CodecError::Malformed(
                    "variable-blend single-radius selector conflicts with its tail".into(),
                ));
            }
        }
    }
    for range in [construction.u_range, construction.v_range] {
        for endpoint in range {
            bytes.push(native_bool(endpoint.is_some()));
            if let Some(value) = endpoint {
                native_f64(bytes, value);
            }
        }
    }
    native_i64(bytes, construction.shape_prefix);
    native_f64(bytes, construction.shape_parameter);
    native_f64(bytes, construction.shape_length / LEN_TO_MM);
    native_i64(bytes, construction.shape_tail);
    native_enum(bytes, construction.cache_selector);
    native_nurbs_surface(bytes, solved_cache)?;
    native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
    for values in &construction.discontinuities {
        native_i64(
            bytes,
            i64::try_from(values.len()).map_err(|_| {
                CodecError::NotImplemented("variable-blend discontinuity count exceeds i64".into())
            })?,
        );
        for value in values {
            native_f64(bytes, *value);
        }
    }
    bytes.push(native_bool(construction.tail_flag));
    for extension in construction.tail_extensions {
        native_i64(bytes, extension);
    }
    if let Some(secondary) = &construction.secondary_curve {
        let secondary_range = match construction.secondary_range {
            [Some(lower), Some(upper)] => Some([lower, upper]),
            _ => None,
        };
        let secondary = native_loft_curve_in_range(target, secondary, secondary_range)?;
        native_nurbs_curve(bytes, &secondary)?;
        for endpoint in construction.secondary_range {
            bytes.push(native_bool(endpoint.is_some()));
            if let Some(value) = endpoint {
                native_f64(bytes, value);
            }
        }
    } else {
        native_ident(bytes, "null_curve")?;
    }
    bytes.push(native_bool(matches!(
        construction.convexity,
        cadmpeg_ir::geometry::VariableBlendConvexity::Convex
    )));
    bytes.push(native_bool(matches!(
        construction.render_mode,
        cadmpeg_ir::geometry::VariableBlendRenderMode::RollingBallEnvelope
    )));
    for endpoint in construction.post_range {
        bytes.push(native_bool(endpoint.is_some()));
        if let Some(value) = endpoint {
            native_f64(bytes, value);
        }
    }
    if let Some(post_curve) = &construction.post_curve {
        let post_range = match construction.post_range {
            [Some(lower), Some(upper)] => Some([lower, upper]),
            _ => None,
        };
        let post_curve = native_loft_curve_in_range(target, post_curve, post_range)?;
        native_nurbs_curve(bytes, &post_curve)?;
    } else {
        native_ident(bytes, "nullbs")?;
    }
    native_optional_pcurve(bytes, construction.post_pcurve.as_ref())?;
    bytes.push(0x10);
    Ok(())
}

fn native_rolling_ball_side(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    side: &cadmpeg_ir::geometry::RollingBallSide,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::VariableBlendSupportKind;
    native_string(
        bytes,
        match side.support_kind {
            VariableBlendSupportKind::CosineCurve => "blend_support_cos_curve",
            VariableBlendSupportKind::Curve => "blend_support_curve",
            VariableBlendSupportKind::PointCurve => "blend_support_point_curve",
            VariableBlendSupportKind::Surface => "blend_support_surface",
            VariableBlendSupportKind::ZeroCurve => "blend_support_zero_curve",
        },
    )?;
    if let Some(id) = &side.surface {
        let surface = target
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *id)
            .ok_or_else(|| {
                CodecError::Malformed(format!("rolling-ball support {id} is missing"))
            })?;
        native_embedded_surface(bytes, &surface.geometry)?;
        if matches!(
            surface.geometry,
            SurfaceGeometry::Cylinder { .. }
                | SurfaceGeometry::Cone { .. }
                | SurfaceGeometry::Sphere { .. }
                | SurfaceGeometry::Torus { .. }
        ) {
            bytes.truncate(bytes.len() - 4);
        }
        for range in side.surface_ranges {
            for endpoint in range {
                bytes.push(native_bool(endpoint.is_some()));
                if let Some(value) = endpoint {
                    native_f64(bytes, value);
                }
            }
        }
    } else {
        native_ident(bytes, "null_surface")?;
    }
    if let Some(id) = &side.curve {
        let curve = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *id)
            .ok_or_else(|| {
                CodecError::Malformed(format!("rolling-ball side curve {id} is missing"))
            })?;
        let curve = native_spline_field_curve(
            &curve.geometry,
            match side.curve_range {
                [Some(lower), Some(upper)] => Some([lower, upper]),
                _ => native_pcurve_knot_domain(side.pcurve.as_ref())?,
            },
        )?;
        native_nurbs_curve(bytes, &curve)?;
        for endpoint in side.curve_range {
            bytes.push(native_bool(endpoint.is_some()));
            if let Some(value) = endpoint {
                native_f64(bytes, value);
            }
        }
    } else {
        native_ident(bytes, "null_curve")?;
    }
    native_optional_pcurve(bytes, side.pcurve.as_ref())?;
    native_point(
        bytes,
        [
            side.location.x / LEN_TO_MM,
            side.location.y / LEN_TO_MM,
            side.location.z / LEN_TO_MM,
        ],
    );
    native_optional_pcurve(bytes, side.secondary_pcurve.as_ref())?;
    if let Some(extension) = side.extension {
        native_i64(bytes, extension);
        native_optional_pcurve(bytes, side.tertiary_pcurve.as_ref())?;
    } else if side.tertiary_pcurve.is_some() {
        return Err(CodecError::Malformed(
            "rolling-ball tertiary pcurve requires an extension integer".into(),
        ));
    }
    Ok(())
}

fn native_rolling_ball_third_side(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    side: &cadmpeg_ir::geometry::RollingBallThirdSide,
) -> Result<(), CodecError> {
    native_string(bytes, &side.label)?;
    let surface = target
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == side.surface)
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "rolling-ball third support {} is missing",
                side.surface
            ))
        })?;
    native_embedded_surface(bytes, &surface.geometry)?;
    let curve = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == side.curve)
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "rolling-ball third-side curve {} is missing",
                side.curve
            ))
        })?;
    let pcurve = [
        side.pcurve.as_ref(),
        side.secondary_pcurve.as_ref(),
        side.tertiary_pcurve.as_ref(),
    ]
    .into_iter()
    .flatten()
    .find(|pcurve| matches!(pcurve, PcurveGeometry::Nurbs { .. }));
    let curve = native_spline_field_curve(&curve.geometry, native_pcurve_knot_domain(pcurve)?)?;
    native_nurbs_curve(bytes, &curve)?;
    native_optional_pcurve(bytes, side.pcurve.as_ref())?;
    native_vector(
        bytes,
        [side.direction.x, side.direction.y, side.direction.z],
    );
    native_optional_pcurve(bytes, side.secondary_pcurve.as_ref())?;
    native_i64(bytes, side.extension);
    native_optional_pcurve(bytes, side.tertiary_pcurve.as_ref())?;
    bytes.push(native_bool(side.flag));
    Ok(())
}

fn encode_complete_native_rolling_ball(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::RollingBallConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
        CodecError::Malformed("rolling-ball blend requires a native cache-fit tolerance".into())
    })?;
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(
        bytes,
        if construction.third.is_some() {
            "sss_blend_spl_sur"
        } else {
            "rb_blend_spl_sur"
        },
    )?;
    native_i64(bytes, construction.definition_index);
    for side in construction.sides.iter() {
        native_rolling_ball_side(bytes, target, side)?;
    }
    let slice_range = match construction.slice_range {
        [Some(lower), Some(upper)] => Some([lower, upper]),
        _ => match construction.u_range {
            [Some(lower), Some(upper)] => Some([lower, upper]),
            _ => None,
        },
    };
    let slice = native_loft_curve_in_range(target, &construction.slice, slice_range)?;
    native_nurbs_curve(bytes, &slice)?;
    for endpoint in construction.slice_range {
        bytes.push(native_bool(endpoint.is_some()));
        if let Some(value) = endpoint {
            native_f64(bytes, value);
        }
    }
    for offset in construction.offsets {
        native_f64(bytes, offset / LEN_TO_MM);
    }
    match construction.radius_selector {
        cadmpeg_ir::geometry::RollingBallRadiusSelector::None => native_enum(bytes, -1),
        cadmpeg_ir::geometry::RollingBallRadiusSelector::Value { value } => {
            native_f64(bytes, value);
        }
    }
    for range in [construction.u_range, construction.v_range] {
        for endpoint in range {
            bytes.push(native_bool(endpoint.is_some()));
            if let Some(value) = endpoint {
                native_f64(bytes, value);
            }
        }
    }
    native_i64(bytes, construction.shape_prefix);
    for parameter in construction.parameters {
        native_f64(bytes, parameter);
    }
    native_i64(bytes, construction.tail);
    native_enum(bytes, construction.cache_selector);
    native_nurbs_surface(bytes, solved_cache)?;
    native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
    for values in &construction.discontinuities {
        native_i64(
            bytes,
            i64::try_from(values.len()).map_err(|_| {
                CodecError::NotImplemented("rolling-ball discontinuity count exceeds i64".into())
            })?,
        );
        for value in values {
            native_f64(bytes, *value);
        }
    }
    if let Some(third) = &construction.third {
        native_rolling_ball_third_side(bytes, target, third)?;
    }
    bytes.push(0x10);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn encode_native_rolling_ball(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    supports: &[Option<cadmpeg_ir::geometry::BlendSupport>; 2],
    spine: Option<&cadmpeg_ir::ids::CurveId>,
    radius: &BlendRadiusLaw,
    cross_section: &cadmpeg_ir::geometry::BlendCrossSection,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    if *cross_section != cadmpeg_ir::geometry::BlendCrossSection::Circular {
        return Err(CodecError::NotImplemented(
            "source-less rb_blend_spl_sur requires a circular cross-section".into(),
        ));
    }
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "rb_blend_spl_sur")?;
    for (side, support) in supports.iter().enumerate() {
        let Some(support) = support else { continue };
        if support.reversed {
            return Err(CodecError::NotImplemented(
                "source-less rb_blend_spl_sur reversed support is not defined".into(),
            ));
        }
        let carrier = target
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == support.surface)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "procedural surface {} references missing support {}",
                    procedural.id, support.surface
                ))
            })?;
        native_string(bytes, "blend_support_surface")?;
        native_subident(bytes, if side == 0 { "plane" } else { "sphere" })?;
        native_embedded_surface(bytes, &carrier.geometry)?;
    }
    let spine = spine.ok_or_else(|| {
        CodecError::Malformed("source-less rb_blend_spl_sur lacks a spine".into())
    })?;
    let spine = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *spine)
        .ok_or_else(|| CodecError::Malformed(format!("blend references missing spine {spine}")))?;
    let spine_range = [
        solved_cache.u_knots.first().copied().ok_or_else(|| {
            CodecError::Malformed("rolling-ball solved surface has no U knot domain".into())
        })?,
        solved_cache.u_knots.last().copied().ok_or_else(|| {
            CodecError::Malformed("rolling-ball solved surface has no U knot domain".into())
        })?,
    ];
    let spine = native_interval_curve(&spine.geometry, spine_range)?;
    native_nurbs_curve(bytes, &spine)?;
    let (start, end) = match radius {
        BlendRadiusLaw::Constant { signed_radius } => (*signed_radius, *signed_radius),
        BlendRadiusLaw::Linear { start, end } => (*start, *end),
        BlendRadiusLaw::Law { .. } => {
            return Err(CodecError::NotImplemented(
                "source-less rb_blend_spl_sur explicit radius law is not defined".into(),
            ))
        }
    };
    native_f64(bytes, start / LEN_TO_MM);
    native_f64(bytes, end / LEN_TO_MM);
    native_enum(bytes, -1);
    native_nurbs_surface(bytes, solved_cache)?;
    if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
        native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
    }
    bytes.push(0x10);
    Ok(())
}

pub(crate) fn native_nurbs_curve(
    bytes: &mut Vec<u8>,
    curve: &NurbsCurve,
) -> Result<(), CodecError> {
    let degree = usize::try_from(curve.degree)
        .map_err(|_| CodecError::NotImplemented("F3D NURBS curve degree exceeds usize".into()))?;
    if curve.knots.len() != curve.control_points.len() + degree + 1
        || curve
            .weights
            .as_ref()
            .is_some_and(|weights| weights.len() != curve.control_points.len())
    {
        return Err(CodecError::Malformed(
            "source-less F3D NURBS curve has inconsistent cardinality".into(),
        ));
    }
    native_ident(
        bytes,
        if curve.weights.is_some() {
            "nurbs"
        } else {
            "nubs"
        },
    )?;
    native_i64(bytes, i64::from(curve.degree));
    native_enum(bytes, if curve.periodic { 2 } else { 0 });
    native_i64(
        bytes,
        i64::try_from(unique_knot_count(&curve.knots))
            .map_err(|_| CodecError::NotImplemented("F3D unique-knot count exceeds i64".into()))?,
    );
    native_nurbs_knots(bytes, &curve.knots)?;
    for (index, point) in curve.control_points.iter().enumerate() {
        native_f64(bytes, point.x / LEN_TO_MM);
        native_f64(bytes, point.y / LEN_TO_MM);
        native_f64(bytes, point.z / LEN_TO_MM);
        if let Some(weights) = curve.weights.as_ref() {
            native_f64(bytes, weights[index]);
        }
    }
    Ok(())
}

fn native_spline_field_curve(
    geometry: &CurveGeometry,
    parameter_range: Option<[f64; 2]>,
) -> Result<NurbsCurve, CodecError> {
    match (geometry, parameter_range) {
        (CurveGeometry::Nurbs(curve), _) => Ok(curve.clone()),
        (_, Some(range)) => native_interval_curve(geometry, range),
        (CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. }, None) => {
            native_interval_curve(geometry, [0.0, std::f64::consts::TAU])
        }
        _ => Err(CodecError::NotImplemented(
            "source-less F3D spline field lacks a finite curve domain".into(),
        )),
    }
}

fn native_pcurve_knot_domain(
    pcurve: Option<&PcurveGeometry>,
) -> Result<Option<[f64; 2]>, CodecError> {
    let Some(PcurveGeometry::Nurbs { knots, .. }) = pcurve else {
        return Ok(None);
    };
    Ok(Some([
        *knots
            .first()
            .ok_or_else(|| CodecError::Malformed("pcurve has no knot domain".into()))?,
        *knots
            .last()
            .ok_or_else(|| CodecError::Malformed("pcurve has no knot domain".into()))?,
    ]))
}

fn native_interval_curve(
    geometry: &CurveGeometry,
    parameter_range: [f64; 2],
) -> Result<NurbsCurve, CodecError> {
    if !parameter_range.into_iter().all(f64::is_finite) || parameter_range[0] >= parameter_range[1]
    {
        return Err(CodecError::Malformed(
            "source-less F3D interval curve requires a finite ordered range".into(),
        ));
    }
    match geometry {
        CurveGeometry::Nurbs(curve) => Ok(curve.clone()),
        CurveGeometry::Line { origin, direction } => {
            if !finite_point(*origin) || !finite_vector(*direction) || direction.norm() == 0.0 {
                return Err(CodecError::Malformed(
                    "source-less F3D interval line requires finite nonzero geometry".into(),
                ));
            }
            let point = |parameter: f64| {
                Point3::new(
                    origin.x + parameter * direction.x,
                    origin.y + parameter * direction.y,
                    origin.z + parameter * direction.z,
                )
            };
            Ok(NurbsCurve {
                degree: 1,
                knots: vec![
                    parameter_range[0],
                    parameter_range[0],
                    parameter_range[1],
                    parameter_range[1],
                ],
                control_points: vec![point(parameter_range[0]), point(parameter_range[1])],
                weights: None,
                periodic: false,
            })
        }
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => native_conic_interval_curve(
            *center,
            *axis,
            *ref_direction,
            *radius,
            *radius,
            parameter_range,
        ),
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => native_conic_interval_curve(
            *center,
            *axis,
            *major_direction,
            *major_radius,
            *minor_radius,
            parameter_range,
        ),
        _ => Err(CodecError::NotImplemented(
            "source-less F3D interval construction requires a NURBS, line, circle, or ellipse source curve".into(),
        )),
    }
}

fn native_conic_interval_curve(
    center: Point3,
    axis: Vector3,
    major_direction: Vector3,
    major_radius: f64,
    minor_radius: f64,
    parameter_range: [f64; 2],
) -> Result<NurbsCurve, CodecError> {
    if !finite_point(center)
        || !finite_vector(axis)
        || !finite_vector(major_direction)
        || !major_radius.is_finite()
        || !minor_radius.is_finite()
        || axis.norm() == 0.0
        || major_direction.norm() == 0.0
        || major_radius <= 0.0
        || minor_radius <= 0.0
    {
        return Err(CodecError::Malformed(
            "source-less F3D conic interval requires finite nondegenerate geometry".into(),
        ));
    }
    let axis_norm = axis.norm();
    let axis = Vector3::new(axis.x / axis_norm, axis.y / axis_norm, axis.z / axis_norm);
    let major_norm = major_direction.norm();
    let major_direction = Vector3::new(
        major_direction.x / major_norm,
        major_direction.y / major_norm,
        major_direction.z / major_norm,
    );
    let minor_direction = Vector3::new(
        axis.y * major_direction.z - axis.z * major_direction.y,
        axis.z * major_direction.x - axis.x * major_direction.z,
        axis.x * major_direction.y - axis.y * major_direction.x,
    );
    let minor_norm = minor_direction.norm();
    if !minor_norm.is_finite() || minor_norm == 0.0 {
        return Err(CodecError::Malformed(
            "source-less F3D conic axis and major direction must not be parallel".into(),
        ));
    }
    let minor_direction = Vector3::new(
        minor_direction.x / minor_norm,
        minor_direction.y / minor_norm,
        minor_direction.z / minor_norm,
    );
    let delta = parameter_range[1] - parameter_range[0];
    let spans = (delta / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = delta / spans as f64;
    let mut control_points = Vec::with_capacity(spans * 2 + 1);
    let mut weights = Vec::with_capacity(spans * 2 + 1);
    let mut knots = Vec::with_capacity(spans * 2 + 4);
    let point = |angle: f64, scale: f64| {
        let major_scale = major_radius * angle.cos() * scale;
        let minor_scale = minor_radius * angle.sin() * scale;
        Point3::new(
            center.x + major_direction.x * major_scale + minor_direction.x * minor_scale,
            center.y + major_direction.y * major_scale + minor_direction.y * minor_scale,
            center.z + major_direction.z * major_scale + minor_direction.z * minor_scale,
        )
    };
    for span in 0..spans {
        let start = parameter_range[0] + step * span as f64;
        let end = start + step;
        let middle = (start + end) * 0.5;
        let weight = (step * 0.5).cos();
        if !weight.is_finite() || weight <= 0.0 {
            return Err(CodecError::Malformed(
                "source-less F3D conic interval has an invalid rational span".into(),
            ));
        }
        if span == 0 {
            control_points.push(point(start, 1.0));
            weights.push(1.0);
            knots.extend([start, start, start]);
        } else {
            knots.extend([start, start]);
        }
        control_points.push(point(middle, 1.0 / weight));
        weights.push(weight);
        control_points.push(point(end, 1.0));
        weights.push(1.0);
        if span + 1 == spans {
            knots.extend([end, end, end]);
        }
    }
    Ok(NurbsCurve {
        degree: 2,
        knots,
        control_points,
        weights: Some(weights),
        periodic: false,
    })
}

#[cfg(test)]
mod native_interval_curve_tests {
    use super::*;

    #[test]
    fn generated_circle_interval_lowers_to_exact_rational_nurbs() {
        let curve = native_interval_curve(
            &CurveGeometry::Circle {
                center: Point3::new(2.0, 3.0, 4.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 5.0,
            },
            [0.0, std::f64::consts::PI],
        )
        .expect("generated circle interval");
        let midpoint = cadmpeg_ir::eval::nurbs_curve_point(
            curve.degree,
            &curve.knots,
            &curve.control_points,
            curve.weights.as_deref(),
            std::f64::consts::FRAC_PI_2,
        )
        .expect("evaluate generated circle interval");
        assert!((midpoint.x - 2.0).abs() < 1.0e-12);
        assert!((midpoint.y - 8.0).abs() < 1.0e-12);
        assert!((midpoint.z - 4.0).abs() < 1.0e-12);
    }

    #[test]
    fn generated_ellipse_interval_preserves_both_radii() {
        let curve = native_interval_curve(
            &CurveGeometry::Ellipse {
                center: Point3::new(-1.0, 2.0, 0.5),
                axis: Vector3::new(0.0, 0.0, 1.0),
                major_direction: Vector3::new(1.0, 0.0, 0.0),
                major_radius: 6.0,
                minor_radius: 2.0,
            },
            [0.0, std::f64::consts::FRAC_PI_2],
        )
        .expect("generated ellipse interval");
        assert_eq!(curve.control_points[0], Point3::new(5.0, 2.0, 0.5));
        assert!((curve.control_points[2].x + 1.0).abs() < 1.0e-12);
        assert_eq!(curve.control_points[2].y, 4.0);
        assert_eq!(curve.control_points[2].z, 0.5);
        assert_eq!(
            curve.knots,
            vec![
                0.0,
                0.0,
                0.0,
                std::f64::consts::FRAC_PI_2,
                std::f64::consts::FRAC_PI_2,
                std::f64::consts::FRAC_PI_2,
            ]
        );
    }

    #[test]
    fn generated_domainless_circle_uses_its_full_natural_domain() {
        let geometry = CurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 3.0,
        };
        let curve = native_spline_field_curve(&geometry, None)
            .expect("generated domainless circle spline field");
        assert_eq!(curve.knots.first().copied(), Some(0.0));
        assert_eq!(curve.knots.last().copied(), Some(std::f64::consts::TAU));
        assert_eq!(curve.control_points.len(), 9);
        assert_eq!(curve.weights.as_ref().map(Vec::len), Some(9));
    }

    #[test]
    fn generated_domainless_line_remains_rejected() {
        let geometry = CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        };
        assert!(native_spline_field_curve(&geometry, None).is_err());
    }
}

pub(crate) fn native_procedural_curve(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    curve_id: &cadmpeg_ir::ids::CurveId,
    solved_cache: &NurbsCurve,
) -> Result<bool, CodecError> {
    let mut definitions = target
        .model
        .procedural_curves
        .iter()
        .filter(|procedural| procedural.curve == *curve_id);
    let Some(procedural) = definitions.next() else {
        return Ok(false);
    };
    if definitions.next().is_some() {
        return Err(CodecError::Malformed(format!(
            "curve {curve_id} has multiple procedural constructions"
        )));
    }
    if matches!(
        procedural.definition,
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Unknown { .. }
    ) {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D unknown procedural curve {} cannot be regenerated losslessly",
            procedural.id
        )));
    }
    let write_cache_fit_tolerance = |bytes: &mut Vec<u8>| {
        if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
            native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
        }
    };
    if matches!(
        procedural.definition,
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Exact
    ) {
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "exact_int_cur")?;
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Law {
        context,
        extension,
        primary,
        additional,
    } = &procedural.definition
    {
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "law_int_cur")?;
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        native_intcurve_support_context(bytes, target, context)?;
        native_i64(bytes, *extension);
        native_law_formula(bytes, target, primary)?;
        native_i64(
            bytes,
            i64::try_from(additional.len())
                .map_err(|_| CodecError::NotImplemented("law formula count exceeds i64".into()))?,
        );
        for formula in additional {
            native_law_formula(bytes, target, formula)?;
        }
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Deformable {
        extension,
        bend,
        data,
    } = &procedural.definition
    {
        let bend = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *bend)
            .ok_or_else(|| CodecError::Malformed("deformable bend curve is missing".into()))?;
        let bend_range = [
            solved_cache.knots.first().copied().ok_or_else(|| {
                CodecError::Malformed("deformable solved curve has no knot domain".into())
            })?,
            solved_cache.knots.last().copied().ok_or_else(|| {
                CodecError::Malformed("deformable solved curve has no knot domain".into())
            })?,
        ];
        let bend = native_interval_curve(&bend.geometry, bend_range)?;
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "defm_int_cur")?;
        native_i64(bytes, *extension);
        native_nurbs_curve(bytes, &bend)?;
        match data {
            cadmpeg_ir::geometry::DeformableCurveData::VectorField {
                vectors,
                parameter_pairs,
            } => {
                native_i64(bytes, 8);
                for vector in vectors {
                    native_vector(bytes, [vector.x, vector.y, vector.z]);
                }
                native_i64(
                    bytes,
                    i64::try_from(parameter_pairs.len()).map_err(|_| {
                        CodecError::NotImplemented(
                            "deformable parameter-pair count exceeds i64".into(),
                        )
                    })?,
                );
                for pair in parameter_pairs {
                    native_f64(bytes, pair[0]);
                    native_f64(bytes, pair[1]);
                }
            }
            cadmpeg_ir::geometry::DeformableCurveData::Surface { surface } => {
                native_i64(bytes, 5);
                let surface = target
                    .model
                    .surfaces
                    .iter()
                    .find(|candidate| candidate.id == *surface)
                    .ok_or_else(|| {
                        CodecError::Malformed("deformable support surface is missing".into())
                    })?;
                native_embedded_surface(bytes, &surface.geometry)?;
            }
        }
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Projection {
        context,
        discontinuity_flag,
        source,
        tail,
    } = &procedural.definition
    {
        let source = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *source)
            .ok_or_else(|| CodecError::Malformed("projection source curve is missing".into()))?;
        let source = native_interval_curve(&source.geometry, context.parameter_range)?;
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "proj_int_cur")?;
        native_intcurve_support_context(bytes, target, context)?;
        bytes.push(native_bool(*discontinuity_flag));
        native_nurbs_curve(bytes, &source)?;
        match tail {
            cadmpeg_ir::geometry::ProjectionTail::EarlyClose { flag } => {
                bytes.push(native_bool(*flag));
                bytes.push(0x10);
                native_nurbs_curve(bytes, solved_cache)?;
                write_cache_fit_tolerance(bytes);
            }
            cadmpeg_ir::geometry::ProjectionTail::Ranged {
                flag,
                parameter_range,
                role,
            } => {
                bytes.push(native_bool(*flag));
                for value in parameter_range {
                    native_f64(bytes, *value);
                }
                native_string(bytes, role)?;
                native_nurbs_curve(bytes, solved_cache)?;
                write_cache_fit_tolerance(bytes);
                bytes.push(0x10);
            }
        }
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound {
        parameters,
        component_parameters,
        components,
    } = &procedural.definition
    {
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "comp_int_cur")?;
        native_i64(
            bytes,
            i64::try_from(parameters.len()).map_err(|_| {
                CodecError::NotImplemented("compound parameter count exceeds i64".into())
            })?,
        );
        for value in parameters {
            native_f64(bytes, *value);
        }
        native_i64(
            bytes,
            i64::try_from(components.len()).map_err(|_| {
                CodecError::NotImplemented("compound component count exceeds i64".into())
            })?,
        );
        for value in component_parameters {
            native_f64(bytes, *value);
        }
        bytes.push(0x0b);
        for (ordinal, component) in components.iter().enumerate() {
            let component = target
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *component)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "compound curve references missing component {component}"
                    ))
                })?;
            let parameter_range = if matches!(component.geometry, CurveGeometry::Nurbs(_)) {
                None
            } else {
                let range = parameters.get(ordinal..ordinal + 2).ok_or_else(|| {
                    CodecError::Malformed("compound component has no construction interval".into())
                })?;
                Some([range[0], range[1]])
            };
            let component = native_spline_field_curve(&component.geometry, parameter_range)?;
            native_nurbs_curve(bytes, &component)?;
        }
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection {
        context,
        discontinuity_flag,
    } = &procedural.definition
    {
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "int_int_cur")?;
        native_intcurve_support_context(bytes, target, context)?;
        bytes.push(native_bool(*discontinuity_flag));
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve {
        family,
        context,
        tail,
    } = &procedural.definition
    {
        let name = match family {
            cadmpeg_ir::geometry::SurfaceCurveFamily::Blend => "blend_int_cur",
            cadmpeg_ir::geometry::SurfaceCurveFamily::SurfaceConstrained => "surf_int_cur",
            cadmpeg_ir::geometry::SurfaceCurveFamily::Parametric => "par_int_cur",
            cadmpeg_ir::geometry::SurfaceCurveFamily::Skin => "skin_int_cur",
        };
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, name)?;
        if let Some(tail) = tail {
            native_cache_first_curve_context(
                bytes,
                target,
                context,
                &cadmpeg_ir::geometry::CacheFirstCurveForm {
                    revision: tail.revision,
                    support_bounds: tail.support_bounds,
                    solved_range: tail.solved_range,
                    extension: tail.extension,
                },
                solved_cache,
                procedural.cache_fit_tolerance,
            )?;
            bytes.push(native_bool(tail.flag));
            if let Some(second_flag) = tail.second_flag {
                bytes.push(native_bool(second_flag));
            }
        } else {
            native_intcurve_support_context(bytes, target, context)?;
            native_nurbs_curve(bytes, solved_cache)?;
            write_cache_fit_tolerance(bytes);
        }
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Silhouette {
        context,
        silhouette,
        cast_surface,
        light_direction,
    } = &procedural.definition
    {
        let (name, draft_factor) = match silhouette {
            cadmpeg_ir::geometry::SilhouetteKind::Standard => ("silh_int_cur", None),
            cadmpeg_ir::geometry::SilhouetteKind::Parametric => ("para_silh_int_cur", None),
            cadmpeg_ir::geometry::SilhouetteKind::Taper { draft_factor } => {
                ("taper_silh_int_cur", Some(*draft_factor))
            }
        };
        let cast_surface = target
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *cast_surface)
            .ok_or_else(|| CodecError::Malformed("silhouette cast surface is missing".into()))?;
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, name)?;
        native_intcurve_support_context(bytes, target, context)?;
        native_embedded_surface(bytes, &cast_surface.geometry)?;
        native_vector(
            bytes,
            [light_direction.x, light_direction.y, light_direction.z],
        );
        if let Some(draft_factor) = draft_factor {
            native_f64(bytes, draft_factor);
        }
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceOffset {
        context,
        discontinuity_flag,
        base_u_range,
        base_v_range,
        base,
        base_range,
        base_endpoints,
        cache_first,
        distance,
        shift,
        scale,
    } = &procedural.definition
    {
        let base = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *base)
            .ok_or_else(|| CodecError::Malformed("surface-offset base curve is missing".into()))?;
        let base = native_interval_curve(&base.geometry, *base_range)?;
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "off_surf_int_cur")?;
        if let Some(form) = cache_first {
            native_cache_first_curve_context(
                bytes,
                target,
                context,
                form,
                solved_cache,
                procedural.cache_fit_tolerance,
            )?;
            for range in [base_u_range, base_v_range] {
                for value in *range {
                    native_optional_f64(bytes, Some(value));
                }
            }
            native_nurbs_curve(bytes, &base)?;
            for value in base_endpoints {
                native_optional_f64(bytes, *value);
            }
            for value in base_range {
                native_optional_f64(bytes, Some(*value));
            }
            native_f64(bytes, *distance / LEN_TO_MM);
            native_f64(bytes, *shift);
            native_f64(bytes, *scale);
        } else {
            native_intcurve_support_context(bytes, target, context)?;
            bytes.push(native_bool(*discontinuity_flag));
            for range in [base_u_range, base_v_range] {
                for value in *range {
                    native_f64(bytes, value);
                }
            }
            native_nurbs_curve(bytes, &base)?;
            for value in base_range {
                native_f64(bytes, *value);
            }
            native_f64(bytes, *distance / LEN_TO_MM);
            native_f64(bytes, *shift);
            native_f64(bytes, *scale);
            native_nurbs_curve(bytes, solved_cache)?;
            write_cache_fit_tolerance(bytes);
        }
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Spring {
        context,
        surface_parameter_ranges,
        first_pcurve_parameter_range,
        discontinuity_flag,
        cache_first,
        direction,
    } = &procedural.definition
    {
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "spring_int_cur")?;
        if let Some(form) = cache_first {
            if surface_parameter_ranges.iter().any(Option::is_some)
                || first_pcurve_parameter_range.is_some()
            {
                return Err(CodecError::Malformed(
                    "cache-first spring context stores no conditional null-carrier ranges".into(),
                ));
            }
            native_cache_first_curve_context(
                bytes,
                target,
                context,
                form,
                solved_cache,
                procedural.cache_fit_tolerance,
            )?;
            native_enum(bytes, *direction);
            bytes.push(0x10);
            return Ok(true);
        }
        for (side_index, side) in context.sides.iter().enumerate() {
            if let Some(surface_id) = &side.surface {
                if surface_parameter_ranges[side_index].is_some() {
                    return Err(CodecError::Malformed(
                        "spring surface ranges require a null_surface support".into(),
                    ));
                }
                let surface = target
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == *surface_id)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "spring references missing support {surface_id}"
                        ))
                    })?;
                native_embedded_surface(bytes, &surface.geometry)?;
            } else {
                native_ident(bytes, "null_surface")?;
                let ranges = surface_parameter_ranges[side_index].ok_or_else(|| {
                    CodecError::Malformed(
                        "spring null_surface support requires U/V parameter ranges".into(),
                    )
                })?;
                for range in ranges {
                    for value in range {
                        native_f64(bytes, value);
                    }
                }
            }
        }
        for (side_index, side) in context.sides.iter().enumerate() {
            if let Some(pcurve) = &side.pcurve {
                if side_index == 0 && first_pcurve_parameter_range.is_some() {
                    return Err(CodecError::Malformed(
                        "spring first-pcurve range requires a nullbs support".into(),
                    ));
                }
                native_nurbs_pcurve_block(bytes, pcurve)?;
            } else {
                native_ident(bytes, "nullbs")?;
                if side_index == 0 {
                    let range = first_pcurve_parameter_range.ok_or_else(|| {
                        CodecError::Malformed(
                            "spring first nullbs support requires a parameter range".into(),
                        )
                    })?;
                    for value in range {
                        native_f64(bytes, value);
                    }
                }
            }
        }
        for value in context.parameter_range {
            native_f64(bytes, value);
        }
        for discontinuities in &context.discontinuities {
            native_i64(
                bytes,
                i64::try_from(discontinuities.len()).map_err(|_| {
                    CodecError::NotImplemented("discontinuity count exceeds i64".into())
                })?,
            );
            for value in discontinuities {
                native_f64(bytes, *value);
            }
        }
        bytes.push(native_bool(*discontinuity_flag));
        native_enum(bytes, *direction);
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::ThreeSurfaceIntersection {
        context,
        selector,
        third,
    } = &procedural.definition
    {
        let surface_id = third.surface.as_ref().ok_or_else(|| {
            CodecError::NotImplemented(
                "source-less F3D sss_int_cur requires a third support surface".into(),
            )
        })?;
        let surface = target
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *surface_id)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "three-surface intersection references missing support {surface_id}"
                ))
            })?;
        let pcurve = third.pcurve.as_ref().ok_or_else(|| {
            CodecError::NotImplemented(
                "source-less F3D sss_int_cur requires a third support pcurve".into(),
            )
        })?;
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "sss_int_cur")?;
        native_intcurve_support_context(bytes, target, context)?;
        native_i64(bytes, *selector);
        native_embedded_surface(bytes, &surface.geometry)?;
        native_nurbs_pcurve_block(bytes, pcurve)?;
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::TwoSidedOffset {
        context,
        discontinuity_flag,
        offsets,
    } = &procedural.definition
    {
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "off_int_cur")?;
        native_intcurve_support_context(bytes, target, context)?;
        bytes.push(native_bool(*discontinuity_flag));
        for offset in offsets {
            native_f64(bytes, *offset / LEN_TO_MM);
        }
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::VectorOffset {
        source,
        parameter_range,
        offset,
        labels: [labels_0, labels_1, ..],
        codes,
    } = &procedural.definition
    {
        let source = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *source)
            .ok_or_else(|| CodecError::Malformed("vector offset source curve is missing".into()))?;
        let source = native_interval_curve(&source.geometry, *parameter_range)?;
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "offset_int_cur")?;
        bytes.push(0x0b);
        native_nurbs_curve(bytes, &source)?;
        native_f64(bytes, parameter_range[0]);
        native_f64(bytes, parameter_range[1]);
        native_vector(
            bytes,
            [
                offset.x / LEN_TO_MM,
                offset.y / LEN_TO_MM,
                offset.z / LEN_TO_MM,
            ],
        );
        native_string(bytes, labels_0)?;
        native_i64(bytes, codes[0]);
        native_string(bytes, labels_1)?;
        native_i64(bytes, codes[1]);
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
        source,
        parameter_range,
    } = &procedural.definition
    {
        let source = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *source)
            .ok_or_else(|| CodecError::Malformed("subset source curve is missing".into()))?;
        let source = native_interval_curve(&source.geometry, *parameter_range)?;
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "subset_int_cur")?;
        native_nurbs_curve(bytes, &source)?;
        native_f64(bytes, parameter_range[0]);
        native_f64(bytes, parameter_range[1]);
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    let (angle_range, center, major, minor, pitch, apex_factor, axis) = match &procedural.definition
    {
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
            angle_range,
            center,
            major,
            minor,
            pitch,
            apex_factor,
            axis,
        } => (angle_range, center, major, minor, pitch, apex_factor, axis),
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Offset { .. } => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D offset curve {} lacks a defined native offset-law grammar",
                procedural.id
            )))
        }
        cadmpeg_ir::geometry::ProceduralCurveDefinition::BlendSpine { .. } => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D blend-spine curve {} lacks its native blend construction",
                procedural.id
            )))
        }
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Exact
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::Law { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::ThreeSurfaceIntersection { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::Silhouette { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceOffset { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::Spring { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::Deformable { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::Projection { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::TwoSidedOffset { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::VectorOffset { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::Unknown { .. } => {
            unreachable!("procedural curve variant returned from its native writer")
        }
    };
    native_curve_base(bytes, "intcurve")?;
    bytes.push(0x0f);
    native_ident(bytes, "helix_int_cur")?;
    for value in *angle_range {
        bytes.push(0x0a);
        native_f64(bytes, value);
    }
    native_point(
        bytes,
        [
            center.x / LEN_TO_MM,
            center.y / LEN_TO_MM,
            center.z / LEN_TO_MM,
        ],
    );
    for vector in [major, minor, pitch] {
        native_point(
            bytes,
            [
                vector.x / LEN_TO_MM,
                vector.y / LEN_TO_MM,
                vector.z / LEN_TO_MM,
            ],
        );
    }
    native_f64(bytes, *apex_factor);
    native_vector(bytes, [axis.x, axis.y, axis.z]);
    native_nurbs_curve(bytes, solved_cache)?;
    write_cache_fit_tolerance(bytes);
    bytes.push(0x10);
    Ok(true)
}

pub(crate) fn native_cacheless_procedural_curve(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    curve_id: &cadmpeg_ir::ids::CurveId,
) -> Result<bool, CodecError> {
    let mut definitions = target
        .model
        .procedural_curves
        .iter()
        .filter(|procedural| procedural.curve == *curve_id);
    let Some(procedural) = definitions.next() else {
        return Ok(false);
    };
    if definitions.next().is_some() {
        return Err(CodecError::Malformed(format!(
            "curve {curve_id} has multiple procedural constructions"
        )));
    }
    if procedural.cache_fit_tolerance.is_some() {
        return Err(CodecError::Malformed(format!(
            "cacheless procedural curve {} carries a cache-fit tolerance",
            procedural.id
        )));
    }
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
        angle_range,
        center,
        major,
        minor,
        pitch,
        apex_factor,
        axis,
    } = &procedural.definition
    else {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize cacheless procedural curve {}",
            procedural.id
        )));
    };
    native_curve_base(bytes, "intcurve")?;
    bytes.push(0x0f);
    native_ident(bytes, "helix_int_cur")?;
    for value in *angle_range {
        bytes.push(0x0a);
        native_f64(bytes, value);
    }
    native_point(
        bytes,
        [
            center.x / LEN_TO_MM,
            center.y / LEN_TO_MM,
            center.z / LEN_TO_MM,
        ],
    );
    for vector in [major, minor, pitch] {
        native_point(
            bytes,
            [
                vector.x / LEN_TO_MM,
                vector.y / LEN_TO_MM,
                vector.z / LEN_TO_MM,
            ],
        );
    }
    native_f64(bytes, *apex_factor);
    native_vector(bytes, [axis.x, axis.y, axis.z]);
    bytes.push(0x10);
    Ok(true)
}

fn native_embedded_surface(
    bytes: &mut Vec<u8>,
    geometry: &SurfaceGeometry,
) -> Result<(), CodecError> {
    match geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            native_ident(bytes, "plane")?;
            native_point(
                bytes,
                [
                    origin.x / LEN_TO_MM,
                    origin.y / LEN_TO_MM,
                    origin.z / LEN_TO_MM,
                ],
            );
            native_vector(bytes, [normal.x, normal.y, normal.z]);
            native_vector(bytes, [u_axis.x, u_axis.y, u_axis.z]);
            bytes.push(0x0b);
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => native_embedded_cone(bytes, *origin, *axis, *ref_direction, *radius, 1.0, 0.0)?,
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } => native_embedded_cone(
            bytes,
            *origin,
            *axis,
            *ref_direction,
            *radius,
            *ratio,
            *half_angle,
        )?,
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            native_ident(bytes, "sphere")?;
            native_point(
                bytes,
                [
                    center.x / LEN_TO_MM,
                    center.y / LEN_TO_MM,
                    center.z / LEN_TO_MM,
                ],
            );
            native_f64(bytes, *radius / LEN_TO_MM);
            native_vector(bytes, [ref_direction.x, ref_direction.y, ref_direction.z]);
            native_vector(bytes, [axis.x, axis.y, axis.z]);
            bytes.extend_from_slice(&[0x0b; 5]);
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            native_ident(bytes, "torus")?;
            native_point(
                bytes,
                [
                    center.x / LEN_TO_MM,
                    center.y / LEN_TO_MM,
                    center.z / LEN_TO_MM,
                ],
            );
            native_vector(bytes, [axis.x, axis.y, axis.z]);
            native_f64(bytes, *major_radius / LEN_TO_MM);
            native_f64(bytes, *minor_radius / LEN_TO_MM);
            native_vector(bytes, [ref_direction.x, ref_direction.y, ref_direction.z]);
            bytes.extend_from_slice(&[0x0b; 5]);
        }
        SurfaceGeometry::Nurbs(surface) => {
            native_ident(bytes, "spline")?;
            native_nurbs_surface(bytes, surface)?;
        }
        SurfaceGeometry::Procedural { .. } | SurfaceGeometry::Unknown { .. } => {
            return Err(CodecError::NotImplemented(
                "source-less F3D embedded procedural or unknown support surfaces are unsupported"
                    .into(),
            ));
        }
    }
    Ok(())
}

fn native_intcurve_support_context(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    context: &cadmpeg_ir::geometry::IntcurveSupportContext,
) -> Result<(), CodecError> {
    for side in &context.sides {
        if let Some(surface_id) = &side.surface {
            let surface = target
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == *surface_id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "intcurve references missing support {surface_id}"
                    ))
                })?;
            native_embedded_surface(bytes, &surface.geometry)?;
        } else {
            native_ident(bytes, "null_surface")?;
        }
    }
    for side in &context.sides {
        if let Some(pcurve) = &side.pcurve {
            native_nurbs_pcurve_block(bytes, pcurve)?;
        } else {
            native_ident(bytes, "nullbs")?;
        }
    }
    for value in context.parameter_range {
        native_f64(bytes, value);
    }
    for discontinuities in &context.discontinuities {
        native_i64(
            bytes,
            i64::try_from(discontinuities.len()).map_err(|_| {
                CodecError::NotImplemented("discontinuity count exceeds i64".into())
            })?,
        );
        for value in discontinuities {
            native_f64(bytes, *value);
        }
    }
    Ok(())
}

fn native_optional_f64(bytes: &mut Vec<u8>, value: Option<f64>) {
    match value {
        Some(value) => {
            bytes.push(0x0a);
            native_f64(bytes, value);
        }
        None => bytes.push(0x0b),
    }
}

/// Emit an embedded support surface followed by its four optional U/V bound
/// fields when the surface kind carries them.
fn native_embedded_surface_with_bounds(
    bytes: &mut Vec<u8>,
    geometry: &SurfaceGeometry,
    bounds: &[Option<f64>; 4],
) -> Result<(), CodecError> {
    native_embedded_surface(bytes, geometry)?;
    if matches!(
        geometry,
        SurfaceGeometry::Nurbs(_) | SurfaceGeometry::Plane { .. }
    ) {
        for bound in bounds {
            native_optional_f64(bytes, *bound);
        }
    } else if bounds.iter().any(Option::is_some) {
        return Err(CodecError::Malformed(
            "support bounds require a spline or plane support".into(),
        ));
    }
    Ok(())
}

/// Emit the shared revision-gated surface tail: enum, solved cache, fit
/// tolerance, six discontinuity arrays, tail boolean, and trailing booleans.
fn native_revision_surface_tail(
    bytes: &mut Vec<u8>,
    form: &cadmpeg_ir::geometry::RevisionSurfaceForm,
    solved_cache: &cadmpeg_ir::geometry::NurbsSurface,
    cache_fit_tolerance: Option<f64>,
) -> Result<(), CodecError> {
    native_enum(bytes, form.tail_enum);
    native_nurbs_surface(bytes, solved_cache)?;
    native_f64(bytes, cache_fit_tolerance.unwrap_or(0.0) / LEN_TO_MM);
    for discontinuities in &form.discontinuities {
        native_i64(
            bytes,
            i64::try_from(discontinuities.len()).map_err(|_| {
                CodecError::NotImplemented("discontinuity count exceeds i64".into())
            })?,
        );
        for value in discontinuities {
            native_f64(bytes, *value);
        }
    }
    bytes.push(native_bool(form.tail_flag));
    for flag in &form.trailing_flags {
        bytes.push(native_bool(*flag));
    }
    Ok(())
}

/// Emit the shared cache-first intcurve context: revision, enum zero, solved
/// cache and fit tolerance, bounded supports, nullable pcurves, optional
/// solved-interval endpoints, discontinuity arrays, and the extension integer.
fn native_cache_first_curve_context(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    context: &cadmpeg_ir::geometry::IntcurveSupportContext,
    form: &cadmpeg_ir::geometry::CacheFirstCurveForm,
    solved_cache: &cadmpeg_ir::geometry::NurbsCurve,
    cache_fit_tolerance: Option<f64>,
) -> Result<(), CodecError> {
    if form.revision <= 0 {
        return Err(CodecError::Malformed(
            "cache-first intcurve context requires a positive serializer revision".into(),
        ));
    }
    native_i64(bytes, form.revision);
    native_enum(bytes, 0);
    native_nurbs_curve(bytes, solved_cache)?;
    native_f64(bytes, cache_fit_tolerance.unwrap_or(0.0) / LEN_TO_MM);
    for (side, bounds) in context.sides.iter().zip(&form.support_bounds) {
        if let Some(surface_id) = &side.surface {
            let surface = target
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == *surface_id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "cache-first intcurve references missing support {surface_id}"
                    ))
                })?;
            native_embedded_surface(bytes, &surface.geometry)?;
            if matches!(
                surface.geometry,
                SurfaceGeometry::Nurbs(_) | SurfaceGeometry::Plane { .. }
            ) {
                for bound in bounds {
                    native_optional_f64(bytes, *bound);
                }
            } else if bounds.iter().any(Option::is_some) {
                return Err(CodecError::Malformed(
                    "cache-first support bounds require a spline or plane support".into(),
                ));
            }
        } else {
            native_ident(bytes, "null_surface")?;
            if bounds.iter().any(Option::is_some) {
                return Err(CodecError::Malformed(
                    "cache-first support bounds require a non-null support".into(),
                ));
            }
        }
    }
    for side in &context.sides {
        if let Some(pcurve) = &side.pcurve {
            native_nurbs_pcurve_block(bytes, pcurve)?;
        } else {
            native_ident(bytes, "nullbs")?;
        }
    }
    for value in form.solved_range {
        native_optional_f64(bytes, value);
    }
    for discontinuities in &context.discontinuities {
        native_i64(
            bytes,
            i64::try_from(discontinuities.len()).map_err(|_| {
                CodecError::NotImplemented("discontinuity count exceeds i64".into())
            })?,
        );
        for value in discontinuities {
            native_f64(bytes, *value);
        }
    }
    native_i64(bytes, form.extension);
    Ok(())
}

fn native_embedded_cone(
    bytes: &mut Vec<u8>,
    origin: cadmpeg_ir::math::Point3,
    axis: Vector3,
    ref_direction: Vector3,
    radius: f64,
    ratio: f64,
    half_angle: f64,
) -> Result<(), CodecError> {
    native_ident(bytes, "cone")?;
    native_point(
        bytes,
        [
            origin.x / LEN_TO_MM,
            origin.y / LEN_TO_MM,
            origin.z / LEN_TO_MM,
        ],
    );
    native_vector(bytes, [axis.x, axis.y, axis.z]);
    native_vector(
        bytes,
        [
            ref_direction.x * radius / LEN_TO_MM,
            ref_direction.y * radius / LEN_TO_MM,
            ref_direction.z * radius / LEN_TO_MM,
        ],
    );
    native_f64(bytes, ratio);
    bytes.extend_from_slice(&[0x0b, 0x0b]);
    native_f64(bytes, half_angle.sin());
    native_f64(bytes, half_angle.cos());
    native_f64(bytes, radius / LEN_TO_MM);
    bytes.extend_from_slice(&[0x0b; 5]);
    Ok(())
}

pub(crate) fn pcurve_uses_ref_form(pcurve: &Pcurve) -> Result<bool, CodecError> {
    match (
        pcurve.wrapper_reversed,
        pcurve.native_tail_flags,
        pcurve.fit_tolerance,
    ) {
        (None, None, None) => Ok(true),
        (Some(_), Some(_), Some(_)) => Ok(false),
        _ => Err(CodecError::Malformed(format!(
            "pcurve {} mixes inline and ref-form native fields",
            pcurve.id
        ))),
    }
}

pub(crate) fn native_pcurve(
    bytes: &mut Vec<u8>,
    pcurve: &Pcurve,
    companion_ref: Option<i64>,
) -> Result<(), CodecError> {
    if pcurve_uses_ref_form(pcurve)? {
        let companion_ref = companion_ref.ok_or_else(|| {
            CodecError::Malformed(format!(
                "ref-form pcurve {} has no companion record",
                pcurve.id
            ))
        })?;
        let range = pcurve.parameter_range.ok_or_else(|| {
            CodecError::Malformed(format!(
                "ref-form pcurve {} has no parameter range",
                pcurve.id
            ))
        })?;
        native_ident(bytes, "pcurve")?;
        native_ref(bytes, -1);
        native_i64(bytes, -1);
        native_ref(bytes, -1);
        native_i64(bytes, 2);
        native_ref(bytes, companion_ref);
        native_f64(bytes, range[0]);
        native_f64(bytes, range[1]);
        return Ok(());
    }
    if companion_ref.is_some() {
        return Err(CodecError::Malformed(format!(
            "inline pcurve {} unexpectedly has a companion record",
            pcurve.id
        )));
    }
    let range = pcurve.parameter_range.unwrap_or([0.0, 1.0]);
    let NativePcurveGeometry {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    } = native_pcurve_geometry(&pcurve.geometry, range)?;
    let degree_usize = usize::try_from(degree)
        .map_err(|_| CodecError::NotImplemented("F3D pcurve degree exceeds usize".into()))?;
    if knots.len() != control_points.len() + degree_usize + 1
        || weights
            .as_ref()
            .is_some_and(|weights| weights.len() != control_points.len())
    {
        return Err(CodecError::Malformed(
            "source-less F3D pcurve has inconsistent cardinality".into(),
        ));
    }
    native_ident(bytes, "pcurve")?;
    native_ref(bytes, -1);
    native_i64(bytes, -1);
    native_ref(bytes, -1);
    native_i64(bytes, 0);
    bytes.push(native_bool(pcurve.wrapper_reversed.unwrap_or(false)));
    bytes.push(0x0f);
    native_ident(bytes, "exp_par_cur")?;
    native_ident(bytes, if weights.is_some() { "nurbs" } else { "nubs" })?;
    native_i64(bytes, i64::from(degree));
    native_enum(bytes, if periodic { 2 } else { 0 });
    native_i64(
        bytes,
        i64::try_from(unique_knot_count(&knots)).map_err(|_| {
            CodecError::NotImplemented("F3D pcurve unique-knot count exceeds i64".into())
        })?,
    );
    native_nurbs_knots(bytes, &knots)?;
    for (index, point) in control_points.iter().enumerate() {
        native_f64(bytes, point.u);
        native_f64(bytes, point.v);
        if let Some(weights) = weights.as_ref() {
            native_f64(bytes, weights[index]);
        }
    }
    native_f64(bytes, pcurve.fit_tolerance.unwrap_or(0.0));
    bytes.push(0x10);
    for flag in pcurve.native_tail_flags.unwrap_or([true; 4]) {
        bytes.push(native_bool(flag));
    }
    let range = pcurve.parameter_range.unwrap_or_else(|| {
        [
            knots.first().copied().unwrap_or(0.0),
            knots.last().copied().unwrap_or(0.0),
        ]
    });
    native_f64(bytes, range[0]);
    native_f64(bytes, range[1]);
    Ok(())
}

pub(crate) fn native_ref_pcurve_companion(
    bytes: &mut Vec<u8>,
    pcurve: &Pcurve,
) -> Result<(), CodecError> {
    if !pcurve_uses_ref_form(pcurve)? {
        return Err(CodecError::Malformed(format!(
            "inline pcurve {} cannot emit a ref-form companion",
            pcurve.id
        )));
    }
    let range = pcurve.parameter_range.ok_or_else(|| {
        CodecError::Malformed(format!(
            "ref-form pcurve {} has no parameter range",
            pcurve.id
        ))
    })?;
    let native = native_pcurve_geometry(&pcurve.geometry, range)?;
    let lifted = NurbsCurve {
        degree: native.degree,
        knots: native.knots,
        control_points: native
            .control_points
            .into_iter()
            .map(|point| Point3::new(point.u * 10.0, point.v * 10.0, 0.0))
            .collect(),
        weights: native.weights,
        periodic: native.periodic,
    };
    native_curve_base(bytes, "intcurve")?;
    native_nurbs_curve(bytes, &lifted)?;
    native_nurbs_pcurve_block(bytes, &pcurve.geometry)?;
    Ok(())
}

struct NativePcurveGeometry {
    degree: u32,
    knots: Vec<f64>,
    control_points: Vec<cadmpeg_ir::math::Point2>,
    weights: Option<Vec<f64>>,
    periodic: bool,
}

fn native_pcurve_geometry(
    geometry: &PcurveGeometry,
    range: [f64; 2],
) -> Result<NativePcurveGeometry, CodecError> {
    match geometry {
        PcurveGeometry::Line { origin, direction } => {
            if !range.iter().all(|value| value.is_finite()) || range[0] >= range[1] {
                return Err(CodecError::Malformed(
                    "source-less F3D line pcurve requires an ordered finite range".into(),
                ));
            }
            Ok(NativePcurveGeometry {
                degree: 1,
                knots: vec![range[0], range[0], range[1], range[1]],
                control_points: vec![
                    cadmpeg_ir::math::Point2::new(
                        origin.u + range[0] * direction.u,
                        origin.v + range[0] * direction.v,
                    ),
                    cadmpeg_ir::math::Point2::new(
                        origin.u + range[1] * direction.u,
                        origin.v + range[1] * direction.v,
                    ),
                ],
                weights: None,
                periodic: false,
            })
        }
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic,
        } => Ok(NativePcurveGeometry {
            degree: *degree,
            knots: knots.clone(),
            control_points: control_points.clone(),
            weights: weights.clone(),
            periodic: *periodic,
        }),
    }
}

fn native_nurbs_pcurve_block(
    bytes: &mut Vec<u8>,
    geometry: &PcurveGeometry,
) -> Result<(), CodecError> {
    let NativePcurveGeometry {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    } = native_pcurve_geometry(geometry, [0.0, 1.0])?;
    let degree_usize = usize::try_from(degree)
        .map_err(|_| CodecError::NotImplemented("F3D pcurve degree exceeds usize".into()))?;
    if knots.len() != control_points.len() + degree_usize + 1
        || weights
            .as_ref()
            .is_some_and(|weights| weights.len() != control_points.len())
    {
        return Err(CodecError::Malformed(
            "embedded F3D support pcurve has inconsistent cardinality".into(),
        ));
    }
    native_ident(bytes, if weights.is_some() { "nurbs" } else { "nubs" })?;
    native_i64(bytes, i64::from(degree));
    native_enum(bytes, if periodic { 2 } else { 0 });
    native_i64(
        bytes,
        i64::try_from(unique_knot_count(&knots)).map_err(|_| {
            CodecError::NotImplemented("F3D pcurve unique-knot count exceeds i64".into())
        })?,
    );
    native_nurbs_knots(bytes, &knots)?;
    for (index, point) in control_points.iter().enumerate() {
        native_f64(bytes, point.u);
        native_f64(bytes, point.v);
        if let Some(weights) = weights.as_ref() {
            native_f64(bytes, weights[index]);
        }
    }
    Ok(())
}

fn native_nurbs_knot_counts(bytes: &mut Vec<u8>, knots: [&[f64]; 2]) -> Result<(), CodecError> {
    for knots in knots {
        native_i64(
            bytes,
            i64::try_from(unique_knot_count(knots)).map_err(|_| {
                CodecError::NotImplemented("F3D unique-knot count exceeds i64".into())
            })?,
        );
    }
    Ok(())
}

fn native_nurbs_knots(bytes: &mut Vec<u8>, knots: &[f64]) -> Result<(), CodecError> {
    let mut runs = Vec::<(f64, usize)>::new();
    for knot in knots {
        if let Some((value, count)) = runs.last_mut() {
            if *value == *knot {
                *count += 1;
                continue;
            }
        }
        runs.push((*knot, 1));
    }
    let run_count = runs.len();
    for (index, (value, expanded)) in runs.into_iter().enumerate() {
        let endpoint_extra = usize::from(index == 0 || index + 1 == run_count);
        let stored = expanded
            .checked_sub(endpoint_extra)
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                CodecError::Malformed("F3D NURBS endpoint multiplicity is invalid".into())
            })?;
        native_f64(bytes, value);
        native_i64(
            bytes,
            i64::try_from(stored).map_err(|_| {
                CodecError::NotImplemented("F3D knot multiplicity exceeds i64".into())
            })?,
        );
    }
    Ok(())
}
