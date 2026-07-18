// SPDX-License-Identifier: Apache-2.0
//! Decode a multi-document `.f3z` archive
//! ([spec §1.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#15-multi-document-archives-f3z)).
//!
//! A `.f3z` holds `Manifest.json` (naming the root `.f3d` member),
//! `DesignDescription.json`, and one `.f3d` member per document. [`decode`]
//! decodes the root member, resolves its outgoing XREFs through the root's
//! `RedirectionsStream.dat`, decodes each referenced member, and merges the
//! component models into the root document with each occurrence-local Design
//! placement applied to its target model.

use std::io::Cursor;

use serde::Deserialize;
use serde_json::Value;

use cadmpeg_ir::codec::{CodecError, DecodeOptions, DecodeResult};
use cadmpeg_ir::document::Model;
use cadmpeg_ir::report::{LossCategory, LossNote, Severity};

use crate::container::ContainerScan;
use crate::native::F3dNative;
use crate::records::XrefReference;
use crate::xref::{self, XrefTable};

/// The archive-level member naming the root document.
pub const MANIFEST_ENTRY: &str = "Manifest.json";
/// The archive-level design-graph member.
pub const DESIGN_DESCRIPTION_ENTRY: &str = "DesignDescription.json";

/// Whether an archive member name is a `.f3d` document member.
fn is_f3d_member(name: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("f3d"))
}

#[derive(Deserialize)]
struct ManifestJson {
    root: String,
}

/// Whether a scanned archive is a `.f3z`: it carries the two archive-level
/// JSON members and at least one `.f3d` document member.
pub fn is_f3z(scan: &ContainerScan) -> bool {
    let has = |name: &str| scan.entries.iter().any(|entry| entry.name == name);
    has(MANIFEST_ENTRY)
        && has(DESIGN_DESCRIPTION_ENTRY)
        && scan
            .entries
            .iter()
            .any(|entry| is_f3d_member(&entry.name) && !entry.name.contains('/'))
}

/// Decode a scanned `.f3z` archive into one merged document.
pub fn decode(scan: &ContainerScan, options: &DecodeOptions) -> Result<DecodeResult, CodecError> {
    let manifest: ManifestJson = serde_json::from_slice(scan.entry_bytes(MANIFEST_ENTRY)?)
        .map_err(|error| {
            CodecError::Malformed(format!("{MANIFEST_ENTRY} is not valid JSON: {error}"))
        })?;
    let root_bytes = scan.entry_bytes(&manifest.root).map_err(|_| {
        CodecError::Malformed(format!(
            "f3z root member {} is not present in the archive",
            manifest.root
        ))
    })?;
    let mut root = crate::decode::decode(&mut Cursor::new(root_bytes.to_vec()), options)?;
    let member_count = scan
        .entries
        .iter()
        .filter(|entry| is_f3d_member(&entry.name))
        .count();
    root.report.notes.push(format!(
        "f3z archive: {member_count} document member(s); root {}",
        manifest.root
    ));
    if options.container_only {
        return Ok(root);
    }

    let table = root
        .ir
        .native
        .namespace("f3d")
        .map(F3dNative::load)
        .transpose()
        .map_err(|error| CodecError::Malformed(format!("invalid root F3D native data: {error}")))?
        .map_or_else(XrefTable::default, |native| XrefTable {
            designs: native.xref_designs,
            references: native.xref_references,
        });
    let mut merged = 0_usize;
    let mut model_value = serialize_model(&root.ir.model)?;
    for reference in &table.references {
        let occurrence = occurrence_key(reference);
        let label = xref::design_for(&table, reference).map_or_else(
            || reference.relative_path.clone(),
            |design| design.display_name.clone(),
        );
        let Ok(member_bytes) = scan.entry_bytes(&reference.relative_path) else {
            root.report.losses.push(LossNote {
                category: LossCategory::Geometry,
                severity: Severity::Error,
                message: format!(
                    "xref {label}: member {} is not present in the archive; the occurrence was \
                     not resolved",
                    reference.relative_path
                ),
                provenance: None,
            });
            continue;
        };
        let mut component = crate::decode::decode(&mut Cursor::new(member_bytes.to_vec()), options)
            .map_err(|error| {
                CodecError::Malformed(format!(
                    "xref member {} failed to decode: {error}",
                    reference.relative_path
                ))
            })?;
        if component.ir.units != root.ir.units {
            root.report.losses.push(LossNote {
                category: LossCategory::Geometry,
                severity: Severity::Error,
                message: format!(
                    "xref {label}: component units differ from the root document; the occurrence \
                     was not merged"
                ),
                provenance: None,
            });
            continue;
        }
        if let Some(transform) = reference.transform {
            apply_occurrence_transform(&mut component.ir.model, transform);
        }
        let mut component_value = serialize_model(&component.ir.model)?;
        remap_ids(&mut component_value, &occurrence);
        extend_arenas(&mut model_value, component_value)?;
        merged += 1;
        root.report.geometry_transferred |= component.report.geometry_transferred;
        for loss in component.report.losses {
            root.report.losses.push(LossNote {
                message: format!("xref {label}: {}", loss.message),
                ..loss
            });
        }
        let placement = if reference.transform.is_some() {
            "Design occurrence transform"
        } else {
            "identity placement"
        };
        root.report.notes.push(format!(
            "xref {label}: merged {} as occurrence {occurrence} ({placement})",
            reference.relative_path
        ));
    }
    root.ir.model = serde_json::from_value(model_value).map_err(|error| {
        CodecError::Malformed(format!("merged model round-trip failed: {error}"))
    })?;
    if merged > 0 {
        root.report.losses.push(LossNote {
            category: LossCategory::Metadata,
            severity: Severity::Info,
            message: format!(
                "{merged} merged component(s) retain model entities only; component native \
                 records and annotations are available by decoding each member individually"
            ),
            provenance: None,
        });
    }
    root.report.notes.push(format!(
        "merged {merged} of {} external reference(s) from the f3z archive",
        table.references.len()
    ));
    root.ir.finalize();
    let hash = crate::decode::semantic_hash(&root.ir);
    if let Some(source) = &mut root.ir.source {
        source.attributes.insert("semantic_sha256".into(), hash);
        source
            .attributes
            .insert("f3z_root".into(), manifest.root.clone());
    }
    Ok(DecodeResult::new(root.ir, root.report))
}

