// SPDX-License-Identifier: Apache-2.0
//! Mark declared identity fields without interpreting names or expressions.
use cadmpeg_core::{CodecError, decode::DecodeContext};
use cadmpeg_ir::{
    CadIr,
    document::{EntityRewrite, Model},
    schema::rewrite::{self, typed::RewriteIdentities},
};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
pub(in crate::contract) struct Markers {
    pub(super) targets: BTreeMap<String, String>,
    pub(super) keys: HashMap<String, String>,
}
struct Mark<'a, 'ctx> {
    ctx: &'a DecodeContext<'ctx>,
    ids: BTreeMap<String, String>,
    prefix: String,
}
impl Mark<'_, '_> {
    fn marker(&mut self, key: &str) -> String {
        let next = self.ids.len();
        self.ids
            .entry(key.to_owned())
            .or_insert_with(|| format!("{}#{next}", self.prefix))
            .clone()
    }
}
impl EntityRewrite for Mark<'_, '_> {
    type Error = CodecError;
    fn rewrite<T: RewriteIdentities>(&mut self, entity: T) -> Result<T, Self::Error> {
        rewrite::identities(self.ctx, entity, |id| Ok(self.marker(id)))
    }
}
fn native_key(format: &str, role: &str, id: &str) -> String {
    // A complete CADIR identity shares its graph node with typed native_ref.
    if cadmpeg_ir::ids::Identity::new(id).is_ok() {
        id.to_owned()
    } else {
        format!("native/{format}/{role}/{id}")
    }
}
fn mark_native_association(source: &Value, target: &mut Value, mark: &mut Mark<'_, '_>) {
    let Some(source) = source.as_object() else {
        return;
    };
    let format = source
        .get("format")
        .and_then(Value::as_str)
        .expect("source format");
    for (key, role) in [("object_id", "object"), ("layer", "layer")] {
        if let Some(Value::String(id)) = source.get(key) {
            target[key] = Value::String(mark.marker(&native_key(format, role, id)));
        }
    }
    if let Some(Value::Array(path)) = source.get("instance_path") {
        target["instance_path"] = Value::Array(
            path.iter()
                .map(|id| {
                    Value::String(mark.marker(&native_key(
                        format,
                        "object",
                        id.as_str().expect("native instance identity"),
                    )))
                })
                .collect(),
        );
    }
}
pub(in crate::contract) fn model(ir: &CadIr) -> (Value, Value, Markers) {
    let ctx = crate::service_decode_context();
    let mut normalized = ir.clone();
    normalized.model.finalize();
    let original = serde_json::to_value(&normalized.model).expect("serialize neutral model");
    let text = original.to_string();
    let mut suffix = 0;
    let prefix = loop {
        let prefix = format!("contractmark{suffix}:identity:marker");
        if !text.contains(&prefix) {
            break prefix;
        }
        suffix += 1;
    };
    let mut mark = Mark {
        ctx: &ctx,
        ids: BTreeMap::new(),
        prefix,
    };
    let mut rewritten = Model::default();
    rewritten
        .extend_rewritten(normalized.model, &mut mark)
        .expect("mark typed identities");
    let mut marked = serde_json::to_value(rewritten).expect("serialize marked neutral model");
    // These five entity owners explicitly declare SourceObjectAssociation.
    // Its native object/layer/instance keys are identities even when not full
    // CADIR ids. The other fields, including format/name/color, stay literal.
    for arena in ["points", "curves", "surfaces", "subds", "tessellations"] {
        if let (Some(Value::Array(records)), Some(Value::Array(output))) =
            (original.get(arena), marked.get_mut(arena))
        {
            for (record, target) in records.iter().zip(output) {
                if let (Some(source), Some(target)) =
                    (record.get("source_object"), target.get_mut("source_object"))
                {
                    mark_native_association(source, target, &mut mark);
                }
            }
        }
    }
    let keys = mark
        .ids
        .iter()
        .map(|(key, marker)| (marker.clone(), key.clone()))
        .collect();
    (
        original,
        marked,
        Markers {
            targets: mark.ids,
            keys,
        },
    )
}
