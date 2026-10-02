// SPDX-License-Identifier: Apache-2.0
//! Typed identity traversal for ASM-native record variants.

use super::{EndpointSlot, EvaluatedToleranceSlot, FaceContainment, FaceSidedness, TolerantCoedgeExtension, WireMembers, WireSide};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

macro_rules! scalar {
    ($($owner:ty),*) => {$(
        impl RewriteIdentities for $owner {
            fn visit_identity_references(&self, ctx: &DecodeContext<'_>, _visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>) -> Result<(), CodecError> { ctx.charge_work(1, "walk ASM scalar reference") }
            fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(self, ctx: &DecodeContext<'_>, _map: &mut IdentityMap<'_, F>) -> Result<Self, CodecError> { ctx.charge_work(1, "rewrite ASM scalar")?; Ok(self) }
        }
    )*};
}
scalar!(EndpointSlot, EvaluatedToleranceSlot, FaceContainment, TolerantCoedgeExtension, WireSide);

impl RewriteIdentities for WireMembers {
    fn visit_identity_references(&self, ctx: &DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk ASM wire members")?;
        ctx.charge_work(1, "walk ASM wire members")?;
        match self { Self::Edges(edges) => edges.visit_identity_references(ctx, visitor), Self::Vertex(vertex) => vertex.visit_identity_references(ctx, visitor) }
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(self, ctx: &DecodeContext<'_>, map: &mut IdentityMap<'_, F>) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("rewrite ASM wire members")?;
        ctx.charge_work(1, "rewrite ASM wire members")?;
        Ok(match self { Self::Edges(edges) => Self::Edges(edges.rewrite_identities(ctx, map)?), Self::Vertex(vertex) => Self::Vertex(vertex.rewrite_identities(ctx, map)?) })
    }
}

impl RewriteIdentities for FaceSidedness {
    fn visit_identity_references(&self, ctx: &DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk ASM face sidedness")?;
        ctx.charge_work(1, "walk ASM face sidedness")?;
        self.source_namespace.visit(ctx, "face-sidedness", self.record_index, visitor)?;
        self.face.visit_identity_references(ctx, visitor)
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(mut self, ctx: &DecodeContext<'_>, map: &mut IdentityMap<'_, F>) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("rewrite ASM face sidedness")?;
        ctx.charge_work(1, "rewrite ASM face sidedness")?;
        self.source_namespace = self.source_namespace.rewrite(ctx, "face-sidedness", self.record_index, map)?;
        self.face = self.face.rewrite_identities(ctx, map)?;
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

    fn record() -> super::super::EdgeContinuity {
        serde_json::from_value(serde_json::json!({"id":"f3d:asm:edge-continuity#1","record_index":1,"edge":"f3d:brep:entity#1","sense":"forward","continuity":"tangent"})).unwrap()
    }

    #[test]
    fn asm_typed_rewrite_preserves_native_record_wire() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let mut map = IdentityMap::new(&ctx, "qualify ASM typed record", |source: &str| ctx.format_retained(format_args!("f3d:brep/source/{}", source.strip_prefix("f3d:").unwrap()), "qualify ASM identity")).unwrap();
        let value = record().rewrite_identities(&ctx, &mut map).unwrap();
        assert_eq!(serde_json::to_value(value).unwrap(), serde_json::json!({"id":"f3d:brep/source/asm:edge-continuity#1","record_index":1,"edge":"f3d:brep/source/brep:entity#1","sense":"forward","continuity":"tangent"}));
        drop(map);
        ctx.finish_session().unwrap();
    }

    #[test]
    fn asm_typed_rewrite_preserves_every_resource_refusal() {
        for dimension in [ResourceDimension::RetainedBytes, ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems, ResourceDimension::WorkUnits, ResourceDimension::RecursionDepth] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
                _ => panic!("ASM rewrite dimensions"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut map = IdentityMap::new(&ctx, "qualify ASM typed record", |source: &str| ctx.copy_retained_text(source, "qualify ASM identity")).unwrap();
            let CodecError::ResourceLimit(limit) = record().rewrite_identities(&ctx, &mut map).unwrap_err() else { panic!("ASM rewrite must refuse"); };
            assert_eq!(limit.dimension, dimension);
            drop(map);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit));
        }
    }

    #[test]
    fn asm_typed_rewrite_refuses_record_kind_or_index_changes() {
        for target in ["f3d:asm:edge-continuity#2", "f3d:asm:other-kind#1", "f3d:asm:edge-continuity#01"] {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
            let mut map = IdentityMap::new(&ctx, "qualify ASM typed record", |_source: &str| ctx.copy_retained_text(target, "qualify ASM identity")).unwrap();
            let error = record().rewrite_identities(&ctx, &mut map).unwrap_err();
            assert!(matches!(error, CodecError::Malformed(message) if message == "ASM identity rewrite must preserve record kind and index"));
        }
    }
}
