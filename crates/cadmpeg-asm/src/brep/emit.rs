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
use crate::ids::IdFormat;
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
    EmbeddedLoftPathLayout, EmbeddedLoftProfileMember, EmbeddedNetSurface, EmbeddedOffsetLayout,
    EmbeddedRevisionCompoundLoft, EmbeddedRevisionG2Blend, EmbeddedRollingBall,
    EmbeddedScaledCompoundLoft, EmbeddedScaledCompoundLoftBranch, EmbeddedScaledCompoundLoftShape,
    EmbeddedSkinSurface, EmbeddedSkinSurfaceLayout, EmbeddedSweepSurface,
    EmbeddedSweepSurfaceLayout, EmbeddedVariableBlend, EmbeddedVertexBlend,
    EmbeddedVertexBlendBoundaryGeometry, LegacySweepLayout, LoftProfileData,
    ProceduralSurfaceCache, SweepLawOrFormula,
};
use crate::nurbs::reader::LEN_TO_MM;
use crate::sab::{Record, Token};
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, BlendSupport, Curve, CurveGeometry, LoftPathCurve,
    NurbsCurve, Pcurve, PcurveGeometry, PcurveInlineForm, PcurveMetadata, PcurveNurbs,
    ProceduralCurve, ProceduralSurface, ProceduralSurfaceDefinition, RollingBallConstruction,
    RollingBallRadiusSelector, RollingBallSide, RollingBallSideExtension, RollingBallSupportCurve,
    RollingBallSupportSurface, RollingBallThirdSide, SolvedCurveGeometry, SolvedSurfaceGeometry,
    Surface, SurfaceGeometry, VariableBlendConstruction, VertexBlendBoundary,
    VertexBlendBoundaryGeometry, VertexBlendConstruction,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex};
use cadmpeg_ir::unknown::UnknownRecord;

