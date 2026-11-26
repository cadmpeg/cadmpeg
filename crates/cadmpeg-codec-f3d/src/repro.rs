// SPDX-License-Identifier: Apache-2.0
//! Anonymous stream identities with the native record grammar.
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

pub(crate) fn redactions(ir: &CadIr) -> Result<BTreeMap<String, String>, CodecError> {
    let mut replacements = BTreeMap::new();
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(replacements);
    };
    let mut components = BTreeMap::new();
    let value = cadmpeg_ir::hash::finite_json::to_json_value(ir).map_err(CodecError::malformed)?;
    for row in cadmpeg_ir::repro::rows(&value) {
        let Some(id) = cadmpeg_ir::repro::record(&value, &row)["id"].as_str() else {
            continue;
        };
        if let Some(scope) = id
            .strip_prefix("f3d:")
            .and_then(|tail| tail.split_once(':'))
            .map(|(scope, _)| scope)
        {
            let (key, local) = qualifier(scope);
            if let Some(raw) = crate::ids::decode_identity_key_component(local) {
                remember_scope(key, &raw, &mut components, &mut replacements);
            }
        }
    }
    drop(value);
    for records in namespace.arenas().values() {
        for record in records {
            let fields = record.fields().map_err(CodecError::from)?;
            for (field, value) in fields {
                let Some(text) = value.as_str() else {
                    continue;
                };
                if field == "stream" {
                    remember_scope(None, text, &mut components, &mut replacements);
                }
                if matches!(field.as_str(), "blob_name" | "source_brep") {
                    if let Some(name) = text.strip_prefix("BREP.") {
                        let anonymous = anonymous_component(name, &mut components);
                        replacements.insert(text.into(), format!("BREP.{anonymous}"));
                    }
                }
                if field == "source_namespace" {
                    if let Some(scope) = text.strip_prefix("f3d:") {
                        let (key, local) = qualifier(scope);
                        remember_scope(key, local, &mut components, &mut replacements);
                    }
                }
            }
        }
    }
    Ok(replacements)
}

/// The definition key of a qualified scope and the scope it qualifies.
fn qualifier(scope: &str) -> (Option<&str>, &str) {
    scope
        .split_once('@')
        .map_or((None, scope), |(key, local)| (Some(key), local))
}

/// Anonymizes the scope `raw`, qualified by definition key `key` when the
/// record was read from an archive member.
fn remember_scope(
    key: Option<&str>,
    raw: &str,
    components: &mut BTreeMap<String, String>,
    replacements: &mut BTreeMap<String, String>,
) {
    let anonymous = anonymous_path(raw, components);
    replacements.insert(raw.into(), anonymous.clone());
    let encoded = crate::ids::identity_key_component(raw);
    let anonymous_encoded = crate::ids::identity_key_component(&anonymous);
    let (scope, anonymous_scope) = match key {
        Some(key) => {
            let anonymous_key = anonymous_path(key, components);
            replacements.insert(encoded.clone(), anonymous_encoded.clone());
            (
                format!("{key}@{encoded}"),
                format!("{anonymous_key}@{anonymous_encoded}"),
            )
        }
        None => (encoded, anonymous_encoded),
    };
    replacements.insert(scope.clone(), anonymous_scope.clone());
    replacements.insert(format!("f3d:{scope}"), format!("f3d:{anonymous_scope}"));
}

fn anonymous_path(raw: &str, components: &mut BTreeMap<String, String>) -> String {
    raw.split('/')
        .map(|component| {
            let occurrence = component
                .strip_prefix("occurrence-")
                .is_some_and(|ordinal| ordinal.parse::<u64>().is_ok());
            if occurrence
                || matches!(
                    component,
                    "xref"
                        | "brep"
                        | "design"
                        | "model"
                        | "feature-input"
                        | "appearance"
                        | "tspline"
                        | "BulkStream.dat"
                        | "MetaStream.dat"
                )
            {
                component.into()
            } else {
                anonymous_component(component, components)
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn anonymous_component(raw: &str, components: &mut BTreeMap<String, String>) -> String {
    let ordinal = components.len();
    components
        .entry(raw.into())
        .or_insert_with(|| format!("repro{ordinal}"))
        .clone()
}

#[cfg(test)]
mod tests;
