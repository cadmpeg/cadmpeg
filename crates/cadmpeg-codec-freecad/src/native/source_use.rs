// SPDX-License-Identifier: Apache-2.0
//! Reference-only identities for actual native root and child uses.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};
use std::fmt::Write;

/// Scalar source positions and real native parent references. No persistent
/// integer path collection is introduced (CADIR v8 #246 IV.2 check 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SourceUse {
    Root { root: usize },
    Child { parent: String, slot: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceUseSelector {
    pub(crate) id: String,
    pub(crate) owner: String,
    pub(crate) source_use: SourceUse,
}

#[derive(Clone, Copy)]
pub(crate) enum SourceUseValue<'a> {
    Root(usize),
    Child { parent: &'a str, slot: usize },
}

fn copy_text(ctx: &DecodeContext<'_>, text: &str) -> Result<String, CodecError> {
    let mut result = String::new();
    ctx.reserve_text_working(&mut result, text.len(), "FreeCAD source-use identity")?;
    result.push_str(text);
    Ok(result)
}

impl SourceUseSelector {
    /// Fresh decoder output only. Encoder reprojections reuse actual existing
    /// identities through their working index and never call this admission.
    pub(crate) fn admit(
        ctx: &DecodeContext<'_>,
        owner: &str,
        ordinal: usize,
        source: SourceUseValue<'_>,
    ) -> Result<Self, CodecError> {
        let _record = ctx.enter_nested("construct canonical native value")?;
        let _source = ctx.enter_nested("construct canonical native value")?;
        let bytes = retained_text_size(owner, ordinal, source)?
            .checked_add(size_of::<cadmpeg_ir::native::NativeRecord>())
            .ok_or_else(|| CodecError::malformed("source-use native row size overflows"))?;
        ctx.charge_entities(1, "FreeCAD source-use native entities")?;
        ctx.retain_bytes(u64_from_index(bytes), "FreeCAD source-use native row")?;
        let capacity = owner
            .len()
            .checked_add(":topology-use:".len())
            .and_then(|size| size.checked_add(digits(ordinal)))
            .ok_or_else(|| CodecError::malformed("source-use native identity size overflows"))?;
        let mut id = String::new();
        ctx.reserve_text_working(&mut id, capacity, "FreeCAD source-use identity")?;
        write!(id, "{owner}:topology-use:{ordinal}")
            .map_err(|_| CodecError::malformed("source-use native identity cannot be formed"))?;
        let id = cadmpeg_ir::ids::Identity::new(id)
            .map_err(CodecError::malformed)?
            .into_string();
        let source_use = match source {
            SourceUseValue::Root(root) => SourceUse::Root { root },
            SourceUseValue::Child { parent, slot } => SourceUse::Child {
                parent: copy_text(ctx, parent)?,
                slot,
            },
        };
        Ok(Self {
            id,
            owner: copy_text(ctx, owner)?,
            source_use,
        })
    }

    pub(crate) fn into_record(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<cadmpeg_ir::native::NativeRecord, CodecError> {
        let _record = ctx.enter_nested("construct canonical native value")?;
        let _source = ctx.enter_nested("construct canonical native value")?;
        let serde_json::Value::Object(mut fields) =
            serde_json::to_value(&self).map_err(CodecError::malformed)?
        else {
            return Err(CodecError::malformed(
                "source-use native row is not an object",
            ));
        };
        fields.remove("id");
        let id = cadmpeg_ir::ids::Identity::new(self.id).map_err(CodecError::malformed)?;
        // Admission above already charged this exact JSON and NativeRecord
        // header. set_arena_from would charge the same home a second time.
        Ok(cadmpeg_ir::native::NativeRecord::new(id, fields)?)
    }
}

fn digits(mut value: usize) -> usize {
    let mut result = 1;
    while value >= 10 {
        value /= 10;
        result += 1;
    }
    result
}

fn escaped_content_size(text: &str) -> Result<usize, CodecError> {
    struct Counter(Option<usize>);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.and_then(|count| count.checked_add(bytes.len()));
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Counter(Some(0));
    serde_json::to_writer(&mut count, text).map_err(CodecError::malformed)?;
    count
        .0
        .and_then(|count| count.checked_sub(2))
        .ok_or_else(|| CodecError::malformed("source-use text size is invalid"))
}

fn retained_text_size(
    owner: &str,
    ordinal: usize,
    source: SourceUseValue<'_>,
) -> Result<usize, CodecError> {
    let owner = escaped_content_size(owner)?;
    let common = owner
        .checked_mul(2)
        .and_then(|size| size.checked_add(":topology-use:".len()))
        .and_then(|size| size.checked_add(digits(ordinal)))
        .ok_or_else(|| CodecError::malformed("source-use text size overflows"))?;
    let (fixed, value) = match source {
        SourceUseValue::Root(root) => (
            r#"{"id":"","owner":"","source_use":{"kind":"root","root":}}"#.len(),
            digits(root),
        ),
        SourceUseValue::Child { parent, slot } => (
            r#"{"id":"","owner":"","source_use":{"kind":"child","parent":"","slot":}}"#.len(),
            escaped_content_size(parent)?
                .checked_add(digits(slot))
                .ok_or_else(|| CodecError::malformed("source-use text size overflows"))?,
        ),
    };
    common
        .checked_add(fixed)
        .and_then(|size| size.checked_add(value))
        .ok_or_else(|| CodecError::malformed("source-use text size overflows"))
}

impl cadmpeg_ir::schema::rewrite::typed::RewriteIdentities for SourceUseSelector {
    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visitor(&self.owner)?;
        if let SourceUse::Child { parent, .. } = &self.source_use {
            visitor(parent)?;
        }
        Ok(())
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        _ctx: &DecodeContext<'_>,
        map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        self.id = map.identity(&self.id)?;
        self.owner = map.identity(&self.owner)?;
        if let SourceUse::Child { parent, .. } = &mut self.source_use {
            *parent = map.identity(parent)?;
        }
        Ok(self)
    }
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        _ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        fn identity<F: FnMut(&str) -> Result<String, CodecError>>(
            value: &mut serde_json::Value,
            map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, F>,
        ) -> Result<(), CodecError> {
            let text = value
                .as_str()
                .ok_or_else(|| CodecError::malformed("source-use native identity is not text"))?;
            *value = map.identity(text)?.into();
            Ok(())
        }
        for name in ["id", "owner"] {
            identity(
                value
                    .get_mut(name)
                    .ok_or_else(|| CodecError::malformed("source-use native identity is absent"))?,
                map,
            )?;
        }
        if value["source_use"]["kind"] == "child" {
            identity(
                value
                    .get_mut("source_use")
                    .and_then(|value| value.get_mut("parent"))
                    .ok_or_else(|| CodecError::malformed("source-use native parent is absent"))?,
                map,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scalar_selector_retention_matches_root_and_child_native_json() {
        for owner in ["source", "source\\\"☃"] {
            for ordinal in [1, 10, 1000] {
                for source in [
                    SourceUseValue::Root(12),
                    SourceUseValue::Child {
                        parent: "opaque:native:use#名\\\"",
                        slot: 17,
                    },
                ] {
                    let source_use = match source {
                        SourceUseValue::Root(root) => SourceUse::Root { root },
                        SourceUseValue::Child { parent, slot } => SourceUse::Child {
                            parent: parent.to_owned(),
                            slot,
                        },
                    };
                    let row = SourceUseSelector {
                        id: format!("{owner}:topology-use:{ordinal}"),
                        owner: owner.to_owned(),
                        source_use,
                    };
                    assert_eq!(
                        retained_text_size(owner, ordinal, source).unwrap(),
                        serde_json::to_vec(&row).unwrap().len()
                    );
                }
            }
        }
    }
}