fn map_law_formula(
    formula: EmbeddedLawFormula,
    mut map: impl FnMut(usize, EmbeddedLawExpression) -> cadmpeg_ir::geometry::LawExpression,
) -> cadmpeg_ir::geometry::LawFormula {
    match formula {
        EmbeddedLawFormula::Null => cadmpeg_ir::geometry::LawFormula::Null {},
        EmbeddedLawFormula::Named { name, variables } => cadmpeg_ir::geometry::LawFormula::Named {
            name,
            variables: variables
                .into_iter()
                .enumerate()
                .map(|(index, expression)| map(index, expression))
                .collect(),
        },
    }
}
use std::collections::{HashMap, HashSet};

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
    out: &mut AsmBrep,
    r: &Record,
    i: i64,
    carriers: &mut Carriers,
    reach: &Reachable,
    format: IdFormat<'_>,
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
    out.surfaces.push(Surface {
        id: SurfaceId::mint(id(format, i)).expect("identity grammar"),
        geometry,
        source_object: None,
    });
    if let Some(procedural) = procedural_surface_defs.remove(&i) {
        let support_start = out.surfaces.len();
        let curve_start = out.curves.len();
        let (definition, cache) = procedural.into_parts();
        let definition = match definition {
            DecodedProceduralSurfaceDefinition::Deformable(embedded) => {
                emit_deformable_surface(out, i, embedded, format)?
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
                            construction.revision_form,
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
                    .map(|(component, item)| {
                        let id = SurfaceId::mint(format!(
                            "{format}:brep:procedural_surface#{i}:component{component}"
                        ))
                        .expect("identity grammar");
                        out.surfaces.push(Surface {
                            id: id.clone(),
                            geometry: item.component,
                            source_object: None,
                        });
                        cadmpeg_ir::geometry::CompoundComponent {
                            parameter: item.parameter,
                            component: id,
                        }
                    })
                    .collect();
                ProceduralSurfaceDefinition::Compound(
                    cadmpeg_ir::geometry::surface_payloads::CompoundSurfacePayload::try_new(
                        component_ids,
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                )
            }
            DecodedProceduralSurfaceDefinition::SubSurface {
                support,
                parameter_ranges,
            } => {
                let support_id = SurfaceId::mint(format!(
                    "{format}:brep:procedural_surface#{i}:sub_surface:support"
                ))
                .expect("identity grammar");
                out.surfaces.push(Surface {
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
                let support_id =
                    SurfaceId::mint(format!("{format}:brep:procedural_surface#{i}:support"))
                        .expect("identity grammar");
                out.surfaces.push(Surface {
                    id: support_id.clone(),
                    geometry: support,
                    source_object: None,
                });
                let reference_id =
                    CurveId::mint(format!("{format}:brep:procedural_surface#{i}:reference"))
                        .expect("identity grammar");
                out.curves.push(Curve {
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
                        revision_form,
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                )
            }
            DecodedProceduralSurfaceDefinition::Loft(embedded) => {
                emit_loft_surface(out, i, embedded, format)?
            }
            DecodedProceduralSurfaceDefinition::CompoundLoft(embedded) => {
                emit_compound_loft_surface(out, i, *embedded, format)?
            }
            DecodedProceduralSurfaceDefinition::ScaledCompoundLoft(embedded) => {
                emit_scaled_compound_loft_surface(out, i, embedded, format)?
            }
            DecodedProceduralSurfaceDefinition::Law(embedded) => {
                emit_law_surface(out, i, embedded, format)?
            }
            DecodedProceduralSurfaceDefinition::Skin(embedded) => {
                emit_skin_surface(out, i, embedded, format)?
            }
            DecodedProceduralSurfaceDefinition::Net(embedded) => {
                emit_net_surface(out, i, embedded, format)?
            }
            DecodedProceduralSurfaceDefinition::Sweep(embedded) => {
                emit_sweep_surface(out, i, embedded, format)?
            }
            DecodedProceduralSurfaceDefinition::G2Blend(embedded) => {
                emit_g2_blend_surface(out, i, embedded, format)?
            }
            DecodedProceduralSurfaceDefinition::Ruled { first, second } => {
                let first_id =
                    CurveId::mint(format!("{format}:brep:procedural_surface#{i}:profile0"))
                        .expect("identity grammar");
                let second_id =
                    CurveId::mint(format!("{format}:brep:procedural_surface#{i}:profile1"))
                        .expect("identity grammar");
                out.curves.push(Curve {
                    id: first_id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(first)),
                    source_object: None,
                });
                out.curves.push(Curve {
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
                let first_id =
                    CurveId::mint(format!("{format}:brep:procedural_surface#{i}:curve0"))
                        .expect("identity grammar");
                let second_id =
                    CurveId::mint(format!("{format}:brep:procedural_surface#{i}:curve1"))
                        .expect("identity grammar");
                out.curves.push(Curve {
                    id: first_id.clone(),
                    geometry: first,
                    source_object: None,
                });
                out.curves.push(Curve {
                    id: second_id.clone(),
                    geometry: second,
                    source_object: None,
                });
                ProceduralSurfaceDefinition::Sum(
                    cadmpeg_ir::geometry::surface_payloads::SumSurfaceConstruction::try_new(
                        first_id,
                        second_id,
                        basepoint,
                        revision_form,
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
                let directrix_id =
                    CurveId::mint(format!("{format}:brep:procedural_surface#{i}:directrix"))
                        .expect("identity grammar");
                out.curves.push(Curve {
                    id: directrix_id.clone(),
                    geometry: directrix,
                    source_object: None,
                });
                ProceduralSurfaceDefinition::Revolution(
                    cadmpeg_ir::geometry::surface_payloads::RevolutionSurfaceConstruction::try_new(
                        directrix_id,
                        (axis_origin, axis_direction),
                        angular_interval,
                        None,
                        Some(parameter_interval),
                        false,
                        revision_form,
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                )
            }
            DecodedProceduralSurfaceDefinition::Offset {
                support,
                distance,
                layout,
            } => {
                let support_id =
                    SurfaceId::mint(format!("{format}:brep:procedural_surface#{i}:support"))
                        .expect("identity grammar");
                out.surfaces.push(Surface {
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
                let directrix_id =
                    CurveId::mint(format!("{format}:brep:procedural_surface#{i}:directrix"))
                        .expect("identity grammar");
                out.curves.push(Curve {
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
                        revision_form,
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                )
            }
            DecodedProceduralSurfaceDefinition::VariableBlend(construction) => {
                emit_variable_blend_surface(out, i, construction, format)?
            }
            DecodedProceduralSurfaceDefinition::RevisionCompoundLoft(construction) => {
                emit_revision_compound_loft_surface(out, i, construction, format)
            }
            DecodedProceduralSurfaceDefinition::RevisionG2Blend(construction) => {
                emit_revision_g2_blend_surface(out, i, construction, format)
            }
            DecodedProceduralSurfaceDefinition::VertexBlend(construction) => {
                emit_vertex_blend_surface(out, i, *construction, format)?
            }
            DecodedProceduralSurfaceDefinition::Blend {
                supports,
                spine,
                radius,
                cross_section,
                native,
            } => emit_blend_surface(
                out,
                i,
                supports,
                spine,
                radius,
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
            ProceduralSurfaceId::mint(format!("{format}:brep:procedural_surface#{i}"))
                .expect("valid owning format and numeric record index"),
            definition,
            nurbs::proc_curve::record_trailing_surface_bounds(&r.tokens),
        )
        .map_err(cadmpeg_core::CodecError::malformed)?;
        out.procedural_surfaces.push((
            SurfaceId::mint(id(format, i)).expect("identity grammar"),
            surface,
        ));
    } else if cached_unknown_procedural_surfaces.contains(&i) {
        out.procedural_surfaces.push((
            SurfaceId::mint(id(format, i)).expect("identity grammar"),
            ProceduralSurface::new(
                ProceduralSurfaceId::mint(format!("{format}:brep:procedural_surface#{i}"))
                    .expect("valid owning format and numeric record index"),
                ProceduralSurfaceDefinition::Unknown {
                    record: Some(
                        UnknownId::mint(unknown_record_id(r, format)).expect("identity grammar"),
                    ),
                    cache: None,
                },
                None,
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,
        ));
    }

    Ok(())
}

/// Emit a kept 3D curve carrier (with its `:reversed` clone when shared) and
/// any procedural-curve construction and nested support carriers.
fn emit_deformable_surface(
    out: &mut AsmBrep,
    i: i64,
    embedded: Box<EmbeddedDeformableSurface>,
    format: IdFormat<'_>,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let embedded = *embedded;
    let support = SurfaceId::mint(format!(
        "{format}:brep:procedural_surface#{i}:deformable:support"
    ))
    .expect("identity grammar");
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
    out.surfaces.push(Surface {
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
            let secondary_surface = SurfaceId::mint(format!(
                "{format}:brep:procedural_surface#{i}:deformable:secondary"
            ))
            .expect("identity grammar");
            out.surfaces.push(Surface {
                id: secondary_surface.clone(),
                geometry: surface,
                source_object: None,
            });
            let curve_id = CurveId::mint(format!(
                "{format}:brep:procedural_surface#{i}:deformable:curve"
            ))
            .expect("identity grammar");
            out.curves.push(Curve {
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
            let secondary_surface = SurfaceId::mint(format!(
                "{format}:brep:procedural_surface#{i}:deformable:secondary"
            ))
            .expect("identity grammar");
            out.surfaces.push(Surface {
                id: secondary_surface.clone(),
                geometry: surface,
                source_object: None,
            });
            let curve_id = CurveId::mint(format!(
                "{format}:brep:procedural_surface#{i}:deformable:curve"
            ))
            .expect("identity grammar");
            out.curves.push(Curve {
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
    out: &mut AsmBrep,
    data: ClassicLoftProfileData,
    support_id: String,
) -> (i64, cadmpeg_ir::geometry::ClassicLoftProfileData) {
    let surface = SurfaceId::mint(support_id).expect("identity grammar");
    out.surfaces.push(Surface {
        id: surface.clone(),
        geometry: data.surface,
        source_object: None,
    });
    (
        data.type_code,
        cadmpeg_ir::geometry::ClassicLoftProfileData {
            surface,
            pcurve: data.pcurve.map(|nurbs| PcurveGeometry::Nurbs { nurbs }),
            first_flag: data.first_flag,
            asm_extension: data.asm_extension,
            subdata: data.subdata,
            direction: data.direction,
        },
    )
}

fn emit_loft_member_form(
    out: &mut AsmBrep,
    data: LoftProfileData,
    support_id: String,
) -> cadmpeg_ir::geometry::LoftMemberForm {
    match data {
        LoftProfileData::Classic(data) => {
            let (type_code, data) = emit_classic_loft_data(out, data, support_id);
            cadmpeg_ir::geometry::LoftMemberForm::Support {
                type_code,
                surface: Some(data.surface),
                support_bounds: [None; 4],
                pcurve: data.pcurve,
                first_flag: data.first_flag,
                asm_extension: Some(data.asm_extension),
                subdata: data.subdata,
                direction: data.direction,
            }
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
            let surface = surface.map(|geometry| {
                let surface = SurfaceId::mint(support_id).expect("identity grammar");
                out.surfaces.push(Surface {
                    id: surface.clone(),
                    geometry,
                    source_object: None,
                });
                surface
            });
            cadmpeg_ir::geometry::LoftMemberForm::Support {
                type_code: type_code.get(),
                surface,
                support_bounds,
                pcurve: pcurve.map(|nurbs| PcurveGeometry::Nurbs { nurbs }),
                first_flag,
                asm_extension,
                subdata,
                direction,
            }
        }
        LoftProfileData::RevisionPcurvePair {
            endpoints: _,
            pcurve,
            secondary_pcurve,
            asm_extension,
            subdata,
            direction,
        } => cadmpeg_ir::geometry::LoftMemberForm::PcurvePair {
            pcurve: pcurve.map(|nurbs| PcurveGeometry::Nurbs { nurbs }),
            secondary_pcurve: secondary_pcurve.map(|nurbs| PcurveGeometry::Nurbs { nurbs }),
            asm_extension,
            subdata,
            direction,
        },
    }
}

fn emit_loft_path_curve(
    out: &mut AsmBrep,
    layout: EmbeddedLoftPathLayout,
    id: String,
) -> Option<LoftPathCurve> {
    let (geometry, endpoints) = match layout {
        EmbeddedLoftPathLayout::Legacy(curve) => (curve, None),
        EmbeddedLoftPathLayout::Revision(curve) => {
            let curve = curve?;
            (curve.geometry, Some(curve.endpoints))
        }
    };
    let id = CurveId::mint(id).expect("identity grammar");
    out.curves.push(Curve {
        id: id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(geometry)),
        source_object: None,
    });
    Some(LoftPathCurve { id, endpoints })
}

fn emit_loft_surface(
    out: &mut AsmBrep,
    i: i64,
    embedded: EmbeddedLoft,
    format: IdFormat<'_>,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let mut section_index = 0;
    let sections = embedded.sections.map(
                                |entries| {
                                    let current_section = section_index;
                                    section_index += 1;
                                    let section_index = current_section;
                                    let entries = entries.into_iter().enumerate().map(
                                        |(entry_index, entry)| {
                                            let profile = entry.profile.into_iter().enumerate().map(
                                                |(member_index, member)| {
                                                    let curve = CurveId::mint(format!(
                                                        "{format}:brep:procedural_surface#{i}:loft:{section_index}:{entry_index}:profile:{member_index}"
                                                    )).expect("identity grammar");
                                                    out.curves.push(Curve {
                                                        id: curve.clone(),
                                                        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(member.curve)),
                                                        source_object: None,
                                                    });
                                                    cadmpeg_ir::geometry::LoftProfileMember {
                                                        profile: LoftPathCurve {
                                                            id: curve,
                                                            endpoints: member.data.endpoints(),
                                                        },
                                                        form: emit_loft_member_form(
                                                            out,
                                                            member.data,
                                                            format!(
                                                                "{format}:brep:procedural_surface#{i}:loft:{section_index}:{entry_index}:support:{member_index}"
                                                            ),
                                                        ),
                                                    }
                                                },
                                            ).collect();
                                            let path_curve = emit_loft_path_curve(
                                                out, entry.path.layout,
                                                format!("{format}:brep:procedural_surface#{i}:loft:{section_index}:{entry_index}:path"),
                                            );
                                            let auxiliaries = entry.path.auxiliaries.into_iter().enumerate().map(
                                                |(auxiliary_index, geometry)| {
                                                    let id = CurveId::mint(format!(
                                                        "{format}:brep:procedural_surface#{i}:loft:{section_index}:{entry_index}:auxiliary:{auxiliary_index}"
                                                    )).expect("identity grammar");
                                                    out.curves.push(Curve {
                                                        id: id.clone(),
                                                        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(geometry)),
                                                        source_object: None,
                                                    });
                                                    id
                                                },
                                            ).collect();
                                            cadmpeg_ir::geometry::LoftSectionEntry {
                                                parameter: entry.parameter,
                                                profile,
                                                path: cadmpeg_ir::geometry::LoftPath {
                                                    path: path_curve,
                                                    auxiliaries,
                                                    flag: entry.path.flag,
                                                },
                                            }
                                        },
                                    ).collect();
                                    cadmpeg_ir::geometry::LoftSection { entries }
                                },
                            );
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
                None,
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
                Some(*form),
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,
        ),
    })
}

fn emit_compound_loft_surface(
    out: &mut AsmBrep,
    i: i64,
    embedded: EmbeddedCompoundLoft,
    format: IdFormat<'_>,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let map_scale = |out: &mut AsmBrep, name: &str, scale: EmbeddedCompoundLoftScale| {
        let members = scale
                                    .members
                                    .into_iter()
                                    .enumerate()
                                    .map(|(member_index, member)| {
                                        let curve = CurveId::mint(format!(
                                            "{format}:brep:procedural_surface#{i}:cloft:{name}:member:{member_index}:curve"
                                        )).expect("identity grammar");
                                        out.curves.push(Curve {
                                            id: curve.clone(),
                                            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(member.curve)),
                                            source_object: None,
                                        });
                                        let (type_code, data) = emit_classic_loft_data(out, member.data, format!(
                                                    "{format}:brep:procedural_surface#{i}:cloft:{name}:member:{member_index}:surface"
                                                ));
                    cadmpeg_ir::geometry::CompoundLoftScaleMember { type_code, curve, data }
                                    })
                                    .collect();
        let path = CurveId::mint(format!(
            "{format}:brep:procedural_surface#{i}:cloft:{name}:path"
        ))
        .expect("identity grammar");
        out.curves.push(Curve {
            id: path.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(scale.path)),
            source_object: None,
        });
        let auxiliaries = scale
            .auxiliaries
            .into_iter()
            .enumerate()
            .map(|(index, geometry)| {
                let id = CurveId::mint(format!(
                    "{format}:brep:procedural_surface#{i}:cloft:{name}:auxiliary:{index}"
                ))
                .expect("identity grammar");
                out.curves.push(Curve {
                    id: id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(geometry)),
                    source_object: None,
                });
                id
            })
            .collect();
        cadmpeg_ir::geometry::CompoundLoftScale {
            members,
            path,
            auxiliaries,
            tail: scale.tail,
        }
    };
    let mut scale_index = 0;
    let scales = (*embedded.scales).map(|scale| {
        let name = format!("scale{scale_index}");
        scale_index += 1;
        scale.map(|scale| map_scale(&mut *out, &name, scale))
    });
    let fifth_scale = embedded
        .fifth_scale
        .map(|scale| Box::new(map_scale(&mut *out, "fifth", *scale)));
    let tail = match embedded.tail {
        EmbeddedCompoundLoftTail::Six {
            flags,
            scale,
            selector,
            direction,
            parameter_range,
            curve,
        } => {
            let curve_id = CurveId::mint(format!(
                "{format}:brep:procedural_surface#{i}:cloft:tail6:curve"
            ))
            .expect("identity grammar");
            out.curves.push(Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                source_object: None,
            });
            cadmpeg_ir::geometry::CompoundLoftTail::Six {
                flags,
                scale: Box::new(map_scale(&mut *out, "tail6", *scale)),
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
            first_scale: first_scale
                .map(|scale| Box::new(map_scale(&mut *out, "tail7:first", *scale))),
            second_flag,
            second_scale: Box::new(map_scale(&mut *out, "tail7:second", *second_scale)),
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
                    let id = CurveId::mint(format!(
                        "{format}:brep:procedural_surface#{i}:cloft:tail0:direction"
                    ))
                    .expect("identity grammar");
                    out.curves.push(Curve {
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
        cadmpeg_ir::geometry::surface_payloads::CompoundLoftSurfacePayload::try_new(Box::new(
            cadmpeg_ir::geometry::CompoundLoftConstruction {
                scales: cadmpeg_ir::geometry::CompoundLoftScales::try_from_slots(
                    scales.into_iter().chain([fifth_scale.map(|scale| *scale)]),
                )
                .map_err(cadmpeg_core::CodecError::malformed)?,
                flags: embedded.flags,
                tail,
            },
        ))
        .map_err(cadmpeg_core::CodecError::malformed)?,
    ))
}

fn emit_scaled_compound_loft_surface(
    out: &mut AsmBrep,
    i: i64,
    embedded: Box<EmbeddedScaledCompoundLoft>,
    format: IdFormat<'_>,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let embedded = *embedded;
    let map_scale = |out: &mut AsmBrep, name: &str, scale: EmbeddedCompoundLoftScale| {
        let members = scale
                                    .members
                                    .into_iter()
                                    .enumerate()
                                    .map(|(member_index, member)| {
                                        let curve = CurveId::mint(format!(
                                            "{format}:brep:procedural_surface#{i}:scaled_cloft:{name}:member:{member_index}:curve"
                                        )).expect("identity grammar");
                                        out.curves.push(Curve {
                                            id: curve.clone(),
                                            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(member.curve)),
                                            source_object: None,
                                        });
                                        let (type_code, data) = emit_classic_loft_data(out, member.data, format!(
                                                    "{format}:brep:procedural_surface#{i}:scaled_cloft:{name}:member:{member_index}:surface"
                                                ));
                    cadmpeg_ir::geometry::CompoundLoftScaleMember { type_code, curve, data }
                                    })
                                    .collect();
        let path = CurveId::mint(format!(
            "{format}:brep:procedural_surface#{i}:scaled_cloft:{name}:path"
        ))
        .expect("identity grammar");
        out.curves.push(Curve {
            id: path.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(scale.path)),
            source_object: None,
        });
        let auxiliaries = scale
            .auxiliaries
            .into_iter()
            .enumerate()
            .map(|(index, geometry)| {
                let id = CurveId::mint(format!(
                    "{format}:brep:procedural_surface#{i}:scaled_cloft:{name}:auxiliary:{index}"
                ))
                .expect("identity grammar");
                out.curves.push(Curve {
                    id: id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(geometry)),
                    source_object: None,
                });
                id
            })
            .collect();
        cadmpeg_ir::geometry::CompoundLoftScale {
            members,
            path,
            auxiliaries,
            tail: scale.tail,
        }
    };
    let mut scale_index = 0;
    let scales = (*embedded.scales).map(|scale| {
        let name = format!("scale{scale_index}");
        scale_index += 1;
        scale.map(|scale| map_scale(&mut *out, &name, scale))
    });
    let map_direction = |out: &mut AsmBrep, name: &str, direction| match direction {
        EmbeddedCompoundLoftDirection::Vector(value) => {
            cadmpeg_ir::geometry::CompoundLoftDirection::Vector { value }
        }
        EmbeddedCompoundLoftDirection::Curve { selector, curve } => {
            let id = CurveId::mint(format!(
                "{format}:brep:procedural_surface#{i}:scaled_cloft:{name}"
            ))
            .expect("identity grammar");
            out.curves.push(Curve {
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
    let branch = match embedded.branch {
        EmbeddedScaledCompoundLoftBranch::ExtendedVector {
            first_scale,
            second_scale,
            selector,
            direction,
        } => cadmpeg_ir::geometry::ScaledCompoundLoftBranch::ExtendedVector {
            first_scale: first_scale
                .map(|scale| Box::new(map_scale(&mut *out, "branch:first", *scale))),
            second_scale: Box::new(map_scale(&mut *out, "branch:second", *second_scale)),
            selector,
            direction,
        },
        EmbeddedScaledCompoundLoftBranch::ExtendedCurve {
            scale,
            flag,
            singularity,
            curve,
        } => {
            let id = CurveId::mint(format!(
                "{format}:brep:procedural_surface#{i}:scaled_cloft:branch:curve"
            ))
            .expect("identity grammar");
            out.curves.push(Curve {
                id: id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                source_object: None,
            });
            cadmpeg_ir::geometry::ScaledCompoundLoftBranch::ExtendedCurve {
                scale: scale.map(|scale| Box::new(map_scale(&mut *out, "branch", *scale))),
                flag,
                singularity,
                curve: id,
            }
        }
        EmbeddedScaledCompoundLoftBranch::Direct { flag, direction } => {
            cadmpeg_ir::geometry::ScaledCompoundLoftBranch::Direct {
                flag,
                direction: map_direction(&mut *out, "branch:direction", direction),
            }
        }
    };
    let tail_curve = CurveId::mint(format!(
        "{format}:brep:procedural_surface#{i}:scaled_cloft:tail:curve"
    ))
    .expect("identity grammar");
    out.curves.push(Curve {
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
        )
        .map_err(cadmpeg_core::CodecError::malformed)?,
    ))
}

#[derive(Clone, Copy)]
enum LawExpressionScope<'a> {
    Surface(&'a str),
    Curve(&'a str),
}

fn map_law_expression(
    out: &mut AsmBrep,
    scope: LawExpressionScope<'_>,
    path: &str,
    expression: EmbeddedLawExpression,
) -> cadmpeg_ir::geometry::LawExpression {
    match expression {
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
            let id = CurveId::mint(match scope {
                LawExpressionScope::Surface(prefix) => format!("{prefix}:{path}:edge"),
                LawExpressionScope::Curve(prefix) => format!("{prefix}:{path}"),
            })
            .expect("identity grammar");
            out.curves.push(Curve {
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
            cadmpeg_ir::geometry::LawExpression::Algebraic {
                operator,
                operands: operands
                    .into_iter()
                    .enumerate()
                    .map(|(index, operand)| {
                        map_law_expression(out, scope, &format!("{path}:{index}"), operand)
                    })
                    .collect(),
            }
        }
    }
}

fn emit_law_surface(
    out: &mut AsmBrep,
    i: i64,
    embedded: Box<EmbeddedLawSurface>,
    format: IdFormat<'_>,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let prefix = format!("{format}:brep:procedural_surface#{i}:law");
    let scope = LawExpressionScope::Surface(&prefix);
    let map_formula = |out: &mut AsmBrep, path: &str, formula: EmbeddedLawFormula| {
        map_law_formula(formula, |index, expression| {
            map_law_expression(out, scope, &format!("{path}:{index}"), expression)
        })
    };
    let embedded = *embedded;
    let primary = map_formula(&mut *out, "primary", embedded.primary);
    let additional = embedded
        .additional
        .into_iter()
        .enumerate()
        .map(|(index, formula)| map_formula(&mut *out, &format!("additional:{index}"), formula))
        .collect();
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
    out: &mut AsmBrep,
    i: i64,
    embedded: Box<EmbeddedSkinSurface>,
    format: IdFormat<'_>,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let prefix = format!("{format}:brep:procedural_surface#{i}:skin:law");
    let scope = LawExpressionScope::Surface(&prefix);
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
            let curve_id =
                CurveId::mint(format!("{format}:brep:procedural_surface#{i}:skin:curve"))
                    .expect("identity grammar");
            out.curves.push(Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                source_object: None,
            });
            let secondary_id = CurveId::mint(format!(
                "{format}:brep:procedural_surface#{i}:skin:secondary"
            ))
            .expect("identity grammar");
            out.curves.push(Curve {
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
                .map(|(index, profile)| {
                    let curve = CurveId::mint(format!(
                        "{format}:brep:procedural_surface#{i}:skin:profile:{index}:curve"
                    ))
                    .expect("identity grammar");
                    out.curves.push(Curve {
                        id: curve.clone(),
                        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(profile.curve)),
                        source_object: None,
                    });
                    let (type_code, data) = emit_classic_loft_data(
                        out,
                        profile.data,
                        format!(
                            "{format}:brep:procedural_surface#{i}:skin:profile:{index}:surface"
                        ),
                    );
                    cadmpeg_ir::geometry::SkinSurfaceProfile {
                        type_code,
                        curve,
                        data,
                    }
                })
                .collect();
            let path_id = CurveId::mint(format!("{format}:brep:procedural_surface#{i}:skin:path"))
                .expect("identity grammar");
            out.curves.push(Curve {
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
    let parameter_curve = CurveId::mint(format!(
        "{format}:brep:procedural_surface#{i}:skin:parameter_curve"
    ))
    .expect("identity grammar");
    out.curves.push(Curve {
        id: parameter_curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(embedded.parameter_curve)),
        source_object: None,
    });
    let formula = map_law_formula(embedded.formula, |variable_index, variable| {
        map_law_expression(&mut *out, scope, &variable_index.to_string(), variable)
    });
    Ok(ProceduralSurfaceDefinition::Skin(
        cadmpeg_ir::geometry::surface_payloads::SkinSurfacePayload::try_new(Box::new(
            cadmpeg_ir::geometry::SkinSurfaceConstruction {
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
            },
        ))
        .map_err(cadmpeg_core::CodecError::malformed)?,
    ))
}

fn emit_net_surface(
    out: &mut AsmBrep,
    i: i64,
    embedded: Box<EmbeddedNetSurface>,
    format: IdFormat<'_>,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let prefix = format!("{format}:brep:procedural_surface#{i}:net:law");
    let scope = LawExpressionScope::Surface(&prefix);
    let embedded = *embedded;
    let mut next_section = 0;
    let sections = (*embedded.sections).map(|entries| {
                                    let section_index = next_section;
                                    next_section += 1;
                                    let entries = entries
                                        .into_iter()
                                        .enumerate()
                                        .map(|(entry_index, entry)| {
                                            let profile = entry
                                                .profile
                                                .into_iter()
                                                .enumerate()
                                                .map(|(member_index, member)| {
                                                    let curve = CurveId::mint(format!(
                                                        "{format}:brep:procedural_surface#{i}:net:{section_index}:{entry_index}:member:{member_index}:curve"
                                                    )).expect("identity grammar");
                                                    out.curves.push(Curve {
                                                        id: curve.clone(),
                                                        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(member.curve)),
                                                        source_object: None,
                                                    });
                                                    cadmpeg_ir::geometry::LoftProfileMember {
                                                        profile: LoftPathCurve {
                                                            id: curve,
                                                            endpoints: member.data.endpoints(),
                                                        },
                                                        form: emit_loft_member_form(
                                                            out,
                                                            member.data,
                                                            format!(
                                                                "{format}:brep:procedural_surface#{i}:net:{section_index}:{entry_index}:member:{member_index}:surface"
                                                            ),
                                                        ),
                                                    }
                                                })
                                                .collect();
                                            let path = emit_loft_path_curve(
                                                out, entry.path.layout,
                                                format!("{format}:brep:procedural_surface#{i}:net:{section_index}:{entry_index}:path"),
                                            );
                                            let auxiliaries = entry
                                                .path
                                                .auxiliaries
                                                .into_iter()
                                                .enumerate()
                                                .map(|(index, geometry)| {
                                                    let id = CurveId::mint(format!(
                                                        "{format}:brep:procedural_surface#{i}:net:{section_index}:{entry_index}:auxiliary:{index}"
                                                    )).expect("identity grammar");
                                                    out.curves.push(Curve {
                                                        id: id.clone(),
                                                        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(geometry)),
                                                        source_object: None,
                                                    });
                                                    id
                                                })
                                                .collect();
                                            cadmpeg_ir::geometry::LoftSectionEntry {
                                                parameter: entry.parameter,
                                                profile,
                                                path: cadmpeg_ir::geometry::LoftPath {
                                                    path,
                                                    auxiliaries,
                                                    flag: entry.path.flag,
                                                },
                                            }
                                        })
                                        .collect();
                                    cadmpeg_ir::geometry::LoftSection { entries }
                                });
    let mut next_formula = 0;
    let formulas = (*embedded.formulas).map(|formula| {
        let formula_index = next_formula;
        next_formula += 1;
        map_law_formula(formula, |index, variable| {
            map_law_expression(
                &mut *out,
                scope,
                &format!("{formula_index}:{index}"),
                variable,
            )
        })
    });
    Ok(ProceduralSurfaceDefinition::Net(
        cadmpeg_ir::geometry::surface_payloads::NetSurfacePayload::try_new(Box::new(
            cadmpeg_ir::geometry::NetSurfaceConstruction {
                sections: Box::new(sections),
                frame_parameters: embedded.frame_parameters,
                flag: embedded.flag,
                directions: embedded.directions,
                formulas: Box::new(formulas),
                discontinuities: embedded.discontinuities,
                discontinuity_flag: embedded.discontinuity_flag,
            },
        ))
        .map_err(cadmpeg_core::CodecError::malformed)?,
    ))
}

fn emit_sweep_surface(
    out: &mut AsmBrep,
    i: i64,
    embedded: Box<EmbeddedSweepSurface>,
    format: IdFormat<'_>,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let prefix = format!("{format}:brep:procedural_surface#{i}:sweep:law");
    let scope = LawExpressionScope::Surface(&prefix);
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
            let mut next_formula = 0;
            let formulas = (*formulas).map(|formula| {
                let formula_index = next_formula;
                next_formula += 1;
                map_law_formula(formula, |index, variable| {
                    map_law_expression(
                        &mut *out,
                        scope,
                        &format!("{formula_index}:{index}"),
                        variable,
                    )
                })
            });
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
                    let formula = map_law_formula(formula, |index, variable| {
                        map_law_expression(&mut *out, scope, &format!("explicit:{index}"), variable)
                    });
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
                    let guide_curve_id =
                        CurveId::mint(format!("{format}:brep:procedural_surface#{i}:sweep:guide"))
                            .expect("identity grammar");
                    out.curves.push(Curve {
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
                    let support_surface_id = SurfaceId::mint(format!(
                        "{format}:brep:procedural_surface#{i}:sweep:support"
                    ))
                    .expect("identity grammar");
                    out.surfaces.push(Surface {
                        id: support_surface_id.clone(),
                        geometry: support_surface,
                        source_object: None,
                    });
                    let auxiliary_curve = auxiliary_curve.map(|geometry| {
                        let id = CurveId::mint(format!(
                            "{format}:brep:procedural_surface#{i}:sweep:auxiliary"
                        ))
                        .expect("identity grammar");
                        out.curves.push(Curve {
                            id: id.clone(),
                            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(geometry)),
                            source_object: None,
                        });
                        id
                    });
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
                    let first_law = map_law_expression(&mut *out, scope, "law:first", *first_law);
                    let second_law =
                        map_law_expression(&mut *out, scope, "law:second", *second_law);
                    let formula = map_law_formula(formula, |index, variable| {
                        map_law_expression(
                            &mut *out,
                            scope,
                            &format!("law:formula:{index}"),
                            variable,
                        )
                    });
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
    let profile = CurveId::mint(format!(
        "{format}:brep:procedural_surface#{i}:sweep:profile"
    ))
    .expect("identity grammar");
    out.curves.push(Curve {
        id: profile.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(profile_geometry)),
        source_object: None,
    });
    let spine = CurveId::mint(format!("{format}:brep:procedural_surface#{i}:sweep:spine"))
        .expect("identity grammar");
    out.curves.push(Curve {
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
    out: &mut AsmBrep,
    i: i64,
    embedded: Box<EmbeddedG2Blend>,
    format: IdFormat<'_>,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let embedded = *embedded;
    let mut add_side = |name: &str, side: EmbeddedG2Side| {
        let surface = SurfaceId::mint(format!(
            "{format}:brep:procedural_surface#{i}:g2:{name}:surface"
        ))
        .expect("identity grammar");
        out.surfaces.push(Surface {
            id: surface.clone(),
            geometry: side.surface,
            source_object: None,
        });
        let curve = CurveId::mint(format!(
            "{format}:brep:procedural_surface#{i}:g2:{name}:curve"
        ))
        .expect("identity grammar");
        out.curves.push(Curve {
            id: curve.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(side.curve)),
            source_object: None,
        });
        let pcurves = side
            .pcurves
            .map(|pcurve| pcurve.map(|nurbs| PcurveGeometry::Nurbs { nurbs }));
        cadmpeg_ir::geometry::G2BlendSide {
            label: side.label,
            surface,
            curve,
            pcurves,
            direction: side.direction,
        }
    };
    let first = add_side("first", embedded.first);
    let second = add_side("second", embedded.second);
    let first_shape = match embedded.first_shape {
        EmbeddedG2FirstShape::Full(support) => {
            let support = support.map(|(geometry, tolerance)| {
                let id = SurfaceId::mint(format!(
                    "{format}:brep:procedural_surface#{i}:g2:first_exact"
                ))
                .expect("identity grammar");
                out.surfaces.push(Surface {
                    id: id.clone(),
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(geometry)),
                    source_object: None,
                });
                cadmpeg_ir::geometry::G2BlendFullSupport {
                    surface: id,
                    tolerance,
                }
            });
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
    let second_exact_surface = SurfaceId::mint(format!(
        "{format}:brep:procedural_surface#{i}:g2:second_exact"
    ))
    .expect("identity grammar");
    out.surfaces.push(Surface {
        id: second_exact_surface.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
            embedded.second_exact_surface,
        )),
        source_object: None,
    });
    let center_curve = CurveId::mint(format!("{format}:brep:procedural_surface#{i}:g2:center"))
        .expect("identity grammar");
    out.curves.push(Curve {
        id: center_curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(embedded.center_curve)),
        source_object: None,
    });
    Ok(ProceduralSurfaceDefinition::G2Blend(
        cadmpeg_ir::geometry::surface_payloads::G2BlendSurfacePayload::try_new(Box::new(
            cadmpeg_ir::geometry::G2BlendConstruction {
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
            },
        ))
        .map_err(cadmpeg_core::CodecError::malformed)?,
    ))
}

fn emit_rolling_ball_side(
    out: &mut AsmBrep,
    prefix: &str,
    side: RollingBallSide<SurfaceGeometry, CurveGeometry, PcurveNurbs>,
) -> RollingBallSide {
    let surface = side.surface.map(|support| {
        let id = SurfaceId::mint(format!("{prefix}:surface")).expect("identity grammar");
        out.surfaces.push(Surface {
            id: id.clone(),
            geometry: support.surface,
            source_object: None,
        });
        RollingBallSupportSurface {
            surface: id,
            parameter_ranges: support.parameter_ranges,
        }
    });
    let curve = side.curve.map(|support| {
        let id = CurveId::mint(format!("{prefix}:curve")).expect("identity grammar");
        out.curves.push(Curve {
            id: id.clone(),
            geometry: support.curve,
            source_object: None,
        });
        RollingBallSupportCurve {
            curve: id,
            parameter_range: support.parameter_range,
        }
    });
    RollingBallSide {
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
    }
}

fn emit_variable_blend_surface(
    out: &mut AsmBrep,
    i: i64,
    construction: Box<EmbeddedVariableBlend>,
    format: IdFormat<'_>,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let mut next_side_index = 0;
    let sides = (*construction.sides).map(|side| {
        let side_index = next_side_index;
        next_side_index += 1;
        let prefix = format!("{format}:brep:procedural_surface#{i}:variable_side{side_index}");
        emit_rolling_ball_side(out, &prefix, side)
    });
    let mut add_curve = |suffix: &str, geometry: CurveGeometry| {
        let id = CurveId::mint(format!(
            "{format}:brep:procedural_surface#{i}:variable_{suffix}"
        ))
        .expect("identity grammar");
        out.curves.push(Curve {
            id: id.clone(),
            geometry,
            source_object: None,
        });
        id
    };
    let slice = add_curve("slice", construction.slice);
    let secondary_curve = construction
        .secondary_curve
        .map(|support| RollingBallSupportCurve {
            curve: add_curve("secondary", support.curve),
            parameter_range: support.parameter_range,
        });
    let post_curve = construction.post_curve.map(|curve| {
        add_curve(
            "post",
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
        )
    });
    Ok(ProceduralSurfaceDefinition::VariableBlend(
        cadmpeg_ir::geometry::surface_payloads::VariableBlendSurfacePayload::try_new(Box::new(
            VariableBlendConstruction {
                subtype: construction.subtype,
                revision: construction.revision,
                sides: Box::new(sides),
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
    out: &mut AsmBrep,
    i: i64,
    construction: Box<EmbeddedRevisionCompoundLoft>,
    format: IdFormat<'_>,
) -> ProceduralSurfaceDefinition {
    let convert_profile = |scope: String,
                           profile: Vec<EmbeddedLoftProfileMember>,
                           out: &mut AsmBrep|
     -> Vec<cadmpeg_ir::geometry::LoftProfileMember> {
        profile
            .into_iter()
            .enumerate()
            .map(|(member_index, member)| {
                let curve = CurveId::mint(format!("{scope}:profile:{member_index}"))
                    .expect("identity grammar");
                out.curves.push(Curve {
                    id: curve.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(member.curve)),
                    source_object: None,
                });
                cadmpeg_ir::geometry::LoftProfileMember {
                    profile: LoftPathCurve {
                        id: curve,
                        endpoints: member.data.endpoints(),
                    },
                    form: emit_loft_member_form(
                        out,
                        member.data,
                        format!("{scope}:support:{member_index}"),
                    ),
                }
            })
            .collect()
    };
    let convert_path = |scope: String,
                        path: EmbeddedLoftPath,
                        out: &mut AsmBrep|
     -> cadmpeg_ir::geometry::LoftPath {
        let curve = emit_loft_path_curve(out, path.layout, format!("{scope}:path"));
        let auxiliaries = path
            .auxiliaries
            .into_iter()
            .enumerate()
            .map(|(auxiliary_index, geometry)| {
                let id = CurveId::mint(format!("{scope}:auxiliary:{auxiliary_index}"))
                    .expect("identity grammar");
                out.curves.push(Curve {
                    id: id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(geometry)),
                    source_object: None,
                });
                id
            })
            .collect();
        cadmpeg_ir::geometry::LoftPath {
            path: curve,
            auxiliaries,
            flag: path.flag,
        }
    };
    let base = format!("{format}:brep:procedural_surface#{i}:cloft:base");
    let base_profile = convert_profile(base.clone(), construction.base_profile, &mut *out);
    let base_path = convert_path(base, construction.base_path, &mut *out);
    let entries: Vec<_> = construction
        .entries
        .into_iter()
        .enumerate()
        .map(|(entry_index, entry)| {
            let scope = format!("{format}:brep:procedural_surface#{i}:cloft:{entry_index}");
            cadmpeg_ir::geometry::LoftSectionEntry {
                parameter: entry.parameter,
                profile: convert_profile(scope.clone(), entry.profile, &mut *out),
                path: convert_path(scope, entry.path, &mut *out),
            }
        })
        .collect();
    let direction = match construction.direction {
        EmbeddedCompoundLoftDirection::Vector(value) => {
            cadmpeg_ir::geometry::CompoundLoftDirection::Vector { value }
        }
        EmbeddedCompoundLoftDirection::Curve { selector, curve } => {
            let id = CurveId::mint(format!(
                "{format}:brep:procedural_surface#{i}:cloft:direction"
            ))
            .expect("identity grammar");
            out.curves.push(Curve {
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
    let tail = construction.tail.map(|geometry| {
        let id = CurveId::mint(format!(
            "{format}:brep:procedural_surface#{i}:cloft:trailing"
        ))
        .expect("identity grammar");
        out.curves.push(Curve {
            id: id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(geometry)),
            source_object: None,
        });
        id
    });
    ProceduralSurfaceDefinition::RevisionCompoundLoft {
        construction: Box::new(cadmpeg_ir::geometry::RevisionCompoundLoftConstruction {
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
        }),
    }
}

fn emit_revision_g2_blend_surface(
    out: &mut AsmBrep,
    i: i64,
    construction: Box<EmbeddedRevisionG2Blend>,
    format: IdFormat<'_>,
) -> ProceduralSurfaceDefinition {
    let mut next_side_index = 0;
    let sides = (*construction.sides).map(|side| {
        let side_index = next_side_index;
        next_side_index += 1;
        let prefix = format!("{format}:brep:procedural_surface#{i}:g2_side{side_index}");
        emit_rolling_ball_side(out, &prefix, side)
    });
    let center_id = CurveId::mint(format!("{format}:brep:procedural_surface#{i}:g2_center"))
        .expect("identity grammar");
    out.curves.push(Curve {
        id: center_id.clone(),
        geometry: construction.center,
        source_object: None,
    });
    ProceduralSurfaceDefinition::RevisionG2Blend {
        construction: Box::new(cadmpeg_ir::geometry::RevisionG2BlendConstruction {
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
        }),
    }
}

fn emit_vertex_blend_surface(
    out: &mut AsmBrep,
    i: i64,
    construction: EmbeddedVertexBlend,
    format: IdFormat<'_>,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let mut boundaries = Vec::with_capacity(construction.boundaries.len());
    for (boundary_index, boundary) in construction.boundaries.into_iter().enumerate() {
        let prefix =
            format!("{format}:brep:procedural_surface#{i}:vertex_boundary{boundary_index}");
        let geometry = match boundary.geometry {
            EmbeddedVertexBlendBoundaryGeometry::Circle {
                curve,
                curve_endpoints,
                twists,
                parameters,
                sense,
            } => {
                let id = CurveId::mint(format!("{prefix}:curve")).expect("identity grammar");
                out.curves.push(Curve {
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
                let id = SurfaceId::mint(format!("{prefix}:surface")).expect("identity grammar");
                out.surfaces.push(Surface {
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
                let id = CurveId::mint(format!("{prefix}:curve")).expect("identity grammar");
                out.curves.push(Curve {
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
        cadmpeg_ir::geometry::surface_payloads::VertexBlendSurfacePayload::try_new(Box::new(
            VertexBlendConstruction {
                revision: construction.revision,
                boundaries,
                grid_size: construction.grid_size,
                fit_tolerance: construction.fit_tolerance,
            },
        ))
        .map_err(cadmpeg_core::CodecError::malformed)?,
    ))
}

#[allow(clippy::too_many_arguments)]
fn emit_blend_surface(
    out: &mut AsmBrep,
    i: i64,
    supports: Box<[Option<SurfaceGeometry>; 2]>,
    spine: Option<NurbsCurve>,
    radius: BlendRadiusLaw,
    cross_section: BlendCrossSection,
    native: Option<Box<EmbeddedRollingBall>>,
    format: IdFormat<'_>,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    let mut resolved_supports = [None, None];
    for (side, support) in supports.into_iter().enumerate() {
        if let Some(support) = support {
            let support_id = SurfaceId::mint(format!(
                "{format}:brep:procedural_surface#{i}:support{side}"
            ))
            .expect("identity grammar");
            out.surfaces.push(Surface {
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
    let spine = spine.map(|spine| {
        let spine_id = CurveId::mint(format!("{format}:brep:procedural_surface#{i}:spine"))
            .expect("identity grammar");
        out.curves.push(Curve {
            id: spine_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(spine)),
            source_object: None,
        });
        spine_id
    });
    let native = native.map(|native| {
        let mut next_side_index = 0;
        let resolved_sides = (*native.sides).map(|side| {
            let side_index = next_side_index;
            next_side_index += 1;
            let prefix = format!("{format}:brep:procedural_surface#{i}:native_side{side_index}");
            let side = emit_rolling_ball_side(out, &prefix, side);
            if resolved_supports[side_index].is_none() {
                resolved_supports[side_index] = side.surface.as_ref().map(|support| BlendSupport {
                    surface: support.surface.clone(),
                    reversed: false,
                });
            }
            side
        });
        let slice = CurveId::mint(format!("{format}:brep:procedural_surface#{i}:native_slice"))
            .expect("identity grammar");
        out.curves.push(Curve {
            id: slice.clone(),
            geometry: native.slice,
            source_object: None,
        });
        let third = native.third.map(|side| {
            let prefix = format!("{format}:brep:procedural_surface#{i}:native_third");
            let surface = SurfaceId::mint(format!("{prefix}:surface")).expect("identity grammar");
            out.surfaces.push(Surface {
                id: surface.clone(),
                geometry: side.surface,
                source_object: None,
            });
            let curve = CurveId::mint(format!("{prefix}:curve")).expect("identity grammar");
            out.curves.push(Curve {
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
        });
        Box::new(RollingBallConstruction {
            definition_index: native.definition_index,
            sides: Box::new(resolved_sides),
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
    });
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
        cadmpeg_ir::geometry::surface_payloads::BlendSurfacePayload::try_new(
            resolved_supports,
            spine,
            radius,
            cross_section,
            native,
        )
        .map_err(cadmpeg_core::CodecError::malformed)?,
    ))
}

fn emit_carrier_curve(
    out: &mut AsmBrep,
    i: i64,
    carriers: &mut Carriers,
    reversed_curve_refs: &HashSet<i64>,
    forward_curve_refs: &HashSet<i64>,
    format: IdFormat<'_>,
) {
    use cadmpeg_ir::geometry::curve_payloads::{
        DeformableCurveConstruction, TwoSidedOffsetCurveConstruction, VectorOffsetCurveConstruction,
    };

    let Carriers {
        curve_geo,
        procedural_curve_defs,
        ..
    } = &mut *carriers;
    let Some(mut geometry) = curve_geo.remove(&i) else {
        return;
    };
    if reversed_curve_refs.contains(&i) {
        if forward_curve_refs.contains(&i) {
            let mut reversed = geometry.clone();
            reverse_curve_geometry(&mut reversed);
            out.curves.push(Curve {
                id: CurveId::mint(format!("{}:reversed", id(format, i))).expect("identity grammar"),
                geometry: reversed,
                source_object: None,
            });
        } else {
            reverse_curve_geometry(&mut geometry);
        }
    }
    out.curves.push(Curve {
        id: CurveId::mint(id(format, i)).expect("identity grammar"),
        geometry,
        source_object: None,
    });
    let surface_start = out.surfaces.len();
    let curve_start = out.curves.len();
    let admission_cause = |error| match error {
        cadmpeg_ir::geometry::ProceduralGeometryError::Payload(message) => message,
        cadmpeg_ir::geometry::ProceduralGeometryError::Cache(_) => {
            "invalid procedural curve cache tolerance"
        }
    };
    let procedural = match procedural_curve_defs.remove(&i) {
        Some(super::ProceduralCurveSource::Cached {
            construction,
            cache_fit_tolerance,
            parsed_domain: solved_domain,
        }) => {
            let definition = (|| -> Result<_, &'static str> {
                Ok(match *construction {
                    ProceduralCurveConstruction::VectorOffset((
                        source,
                        parameter_range,
                        offset,
                        roles,
                    )) => {
                        let source_id =
                            CurveId::mint(format!("{format}:brep:procedural_curve#{i}:source"))
                                .expect("identity grammar");
                        out.curves.push(Curve {
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
                            )
                            .map_err(|_| "vector-offset fields are not finite and ordered")?,
                        )
                    }
                    ProceduralCurveConstruction::Subset((source, parameter_range)) => {
                        let source_id =
                            CurveId::mint(format!("{format}:brep:procedural_curve#{i}:source"))
                                .expect("identity grammar");
                        out.curves.push(Curve {
                            id: source_id.clone(),
                            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(source)),
                            source_object: None,
                        });
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset(
                            cadmpeg_ir::geometry::curve_payloads::SubsetCurveConstruction::try_new(
                                source_id,
                                parameter_range,
                                true,
                            )
                            .map_err(|_| "subset-curve range is not finite and ordered")?,
                        )
                    }
                    ProceduralCurveConstruction::TwoSidedOffset(embedded) => {
                        let mut next_side = 0;
                        let surfaces: [Option<SurfaceId>; 2] = embedded.surfaces.map(|geometry| {
                            let side = next_side;
                            next_side += 1;
                            let geometry = geometry?;
                            let id = SurfaceId::mint(format!(
                                "{format}:brep:procedural_curve#{i}:support{side}"
                            ))
                            .expect("identity grammar");
                            out.surfaces.push(Surface {
                                id: id.clone(),
                                geometry,
                                source_object: None,
                            });
                            Some(id)
                        });
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
                            )
                            .map_err(|_| "two-sided offset fields are not finite and ordered")?,
                        )
                    }
                    ProceduralCurveConstruction::Intersection(embedded, discontinuity_flag) => {
                        let mut next_side = 0;
                        let surfaces: [Option<SurfaceId>; 2] = embedded.surfaces.map(|geometry| {
                            let side = next_side;
                            next_side += 1;
                            let geometry = geometry.into_surface()?;
                            let id = SurfaceId::mint(format!(
                                "{format}:brep:procedural_curve#{i}:support{side}"
                            ))
                            .expect("identity grammar");
                            out.surfaces.push(Surface {
                                id: id.clone(),
                                geometry,
                                source_object: None,
                            });
                            Some(id)
                        });
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
                        let mut next_side = 0;
                        let surface_ids: [SurfaceId; 3] = embedded.surfaces.map(|geometry| {
                            let side = next_side;
                            next_side += 1;
                            let id = SurfaceId::mint(format!(
                                "{format}:brep:procedural_curve#{i}:support{side}"
                            ))
                            .expect("identity grammar");
                            out.surfaces.push(Surface {
                                id: id.clone(),
                                geometry,
                                source_object: None,
                            });
                            id
                        });
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
                            family: emit_surface_curve_family(
                                out,
                                i,
                                format,
                                family,
                                solved_domain,
                            )?,
                        }
                    }
                    ProceduralCurveConstruction::Silhouette(embedded) => {
                        emit_silhouette_curve(out, i, embedded, format)?
                    }
                    ProceduralCurveConstruction::SurfaceOffset(embedded) => {
                        emit_surface_offset_curve(out, i, embedded, format, solved_domain)?
                    }
                    ProceduralCurveConstruction::Spring(embedded) => {
                        emit_spring_curve(out, i, embedded, format, solved_domain)?
                    }
                    ProceduralCurveConstruction::Deformable(embedded) => {
                        let (context, form) = embedded.context.into_intersection(
                            solved_domain.ok_or("missing procedural curve cache domain")?,
                        );
                        let mut next_side = 0;
                        let support_ids: [Option<SurfaceId>; 2] =
                            context.surfaces.map(|geometry| {
                                let side = next_side;
                                next_side += 1;
                                geometry.into_surface().map(|geometry| {
                                    let id = SurfaceId::mint(format!(
                                "{format}:brep:procedural_curve#{i}:deformable_support{side}"
                            ))
                                    .expect("identity grammar");
                                    out.surfaces.push(Surface {
                                        id: id.clone(),
                                        geometry,
                                        source_object: None,
                                    });
                                    id
                                })
                            });
                        let pcurves = context.pcurves.map(|pcurve| {
                            pcurve.map(|nurbs| {
                                cadmpeg_ir::geometry::SupportPcurve::from(PcurveGeometry::Nurbs {
                                    nurbs,
                                })
                            })
                        });
                        let source = match embedded.source {
                        crate::nurbs::proc_curve::EmbeddedDeformableSource::Curve(geometry) => {
                            let curve = CurveId::mint(format!(
                                "{format}:brep:procedural_curve#{i}:deformable_source"
                            ))
                            .expect("identity grammar");
                            out.curves.push(Curve {
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
                        emit_projection_curve(out, i, embedded, format)?
                    }
                    ProceduralCurveConstruction::Law(embedded) => {
                        emit_law_curve(out, i, embedded, format, solved_domain)?
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
                            .map(|(component, curve)| {
                                let id = CurveId::mint(format!(
                                    "{format}:brep:procedural_curve#{i}:component:{component}"
                                ))
                                .expect("identity grammar");
                                out.curves.push(Curve {
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
                            })
                            .collect();
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound(
                            cadmpeg_ir::geometry::CompoundCurveConstruction::try_new(
                                parameters, components,
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
                            .set_legacy_cache(Some(cache))
                            .map_err(|_| "invalid procedural curve cache tolerance")?;
                    }
                }
                ProceduralCurve::new(
                    ProceduralCurveId::mint(format!("{format}:brep:procedural_curve#{i}"))
                        .expect("valid owning format and numeric record index"),
                    definition,
                )
                .map_err(admission_cause)
            })
        }
        Some(super::ProceduralCurveSource::Cacheless(definition)) => ProceduralCurve::new(
            ProceduralCurveId::mint(format!("{format}:brep:procedural_curve#{i}"))
                .expect("valid owning format and numeric record index"),
            *definition,
        )
        .map_err(admission_cause),
        None => return,
    };
    match procedural {
        Ok(procedural) => out.procedural_curves.push((
            CurveId::mint(id(format, i)).expect("identity grammar"),
            procedural,
        )),
        Err(cause) => {
            out.surfaces.truncate(surface_start);
            out.curves.truncate(curve_start);
            count_kind(&mut out.stats.procedural_curve_kinds, cause);
        }
    }
}

fn emit_surface_curve_layout<F>(
    out: &mut AsmBrep,
    i: i64,
    format: IdFormat<'_>,
    layout: crate::nurbs::proc_curve::EmbeddedSurfaceCurveLayout<F>,
    solved_domain: Option<[f64; 2]>,
) -> Result<
    (
        cadmpeg_ir::geometry::IntcurveSupportContext,
        Option<cadmpeg_ir::geometry::SurfaceCurveCacheFirst<F>>,
    ),
    &'static str,
> {
    use crate::nurbs::proc_curve::EmbeddedSurfaceCurveLayout;
    let (embedded, tail) = match layout {
        EmbeddedSurfaceCurveLayout::ContextFirst(context) => (context, None),
        EmbeddedSurfaceCurveLayout::CacheFirst { context, flags } => {
            let (context, form) = context
                .into_intersection(solved_domain.ok_or("missing procedural curve cache domain")?);
            let curve_tail = cadmpeg_ir::geometry::SurfaceCurveTail {
                extension: form.extension,
                revision: form.revision,
                cache: form.cache,
                support_bounds: form.support_bounds,
                solved_range: form.solved_range,
            };
            (
                context,
                Some(cadmpeg_ir::geometry::SurfaceCurveCacheFirst {
                    form: curve_tail,
                    flags,
                }),
            )
        }
    };
    let mut next_side = 0;
    let surfaces: [Option<SurfaceId>; 2] = embedded.surfaces.map(|geometry| {
        let side = next_side;
        next_side += 1;
        let geometry = geometry.into_surface()?;
        let id = SurfaceId::mint(format!("{format}:brep:procedural_curve#{i}:support{side}"))
            .expect("identity grammar");
        out.surfaces.push(Surface {
            id: id.clone(),
            geometry,
            source_object: None,
        });
        Some(id)
    });
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
    out: &mut AsmBrep,
    i: i64,
    format: IdFormat<'_>,
    family: crate::nurbs::proc_curve::EmbeddedSurfaceCurve,
    solved_domain: Option<[f64; 2]>,
) -> Result<cadmpeg_ir::geometry::SurfaceCurveFamily, &'static str> {
    use crate::nurbs::proc_curve::EmbeddedSurfaceCurve;
    Ok(match family {
        EmbeddedSurfaceCurve::Blend(layout) => {
            let (context, tail) = emit_surface_curve_layout(out, i, format, layout, solved_domain)?;
            cadmpeg_ir::geometry::SurfaceCurveFamily::Blend { context, tail }
        }
        EmbeddedSurfaceCurve::SurfaceConstrained(layout) => {
            let (context, tail) = emit_surface_curve_layout(out, i, format, layout, solved_domain)?;
            cadmpeg_ir::geometry::SurfaceCurveFamily::SurfaceConstrained { context, tail }
        }
        EmbeddedSurfaceCurve::Parametric(layout) => {
            let (context, tail) = emit_surface_curve_layout(out, i, format, layout, solved_domain)?;
            cadmpeg_ir::geometry::SurfaceCurveFamily::Parametric { context, tail }
        }
        EmbeddedSurfaceCurve::Skin(layout) => {
            let (context, tail) = emit_surface_curve_layout(out, i, format, layout, solved_domain)?;
            cadmpeg_ir::geometry::SurfaceCurveFamily::Skin { context, tail }
        }
    })
}

fn emit_silhouette_curve(
    out: &mut AsmBrep,
    i: i64,
    embedded: EmbeddedSilhouette,
    format: IdFormat<'_>,
) -> Result<cadmpeg_ir::geometry::ProceduralCurveDefinition, &'static str> {
    let mut next_side = 0;
    let support_ids: [Option<SurfaceId>; 2] = embedded.surfaces.map(|geometry| {
        let side = next_side;
        next_side += 1;
        let id = SurfaceId::mint(format!("{format}:brep:procedural_curve#{i}:support{side}"))
            .expect("identity grammar");
        out.surfaces.push(Surface {
            id: id.clone(),
            geometry,
            source_object: None,
        });
        Some(id)
    });
    let pcurves = embedded.pcurves.map(|pcurve| {
        Some(cadmpeg_ir::geometry::SupportPcurve::from(
            PcurveGeometry::Nurbs { nurbs: pcurve },
        ))
    });
    let cast_surface = SurfaceId::mint(format!("{format}:brep:procedural_curve#{i}:cast_surface"))
        .expect("identity grammar");
    out.surfaces.push(Surface {
        id: cast_surface.clone(),
        geometry: embedded.cast_surface,
        source_object: None,
    });
    Ok(cadmpeg_ir::geometry::ProceduralCurveDefinition::Silhouette(
        cadmpeg_ir::geometry::curve_payloads::SilhouetteCurveConstruction::try_new(
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
        )
        .map_err(|_| "silhouette fields are not finite or the light direction is degenerate")?,
    ))
}

fn emit_surface_offset_curve(
    out: &mut AsmBrep,
    i: i64,
    embedded: EmbeddedSurfaceOffset,
    format: IdFormat<'_>,
    solved_domain: Option<[f64; 2]>,
) -> Result<cadmpeg_ir::geometry::ProceduralCurveDefinition, &'static str> {
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
    let mut next_side = 0;
    let support_ids: [Option<SurfaceId>; 2] = context.surfaces.map(|geometry| {
        let side = next_side;
        next_side += 1;
        let geometry = geometry.into_surface()?;
        let id = SurfaceId::mint(format!("{format}:brep:procedural_curve#{i}:support{side}"))
            .expect("identity grammar");
        out.surfaces.push(Surface {
            id: id.clone(),
            geometry,
            source_object: None,
        });
        Some(id)
    });
    let pcurves = context.pcurves.map(|pcurve| {
        pcurve
            .map(|nurbs| cadmpeg_ir::geometry::SupportPcurve::from(PcurveGeometry::Nurbs { nurbs }))
    });
    let base = CurveId::mint(format!("{format}:brep:procedural_curve#{i}:base"))
        .expect("identity grammar");
    out.curves.push(Curve {
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
                cache_first,
                embedded.distance,
                [embedded.shift, embedded.scale],
            )
            .map_err(|_| "surface-offset fields are not finite and ordered")?,
        ),
    )
}

fn emit_spring_surface(
    out: &mut AsmBrep,
    i: i64,
    format: IdFormat<'_>,
    side: usize,
    geometry: SurfaceGeometry,
) -> SurfaceId {
    let id = SurfaceId::mint(format!("{format}:brep:procedural_curve#{i}:support{side}"))
        .expect("identity grammar");
    out.surfaces.push(Surface {
        id: id.clone(),
        geometry,
        source_object: None,
    });
    id
}

fn emit_spring_support(
    out: &mut AsmBrep,
    i: i64,
    format: IdFormat<'_>,
    side: usize,
    support: EmbeddedSpringSupport,
) -> cadmpeg_ir::geometry::SpringSupport {
    match support {
        EmbeddedSpringSupport::Surface(geometry) => cadmpeg_ir::geometry::SpringSupport::Surface(
            emit_spring_surface(out, i, format, side, geometry),
        ),
        EmbeddedSpringSupport::Ranges(ranges) => {
            cadmpeg_ir::geometry::SpringSupport::Ranges(ranges)
        }
    }
}

fn emit_spring_curve(
    out: &mut AsmBrep,
    i: i64,
    embedded: EmbeddedSpring,
    format: IdFormat<'_>,
    solved_domain: Option<[f64; 2]>,
) -> Result<cadmpeg_ir::geometry::ProceduralCurveDefinition, &'static str> {
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
                emit_spring_support(out, i, format, 0, first_support),
                emit_spring_support(out, i, format, 1, second_support),
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
                                .map(|surface| emit_spring_surface(out, i, format, 0, surface)),
                            pcurve: first_pcurve.map(emit_pcurve).map(Into::into),
                        },
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: second_surface
                                .map(|surface| emit_spring_surface(out, i, format, 1, surface)),
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
        .map_err(|_| "spring context or null-support ranges are invalid")?,
    ))
}

fn emit_projection_curve(
    out: &mut AsmBrep,
    i: i64,
    embedded: EmbeddedProjection,
    format: IdFormat<'_>,
) -> Result<cadmpeg_ir::geometry::ProceduralCurveDefinition, &'static str> {
    let mut next_side = 0;
    let surfaces: [Option<SurfaceId>; 2] = embedded.surfaces.map(|geometry| {
        let side = next_side;
        next_side += 1;
        let id = SurfaceId::mint(format!("{format}:brep:procedural_curve#{i}:support{side}"))
            .expect("identity grammar");
        out.surfaces.push(Surface {
            id: id.clone(),
            geometry,
            source_object: None,
        });
        Some(id)
    });
    let pcurves = embedded.pcurves.map(|pcurve| {
        Some(cadmpeg_ir::geometry::SupportPcurve::from(
            PcurveGeometry::Nurbs { nurbs: pcurve },
        ))
    });
    let source = CurveId::mint(format!("{format}:brep:procedural_curve#{i}:source"))
        .expect("identity grammar");
    out.curves.push(Curve {
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
    out: &mut AsmBrep,
    i: i64,
    embedded: EmbeddedLawCurve,
    format: IdFormat<'_>,
    solved_domain: Option<[f64; 2]>,
) -> Result<cadmpeg_ir::geometry::ProceduralCurveDefinition, &'static str> {
    let prefix = format!("{format}:brep:procedural_curve#{i}:law");
    let scope = LawExpressionScope::Curve(&prefix);
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
                Some(cadmpeg_ir::geometry::LawCurveVersionForm {
                    stamp,
                    post_enum,
                    parameter_range,
                }),
            )
        }
    };
    let mut next_side = 0;
    let surfaces: [Option<SurfaceId>; 2] = embedded.surfaces.map(|geometry| {
        let side = next_side;
        next_side += 1;
        let geometry = geometry.into_surface()?;
        let id = SurfaceId::mint(format!("{format}:brep:procedural_curve#{i}:support{side}"))
            .expect("identity grammar");
        out.surfaces.push(Surface {
            id: id.clone(),
            geometry,
            source_object: None,
        });
        Some(id)
    });
    let pcurves = embedded.pcurves.map(|pcurve| {
        pcurve
            .map(|nurbs| cadmpeg_ir::geometry::SupportPcurve::from(PcurveGeometry::Nurbs { nurbs }))
    });
    let mut map_formula = |path: &str, formula: EmbeddedLawFormula| {
        map_law_formula(formula, |index, expression| {
            map_law_expression(&mut *out, scope, &format!("{path}:{index}"), expression)
        })
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
        primary: map_formula("primary", embedded.primary),
        additional: embedded
            .additional
            .into_iter()
            .enumerate()
            .map(|(index, formula)| map_formula(&format!("additional:{index}"), formula))
            .collect(),
        cache: None,
    })
}

/// Pass 3: emit surface and curve carriers in `RecordTable` order for
/// deterministic output.
pub(crate) fn emit_carrier_records(
    out: &mut AsmBrep,
    records: &[Record],
    carriers: &mut Carriers,
    reach: &Reachable,
    reversed_curve_refs: &HashSet<i64>,
    forward_curve_refs: &HashSet<i64>,
    format: IdFormat<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    for r in records {
        let i = r.index as i64;
        match r.head() {
            _ if reach.surfaces.contains(&i) => {
                emit_carrier_surface(out, r, i, carriers, reach, format)?;
            }
            _ if reach.unknown_surface_records.contains(&i) => {
                // Topology-known face on an undecoded surface: emit an opaque
                // carrier linking to the preserved record bytes, marked Unknown.
                out.surfaces.push(Surface {
                    id: SurfaceId::mint(id(format, i)).expect("identity grammar"),
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                        record: Some(
                            UnknownId::mint(unknown_record_id(r, format))
                                .expect("identity grammar"),
                        ),
                    }),
                    source_object: None,
                });
            }
            _ if reach.curves.contains(&i) => {
                emit_carrier_curve(
                    out,
                    i,
                    carriers,
                    reversed_curve_refs,
                    forward_curve_refs,
                    format,
                );
            }
            _ => {}
        }
    }
    Ok(())
}

/// Emit reachable pcurve carriers with their wrapper and fit-tolerance tails.
pub(crate) fn emit_pcurves(
    out: &mut AsmBrep,
    records: &[Record],
    carriers: &mut Carriers,
    reach: &Reachable,
    format: IdFormat<'_>,
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
                            .and_then(nurbs::pcurve::pcurve_fit_tolerance)
                    }
                    _ => None,
                };
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
                        form: PcurveInlineForm::try_new(
                            wrapper_reversed,
                            native_tail_flags,
                            parameter_range,
                            fit_tolerance,
                        )
                        .map_err(cadmpeg_core::CodecError::malformed)?,
                    },
                    (wrapper_reversed, _, parameter_range, fit_tolerance) => {
                        PcurveMetadata::try_general(
                            wrapper_reversed,
                            parameter_range,
                            fit_tolerance,
                        )
                        .map_err(cadmpeg_core::CodecError::malformed)?
                    }
                };
                out.pcurves.push(Pcurve {
                    id: PcurveId::mint(id(format, i)).expect("identity grammar"),
                    geometry,
                    metadata,
                });
            }
        }
    }

    Ok(())
}

/// Emit reachable point carriers, scaled to millimetres.
pub(crate) fn emit_points(
    out: &mut AsmBrep,
    records: &[Record],
    reach: &Reachable,
    format: IdFormat<'_>,
) {
    let Reachable {
        points: kept_points,
        ..
    } = reach;
    for r in records {
        let i = r.index as i64;
        if r.head() == "point" && kept_points.contains(&i) {
            let c = collect_carrier(r);
            if let Some(p) = c.positions.first() {
                out.points.push(Point {
                    id: PointId::mint(id(format, i)).expect("identity grammar"),
                    position: scale_point(*p),
                    source_object: None,
                });
            }
        }
    }
}

/// Emit reachable vertices with their tolerant tails and ownership records.
pub(crate) fn emit_vertices(
    out: &mut AsmBrep,
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
    reach: &Reachable,
    format: IdFormat<'_>,
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
                    out.vertices.push(Vertex {
                        id: VertexId::mint(id(format, i)).expect("identity grammar"),
                        point: PointId::mint(id(format, pi)).expect("identity grammar"),
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
                                        cadmpeg_ir::units::PositiveScalar::new(*value * LEN_TO_MM)
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
                            out.tolerant_vertex_tails.push(TolerantVertexTail {
                                source_namespace:
                                    crate::brep::records::identity::NativeRecordNamespace::new(
                                        format,
                                    ),
                                vertex: VertexId::mint(id(format, i)).expect("identity grammar"),
                                record_index: r.index as u32,
                                leading_tolerances: [*first, *second],
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
                                        _ => EvaluatedToleranceSlot::Absent,
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
                        out.vertex_ownerships.push(VertexOwnership {
                            source_namespace:
                                crate::brep::records::identity::NativeRecordNamespace::new(format),
                            vertex: VertexId::mint(id(format, i)).expect("identity grammar"),
                            record_index: r.index as u32,
                            owning_edge: EdgeId::mint(id(format, owning_edge))
                                .expect("identity grammar"),
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
pub(crate) fn emit_edges(
    out: &mut AsmBrep,
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
    reach: &Reachable,
    reversed_curve_refs: &HashSet<i64>,
    forward_curve_refs: &HashSet<i64>,
    format: IdFormat<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let Reachable {
        edges: kept_edges,
        vertices: kept_vertices,
        curves: kept_curves,
        ..
    } = reach;
    let reversed_curve_id = |c: i64| {
        if reversed_curve_refs.contains(&c) && forward_curve_refs.contains(&c) {
            CurveId::mint(format!("{}:reversed", id(format, c))).expect("identity grammar")
        } else {
            CurveId::mint(id(format, c)).expect("identity grammar")
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
                            let scale = collect_carrier(curve_record)
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
                Sense::Forward => CurveId::mint(id(format, c)).expect("identity grammar"),
            });
            // The tedge tail carries the model-space tolerance, then the
            // per-entity serializer revision stamp, then a trailing LONG
            // present when the stream's full format version (save format
            // x 100 + header revision) is at least 2250003. All forms are
            // retained verbatim.
            let tolerant_tail = match (r.head(), r.chunk(11), r.chunk(12)) {
                ("tedge", Some(Token::Double(tolerance)), Some(Token::Long(revision)))
                    if tolerance.is_finite() && *tolerance >= 0.0 =>
                {
                    let trailing = match r.chunk(13) {
                        Some(Token::Long(second)) => Some(*second),
                        _ => None,
                    };
                    Some((*tolerance, *revision, trailing))
                }
                _ => None,
            };
            out.edges.push(Edge {
                id: EdgeId::mint(id(format, i)).expect("identity grammar"),
                carrier: cadmpeg_ir::topology::EdgeCarrier::new(curve, param_range)
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                start: VertexId::mint(id(format, start)).expect("identity grammar"),
                end: VertexId::mint(id(format, end)).expect("identity grammar"),
                tolerance: tolerant_tail
                    .map(|(tolerance, _, _)| {
                        cadmpeg_ir::units::PositiveScalar::new(tolerance * LEN_TO_MM).ok_or_else(
                            || {
                                cadmpeg_core::CodecError::malformed(
                                    "edge tolerance must be positive and finite",
                                )
                            },
                        )
                    })
                    .transpose()?,
            });
            if let Some((_, entity_revision, trailing_field)) = tolerant_tail {
                out.tolerant_edge_tails.push(TolerantEdgeTail {
                    source_namespace: crate::brep::records::identity::NativeRecordNamespace::new(
                        format,
                    ),
                    edge: EdgeId::mint(id(format, i)).expect("identity grammar"),
                    record_index: r.index as u32,
                    entity_revision,
                    trailing_field,
                });
            }
            out.edge_ownerships.push(EdgeOwnership {
                source_namespace: crate::brep::records::identity::NativeRecordNamespace::new(
                    format,
                ),
                edge: EdgeId::mint(id(format, i)).expect("identity grammar"),
                record_index: r.index as u32,
                owner_coedge: r
                    .ref_at(7)
                    .map(|owner| CoedgeId::mint(id(format, owner)).expect("identity grammar")),
            });
            if let Some(Token::Str(continuity)) = r.chunk(10) {
                out.edge_continuities.push(EdgeContinuity {
                    source_namespace: crate::brep::records::identity::NativeRecordNamespace::new(
                        format,
                    ),
                    edge: EdgeId::mint(id(format, i)).expect("identity grammar"),
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
pub(crate) fn emit_coedges(
    out: &mut AsmBrep,
    records: &[Record],
    token_table: &nurbs::toks::SubtypeTable,
    save_format_major: Option<u32>,
    carriers: &Carriers,
    reach: &Reachable,
    format: IdFormat<'_>,
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
                            Some(_) => Some(TolerantCoedgeExtension::None),
                            None => None,
                        };
                        extension.map(|extension| ([*start, *end], extension))
                    }
                    _ => None,
                }
            } else {
                None
            };
            let use_curve = tolerant.as_ref().and_then(|(range, extension)| {
                let TolerantCoedgeExtension::EmbeddedCurve {
                    curve_reversed,
                    parameter_range,
                    ..
                } = extension
                else {
                    return None;
                };
                let mut curve = nurbs::core::curve_cache_resolving_refs(&r.tokens, token_table)?;
                if *curve_reversed {
                    curve.reverse_parameterization();
                }
                let curve_id = CurveId::mint(format!("{format}:brep:tolerant-coedge-curve#{i}"))
                    .expect("identity grammar");
                out.curves.push(Curve {
                    id: curve_id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                    source_object: None,
                });
                Some((curve_id, parameter_range.unwrap_or(*range)))
            });
            out.coedges.push(Coedge {
                id: CoedgeId::mint(id(format, i)).expect("identity grammar"),
                owner_loop: LoopId::mint(id(format, owner)).expect("identity grammar"),
                edge: EdgeId::mint(id(format, edge)).expect("identity grammar"),
                radial_next: partner.map_or_else(
                    || CoedgeId::mint(id(format, i)).expect("identity grammar"),
                    |p| CoedgeId::mint(id(format, p)).expect("identity grammar"),
                ),
                sense: sense_at(r, 7),
                pcurves: coedge_pcurve_ref(r)
                    .filter(|p| kept_pcurves.contains(p))
                    .map(|p| {
                        Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::topology::PcurveUse {
                            pcurve: PcurveId::mint(id(format, p)).expect("identity grammar"),
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
                    .collect(),
                use_curve: use_curve
                    .map(|(curve, parameter_range)| {
                        Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::topology::CoedgeUseCurve {
                            curve,
                            parameter_range: cadmpeg_ir::topology::ParameterInterval::new(
                                parameter_range,
                            )
                            .map_err(cadmpeg_core::CodecError::malformed)?,
                        })
                    })
                    .transpose()?,
            });
            if let Some((parameter_range, extension)) = tolerant {
                out.tolerant_coedge_parameters
                    .push(TolerantCoedgeParameters {
                        source_namespace:
                            crate::brep::records::identity::NativeRecordNamespace::new(format),
                        coedge: CoedgeId::mint(id(format, i)).expect("identity grammar"),
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
pub(crate) fn emit_loops(
    out: &mut AsmBrep,
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
    reach: &Reachable,
    format: IdFormat<'_>,
) {
    let Reachable {
        loops: kept_loops,
        coedges: kept_coedges,
        ..
    } = reach;
    for r in records {
        let i = r.index as i64;
        if r.head() == "loop" && kept_loops.contains(&i) {
            let Some(owner) = r.ref_at(5) else { continue };
            let coedges = ring_coedges(r, by_index, kept_coedges, format);
            let Ok(ring) = cadmpeg_ir::topology::LoopRing::new(coedges, Vec::new()) else {
                continue;
            };
            out.loops.push(Loop {
                id: LoopId::mint(id(format, i)).expect("identity grammar"),
                face: FaceId::mint(id(format, owner)).expect("identity grammar"),
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
            });
        }
    }
}

/// Emit reachable faces, folding surface reversal into the normalized sense and
/// recording native sidedness.
pub(crate) fn emit_faces(
    out: &mut AsmBrep,
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
    reach: &Reachable,
    inward_normal_surfaces: &HashSet<i64>,
    format: IdFormat<'_>,
) {
    let Reachable {
        faces: kept_faces,
        loops: kept_loops,
        ..
    } = reach;
    let subshell_shells = subshell_ancestor_shells(records, by_index);
    let attribute_color = |entity: &Record| attribute_chain_color(entity, by_index);
    let attribute_name = |entity: &Record| attribute_chain_name(entity, by_index);
    for r in records {
        let i = r.index as i64;
        if r.head() == "face" && kept_faces.contains(&i) {
            let (Some(surface), Some(owner)) = (r.ref_at(7), r.ref_at(5)) else {
                continue;
            };
            let loops = loop_chain(r, by_index, kept_loops, format);
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
            out.faces.push(Face {
                id: FaceId::mint(id(format, i)).expect("identity grammar"),
                shell: ShellId::mint(id(
                    format,
                    subshell_shells.get(&owner).copied().unwrap_or(owner),
                ))
                .expect("identity grammar"),
                surface: SurfaceId::mint(id(format, surface)).expect("identity grammar"),
                sense,
                loops: loops.into(),
                name: attribute_name(r),
                color: attribute_color(r),
                tolerance: None,
            });
            let containment = match (r.chunk(9), r.chunk(10)) {
                (Some(Token::True), Some(Token::True)) => Some(FaceContainment::In),
                (Some(Token::True), Some(Token::False)) => Some(FaceContainment::Out),
                _ => None,
            };
            out.face_sidedness.push(FaceSidedness {
                source_namespace: crate::brep::records::identity::NativeRecordNamespace::new(
                    format,
                ),
                face: FaceId::mint(id(format, i)).expect("identity grammar"),
                record_index: r.index as u32,
                native_sense,
                carrier_flipped,
                containment,
            });
            if let Some(Token::Long(key)) = r.chunk(1) {
                let face_id = FaceId::mint(id(format, i)).expect("identity grammar");
                out.face_native_keys.push(FaceNativeKey {
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
}

/// Emit shells, regions, and bodies for every record so back-references
/// resolve, filtering child lists to reachable entities.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_containers(
    out: &mut AsmBrep,
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
    reach: &Reachable,
    wire: &WireShellTopology,
    stream: &str,
    header_scale: f64,
    format: IdFormat<'_>,
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
    let attribute_name = |entity: &Record| attribute_chain_name(entity, by_index);
    for r in records {
        let i = r.index as i64;
        match r.head() {
            "shell" => {
                let Some(owner) = r.ref_at(7) else { continue };
                let faces = shell_faces(r, by_index, kept_faces, format);
                out.shells.push(
                    Shell::new(
                        ShellId::mint(id(format, i)).expect("identity grammar"),
                        RegionId::mint(id(format, owner)).expect("identity grammar"),
                        faces,
                        wire_edges_by_shell
                            .get(&i)
                            .into_iter()
                            .flatten()
                            .map(|edge| EdgeId::mint(id(format, *edge)).expect("identity grammar"))
                            .collect(),
                        free_vertices_by_shell
                            .get(&i)
                            .into_iter()
                            .flatten()
                            .map(|vertex| {
                                VertexId::mint(id(format, *vertex)).expect("identity grammar")
                            })
                            .collect(),
                    )
                    .map_err(|message| cadmpeg_core::CodecError::Malformed(message.into()))?,
                );
            }
            // Save-format 231 names this record `region`; format-227 streams
            // carry the original ACIS head `lump`. Same layout in both.
            "region" | "lump" => {
                let Some(owner) = r.ref_at(5) else { continue };
                let shells = shell_chain(r, by_index, format);
                out.regions.push(Region {
                    id: RegionId::mint(id(format, i)).expect("identity grammar"),
                    body: BodyId::mint(id(format, owner)).expect("identity grammar"),
                    shells,
                });
            }
            "body" => {
                let regions = region_chain(r, by_index, format);
                let body_id = BodyId::mint(id(format, i)).expect("identity grammar");
                if let Some(Token::Long(key)) = r.chunk(1) {
                    out.body_native_keys.push(BodyNativeKey {
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
                    let flags = transform
                        .tokens
                        .iter()
                        .filter_map(|token| match token {
                            Token::True => Some(true),
                            Token::False => Some(false),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    if let [rotation, reflection, shear] = flags.as_slice() {
                        out.transform_hints.push(TransformHints {
                            source_namespace:
                                crate::brep::records::identity::NativeRecordNamespace::new(format),
                            body: body_id.clone(),
                            record_index: transform.index as u32,
                            rotation: *rotation,
                            reflection: *reflection,
                            shear: *shear,
                        });
                    }
                }
                out.bodies.push(Body {
                    id: body_id,
                    kind: cadmpeg_ir::topology::BodyKind::Solid,
                    regions,
                    transform: transform_record
                        .and_then(|transform| decode_transform(transform, header_scale)),
                    name: attribute_name(r),
                    color: attribute_color(r),
                    visible: None,
                });
            }
            _ => {}
        }
    }
    for &edge in saved_free_edges {
        let body_id = BodyId::mint(format!("{format}:brep:saved-edge-body#{edge}"))
            .expect("identity grammar");
        let region_id = RegionId::mint(format!("{format}:brep:saved-edge-region#{edge}"))
            .expect("identity grammar");
        let shell_id = ShellId::mint(format!("{format}:brep:saved-edge-shell#{edge}"))
            .expect("identity grammar");
        out.bodies.push(Body {
            id: body_id.clone(),
            kind: cadmpeg_ir::topology::BodyKind::Wire,
            regions: vec![region_id.clone()],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        out.regions.push(Region {
            id: region_id.clone(),
            body: body_id,
            shells: vec![shell_id.clone()],
        });
        out.shells.push(Shell::with_wire_edge(
            shell_id,
            region_id,
            EdgeId::mint(id(format, edge)).expect("identity grammar"),
        ));
    }
    Ok(())
}

/// Emit direct and inherited entity attributes and derive the link, tag, and
/// timestamp projections. Returns the set of emitted attribute record indices.
pub(crate) fn emit_attributes(
    out: &mut AsmBrep,
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
    reach: &Reachable,
    format: IdFormat<'_>,
) -> HashSet<i64> {
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
                    .any(|entity| entity.id.as_str() == id(format, index)) =>
            {
                Some(AttributeTarget::Body(
                    BodyId::mint(id(format, index)).expect("identity grammar"),
                ))
            }
            "shell"
                if out
                    .shells
                    .iter()
                    .any(|entity| entity.id.as_str() == id(format, index)) =>
            {
                Some(AttributeTarget::Shell(
                    ShellId::mint(id(format, index)).expect("identity grammar"),
                ))
            }
            // ASM-227 names a region's topological owner `lump`, while
            // ASM-231 names the same record `region`. The neutral model has
            // no region-level attribute target, so retain these attributes on
            // their owning body after the region graph has been emitted.
            "region" | "lump" => out
                .regions
                .iter()
                .find(|entity| entity.id.as_str() == id(format, index))
                .map(|entity| AttributeTarget::Body(entity.body.clone())),
            "face" if kept_faces.contains(&index) => Some(AttributeTarget::Face(
                FaceId::mint(id(format, index)).expect("identity grammar"),
            )),
            "loop" if kept_loops.contains(&index) => Some(AttributeTarget::Loop(
                LoopId::mint(id(format, index)).expect("identity grammar"),
            )),
            "coedge" | "tcoedge" if kept_coedges.contains(&index) => Some(AttributeTarget::Coedge(
                CoedgeId::mint(id(format, index)).expect("identity grammar"),
            )),
            "edge" | "tedge" if kept_edges.contains(&index) => Some(AttributeTarget::Edge(
                EdgeId::mint(id(format, index)).expect("identity grammar"),
            )),
            "vertex" | "tvertex" if kept_vertices.contains(&index) => {
                Some(AttributeTarget::Vertex(
                    VertexId::mint(id(format, index)).expect("identity grammar"),
                ))
            }
            _ => None,
        };
        if let Some(target) = target {
            attribute_targets.insert(index, target.clone());
            collect_attributes(
                record,
                &target,
                by_index,
                &mut emitted_attributes,
                &mut out.attributes,
                format,
            );
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
            emitted_attributes.insert(index);
            out.attributes
                .push(source_attribute(record, target, format));
        }
    }
    emitted_attributes
}

/// Preserve undecoded carriers and opaque cached procedural surfaces referenced
/// by real topology as passthrough unknown records.
pub(crate) fn emit_passthrough_unknowns(
    out: &mut AsmBrep,
    records: &[Record],
    bytes: &[u8],
    reach: &Reachable,
    format: IdFormat<'_>,
) {
    let Reachable {
        undecoded_carriers,
        cached_unknown_procedural_surfaces,
        ..
    } = reach;
    for r in records {
        let i = r.index as i64;
        if undecoded_carriers.contains(&i) || cached_unknown_procedural_surfaces.contains(&i) {
            out.unknowns.push(UnknownRecord::retained(
                UnknownId::mint(unknown_record_id(r, format)).expect("identity grammar"),
                r.offset as u64,
                bytes[r.offset..(r.offset + r.len).min(bytes.len())].to_vec(),
                Vec::new(),
            ));
        }
    }
}

/// Count record kinds that were neither emitted nor preserved.
pub(crate) fn count_other_records(
    out: &mut AsmBrep,
    records: &[Record],
    reach: &Reachable,
    emitted_attributes: &HashSet<i64>,
) {
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
        .collect();
    let pcurve_intcurves: HashSet<i64> = records
        .iter()
        .filter(|record| kept_pcurves.contains(&(record.index as i64)))
        .filter_map(|record| record.ref_at(4))
        .collect();
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
            *out.stats
                .other_record_kinds
                .entry(r.name.clone())
                .or_default() += 1;
        }
    }
}

#[cfg(test)]
mod tests;
