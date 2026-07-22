// SPDX-License-Identifier: Apache-2.0
//! Decode a multi-document `.f3z` archive
//! ([spec §1.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#15-multi-document-archives-f3z)).
//!
//! A `.f3z` holds `Manifest.json` (naming the root `.f3d` member),
//! `DesignDescription.json`, and one `.f3d` member per document. [`decode`]
//! decodes the root member, recursively resolves each member's outgoing XREFs,
//! and merges the component models into the root document with every
//! occurrence-local Design placement applied from child to ancestor.

use serde::Deserialize;
use serde_value::Value;

use cadmpeg_ir::codec::{CodecError, DecodeResult};
use cadmpeg_ir::decode::DecodeContext;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::report::{LossCategory, LossCode, LossNote, Severity};

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
pub fn decode(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<DecodeResult, CodecError> {
    let manifest: ManifestJson = serde_json::from_slice(scan.entry_bytes(MANIFEST_ENTRY)?)
        .map_err(|error| {
            CodecError::Malformed(format!("{MANIFEST_ENTRY} is not valid JSON: {error}"))
        })?;
    let root_view = scan.entry_view(&manifest.root).ok_or_else(|| {
        CodecError::Malformed(format!(
            "f3z root member {} is not present in the archive",
            manifest.root
        ))
    })?;
    let mut root = crate::decode::decode(ctx, root_view)?;
    let member_count = scan
        .entries
        .iter()
        .filter(|entry| is_f3d_member(&entry.name))
        .count();
    root.report.notes.push(format!(
        "f3z archive: {member_count} document member(s); root {}",
        manifest.root
    ));
    if ctx.container_only() {
        return Ok(root);
    }

    let table = xref_table_from_ir(&root.ir)?;
    let mut stack = vec![manifest.root.clone()];
    let merged = merge_references(ctx, &mut root, scan, &table, &mut stack)?;
    if merged > 0 {
        root.report.notes.push(format!(
            "{merged} merged component(s) retain occurrence-scoped model entities and native \
             records; member source streams remain archive-local"
        ));
    }
    root.report.notes.push(format!(
        "merged {merged} external occurrence(s) from the f3z archive"
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

fn xref_table_from_ir(ir: &cadmpeg_ir::CadIr) -> Result<XrefTable, CodecError> {
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(XrefTable::default());
    };
    let native = F3dNative::load(namespace)
        .map_err(|error| CodecError::Malformed(format!("invalid F3D native data: {error}")))?;
    Ok(XrefTable {
        designs: native.xref_designs,
        references: native.xref_references,
    })
}

fn merge_references(
    ctx: &DecodeContext<'_>,
    parent: &mut DecodeResult,
    scan: &ContainerScan<'_>,
    table: &XrefTable,
    stack: &mut Vec<String>,
) -> Result<usize, CodecError> {
    let mut merged = 0usize;
    let mut model_value = serialize_model(&parent.ir.model)?;
    let mut native_value = serialize_native(&parent.ir)?;
    for reference in &table.references {
        let occurrence = occurrence_key(reference);
        let label = xref::design_for(table, reference).map_or_else(
            || reference.relative_path.clone(),
            |design| design.display_name.clone(),
        );
        if stack.contains(&reference.relative_path) {
            parent.report.losses.push(LossNote {
                code: LossCode::AssemblyComponentsExternal,
                category: LossCategory::Geometry,
                severity: Severity::Error,
                message: format!(
                    "xref {label}: reference cycle through {}; the occurrence was not resolved",
                    reference.relative_path
                ),
                provenance: None,
            });
            continue;
        }
        let Some(member_view) = scan.entry_view(&reference.relative_path) else {
            parent.report.losses.push(LossNote {
                code: LossCode::AssemblyComponentsExternal,
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
        let mut component = crate::decode::decode(ctx, member_view).map_err(|error| {
            CodecError::Malformed(format!(
                "xref member {} failed to decode: {error}",
                reference.relative_path
            ))
        })?;
        if component.ir.units != parent.ir.units {
            parent.report.losses.push(LossNote {
                code: LossCode::AssemblyComponentsExternal,
                category: LossCategory::Geometry,
                severity: Severity::Error,
                message: format!(
                    "xref {label}: component units differ from the containing document; the \
                     occurrence was not merged"
                ),
                provenance: None,
            });
            continue;
        }
        let child_table = xref_table_from_ir(&component.ir)?;
        stack.push(reference.relative_path.clone());
        let descendants = merge_references(ctx, &mut component, scan, &child_table, stack)?;
        stack.pop();
        if let Some(transform) = reference.transform {
            apply_occurrence_transform(&mut component.ir.model, transform);
        }
        let mut component_value = serialize_model(&component.ir.model)?;
        remap_ids(&mut component_value, &occurrence);
        extend_arenas(&mut model_value, component_value)?;
        let mut component_native = serialize_native(&component.ir)?;
        remap_ids(&mut component_native, &occurrence);
        extend_native_arenas(&mut native_value, component_native)?;
        merged += descendants + 1;
        parent.report.geometry_transferred |= component.report.geometry_transferred;
        for loss in component.report.losses {
            parent.report.losses.push(LossNote {
                message: format!("xref {label}: {}", loss.message),
                ..loss
            });
        }
        let placement = if reference.transform.is_some() {
            "Design occurrence transform"
        } else {
            "identity placement"
        };
        parent.report.notes.push(format!(
            "xref {label}: merged {} as occurrence {occurrence} ({placement}; {descendants} nested \
             occurrence(s))",
            reference.relative_path
        ));
    }
    parent.ir.model = model_value.deserialize_into().map_err(|error| {
        CodecError::Malformed(format!("merged model round-trip failed: {error}"))
    })?;
    let native: F3dNative = native_value.deserialize_into().map_err(|error| {
        CodecError::Malformed(format!("merged native data round-trip failed: {error}"))
    })?;
    native
        .store(parent.ir.native.namespace_mut("f3d"))
        .map_err(|error| {
            CodecError::Malformed(format!("merged native data is invalid: {error}"))
        })?;
    Ok(merged)
}

fn serialize_model(model: &Model) -> Result<Value, CodecError> {
    serde_value::to_value(model)
        .map_err(|error| CodecError::Malformed(format!("model serialization failed: {error}")))
}

fn serialize_native(ir: &cadmpeg_ir::CadIr) -> Result<Value, CodecError> {
    let native = ir
        .native
        .namespace("f3d")
        .map(F3dNative::load)
        .transpose()
        .map_err(|error| CodecError::Malformed(format!("invalid F3D native data: {error}")))?
        .unwrap_or_default();
    serde_value::to_value(native)
        .map_err(|error| CodecError::Malformed(format!("native serialization failed: {error}")))
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
        Value::Seq(items) => {
            for item in items {
                remap_ids(item, occurrence);
            }
        }
        Value::Map(fields) => {
            let entries = std::mem::take(fields);
            for (mut key, mut item) in entries {
                remap_ids(&mut key, occurrence);
                remap_ids(&mut item, occurrence);
                fields.insert(key, item);
            }
        }
        Value::Option(Some(item)) | Value::Newtype(item) => remap_ids(item, occurrence),
        _ => {}
    }
}

/// Append every serialized component arena onto the corresponding root arena.
fn extend_arenas(root: &mut Value, component: Value) -> Result<(), CodecError> {
    let (Value::Map(root_fields), Value::Map(mut component_fields)) = (root, component) else {
        return Err(CodecError::Malformed(
            "serialized model is not a struct map".into(),
        ));
    };
    for name in Model::arena_names() {
        let key = Value::String((*name).to_string());
        let Some(Value::Seq(source)) = component_fields.remove(&key) else {
            continue;
        };
        if source.is_empty() {
            continue;
        }
        let Some(Value::Seq(target)) = root_fields.get_mut(&key) else {
            root_fields.insert(key, Value::Seq(source));
            continue;
        };
        target.extend(source);
    }
    Ok(())
}

fn extend_native_arenas(root: &mut Value, component: Value) -> Result<(), CodecError> {
    let (Value::Map(root_fields), Value::Map(mut component_fields)) = (root, component) else {
        return Err(CodecError::Malformed(
            "serialized native data is not a struct map".into(),
        ));
    };
    for name in crate::native::F3D_ARENA_NAMES {
        let key = Value::String((*name).to_string());
        let Some(Value::Seq(source)) = component_fields.remove(&key) else {
            continue;
        };
        if source.is_empty() {
            continue;
        }
        let Some(Value::Seq(target)) = root_fields.get_mut(&key) else {
            return Err(CodecError::Malformed(format!(
                "serialized native arena {name} is not a sequence"
            )));
        };
        target.extend(source);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::native::F3dNative;
    use crate::records::DesignSketchPlacement;
    use cadmpeg_ir::document::Model;
    use cadmpeg_ir::ids::{BodyId, RegionId};
    use cadmpeg_ir::topology::{Body, BodyKind, Region};
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

    #[test]
    fn repeated_occurrence_merge_remaps_typed_graphs_disjointly() {
        let mut root = super::serialize_model(&Model::default()).expect("serialize root model");
        let component = Model {
            bodies: vec![Body {
                id: BodyId("f3d:brep:entity#1".into()),
                kind: BodyKind::Solid,
                regions: vec![RegionId("f3d:brep:entity#2".into())],
                transform: None,
                name: None,
                color: None,
                visible: None,
            }],
            regions: vec![Region {
                id: RegionId("f3d:brep:entity#2".into()),
                body: BodyId("f3d:brep:entity#1".into()),
                shells: Vec::new(),
            }],
            ..Model::default()
        };
        for ordinal in 0..2 {
            let mut occurrence =
                super::serialize_model(&component).expect("serialize component model");
            super::remap_ids(&mut occurrence, &format!("role/occurrence-{ordinal}"));
            super::extend_arenas(&mut root, occurrence).expect("merge component arenas");
        }
        let merged: Model = root.deserialize_into().expect("deserialize merged model");

        for ordinal in 0..2 {
            let prefix = format!("f3d:xref/role/occurrence-{ordinal}/brep:entity#");
            assert_eq!(merged.bodies[ordinal].id.0, format!("{prefix}1"));
            assert_eq!(merged.bodies[ordinal].regions[0].0, format!("{prefix}2"));
            assert_eq!(merged.regions[ordinal].id.0, format!("{prefix}2"));
            assert_eq!(merged.regions[ordinal].body.0, format!("{prefix}1"));
        }
    }

    #[test]
    fn occurrence_merge_remaps_and_retains_native_records() {
        let mut root = serde_value::to_value(F3dNative::default()).expect("serialize root native");
        let mut component = F3dNative::default();
        component
            .design_sketch_placements
            .push(DesignSketchPlacement {
                id: "f3d:Design/BulkStream.dat:design-sketch-placement#42".into(),
                scope_record_index: None,
                entity_id: "Sketch_1".into(),
                entity_suffix: 1,
                byte_offset: 42,
                class_tag: "001".into(),
                record_index: 7,
                frame_length: 34,
                transform: [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
                transform_offset: None,
                paired_class_tag: "002".into(),
                paired_byte_offset: 76,
                member_run_head: true,
            });
        let mut occurrence = serde_value::to_value(component).expect("serialize component native");
        super::remap_ids(&mut occurrence, "role/occurrence-0");
        super::extend_native_arenas(&mut root, occurrence).expect("merge component native");

        let merged: F3dNative = root.deserialize_into().expect("deserialize merged native");
        assert_eq!(
            merged.design_sketch_placements[0].id,
            "f3d:xref/role/occurrence-0/Design/BulkStream.dat:design-sketch-placement#42"
        );
    }
}
