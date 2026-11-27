// SPDX-License-Identifier: Apache-2.0
//! The decode of one stored state of a partition site: the partition
//! transmit that the replay of the site's Deltas stream writes for a mark
//! (`docs/formats/sldprt.md` §3.4), decoded as the site's
//! current state is.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::document::Topology;
use cadmpeg_ir::ids::{BodyId, FaceId};

use crate::brep::attrib::NativeIdentity;
use crate::brep::graph::decode_bodies;
use crate::container::Section;

/// The records of one stored state.
pub(crate) struct DecodedState {
    pub(crate) geometry: Topology,
    pub(crate) bodies: Vec<BodyId>,
    pub(crate) faces: Vec<NativeIdentity<FaceId>>,
    pub(crate) body_identities: Vec<NativeIdentity<BodyId>>,
    /// The exactness of the records. A replayed transmit is no file
    /// location, so the records have no provenance.
    pub(crate) annotations: Annotations,
}

/// Decode `transmit`, a state of the partition site `section`, with the ids
/// of the site's current records: qualified by `qualifier` where the site is
/// not the active one. A transmit that does not decode, or whose decode
/// drops a topology record, gives the reason.
pub(crate) fn decode_state(
    ctx: &DecodeContext<'_>,
    section: Section<'_>,
    qualifier: Option<&str>,
    transmit: &[u8],
) -> Result<Result<DecodedState, String>, CodecError> {
    let header = match crate::parasolid::stream_header(transmit) {
        Ok(Some(header)) => header,
        Ok(None) => return Ok(Err("the state transmit has no header".to_owned())),
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(error) => return Ok(Err(error.to_string())),
    };
    let mut brep = match decode_bodies(ctx, &[(transmit, &header)], section.source_stream()) {
        Ok(brep) => brep,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(error) => return Ok(Err(error.to_string())),
    };
    if !brep.dropped.is_empty() {
        return Ok(Err(format!(
            "the decode of the state drops {} topology record(s)",
            brep.dropped.len()
        )));
    }
    if let Some(site) = qualifier {
        brep.qualify_ids(site)?;
    }
    super::bind_opaque_geometry(&mut brep, &section.native_id());
    let mut annotations = std::mem::take(&mut brep.annotations);
    annotations.retain_provenance(|_| false);
    Ok(Ok(DecodedState {
        bodies: brep.bodies.iter().map(|body| body.id.clone()).collect(),
        faces: std::mem::take(&mut brep.native_face_identities),
        body_identities: std::mem::take(&mut brep.native_body_identities),
        annotations,
        geometry: Topology {
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
        },
    }))
}
