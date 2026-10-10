// SPDX-License-Identifier: Apache-2.0
//! Typed identity traversal for ASM-native record variants.

use super::{
    EndpointSlot, EvaluatedToleranceSlot, FaceContainment, FaceSidedness, TolerantCoedgeExtension,
    WireMembers, WireSide,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

macro_rules! scalar {
    ($($owner:ty),*) => {$(
        impl RewriteIdentities for $owner {
            fn visit_identity_references(&self, ctx: &DecodeContext<'_>, _visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>) -> Result<(), CodecError> {
                if let Some(refusal) = ctx.resource_refusal() {
                    return Err(refusal.into());
                }
                Ok(())
            }
            fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(self, ctx: &DecodeContext<'_>, _map: &mut IdentityMap<'_, F>) -> Result<Self, CodecError> {
                if let Some(refusal) = ctx.resource_refusal() {
                    return Err(refusal.into());
                }
                Ok(self)
            }
        }
    )*};
}
scalar!(
    EndpointSlot,
    EvaluatedToleranceSlot,
    FaceContainment,
    TolerantCoedgeExtension,
    WireSide
);

impl RewriteIdentities for WireMembers {
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk ASM wire members")?;
        ctx.charge_work(1, "walk ASM wire members")?;
        match self {
            Self::Edges(edges) => edges.visit_identity_references(ctx, visitor),
            Self::Vertex(vertex) => vertex.visit_identity_references(ctx, visitor),
        }
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("rewrite ASM wire members")?;
        ctx.charge_work(1, "rewrite ASM wire members")?;
        Ok(match self {
            Self::Edges(edges) => Self::Edges(edges.rewrite_identities(ctx, map)?),
            Self::Vertex(vertex) => Self::Vertex(vertex.rewrite_identities(ctx, map)?),
        })
    }
}

impl RewriteIdentities for FaceSidedness {
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk ASM face sidedness")?;
        ctx.charge_work(1, "walk ASM face sidedness")?;
        self.source_namespace
            .visit(ctx, "face-sidedness", self.record_index, visitor)?;
        self.face.visit_identity_references(ctx, visitor)
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("rewrite ASM face sidedness")?;
        ctx.charge_work(1, "rewrite ASM face sidedness")?;
        self.source_namespace =
            self.source_namespace
                .rewrite(ctx, "face-sidedness", self.record_index, map)?;
        self.face = self.face.rewrite_identities(ctx, map)?;
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EndpointSlot, EvaluatedToleranceSlot, FaceContainment, TolerantCoedgeExtension, WireSide,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

    macro_rules! scalar_cases {
        ($check:ident) => {
            for value in [EndpointSlot::Start, EndpointSlot::End] {
                $check(&value);
            }
            for value in [FaceContainment::In, FaceContainment::Out] {
                $check(&value);
            }
            for value in [WireSide::In, WireSide::Out] {
                $check(&value);
            }
            for value in [
                EvaluatedToleranceSlot::Absent {},
                EvaluatedToleranceSlot::Unset { trailing: None },
                EvaluatedToleranceSlot::Unset {
                    trailing: Some(i64::MAX),
                },
                EvaluatedToleranceSlot::Evaluated { trailing: None },
                EvaluatedToleranceSlot::Evaluated {
                    trailing: Some(i64::MAX),
                },
            ] {
                $check(&value);
            }
            for value in [
                TolerantCoedgeExtension::None {},
                TolerantCoedgeExtension::Reference { target: None },
                TolerantCoedgeExtension::Reference {
                    target: Some(i64::MAX),
                },
                TolerantCoedgeExtension::Empty { target: None },
                TolerantCoedgeExtension::Empty {
                    target: Some(i64::MAX),
                },
                TolerantCoedgeExtension::EmbeddedCurve {
                    target: None,
                    curve_reversed: false,
                    payload_token_count: 0,
                    parameter_range: None,
                },
                TolerantCoedgeExtension::EmbeddedCurve {
                    target: Some(i64::MAX),
                    curve_reversed: true,
                    payload_token_count: u32::MAX,
                    parameter_range: None,
                },
            ] {
                $check(&value);
            }
        };
    }

    fn zero_policy() -> DecodePolicy {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        policy
    }

    fn check_free_scalar<T: RewriteIdentities + Clone + serde::Serialize>(value: &T) {
        let wire = serde_json::to_value(value).unwrap();
        for count in [1, 64] {
            let arena = DecodeArena::new();
            let policy = zero_policy();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut map = IdentityMap::new(&ctx, "test unused ASM scalar map", |_source: &str| {
                panic!("a fixed scalar must not map an identity");
            })
            .unwrap();
            for _ in 0..count {
                value
                    .visit_identity_references(&ctx, &mut |_source| {
                        panic!("a fixed scalar must not visit an identity");
                    })
                    .unwrap();
                let rewritten = value.clone().rewrite_identities(&ctx, &mut map).unwrap();
                assert_eq!(serde_json::to_value(rewritten).unwrap(), wire);
            }
            drop(map);
            ctx.finish_session().unwrap();
        }
    }

