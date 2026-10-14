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
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("ASM entity count overflows u64"))
    })?;
    ctx.charge_entities(entity_count, "admit ASM entities")?;
    ir.model.bodies.extend(bodies);
    ir.model.regions.extend(regions);
    ir.model.shells.extend(shells);
    ir.model.faces.extend(faces);
    ir.model.loops.extend(loops);
    ir.model.coedges.extend(coedges);
    ir.model.edges.extend(edges);
    ir.model.vertices.extend(vertices);
    ir.model.points.extend(points);
    ir.model.surfaces.extend(surfaces);
    ir.model.curves.extend(curves);
    ir.model.pcurves.extend(pcurves);
    ir.model
        .add_procedural_surfaces(ctx, procedural_surfaces)?
        .map_err(CodecError::malformed)?;
    ir.model
        .add_procedural_curves(ctx, procedural_curves)?
        .map_err(CodecError::malformed)?;
    ir.model.attributes.extend(attributes);

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
}
