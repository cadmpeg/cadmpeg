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
            fn visit_identity_references(&self, _ctx: &DecodeContext<'_>, _visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>) -> Result<(), CodecError> { Ok(()) }
            fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(self, _ctx: &DecodeContext<'_>, _map: &mut IdentityMap<'_, F>) -> Result<Self, CodecError> { Ok(self) }
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

        Ok(match self {
            Self::Edges(edges) => Self::Edges(edges.rewrite_identities(ctx, map)?),
            Self::Vertex(vertex) => Self::Vertex(vertex.rewrite_identities(ctx, map)?),
        })
    }
}

impl RewriteIdentities for FaceSidedness {
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        rewrite_face_native_value(ctx, value, map, "face-sidedness", true)
    }

    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk ASM face sidedness")?;

        self.source_namespace
            .visit("face-sidedness", self.record_index, visitor)?;
        if let Some(reference) = &self.face_key {
            visitor(reference)?;
        }
        Ok(())
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("rewrite ASM face sidedness")?;

        self.source_namespace =
            self.source_namespace
                .rewrite("face-sidedness", self.record_index, map)?;
        self.face_key = self
            .face_key
            .as_deref()
            .map(|reference| map.identity(reference))
            .transpose()?;
        Ok(self)
    }
}

pub(super) fn rewrite_face_key_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
    ctx: &DecodeContext<'_>,
    value: &mut serde_json::Value,
    map: &mut IdentityMap<'_, F>,
) -> Result<(), CodecError> {
    rewrite_face_native_value(ctx, value, map, "face-native-key", false)
}

