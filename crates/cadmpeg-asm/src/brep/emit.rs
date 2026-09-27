// SPDX-License-Identifier: Apache-2.0
//! Emit decoded carriers, pcurves, and topology entities into the [`AsmBrep`]
//! graph, one pass per entity kind.

use super::count_kind;
use super::records::{
    BodyNativeKey, EdgeContinuity, EdgeOwnership, EndpointSlot, EvaluatedToleranceSlot,
    FaceContainment, FaceNativeKey, FaceSidedness, TolerantCoedgeExtension,
    TolerantCoedgeParameters, TolerantEdgeTail, TolerantVertexTail, TransformHints,
    VertexOwnership,
};
use crate::ids::{brep_id, brep_key, IdFormat};
use crate::nurbs;
use crate::nurbs::proc_curve::{
    EmbeddedDeformableData, EmbeddedLawCurve, EmbeddedLawCurveLayout, EmbeddedProjection,
    EmbeddedSilhouette, EmbeddedSpring, EmbeddedSpringLayout, EmbeddedSpringPcurve,
    EmbeddedSpringSupport, EmbeddedSurfaceOffset, EmbeddedSurfaceOffsetLayout,
    ProceduralCurveConstruction,
};
use crate::nurbs::proc_surface::{
    ClassicLoftProfileData, DecodedProceduralSurfaceDefinition, EmbeddedCompoundLoft,
    EmbeddedCompoundLoftDirection, EmbeddedCompoundLoftScale, EmbeddedCompoundLoftTail,
    EmbeddedDeformableSurface, EmbeddedDeformableSurfaceData, EmbeddedDeformableSurfaceLayout,
    EmbeddedG2Blend, EmbeddedG2FirstShape, EmbeddedG2Side, EmbeddedLawExpression,
    EmbeddedLawFormula, EmbeddedLawSurface, EmbeddedLoft, EmbeddedLoftLayout, EmbeddedLoftPath,
    EmbeddedLoftPathLayout, EmbeddedLoftProfileMember, EmbeddedLoftSectionEntry,
    EmbeddedNetSurface, EmbeddedOffsetLayout, EmbeddedRevisionCompoundLoft,
    EmbeddedRevisionG2Blend, EmbeddedRollingBall, EmbeddedScaledCompoundLoft,
    EmbeddedScaledCompoundLoftBranch, EmbeddedScaledCompoundLoftShape, EmbeddedSkinSurface,
    EmbeddedSkinSurfaceLayout, EmbeddedSweepSurface, EmbeddedSweepSurfaceLayout,
    EmbeddedVariableBlend, EmbeddedVertexBlend, EmbeddedVertexBlendBoundaryGeometry,
    LegacySweepLayout, LoftProfileData, ProceduralSurfaceCache, SweepLawOrFormula,
};
use crate::nurbs::reader::LEN_TO_MM;
use crate::sab::{Record, Token};
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::geometry::{
    nurbs::NurbsCurve,
    pcurve::{Pcurve, PcurveGeometry, PcurveInlineForm, PcurveMetadata, PcurveNurbs},
    BlendCrossSection, BlendRadiusLaw, BlendSupport, Curve, CurveGeometry, LoftPathCurve,
    ProceduralCurve, ProceduralSurface, ProceduralSurfaceDefinition, RollingBallConstruction,
    RollingBallRadiusSelector, RollingBallSide, RollingBallSideExtension, RollingBallSupportCurve,
    RollingBallSupportSurface, RollingBallThirdSide, SolvedCurveGeometry, SolvedSurfaceGeometry,
    Surface, SurfaceGeometry, VariableBlendConstruction, VertexBlendBoundary,
    VertexBlendBoundaryGeometry, VertexBlendConstruction,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    ProceduralSurfaceId, RegionId, ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex};
use cadmpeg_ir::unknown::UnknownRecord;

macro_rules! charged_push {
    ($ctx:expr, $values:expr, $value:expr $(,)?) => {{
        crate::decode_alloc::reserve_vec_slot($ctx, &mut $values, stringify!($values))?;
        $values.push($value);
    }};
}

fn map_law_formula(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    formula: EmbeddedLawFormula,
    mut map: impl FnMut(usize, EmbeddedLawExpression) -> Result<cadmpeg_ir::geometry::LawExpression, cadmpeg_core::CodecError>,
) -> Result<cadmpeg_ir::geometry::LawFormula, cadmpeg_core::CodecError> {
    match formula {
        EmbeddedLawFormula::Null => Ok(cadmpeg_ir::geometry::LawFormula::Null {}),
        EmbeddedLawFormula::Named { name, variables } => {
            let mut mapped = crate::decode_alloc::counted_vec(
                ctx, variables.len(), "ASM law formula variables",
            )?;
            for (index, expression) in variables.into_iter().enumerate() {
                mapped.push(map(index, expression)?);
            }
            Ok(cadmpeg_ir::geometry::LawFormula::Named { name, variables: mapped })
        }
    }
}
use std::collections::{HashMap, HashSet};
use crate::decode_alloc::CountedIteratorExt;

use super::attributes::{
    attribute_chain_color, attribute_chain_name, attribute_owner, collect_attributes,
    decode_transform, source_attribute, unknown_record_id,
};
use super::geometry::{
    coedge_pcurve_ref, collect_carrier, double_at, is_asm_stream_delimiter, is_coedge_record,
    is_edge_record, is_known_record_head, is_vertex_record, norm3, pcurve_inline_tail_flags,
    pcurve_parameter_range, record_reversed, reverse_curve_geometry, scale_point, sense_at,
    tolerant_coedge_extension, vertex_point_ref,
};
use super::topology::{
    loop_chain, region_chain, ring_coedges, shell_chain, shell_faces, subshell_ancestor_shells,
};
use super::{id, inherited_attribute_target, AsmBrep, Carriers, Reachable, WireShellTopology};
const EPS_EMIT_EMIT_EDGES_E9: f64 = 1.0e-9;

/// Emit a kept surface carrier and, when present, its procedural-surface
/// construction and nested support carriers.
fn emit_carrier_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    r: &Record,
    i: i64,
    carriers: &mut Carriers,
    reach: &Reachable,
    format: IdFormat,
) -> Result<(), cadmpeg_core::CodecError> {
    let Carriers {
        surface_geo,
        procedural_surface_defs,
        procedural_support_sources,
        procedural_curve_child_sources,
        ..
    } = &mut *carriers;
    let Reachable {
        cached_unknown_procedural_surfaces,
        ..
    } = reach;
    // A record index appears at most once in `records`; a duplicate
    // would have consumed the entry already, so skip rather than panic.
    let Some(geometry) = surface_geo.remove(&i) else {
        return Ok(());
    };
    charged_push!(ctx, out.surfaces, Surface {
        id: <SurfaceId>::from(id(format, i)),
        geometry,
        source_object: None,
    });
    if let Some(procedural) = procedural_surface_defs.remove(&i) {
        let support_start = out.surfaces.len();
        let curve_start = out.curves.len();
        let (definition, cache) = procedural.into_parts();
        let definition = match definition {
            DecodedProceduralSurfaceDefinition::Deformable(embedded) => {
                emit_deformable_surface(ctx, out, i, embedded, format)?
            }
            DecodedProceduralSurfaceDefinition::Helix(construction) => {
                ProceduralSurfaceDefinition::Helix { construction }
            }
            DecodedProceduralSurfaceDefinition::TSpline(construction) => {
                use crate::nurbs::proc_surface::EmbeddedTSplineSubtransform;
                use cadmpeg_core::CodecError;
                use cadmpeg_ir::geometry::{
                    InlineTSplineSubtransform, SubtypeTableIndex, TSplineSubtransform,
                    TSplineSurfaceConstruction,
                };

                let subtransform = match construction.subtransform {
                    EmbeddedTSplineSubtransform::Inline {
                        program,
                        separator,
                        values,
                    } => TSplineSubtransform::Inline(
                        InlineTSplineSubtransform::try_new(program, separator, values)
                            .map_err(CodecError::malformed)?,
                    ),
                    EmbeddedTSplineSubtransform::Reference { index, resolved } => {
                        TSplineSubtransform::Resolved {
                            index: SubtypeTableIndex::try_new(index)
                                .map_err(CodecError::malformed)?,
                            transform: Box::new(resolved.ok_or_else(|| {
                                CodecError::malformed("T-spline subtransform is unresolved")
                            })?),
                        }
                    }
                };
                ProceduralSurfaceDefinition::TSpline {
                    construction: Box::new(
                        TSplineSurfaceConstruction::try_new(
                            construction.parameter_ranges,
                            construction.type_code,
                            subtransform,
                            construction.trailing_value,
                            construction.discontinuities,
                            construction.discontinuity_flag,
                            cadmpeg_ir::geometry::CacheContract::from_form(
                                construction.revision_form,
                            ),
                        )
                        .map_err(CodecError::malformed)?,
                    ),
                }
            }
            DecodedProceduralSurfaceDefinition::Exact { spline } => {
                ProceduralSurfaceDefinition::Exact(
                    cadmpeg_ir::geometry::surface_payloads::ExactSurfacePayload::try_new(spline)
                        .map_err(cadmpeg_core::CodecError::malformed)?,
                )
            }
            DecodedProceduralSurfaceDefinition::Compound { components } => {
                let component_ids = components
                    .into_iter()
                    .enumerate()
                    .map(|(component, item)|  -> Result<_, cadmpeg_core::CodecError> { Ok({
                        let id = brep_id!(
                            format,
                            SurfaceId,
                            "procedural_surface",
                            brep_key!(i, ":component", component)
                        );
                        charged_push!(ctx, out.surfaces, Surface {
                            id: id.clone(),
                            geometry: item.component,
                            source_object: None,
                        });
                        cadmpeg_ir::geometry::CompoundComponent {
                            parameter: item.parameter,
                            component: id,
                        }
                    })})
                    .try_collect_counted_vec(ctx, "ASM compound surface components")?;
                ProceduralSurfaceDefinition::Compound(
                    cadmpeg_ir::geometry::surface_payloads::CompoundSurfacePayload::try_new(
                        component_ids,
                        None,
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                )
            }
            DecodedProceduralSurfaceDefinition::SubSurface {
                support,
                parameter_ranges,
            } => {
                let support_id = brep_id!(
                    format,
                    SurfaceId,
                    "procedural_surface",
                    brep_key!(i, ":sub_surface:support")
                );
                charged_push!(ctx, out.surfaces, Surface {
                    id: support_id.clone(),
                    geometry: support,
                    source_object: None,
                });
                ProceduralSurfaceDefinition::SubSurface(
                    cadmpeg_ir::geometry::surface_payloads::SubSurfaceConstruction::try_new(
                        support_id,
                        parameter_ranges,
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                )
            }
            DecodedProceduralSurfaceDefinition::Taper {
                support,
                reference,
                pcurve,
                parameter,
                taper,
                revision_form,
            } => {
                let support_id = brep_id!(
                    format,
                    SurfaceId,
                    "procedural_surface",
                    brep_key!(i, ":support")
                );
                charged_push!(ctx, out.surfaces, Surface {
                    id: support_id.clone(),
                    geometry: support,
                    source_object: None,
                });
                let reference_id = brep_id!(
                    format,
                    CurveId,
                    "procedural_surface",
                    brep_key!(i, ":reference")
                );
                charged_push!(ctx, out.curves, Curve {
                    id: reference_id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(reference)),
                    source_object: None,
                });
                let pcurve = pcurve.map(|nurbs| PcurveGeometry::Nurbs { nurbs });
                ProceduralSurfaceDefinition::Taper(
                    cadmpeg_ir::geometry::surface_payloads::TaperSurfaceConstruction::try_new(
                        support_id,
                        reference_id,
                        pcurve,
                        parameter,
                        taper,
                        cadmpeg_ir::geometry::CacheContract::from_form(revision_form),
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                )
            }
            DecodedProceduralSurfaceDefinition::Loft(embedded) => {
                emit_loft_surface(ctx, out, i, embedded, format)?
            }
            DecodedProceduralSurfaceDefinition::CompoundLoft(embedded) => {
                emit_compound_loft_surface(ctx, out, i, *embedded, format)?
            }
            DecodedProceduralSurfaceDefinition::ScaledCompoundLoft(embedded) => {
                emit_scaled_compound_loft_surface(ctx, out, i, embedded, format)?
            }
            DecodedProceduralSurfaceDefinition::Law(embedded) => {
                emit_law_surface(ctx, out, i, embedded, format)?
            }
            DecodedProceduralSurfaceDefinition::Skin(embedded) => {
                emit_skin_surface(ctx, out, i, embedded, format)?
            }
            DecodedProceduralSurfaceDefinition::Net(embedded) => {
                emit_net_surface(ctx, out, i, embedded, format)?
            }
            DecodedProceduralSurfaceDefinition::Sweep(embedded) => {
                emit_sweep_surface(ctx, out, i, embedded, format)?
            }
            DecodedProceduralSurfaceDefinition::G2Blend(embedded) => {
                emit_g2_blend_surface(ctx, out, i, embedded, format)?
            }
            DecodedProceduralSurfaceDefinition::Ruled { first, second } => {
                let first_id = brep_id!(
                    format,
                    CurveId,
                    "procedural_surface",
                    brep_key!(i, ":profile0")
                );
                let second_id = brep_id!(
                    format,
                    CurveId,
                    "procedural_surface",
                    brep_key!(i, ":profile1")
                );
                charged_push!(ctx, out.curves, Curve {
                    id: first_id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(first)),
                    source_object: None,
                });
                charged_push!(ctx, out.curves, Curve {
                    id: second_id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(second)),
                    source_object: None,
                });
                ProceduralSurfaceDefinition::Ruled {
                    first: first_id,
                    second: second_id,
                    cache: None,
                }
            }
            DecodedProceduralSurfaceDefinition::Sum {
                first,
                second,
                basepoint,
                revision_form,
            } => {
                let first_id = brep_id!(
                    format,
                    CurveId,
                    "procedural_surface",
                    brep_key!(i, ":curve0")
                );
                let second_id = brep_id!(
                    format,
                    CurveId,
                    "procedural_surface",
                    brep_key!(i, ":curve1")
                );
                charged_push!(ctx, out.curves, Curve {
                    id: first_id.clone(),
                    geometry: first,
                    source_object: None,
                });
                charged_push!(ctx, out.curves, Curve {
                    id: second_id.clone(),
                    geometry: second,
                    source_object: None,
                });
                ProceduralSurfaceDefinition::Sum(
                    cadmpeg_ir::geometry::surface_payloads::SumSurfaceConstruction::try_new(
                        first_id,
                        second_id,
                        basepoint,
                        cadmpeg_ir::geometry::CacheContract::from_form(revision_form),
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                )
            }
            DecodedProceduralSurfaceDefinition::Revolution {
                directrix,
                axis_origin,
                axis_direction,
                angular_interval,
                parameter_interval,
                revision_form,
            } => {
                let directrix_id = brep_id!(
                    format,
                    CurveId,
                    "procedural_surface",
                    brep_key!(i, ":directrix")
                );
                charged_push!(ctx, out.curves, Curve {
                    id: directrix_id.clone(),
                    geometry: directrix,
                    source_object: None,
                });
                ProceduralSurfaceDefinition::Revolution(
                    cadmpeg_ir::features::FinitePoint3::new(axis_origin)
                    .ok_or(cadmpeg_ir::geometry::ProceduralGeometryError::Payload(
                        "revolution axis_origin and axis_direction must be finite, with unit axis_direction",
                    ))
                    .and_then(|origin| {
                        cadmpeg_ir::geometry::surface_payloads::RevolutionSurfaceConstruction::try_new(
                            directrix_id,
                            (origin, axis_direction),
                            angular_interval,
                            None,
                            Some(parameter_interval),
                            false,
                            cadmpeg_ir::geometry::CacheContract::from_form(revision_form),
                        )
                    })
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                )
            }
            DecodedProceduralSurfaceDefinition::Offset {
                support,
                distance,
                layout,
            } => {
                let support_id = brep_id!(
                    format,
                    SurfaceId,
                    "procedural_surface",
                    brep_key!(i, ":support")
                );
                charged_push!(ctx, out.surfaces, Surface {
                    id: support_id.clone(),
                    geometry: support,
                    source_object: None,
                });
                let (u_sense, v_sense, extension) = match layout {
                    EmbeddedOffsetLayout::Legacy {
                        u_sense,
                        v_sense,
                        extension,
                    } => (
                        Some(u_sense),
                        Some(v_sense),
                        cadmpeg_ir::geometry::OffsetExtension::Legacy {
                            flags: extension,
                            cache: None,
                        },
                    ),
                    EmbeddedOffsetLayout::Revision(form) => (
                        None,
                        None,
                        cadmpeg_ir::geometry::OffsetExtension::Revision { form: *form },
                    ),
                };
                ProceduralSurfaceDefinition::Offset(
                    cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                        support_id, distance, u_sense, v_sense, false, extension,
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                )
            }
            DecodedProceduralSurfaceDefinition::Extrusion {
                directrix,
                parameter_interval,
                direction,
                native_position,
                revision_form,
            } => {
                let directrix_id = brep_id!(
                    format,
                    CurveId,
                    "procedural_surface",
                    brep_key!(i, ":directrix")
                );
                charged_push!(ctx, out.curves, Curve {
                    id: directrix_id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(directrix)),
                    source_object: None,
                });
                ProceduralSurfaceDefinition::Extrusion(
                    cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                        directrix_id,
                        Some(parameter_interval),
                        direction,
                        Some(native_position),
                        cadmpeg_ir::geometry::CacheContract::from_form(revision_form),
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                )
            }
            DecodedProceduralSurfaceDefinition::VariableBlend(construction) => {
                emit_variable_blend_surface(ctx, out, i, construction, format)?
            }
            DecodedProceduralSurfaceDefinition::RevisionCompoundLoft(construction) => {
                emit_revision_compound_loft_surface(ctx, out, i, construction, format)?
            }
            DecodedProceduralSurfaceDefinition::RevisionG2Blend(construction) => {
                emit_revision_g2_blend_surface(ctx, out, i, construction, format)?
            }
            DecodedProceduralSurfaceDefinition::VertexBlend(construction) => {
                emit_vertex_blend_surface(ctx, out, i, *construction, format)?
            }
            DecodedProceduralSurfaceDefinition::Blend {
                supports,
                spine,
                radius_offsets,
                cross_section,
                native,
            } => emit_blend_surface(ctx,
                out,
                i,
                supports,
                spine,
                radius_offsets,
                cross_section,
                native,
                format,
            )?,
        };
        procedural_support_sources.extend(
            out.surfaces[support_start..]
                .iter()
                .map(|surface| (i, surface.id.clone())),
        );
        procedural_curve_child_sources.extend(
            out.curves[curve_start..]
                .iter()
                .map(|curve| (i, curve.id.clone())),
        );
        let cache_fit_tolerance = match cache {
            ProceduralSurfaceCache::Legacy(tolerance) => tolerance,
            ProceduralSurfaceCache::Revision => None,
        };
        let mut definition = definition;
        if let Some(tolerance) = cache_fit_tolerance {
            definition
                .set_legacy_cache(Some(
                    cadmpeg_ir::geometry::LegacyCache::try_new(tolerance)
                        .map_err(cadmpeg_core::CodecError::malformed)?,
                ))
                .map_err(cadmpeg_core::CodecError::malformed)?;
        }
        let surface = ProceduralSurface::new(
            brep_id!(format, ProceduralSurfaceId, "procedural_surface", i),
            definition,
            nurbs::proc_curve::record_trailing_surface_bounds(&r.tokens)
                .map(cadmpeg_ir::geometry::RecordBounds::try_from)
                .transpose()
                .map_err(cadmpeg_core::CodecError::malformed)?,
        );
        charged_push!(ctx, out.procedural_surfaces, (SurfaceId::from(id(format, i)), surface));
    } else if cached_unknown_procedural_surfaces.contains(&i) {
        charged_push!(ctx, out.procedural_surfaces, (
            <SurfaceId>::from(id(format, i)),
            ProceduralSurface::new(
                brep_id!(format, ProceduralSurfaceId, "procedural_surface", i),
                ProceduralSurfaceDefinition::Unknown {
                    record: Some(unknown_record_id(r, format)?),
                    cache: None,
                },
                None,
            ),
        ));
    }

    Ok(())
}

