// SPDX-License-Identifier: Apache-2.0
//! Walk the typed ASM graph without rebuilding it through serde.

use super::annotations::AnnotationRecord;
use super::{AsmBrep, Stats};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

macro_rules! graph {
    ($owner:ty; $($field:ident),*) => {
        impl RewriteIdentities for $owner {
            fn visit_identity_references(&self, ctx: &DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>) -> Result<(), CodecError> {
                let _depth = ctx.enter_nested("walk ASM graph references")?;
                ctx.charge_work(1, "walk ASM graph references")?;
                $(self.$field.visit_identity_references(ctx, visitor)?;)*
                Ok(())
            }
            fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(mut self, ctx: &DecodeContext<'_>, map: &mut IdentityMap<'_, F>) -> Result<Self, CodecError> {
                let _depth = ctx.enter_nested("rewrite ASM graph fields")?;
                ctx.charge_work(1, "rewrite ASM graph fields")?;
                $(self.$field = self.$field.rewrite_identities(ctx, map)?;)*
                Ok(self)
            }
        }
    };
}
graph!(AsmBrep; bodies, regions, shells, faces, loops, coedges, edges, vertices, points, surfaces, curves, pcurves, procedural_surfaces, procedural_curves, edge_continuities, edge_ownerships, vertex_ownerships, face_sidedness, face_native_keys, tolerant_coedge_parameters, tolerant_edge_tails, tolerant_vertex_tails, mesh_surface_sentinels, transform_hints, body_native_keys, wire_topologies, attributes, unknowns, stats, annotation_records);
graph!(Stats; missing_face_surface_kinds, unknown_surface_kinds, mesh_surface_faces, nurbs_surfaces, nurbs_curves, procedural_curve_kinds, undecoded_pcurve_kinds, partial_procedural_supports, other_record_kinds);

impl RewriteIdentities for AnnotationRecord {
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        ctx.charge_work(1, "walk ASM annotation identity")?;
        visitor(&self.id)
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(1, "rewrite ASM annotation identity")?;
        self.id = self.id.rewrite_identities(ctx, map)?;
        Ok(self)
    }
}
