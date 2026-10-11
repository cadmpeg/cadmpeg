// SPDX-License-Identifier: Apache-2.0
//! Exhaustive transfer from an ASM graph into neutral and native IR arenas.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::native::NativeNamespace;
use cadmpeg_ir::unknown::UnknownRecord;

use super::annotations::AnnotationRecord;
use super::stats::Stats;
use super::AsmBrep;

const ASM_NATIVE_ARENAS: [&str; 12] = [
    "edge_continuities",
    "edge_ownerships",
    "vertex_ownerships",
    "face_sidedness",
    "face_native_keys",
    "tolerant_vertex_tails",
    "tolerant_edge_tails",
    "tolerant_coedge_parameters",
    "mesh_surface_sentinels",
    "wire_topologies",
    "transform_hints",
    "body_native_keys",
];

/// ASM facts that remain owned by the embedding codec after IR transfer.
pub struct AsmTransferRemainder {
    /// Undecoded ASM records for source-fidelity retention.
    pub unknowns: Vec<UnknownRecord>,
    /// ASM loss statistics used to build the embedding format's report.
    pub stats: Stats,
    /// Source offsets used to build decode annotations.
    pub annotation_records: Vec<AnnotationRecord>,
}

/// Moves one complete ASM graph into the IR and serializes every ASM-native arena.
///
/// The exhaustive [`AsmBrep`] destructure makes a newly added decoder field a
/// compile error until this boundary assigns its disposition.
pub fn transfer_into_ir<'ir>(
    ctx: &DecodeContext<'_>,
    ir: &'ir mut CadIr,
    native_format: &str,
    brep: AsmBrep,
) -> Result<(&'ir NativeNamespace, AsmTransferRemainder), CodecError> {
    if ir.native.namespace(native_format).is_some_and(|namespace| {
        ASM_NATIVE_ARENAS.iter().any(|name| {
            namespace
                .arenas()
                .get(*name)
                .is_some_and(|records| !records.is_empty())
        })
    }) {
        return Err(CodecError::malformed(format_args!(
            "native namespace {native_format} already contains ASM records"
        )));
    }

    let AsmBrep {
        bodies,
        regions,
        shells,
        faces,
        loops,
        coedges,
        edges,
        vertices,
        points,
        surfaces,
        curves,
        pcurves,
        procedural_surfaces,
        procedural_curves,
        edge_continuities,
        edge_ownerships,
        vertex_ownerships,
        face_sidedness,
        face_native_keys,
        tolerant_coedge_parameters,
        tolerant_edge_tails,
        tolerant_vertex_tails,
        mesh_surface_sentinels,
        transform_hints,
        body_native_keys,
        wire_topologies,
        attributes,
        unknowns,
        stats,
        annotation_records,
    } = brep;

    let entity_count = [
        bodies.len(),
        regions.len(),
        shells.len(),
        faces.len(),
        loops.len(),
        coedges.len(),
        edges.len(),
        vertices.len(),
        points.len(),
        surfaces.len(),
        curves.len(),
        pcurves.len(),
        procedural_surfaces.len(),
        procedural_curves.len(),
        attributes.len(),
    ]
    .into_iter()
    .try_fold(0_u64, |total, count| {
        total
            .checked_add(cadmpeg_core::decode::u64_from_index(count))
            .ok_or_else(|| ctx.refuse_codec_limit("admit ASM entities", u64::MAX, u64::MAX))
    })?;
    ctx.charge_entities(entity_count, "admit ASM entities")?;
    ctx.extend_vec(&mut ir.model.bodies, bodies, "ASM transfer bodies")?;
    ctx.extend_vec(&mut ir.model.regions, regions, "ASM transfer regions")?;
    ctx.extend_vec(&mut ir.model.shells, shells, "ASM transfer shells")?;
    ctx.extend_vec(&mut ir.model.faces, faces, "ASM transfer faces")?;
    ctx.extend_vec(&mut ir.model.loops, loops, "ASM transfer loops")?;
    ctx.extend_vec(&mut ir.model.coedges, coedges, "ASM transfer coedges")?;
    ctx.extend_vec(&mut ir.model.edges, edges, "ASM transfer edges")?;
    ctx.extend_vec(&mut ir.model.vertices, vertices, "ASM transfer vertices")?;
    ctx.extend_vec(&mut ir.model.points, points, "ASM transfer points")?;
    ctx.extend_vec(&mut ir.model.surfaces, surfaces, "ASM transfer surfaces")?;
    ctx.extend_vec(&mut ir.model.curves, curves, "ASM transfer curves")?;
    ctx.extend_vec(&mut ir.model.pcurves, pcurves, "ASM transfer pcurves")?;
    if !procedural_surfaces.is_empty() {
        let outcomes = ir.model.add_procedural_surfaces(ctx, procedural_surfaces)?;
        if let Some(error) = ctx.find_map(
            outcomes,
            |outcome| Ok(outcome.err()),
            "ASM procedural surface transfer",
        )? {
            return Err(CodecError::Malformed(ctx.format_retained(
                format_args!("{error}"),
                "ASM procedural surface transfer error",
            )?));
        }
    }
    if !procedural_curves.is_empty() {
        let outcomes = ir.model.add_procedural_curves(ctx, procedural_curves)?;
        if let Some(error) = ctx.find_map(
            outcomes,
            |outcome| Ok(outcome.err()),
            "ASM procedural curve transfer",
        )? {
            return Err(CodecError::Malformed(ctx.format_retained(
                format_args!("{error}"),
                "ASM procedural curve transfer error",
            )?));
        }
    }
    ctx.extend_vec(
        &mut ir.model.attributes,
        attributes,
        "ASM transfer attributes",
    )?;

    let namespace = ir.native.namespace_mut(native_format);
    namespace.set_arena(ctx, "edge_continuities", &edge_continuities)?;
    namespace.set_arena(ctx, "edge_ownerships", &edge_ownerships)?;
    namespace.set_arena(ctx, "vertex_ownerships", &vertex_ownerships)?;
    namespace.set_arena(ctx, "face_sidedness", &face_sidedness)?;
    namespace.set_arena(ctx, "face_native_keys", &face_native_keys)?;
    namespace.set_arena(ctx, "tolerant_vertex_tails", &tolerant_vertex_tails)?;
    namespace.set_arena(ctx, "tolerant_edge_tails", &tolerant_edge_tails)?;
    namespace.set_arena(
        ctx,
        "tolerant_coedge_parameters",
        &tolerant_coedge_parameters,
    )?;
    namespace.set_arena(ctx, "mesh_surface_sentinels", &mesh_surface_sentinels)?;
    namespace.set_arena(ctx, "wire_topologies", &wire_topologies)?;
    namespace.set_arena(ctx, "transform_hints", &transform_hints)?;
    namespace.set_arena(ctx, "body_native_keys", &body_native_keys)?;

    Ok((
        namespace,
        AsmTransferRemainder {
            unknowns,
            stats,
            annotation_records,
        },
    ))
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

    use super::transfer_into_ir;
    use crate::brep::AsmBrep;
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_ir::document::CadIr;

    #[test]
    fn empty_transfer_still_declares_every_asm_native_arena() {
        let source = [0_u8];
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::default())
            .expect("test root fits policy");
        let mut ir = CadIr::empty();
        let (namespace, remainder) = transfer_into_ir(&ctx, &mut ir, "test", AsmBrep::default())
            .expect("empty ASM transfer succeeds");
        assert!(remainder.unknowns.is_empty());
        assert!(remainder.annotation_records.is_empty());
        assert_eq!(namespace.arenas().len(), 12);
    }

    #[test]
    fn transfer_regions_refuses_collection_limit() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        use cadmpeg_ir::ids::{BodyId, RegionId};
        use cadmpeg_ir::topology::Region;

        let source = [0_u8];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy)
            .expect("test root fits policy");
        let mut brep = AsmBrep::default();
        brep.regions.push(Region {
            id: RegionId::mint("f3d:brep:region#1").unwrap(),
            body: BodyId::mint("f3d:brep:body#1").unwrap(),
            shells: Vec::new(),
        });
        let error = transfer_into_ir(&ctx, &mut CadIr::empty(), "test", brep)
            .err()
            .expect("one region exceeds zero items");
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected collection refusal: {error:?}");
        };
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
        assert_eq!(limit.operation, "ASM transfer regions");
    }

    #[test]
    fn transfer_refuses_entities_before_mutating_the_destination() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        use cadmpeg_ir::ids::{BodyId, RegionId};
        use cadmpeg_ir::topology::Region;

        let source = [0_u8];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy)
            .expect("test root fits policy");
        let mut brep = AsmBrep::default();
        brep.regions.push(Region {
            id: RegionId::mint("sat:brep:region#1").unwrap(),
            body: BodyId::mint("sat:brep:body#1").unwrap(),
            shells: Vec::new(),
        });
        let mut ir = CadIr::empty();
        let error = transfer_into_ir(&ctx, &mut ir, "test", brep)
            .err()
            .expect("one entity exceeds zero entities");
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected entity refusal: {error:?}");
        };
        assert_eq!(limit.dimension, ResourceDimension::Entities);
        assert_eq!(limit.operation, "admit ASM entities");
        assert_eq!(ir.model.entity_count(), 0);
        assert!(ir.model.regions.is_empty());
        assert!(ir.native.namespace("test").is_none());
    }

    #[test]
    fn transfer_refuses_to_replace_existing_asm_native_records() {
        #[derive(serde::Deserialize, serde::Serialize)]
        struct HeldRecord {
            id: String,
        }

        let source = [0_u8];
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::default())
            .expect("test root fits policy");
        let mut ir = CadIr::empty();
        ir.native
            .namespace_mut("test")
            .set_arena(
                &ctx,
                "body_native_keys",
                &[HeldRecord {
                    id: "sat:test:held#0".into(),
                }],
            )
            .expect("test native record serializes");
        assert!(transfer_into_ir(&ctx, &mut ir, "test", AsmBrep::default()).is_err());
        let held: Vec<HeldRecord> = ir
            .native
            .namespace("test")
            .expect("namespace remains present")
            .arena_as("body_native_keys")
            .expect("held record remains readable");
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].id, "sat:test:held#0");
    }

    fn procedural_brep(count: usize, surfaces: bool) -> AsmBrep {
        use cadmpeg_ir::features::FinitePoint3;
        use cadmpeg_ir::geometry::{
            Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
            ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
            SurfaceGeometry,
        };
        use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
        use cadmpeg_ir::math::{Point3, Vector3};
        use cadmpeg_ir::units::{OrthonormalFrame3, UnitVector3};
        let mut brep = AsmBrep::default();
        for index in 0..count {
            // Long valid identities exercise the byte work of the owner index.
            if surfaces {
                let owner = SurfaceId::mint(format!("test:model:surface#{index:0192}"))
                    .expect("owner identity");
                let procedural = ProceduralSurface::new(
                    ProceduralSurfaceId::mint(format!(
                        "test:model:procedural-surface#{index:0192}"
                    ))
                    .expect("construction identity"),
                    ProceduralSurfaceDefinition::Unknown {
                        record: None,
                        cache: None,
                    },
                    None,
                );
                let frame = OrthonormalFrame3::from_units(UnitVector3::Z_AXIS, UnitVector3::X_AXIS)
                    .expect("orthonormal frame");
                brep.surfaces.push(Surface {
                    id: owner.clone(),
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                        cadmpeg_ir::geometry::analytic::PlaneSurface::new(
                            FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("finite origin"),
                            frame,
                        ),
                    )),
                    source_object: None,
                });
                brep.procedural_surfaces.push((owner, procedural));
            } else {
                let owner = CurveId::mint(format!("test:model:curve#{index:0192}"))
                    .expect("owner identity");
                let procedural = ProceduralCurve::new(
                    ProceduralCurveId::mint(format!("test:model:procedural-curve#{index:0192}"))
                        .expect("construction identity"),
                    ProceduralCurveDefinition::Exact { cache: None },
                );
                brep.curves.push(Curve {
                    id: owner.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                            Point3::new(0.0, 0.0, 0.0),
                            Vector3::new(1.0, 0.0, 0.0),
                        )
                        .expect("line carrier"),
                    )),
                    source_object: None,
                });
                brep.procedural_curves.push((owner, procedural));
            }
        }
        brep
    }

    fn transfer_thousands(surfaces: bool) {
        const COUNT: usize = 4096;
        let brep = procedural_brep(COUNT, surfaces);
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
            .expect("default context");
        let mut ir = CadIr::empty();
        transfer_into_ir(&ctx, &mut ir, "test", brep)
            .expect("thousands of attachments fit default budget");
        ctx.finish_session().expect("unfused transfer");
        let validation = cadmpeg_ir::validate_neutral(&ir, Vec::new()).expect("validation budget");
        assert!(validation.is_ok(), "{:?}", validation.findings);
        if surfaces {
            assert_eq!(ir.model.surfaces.len(), COUNT);
            assert_eq!(ir.model.procedural_surfaces.len(), COUNT);
            for (owner, procedural) in ir.model.surfaces.iter().zip(&ir.model.procedural_surfaces) {
                assert_eq!(
                    owner.geometry.procedural_construction(),
                    Some(&procedural.id)
                );
                assert!(owner.geometry.solved().is_some());
            }
        } else {
            assert_eq!(ir.model.curves.len(), COUNT);
            assert_eq!(ir.model.procedural_curves.len(), COUNT);
            for (owner, procedural) in ir.model.curves.iter().zip(&ir.model.procedural_curves) {
                assert_eq!(
                    owner.geometry.procedural_construction(),
                    Some(&procedural.id)
                );
                assert!(owner.geometry.solved().is_some());
            }
        }
    }

    #[test]
    fn surface_transfer_batch_accepts_thousands_under_default_budget() {
        transfer_thousands(true);
    }

    #[test]
    fn curve_transfer_batch_accepts_thousands_under_default_budget() {
        transfer_thousands(false);
    }

    #[test]
    fn transfer_batches_report_the_first_invalid_carrier() {
        use cadmpeg_core::CodecError;
        for surfaces in [true, false] {
            let mut brep = procedural_brep(2, surfaces);
            if surfaces {
                brep.surfaces.clear();
            } else {
                brep.curves.clear();
            }
            let noun = if surfaces { "surface" } else { "curve" };
            let expected = CodecError::malformed(format_args!(
                "procedural {noun} test:model:procedural-{noun}#{:0192} references missing {noun} test:model:{noun}#{:0192}",
                0, 0,
            ));
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
                .expect("default context");
            let actual = transfer_into_ir(&ctx, &mut CadIr::empty(), "test", brep)
                .err()
                .expect("missing owner refuses transfer");
            assert_eq!(actual.to_string(), expected.to_string());
        }
    }
}