/// Emit a kept 3D curve carrier (with its `:reversed` clone when shared) and
/// any procedural-curve construction and nested support carriers.
fn emit_deformable_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,

    out: &mut AsmBrep,
    i: i64,
    embedded: Box<EmbeddedDeformableSurface>,
    format: IdFormat,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let embedded = *embedded;
    let support = brep_id!(
        format,
        SurfaceId,
        "procedural_surface",
        brep_key!(i, ":deformable:support")
    );
    let (revision_form, discontinuities, discontinuity_flag) = match embedded.layout {
        EmbeddedDeformableSurfaceLayout::Legacy {
            discontinuities,
            discontinuity_flag,
        } => (None, discontinuities, discontinuity_flag),
        EmbeddedDeformableSurfaceLayout::Revision(form) => {
            let discontinuities = form.discontinuities.clone();
            let flag = form.tail_flag;
            (Some(*form), discontinuities, flag)
        }
    };
    charged_push!(ctx, out.surfaces, Surface {
        id: support.clone(),
        geometry: embedded.support,
        source_object: None,
    });
    let data = match embedded.data {
        EmbeddedDeformableSurfaceData::Resolved(data) => data,
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
        } => {
            let secondary_surface = brep_id!(
                format,
                SurfaceId,
                "procedural_surface",
                brep_key!(i, ":deformable:secondary")
            );
            charged_push!(ctx, out.surfaces, Surface {
                id: secondary_surface.clone(),
                geometry: surface,
                source_object: None,
            });
            let curve_id = brep_id!(
                format,
                CurveId,
                "procedural_surface",
                brep_key!(i, ":deformable:curve")
            );
            charged_push!(ctx, out.curves, Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                source_object: None,
            });
            cadmpeg_ir::geometry::DeformableSurfaceData::SurfaceCurve {
                surface: secondary_surface,
                native_id,
                flag,
                first_parameter,
                selector,
                second_parameter,
                curve: curve_id,
                vectors,
                frame_parameter,
                flags,
                parameter_triples,
            }
        }
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
            trailing_value,
        } => {
            let secondary_surface = brep_id!(
                format,
                SurfaceId,
                "procedural_surface",
                brep_key!(i, ":deformable:secondary")
            );
            charged_push!(ctx, out.surfaces, Surface {
                id: secondary_surface.clone(),
                geometry: surface,
                source_object: None,
            });
            let curve_id = brep_id!(
                format,
                CurveId,
                "procedural_surface",
                brep_key!(i, ":deformable:curve")
            );
            charged_push!(ctx, out.curves, Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                source_object: None,
            });
            cadmpeg_ir::geometry::DeformableSurfaceData::Full {
                leading_vectors,
                leading_parameter,
                leading_flags,
                selector,
                surface: secondary_surface,
                native_id,
                flag,
                first_parameter,
                version_value,
                second_parameter,
                curve: curve_id,
                frames,
                trailing_value,
            }
        }
    };
    Ok(ProceduralSurfaceDefinition::Deformable(
        cadmpeg_ir::geometry::surface_payloads::DeformableSurfacePayload::try_new(Box::new(
            cadmpeg_ir::geometry::DeformableSurfaceConstruction {
                support,
                data,
                cache: cadmpeg_ir::geometry::CacheContract::from_form(revision_form),
                discontinuities,
                discontinuity_flag,
            },
        ))
        .map_err(cadmpeg_core::CodecError::malformed)?,
    ))
}

fn emit_classic_loft_data(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,

    out: &mut AsmBrep,
    data: ClassicLoftProfileData,
    support_id: SurfaceId,
) -> Result<(i64, cadmpeg_ir::geometry::ClassicLoftProfileData), cadmpeg_core::CodecError> {
    charged_push!(ctx, out.surfaces, Surface {
        id: support_id.clone(),
        geometry: data.surface,
        source_object: None,
    });
    Ok((
        data.type_code,
        cadmpeg_ir::geometry::ClassicLoftProfileData {
            surface: support_id,
            pcurve: data.pcurve.map(|nurbs| PcurveGeometry::Nurbs { nurbs }),
            first_flag: data.first_flag,
            asm_extension: data.asm_extension,
            subdata: data.subdata,
            direction: data.direction,
        },
    ))
}

fn emit_loft_member_form(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,

    out: &mut AsmBrep,
    data: LoftProfileData,
    support_id: SurfaceId,
) -> Result<cadmpeg_ir::geometry::LoftMemberForm, cadmpeg_core::CodecError> {
    match data {
        LoftProfileData::Classic(data) => {
            let (type_code, data) = emit_classic_loft_data(ctx, out, data, support_id)?;
            Ok(cadmpeg_ir::geometry::LoftMemberForm::Support {
                type_code,
                surface: Some(data.surface),
                support_bounds: [None; 4],
                pcurve: data.pcurve,
                first_flag: data.first_flag,
                asm_extension: Some(data.asm_extension),
                subdata: data.subdata,
                direction: data.direction,
            })
        }
        LoftProfileData::RevisionSupport {
            endpoints: _,
            type_code,
            surface,
            support_bounds,
            pcurve,
            first_flag,
            asm_extension,
            subdata,
            direction,
        } => {
            let surface = surface.map(|geometry| -> Result<_, cadmpeg_core::CodecError> {
                charged_push!(ctx, out.surfaces, Surface {
                    id: support_id.clone(),
                    geometry,
                    source_object: None,
                });
                Ok(support_id.clone())
            }).transpose()?;
            Ok(cadmpeg_ir::geometry::LoftMemberForm::Support {
                type_code: type_code.get(),
                surface,
                support_bounds,
                pcurve: pcurve.map(|nurbs| PcurveGeometry::Nurbs { nurbs }),
                first_flag,
                asm_extension,
                subdata,
                direction,
            })
        }
        LoftProfileData::RevisionPcurvePair {
            endpoints: _,
            pcurve,
            secondary_pcurve,
            asm_extension,
            subdata,
            direction,
        } => Ok(cadmpeg_ir::geometry::LoftMemberForm::PcurvePair {
            pcurve: pcurve.map(|nurbs| PcurveGeometry::Nurbs { nurbs }),
            secondary_pcurve: secondary_pcurve.map(|nurbs| PcurveGeometry::Nurbs { nurbs }),
            asm_extension,
            subdata,
            direction,
        }),
    }
}

fn emit_loft_path_curve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,

    out: &mut AsmBrep,
    layout: EmbeddedLoftPathLayout,
    id: CurveId,
) -> Result<Option<LoftPathCurve>, cadmpeg_core::CodecError> {
    let (geometry, endpoints) = match layout {
        EmbeddedLoftPathLayout::Legacy(curve) => (curve, None),
        EmbeddedLoftPathLayout::Revision(curve) => {
            let Some(curve) = curve else { return Ok(None) };
            (curve.geometry, Some(curve.endpoints))
        }
    };
    charged_push!(ctx, out.curves, Curve {
        id: id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(geometry)),
        source_object: None,
    });
    Ok(Some(LoftPathCurve { id, endpoints }))
}

fn emit_loft_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    i: i64,
    embedded: EmbeddedLoft,
    format: IdFormat,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let mut map_section = |section_index: usize,
                           entries: Vec<EmbeddedLoftSectionEntry>|
     -> Result<cadmpeg_ir::geometry::LoftSection, cadmpeg_core::CodecError> {
        let entries = entries
            .into_iter()
            .enumerate()
            .map(|(entry_index, entry)| {
                let profile = entry
                    .profile
                    .into_iter()
                    .enumerate()
                    .map(|(member_index, member)|  -> Result<_, cadmpeg_core::CodecError> { Ok({
                        let curve = brep_id!(
                            format,
                            CurveId,
                            "procedural_surface",
                            brep_key!(
                                i,
                                ":loft:",
                                section_index,
                                ":",
                                entry_index,
                                ":profile:",
                                member_index
                            )
                        );
                        let endpoints = member.data.endpoints();
                        let form = emit_loft_member_form(ctx,
                            out,
                            member.data,
                            brep_id!(
                                format,
                                SurfaceId,
                                "procedural_surface",
                                brep_key!(
                                    i,
                                    ":loft:",
                                    section_index,
                                    ":",
                                    entry_index,
                                    ":support:",
                                    member_index
                                )
                            ),
                        )?;
                        charged_push!(ctx, out.curves, Curve {
                            id: curve.clone(),
                            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                                member.curve,
                            )),
                            source_object: None,
                        });
                        cadmpeg_ir::geometry::LoftProfileMember {
                            profile: LoftPathCurve {
                                id: curve,
                                endpoints,
                            },
                            form,
                        }
                    })})
                    .try_collect_counted_vec(ctx, "ASM loft profile members")?;
                let path_curve = emit_loft_path_curve(ctx,
                    out,
                    entry.path.layout,
                    brep_id!(
                        format,
                        CurveId,
                        "procedural_surface",
                        brep_key!(i, ":loft:", section_index, ":", entry_index, ":path")
                    ),
                )?;
                let auxiliaries = entry
                    .path
                    .auxiliaries
                    .into_iter()
                    .enumerate()
                    .map(|(auxiliary_index, geometry)|  -> Result<_, cadmpeg_core::CodecError> { Ok({
                        let id = brep_id!(
                            format,
                            CurveId,
                            "procedural_surface",
                            brep_key!(
                                i,
                                ":loft:",
                                section_index,
                                ":",
                                entry_index,
                                ":auxiliary:",
                                auxiliary_index
                            )
                        );
                        charged_push!(ctx, out.curves, Curve {
                            id: id.clone(),
                            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(geometry)),
                            source_object: None,
                        });
                        id
                    })})
                    .try_collect_counted_vec(ctx, "ASM loft auxiliary curves")?;
                Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::geometry::LoftSectionEntry {
                    parameter: entry.parameter,
                    profile,
                    path: cadmpeg_ir::geometry::LoftPath {
                        path: path_curve,
                        auxiliaries,
                        flag: entry.path.flag,
                    },
                })
            })
            .try_collect_counted_vec(ctx, "ASM loft section entries")?;
        Ok(cadmpeg_ir::geometry::LoftSection { entries })
    };
    let [first, second] = embedded.sections;
    let sections = [map_section(0, first)?, map_section(1, second)?];
    Ok(match embedded.layout {
        EmbeddedLoftLayout::Legacy {
            ranges,
            closures,
            singularities,
            mode,
            bridge,
        } => ProceduralSurfaceDefinition::Loft(
            cadmpeg_ir::geometry::surface_payloads::LoftSurfacePayload::try_new(
                sections,
                cadmpeg_ir::geometry::SplineSurfaceParameters::OrderedRanges { ranges },
                closures,
                singularities,
                mode,
                bridge,
                cadmpeg_ir::geometry::CacheContract::from_form(None),
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,
        ),
        EmbeddedLoftLayout::Revision(form, intervals) => ProceduralSurfaceDefinition::Loft(
            cadmpeg_ir::geometry::surface_payloads::LoftSurfacePayload::try_new(
                sections,
                cadmpeg_ir::geometry::SplineSurfaceParameters::RevisionRanges { intervals },
                [0; 2],
                [0; 2],
                0,
                Vec::new(),
                cadmpeg_ir::geometry::CacheContract::from_form(Some(*form)),
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,
        ),
    })
}

fn emit_compound_loft_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    i: i64,
    embedded: EmbeddedCompoundLoft,
    format: IdFormat,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let map_scale = |out: &mut AsmBrep,
                     name: cadmpeg_ir::ids::IdentityKey,
                     scale: EmbeddedCompoundLoftScale|
     -> Result<cadmpeg_ir::geometry::CompoundLoftScale, cadmpeg_core::CodecError> {
        let members = scale
            .members
            .into_iter()
            .enumerate()
            .map(|(member_index, member)|  -> Result<_, cadmpeg_core::CodecError> { Ok({
                let curve = brep_id!(
                    format,
                    CurveId,
                    "procedural_surface",
                    brep_key!(
                        i,
                        ":cloft:",
                        name.clone(),
                        ":member:",
                        member_index,
                        ":curve"
                    )
                );
                charged_push!(ctx, out.curves, Curve {
                    id: curve.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(member.curve)),
                    source_object: None,
                });
                let (type_code, data) = emit_classic_loft_data(ctx,
                    out,
                    member.data,
                    brep_id!(
                        format,
                        SurfaceId,
                        "procedural_surface",
                        brep_key!(
                            i,
                            ":cloft:",
                            name.clone(),
                            ":member:",
                            member_index,
                            ":surface"
                        )
                    ),
                )?;
                cadmpeg_ir::geometry::CompoundLoftScaleMember {
                    type_code,
                    curve,
                    data,
                }
            })})
            .try_collect_counted_vec(ctx, "ASM compound loft scale members")?;
        let path = brep_id!(
            format,
            CurveId,
            "procedural_surface",
            brep_key!(i, ":cloft:", name.clone(), ":path")
        );
        charged_push!(ctx, out.curves, Curve {
            id: path.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(scale.path)),
            source_object: None,
        });
        let auxiliaries = scale
            .auxiliaries
            .into_iter()
            .enumerate()
            .map(|(index, geometry)|  -> Result<_, cadmpeg_core::CodecError> { Ok({
                let id = brep_id!(
                    format,
                    CurveId,
                    "procedural_surface",
                    brep_key!(i, ":cloft:", name.clone(), ":auxiliary:", index)
                );
                charged_push!(ctx, out.curves, Curve {
                    id: id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(geometry)),
                    source_object: None,
                });
                id
            })})
            .try_collect_counted_vec(ctx, "ASM compound loft scale auxiliaries")?;
        Ok(cadmpeg_ir::geometry::CompoundLoftScale {
            members,
            path,
            auxiliaries,
            tail: scale.tail,
        })
    };
    let mut scale_index = 0;
    let scales = (*embedded.scales).map(|scale| {
        let name = brep_key!("scale", scale_index);
        scale_index += 1;
        scale.map(|scale| map_scale(&mut *out, name, scale)).transpose()
    });
    let [first, second, third, fourth] = scales;
    let scales = [first?, second?, third?, fourth?];
    let fifth_scale = embedded.fifth_scale.map(|scale| {
        map_scale(
            &mut *out,
            cadmpeg_ir::identity_key!("fifth"),
            *scale,
        ).map(Box::new)
    }).transpose()?;
    let tail = match embedded.tail {
        EmbeddedCompoundLoftTail::Six {
            flags,
            scale,
            selector,
            direction,
            parameter_range,
            curve,
        } => {
            let curve_id = brep_id!(
                format,
                CurveId,
                "procedural_surface",
                brep_key!(i, ":cloft:tail6:curve")
            );
            charged_push!(ctx, out.curves, Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                source_object: None,
            });
            cadmpeg_ir::geometry::CompoundLoftTail::Six {
                flags,
                scale: Box::new(map_scale(
                    &mut *out,
                    cadmpeg_ir::identity_key!("tail6"),
                    *scale,
                )?),
                selector,
                direction,
                parameter_range,
                curve: curve_id,
            }
        }
        EmbeddedCompoundLoftTail::Seven {
            first_flag,
            first_scale,
            second_flag,
            second_scale,
            selector,
            direction,
            trailing_flags,
        } => cadmpeg_ir::geometry::CompoundLoftTail::Seven {
            first_flag,
            first_scale: first_scale.map(|scale| {
                map_scale(
                    &mut *out,
                    cadmpeg_ir::identity_key!("tail7:first"),
                    *scale,
                ).map(Box::new)
            }).transpose()?,
            second_flag,
            second_scale: Box::new(map_scale(
                &mut *out,
                cadmpeg_ir::identity_key!("tail7:second"),
                *second_scale,
            )?),
            selector,
            direction,
            trailing_flags,
        },
        EmbeddedCompoundLoftTail::Zero {
            flags,
            direction,
            trailing_flags,
        } => {
            let direction = match direction {
                EmbeddedCompoundLoftDirection::Vector(value) => {
                    cadmpeg_ir::geometry::CompoundLoftDirection::Vector { value }
                }
                EmbeddedCompoundLoftDirection::Curve { selector, curve } => {
                    let id = brep_id!(
                        format,
                        CurveId,
                        "procedural_surface",
                        brep_key!(i, ":cloft:tail0:direction")
                    );
                    charged_push!(ctx, out.curves, Curve {
                        id: id.clone(),
                        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                        source_object: None,
                    });
                    cadmpeg_ir::geometry::CompoundLoftDirection::Curve {
                        curve: id,
                        selector,
                    }
                }
            };
            cadmpeg_ir::geometry::CompoundLoftTail::Zero {
                flags,
                direction,
                trailing_flags,
            }
        }
    };
    Ok(ProceduralSurfaceDefinition::CompoundLoft(
        cadmpeg_ir::geometry::surface_payloads::CompoundLoftSurfacePayload::try_new(
            cadmpeg_ir::geometry::CompoundLoftConstruction {
                scales: cadmpeg_ir::geometry::CompoundLoftScales::try_from_slots(
                    scales.into_iter().chain([fifth_scale.map(|scale| *scale)]),
                )
                .map_err(cadmpeg_core::CodecError::malformed)?,
                flags: embedded.flags,
                tail,
            },
            None,
        )
        .map_err(cadmpeg_core::CodecError::malformed)?,
    ))
}

