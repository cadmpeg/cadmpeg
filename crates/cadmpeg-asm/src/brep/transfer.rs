// SPDX-License-Identifier: Apache-2.0
//! Exhaustive transfer from an ASM graph into neutral and native IR arenas.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScratchContext};
use cadmpeg_core::text::CodecId;
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::SourceAttribute;
use cadmpeg_ir::document::{CadIr, Topology};
use cadmpeg_ir::native::NativeNamespace;
use cadmpeg_ir::unknown::UnknownRecord;

use super::annotations::AnnotationRecord;
use super::records::BodyNativeKey;
use super::stats::Stats;
use super::{AsmBrep, UnboundedFace};

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
    /// Faces stated partial because their record states no loop on an
    /// unbounded surface; the embedding codec charges one loss for each.
    pub unbounded_faces: Vec<UnboundedFace>,
    /// Records the model decode read and left out; the embedding codec
    /// gives each up as a part.
    pub withheld: Vec<super::given_up::WithheldRecord>,
}

/// Neutral geometry and source links available to an inspecting codec.
pub struct InspectionGeometry {
    /// The graph's neutral geometry.
    pub geometry: Topology,
    /// The graph's typed source attributes.
    pub attributes: Vec<SourceAttribute>,
    /// Native keys of the graph's bodies.
    pub body_keys: Vec<BodyNativeKey>,
}

/// Moves an inspection graph into neutral geometry without native arenas.
pub fn transfer_inspection(
    ctx: &ScratchContext<'_>,
    mut brep: AsmBrep,
) -> Result<InspectionGeometry, CodecError> {
    let mut geometry = Topology::default();
    transfer_geometry(ctx, &mut geometry, &mut brep)?;
    Ok(InspectionGeometry {
        geometry,
        attributes: brep.attributes,
        body_keys: brep.body_native_keys,
    })
}

/// Moves one complete ASM graph into the IR and serializes every ASM-native arena.
///
/// The exhaustive [`AsmBrep`] destructure makes a newly added decoder field a
/// compile error until this boundary assigns its disposition.
pub fn transfer_into_ir<'ir>(
    ctx: &DecodeContext<'_>,
    ir: &'ir mut CadIr,
    native_format: CodecId,
    mut brep: AsmBrep,
) -> Result<(&'ir NativeNamespace, AsmTransferRemainder), CodecError> {
    if ir
        .native
        .namespace(native_format.as_str())
        .is_some_and(|namespace| {
            ASM_NATIVE_ARENAS.iter().any(|name| {
                namespace
                    .arenas()
                    .get(*name)
                    .is_some_and(|records| !records.is_empty())
            })
        })
    {
        return Err(CodecError::malformed(format_args!(
            "native namespace {native_format} already contains ASM records"
        )));
    }

    transfer_geometry(ctx, &mut ir.geometry, &mut brep)?;
    let AsmBrep {
        bodies: _,
        regions: _,
        shells: _,
        faces: _,
        loops: _,
        coedges: _,
        edges: _,
        vertices: _,
        points: _,
        surfaces: _,
        curves: _,
        pcurves: _,
        procedural_surfaces: _,
        procedural_curves: _,
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
        unbounded_faces,
        chain_order: _,
        withheld,
    } = brep;
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
            unbounded_faces,
            withheld,
        },
    ))
}

fn transfer_geometry(
    ctx: &DecodeContext<'_>,
    geometry: &mut Topology,
    brep: &mut AsmBrep,
) -> Result<(), CodecError> {
    fn move_arena<T>(to: &mut Vec<T>, from: &mut Vec<T>) {
        let records = std::mem::take(from);
        if to.is_empty() {
            *to = records;
        } else {
            to.extend(records);
        }
    }

    super::subsets::localize_subsets(
        &mut brep.curves,
        &mut brep.edges,
        &mut brep.coedges,
        &brep.procedural_curves,
    )?;
    let entity_count = [
        brep.bodies.len(),
        brep.regions.len(),
        brep.shells.len(),
        brep.faces.len(),
        brep.loops.len(),
        brep.coedges.len(),
        brep.edges.len(),
        brep.vertices.len(),
        brep.points.len(),
        brep.surfaces.len(),
        brep.curves.len(),
        brep.pcurves.len(),
        brep.procedural_surfaces.len(),
        brep.procedural_curves.len(),
        brep.attributes.len(),
    ]
    .into_iter()
    .try_fold(0_u64, |total, count| {
        total
            .checked_add(u64_from_index(count))
            .ok_or_else(|| CodecError::malformed("ASM entity count overflows u64"))
    })?;
    ctx.charge_entities(entity_count, "admit ASM entities")?;
    move_arena(&mut geometry.bodies, &mut brep.bodies);
    move_arena(&mut geometry.regions, &mut brep.regions);
    move_arena(&mut geometry.shells, &mut brep.shells);
    move_arena(&mut geometry.faces, &mut brep.faces);
    move_arena(&mut geometry.loops, &mut brep.loops);
    move_arena(&mut geometry.coedges, &mut brep.coedges);
    move_arena(&mut geometry.edges, &mut brep.edges);
    move_arena(&mut geometry.vertices, &mut brep.vertices);
    move_arena(&mut geometry.points, &mut brep.points);
    move_arena(&mut geometry.surfaces, &mut brep.surfaces);
    move_arena(&mut geometry.curves, &mut brep.curves);
    move_arena(&mut geometry.pcurves, &mut brep.pcurves);
    geometry
        .add_procedural_surfaces(std::mem::take(&mut brep.procedural_surfaces))?
        .map_err(CodecError::malformed)?;
    geometry
        .add_procedural_curves(std::mem::take(&mut brep.procedural_curves))?
        .map_err(CodecError::malformed)?;
    Ok(())
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
        let (namespace, remainder) = transfer_into_ir(
            &ctx,
            &mut ir,
            cadmpeg_ir::codec_id!("test"),
            AsmBrep::default(),
        )
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
        brep.regions.push(Region::new(
            RegionId::mint("sat:brep:region#1").unwrap(),
            BodyId::mint("sat:brep:body#1").unwrap(),
            Vec::new(),
        ));
        let mut ir = CadIr::empty();
        let error = transfer_into_ir(&ctx, &mut ir, cadmpeg_ir::codec_id!("test"), brep)
            .err()
            .expect("one entity exceeds zero entities");
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected entity refusal: {error:?}");
        };
        assert_eq!(limit.dimension, ResourceDimension::Entities);
        assert_eq!(limit.operation, "admit ASM entities");
        assert_eq!(ir.entity_count(), 0);
        assert!(ir.geometry.regions.is_empty());
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
            .namespace_mut(cadmpeg_ir::codec_id!("test"))
            .set_arena(
                &ctx,
                "body_native_keys",
                &[HeldRecord {
                    id: "sat:test:held#0".into(),
                }],
            )
            .expect("test native record serializes");
        assert!(transfer_into_ir(
            &ctx,
            &mut ir,
            cadmpeg_ir::codec_id!("test"),
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
        assert_eq!(held[0].id, "sat:test:held#0");
    }
}
