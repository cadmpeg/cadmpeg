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
    /// Identify the namespace of an unqualified ASM stream.
    #[must_use]
    pub fn new(format: IdFormat) -> Self {
        Self {
            namespace: format!("{format}:asm"),
        }
    }

    pub(super) fn id(&self, kind: &str, record_index: u32) -> String {
        format!("{}:{kind}#{}", self.namespace, record_index)
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

#[cfg(test)]
mod tests {
    use serde::{de::DeserializeOwned, Serialize};

    fn namespace_controls<T: DeserializeOwned + Serialize>(
        kind: &str,
        mut wire: serde_json::Value,
    ) {
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
                .set_arena("namespace_probe", &[typed])
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