fn emit_scaled_compound_loft_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    i: i64,
    embedded: Box<EmbeddedScaledCompoundLoft>,
    format: IdFormat,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let embedded = *embedded;
    let map_scale = |out: &mut AsmBrep,
                     name: cadmpeg_ir::ids::IdentityKey,
                     scale: EmbeddedCompoundLoftScale|
     -> Result<cadmpeg_ir::geometry::CompoundLoftScale, cadmpeg_core::CodecError> {
        let members = scale
            .members
            .into_iter()
            .enumerate()
            .map(|(member_index, member)|  -> Result<_, cadmpeg_core::CodecError> { Ok({
                let curve = brep_id!(
                    format,
                    CurveId,
                    "procedural_surface",
                    brep_key!(
                        i,
                        ":scaled_cloft:",
                        name.clone(),
                        ":member:",
                        member_index,
                        ":curve"
                    )
                );
                charged_push!(ctx, out.curves, Curve {
                    id: curve.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(member.curve)),
                    source_object: None,
                });
                let (type_code, data) = emit_classic_loft_data(ctx,
                    out,
                    member.data,
                    brep_id!(
                        format,
                        SurfaceId,
                        "procedural_surface",
                        brep_key!(
                            i,
                            ":scaled_cloft:",
                            name.clone(),
                            ":member:",
                            member_index,
                            ":surface"
                        )
                    ),
                )?;
                cadmpeg_ir::geometry::CompoundLoftScaleMember {
                    type_code,
                    curve,
                    data,
                }
            })})
            .try_collect_counted_vec(ctx, "ASM scaled compound loft scale members")?;
        let path = brep_id!(
            format,
            CurveId,
            "procedural_surface",
            brep_key!(i, ":scaled_cloft:", name.clone(), ":path")
        );
        charged_push!(ctx, out.curves, Curve {
            id: path.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(scale.path)),
            source_object: None,
        });
        let auxiliaries = scale
            .auxiliaries
            .into_iter()
            .enumerate()
            .map(|(index, geometry)|  -> Result<_, cadmpeg_core::CodecError> { Ok({
                let id = brep_id!(
                    format,
                    CurveId,
                    "procedural_surface",
                    brep_key!(i, ":scaled_cloft:", name.clone(), ":auxiliary:", index)
                );
                charged_push!(ctx, out.curves, Curve {
                    id: id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(geometry)),
                    source_object: None,
                });
                id
            })})
            .try_collect_counted_vec(ctx, "ASM scaled compound loft scale auxiliaries")?;
        Ok(cadmpeg_ir::geometry::CompoundLoftScale {
            members,
            path,
            auxiliaries,
            tail: scale.tail,
        })
    };
    let mut scale_index = 0;
    let scales = (*embedded.scales).map(|scale| {
        let name = brep_key!("scale", scale_index);
        scale_index += 1;
        scale.map(|scale| map_scale(&mut *out, name, scale)).transpose()
    });
    let [first, second, third] = scales;
    let scales = [first?, second?, third?];
    let map_direction = |out: &mut AsmBrep,
                         name: cadmpeg_ir::ids::IdentityKey,
                         direction|
     -> Result<cadmpeg_ir::geometry::CompoundLoftDirection, cadmpeg_core::CodecError> {
        Ok(match direction {
            EmbeddedCompoundLoftDirection::Vector(value) => {
                cadmpeg_ir::geometry::CompoundLoftDirection::Vector { value }
            }
            EmbeddedCompoundLoftDirection::Curve { selector, curve } => {
                let id = brep_id!(
                    format,
                    CurveId,
                    "procedural_surface",
                    brep_key!(i, ":scaled_cloft:", name)
                );
                charged_push!(ctx, out.curves, Curve {
                    id: id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                    source_object: None,
                });
                cadmpeg_ir::geometry::CompoundLoftDirection::Curve {
                    curve: id,
                    selector,
                }
            }
        })
    };
    let branch = match embedded.branch {
        EmbeddedScaledCompoundLoftBranch::ExtendedVector {
            first_scale,
            second_scale,
            selector,
            direction,
        } => cadmpeg_ir::geometry::ScaledCompoundLoftBranch::ExtendedVector {
            first_scale: first_scale.map(|scale| {
                map_scale(
                    &mut *out,
                    cadmpeg_ir::identity_key!("branch:first"),
                    *scale,
                ).map(Box::new)
            }).transpose()?,
            second_scale: Box::new(map_scale(
                &mut *out,
                cadmpeg_ir::identity_key!("branch:second"),
                *second_scale,
            )?),
            selector,
            direction,
        },
        EmbeddedScaledCompoundLoftBranch::ExtendedCurve {
            scale,
            flag,
            singularity,
            curve,
        } => {
            let id = brep_id!(
                format,
                CurveId,
                "procedural_surface",
                brep_key!(i, ":scaled_cloft:branch:curve")
            );
            charged_push!(ctx, out.curves, Curve {
                id: id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                source_object: None,
            });
            cadmpeg_ir::geometry::ScaledCompoundLoftBranch::ExtendedCurve {
                scale: scale.map(|scale| {
                    map_scale(
                        &mut *out,
                        cadmpeg_ir::identity_key!("branch"),
                        *scale,
                    ).map(Box::new)
                }).transpose()?,
                flag,
                singularity,
                curve: id,
            }
        }
        EmbeddedScaledCompoundLoftBranch::Direct { flag, direction } => {
            cadmpeg_ir::geometry::ScaledCompoundLoftBranch::Direct {
                flag,
                direction: map_direction(
                    &mut *out,
                    cadmpeg_ir::identity_key!("branch:direction"),
                    direction,
                )?,
            }
        }
    };
    let tail_curve = brep_id!(
        format,
        CurveId,
        "procedural_surface",
        brep_key!(i, ":scaled_cloft:tail:curve")
    );
    charged_push!(ctx, out.curves, Curve {
        id: tail_curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(embedded.tail_curve)),
        source_object: None,
    });
    let shape = match embedded.shape {
        EmbeddedScaledCompoundLoftShape::Full => {
            cadmpeg_ir::geometry::ScaledCompoundLoftShape::Full {}
        }
        EmbeddedScaledCompoundLoftShape::None {
            parameter_ranges,
            parameters,
        } => cadmpeg_ir::geometry::ScaledCompoundLoftShape::None {
            parameter_ranges,
            parameters,
        },
    };
    Ok(ProceduralSurfaceDefinition::ScaledCompoundLoft(
        cadmpeg_ir::geometry::surface_payloads::ScaledCompoundLoftSurfacePayload::try_new(
            Box::new(cadmpeg_ir::geometry::ScaledCompoundLoftConstruction {
                singularity: embedded.singularity,
                shape,
                discontinuities: embedded.discontinuities,
                discontinuity_flag: embedded.discontinuity_flag,
                scales: cadmpeg_ir::geometry::CompoundLoftScales::try_from_slots(scales)
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                flags: embedded.flags,
                selector: embedded.selector,
                branch,
                trailing_flags: embedded.trailing_flags,
                tail_kind: embedded.tail_kind,
                tail_directions: embedded.tail_directions,
                tail_singularity: embedded.tail_singularity,
                tail_curve,
            }),
            None,
        )
        .map_err(cadmpeg_core::CodecError::malformed)?,
    ))
}

#[derive(Clone)]
enum LawExpressionScope {
    Surface(cadmpeg_ir::ids::IdentityKey),
    Curve(cadmpeg_ir::ids::IdentityKey),
}

fn map_law_expression(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    format: IdFormat,
    scope: LawExpressionScope,
    path: cadmpeg_ir::ids::IdentityKey,
    expression: EmbeddedLawExpression,
) -> Result<cadmpeg_ir::geometry::LawExpression, cadmpeg_core::CodecError> {
    Ok(match expression {
        EmbeddedLawExpression::Null => cadmpeg_ir::geometry::LawExpression::Null {},
        EmbeddedLawExpression::Text(value) => cadmpeg_ir::geometry::LawExpression::Text { value },
        EmbeddedLawExpression::Integer(value) => {
            cadmpeg_ir::geometry::LawExpression::Integer { value }
        }
        EmbeddedLawExpression::Double(value) => {
            cadmpeg_ir::geometry::LawExpression::Double { value }
        }
        EmbeddedLawExpression::Point(value) => cadmpeg_ir::geometry::LawExpression::Point { value },
        EmbeddedLawExpression::Vector(value) => {
            cadmpeg_ir::geometry::LawExpression::Vector { value }
        }
        EmbeddedLawExpression::Transform { scalars, enums } => {
            cadmpeg_ir::geometry::LawExpression::Transform { scalars, enums }
        }
        EmbeddedLawExpression::TransformVec {
            vectors,
            scale,
            flags,
        } => cadmpeg_ir::geometry::LawExpression::TransformVec {
            vectors,
            scale,
            flags,
        },
        EmbeddedLawExpression::Edge {
            curve,
            endpoints,
            parameters,
        } => {
            // `prefix` ends on the `law` component and `path` starts a new
            // one, so the two are joined by a colon. `then` concatenates, and
            // using it here read `law` and `primary` as the single component
            // `lawprimary`, which two different component pairs can spell.
            let id = match scope {
                LawExpressionScope::Surface(prefix) => brep_id!(
                    format,
                    CurveId,
                    "procedural_surface",
                    prefix.colon(path).then(cadmpeg_ir::identity_key!(":edge"))
                ),
                LawExpressionScope::Curve(prefix) => {
                    brep_id!(format, CurveId, "procedural_curve", prefix.colon(path))
                }
            };
            charged_push!(ctx, out.curves, Curve {
                id: id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                source_object: None,
            });
            cadmpeg_ir::geometry::LawExpression::Edge {
                curve: LoftPathCurve { id, endpoints },
                parameters,
            }
        }
        EmbeddedLawExpression::Spline {
            native_id,
            knots,
            controls,
            point,
        } => cadmpeg_ir::geometry::LawExpression::Spline {
            native_id,
            knots,
            controls,
            point,
        },
        EmbeddedLawExpression::Algebraic { operator, operands } => {
            let mut mapped = crate::decode_alloc::counted_vec(
                ctx, operands.len(), "ASM law expression operands",
            )?;
            for (index, operand) in operands.into_iter().enumerate() {
                mapped.push(map_law_expression(ctx, out, format, scope.clone(), brep_key!(path.clone(), ":", index), operand,
                )?);
            }
            cadmpeg_ir::geometry::LawExpression::Algebraic {
                operator,
                operands: mapped,
            }
        }
    })
}

fn emit_law_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    i: i64,
    embedded: Box<EmbeddedLawSurface>,
    format: IdFormat,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let prefix = brep_key!(i, ":law");
    let scope = LawExpressionScope::Surface(prefix);
    let map_formula =
        |out: &mut AsmBrep, path: cadmpeg_ir::ids::IdentityKey, formula: EmbeddedLawFormula| {
            map_law_formula(ctx, formula, |index, expression| {
                map_law_expression(ctx, out,
                    format,
                    scope.clone(),
                    brep_key!(path.clone(), ":", index),
                    expression,
                )
            })
        };
    let embedded = *embedded;
    let primary = map_formula(
        &mut *out,
        cadmpeg_ir::identity_key!("primary"),
        embedded.primary,
    )?;
    let additional = embedded
        .additional
        .into_iter()
        .enumerate()
        .map(|(index, formula)| map_formula(&mut *out, brep_key!("additional:", index), formula))
        .try_collect_counted_vec(ctx, "ASM law surface additional formulas")?;
    Ok(ProceduralSurfaceDefinition::Law(
        cadmpeg_ir::geometry::surface_payloads::LawSurfacePayload::try_new(Box::new(
            cadmpeg_ir::geometry::LawSurfaceConstruction {
                parameter_ranges: embedded.parameter_ranges,
                primary,
                additional,
                tail: embedded.tail,
                discontinuities: embedded.discontinuities,
            },
        ))
        .map_err(cadmpeg_core::CodecError::malformed)?,
    ))
}

fn emit_skin_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    i: i64,
    embedded: Box<EmbeddedSkinSurface>,
    format: IdFormat,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let prefix = brep_key!(i, ":skin:law");
    let scope = LawExpressionScope::Surface(prefix);
    let embedded = *embedded;
    let layout = match embedded.layout {
        EmbeddedSkinSurfaceLayout::Compact {
            inner_count,
            curve,
            subdata,
            first_tail,
            secondary_curve,
            second_tail,
        } => {
            let curve_id = brep_id!(
                format,
                CurveId,
                "procedural_surface",
                brep_key!(i, ":skin:curve")
            );
            charged_push!(ctx, out.curves, Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                source_object: None,
            });
            let secondary_id = brep_id!(
                format,
                CurveId,
                "procedural_surface",
                brep_key!(i, ":skin:secondary")
            );
            charged_push!(ctx, out.curves, Curve {
                id: secondary_id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(secondary_curve)),
                source_object: None,
            });
            cadmpeg_ir::geometry::SkinSurfaceLayout::Compact {
                inner_count,
                curve: curve_id,
                subdata,
                first_tail,
                secondary_curve: secondary_id,
                second_tail,
            }
        }
        EmbeddedSkinSurfaceLayout::Profiles {
            profiles,
            path,
            tail,
        } => {
            let profiles = profiles
                .into_iter()
                .enumerate()
                .map(|(index, profile)|  -> Result<_, cadmpeg_core::CodecError> { Ok({
                    let curve = brep_id!(
                        format,
                        CurveId,
                        "procedural_surface",
                        brep_key!(i, ":skin:profile:", index, ":curve")
                    );
                    charged_push!(ctx, out.curves, Curve {
                        id: curve.clone(),
                        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(profile.curve)),
                        source_object: None,
                    });
                    let (type_code, data) = emit_classic_loft_data(ctx,
                        out,
                        profile.data,
                        brep_id!(
                            format,
                            SurfaceId,
                            "procedural_surface",
                            brep_key!(i, ":skin:profile:", index, ":surface")
                        ),
                    )?;
                    cadmpeg_ir::geometry::SkinSurfaceProfile {
                        type_code,
                        curve,
                        data,
                    }
                })})
                .try_collect_counted_vec(ctx, "ASM skin surface profiles")?;
            let path_id = brep_id!(
                format,
                CurveId,
                "procedural_surface",
                brep_key!(i, ":skin:path")
            );
            charged_push!(ctx, out.curves, Curve {
                id: path_id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(path)),
                source_object: None,
            });
            cadmpeg_ir::geometry::SkinSurfaceLayout::Profiles {
                profiles,
                path: path_id,
                tail,
            }
        }
    };
    let parameter_curve = brep_id!(
        format,
        CurveId,
        "procedural_surface",
        brep_key!(i, ":skin:parameter_curve")
    );
    charged_push!(ctx, out.curves, Curve {
        id: parameter_curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(embedded.parameter_curve)),
        source_object: None,
    });
    let formula = map_law_formula(ctx, embedded.formula, |variable_index, variable| {
        map_law_expression(ctx, &mut *out,
            format,
            scope.clone(),
            variable_index.into(),
            variable,
        )
    })?;
    Ok(ProceduralSurfaceDefinition::Skin(
        cadmpeg_ir::geometry::surface_payloads::SkinSurfacePayload::try_new(
            Box::new(cadmpeg_ir::geometry::SkinSurfaceConstruction {
                surface_boolean: embedded.surface_boolean,
                surface_normal: embedded.surface_normal,
                surface_direction: embedded.surface_direction,
                count: embedded.count,
                parameter: embedded.parameter,
                layout,
                direction: embedded.direction,
                trailing_parameter: embedded.trailing_parameter,
                formula,
                parameter_curve,
                discontinuities: embedded.discontinuities,
                discontinuity_flag: embedded.discontinuity_flag,
            }),
            None,
        )
        .map_err(cadmpeg_core::CodecError::malformed)?,
    ))
}

fn emit_net_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    i: i64,
    embedded: Box<EmbeddedNetSurface>,
    format: IdFormat,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let prefix = brep_key!(i, ":net:law");
    let scope = LawExpressionScope::Surface(prefix);
    let embedded = *embedded;
    let mut map_section = |section_index: usize,
                           entries: Vec<EmbeddedLoftSectionEntry>|
     -> Result<cadmpeg_ir::geometry::LoftSection, cadmpeg_core::CodecError> {
        let entries = entries
            .into_iter()
            .enumerate()
            .map(|(entry_index, entry)| {
                let profile = entry
                    .profile
                    .into_iter()
                    .enumerate()
                    .map(|(member_index, member)|  -> Result<_, cadmpeg_core::CodecError> { Ok({
                        let curve = brep_id!(
                            format,
                            CurveId,
                            "procedural_surface",
                            brep_key!(
                                i,
                                ":net:",
                                section_index,
                                ":",
                                entry_index,
                                ":member:",
                                member_index,
                                ":curve"
                            )
                        );
                        let endpoints = member.data.endpoints();
                        let form = emit_loft_member_form(ctx,
                            out,
                            member.data,
                            brep_id!(
                                format,
                                SurfaceId,
                                "procedural_surface",
                                brep_key!(
                                    i,
                                    ":net:",
                                    section_index,
                                    ":",
                                    entry_index,
                                    ":member:",
                                    member_index,
                                    ":surface"
                                )
                            ),
                        )?;
                        charged_push!(ctx, out.curves, Curve {
                            id: curve.clone(),
                            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                                member.curve,
                            )),
                            source_object: None,
                        });
                        cadmpeg_ir::geometry::LoftProfileMember {
                            profile: LoftPathCurve {
                                id: curve,
                                endpoints,
                            },
                            form,
                        }
                    })})
                    .try_collect_counted_vec(ctx, "ASM net surface profile members")?;
                let path = emit_loft_path_curve(ctx,
                    out,
                    entry.path.layout,
                    brep_id!(
                        format,
                        CurveId,
                        "procedural_surface",
                        brep_key!(i, ":net:", section_index, ":", entry_index, ":path")
                    ),
                )?;
                let auxiliaries = entry
                    .path
                    .auxiliaries
                    .into_iter()
                    .enumerate()
                    .map(|(index, geometry)|  -> Result<_, cadmpeg_core::CodecError> { Ok({
                        let id = brep_id!(
                            format,
                            CurveId,
                            "procedural_surface",
                            brep_key!(
                                i,
                                ":net:",
                                section_index,
                                ":",
                                entry_index,
                                ":auxiliary:",
                                index
                            )
                        );
                        charged_push!(ctx, out.curves, Curve {
                            id: id.clone(),
                            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(geometry)),
                            source_object: None,
                        });
                        id
                    })})
                    .try_collect_counted_vec(ctx, "ASM net surface auxiliary curves")?;
                Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::geometry::LoftSectionEntry {
                    parameter: entry.parameter,
                    profile,
                    path: cadmpeg_ir::geometry::LoftPath {
                        path,
                        auxiliaries,
                        flag: entry.path.flag,
                    },
                })
            })
            .try_collect_counted_vec(ctx, "ASM net surface section entries")?;
        Ok(cadmpeg_ir::geometry::LoftSection { entries })
    };
    let [first, second] = *embedded.sections;
    let sections = Box::new([map_section(0, first)?, map_section(1, second)?]);
    let [first_formula, second_formula, third_formula, fourth_formula] = *embedded.formulas;
    let mut map_formula = |formula_index, formula| {
        map_law_formula(ctx, formula, |index, variable| {
            map_law_expression(ctx, &mut *out,
                format,
                scope.clone(),
                brep_key!(formula_index, ":", index),
                variable,
            )
        })
    };
    let formulas = [
        map_formula(0, first_formula)?,
        map_formula(1, second_formula)?,
        map_formula(2, third_formula)?,
        map_formula(3, fourth_formula)?,
    ];
    Ok(ProceduralSurfaceDefinition::Net(
        cadmpeg_ir::geometry::surface_payloads::NetSurfacePayload::try_new(
            Box::new(cadmpeg_ir::geometry::NetSurfaceConstruction {
                sections,
                frame_parameters: embedded.frame_parameters,
                flag: embedded.flag,
                directions: embedded.directions,
                formulas: Box::new(formulas),
                discontinuities: embedded.discontinuities,
                discontinuity_flag: embedded.discontinuity_flag,
            }),
            None,
        )
        .map_err(cadmpeg_core::CodecError::malformed)?,
    ))
}

