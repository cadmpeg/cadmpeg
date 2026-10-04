// SPDX-License-Identifier: Apache-2.0
//! Source identity shared by ASM-native record projections.

use crate::ids::IdFormat;
use cadmpeg_ir::ids::Identity;

/// Admitted format and scope of an ASM-native record identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeRecordNamespace {
    namespace: String,
}

impl NativeRecordNamespace {
    /// The source namespace without a record kind or index.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.namespace
    }

    /// Identify the namespace of an unqualified ASM stream.
    #[must_use]
    pub fn new(format: IdFormat) -> Self {
        Self {
            namespace: format!("{format}:asm"),
        }
    }

    pub(super) fn id(&self, kind: &str, record_index: u32) -> String {
        self.serialized_id(kind, record_index).to_string()
    }

    pub(super) fn serialized_id<'a>(
        &'a self,
        kind: &'a str,
        record_index: u32,
    ) -> NativeRecordIdentity<'a> {
        NativeRecordIdentity {
            namespace: self,
            kind,
            record_index,
        }
    }

    pub(super) fn rewrite<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        kind: &str,
        record_index: u32,
        map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, F>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let source = ctx.format_scoped(
            format_args!("{}", self.serialized_id(kind, record_index)),
            "format ASM identity rewrite source",
        )?;
        let mut target = map.identity(ctx, &source.0)?;
        let work = cadmpeg_core::decode::u64_from_index(target.len())
            .checked_mul(3)
            .and_then(|work| work.checked_add(cadmpeg_core::decode::u64_from_index(source.0.len())))
            .ok_or_else(|| {
                ctx.refuse_codec_limit("split ASM identity rewrite target", u64::MAX - 1, u64::MAX)
            })?;
        ctx.charge_work(work, "split ASM identity rewrite target")?;
        let (namespace, record) = target.rsplit_once(':').ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("ASM identity has no record separator")
        })?;
        let source_record = source
            .0
            .rsplit_once(':')
            .map(|(_, record)| record)
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("ASM source identity has no record separator")
            })?;
        if record != source_record {
            return Err(cadmpeg_core::CodecError::malformed(
                "ASM identity rewrite must preserve record kind and index",
            ));
        }
        let namespace_length = namespace.len();
        target.truncate(namespace_length);
        Ok(Self { namespace: target })
    }

    pub(super) fn visit(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        kind: &str,
        record_index: u32,
        visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let source = ctx.format_scoped(
            format_args!("{}", self.serialized_id(kind, record_index)),
            "format ASM identity reference",
        )?;
        visitor(&source.0)
    }

    pub(super) fn from_wire(id: &str, record_index: u32, kind: &str) -> Result<Self, String> {
        let id = Identity::new(id).map_err(|error| error.to_string())?;
        let suffix = format!(":{kind}#{record_index}");
        let namespace = id.as_str().strip_suffix(&suffix).ok_or_else(|| {
            format!("native id does not match {kind} record_index {record_index}")
        })?;
        Ok(Self {
            namespace: namespace.to_owned(),
        })
    }
}

/// Borrow the identity components until the serializer admits their text.
pub(super) struct NativeRecordIdentity<'a> {
    namespace: &'a NativeRecordNamespace,
    kind: &'a str,
    record_index: u32,
}

impl std::fmt::Display for NativeRecordIdentity<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}:{}#{}",
            self.namespace.as_str(),
            self.kind,
            self.record_index
        )
    }
}

impl serde::Serialize for NativeRecordIdentity<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[cfg(test)]
mod tests {
    use serde::{de::DeserializeOwned, Serialize};

    #[test]
    fn native_identity_serialization_borrows_its_namespace() {
        let namespace = super::NativeRecordNamespace::new(crate::asm_format!("f3d"));
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 25;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let id = namespace.serialized_id("edge-continuity", 1);
        let projected =
            cadmpeg_ir::schema::structural::project(&ctx, &id, "project native identity").unwrap();
        assert_eq!(
            *projected,
            serde_value::Value::String("f3d:asm:edge-continuity#1".into())
        );
        assert_eq!(id.to_string(), namespace.id("edge-continuity", 1));
        drop(projected);
        ctx.finish_session().unwrap();
    }

    fn namespace_controls<T: DeserializeOwned + Serialize>(
        kind: &str,
        mut wire: serde_json::Value,
    ) {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::default(),
        )
        .expect("empty test input fits the service policy");
        for namespace in [
            "f3d:asm",
            "sat:asm",
            "f3d:xref/role-part/reference-1/occurrence-0/asm",
            "f3d:xref/role-part/reference-1/occurrence-0/xref/child/asm",
            "custom-format:native%20scope",
        ] {
            let id = format!("{namespace}:{kind}#1");
            wire["id"] = id.clone().into();
            let typed: T = serde_json::from_value(wire.clone()).expect("valid scoped record");
            assert_eq!(serde_json::to_value(&typed).unwrap(), wire);
            let mut document = cadmpeg_ir::CadIr::empty();
            document
                .native
                .namespace_mut("f3d")
                .set_arena(&ctx, "namespace_probe", &[typed])
                .unwrap();
            let document: cadmpeg_ir::CadIr =
                serde_json::from_value(serde_json::to_value(document).unwrap()).unwrap();
            let admitted: Vec<T> = document
                .native
                .namespace("f3d")
                .unwrap()
                .arena_as("namespace_probe")
                .unwrap();
            assert_eq!(serde_json::to_value(&admitted[0]).unwrap(), wire);
        }
        for namespace in [
            "",
            "f3d",
            ":asm",
            "f3d:",
            "f3d:extra:asm",
            "f3d:bad scope",
            "f3d:bad\u{2003}scope",
            "f3d:bad#scope",
        ] {
            wire["id"] = format!("{namespace}:{kind}#1").into();
            let error = serde_json::from_value::<T>(wire.clone())
                .err()
                .expect("malformed namespace must fail typed admission")
                .to_string();
            assert!(error.contains("identity"), "{error}");
        }
        for id in [format!("f3d:asm:{kind}#2"), "f3d:asm:other-kind#1".into()] {
            wire["id"] = id.into();
            let error = serde_json::from_value::<T>(wire.clone())
                .err()
                .expect("identity must agree with kind and record index")
                .to_string();
            assert!(error.contains("record_index"), "{error}");
        }
    }

    #[test]
    fn macro_record_namespace_is_admitted_before_typed_construction() {
        namespace_controls::<super::super::EdgeContinuity>(
            "edge-continuity",
            serde_json::json!({
                "id": "f3d:asm:edge-continuity#1", "record_index": 1,
                "edge": "f3d:brep:entity#1", "sense": "forward", "continuity": "tangent"
            }),
        );
    }

    #[test]
    fn face_record_namespace_is_admitted_before_typed_construction() {
        namespace_controls::<super::super::FaceSidedness>(
            "face-sidedness",
            serde_json::json!({
                "id": "f3d:asm:face-sidedness#1", "record_index": 1,
                "face": "f3d:brep:entity#1", "native_sense": "forward", "normalized_sense": "forward"
            }),
        );
    }
}
