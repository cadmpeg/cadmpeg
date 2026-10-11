// SPDX-License-Identifier: Apache-2.0
//! Standard extrusion directrix construction and support retention.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, DirectedParameterRange, IntcurveSupportContext, IntcurveSupportSide,
    ProceduralCurve, ProceduralCurveDefinition, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SupportPcurve, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::{AnnotationBuilder, Exactness};
use std::collections::HashMap;

use crate::assemble::{annotate, cgm_source};
use crate::families::FamilyEntityAdmission;

fn standard_extrusion_support_id(
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    surfaces: &mut Vec<Surface>,
    procedural_supports: &mut HashMap<u32, usize>,
    map_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    surface_object_id: u32,
    geometry: &SurfaceGeometry,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<SurfaceId, cadmpeg_core::CodecError> {
    let ctx = admission.context();
    if let Some(&index) = ctx.get_hash_map(
        procedural_supports,
        &surface_object_id,
        "catia_extrusion_support_lookup",
    )? {
        return surfaces[index]
            .id
            .try_clone_for_decode(ctx, "catia_extrusion_existing_support_id_copy");
    }
    let source_object = cgm_source(ctx, "surface", surface_object_id)?;
    let id = crate::resource::compose_u32_id(
        ctx,
        &cadmpeg_ir::identity_namespace!("catia", "standard", "procedural-support"),
        surface_object_id,
        SurfaceId::mint,
        "catia_extrusion_support_surface_id",
    )?;
    annotate(
        ctx,
        annotations,
        &id,
        "object_stream_b5_03",
        0,
        format_args!("surface:{surface_object_id:08x}"),
        Exactness::ByteExact,
    )?;
    admission.reserve_entity(surfaces, "catia_standard_extrusion_support_surfaces")?;
    surfaces.push(Surface {
        id: id.try_clone_for_decode(ctx, "catia_extrusion_support_surface_id_copy")?,
        geometry: geometry.try_clone_for_decode(ctx, "catia_standard_offset_support_copy")?,
        source_object: Some(source_object),
    });
    map_storage.with_storage(|| {
        ctx.insert_hash_map(
            procedural_supports,
            surface_object_id,
            surfaces.len() - 1,
            "catia_extrusion_support_map",
        )
    })?;
    Ok(id)
}

/// Emit one resolved object-stream extrusion construction in the standard family.
pub(super) fn emit_standard_extrusion_definition(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    (surfaces, procedural_supports, map_storage): (
        &mut Vec<Surface>,
        &mut HashMap<u32, usize>,
        &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ),
    extrusion_definitions: &mut HashMap<
        u32,
        cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction,
    >,
    extrusion: &crate::families::b5::transfer::ResolvedExtrusionSurface,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    if let Some(definition) = ctx.get_hash_map(
        extrusion_definitions,
        &extrusion.surface_object_id,
        "catia_extrusion_definition_lookup",
    )? {
        return copy_standard_extrusion_definition(ctx, definition);
    }
    let surface_object_id = extrusion.surface_object_id;
    let directrix_id = map_storage.with_storage(|| {
        crate::resource::compose_u32_id(
            ctx,
            &cadmpeg_ir::identity_namespace!("catia", "standard", "extrusion-directrix"),
            extrusion.directrix_object_id,
            CurveId::mint,
            "catia_extrusion_directrix_identity",
        )
    })?;
    let parameter_range = extrusion.directrix_parameter_range;
    let direction = extrusion.direction;
    match &extrusion.directrix {
        crate::families::b5::transfer::ResolvedExtrusionDirectrix::Intersection {
            supports,
            cache_fit_tolerance,
        } => {
            let [first, second] = supports.as_ref();
            let mut build_side =
                |side: &crate::families::b5::transfer::ResolvedExtrusionSupport| {
                    Ok::<_, cadmpeg_core::CodecError>(IntcurveSupportSide {
                        surface: Some(standard_extrusion_support_id(
                            annotations,
                            surfaces,
                            procedural_supports,
                            map_storage,
                            side.surface_object_id,
                            &side.surface,
                            admission,
                        )?),
                        pcurve: Some(SupportPcurve::new(
                            side.pcurve.try_clone_for_decode(
                                ctx,
                                "catia_standard_extrusion_pcurve_copy",
                            )?,
                            (side.pcurve_parameter_range
                                != extrusion.directrix_parameter_range.endpoints())
                            .then(|| DirectedParameterRange::new(side.pcurve_parameter_range).ok())
                            .flatten(),
                        )),
                    })
                };
            let sides = [build_side(first)?, build_side(second)?];
            annotate(
                ctx,
                annotations,
                &directrix_id,
                "object_stream_a8_03_25",
                0,
                "two_support_directrix",
                Exactness::Unknown,
            )?;
            admission.reserve_entity(&mut ir.model.curves, "catia_extrusion_directrix_curves")?;
            ir.model.curves.push(Curve {
                id: directrix_id.try_clone_for_decode(ctx, "catia_extrusion_directrix_curve_id")?,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                source_object: Some(cgm_source(ctx, "curve", extrusion.directrix_object_id)?),
            });
            let procedure_id = crate::resource::compose_u32_id(
                ctx,
                &cadmpeg_ir::identity_namespace!(
                    "catia",
                    "standard",
                    "extrusion-directrix-procedure"
                ),
                extrusion.directrix_object_id,
                ProceduralCurveId::mint,
                "catia_extrusion_directrix_procedure_identity",
            )?;
            annotate(
                ctx,
                annotations,
                &procedure_id,
                "object_stream_a8_03_25",
                0,
                "two_surface_pcurve_intersection",
                Exactness::ByteExact,
            )?;
            admission.charge()?;
            let procedure = ProceduralCurve::new(
                procedure_id,
                ProceduralCurveDefinition::Intersection {
                    context: IntcurveSupportContext::over_interval(
                        sides,
                        extrusion.directrix_parameter_range,
                    ),
                    discontinuity_flag: false,
                    cache: Some(cadmpeg_ir::geometry::LegacyCache::new(
                        cadmpeg_ir::scalar::NonNegativeReal::from(*cache_fit_tolerance).into(),
                    )),
                },
            );
            let _attached = ir
                .model
                .add_procedural_curve(ctx, &directrix_id, procedure)?;
        }
        crate::families::b5::transfer::ResolvedExtrusionDirectrix::SurfaceCurve {
            curve, ..
        } => {
            annotate(
                ctx,
                annotations,
                &directrix_id,
                "object_stream_b5_03_24",
                0,
                "support_pcurve_lift",
                Exactness::Derived,
            )?;
            admission.reserve_entity(
                &mut ir.model.curves,
                "catia_extrusion_surface_directrix_curves",
            )?;
            ir.model.curves.push(Curve {
                id: directrix_id.try_clone_for_decode(ctx, "catia_extrusion_surface_curve_id")?,
                geometry: curve
                    .try_clone_for_decode(ctx, "catia_standard_extrusion_directrix_copy")?,
                source_object: Some(cgm_source(ctx, "curve", extrusion.directrix_object_id)?),
            });
        }
        crate::families::b5::transfer::ResolvedExtrusionDirectrix::Offset {
            source_object_id,
            support,
            source_curve,
            source_parameter_range,
            distance,
            direction,
        } => {
            let source_id = crate::resource::compose_u32_id(
                ctx,
                &cadmpeg_ir::identity_namespace!("catia", "standard", "extrusion-directrix-source"),
                *source_object_id,
                CurveId::mint,
                "catia_extrusion_source_identity",
            )?;
            annotate(
                ctx,
                annotations,
                &source_id,
                "object_stream_b5_03_24",
                0,
                "support_pcurve_lift",
                Exactness::Derived,
            )?;
            admission
                .reserve_entity(&mut ir.model.curves, "catia_extrusion_offset_source_curves")?;
            ir.model.curves.push(Curve {
                id: source_id.try_clone_for_decode(ctx, "catia_extrusion_offset_source_id")?,
                geometry: source_curve
                    .try_clone_for_decode(ctx, "catia_standard_extrusion_source_copy")?,
                source_object: Some(cgm_source(ctx, "curve", *source_object_id)?),
            });
            annotate(
                ctx,
                annotations,
                &directrix_id,
                "object_stream_b5_03_14",
                0,
                "fixed_direction_offset_curve",
                Exactness::Unknown,
            )?;
            admission.reserve_entity(
                &mut ir.model.curves,
                "catia_extrusion_offset_directrix_curves",
            )?;
            ir.model.curves.push(Curve {
                id: directrix_id
                    .try_clone_for_decode(ctx, "catia_extrusion_offset_directrix_id")?,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                source_object: Some(cgm_source(ctx, "curve", extrusion.directrix_object_id)?),
            });
            let procedure_id = crate::resource::compose_u32_id(
                ctx,
                &cadmpeg_ir::identity_namespace!(
                    "catia",
                    "standard",
                    "extrusion-directrix-procedure"
                ),
                extrusion.directrix_object_id,
                ProceduralCurveId::mint,
                "catia_extrusion_directrix_procedure_identity",
            )?;
            annotate(
                ctx,
                annotations,
                &procedure_id,
                "object_stream_b5_03_14",
                0,
                "fixed_direction_offset_curve",
                Exactness::ByteExact,
            )?;
            let support = standard_extrusion_support_id(
                annotations,
                surfaces,
                procedural_supports,
                map_storage,
                support.surface_object_id,
                &support.surface,
                admission,
            )?;
            admission.charge()?;

            let _attached = ir.model.add_procedural_curve(ctx, &directrix_id, ProceduralCurve::new(
                    procedure_id,
                    ProceduralCurveDefinition::Offset(
                        cadmpeg_ir::geometry::curve_payloads::OffsetCurveConstruction::along_direction(
                            source_id,
                            *distance,
                            *direction,
                            Some(support),
                            *source_parameter_range,
                        ),
                    ),
                ))?;
        }
    }
    let definition = cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::legacy(
        directrix_id,
        Some(parameter_range.into()),
        direction.into(),
        None,
        None,
    );
    let returned = copy_standard_extrusion_definition(ctx, &definition)?;
    map_storage.with_storage(|| {
        ctx.insert_hash_map(
            extrusion_definitions,
            surface_object_id,
            definition,
            "catia_extrusion_definitions",
        )
    })?;
    Ok(returned)
}

fn copy_standard_extrusion_definition(
    ctx: &DecodeContext<'_>,
    definition: &cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction,
) -> Result<ProceduralSurfaceDefinition, CodecError> {
    let directrix = definition
        .directrix()
        .try_clone_for_decode(ctx, "catia_extrusion_definition_directrix_id")?;
    Ok(ProceduralSurfaceDefinition::Extrusion(
        cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::legacy(
            directrix,
            definition.parameter_interval(),
            *definition.direction(),
            definition.native_position(),
            None,
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::{copy_standard_extrusion_definition, standard_extrusion_support_id};
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::AnnotationBuilder;
    use std::collections::HashMap;

    #[test]
    fn standard_extrusion_support_refuses_before_surface_storage() {
        let mut surfaces = Vec::new();
        let mut supports = HashMap::new();
        let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
            standard_extrusion_support_id(
                &mut AnnotationBuilder::new(),
                &mut surfaces,
                &mut supports,
                &mut ctx.reserve_scoped(0, "test support map").expect("scope"),
                7,
                &geometry,
                &mut admission,
            )
        });
        assert!(matches!(
            limited,
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
        crate::test_support::with_service_context(|ctx| {
            let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
            standard_extrusion_support_id(
                &mut AnnotationBuilder::new(),
                &mut surfaces,
                &mut supports,
                &mut ctx.reserve_scoped(0, "test support map").expect("scope"),
                7,
                &geometry,
                &mut admission,
            )
        })
        .expect("service context admits support");
        assert_eq!(surfaces.len(), 1);
        assert_eq!(supports.len(), 1);
    }

    #[test]
    fn standard_extrusion_definition_copy_refuses_retained_id_limit() {
        let directrix =
            cadmpeg_ir::ids::CurveId::mint("catia:test:directrix#7").expect("identity grammar");
        let direction =
            cadmpeg_ir::features::FiniteVector3::new(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0))
                .expect("finite direction");
        let definition =
            cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::legacy(
                directrix, None, direction, None, None,
            );
        let limited = crate::test_support::with_retained_limit(0, |ctx| {
            copy_standard_extrusion_definition(ctx, &definition)
        });
        assert!(matches!(
            limited,
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
        let copied = crate::test_support::with_service_context(|ctx| {
            copy_standard_extrusion_definition(ctx, &definition)
        })
        .expect("service context admits the retained directrix identity");
        assert_eq!(
            copied,
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion(definition),
        );
    }

    #[test]
    fn cached_extrusion_support_does_not_copy_unused_geometry() {
        let id = SurfaceId::mint("catia:test:surface#cached").expect("identity");
        let mut surfaces = vec![Surface {
            id: id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        }];
        let mut supports = HashMap::from([(7, 0usize)]);
        let unused = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
            record: Some(
                cadmpeg_ir::ids::UnknownId::mint(format!(
                    "catia:test:unknown#{}",
                    "x".repeat(16384)
                ))
                .expect("identity"),
            ),
        });
        let copied = crate::test_support::with_retained_limit(128, |ctx| {
            let mut storage = ctx.reserve_scoped(0, "support map")?;
            standard_extrusion_support_id(
                &mut AnnotationBuilder::new(),
                &mut surfaces,
                &mut supports,
                &mut storage,
                7,
                &unused,
                &mut crate::families::FamilyEntityAdmission::new(ctx),
            )
        })
        .expect("only the cached output ID is copied");
        assert_eq!(copied, id);
        assert_eq!(surfaces.len(), 1);
        assert_eq!(supports, HashMap::from([(7, 0usize)]));
    }

    #[test]
    fn procedural_support_lookup_uses_the_stored_arena_index() {
        use super::standard_extrusion_support_id;
        use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
        use cadmpeg_ir::ids::SurfaceId;
        let expected = SurfaceId::mint("catia:test:surface#existing").expect("id");
        let mut surfaces = (0..1024)
            .map(|index| Surface {
                id: SurfaceId::mint(format!("catia:test:surface#{index}")).expect("id"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
                source_object: None,
            })
            .collect::<Vec<_>>();
        surfaces.push(Surface {
            id: expected.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        });
        let mut supports = std::collections::HashMap::from([(7u32, 1024usize)]);
        crate::test_support::with_work_limit(1024, |ctx| {
            let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
            let result = standard_extrusion_support_id(
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &mut surfaces,
                &mut supports,
                &mut ctx.reserve_scoped(0, "test support map").expect("scope"),
                7,
                &SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
                &mut admission,
            )?;
            assert_eq!(result, expected);
            assert_eq!(surfaces.len(), 1025);
            assert_eq!(
                supports,
                std::collections::HashMap::from([(7u32, 1024usize)])
            );
            Ok::<_, cadmpeg_core::CodecError>(())
        })
        .expect("one lookup and identity copy");
    }
}