fn emit_sweep_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    i: i64,
    embedded: Box<EmbeddedSweepSurface>,
    format: IdFormat,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let prefix = brep_key!(i, ":sweep:law");
    let scope = LawExpressionScope::Surface(prefix);
    let embedded = *embedded;
    let (primary_kind, revision_form, layout) = match embedded.layout {
        EmbeddedSweepSurfaceLayout::Legacy {
            primary_kind,
            layout,
        } => (primary_kind, None, layout),
        EmbeddedSweepSurfaceLayout::Revision {
            form,
            profile,
            tail,
        } => (
            0,
            Some(form),
            LegacySweepLayout::Sweep {
                profile,
                tail: crate::nurbs::proc_surface::SweepTail::LawOrFormula(tail),
            },
        ),
    };
    let (profile_geometry, spine_geometry, layout) = match layout {
        LegacySweepLayout::ProfileFirst {
            profile,
            spine,
            secondary_kind,
            directions,
            origin,
            parameters,
            formulas,
        } => {
            let [first_formula, second_formula, third_formula] = *formulas;
            let mut map_formula = |formula_index, formula| {
                map_law_formula(ctx, formula, |index, variable| {
                    map_law_expression(ctx, &mut *out,
                        format,
                        scope.clone(),
                        brep_key!(formula_index, ":", index),
                        variable,
                    )
                })
            };
            let formulas = [
                map_formula(0, first_formula)?,
                map_formula(1, second_formula)?,
                map_formula(2, third_formula)?,
            ];
            (
                profile,
                spine,
                cadmpeg_ir::geometry::SweepSurfaceLayout::ProfileFirst {
                    secondary_kind,
                    directions,
                    origin,
                    parameters,
                    formulas: Box::new(formulas),
                },
            )
        }
        LegacySweepLayout::Sweep {
            profile:
                crate::nurbs::proc_surface::SweepProfile {
                    profile,
                    mode,
                    profile_range,
                    profile_frame,
                    origin,
                    directions,
                    path,
                    path_range,
                    path_parameter,
                },
            tail,
        } => {
            let layout = match tail {
                crate::nurbs::proc_surface::SweepTail::LawOrFormula(
                    SweepLawOrFormula::Formula {
                        trajectory_flag,
                        formula_flag,
                        formula,
                        trailing_flag,
                    },
                ) => {
                    let formula = map_law_formula(ctx, formula, |index, variable| {
                        map_law_expression(ctx, &mut *out,
                            format,
                            scope.clone(),
                            brep_key!("explicit:", index),
                            variable,
                        )
                    })?;
                    cadmpeg_ir::geometry::SweepSurfaceLayout::ExplicitFormula {
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
                    }
                }
                crate::nurbs::proc_surface::SweepTail::Guide {
                    trajectory_flag,
                    guide_flags,
                    guide_curve,
                    guide_range,
                    guide_modes,
                    guide_parameters,
                    trailing_flags,
                } => {
                    let guide_curve_id = brep_id!(
                        format,
                        CurveId,
                        "procedural_surface",
                        brep_key!(i, ":sweep:guide")
                    );
                    charged_push!(ctx, out.curves, Curve {
                        id: guide_curve_id.clone(),
                        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(guide_curve)),
                        source_object: None,
                    });
                    cadmpeg_ir::geometry::SweepSurfaceLayout::ExplicitGuide {
                        mode,
                        profile_range,
                        profile_frame,
                        origin,
                        directions,
                        trajectory_flag,
                        path_range,
                        path_parameter,
                        guide_flags,
                        guide_curve: guide_curve_id,
                        guide_range,
                        guide_modes,
                        guide_parameters,
                        trailing_flags,
                    }
                }
                crate::nurbs::proc_surface::SweepTail::Surface {
                    trajectory_flag,
                    singularity,
                    support_surface,
                    auxiliary_curve,
                    support_flag,
                    legacy_flag,
                } => {
                    let support_surface_id = brep_id!(
                        format,
                        SurfaceId,
                        "procedural_surface",
                        brep_key!(i, ":sweep:support")
                    );
                    charged_push!(ctx, out.surfaces, Surface {
                        id: support_surface_id.clone(),
                        geometry: support_surface,
                        source_object: None,
                    });
                    let auxiliary_curve = auxiliary_curve.map(|geometry|  -> Result<_, cadmpeg_core::CodecError> { Ok({
                        let id = brep_id!(
                            format,
                            CurveId,
                            "procedural_surface",
                            brep_key!(i, ":sweep:auxiliary")
                        );
                        charged_push!(ctx, out.curves, Curve {
                            id: id.clone(),
                            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(geometry)),
                            source_object: None,
                        });
                        id
                    })}).transpose()?;
                    cadmpeg_ir::geometry::SweepSurfaceLayout::ExplicitSurface {
                        mode,
                        profile_range,
                        profile_frame,
                        origin,
                        directions,
                        trajectory_flag,
                        path_range,
                        path_parameter,
                        singularity,
                        support_surface: support_surface_id,
                        auxiliary_curve,
                        support_flag,
                        legacy_flag,
                    }
                }
                crate::nurbs::proc_surface::SweepTail::LawOrFormula(SweepLawOrFormula::Law {
                    first_law,
                    first_mode,
                    first_range,
                    law_direction,
                    path_mode,
                    path_flag,
                    second_law_flag,
                    second_law,
                    formula_mode,
                    formula,
                    trailing_flag,
                }) => {
                    let first_law = map_law_expression(ctx, &mut *out,
                        format,
                        scope.clone(),
                        cadmpeg_ir::identity_key!("law:first"),
                        *first_law,
                    )?;
                    let second_law = map_law_expression(ctx, &mut *out,
                        format,
                        scope.clone(),
                        cadmpeg_ir::identity_key!("law:second"),
                        *second_law,
                    )?;
                    let formula = map_law_formula(ctx, formula, |index, variable| {
                        map_law_expression(ctx, &mut *out,
                            format,
                            scope.clone(),
                            brep_key!("law:formula:", index),
                            variable,
                        )
                    })?;
                    cadmpeg_ir::geometry::SweepSurfaceLayout::LawDriven {
                        mode,
                        profile_range,
                        profile_frame,
                        origin,
                        directions,
                        first_law: Box::new(first_law),
                        first_mode,
                        first_range,
                        law_direction,
                        path_mode,
                        path_flag,
                        path_range,
                        path_parameter,
                        second_law_flag,
                        second_law: Box::new(second_law),
                        formula_mode,
                        formula,
                        trailing_flag,
                    }
                }
            };
            (profile, path, layout)
        }
    };
    let profile = brep_id!(
        format,
        CurveId,
        "procedural_surface",
        brep_key!(i, ":sweep:profile")
    );
    charged_push!(ctx, out.curves, Curve {
        id: profile.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(profile_geometry)),
        source_object: None,
    });
    let spine = brep_id!(
        format,
        CurveId,
        "procedural_surface",
        brep_key!(i, ":sweep:spine")
    );
    charged_push!(ctx, out.curves, Curve {
        id: spine.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(spine_geometry)),
        source_object: None,
    });
    Ok(ProceduralSurfaceDefinition::Sweep(
        cadmpeg_ir::geometry::surface_payloads::SweepSurfacePayload::try_new(
            profile,
            spine,
            Some(Box::new(cadmpeg_ir::geometry::SweepSurfaceConstruction {
                primary_kind,
                cache: cadmpeg_ir::geometry::CacheContract::from_form(revision_form),
                layout,
                discontinuities: embedded.discontinuities,
                discontinuity_flag: embedded.discontinuity_flag,
            })),
        )
        .map_err(cadmpeg_core::CodecError::malformed)?,
    ))
}

fn emit_g2_blend_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,

    out: &mut AsmBrep,
    i: i64,
    embedded: Box<EmbeddedG2Blend>,
    format: IdFormat,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let embedded = *embedded;
    let mut add_side = |name: cadmpeg_ir::ids::IdentityKey,
                        side: EmbeddedG2Side|
     -> Result<cadmpeg_ir::geometry::G2BlendSide, cadmpeg_core::CodecError> {
        let surface = brep_id!(
            format,
            SurfaceId,
            "procedural_surface",
            brep_key!(i, ":g2:", name.clone(), ":surface")
        );
        charged_push!(ctx, out.surfaces, Surface {
            id: surface.clone(),
            geometry: side.surface,
            source_object: None,
        });
        let curve = brep_id!(
            format,
            CurveId,
            "procedural_surface",
            brep_key!(i, ":g2:", name, ":curve")
        );
        charged_push!(ctx, out.curves, Curve {
            id: curve.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(side.curve)),
            source_object: None,
        });
        let pcurves = side
            .pcurves
            .map(|pcurve| pcurve.map(|nurbs| PcurveGeometry::Nurbs { nurbs }));
        Ok(cadmpeg_ir::geometry::G2BlendSide {
            label: side.label,
            surface,
            curve,
            pcurves,
            direction: side.direction,
        })
    };
    let first = add_side(cadmpeg_ir::identity_key!("first"), embedded.first)?;
    let second = add_side(cadmpeg_ir::identity_key!("second"), embedded.second)?;
    let first_shape = match embedded.first_shape {
        EmbeddedG2FirstShape::Full(support) => {
            let support = support.map(|(geometry, tolerance)|  -> Result<_, cadmpeg_core::CodecError> { Ok({
                let id = brep_id!(
                    format,
                    SurfaceId,
                    "procedural_surface",
                    brep_key!(i, ":g2:first_exact")
                );
                charged_push!(ctx, out.surfaces, Surface {
                    id: id.clone(),
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(geometry)),
                    source_object: None,
                });
                cadmpeg_ir::geometry::G2BlendFullSupport {
                    surface: id,
                    tolerance,
                }
            })}).transpose()?;
            cadmpeg_ir::geometry::G2BlendFirstShape::Full { support }
        }
        EmbeddedG2FirstShape::None {
            coefficients,
            tolerance,
            extension,
            pcurve,
        } => cadmpeg_ir::geometry::G2BlendFirstShape::None {
            coefficients,
            tolerance,
            extension,
            pcurve: pcurve.map(|nurbs| PcurveGeometry::Nurbs { nurbs }),
        },
    };
    let second_exact_surface = brep_id!(
        format,
        SurfaceId,
        "procedural_surface",
        brep_key!(i, ":g2:second_exact")
    );
    charged_push!(ctx, out.surfaces, Surface {
        id: second_exact_surface.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
            embedded.second_exact_surface,
        )),
        source_object: None,
    });
    let center_curve = brep_id!(
        format,
        CurveId,
        "procedural_surface",
        brep_key!(i, ":g2:center")
    );
    charged_push!(ctx, out.curves, Curve {
        id: center_curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(embedded.center_curve)),
        source_object: None,
    });
    Ok(ProceduralSurfaceDefinition::G2Blend(
        cadmpeg_ir::geometry::surface_payloads::G2BlendSurfacePayload::try_new(
            Box::new(cadmpeg_ir::geometry::G2BlendConstruction {
                first,
                singularity: embedded.singularity,
                first_shape,
                second,
                second_exact_surface,
                center_curve,
                center_parameters: embedded.center_parameters,
                center_flag: embedded.center_flag,
                parameter_ranges: embedded.parameter_ranges,
                trailing_parameters: embedded.trailing_parameters,
                discontinuities: embedded.discontinuities,
            }),
            None,
        )
        .map_err(cadmpeg_core::CodecError::malformed)?,
    ))
}

fn emit_rolling_ball_side(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,

    out: &mut AsmBrep,
    format: IdFormat,
    prefix: cadmpeg_ir::ids::IdentityKey,
    side: RollingBallSide<SurfaceGeometry, CurveGeometry, PcurveNurbs>,
) -> Result<RollingBallSide, cadmpeg_core::CodecError> {
    let surface = side.surface.map(|support| -> Result<_, cadmpeg_core::CodecError> {
        let id = brep_id!(
            format,
            SurfaceId,
            "procedural_surface",
            prefix.clone().then(cadmpeg_ir::identity_key!(":surface"))
        );
        charged_push!(ctx, out.surfaces, Surface {
            id: id.clone(),
            geometry: support.surface,
            source_object: None,
        });
        Ok(RollingBallSupportSurface {
            surface: id,
            parameter_ranges: support.parameter_ranges,
        })
    }).transpose()?;
    let curve = side.curve.map(|support| -> Result<_, cadmpeg_core::CodecError> {
        let id = brep_id!(
            format,
            CurveId,
            "procedural_surface",
            prefix.then(cadmpeg_ir::identity_key!(":curve"))
        );
        charged_push!(ctx, out.curves, Curve {
            id: id.clone(),
            geometry: support.curve,
            source_object: None,
        });
        Ok(RollingBallSupportCurve {
            curve: id,
            parameter_range: support.parameter_range,
        })
    }).transpose()?;
    Ok(RollingBallSide {
        support_kind: side.support_kind,
        surface,
        curve,
        pcurve: side.pcurve.map(|nurbs| PcurveGeometry::Nurbs { nurbs }),
        location: side.location,
        secondary_pcurve: side
            .secondary_pcurve
            .map(|nurbs| PcurveGeometry::Nurbs { nurbs }),
        extension: side.extension.map(|extension| RollingBallSideExtension {
            value: extension.value,
            pcurve: extension
                .pcurve
                .map(|nurbs| PcurveGeometry::Nurbs { nurbs }),
        }),
    })
}

fn emit_variable_blend_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,

    out: &mut AsmBrep,
    i: i64,
    construction: Box<EmbeddedVariableBlend>,
    format: IdFormat,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let [first_side, second_side] = *construction.sides;
    let sides = [
        {
            let prefix = brep_key!(i, ":variable_side0");
            emit_rolling_ball_side(ctx, out, format, prefix, first_side)?
        },
        {
            let prefix = brep_key!(i, ":variable_side1");
            emit_rolling_ball_side(ctx, out, format, prefix, second_side)?
        },
    ];
    let mut add_curve =
        |suffix: cadmpeg_ir::ids::IdentityKey, geometry: CurveGeometry| -> Result<CurveId, cadmpeg_core::CodecError> {
            let id = brep_id!(
                format,
                CurveId,
                "procedural_surface",
                brep_key!(i, ":variable_", suffix)
            );
            charged_push!(ctx, out.curves, Curve {
                id: id.clone(),
                geometry,
                source_object: None,
            });
            Ok(id)
        };
    let slice = add_curve(cadmpeg_ir::identity_key!("slice"), construction.slice)?;
    let secondary_curve = construction
        .secondary_curve
        .map(|support| -> Result<_, cadmpeg_core::CodecError> { Ok(RollingBallSupportCurve {
            curve: add_curve(cadmpeg_ir::identity_key!("secondary"), support.curve)?,
            parameter_range: support.parameter_range,
        }) }).transpose()?;
    let post_curve = construction.post_curve.map(|curve| {
        add_curve(
            cadmpeg_ir::identity_key!("post"),
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
        )
    }).transpose()?;
    Ok(ProceduralSurfaceDefinition::VariableBlend(
        cadmpeg_ir::geometry::surface_payloads::VariableBlendSurfacePayload::try_new(Box::new(
            VariableBlendConstruction {
                subtype: construction.subtype,
                revision: construction.revision,
                sides,
                slice,
                slice_range: construction.slice_range,
                offsets: construction.offsets,
                radii: construction.radii,
                cross_section: construction.cross_section,
                u_range: construction.u_range,
                v_lower: construction.v_lower,
                shape_parameter: construction.shape_parameter,
                shape_length: construction.shape_length,
                shape_tail: construction.shape_tail,
                cache: construction.cache,
                discontinuities: construction.discontinuities,
                tail_flag: construction.tail_flag,
                tail_extensions: construction.tail_extensions,
                secondary_curve,
                convexity: construction.convexity,
                render_mode: construction.render_mode,
                post_range: construction.post_range,
                post_curve,
                post_pcurve: construction
                    .post_pcurve
                    .map(|nurbs| PcurveGeometry::Nurbs { nurbs }),
            },
        ))
        .map_err(cadmpeg_core::CodecError::malformed)?,
    ))
}

fn emit_revision_compound_loft_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    i: i64,
    construction: Box<EmbeddedRevisionCompoundLoft>,
    format: IdFormat,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let convert_profile = |scope: cadmpeg_ir::ids::IdentityKey,
                           profile: Vec<EmbeddedLoftProfileMember>,
                           out: &mut AsmBrep|
     -> Result<Vec<cadmpeg_ir::geometry::LoftProfileMember>, cadmpeg_core::CodecError> {
        profile
            .into_iter()
            .enumerate()
            .map(|(member_index, member)|  -> Result<_, cadmpeg_core::CodecError> { Ok({
                let curve = brep_id!(
                    format,
                    CurveId,
                    "procedural_surface",
                    brep_key!(scope.clone(), ":profile:", member_index)
                );
                charged_push!(ctx, out.curves, Curve {
                    id: curve.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(member.curve)),
                    source_object: None,
                });
                cadmpeg_ir::geometry::LoftProfileMember {
                    profile: LoftPathCurve {
                        id: curve,
                        endpoints: member.data.endpoints(),
                    },
                    form: emit_loft_member_form(ctx,
                        out,
                        member.data,
                        brep_id!(
                            format,
                            SurfaceId,
                            "procedural_surface",
                            brep_key!(scope.clone(), ":support:", member_index)
                        ),
                    )?,
                }
            })})
            .try_collect_counted_vec(ctx, "ASM revision compound loft profile members")
    };
    let convert_path = |scope: cadmpeg_ir::ids::IdentityKey,
                        path: EmbeddedLoftPath,
                        out: &mut AsmBrep|
     -> Result<cadmpeg_ir::geometry::LoftPath, cadmpeg_core::CodecError> {
        let curve = emit_loft_path_curve(ctx,
            out,
            path.layout,
            brep_id!(
                format,
                CurveId,
                "procedural_surface",
                brep_key!(scope.clone(), ":path")
            ),
        )?;
        let auxiliaries = path
            .auxiliaries
            .into_iter()
            .enumerate()
            .map(|(auxiliary_index, geometry)|  -> Result<_, cadmpeg_core::CodecError> { Ok({
                let id = brep_id!(
                    format,
                    CurveId,
                    "procedural_surface",
                    brep_key!(scope.clone(), ":auxiliary:", auxiliary_index)
                );
                charged_push!(ctx, out.curves, Curve {
                    id: id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(geometry)),
                    source_object: None,
                });
                id
            })})
            .try_collect_counted_vec(ctx, "ASM revision compound loft auxiliary curves")?;
        Ok(cadmpeg_ir::geometry::LoftPath {
            path: curve,
            auxiliaries,
            flag: path.flag,
        })
    };
    let base = brep_key!(i, ":cloft:base");
    let base_profile = convert_profile(base.clone(), construction.base_profile, &mut *out)?;
    let base_path = convert_path(base, construction.base_path, &mut *out)?;
    let entries: Vec<_> = construction
        .entries
        .into_iter()
        .enumerate()
        .map(|(entry_index, entry)| {
            let scope = brep_key!(i, ":cloft:", entry_index);
            Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::geometry::LoftSectionEntry {
                parameter: entry.parameter,
                profile: convert_profile(scope.clone(), entry.profile, &mut *out)?,
                path: convert_path(scope, entry.path, &mut *out)?,
            })
        })
        .try_collect_counted_vec(ctx, "ASM revision compound loft sections")?;
    let direction = match construction.direction {
        EmbeddedCompoundLoftDirection::Vector(value) => {
            cadmpeg_ir::geometry::CompoundLoftDirection::Vector { value }
        }
        EmbeddedCompoundLoftDirection::Curve { selector, curve } => {
            let id = brep_id!(
                format,
                CurveId,
                "procedural_surface",
                brep_key!(i, ":cloft:direction")
            );
            charged_push!(ctx, out.curves, Curve {
                id: id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                source_object: None,
            });
            cadmpeg_ir::geometry::CompoundLoftDirection::Curve {
                curve: id,
                selector,
            }
        }
    };
    let tail = match construction.tail {
        cadmpeg_ir::geometry::RevisionCompoundLoftTail::Unbounded {} => {
            cadmpeg_ir::geometry::RevisionCompoundLoftTail::Unbounded {}
        }
        cadmpeg_ir::geometry::RevisionCompoundLoftTail::LowerBound { lower } => {
            cadmpeg_ir::geometry::RevisionCompoundLoftTail::LowerBound { lower }
        }
        cadmpeg_ir::geometry::RevisionCompoundLoftTail::UpperBound { upper } => {
            cadmpeg_ir::geometry::RevisionCompoundLoftTail::UpperBound { upper }
        }
        cadmpeg_ir::geometry::RevisionCompoundLoftTail::Curve { interval, curve } => {
            let id = brep_id!(
                format,
                CurveId,
                "procedural_surface",
                brep_key!(i, ":cloft:trailing")
            );
            charged_push!(ctx, out.curves, Curve {
                id: id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                source_object: None,
            });
            cadmpeg_ir::geometry::RevisionCompoundLoftTail::Curve {
                interval,
                curve: id,
            }
        }
    };
    Ok(ProceduralSurfaceDefinition::RevisionCompoundLoft {
        construction: Box::new(
            cadmpeg_ir::geometry::RevisionCompoundLoftConstruction::admit(
                cadmpeg_ir::geometry::RevisionCompoundLoftConstructionWire {
                    revision: construction.revision,
                    cache: construction.cache,
                    discontinuities: construction.discontinuities,
                    tail_flag: construction.tail_flag,
                    base_profile,
                    base_path,
                    entries,
                    flags: construction.flags,
                    kind_flags: construction.kind_flags,
                    direction,
                    tail,
                },
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,
        ),
    })
}