fn rewrite_face_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
    ctx: &DecodeContext<'_>,
    value: &mut serde_json::Value,
    map: &mut IdentityMap<'_, F>,
    kind: &str,
    has_face_key: bool,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("rewrite ASM face native identity wire")?;
    let serde_json::Value::Object(fields) = value else {
        return Err(CodecError::malformed(
            "ASM face native record must be an object",
        ));
    };
    let index = fields
        .get("record_index")
        .and_then(serde_json::Value::as_u64)
        .and_then(|index| u32::try_from(index).ok())
        .ok_or_else(|| CodecError::malformed("ASM face record_index must be a u32"))?;
    let source = fields
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| CodecError::malformed("ASM face native identity must be text"))?;
    super::identity::NativeRecordNamespace::from_wire(source, index, kind)
        .map_err(CodecError::malformed)?;
    let source_key = if has_face_key {
        match fields.get("face_key") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(reference)) => Some(reference.as_str()),
            Some(_) => {
                return Err(CodecError::malformed(
                    "ASM native face_key must be optional text",
                ))
            }
        }
    } else {
        None
    };
    let target = map.identity(source)?;
    super::identity::NativeRecordNamespace::from_wire(&target, index, kind)
        .map_err(CodecError::malformed)?;
    let target_key = source_key
        .map(|reference| map.identity(reference))
        .transpose()?;
    *fields
        .get_mut("id")
        .ok_or_else(|| CodecError::malformed("ASM face identity slot is missing"))? =
        serde_json::Value::String(target);
    if let Some(target_key) = target_key {
        *fields
            .get_mut("face_key")
            .ok_or_else(|| CodecError::malformed("ASM native face_key slot is missing"))? =
            serde_json::Value::String(target_key);
    }
    Ok(())
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
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let mut map = IdentityMap::new(|source: &str| {
            Ok(format!(
                "f3d:brep/source/{}",
                source.strip_prefix("f3d:").unwrap()
            ))
        });
        let value = record().rewrite_identities(&ctx, &mut map).unwrap();
        assert_eq!(
            serde_json::to_value(value).unwrap(),
            serde_json::json!({"id":"f3d:brep/source/asm:edge-continuity#1","record_index":1,"edge":"f3d:brep/source/brep:entity#1","sense":"forward","continuity":"tangent"})
        );
        drop(map);
        ctx.finish_session().unwrap();
    }

    #[test]
    fn asm_typed_rewrite_preserves_the_depth_refusal() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut map = IdentityMap::new(|source: &str| Ok(source.to_owned()));
        let CodecError::ResourceLimit(limit) =
            record().rewrite_identities(&ctx, &mut map).unwrap_err()
        else {
            panic!("ASM rewrite must refuse");
        };
        assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
        drop(map);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit)
        );
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
            let mut map = IdentityMap::new(|_source: &str| Ok(target.to_owned()));
            let error = record().rewrite_identities(&ctx, &mut map).unwrap_err();
            assert!(
                matches!(error, CodecError::Malformed(message) if message == "ASM identity rewrite must preserve record kind and index")
            );
        }
    }
    #[test]
    fn face_key_reference_is_visited_and_rewritten_as_identity_without_a_text_index() {
        use super::super::{identity::NativeRecordNamespace, FaceSidedness};
        use cadmpeg_ir::topology::Sense;
        let side = FaceSidedness {
            source_namespace: NativeRecordNamespace::new(crate::asm_format!("f3d")),
            record_index: 4,
            face_key: Some("f3d:asm:face-native-key#4".into()),
            native_sense: Sense::Reversed,
            carrier_flipped: true,
            containment: None,
        };
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut visited = Vec::new();
        side.visit_identity_references(&ctx, &mut |reference| {
            visited.push(reference.to_owned());
            Ok(())
        })
        .unwrap();
        assert_eq!(
            visited,
            ["f3d:asm:face-sidedness#4", "f3d:asm:face-native-key#4"]
        );
        let mut map = IdentityMap::new(|source: &str| {
            Ok(match source {
                "f3d:asm:face-sidedness#4" => "f3d:side-scope/asm:face-sidedness#4",
                "f3d:asm:face-native-key#4" => "f3d:key-scope/asm:face-native-key#4",
                _ => panic!("only declared native identities are rewritten"),
            }
            .to_owned())
        });
        let side = side.rewrite_identities(&ctx, &mut map).unwrap();
        assert_eq!(side.id(), "f3d:side-scope/asm:face-sidedness#4");
        assert_eq!(
            side.face_key.as_deref(),
            Some("f3d:key-scope/asm:face-native-key#4")
        );
        assert!(side.carrier_flipped);
        assert_eq!(side.native_sense, Sense::Reversed);
        map.finish().unwrap();
    }

    #[test]
    fn face_native_wire_handlers_rewrite_only_declared_identities() {
        use super::super::{FaceNativeKey, FaceSidedness};
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut side = serde_json::json!({
            "id": "f3d:asm:face-sidedness#4", "record_index": 4,
            "face_key": "f3d:asm:face-native-key#4", "native_sense": "reversed",
            "normalized_sense": "forward", "containment": "out"
        });
        let mut key = serde_json::json!({"id": "f3d:asm:face-native-key#4", "record_index": 4, "asm_face_key": 77});
        let mut map = IdentityMap::new(|source: &str| {
            Ok(match source {
                "f3d:asm:face-sidedness#4" => "f3d:side-scope/asm:face-sidedness#4",
                "f3d:asm:face-native-key#4" => "f3d:key-scope/asm:face-native-key#4",
                _ => panic!("native scalar or authored text is not an identity"),
            }
            .to_owned())
        });
        FaceSidedness::rewrite_native_value(&ctx, &mut side, &mut map).unwrap();
        FaceNativeKey::rewrite_native_value(&ctx, &mut key, &mut map).unwrap();
        assert_eq!(
            side,
            serde_json::json!({
                "id": "f3d:side-scope/asm:face-sidedness#4", "record_index": 4,
                "face_key": "f3d:key-scope/asm:face-native-key#4", "native_sense": "reversed",
                "normalized_sense": "forward", "containment": "out"
            })
        );
        assert_eq!(
            key,
            serde_json::json!({"id": "f3d:key-scope/asm:face-native-key#4", "record_index": 4, "asm_face_key": 77})
        );
        map.finish().unwrap();
        // The original wire admission contract is preserved before any rewrite.
        side["record_index"] = serde_json::json!(5);
        let original = side.clone();
        let mut map =
            IdentityMap::new(|_: &str| panic!("invalid source identity cannot reach the mapper"));
        assert!(FaceSidedness::rewrite_native_value(&ctx, &mut side, &mut map).is_err());
        assert_eq!(side, original);
    }

    #[test]
    fn face_native_wire_handlers_observe_context_depth_refusal_before_mapping() {
        use super::super::{FaceNativeKey, FaceSidedness};
        for sidedness in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_recursion_depth = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut value = if sidedness {
                serde_json::json!({"id":"f3d:asm:face-sidedness#4", "record_index":4, "native_sense":"forward", "normalized_sense":"forward"})
            } else {
                serde_json::json!({"id":"f3d:asm:face-native-key#4", "record_index":4})
            };
            let original = value.clone();
            let mut map = IdentityMap::new(|_: &str| panic!("context refusal precedes mapping"));
            let error = if sidedness {
                FaceSidedness::rewrite_native_value(&ctx, &mut value, &mut map)
            } else {
                FaceNativeKey::rewrite_native_value(&ctx, &mut value, &mut map)
            }
            .unwrap_err();
            let CodecError::ResourceLimit(limit) = error else {
                panic!("context depth must refuse");
            };
            assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
            assert_eq!(value, original);
            drop(map);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit)
            );
        }
    }
}
