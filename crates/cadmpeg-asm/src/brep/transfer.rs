// SPDX-License-Identifier: Apache-2.0
//! Exhaustive transfer from an ASM graph into neutral and native IR arenas.

use std::collections::HashMap;
use std::num::NonZeroU32;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{BodyId, FaceId};
use cadmpeg_ir::unknown::UnknownRecord;

use super::{AnnotationRecord, AsmBrep, Stats};

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
    /// Native body join keys used by an embedding format's semantic tables.
    pub body_keys: HashMap<BodyId, u64>,
    /// Native face join keys used by an embedding format's semantic tables.
    pub face_keys: HashMap<FaceId, u64>,
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
pub fn transfer_into_ir(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    native_format: &str,
    native_version: NonZeroU32,
    brep: AsmBrep,
) -> Result<AsmTransferRemainder, CodecError> {
    if ir.native.namespace(native_format).is_some_and(|namespace| {
        ASM_NATIVE_ARENAS.iter().any(|name| {
            namespace
                .arenas
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
        face_keys,
        face_native_keys,
        tolerant_coedge_parameters,
        tolerant_edge_tails,
        tolerant_vertex_tails,
        mesh_surface_sentinels,
        transform_hints,
        body_keys,
        body_native_keys,
        wire_topologies,
        attributes,
        unknowns,
        stats,
        annotation_records,
    } = brep;

    let before = ir.model.entity_count();
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
    ir.model.procedural_surfaces.extend(procedural_surfaces);
    ir.model.procedural_curves.extend(procedural_curves);
    ir.model.attributes.extend(attributes);
    ctx.charge_entities(
        ir.model.entity_count().saturating_sub(before) as u64,
        "admit ASM entities",
    )?;

    let namespace = ir.native.namespace_mut(native_format);
    namespace.set_version(native_version);
    namespace.set_arena("edge_continuities", &edge_continuities)?;
    namespace.set_arena("edge_ownerships", &edge_ownerships)?;
    namespace.set_arena("vertex_ownerships", &vertex_ownerships)?;
    namespace.set_arena("face_sidedness", &face_sidedness)?;
    namespace.set_arena("face_native_keys", &face_native_keys)?;
    namespace.set_arena("tolerant_vertex_tails", &tolerant_vertex_tails)?;
    namespace.set_arena("tolerant_edge_tails", &tolerant_edge_tails)?;
    namespace.set_arena("tolerant_coedge_parameters", &tolerant_coedge_parameters)?;
    namespace.set_arena("mesh_surface_sentinels", &mesh_surface_sentinels)?;
    namespace.set_arena("wire_topologies", &wire_topologies)?;
    namespace.set_arena("transform_hints", &transform_hints)?;
    namespace.set_arena("body_native_keys", &body_native_keys)?;

    Ok(AsmTransferRemainder {
        body_keys,
        face_keys,
        unknowns,
        stats,
        annotation_records,
    })
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    use cadmpeg_ir::units::Units;

    use super::*;

    #[test]
    fn empty_transfer_still_declares_every_asm_native_arena() {
        let source = [0_u8];
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::default())
            .expect("test root fits policy");
        let mut ir = CadIr::empty(Units::default());
        let remainder = transfer_into_ir(
            &ctx,
            &mut ir,
            "test",
            std::num::NonZeroU32::new(7).expect("ASM native version is nonzero"),
            AsmBrep::default(),
        )
        .expect("empty ASM transfer succeeds");
        assert!(remainder.body_keys.is_empty());
        assert!(remainder.face_keys.is_empty());
        assert!(remainder.unknowns.is_empty());
        assert!(remainder.annotation_records.is_empty());
        let namespace = ir.native.namespace("test").expect("namespace exists");
        assert_eq!(namespace.version(), 7);
        assert_eq!(namespace.arenas.len(), 12);
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
        let mut ir = CadIr::empty(Units::default());
        ir.native
            .namespace_mut("test")
            .set_arena("body_native_keys", &[HeldRecord { id: "held".into() }])
            .expect("test native record serializes");
        assert!(transfer_into_ir(
            &ctx,
            &mut ir,
            "test",
            std::num::NonZeroU32::new(7).expect("ASM native version is nonzero"),
            AsmBrep::default()
        )
        .is_err());
        let held: Vec<HeldRecord> = ir
            .native
            .namespace("test")
            .expect("namespace remains present")
            .arena_as("body_native_keys")
            .expect("held record remains readable");
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].id, "held");
    }
}