fn emit_revision_g2_blend_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,

    out: &mut AsmBrep,
    i: i64,
    construction: Box<EmbeddedRevisionG2Blend>,
    format: IdFormat,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let [first_side, second_side] = *construction.sides;
    let sides = [
        {
            let prefix = brep_key!(i, ":g2_side0");
            emit_rolling_ball_side(ctx, out, format, prefix, first_side)?
        },
        {
            let prefix = brep_key!(i, ":g2_side1");
            emit_rolling_ball_side(ctx, out, format, prefix, second_side)?
        },
    ];
    let center_id = brep_id!(
        format,
        CurveId,
        "procedural_surface",
        brep_key!(i, ":g2_center")
    );
    charged_push!(ctx, out.curves, Curve {
        id: center_id.clone(),
        geometry: construction.center,
        source_object: None,
    });
    Ok(ProceduralSurfaceDefinition::RevisionG2Blend {
        construction: Box::new(
            cadmpeg_ir::geometry::RevisionG2BlendConstruction::admit(
                cadmpeg_ir::geometry::RevisionG2BlendConstructionWire {
                    revision: construction.revision,
                    leading_parameters: construction.leading_parameters,
                    sides: Box::new(sides),
                    center: center_id,
                    center_range: construction.center_range,
                    radii: construction.radii,
                    radius_selector: construction.radius_selector,
                    u_range: construction.u_range,
                    v_range: construction.v_range,
                    shape_prefix: construction.shape_prefix,
                    shape_parameter: construction.shape_parameter,
                    shape_length: construction.shape_length,
                    shape_tail: construction.shape_tail,
                    cache: construction.cache,
                    discontinuities: construction.discontinuities,
                    tail_flag: construction.tail_flag,
                    tail_extensions: construction.tail_extensions,
                },
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,
        ),
    })
}

fn emit_vertex_blend_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    i: i64,
    construction: EmbeddedVertexBlend,
    format: IdFormat,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let mut boundaries = crate::decode_alloc::counted_vec(
        ctx,
        construction.boundaries.len(),
        "ASM emitted vertex blend boundaries",
    )?;
    for (boundary_index, boundary) in construction.boundaries.into_iter().enumerate() {
        let prefix = brep_key!(i, ":vertex_boundary", boundary_index);
        let geometry = match boundary.geometry {
            EmbeddedVertexBlendBoundaryGeometry::Circle {
                curve,
                curve_endpoints,
                twists,
                parameters,
                sense,
            } => {
                let id = brep_id!(
                    format,
                    CurveId,
                    "procedural_surface",
                    prefix.clone().then(cadmpeg_ir::identity_key!(":curve"))
                );
                charged_push!(ctx, out.curves, Curve {
                    id: id.clone(),
                    geometry: curve,
                    source_object: None,
                });
                VertexBlendBoundaryGeometry::Circle {
                    curve: id,
                    curve_endpoints,
                    twists,
                    parameters,
                    sense,
                }
            }
            EmbeddedVertexBlendBoundaryGeometry::Degenerate { location, normals } => {
                VertexBlendBoundaryGeometry::Degenerate { location, normals }
            }
            EmbeddedVertexBlendBoundaryGeometry::Pcurve {
                surface,
                support_bounds,
                pcurve,
                sense,
                fit_tolerance,
            } => {
                let id = brep_id!(
                    format,
                    SurfaceId,
                    "procedural_surface",
                    prefix.clone().then(cadmpeg_ir::identity_key!(":surface"))
                );
                charged_push!(ctx, out.surfaces, Surface {
                    id: id.clone(),
                    geometry: surface,
                    source_object: None,
                });
                VertexBlendBoundaryGeometry::Pcurve {
                    surface: id,
                    support_bounds,
                    pcurve: pcurve.map(|nurbs| PcurveGeometry::Nurbs { nurbs }),
                    sense,
                    fit_tolerance,
                }
            }
            EmbeddedVertexBlendBoundaryGeometry::Plane {
                normal,
                parameters,
                curve,
                curve_endpoints,
            } => {
                let id = brep_id!(
                    format,
                    CurveId,
                    "procedural_surface",
                    prefix.then(cadmpeg_ir::identity_key!(":curve"))
                );
                charged_push!(ctx, out.curves, Curve {
                    id: id.clone(),
                    geometry: curve,
                    source_object: None,
                });
                VertexBlendBoundaryGeometry::Plane {
                    normal,
                    parameters,
                    curve: id,
                    curve_endpoints,
                }
            }
        };
        boundaries.push(VertexBlendBoundary {
            boundary_type: boundary.boundary_type,
            magic: boundary.magic,
            u_smoothing: boundary.u_smoothing,
            v_smoothing: boundary.v_smoothing,
            fullness: boundary.fullness,
            geometry,
        });
    }
    Ok(ProceduralSurfaceDefinition::VertexBlend(
        cadmpeg_ir::geometry::surface_payloads::VertexBlendSurfacePayload::try_new(
            VertexBlendConstruction {
                revision: construction.revision,
                boundaries,
                grid_size: construction.grid_size,
                fit_tolerance: construction.fit_tolerance,
            },
        )
        .map_err(cadmpeg_core::CodecError::malformed)?,
    ))
}

#[allow(clippy::too_many_arguments)]
fn emit_blend_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,

    out: &mut AsmBrep,
    i: i64,
    supports: Box<[Option<SurfaceGeometry>; 2]>,
    spine: Option<NurbsCurve>,
    radius_offsets: [f64; 2],
    cross_section: BlendCrossSection,
    native: Option<Box<EmbeddedRollingBall>>,
    format: IdFormat,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let mut resolved_supports = [None, None];
    for (side, support) in supports.into_iter().enumerate() {
        if let Some(support) = support {
            let support_id = brep_id!(
                format,
                SurfaceId,
                "procedural_surface",
                brep_key!(i, ":support", side)
            );
            charged_push!(ctx, out.surfaces, Surface {
                id: support_id.clone(),
                geometry: support,
                source_object: None,
            });
            resolved_supports[side] = Some(BlendSupport {
                surface: support_id,
                reversed: false,
            });
        }
    }
    let spine = spine.map(|spine|  -> Result<_, cadmpeg_core::CodecError> { Ok({
        let spine_id = brep_id!(
            format,
            CurveId,
            "procedural_surface",
            brep_key!(i, ":spine")
        );
        charged_push!(ctx, out.curves, Curve {
            id: spine_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(spine)),
            source_object: None,
        });
        spine_id
    })}).transpose()?;
    let native = native.map(|native|  -> Result<_, cadmpeg_core::CodecError> { Ok({
        let [first_native, second_native] = *native.sides;
        let resolved_sides = [
            {
                let prefix = brep_key!(i, ":native_side0");
                emit_rolling_ball_side(ctx, out, format, prefix, first_native)?
            },
            {
                let prefix = brep_key!(i, ":native_side1");
                emit_rolling_ball_side(ctx, out, format, prefix, second_native)?
            },
        ];
        for (side_index, side) in resolved_sides.iter().enumerate() {
            if resolved_supports[side_index].is_none() {
                resolved_supports[side_index] = side.surface.as_ref().map(|support| BlendSupport {
                    surface: support.surface.clone(),
                    reversed: false,
                });
            }
        }
        let slice = brep_id!(
            format,
            CurveId,
            "procedural_surface",
            brep_key!(i, ":native_slice")
        );
        charged_push!(ctx, out.curves, Curve {
            id: slice.clone(),
            geometry: native.slice,
            source_object: None,
        });
        let third = native.third.map(|side|  -> Result<_, cadmpeg_core::CodecError> { Ok({
            let prefix = brep_key!(i, ":native_third");
            let surface = brep_id!(
                format,
                SurfaceId,
                "procedural_surface",
                prefix.clone().then(cadmpeg_ir::identity_key!(":surface"))
            );
            charged_push!(ctx, out.surfaces, Surface {
                id: surface.clone(),
                geometry: side.surface,
                source_object: None,
            });
            let curve = brep_id!(
                format,
                CurveId,
                "procedural_surface",
                prefix.then(cadmpeg_ir::identity_key!(":curve"))
            );
            charged_push!(ctx, out.curves, Curve {
                id: curve.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(side.curve)),
                source_object: None,
            });
            Box::new(RollingBallThirdSide {
                label: side.label,
                surface,
                curve,
                pcurve: side.pcurve.map(|nurbs| PcurveGeometry::Nurbs { nurbs }),
                direction: side.direction,
                secondary_pcurve: side
                    .secondary_pcurve
                    .map(|nurbs| PcurveGeometry::Nurbs { nurbs }),
                extension: side.extension,
                tertiary_pcurve: side
                    .tertiary_pcurve
                    .map(|nurbs| PcurveGeometry::Nurbs { nurbs }),
                flag: side.flag,
            })
        })}).transpose()?;
        Box::new(RollingBallConstruction {
            revision: native.revision,
            sides: resolved_sides,
            slice,
            slice_range: native.slice_range,
            offsets: native.offsets,
            radius_selector: match native.radius_selector {
                None => RollingBallRadiusSelector::None {},
                Some(value) => RollingBallRadiusSelector::Value { value },
            },
            u_range: native.u_range,
            v_range: native.v_range,
            shape_prefix: native.shape_prefix,
            parameters: native.parameters,
            tail: native.tail,
            cache: native.cache,
            discontinuities: native.discontinuities,
            tail_flag: native.tail_flag,
            third,
            tail_extensions: native.tail_extensions,
        })
    })}).transpose()?;
    if resolved_supports
        .iter()
        .filter(|support| support.is_some())
        .count()
        == 1
        && native.is_none()
    {
        out.stats.partial_procedural_supports += 1;
    }
    Ok(ProceduralSurfaceDefinition::Blend(
        blend_radius_law(radius_offsets)
            .and_then(|radius| {
                cadmpeg_ir::geometry::surface_payloads::BlendSurfacePayload::try_new(
                    resolved_supports,
                    spine,
                    radius,
                    cross_section,
                    cadmpeg_ir::geometry::CacheContract::from_form(native),
                )
            })
            .map_err(cadmpeg_core::CodecError::malformed)?,
    ))
}

/// The radius law two signed offsets state: constant when they are equal,
/// linear from the first to the second otherwise.
fn blend_radius_law(
    offsets: [f64; 2],
) -> Result<BlendRadiusLaw, cadmpeg_ir::geometry::ProceduralGeometryError> {
    if offsets[0] == offsets[1] {
        BlendRadiusLaw::constant(offsets[0])
    } else {
        BlendRadiusLaw::linear(offsets[0], offsets[1])
    }
}

enum CarrierCurveError {
    Invalid(&'static str),
    Resource(cadmpeg_core::CodecError),
}

impl From<&'static str> for CarrierCurveError {
    fn from(cause: &'static str) -> Self {
        Self::Invalid(cause)
    }
}

impl From<cadmpeg_core::CodecError> for CarrierCurveError {
    fn from(error: cadmpeg_core::CodecError) -> Self {
        Self::Resource(error)
    }
}

fn emit_carrier_curve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    i: i64,
    carriers: &mut Carriers,
    reversed_curve_refs: &HashSet<i64>,
    forward_curve_refs: &HashSet<i64>,
    format: IdFormat,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::geometry::curve_payloads::{
        DeformableCurveConstruction, TwoSidedOffsetCurveConstruction, VectorOffsetCurveConstruction,
    };

    let Carriers {
        curve_geo,
        procedural_curve_defs,
        ..
    } = &mut *carriers;
    let Some(mut geometry) = curve_geo.remove(&i) else {
        return Ok(());
    };
    if reversed_curve_refs.contains(&i) {
        if forward_curve_refs.contains(&i) {
            let mut reversed = geometry.clone();
            reverse_curve_geometry(&mut reversed);
            charged_push!(ctx, out.curves, Curve {
                id: brep_id!(format, CurveId, "entity", brep_key!(i, ":reversed")),
                geometry: reversed,
                source_object: None,
            });
        } else {
            reverse_curve_geometry(&mut geometry);
        }
    }
    charged_push!(ctx, out.curves, Curve {
        id: <CurveId>::from(id(format, i)),
        geometry,
        source_object: None,
    });
    let surface_start = out.surfaces.len();
    let curve_start = out.curves.len();
    let procedural = match procedural_curve_defs.remove(&i) {
        Some(super::ProceduralCurveSource::Cached {
            construction,
            cache_fit_tolerance,
            parsed_domain: solved_domain,
        }) => {
            let definition = (|| -> Result<_, CarrierCurveError> {
                Ok(match *construction {
                    ProceduralCurveConstruction::VectorOffset((
                        source,
                        parameter_range,
                        offset,
                        roles,
                    )) => {
                        let source_id =
                            brep_id!(format, CurveId, "procedural_curve", brep_key!(i, ":source"));
                        charged_push!(ctx, out.curves, Curve {
                            id: source_id.clone(),
                            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(source)),
                            source_object: None,
                        });
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::VectorOffset(
                            VectorOffsetCurveConstruction::try_new(
                                source_id,
                                parameter_range,
                                offset,
                                roles,
                                None,
                            )
                            .map_err(|_| "vector-offset fields are not finite and ordered")?,
                        )
                    }
                    ProceduralCurveConstruction::Subset((source, parameter_range)) => {
                        let source_id =
                            brep_id!(format, CurveId, "procedural_curve", brep_key!(i, ":source"));
                        charged_push!(ctx, out.curves, Curve {
                            id: source_id.clone(),
                            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(source)),
                            source_object: None,
                        });
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset(
                            cadmpeg_ir::geometry::curve_payloads::SubsetCurveConstruction::try_new(
                                source_id,
                                parameter_range,
                                true,
                                None,
                            )
                            .map_err(|_| "subset-curve range is not finite and ordered")?,
                        )
                    }
                    ProceduralCurveConstruction::TwoSidedOffset(embedded) => {
                        let [first, second] = embedded.surfaces;
                        let mut emit_support = |side, geometry|  -> Result<_, CarrierCurveError> {
                            let Some(geometry) = geometry else { return Ok(None) };
                            let id = brep_id!(
                                format,
                                SurfaceId,
                                "procedural_curve",
                                brep_key!(i, ":support", side)
                            );
                            charged_push!(ctx, out.surfaces, Surface {
                                id: id.clone(),
                                geometry,
                                source_object: None,
                            });
                            Ok(Some(id))
                        };
                        let surfaces = [emit_support(0, first)?, emit_support(1, second)?];
                        let pcurves = embedded.pcurves.map(|pcurve| {
                            pcurve.map(|nurbs| {
                                cadmpeg_ir::geometry::SupportPcurve::from(PcurveGeometry::Nurbs {
                                    nurbs,
                                })
                            })
                        });
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::TwoSidedOffset(
                            TwoSidedOffsetCurveConstruction::try_new(
                                cadmpeg_ir::geometry::IntcurveSupportContext::try_new(
                                    std::array::from_fn(|side| {
                                        cadmpeg_ir::geometry::IntcurveSupportSide {
                                            surface: surfaces[side].clone(),
                                            pcurve: pcurves[side].clone(),
                                        }
                                    }),
                                    embedded.parameter_range,
                                    embedded.discontinuities,
                                )?,
                                embedded.discontinuity_flag,
                                embedded.offsets,
                                None,
                            )
                            .map_err(|_| "two-sided offset fields are not finite and ordered")?,
                        )
                    }
                    ProceduralCurveConstruction::Intersection(embedded, discontinuity_flag) => {
                        let [first, second] = embedded.surfaces;
                        let mut emit_support =
                            |side, slot: crate::nurbs::proc_curve::SupportSlot|  -> Result<_, CarrierCurveError> {
                                let Some(geometry) = slot.into_surface() else { return Ok(None) };
                                let id = brep_id!(
                                    format,
                                    SurfaceId,
                                    "procedural_curve",
                                    brep_key!(i, ":support", side)
                                );
                                charged_push!(ctx, out.surfaces, Surface {
                                    id: id.clone(),
                                    geometry,
                                    source_object: None,
                                });
                                Ok(Some(id))
                            };
                        let surfaces = [emit_support(0, first)?, emit_support(1, second)?];
                        let pcurves = embedded.pcurves.map(|pcurve| {
                            pcurve.map(|nurbs| {
                                cadmpeg_ir::geometry::SupportPcurve::from(PcurveGeometry::Nurbs {
                                    nurbs,
                                })
                            })
                        });
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection {
                            context: cadmpeg_ir::geometry::IntcurveSupportContext::try_new(
                                std::array::from_fn(|side| {
                                    cadmpeg_ir::geometry::IntcurveSupportSide {
                                        surface: surfaces[side].clone(),
                                        pcurve: pcurves[side].clone(),
                                    }
                                }),
                                embedded.parameter_range,
                                embedded.discontinuities,
                            )?,
                            discontinuity_flag,
                            cache: None,
                        }
                    }
                    ProceduralCurveConstruction::ThreeSurface(embedded) => {
                        let [first, second, third] = embedded.surfaces;
                        let mut emit_support = |side, geometry|  -> Result<_, CarrierCurveError> {
                            let id = brep_id!(
                                format,
                                SurfaceId,
                                "procedural_curve",
                                brep_key!(i, ":support", side)
                            );
                            charged_push!(ctx, out.surfaces, Surface {
                                id: id.clone(),
                                geometry,
                                source_object: None,
                            });
                            Ok(id)
                        };
                        let surface_ids = [
                            emit_support(0, first)?,
                            emit_support(1, second)?,
                            emit_support(2, third)?,
                        ];
                        let pcurves = embedded.pcurves.map(|nurbs| {
                            cadmpeg_ir::geometry::SupportPcurve::from(PcurveGeometry::Nurbs {
                                nurbs,
                            })
                        });
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::ThreeSurfaceIntersection(cadmpeg_ir::geometry::curve_payloads::ThreeSurfaceIntersectionCurvePayload::try_new(cadmpeg_ir::geometry::IntcurveSupportContext::try_new(
                                std::array::from_fn(|side| {
                                    cadmpeg_ir::geometry::IntcurveSupportSide {
                                        surface: Some(surface_ids[side].clone()),
                                        pcurve: Some(pcurves[side].clone()),
                                    }
                                }),
                                embedded.parameter_range,
                                embedded.discontinuities,
                            )?, embedded.selector, cadmpeg_ir::geometry::IntcurveSupportSide {
                                surface: Some(surface_ids[2].clone()),
                                pcurve: Some(pcurves[2].clone()),
                            }).map_err(|_| "three-surface intersection context is not finite and ordered")?)
                    }
                    ProceduralCurveConstruction::SurfaceCurve(family) => {
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve {
                            family: emit_surface_curve_family(ctx,
                                out,
                                i,
                                format,
                                family,
                                solved_domain,
                            )?,
                        }
                    }
                    ProceduralCurveConstruction::Silhouette(embedded) => {
                        emit_silhouette_curve(ctx, out, i, embedded, format)?
                    }
                    ProceduralCurveConstruction::SurfaceOffset(embedded) => {
                        emit_surface_offset_curve(ctx, out, i, embedded, format, solved_domain)?
                    }
                    ProceduralCurveConstruction::Spring(embedded) => {
                        emit_spring_curve(ctx, out, i, embedded, format, solved_domain)?
                    }
                    ProceduralCurveConstruction::Deformable(embedded) => {
                        let (context, form) = embedded.context.into_intersection(
                            solved_domain.ok_or("missing procedural curve cache domain")?,
                        );
                        let [first, second] = context.surfaces;
                        let mut emit_support =
                            |side, slot: crate::nurbs::proc_curve::SupportSlot|  -> Result<_, CarrierCurveError> {
                                let Some(geometry) = slot.into_surface() else { return Ok(None) };
                                let id = brep_id!(
                                    format,
                                    SurfaceId,
                                    "procedural_curve",
                                    brep_key!(i, ":deformable_support", side)
                                );
                                charged_push!(ctx, out.surfaces, Surface {
                                    id: id.clone(),
                                    geometry,
                                    source_object: None,
                                });
                                Ok(Some(id))
                            };
                        let support_ids = [emit_support(0, first)?, emit_support(1, second)?];
                        let pcurves = context.pcurves.map(|pcurve| {
                            pcurve.map(|nurbs| {
                                cadmpeg_ir::geometry::SupportPcurve::from(PcurveGeometry::Nurbs {
                                    nurbs,
                                })
                            })
                        });
                        let source = match embedded.source {
                        crate::nurbs::proc_curve::EmbeddedDeformableSource::Curve(geometry) => {
                            let curve = brep_id!(
                                format,
                                CurveId,
                                "procedural_curve",
                                brep_key!(i, ":deformable_source")
                            );
                            charged_push!(ctx, out.curves, Curve {
                                id: curve.clone(),
                                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(geometry)),
                                source_object: None,
                            });
                            cadmpeg_ir::geometry::DeformableCurveSource::Curve { curve }
                        }
                        crate::nurbs::proc_curve::EmbeddedDeformableSource::NativeReference {
                            flag,
                            index,
                        } => cadmpeg_ir::geometry::DeformableCurveSource::NativeReference {
                            flag,
                            index,
                        },
                    };
                        let data = match embedded.data {
                            EmbeddedDeformableData::VectorField {
                                vectors,
                                parameter_pairs,
                            } => cadmpeg_ir::geometry::DeformableCurveData::VectorField {
                                vectors,
                                parameter_pairs,
                            },
                            EmbeddedDeformableData::Mode3 {
                                leading_vectors,
                                leading_parameter,
                                leading_flags,
                                trailing_point,
                                trailing_vectors,
                                frame_parameter,
                                frame_flags,
                                parameters,
                                trailing_flags,
                                trailing_parameter,
                                trailing_value,
                            } => cadmpeg_ir::geometry::DeformableCurveData::Mode3 {
                                leading_vectors,
                                leading_parameter,
                                leading_flags,
                                trailing_point,
                                trailing_vectors,
                                frame_parameter,
                                frame_flags,
                                parameters,
                                trailing_flags,
                                trailing_parameter,
                                trailing_value,
                            },
                        };
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::Deformable(
                            DeformableCurveConstruction::try_new(
                                cadmpeg_ir::geometry::IntcurveSupportContext::try_new(
                                    std::array::from_fn(|side| {
                                        cadmpeg_ir::geometry::IntcurveSupportSide {
                                            surface: support_ids[side].clone(),
                                            pcurve: pcurves[side].clone(),
                                        }
                                    }),
                                    context.parameter_range,
                                    context.discontinuities,
                                )?,
                                form,
                                source,
                                embedded.source_parameter_range,
                                data,
                            )
                            .map_err(|_| "deformable curve payload is not finite")?,
                        )
                    }
                    ProceduralCurveConstruction::Projection(embedded) => {
                        emit_projection_curve(ctx, out, i, embedded, format)?
                    }
                    ProceduralCurveConstruction::Law(embedded) => {
                        emit_law_curve(ctx, out, i, embedded, format, solved_domain)?
                    }
                    ProceduralCurveConstruction::Compound(
                        crate::nurbs::proc_curve::CompoundDefinition {
                            parameters,
                            components,
                        },
                    ) => {
                        let components = components
                            .into_iter()
                            .enumerate()
                            .map(|(component, curve)|  -> Result<_, cadmpeg_core::CodecError> { Ok({
                                let id = brep_id!(
                                    format,
                                    CurveId,
                                    "procedural_curve",
                                    brep_key!(i, ":component:", component)
                                );
                                charged_push!(ctx, out.curves, Curve {
                                    id: id.clone(),
                                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                                        curve.component,
                                    )),
                                    source_object: None,
                                });
                                cadmpeg_ir::geometry::CompoundComponent {
                                    parameter: curve.parameter,
                                    component: id,
                                }
                            })})
                            .try_collect_counted_vec(ctx, "ASM compound curve components")?;
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound(
                            cadmpeg_ir::geometry::CompoundCurveConstruction::try_new(
                                parameters, components, None,
                            )?,
                        )
                    }
                    ProceduralCurveConstruction::Exact => {
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::Exact { cache: None }
                    }
                    ProceduralCurveConstruction::Helix(helix) => helix.into_definition()?,
                    ProceduralCurveConstruction::Unknown(native_kind) => {
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::Unknown {
                            native_kind: Some(native_kind),
                            record: None,
                            cache: None,
                        }
                    }
                })
            })();
            definition.and_then(|mut definition| {
                // A construction that owns a revision-gated cache form states
                // the tolerance inside it; the native legacy value is that same
                // tolerance and is not written a second time.
                if !definition.owns_revision_cache() {
                    if let Some(tolerance) = cache_fit_tolerance {
                        let cache = cadmpeg_ir::geometry::LegacyCache::try_new(tolerance)
                            .map_err(|_| "invalid procedural curve cache tolerance")?;
                        definition
                            .set_legacy_cache(cache)
                            .map_err(|_| "invalid procedural curve cache tolerance")?;
                    }
                }
                Ok(ProceduralCurve::new(
                    brep_id!(format, ProceduralCurveId, "procedural_curve", i),
                    definition,
                ))
            })
        }
        Some(super::ProceduralCurveSource::Cacheless(definition)) => Ok(ProceduralCurve::new(
            brep_id!(format, ProceduralCurveId, "procedural_curve", i),
            *definition,
        )),
        None => return Ok(()),
    };
    match procedural {
        Ok(procedural) => charged_push!(
            ctx,
            out.procedural_curves,
            (<CurveId>::from(id(format, i)), procedural)
        ),
        Err(CarrierCurveError::Resource(error)) => return Err(error),
        Err(CarrierCurveError::Invalid(cause)) => {
            out.surfaces.truncate(surface_start);
            out.curves.truncate(curve_start);
            count_kind(ctx, &mut out.stats.procedural_curve_kinds, cause)?;
        }
    }
    Ok(())
}

