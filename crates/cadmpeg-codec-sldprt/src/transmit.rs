// SPDX-License-Identifier: Apache-2.0
//! B-rep geometry of one standalone neutral-binary Parasolid transmit.
//!
//! A host that carries Parasolid transmits outside a `SolidWorks` container
//! decodes them through the same B-rep builder as a `SolidWorks` partition.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Topology;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{StreamName, UnknownRecord};

/// The B-rep arenas and losses of one decoded transmit. Identities use the
/// `sldprt:` namespace.
#[derive(Debug, Default)]
pub struct TransmitGeometry {
    /// Bodies, topology, and carriers in millimetres.
    pub geometry: Topology,
    /// Records whose carrier kind the builder does not type.
    pub unknowns: Vec<UnknownRecord>,
    /// Withheld and untyped content, in the `sldprt` loss vocabulary.
    pub losses: Vec<LossNote>,
}

/// Decodes the bodies of one neutral-binary transmit (`parasolid.md` §1).
///
/// `stream` names the provenance stream of the decoded entities. Returns
/// `None` when the transmit header is absent.
///
/// # Errors
///
/// Returns the builder's error when the node sequence does not decode, and a
/// resource refusal when a decode limit is reached.
pub fn decode(
    ctx: &DecodeContext<'_>,
    transmit: &[u8],
    stream: &StreamName,
) -> Result<Option<TransmitGeometry>, CodecError> {
    let Some(header) = crate::parasolid::stream_header(transmit)? else {
        return Ok(None);
    };
    let mut brep = crate::brep::graph::decode_bodies(ctx, &[(transmit, &header)], stream)?;
    let losses = crate::decode::brep_losses(&mut brep);
    let geometry = Topology {
        bodies: brep.bodies,
        regions: brep.regions,
        shells: brep.shells,
        faces: brep.faces,
        loops: brep.loops,
        coedges: brep.coedges,
        edges: brep.edges,
        vertices: brep.vertices,
        points: brep.points,
        surfaces: brep.surfaces,
        procedural_surfaces: brep.procedural_surfaces,
        curves: brep.curves,
        procedural_curves: brep.procedural_curves,
        pcurves: brep.pcurves,
        ..Topology::default()
    };
    Ok(Some(TransmitGeometry {
        geometry,
        unknowns: brep.unknowns,
        losses,
    }))
}
