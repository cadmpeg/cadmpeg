// SPDX-License-Identifier: Apache-2.0
//! The support surface of each pcurve in a topology.

use std::collections::HashMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Topology;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::ids::{FaceId, LoopId, PcurveId, SurfaceId};
use cadmpeg_ir::topology::Loop;

/// The uses of each pcurve in one topology, collected in one pass. A pcurve's
/// support is the surface of the faces whose coedges and pole vertices use it.
pub(in crate::writer) struct PcurveSupports<'a> {
    /// Per pcurve, the loop of each use: coedge uses in coedge order, then
    /// pole-vertex uses in loop order. A coedge whose owner loop is missing
    /// gives that loop id.
    uses: HashMap<&'a PcurveId, Vec<Result<&'a Loop, &'a LoopId>>>,
    face_surfaces: HashMap<&'a FaceId, &'a SurfaceId>,
    surfaces: HashMap<&'a SurfaceId, &'a SurfaceGeometry>,
}

impl<'a> PcurveSupports<'a> {
    /// Index `geometry`. A repeated id resolves to its first arena entry.
    pub(in crate::writer) fn new(geometry: &'a Topology) -> Self {
        let mut loops = HashMap::new();
        for loop_ in &geometry.loops {
            loops.entry(&loop_.id).or_insert(loop_);
        }
        let mut uses: HashMap<_, Vec<_>> = HashMap::new();
        for coedge in &geometry.coedges {
            let owner = loops
                .get(&coedge.owner_loop)
                .copied()
                .ok_or(&coedge.owner_loop);
            for use_ in &coedge.pcurves {
                uses.entry(&use_.pcurve).or_default().push(owner);
            }
        }
        for loop_ in &geometry.loops {
            for use_ in loop_.vertex_pcurves() {
                uses.entry(&use_.pcurve).or_default().push(Ok(loop_));
            }
        }
        let mut face_surfaces = HashMap::new();
        for face in &geometry.faces {
            face_surfaces.entry(&face.id).or_insert(&face.surface);
        }
        let mut surfaces = HashMap::new();
        for surface in &geometry.surfaces {
            surfaces.entry(&surface.id).or_insert(&surface.geometry);
        }
        Self {
            uses,
            face_surfaces,
            surfaces,
        }
    }

    /// The surface of the faces that use `pcurve_id`.
    pub(in crate::writer) fn surface(
        &self,
        pcurve_id: &PcurveId,
    ) -> Result<&'a SurfaceId, CodecError> {
        let mut selected: Option<&'a SurfaceId> = None;
        for &owner in self.uses.get(pcurve_id).into_iter().flatten() {
            let loop_ = owner.map_err(|owner_loop| {
                CodecError::malformed(format_args!(
                    "pcurve {pcurve_id} is used by missing loop {owner_loop}"
                ))
            })?;
            let surface = self
                .face_surfaces
                .get(&loop_.face)
                .copied()
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "pcurve {pcurve_id} is used by loop {} with missing face {}",
                        loop_.id, loop_.face
                    ))
                })?;
            if selected.is_some_and(|selected| selected != surface) {
                return Err(CodecError::NotImplemented(format!(
                    "F3D pcurve {pcurve_id} is shared by faces with different support surfaces"
                )));
            }
            selected.get_or_insert(surface);
        }
        selected.ok_or_else(|| {
            CodecError::malformed(format_args!(
                "F3D pcurve {pcurve_id} is not used by a coedge or pole vertex"
            ))
        })
    }

    /// The geometry of the surface of the faces that use `pcurve_id`.
    pub(in crate::writer) fn geometry(
        &self,
        pcurve_id: &PcurveId,
    ) -> Result<&'a SurfaceGeometry, CodecError> {
        let surface_id = self.surface(pcurve_id)?;
        self.surfaces.get(surface_id).copied().ok_or_else(|| {
            CodecError::malformed(format_args!(
                "F3D pcurve {pcurve_id} references missing support {surface_id}"
            ))
        })
    }
}