fn emit_surface_curve_layout<F>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    i: i64,
    format: IdFormat,
    layout: crate::nurbs::proc_curve::EmbeddedSurfaceCurveLayout<F>,
    solved_domain: Option<[f64; 2]>,
) -> Result<
    (
        cadmpeg_ir::geometry::IntcurveSupportContext,
        Option<cadmpeg_ir::geometry::SurfaceCurveCacheFirst<F>>,
    ),
    CarrierCurveError,
> {
    use crate::nurbs::proc_curve::EmbeddedSurfaceCurveLayout;
    let (embedded, tail) = match layout {
        EmbeddedSurfaceCurveLayout::ContextFirst(context) => (context, None),
        EmbeddedSurfaceCurveLayout::CacheFirst { context, flags } => {
            let (context, form) = context
                .into_intersection(solved_domain.ok_or("missing procedural curve cache domain")?);
            let curve_tail = cadmpeg_ir::geometry::SurfaceCurveTail::try_new(
                form.extension,
                form.revision,
                form.cache,
                form.support_bounds,
                form.solved_range,
            )?;
            (
                context,
                Some(cadmpeg_ir::geometry::SurfaceCurveCacheFirst {
                    form: curve_tail,
                    flags,
                }),
            )
        }
    };
    let [first, second] = embedded.surfaces;
    let mut emit_support = |side, slot: crate::nurbs::proc_curve::SupportSlot|  -> Result<_, CarrierCurveError> {
        let Some(geometry) = slot.into_surface() else { return Ok(None) };
        let id = brep_id!(
            format,
            SurfaceId,
            "procedural_curve",
            brep_key!(i, ":support", side)
        );
        charged_push!(ctx, out.surfaces, Surface {
            id: id.clone(),
            geometry,
            source_object: None,
        });
        Ok(Some(id))
    };
    let surfaces = [emit_support(0, first)?, emit_support(1, second)?];
    let pcurves = embedded.pcurves.map(|pcurve| {
        pcurve
            .map(|nurbs| cadmpeg_ir::geometry::SupportPcurve::from(PcurveGeometry::Nurbs { nurbs }))
    });
    let context = cadmpeg_ir::geometry::IntcurveSupportContext::try_new(
        std::array::from_fn(|side| cadmpeg_ir::geometry::IntcurveSupportSide {
            surface: surfaces[side].clone(),
            pcurve: pcurves[side].clone(),
        }),
        embedded.parameter_range,
        embedded.discontinuities,
    )?;
    Ok((context, tail))
}

fn emit_surface_curve_family(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,

    out: &mut AsmBrep,
    i: i64,
    format: IdFormat,
    family: crate::nurbs::proc_curve::EmbeddedSurfaceCurve,
    solved_domain: Option<[f64; 2]>,
) -> Result<cadmpeg_ir::geometry::SurfaceCurveFamily, CarrierCurveError> {
    use crate::nurbs::proc_curve::EmbeddedSurfaceCurve;
    Ok(match family {
        EmbeddedSurfaceCurve::Blend(layout) => {
            let (context, tail) = emit_surface_curve_layout(ctx, out, i, format, layout, solved_domain)?;
            cadmpeg_ir::geometry::SurfaceCurveFamily::Blend { context, tail }
        }
        EmbeddedSurfaceCurve::SurfaceConstrained(layout) => {
            let (context, tail) = emit_surface_curve_layout(ctx, out, i, format, layout, solved_domain)?;
            cadmpeg_ir::geometry::SurfaceCurveFamily::SurfaceConstrained { context, tail }
        }
        EmbeddedSurfaceCurve::Parametric(layout) => {
            let (context, tail) = emit_surface_curve_layout(ctx, out, i, format, layout, solved_domain)?;
            cadmpeg_ir::geometry::SurfaceCurveFamily::Parametric { context, tail }
        }
        EmbeddedSurfaceCurve::Skin(layout) => {
            let (context, tail) = emit_surface_curve_layout(ctx, out, i, format, layout, solved_domain)?;
            cadmpeg_ir::geometry::SurfaceCurveFamily::Skin { context, tail }
        }
    })
}

fn emit_silhouette_curve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,

    out: &mut AsmBrep,
    i: i64,
    embedded: EmbeddedSilhouette,
    format: IdFormat,
) -> Result<cadmpeg_ir::geometry::ProceduralCurveDefinition, CarrierCurveError> {
    let [first, second] = embedded.surfaces;
    let mut emit_support = |side, geometry|  -> Result<_, CarrierCurveError> {
        let id = brep_id!(
            format,
            SurfaceId,
            "procedural_curve",
            brep_key!(i, ":support", side)
        );
        charged_push!(ctx, out.surfaces, Surface {
            id: id.clone(),
            geometry,
            source_object: None,
        });
        Ok(Some(id))
    };
    let support_ids = [emit_support(0, first)?, emit_support(1, second)?];
    let pcurves = embedded.pcurves.map(|pcurve| {
        Some(cadmpeg_ir::geometry::SupportPcurve::from(
            PcurveGeometry::Nurbs { nurbs: pcurve },
        ))
    });
    let cast_surface = brep_id!(
        format,
        SurfaceId,
        "procedural_curve",
        brep_key!(i, ":cast_surface")
    );
    charged_push!(ctx, out.surfaces, Surface {
        id: cast_surface.clone(),
        geometry: embedded.cast_surface,
        source_object: None,
    });
    Ok(cadmpeg_ir::geometry::ProceduralCurveDefinition::Silhouette(
        cadmpeg_ir::geometry::curve_payloads::SilhouetteCurveConstruction::from_unit_direction(
            cadmpeg_ir::geometry::IntcurveSupportContext::try_new(
                std::array::from_fn(|side| cadmpeg_ir::geometry::IntcurveSupportSide {
                    surface: support_ids[side].clone(),
                    pcurve: pcurves[side].clone(),
                }),
                embedded.parameter_range,
                embedded.discontinuities,
            )?,
            embedded.silhouette,
            cast_surface,
            embedded.light_direction,
        ),
    ))
}

fn emit_surface_offset_curve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,

    out: &mut AsmBrep,
    i: i64,
    embedded: EmbeddedSurfaceOffset,
    format: IdFormat,
    solved_domain: Option<[f64; 2]>,
) -> Result<cadmpeg_ir::geometry::ProceduralCurveDefinition, CarrierCurveError> {
    let (context, discontinuity_flag, base_endpoints, cache_first) = match embedded.layout {
        EmbeddedSurfaceOffsetLayout::ContextFirst {
            context,
            discontinuity_flag,
        } => (*context, discontinuity_flag, [None; 2], None),
        EmbeddedSurfaceOffsetLayout::CacheFirst {
            context,
            base_endpoints,
        } => {
            let (context, form) = context
                .into_intersection(solved_domain.ok_or("missing procedural curve cache domain")?);
            (context, false, base_endpoints, Some(form))
        }
    };
    let [first, second] = context.surfaces;
    let mut emit_support = |side, slot: crate::nurbs::proc_curve::SupportSlot|  -> Result<_, CarrierCurveError> {
        let Some(geometry) = slot.into_surface() else { return Ok(None) };
        let id = brep_id!(
            format,
            SurfaceId,
            "procedural_curve",
            brep_key!(i, ":support", side)
        );
        charged_push!(ctx, out.surfaces, Surface {
            id: id.clone(),
            geometry,
            source_object: None,
        });
        Ok(Some(id))
    };
    let support_ids = [emit_support(0, first)?, emit_support(1, second)?];
    let pcurves = context.pcurves.map(|pcurve| {
        pcurve
            .map(|nurbs| cadmpeg_ir::geometry::SupportPcurve::from(PcurveGeometry::Nurbs { nurbs }))
    });
    let base = brep_id!(format, CurveId, "procedural_curve", brep_key!(i, ":base"));
    charged_push!(ctx, out.curves, Curve {
        id: base.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(embedded.base)),
        source_object: None,
    });
    Ok(
        cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceOffset(
            cadmpeg_ir::geometry::curve_payloads::SurfaceOffsetCurveConstruction::try_new(
                cadmpeg_ir::geometry::IntcurveSupportContext::try_new(
                    std::array::from_fn(|side| cadmpeg_ir::geometry::IntcurveSupportSide {
                        surface: support_ids[side].clone(),
                        pcurve: pcurves[side].clone(),
                    }),
                    context.parameter_range,
                    context.discontinuities,
                )?,
                discontinuity_flag,
                [embedded.base_u_range, embedded.base_v_range],
                (base, embedded.base_range, base_endpoints),
                cadmpeg_ir::geometry::CacheContract::from_form(cache_first),
                embedded.distance,
                [embedded.shift, embedded.scale],
            )
            .map_err(|_| "surface-offset fields are not finite and ordered")?,
        ),
    )
}

fn emit_spring_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,

    out: &mut AsmBrep,
    i: i64,
    format: IdFormat,
    side: usize,
    geometry: SurfaceGeometry,
) -> Result<SurfaceId, CarrierCurveError> {
    let id = brep_id!(
        format,
        SurfaceId,
        "procedural_curve",
        brep_key!(i, ":support", side)
    );
    charged_push!(ctx, out.surfaces, Surface {
        id: id.clone(),
        geometry,
        source_object: None,
    });
    Ok(id)
}

fn emit_spring_support(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,

    out: &mut AsmBrep,
    i: i64,
    format: IdFormat,
    side: usize,
    support: EmbeddedSpringSupport,
) -> Result<cadmpeg_ir::geometry::SpringSupport, CarrierCurveError> {
    Ok(match support {
        EmbeddedSpringSupport::Surface(geometry) => cadmpeg_ir::geometry::SpringSupport::Surface(
            emit_spring_surface(ctx, out, i, format, side, geometry)?,
        ),
        EmbeddedSpringSupport::Ranges(ranges) => {
            cadmpeg_ir::geometry::SpringSupport::Ranges(ranges)
        }
    })
}

fn emit_spring_curve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,

    out: &mut AsmBrep,
    i: i64,
    embedded: EmbeddedSpring,
    format: IdFormat,
    solved_domain: Option<[f64; 2]>,
) -> Result<cadmpeg_ir::geometry::ProceduralCurveDefinition, CarrierCurveError> {
    let emit_pcurve = |nurbs| PcurveGeometry::Nurbs { nurbs };
    let layout = match embedded.layout {
        EmbeddedSpringLayout::ContextFirst {
            supports: [first_support, second_support],
            first_pcurve,
            second_pcurve,
            parameter_range,
            discontinuities,
            discontinuity_flag,
        } => cadmpeg_ir::geometry::SpringLayout::ContextFirst {
            supports: [
                emit_spring_support(ctx, out, i, format, 0, first_support)?,
                emit_spring_support(ctx, out, i, format, 1, second_support)?,
            ],
            first_pcurve: match first_pcurve {
                EmbeddedSpringPcurve::Pcurve(pcurve) => {
                    cadmpeg_ir::geometry::SpringPcurve::Pcurve(emit_pcurve(pcurve))
                }
                EmbeddedSpringPcurve::Range(range) => {
                    cadmpeg_ir::geometry::SpringPcurve::Range(range)
                }
            },
            second_pcurve: second_pcurve.map(emit_pcurve),
            parameter_range,
            discontinuities,
            discontinuity_flag,
            cache: None,
        },
        EmbeddedSpringLayout::CacheFirst { context } => {
            let (context, form) = context
                .into_intersection(solved_domain.ok_or("missing procedural curve cache domain")?);
            let [first_surface, second_surface] = context
                .surfaces
                .map(crate::nurbs::proc_curve::SupportSlot::into_surface);
            let [first_pcurve, second_pcurve] = context.pcurves;
            cadmpeg_ir::geometry::SpringLayout::CacheFirst {
                context: cadmpeg_ir::geometry::IntcurveSupportContext::try_new(
                    [
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: first_surface
                                .map(|surface| emit_spring_surface(ctx, out, i, format, 0, surface)).transpose()?,
                            pcurve: first_pcurve.map(emit_pcurve).map(Into::into),
                        },
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: second_surface
                                .map(|surface| emit_spring_surface(ctx, out, i, format, 1, surface)).transpose()?,
                            pcurve: second_pcurve.map(emit_pcurve).map(Into::into),
                        },
                    ],
                    context.parameter_range,
                    context.discontinuities,
                )?,
                form,
            }
        }
    };
    Ok(cadmpeg_ir::geometry::ProceduralCurveDefinition::Spring(
        cadmpeg_ir::geometry::curve_payloads::SpringCurvePayload::try_new(
            layout,
            embedded.direction,
        )
        .map_err(|_| "spring context, null-support ranges, or cache-first form are invalid")?,
    ))
}