fn serialize_model(model: &Model) -> Result<Value, CodecError> {
    serde_json::to_value(model)
        .map_err(|error| CodecError::Malformed(format!("model serialization failed: {error}")))
}

/// The id-prefix key for one occurrence: its role string, or its ordinal when
/// the role is absent.
fn occurrence_key(reference: &XrefReference) -> String {
    let role = if reference.neutron_role.is_empty() {
        format!("ordinal-{}", reference.ordinal)
    } else {
        reference.neutron_role.clone()
    };
    format!("{role}/occurrence-{}", reference.occurrence_ordinal)
}

fn apply_occurrence_transform(model: &mut Model, source_rows: [[f64; 4]; 4]) {
    let mut occurrence = cadmpeg_ir::transform::Transform { rows: source_rows };
    for row in 0..3 {
        occurrence.rows[row][3] *= 10.0;
    }
    for body in &mut model.bodies {
        body.transform = Some(match body.transform {
            Some(local) => compose_transforms(occurrence, local),
            None => occurrence,
        });
    }
}

fn compose_transforms(
    outer: cadmpeg_ir::transform::Transform,
    inner: cadmpeg_ir::transform::Transform,
) -> cadmpeg_ir::transform::Transform {
    let mut rows = [[0.0; 4]; 4];
    for (row, values) in rows.iter_mut().enumerate() {
        for (column, value) in values.iter_mut().enumerate() {
            *value = (0..4)
                .map(|index| outer.rows[row][index] * inner.rows[index][column])
                .sum();
        }
    }
    cadmpeg_ir::transform::Transform { rows }
}

/// Rewrite every `f3d:`-namespaced id string in a serialized [`Model`] to the
/// occurrence-scoped form `f3d:xref/<occurrence>/<rest>`. Model entity ids and
/// their cross-references all carry the `f3d:` prefix, so the rewrite keeps
/// each occurrence's graph internally consistent and disjoint from the root's.
fn remap_ids(value: &mut Value, occurrence: &str) {
    match value {
        Value::String(text) => {
            if let Some(rest) = text.strip_prefix("f3d:") {
                *text = format!("f3d:xref/{occurrence}/{rest}");
            }
        }
        Value::Array(items) => {
            for item in items {
                remap_ids(item, occurrence);
            }
        }
        Value::Object(fields) => {
            for item in fields.values_mut() {
                remap_ids(item, occurrence);
            }
        }
        _ => {}
    }
}

/// Append every serialized component arena onto the corresponding root arena.
fn extend_arenas(root: &mut Value, component: Value) -> Result<(), CodecError> {
    let (Value::Object(root_fields), Value::Object(component_fields)) = (root, component) else {
        return Err(CodecError::Malformed(
            "serialized model is not a JSON object".into(),
        ));
    };
    for name in Model::arena_names() {
        let Some(Value::Array(source)) = component_fields.get(*name) else {
            continue;
        };
        if source.is_empty() {
            continue;
        }
        let Some(Value::Array(target)) = root_fields.get_mut(*name) else {
            root_fields.insert((*name).to_string(), Value::Array(source.clone()));
            continue;
        };
        target.extend(source.iter().cloned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::transform::Transform;

    #[test]
    fn occurrence_transform_composes_outside_existing_body_transform() {
        let outer = Transform {
            rows: [
                [0.0, -1.0, 0.0, 20.0],
                [1.0, 0.0, 0.0, 30.0],
                [0.0, 0.0, 1.0, 40.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };
        let inner = Transform {
            rows: [
                [1.0, 0.0, 0.0, 5.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };

        assert_eq!(
            super::compose_transforms(outer, inner).rows,
            [
                [0.0, -1.0, 0.0, 20.0],
                [1.0, 0.0, 0.0, 35.0],
                [0.0, 0.0, 1.0, 40.0],
                [0.0, 0.0, 0.0, 1.0],
            ]
        );
    }
}
