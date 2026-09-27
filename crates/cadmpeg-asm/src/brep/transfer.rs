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

    let before = ir.model.entity_count();
    crate::decode_alloc::extend_vec(ctx, &mut ir.model.bodies, bodies, "ASM transfer bodies")?;
    crate::decode_alloc::extend_vec(ctx, &mut ir.model.regions, regions, "ASM transfer regions")?;
    crate::decode_alloc::extend_vec(ctx, &mut ir.model.shells, shells, "ASM transfer shells")?;
    crate::decode_alloc::extend_vec(ctx, &mut ir.model.faces, faces, "ASM transfer faces")?;
    crate::decode_alloc::extend_vec(ctx, &mut ir.model.loops, loops, "ASM transfer loops")?;
    crate::decode_alloc::extend_vec(ctx, &mut ir.model.coedges, coedges, "ASM transfer coedges")?;
    crate::decode_alloc::extend_vec(ctx, &mut ir.model.edges, edges, "ASM transfer edges")?;
    crate::decode_alloc::extend_vec(ctx, &mut ir.model.vertices, vertices, "ASM transfer vertices")?;
    crate::decode_alloc::extend_vec(ctx, &mut ir.model.points, points, "ASM transfer points")?;
    crate::decode_alloc::extend_vec(ctx, &mut ir.model.surfaces, surfaces, "ASM transfer surfaces")?;
    crate::decode_alloc::extend_vec(ctx, &mut ir.model.curves, curves, "ASM transfer curves")?;
    crate::decode_alloc::extend_vec(ctx, &mut ir.model.pcurves, pcurves, "ASM transfer pcurves")?;
    for (owner, procedural) in procedural_surfaces {
        ir.model
            .add_procedural_surface(owner, procedural)
            .map_err(|error| CodecError::malformed(error.to_string()))?;
    }
    for (owner, procedural) in procedural_curves {
        ir.model
            .add_procedural_curve(owner, procedural)
            .map_err(|error| CodecError::malformed(error.to_string()))?;
    }
    crate::decode_alloc::extend_vec(ctx, &mut ir.model.attributes, attributes, "ASM transfer attributes")?;
    // Every transfer above appends entities; procedural attachment removes none.
    ctx.charge_entities(
        (ir.model.entity_count() - before) as u64,
        "admit ASM entities",
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
}