fn emit_projection_curve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,

    out: &mut AsmBrep,
    i: i64,
    embedded: EmbeddedProjection,
    format: IdFormat,
) -> Result<cadmpeg_ir::geometry::ProceduralCurveDefinition, CarrierCurveError> {
    let [first, second] = embedded.surfaces;
    let mut emit_support = |side, geometry|  -> Result<_, CarrierCurveError> {
        let id = brep_id!(
            format,
            SurfaceId,
            "procedural_curve",
            brep_key!(i, ":support", side)
        );
        charged_push!(ctx, out.surfaces, Surface {
            id: id.clone(),
            geometry,
            source_object: None,
        });
        Ok(Some(id))
    };
    let surfaces = [emit_support(0, first)?, emit_support(1, second)?];
    let pcurves = embedded.pcurves.map(|pcurve| {
        Some(cadmpeg_ir::geometry::SupportPcurve::from(
            PcurveGeometry::Nurbs { nurbs: pcurve },
        ))
    });
    let source = brep_id!(format, CurveId, "procedural_curve", brep_key!(i, ":source"));
    charged_push!(ctx, out.curves, Curve {
        id: source.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(embedded.source)),
        source_object: None,
    });
    Ok(cadmpeg_ir::geometry::ProceduralCurveDefinition::Projection(
        cadmpeg_ir::geometry::curve_payloads::ProjectionCurvePayload::try_new(
            cadmpeg_ir::geometry::IntcurveSupportContext::try_new(
                std::array::from_fn(|side| cadmpeg_ir::geometry::IntcurveSupportSide {
                    surface: surfaces[side].clone(),
                    pcurve: pcurves[side].clone(),
                }),
                embedded.parameter_range,
                embedded.discontinuities,
            )?,
            embedded.discontinuity_flag,
            source,
            embedded.tail,
        )
        .map_err(|_| "projection fields are not finite and ordered")?,
    ))
}

fn emit_law_curve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    i: i64,
    embedded: EmbeddedLawCurve,
    format: IdFormat,
    solved_domain: Option<[f64; 2]>,
) -> Result<cadmpeg_ir::geometry::ProceduralCurveDefinition, CarrierCurveError> {
    let prefix = brep_key!(i, ":law");
    let scope = LawExpressionScope::Curve(prefix);
    let (parameter_range, version) = match embedded.layout {
        EmbeddedLawCurveLayout::Legacy(range) => (range, None),
        EmbeddedLawCurveLayout::Version {
            stamp,
            post_enum,
            parameter_range,
        } => {
            let domain = solved_domain.ok_or("missing procedural curve cache domain")?;
            (
                std::array::from_fn(|index| parameter_range[index].unwrap_or(domain[index])),
                Some(cadmpeg_ir::geometry::LawCurveVersionForm::try_new(
                    stamp,
                    post_enum,
                    parameter_range,
                )?),
            )
        }
    };
    let [first, second] = embedded.surfaces;
    let mut emit_support = |side, slot: crate::nurbs::proc_curve::SupportSlot|  -> Result<_, CarrierCurveError> {
        let Some(geometry) = slot.into_surface() else { return Ok(None) };
        let id = brep_id!(
            format,
            SurfaceId,
            "procedural_curve",
            brep_key!(i, ":support", side)
        );
        charged_push!(ctx, out.surfaces, Surface {
            id: id.clone(),
            geometry,
            source_object: None,
        });
        Ok(Some(id))
    };
    let surfaces = [emit_support(0, first)?, emit_support(1, second)?];
    let pcurves = embedded.pcurves.map(|pcurve| {
        pcurve
            .map(|nurbs| cadmpeg_ir::geometry::SupportPcurve::from(PcurveGeometry::Nurbs { nurbs }))
    });
    let mut map_formula = |path: cadmpeg_ir::ids::IdentityKey, formula: EmbeddedLawFormula|
        -> Result<cadmpeg_ir::geometry::FiniteLawFormula, CarrierCurveError> {
        let mapped = map_law_formula(ctx, formula,
            |index, expression| {
                map_law_expression(ctx, &mut *out,
                    format,
                    scope.clone(),
                    brep_key!(path.clone(), ":", index),
                    expression,
                )
            },
        )?;
        cadmpeg_ir::geometry::FiniteLawFormula::try_new(mapped)
            .map_err(CarrierCurveError::from)
    };
    Ok(cadmpeg_ir::geometry::ProceduralCurveDefinition::Law {
        context: cadmpeg_ir::geometry::IntcurveSupportContext::try_new(
            std::array::from_fn(|side| cadmpeg_ir::geometry::IntcurveSupportSide {
                surface: surfaces[side].clone(),
                pcurve: pcurves[side].clone(),
            }),
            parameter_range,
            embedded.discontinuities,
        )?,
        version,
        extension: embedded.extension,
        primary: map_formula(cadmpeg_ir::identity_key!("primary"), embedded.primary)?,
        additional: embedded
            .additional
            .into_iter()
            .enumerate()
            .map(|(index, formula)| map_formula(brep_key!("additional:", index), formula))
            .try_collect_counted_vec(ctx, "ASM law curve additional formulas")?,
        cache: None,
    })
}

