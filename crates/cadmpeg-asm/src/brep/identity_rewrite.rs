// SPDX-License-Identifier: Apache-2.0
//! Walk the typed ASM graph without rebuilding it through serde.

use super::annotations::AnnotationRecord;
use super::given_up::WithheldRecord;
use super::{AsmBrep, Stats, UnboundedFace};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

macro_rules! graph {
    ($owner:ty; $($field:ident),*) => {
        impl RewriteIdentities for $owner {
            fn visit_identity_references(&self, ctx: &DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>) -> Result<(), CodecError> {
                let _depth = ctx.enter_nested("walk ASM graph references")?;
                $(self.$field.visit_identity_references(ctx, visitor)?;)*
                Ok(())
            }
            fn rewrite_identities(mut self, ctx: &DecodeContext<'_>, map: &mut IdentityMap<'_>) -> Result<Self, CodecError> {
                let _depth = ctx.enter_nested("rewrite ASM graph fields")?;
                $(self.$field = self.$field.rewrite_identities(ctx, map)?;)*
                Ok(self)
            }
        }
    };
}
graph!(AsmBrep; bodies, regions, shells, faces, loops, coedges, edges, vertices, points, surfaces, curves, pcurves, procedural_surfaces, procedural_curves, edge_continuities, edge_ownerships, vertex_ownerships, face_sidedness, face_native_keys, tolerant_coedge_parameters, tolerant_edge_tails, tolerant_vertex_tails, mesh_surface_sentinels, transform_hints, body_native_keys, wire_topologies, attributes, unknowns, stats, annotation_records, unbounded_faces, withheld);
graph!(Stats; missing_face_surface_kinds, unknown_surface_kinds, mesh_surface_faces, nurbs_surfaces, nurbs_curves, procedural_curve_kinds, undecoded_pcurve_kinds, partial_procedural_supports, helix_axes_assumed, other_record_kinds);

impl RewriteIdentities for UnboundedFace {
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        self.face.visit_identity_references(ctx, visitor)
    }
    fn rewrite_identities(
        mut self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_>,
    ) -> Result<Self, CodecError> {
        self.face = self.face.rewrite_identities(ctx, map)?;
        Ok(self)
    }
}

impl RewriteIdentities for AnnotationRecord {
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        self.id.visit_identity_references(ctx, visitor)
    }
    fn rewrite_identities(
        mut self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_>,
    ) -> Result<Self, CodecError> {
        self.id = self.id.rewrite_identities(ctx, map)?;
        Ok(self)
    }
}

impl RewriteIdentities for WithheldRecord {
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        self.dropped.visit_identity_references(ctx, visitor)?;
        self.marked.visit_identity_references(ctx, visitor)
    }
    fn rewrite_identities(
        mut self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_>,
    ) -> Result<Self, CodecError> {
        self.dropped = self.dropped.rewrite_identities(ctx, map)?;
        self.marked = self.marked.rewrite_identities(ctx, map)?;
        Ok(self)
    }
}