    #[test]
    fn fixed_asm_scalars_visit_and_rewrite_with_zero_budgets() {
        scalar_cases!(check_free_scalar);
    }

    fn check_fused_scalar<T: RewriteIdentities + Clone + serde::Serialize>(value: &T) {
        let wire = serde_json::to_value(value).unwrap();
        for dimension in [
            ResourceDimension::WorkUnits,
            ResourceDimension::CollectionItems,
            ResourceDimension::MaterializedBytes,
            ResourceDimension::RetainedBytes,
            ResourceDimension::Entities,
            ResourceDimension::RecursionDepth,
        ] {
            let arena = DecodeArena::new();
            let policy = zero_policy();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut map = IdentityMap::new(&ctx, "test unused ASM scalar map", |_source: &str| {
                panic!("a refused scalar must not map an identity");
            })
            .unwrap();
            let refused = match dimension {
                ResourceDimension::WorkUnits => {
                    ctx.charge_work(1, "test original ASM scalar refusal")
                }
                ResourceDimension::CollectionItems => {
                    ctx.charge_collection_items(1, "test original ASM scalar refusal")
                }
                ResourceDimension::MaterializedBytes => ctx
                    .reserve_scoped(1, "test original ASM scalar refusal")
                    .map(|_| ()),
                ResourceDimension::RetainedBytes => {
                    ctx.charge_retained(1, "test original ASM scalar refusal")
                }
                ResourceDimension::Entities => {
                    ctx.charge_entities(1, "test original ASM scalar refusal")
                }
                ResourceDimension::RecursionDepth => ctx
                    .enter_nested("test original ASM scalar refusal")
                    .map(|_| ()),
                _ => panic!("scalar refusal dimension"),
            };
            let Err(CodecError::ResourceLimit(first)) = refused else {
                panic!("expected original refusal");
            };
            assert_eq!(first.dimension, dimension);
            for _ in 0..64 {
                assert!(
                    matches!(value.visit_identity_references(&ctx, &mut |_source| {
                    panic!("a refused scalar must not visit an identity");
                }), Err(CodecError::ResourceLimit(last)) if last == first)
                );
                assert!(matches!(value.clone().rewrite_identities(&ctx, &mut map),
                    Err(CodecError::ResourceLimit(last)) if last == first));
                assert_eq!(serde_json::to_value(value).unwrap(), wire);
            }
            drop(map);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)
            );
        }
    }

    #[test]
    fn fixed_asm_scalars_replay_each_original_resource_refusal() {
        scalar_cases!(check_fused_scalar);
    }

    fn record() -> super::super::EdgeContinuity {
        serde_json::from_value(serde_json::json!({"id":"f3d:asm:edge-continuity#1","record_index":1,"edge":"f3d:brep:entity#1","sense":"forward","continuity":"tangent"})).unwrap()
    }

    #[test]
    fn asm_typed_rewrite_preserves_native_record_wire() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let mut map = IdentityMap::new(&ctx, "qualify ASM typed record", |source: &str| {
            ctx.format_retained(
                format_args!("f3d:brep/source/{}", source.strip_prefix("f3d:").unwrap()),
                "qualify ASM identity",
            )
        })
        .unwrap();
        let value = record().rewrite_identities(&ctx, &mut map).unwrap();
        assert_eq!(
            serde_json::to_value(value).unwrap(),
            serde_json::json!({"id":"f3d:brep/source/asm:edge-continuity#1","record_index":1,"edge":"f3d:brep/source/brep:entity#1","sense":"forward","continuity":"tangent"})
        );
        drop(map);
        ctx.finish_session().unwrap();
    }

    #[test]
    fn asm_typed_rewrite_preserves_every_resource_refusal() {
        for dimension in [
            ResourceDimension::RetainedBytes,
            ResourceDimension::MaterializedBytes,
            ResourceDimension::CollectionItems,
            ResourceDimension::WorkUnits,
            ResourceDimension::RecursionDepth,
        ] {
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
            let mut map = IdentityMap::new(&ctx, "qualify ASM typed record", |source: &str| {
                ctx.copy_retained_text(source, "qualify ASM identity")
            })
            .unwrap();
            let CodecError::ResourceLimit(limit) =
                record().rewrite_identities(&ctx, &mut map).unwrap_err()
            else {
                panic!("ASM rewrite must refuse");
            };
            assert_eq!(limit.dimension, dimension);
            drop(map);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit)
            );
        }
    }

    #[test]
    fn asm_typed_rewrite_refuses_record_kind_or_index_changes() {
        for target in [
            "f3d:asm:edge-continuity#2",
            "f3d:asm:other-kind#1",
            "f3d:asm:edge-continuity#01",
        ] {
            let arena = DecodeArena::new();
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
            let mut map = IdentityMap::new(&ctx, "qualify ASM typed record", |_source: &str| {
                ctx.copy_retained_text(target, "qualify ASM identity")
            })
            .unwrap();
            let error = record().rewrite_identities(&ctx, &mut map).unwrap_err();
            assert!(
                matches!(error, CodecError::Malformed(message) if message == "ASM identity rewrite must preserve record kind and index")
            );
        }
    }
}