/// Pass 3: emit surface and curve carriers in `RecordTable` order for
/// deterministic output.
pub(super) fn emit_carrier_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    records: &[Record],
    carriers: &mut Carriers,
    reach: &Reachable,
    reversed_curve_refs: &HashSet<i64>,
    forward_curve_refs: &HashSet<i64>,
    format: IdFormat,
) -> Result<(), cadmpeg_core::CodecError> {
    for r in records {
        let i = r.index as i64;
        match r.head() {
            _ if reach.surfaces.contains(&i) => {
                emit_carrier_surface(ctx, out, r, i, carriers, reach, format)?;
            }
            _ if reach.unknown_surface_records.contains(&i) => {
                // Topology-known face on an undecoded surface: emit an opaque
                // carrier linking to the preserved record bytes, marked Unknown.
                charged_push!(ctx, out.surfaces, Surface {
                    id: SurfaceId::from(id(format, i)),
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                        record: Some(unknown_record_id(r, format)?),
                    }),
                    source_object: None,
                });
            }
            _ if reach.curves.contains(&i) => {
                emit_carrier_curve(
                    ctx,
                    out,
                    i,
                    carriers,
                    reversed_curve_refs,
                    forward_curve_refs,
                    format,
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Emit reachable pcurve carriers with their wrapper and fit-tolerance tails.
pub(super) fn emit_pcurves(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    records: &[Record],
    carriers: &mut Carriers,
    reach: &Reachable,
    format: IdFormat,
) -> Result<(), cadmpeg_core::CodecError> {
    let Carriers { pcurve_geo, .. } = &mut *carriers;
    let Reachable {
        pcurves: kept_pcurves,
        ..
    } = reach;
    for r in records {
        let i = r.index as i64;
        if kept_pcurves.contains(&i) {
            if let Some(geometry) = pcurve_geo.remove(&super::PcurveRecordIndex(i)) {
                let wrapper_reversed = match r.chunk(4) {
                    Some(Token::True) if matches!(r.chunk(3), Some(Token::Long(0))) => Some(true),
                    Some(Token::False) if matches!(r.chunk(3), Some(Token::Long(0))) => Some(false),
                    _ => None,
                };
                let native_tail_flags = pcurve_inline_tail_flags(r);
                let parameter_range = pcurve_parameter_range(r);
                let fit_tolerance = match (r.chunk(3), r.chunk(4)) {
                    (Some(Token::Long(0)), Some(Token::True | Token::False)) => {
                        nurbs::toks::payload_subtype_toks(r, 5, "exp_par_cur")
                            .and_then(|scope| nurbs::pcurve::pcurve_fit_tolerance(ctx, scope))
                    }
                    _ => None,
                };
                let parameter_range = parameter_range
                    .map(|range| {
                        cadmpeg_ir::units::FiniteVector::new(range)
                            .ok_or(PcurveMetadata::NON_FINITE_PARAMETER_RANGE)
                    })
                    .transpose()
                    .map_err(cadmpeg_core::CodecError::malformed)?;
                let fit_tolerance = fit_tolerance
                    .transpose()?
                    .map(|value| {
                        cadmpeg_ir::geometry::FitTolerance::try_new(value)
                            .map_err(|_| PcurveMetadata::INVALID_FIT_TOLERANCE)
                    })
                    .transpose()
                    .map_err(cadmpeg_core::CodecError::malformed)?;
                let metadata = match (
                    wrapper_reversed,
                    native_tail_flags,
                    parameter_range,
                    fit_tolerance,
                ) {
                    (
                        Some(wrapper_reversed),
                        Some(native_tail_flags),
                        Some(parameter_range),
                        Some(fit_tolerance),
                    ) => PcurveMetadata::AsmInline {
                        form: PcurveInlineForm::new(
                            wrapper_reversed,
                            native_tail_flags,
                            parameter_range,
                            fit_tolerance,
                        ),
                    },
                    (wrapper_reversed, _, parameter_range, fit_tolerance) => {
                        PcurveMetadata::general(wrapper_reversed, parameter_range, fit_tolerance)
                    }
                };
                charged_push!(ctx, out.pcurves, Pcurve {
                    id: <PcurveId>::from(id(format, i)),
                    geometry,
                    metadata,
                });
            }
        }
    }

    Ok(())
}

/// Emit reachable point carriers, scaled to millimetres.
pub(super) fn emit_points(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    records: &[Record],
    reach: &Reachable,
    format: IdFormat,
) -> Result<(), cadmpeg_core::CodecError> {
    let Reachable {
        points: kept_points,
        ..
    } = reach;
    for r in records {
        let i = r.index as i64;
        if r.head() == "point" && kept_points.contains(&i) {
            let c = collect_carrier(ctx, r)?;
            if let Some(p) = c.positions.first() {
                let position = cadmpeg_ir::features::FinitePoint3::new(scale_point(*p))
                    .ok_or(Point::NON_FINITE_POSITION)
                    .map_err(cadmpeg_core::CodecError::malformed)?;
                charged_push!(ctx, out.points, Point::new(<PointId>::from(id(format, i)), position, None));
            }
        }
    }

    Ok(())
}

/// Emit reachable vertices with their tolerant tails and ownership records.
pub(super) fn emit_vertices(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,

    out: &mut AsmBrep,
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
    reach: &Reachable,
    format: IdFormat,
) -> Result<(), cadmpeg_core::CodecError> {
    let Reachable {
        vertices: kept_vertices,
        points: kept_points,
        ..
    } = reach;
    for r in records {
        let i = r.index as i64;
        if is_vertex_record(r) && kept_vertices.contains(&i) {
            if let Some(pi) = vertex_point_ref(r) {
                if kept_points.contains(&pi) {
                    charged_push!(ctx, out.vertices, Vertex {
                        id: <VertexId>::from(id(format, i)),
                        point: <PointId>::from(id(format, pi)),
                        // The last of the three f64 tolerance slots is the
                        // evaluated tolerance. A negative value is the unset
                        // sentinel, a marker rather than a length: the
                        // neutral vertex carries no tolerance and the native
                        // tail keeps the unset fact.
                        tolerance: matches!(r.head(), "tvertex")
                            .then(|| -> Result<_, cadmpeg_core::CodecError> {
                                // The save-format 700 layout stores one
                                // tolerance directly after the point.
                                let slot = if matches!(r.chunk(4), Some(Token::Long(_))) {
                                    8
                                } else {
                                    5
                                };
                                Ok(match r.chunk(slot) {
                                    Some(Token::Double(value)) if *value < 0.0 => None,
                                    Some(Token::Double(value)) => Some(
                                        cadmpeg_ir::scalar::PositiveReal::new(*value * LEN_TO_MM)
                                            .ok_or_else(|| {
                                            cadmpeg_core::CodecError::malformed(
                                                "vertex tolerance must be positive and finite",
                                            )
                                        })?,
                                    ),
                                    _ => None,
                                })
                            })
                            .transpose()?
                            .flatten(),
                    });
                    if r.head() == "tvertex" {
                        if let (Some(Token::Double(first)), Some(Token::Double(second))) =
                            (r.chunk(6), r.chunk(7))
                        {
                            let [Some(first), Some(second)] =
                                [*first, *second].map(cadmpeg_ir::scalar::FiniteReal::new)
                            else {
                                return Err(cadmpeg_core::CodecError::malformed(
                                    "vertex leading tolerance must be finite",
                                ));
                            };
                            charged_push!(ctx, out.tolerant_vertex_tails, TolerantVertexTail {
                                source_namespace:
                                    crate::brep::records::identity::NativeRecordNamespace::new(
                                        format,
                                    ),
                                vertex: <VertexId>::from(id(format, i)),
                                record_index: r.index as u32,
                                leading_tolerances: [first, second],
                                evaluated_slot: {
                                    let trailing = match r.chunk(9) {
                                        Some(Token::Long(value)) => Some(*value),
                                        _ => None,
                                    };
                                    match r.chunk(8) {
                                        Some(Token::Double(value)) if *value < 0.0 => {
                                            EvaluatedToleranceSlot::Unset { trailing }
                                        }
                                        Some(Token::Double(_)) => {
                                            EvaluatedToleranceSlot::Evaluated { trailing }
                                        }
                                        _ => EvaluatedToleranceSlot::Absent {},
                                    }
                                },
                            });
                        }
                    }
                    if let (Some(owning_edge), Some(endpoint_index)) = (
                        r.ref_at(3).filter(|owner| {
                            by_index
                                .get(owner)
                                .is_some_and(|record| is_edge_record(record))
                        }),
                        match r.chunk(4) {
                            Some(Token::Long(0)) => Some(EndpointSlot::Start),
                            Some(Token::Long(1)) => Some(EndpointSlot::End),
                            _ => None,
                        },
                    ) {
                        charged_push!(ctx, out.vertex_ownerships, VertexOwnership {
                            source_namespace:
                                crate::brep::records::identity::NativeRecordNamespace::new(format),
                            vertex: <VertexId>::from(id(format, i)),
                            record_index: r.index as u32,
                            owning_edge: <EdgeId>::from(id(format, owning_edge)),
                            endpoint_index,
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

/// Emit reachable edges with parameter ranges, tolerant tails, ownership, and
/// continuity records, folding reversed senses onto the shared carrier.
pub(super) fn emit_edges(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
    reach: &Reachable,
    reversed_curve_refs: &HashSet<i64>,
    forward_curve_refs: &HashSet<i64>,
    format: IdFormat,
) -> Result<(), cadmpeg_core::CodecError> {
    let Reachable {
        edges: kept_edges,
        vertices: kept_vertices,
        curves: kept_curves,
        ..
    } = reach;
    let reversed_curve_id = |c: i64| -> CurveId {
        if reversed_curve_refs.contains(&c) && forward_curve_refs.contains(&c) {
            brep_id!(format, CurveId, "entity", brep_key!(c, ":reversed"))
        } else {
            CurveId::from(id(format, c))
        }
    };
    for r in records {
        let i = r.index as i64;
        if is_edge_record(r) && kept_edges.contains(&i) {
            let (Some(start), Some(end)) = (r.ref_at(3), r.ref_at(5)) else {
                continue;
            };
            if !kept_vertices.contains(&start) || !kept_vertices.contains(&end) {
                continue;
            }
            let curve = r.ref_at(8).filter(|c| kept_curves.contains(c));
            let param_range = match (double_at(r, 4), double_at(r, 6)) {
                (Some(mut a), Some(mut b)) => {
                    if let Some(curve_record) = curve.and_then(|curve| by_index.get(&curve)) {
                        if curve_record.head() == "ellipse" {
                            // Native conic parameters are angles from the
                            // major axis, matching the IR carrier's own
                            // parameterization directly. Wrap the arc start
                            // into the canonical `[0, τ)` domain, preserving
                            // the sweep; a full period keeps its start phase
                            // so the range still anchors on the edge's
                            // vertices.
                            let sweep = b - a;
                            let full_period = (sweep.abs() - std::f64::consts::TAU).abs()
                                < EPS_EMIT_EMIT_EDGES_E9;
                            if !full_period {
                                a = a.rem_euclid(std::f64::consts::TAU);
                                if std::f64::consts::TAU - a < EPS_EMIT_EMIT_EDGES_E9 {
                                    a = 0.0;
                                }
                                b = a + sweep;
                            }
                        } else if curve_record.head() == "straight" {
                            // Native line parameters are multiples of the
                            // stored direction vector, whose length is the
                            // parameter scale; the IR carrier's unit direction
                            // lives in millimeter space.
                            let scale = collect_carrier(ctx, curve_record)?
                                .vectors
                                .first()
                                .map_or(1.0, |vector| norm3(*vector));
                            a *= scale * LEN_TO_MM;
                            b *= scale * LEN_TO_MM;
                        }
                    }
                    Some([a, b])
                }
                _ => None,
            };
            // A reversed edge's raw parameters already live on the reversed
            // parameterization its (reversed) carrier now exposes, so the
            // range transforms identically for both senses; only the carrier
            // link differs when the curve is shared across senses.
            let curve = curve.map(|c| match sense_at(r, 9) {
                Sense::Reversed => reversed_curve_id(c),
                Sense::Forward => CurveId::from(id(format, c)),
            });
            // The tedge tail carries the model-space tolerance, then the
            // per-entity serializer revision stamp, then a trailing LONG
            // present when the stream's full format version (save format
            // x 100 + header revision) is at least 2250003. All forms are
            // retained verbatim.
            let tolerant_tail = match (r.head(), r.chunk(11), r.chunk(12)) {
                ("tedge", Some(Token::Double(tolerance)), Some(Token::Long(revision))) => {
                    cadmpeg_ir::scalar::NonNegativeReal::new(*tolerance).map(|tolerance| {
                        let trailing = match r.chunk(13) {
                            Some(Token::Long(second)) => Some(*second),
                            _ => None,
                        };
                        (tolerance, *revision, trailing)
                    })
                }
                _ => None,
            };
            charged_push!(ctx, out.edges, Edge {
                id: EdgeId::from(id(format, i)),
                carrier: cadmpeg_ir::topology::EdgeCarrier::new(curve, param_range)
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                start: VertexId::from(id(format, start)),
                end: VertexId::from(id(format, end)),
                tolerance: tolerant_tail
                    .map(|(tolerance, _, _)| {
                        cadmpeg_ir::scalar::PositiveReal::new(tolerance.get() * LEN_TO_MM)
                            .ok_or_else(|| {
                                cadmpeg_core::CodecError::malformed(
                                    "edge tolerance must be positive and finite",
                                )
                            })
                    })
                    .transpose()?,
            });
            if let Some((_, entity_revision, trailing_field)) = tolerant_tail {
                charged_push!(ctx, out.tolerant_edge_tails, TolerantEdgeTail {
                    source_namespace: crate::brep::records::identity::NativeRecordNamespace::new(
                        format,
                    ),
                    edge: EdgeId::from(id(format, i)),
                    record_index: r.index as u32,
                    entity_revision,
                    trailing_field,
                });
            }
            charged_push!(ctx, out.edge_ownerships, EdgeOwnership {
                source_namespace: crate::brep::records::identity::NativeRecordNamespace::new(
                    format,
                ),
                edge: EdgeId::from(id(format, i)),
                record_index: r.index as u32,
                owner_coedge: r.ref_at(7).map(|owner| CoedgeId::from(id(format, owner))),
            });
            if let Some(Token::Str(continuity)) = r.chunk(10) {
                charged_push!(ctx, out.edge_continuities, EdgeContinuity {
                    source_namespace: crate::brep::records::identity::NativeRecordNamespace::new(
                        format,
                    ),
                    edge: EdgeId::from(id(format, i)),
                    record_index: r.index as u32,
                    sense: sense_at(r, 9),
                    continuity: continuity.clone(),
                });
            }
        }
    }
    Ok(())
}

/// Emit reachable coedges with pcurve links, tolerant parameters, and any
/// embedded use-curve carrier.
pub(super) fn emit_coedges(ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    records: &[Record],
    token_table: &nurbs::toks::SubtypeTable,
    save_format_major: Option<u32>,
    carriers: &Carriers,
    reach: &Reachable,
    format: IdFormat,
) -> Result<(), cadmpeg_core::CodecError> {
    let Carriers {
        pcurve_parameter_ranges,
        ..
    } = carriers;
    let Reachable {
        coedges: kept_coedges,
        edges: kept_edges,
        loops: kept_loops,
        pcurves: kept_pcurves,
        ..
    } = reach;
    for r in records {
        let i = r.index as i64;
        if is_coedge_record(r) && kept_coedges.contains(&i) {
            let (Some(next), Some(prev), Some(edge), Some(owner)) =
                (r.ref_at(3), r.ref_at(4), r.ref_at(6), r.ref_at(8))
            else {
                continue;
            };
            if !kept_coedges.contains(&next)
                || !kept_coedges.contains(&prev)
                || !kept_edges.contains(&edge)
                || !kept_loops.contains(&owner)
            {
                continue;
            }
            let partner = r.ref_at(5).filter(|p| kept_coedges.contains(p));
            let tolerant = if r.head() == "tcoedge" {
                match (r.chunk(11), r.chunk(12)) {
                    (Some(Token::Double(start)), Some(Token::Double(end))) => {
                        let extension = match save_format_major {
                            Some(major) if major > 219 => tolerant_coedge_extension(r),
                            Some(215..=219) => match r.chunk(13) {
                                Some(Token::Ref(target)) => {
                                    Some(TolerantCoedgeExtension::Reference {
                                        target: (*target >= 0).then_some(*target),
                                    })
                                }
                                _ => None,
                            },
                            Some(_) => Some(TolerantCoedgeExtension::None {}),
                            None => None,
                        };
                        let parameter_range = cadmpeg_ir::units::FiniteVector::new([*start, *end])
                            .ok_or_else(|| {
                                cadmpeg_core::CodecError::malformed(format_args!(
                                    "tolerant coedge parameter interval must be finite"
                                ))
                            })?;
                        extension.map(|extension| (parameter_range, extension))
                    }
                    _ => None,
                }
            } else {
                None
            };
            let use_curve = match tolerant.as_ref() {
                Some((
                    range,
                    TolerantCoedgeExtension::EmbeddedCurve {
                        curve_reversed,
                        parameter_range,
                        ..
                    },
                )) => match nurbs::core::curve_cache_resolving_refs(ctx, &r.tokens, token_table) {
                    Some(Ok(mut curve)) => {
                        if *curve_reversed {
                            curve.reverse_parameterization();
                        }
                        let curve_id = brep_id!(format, CurveId, "tolerant-coedge-curve", i);
                        charged_push!(ctx, out.curves, Curve {
                            id: curve_id.clone(),
                            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                            source_object: None,
                        });
                        Some((curve_id, parameter_range.unwrap_or(*range)))
                    }
                    Some(Err(error)) => return Err(error),
                    None => None,
                },
                _ => None,
            };
            charged_push!(ctx, out.coedges, Coedge {
                id: <CoedgeId>::from(id(format, i)),
                owner_loop: <LoopId>::from(id(format, owner)),
                edge: <EdgeId>::from(id(format, edge)),
                radial_next: match partner {
                    Some(p) => <CoedgeId>::from(id(format, p)),
                    None => <CoedgeId>::from(id(format, i)),
                },
                sense: sense_at(r, 7),
                pcurves: coedge_pcurve_ref(r)
                    .filter(|p| kept_pcurves.contains(p))
                    .map(|p| {
                        Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::topology::PcurveUse {
                            pcurve: <PcurveId>::from(id(format, p)),
                            isoparametric: None,
                            parameter_range: (pcurve_parameter_ranges
                                .get(&super::CoedgeRecordIndex(i))
                                .copied())
                            .map(cadmpeg_ir::geometry::DirectedParameterRange::new)
                            .transpose()
                            .map_err(cadmpeg_core::CodecError::malformed)?,
                        })
                    })
                    .transpose()?
                    .into_iter()
                    .collect_counted_vec(ctx, "ASM coedge pcurve use")?,
                use_curve: use_curve
                    .map(|(curve, parameter_range)| {
                        Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::topology::CoedgeUseCurve {
                            curve,
                            parameter_range:
                                cadmpeg_ir::topology::ParameterInterval::from_finite_endpoints(
                                    parameter_range,
                                )
                                .map_err(cadmpeg_core::CodecError::malformed)?,
                        })
                    })
                    .transpose()?,
            });
            if let Some((parameter_range, extension)) = tolerant {
                charged_push!(ctx, out.tolerant_coedge_parameters, TolerantCoedgeParameters {
                        source_namespace:
                            crate::brep::records::identity::NativeRecordNamespace::new(format),
                        coedge: <CoedgeId>::from(id(format, i)),
                        record_index: r.index as u32,
                        parameter_range,
                        extension,
                    });
            }
        }
    }
    Ok(())
}

/// Emit reachable loops with their coedge rings filtered to kept coedges.
pub(super) fn emit_loops(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
    reach: &Reachable,
    format: IdFormat,
) -> Result<(), cadmpeg_core::CodecError> {
    let Reachable {
        loops: kept_loops,
        coedges: kept_coedges,
        ..
    } = reach;
    for r in records {
        let i = r.index as i64;
        if r.head() == "loop" && kept_loops.contains(&i) {
            let Some(owner) = r.ref_at(5) else { continue };
            let coedges = ring_coedges(ctx, r, by_index, kept_coedges, format)?;
            let Ok(ring) = cadmpeg_ir::topology::LoopRing::new(coedges, Vec::new()) else {
                continue;
            };
            charged_push!(ctx, out.loops, Loop {
                id: <LoopId>::from(id(format, i)),
                face: <FaceId>::from(id(format, owner)),
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
            });
        }
    }
    Ok(())
}

/// Emit reachable faces, folding surface reversal into the normalized sense and
/// recording native sidedness.
pub(super) fn emit_faces(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
    reach: &Reachable,
    inward_normal_surfaces: &HashSet<i64>,
    format: IdFormat,
) -> Result<(), cadmpeg_core::CodecError> {
    let Reachable {
        faces: kept_faces,
        loops: kept_loops,
        ..
    } = reach;
    let subshell_shells = subshell_ancestor_shells(ctx, records, by_index)?;
    let attribute_color = |entity: &Record| attribute_chain_color(entity, by_index);
    let attribute_name = |entity: &Record| attribute_chain_name(ctx, entity, by_index);
    for r in records {
        let i = r.index as i64;
        if r.head() == "face" && kept_faces.contains(&i) {
            let (Some(surface), Some(owner)) = (r.ref_at(7), r.ref_at(5)) else {
                continue;
            };
            let loops = loop_chain(ctx, r, by_index, kept_loops, format)?;
            // The face record's sense is relative to its surface record's
            // orientation. A reversed spline record flips the cache normal,
            // and a negative-cosine cone points its normal toward the axis;
            // the IR stores the forward carrier in both cases, so the
            // reversal folds into the face sense to keep the IR
            // self-consistent.
            let native_sense = sense_at(r, 8);
            let mut sense = native_sense;
            let carrier_flipped = by_index
                .get(&surface)
                .is_some_and(|surf| surf.head() == "spline" && record_reversed(surf))
                ^ inward_normal_surfaces.contains(&surface);
            if carrier_flipped {
                sense = match sense {
                    Sense::Forward => Sense::Reversed,
                    Sense::Reversed => Sense::Forward,
                };
            }
            charged_push!(ctx, out.faces, Face {
                id: <FaceId>::from(id(format, i)),
                shell: ShellId::from(id(
                    format,
                    subshell_shells.get(&owner).copied().unwrap_or(owner),
                )),
                surface: <SurfaceId>::from(id(format, surface)),
                sense,
                loops: cadmpeg_ir::topology::FaceLoops::unspecified(loops),
                name: attribute_name(r)?,
                color: attribute_color(r),
                tolerance: None,
            });
            let containment = match (r.chunk(9), r.chunk(10)) {
                (Some(Token::True), Some(Token::True)) => Some(FaceContainment::In),
                (Some(Token::True), Some(Token::False)) => Some(FaceContainment::Out),
                _ => None,
            };
            charged_push!(ctx, out.face_sidedness, FaceSidedness {
                source_namespace: crate::brep::records::identity::NativeRecordNamespace::new(
                    format,
                ),
                face: <FaceId>::from(id(format, i)),
                record_index: r.index as u32,
                native_sense,
                carrier_flipped,
                containment,
            });
            if let Some(Token::Long(key)) = r.chunk(1) {
                let face_id = <FaceId>::from(id(format, i));
                charged_push!(ctx, out.face_native_keys, FaceNativeKey {
                    source_namespace: crate::brep::records::identity::NativeRecordNamespace::new(
                        format,
                    ),
                    face: face_id,
                    record_index: r.index as u32,
                    asm_face_key: (*key >= 0).then_some(*key as u64),
                });
            }
        }
    }
    Ok(())
}

/// Emit shells, regions, and bodies for every record so back-references
/// resolve, filtering child lists to reachable entities.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_containers(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
    reach: &Reachable,
    wire: &WireShellTopology,
    stream: &str,
    header_scale: f64,
    format: IdFormat,
) -> Result<(), cadmpeg_core::CodecError> {
    let Reachable {
        faces: kept_faces, ..
    } = reach;
    let WireShellTopology {
        wire_edges_by_shell,
        free_vertices_by_shell,
        saved_free_edges,
    } = wire;
    let attribute_color = |entity: &Record| attribute_chain_color(entity, by_index);
    let attribute_name = |entity: &Record| attribute_chain_name(ctx, entity, by_index);
    for r in records {
        let i = r.index as i64;
        match r.head() {
            "shell" => {
                let Some(owner) = r.ref_at(7) else { continue };
                let faces = shell_faces(ctx, r, by_index, kept_faces, format)?;
                charged_push!(ctx, out.shells,
                    Shell::new(
                        <ShellId>::from(id(format, i)),
                        <RegionId>::from(id(format, owner)),
                        faces,
                        wire_edges_by_shell
                            .get(&i)
                            .into_iter()
                            .flatten()
                            .map(|edge| EdgeId::from(id(format, *edge)))
                            .collect_counted_vec(ctx, "ASM shell wire edges")?,
                        free_vertices_by_shell
                            .get(&i)
                            .into_iter()
                            .flatten()
                            .map(|vertex| VertexId::from(id(format, *vertex)))
                            .collect_counted_vec(ctx, "ASM shell free vertices")?,
                    )
                    .map_err(|message| cadmpeg_core::CodecError::Malformed(message.to_string()))?,
                );
            }
            // Save-format 231 names this record `region`; format-227 streams
            // carry the original ACIS head `lump`. Same layout in both.
            "region" | "lump" => {
                let Some(owner) = r.ref_at(5) else { continue };
                let shells = shell_chain(ctx, r, by_index, format)?;
                charged_push!(ctx, out.regions, Region {
                    id: <RegionId>::from(id(format, i)),
                    body: <BodyId>::from(id(format, owner)),
                    shells,
                });
            }
            "body" => {
                let regions = region_chain(ctx, r, by_index, format)?;
                let body_id = <BodyId>::from(id(format, i));
                if let Some(Token::Long(key)) = r.chunk(1) {
                    charged_push!(ctx, out.body_native_keys, BodyNativeKey {
                        source_namespace:
                            crate::brep::records::identity::NativeRecordNamespace::new(format),
                        body: body_id.clone(),
                        record_index: r.index as u32,
                        body_ordinal: out.body_native_keys.len() as u32,
                        source_brep: stream.rsplit('/').next().map(str::to_owned),
                        asm_body_key: (*key >= 0).then_some(*key as u64),
                    });
                }
                let transform_record = r.ref_at(5).and_then(|reference| by_index.get(&reference));
                if let Some(transform) = transform_record {
                    let mut flags = transform
                        .tokens
                        .iter()
                        .filter_map(|token| match token {
                            Token::True => Some(true),
                            Token::False => Some(false),
                            _ => None,
                        });
                    if let (Some(rotation), Some(reflection), Some(shear), None) =
                        (flags.next(), flags.next(), flags.next(), flags.next())
                    {
                        charged_push!(ctx, out.transform_hints, TransformHints {
                            source_namespace:
                                crate::brep::records::identity::NativeRecordNamespace::new(format),
                            body: body_id.clone(),
                            record_index: transform.index as u32,
                            rotation,
                            reflection,
                            shear,
                        });
                    }
                }
                charged_push!(ctx, out.bodies, Body {
                    id: body_id,
                    kind: cadmpeg_ir::topology::BodyKind::Solid,
                    regions,
                    transform: transform_record
                        .and_then(|transform| decode_transform(transform, header_scale)),
                    name: attribute_name(r)?,
                    color: attribute_color(r),
                    visible: None,
                });
            }
            _ => {}
        }
    }
    for &edge in saved_free_edges {
        let body_id = brep_id!(format, BodyId, "saved-edge-body", edge);
        let region_id = brep_id!(format, RegionId, "saved-edge-region", edge);
        let shell_id = brep_id!(format, ShellId, "saved-edge-shell", edge);
        charged_push!(ctx, out.bodies, Body {
            id: body_id.clone(),
            kind: cadmpeg_ir::topology::BodyKind::Wire,
            regions: vec![region_id.clone()],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        charged_push!(ctx, out.regions, Region {
            id: region_id.clone(),
            body: body_id,
            shells: vec![shell_id.clone()],
        });
        charged_push!(ctx, out.shells, Shell::with_wire_edge(
            shell_id,
            region_id,
            <EdgeId>::from(id(format, edge)),
        ));
    }
    Ok(())
}

/// Emit direct and inherited entity attributes and derive the link, tag, and
/// timestamp projections. Returns the set of emitted attribute record indices.
/// An attribute holding a NaN or infinite number refuses the stream.
pub(super) fn emit_attributes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
    reach: &Reachable,
    format: IdFormat,
) -> Result<HashSet<i64>, cadmpeg_core::CodecError> {
    let Reachable {
        faces: kept_faces,
        loops: kept_loops,
        coedges: kept_coedges,
        edges: kept_edges,
        vertices: kept_vertices,
        ..
    } = reach;
    let mut emitted_attributes = HashSet::new();
    let mut attribute_targets = HashMap::new();
    for record in records {
        let index = record.index as i64;
        let target = match record.head() {
            "body"
                if out
                    .bodies
                    .iter()
                    .any(|entity| entity.id.as_str() == id(format, index).as_str()) =>
            {
                Some(AttributeTarget::Body(BodyId::from(id(format, index))))
            }
            "shell"
                if out
                    .shells
                    .iter()
                    .any(|entity| entity.id.as_str() == id(format, index).as_str()) =>
            {
                Some(AttributeTarget::Shell(ShellId::from(id(format, index))))
            }
            // ASM-227 names a region's topological owner `lump`, while
            // ASM-231 names the same record `region`. The neutral model has
            // no region-level attribute target, so retain these attributes on
            // their owning body after the region graph has been emitted.
            "region" | "lump" => out
                .regions
                .iter()
                .find(|entity| entity.id.as_str() == id(format, index).as_str())
                .map(|entity| AttributeTarget::Body(entity.body.clone())),
            "face" if kept_faces.contains(&index) => {
                Some(AttributeTarget::Face(FaceId::from(id(format, index))))
            }
            "loop" if kept_loops.contains(&index) => {
                Some(AttributeTarget::Loop(LoopId::from(id(format, index))))
            }
            "coedge" | "tcoedge" if kept_coedges.contains(&index) => {
                Some(AttributeTarget::Coedge(<CoedgeId>::from(id(format, index))))
            }
            "edge" | "tedge" if kept_edges.contains(&index) => {
                Some(AttributeTarget::Edge(<EdgeId>::from(id(format, index))))
            }
            "vertex" | "tvertex" if kept_vertices.contains(&index) => {
                Some(AttributeTarget::Vertex(<VertexId>::from(id(format, index))))
            }
            _ => None,
        };
        if let Some(target) = target {
            crate::decode_alloc::insert_hash_map(ctx, &mut attribute_targets, index, target.clone(), "ASM attribute targets")?;
            collect_attributes(
                ctx,
                record,
                &target,
                by_index,
                &mut emitted_attributes,
                &mut out.attributes,
                format,
            )?;
        }
    }

    for record in records {
        let index = record.index as i64;
        if !record.name.ends_with("-attrib") || emitted_attributes.contains(&index) {
            continue;
        }
        if let Some(target) = attribute_owner(record)
            .and_then(|owner| inherited_attribute_target(owner, by_index, &attribute_targets))
        {
            crate::decode_alloc::insert_hash_set(ctx, &mut emitted_attributes, index, "ASM emitted attributes")?;
            charged_push!(ctx, out.attributes, source_attribute(ctx, record, target, format)?);
        }
    }
    Ok(emitted_attributes)
}

/// Preserve undecoded carriers and opaque cached procedural surfaces referenced
/// by real topology as passthrough unknown records.
pub(super) fn emit_passthrough_unknowns(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    records: &[Record],
    bytes: &[u8],
    reach: &Reachable,
    format: IdFormat,
) -> Result<(), cadmpeg_core::CodecError> {
    let Reachable {
        undecoded_carriers,
        cached_unknown_procedural_surfaces,
        ..
    } = reach;
    for r in records {
        let i = r.index as i64;
        if undecoded_carriers.contains(&i) || cached_unknown_procedural_surfaces.contains(&i) {
            let end = r.offset.checked_add(r.len).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(format_args!(
                    "record {} at byte {} declares a length of {} bytes, which leaves the address space",
                    r.index, r.offset, r.len
                ))
            })?;
            let retained = bytes.get(r.offset..end).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(format_args!(
                    "record {} declares bytes {}..{end}, but the stream holds {} bytes",
                    r.index,
                    r.offset,
                    bytes.len()
                ))
            })?;
            let retained = ctx.copy_retained(retained, "retain ASM unknown record")?;
            ctx.charge_collection_items(1, "retain ASM unknown record")?;
            out.unknowns
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit("retain ASM unknown record", 0, 1))?;
            out.unknowns.push(UnknownRecord::retained(
                unknown_record_id(r, format)?,
                r.offset as u64,
                retained,
                Vec::new(),
            ));
        }
    }
    Ok(())
}

/// Count record kinds that were neither emitted nor preserved.
pub(super) fn count_other_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    records: &[Record],
    reach: &Reachable,
    emitted_attributes: &HashSet<i64>,
) -> Result<(), cadmpeg_core::CodecError> {
    let Reachable {
        surfaces: kept_surfaces,
        curves: kept_curves,
        pcurves: kept_pcurves,
        undecoded_carriers,
        ..
    } = reach;
    // Count remaining record kinds we neither emitted nor preserved.
    let kept_transforms: HashSet<i64> = records
        .iter()
        .filter(|record| record.head() == "body")
        .filter_map(|record| record.ref_at(5))
        .collect_counted_set(ctx, "ASM retained transform references")?;
    let pcurve_intcurves: HashSet<i64> = records
        .iter()
        .filter(|record| kept_pcurves.contains(&(record.index as i64)))
        .filter_map(|record| record.ref_at(4))
        .collect_counted_set(ctx, "ASM pcurve intcurve references")?;
    for r in records {
        let i = r.index as i64;
        // Spline/intcurve records that decoded into a NURBS carrier are counted
        // as transferred, not as opaque leftovers.
        let transferred = kept_surfaces.contains(&i)
            || kept_curves.contains(&i)
            || kept_pcurves.contains(&i)
            || kept_transforms.contains(&i)
            || emitted_attributes.contains(&i)
            || pcurve_intcurves.contains(&i);
        if !is_known_record_head(r.head())
            && !is_asm_stream_delimiter(&r.name)
            && !undecoded_carriers.contains(&i)
            && !transferred
        {
            count_kind(ctx, &mut out.stats.other_record_kinds, &r.name)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
