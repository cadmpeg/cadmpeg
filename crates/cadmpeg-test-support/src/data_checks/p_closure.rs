// SPDX-License-Identifier: Apache-2.0
//! P-closure (#219 §4.3) and the `Native` kind check (#209 §2.2).
//!
//! The named homes transcribe the native arenas that the Phase 3 codec issues
//! (#220–#230) name with a neutral home, by exact arena name.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use serde_json::Value;

/// How the records of a named native arena reach their neutral home.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    /// Each record has its own neutral record in a home arena.
    Own,
    /// A record may share its neutral record with others of its arena.
    Shared,
    /// A record is consumed by the projection of the record its `owner` names.
    Consumed,
}

/// One native arena that a codec issue names with its neutral home.
#[derive(Debug, Clone, Copy)]
pub struct NamedHome {
    /// Native namespace.
    pub namespace: &'static str,
    /// Native arena; a trailing `*` matches every arena with that prefix.
    pub arena: &'static str,
    /// Neutral home arenas.
    pub homes: &'static [&'static str],
    /// How its records link.
    pub link: Link,
}

const fn home(
    namespace: &'static str,
    arena: &'static str,
    homes: &'static [&'static str],
    link: Link,
) -> NamedHome {
    NamedHome {
        namespace,
        arena,
        homes,
        link,
    }
}

const CONSUMED: &[&str] = &[];

/// The native arenas the codec issues name with a neutral home.
pub const NAMED_HOMES: &[NamedHome] = &[
    home("iges", "solid_assemblies", &["occurrences"], Link::Own),
    home("iges", "solid_instances", &["occurrences"], Link::Own),
    home("iges", "external_references", &["occurrences"], Link::Own),
    home("iges", "drawings", &["drawings"], Link::Own),
    home(
        "iges",
        "views",
        &["drawings", "view_presentations"],
        Link::Own,
    ),
    home(
        "iges",
        "view_visibility",
        &["drawings", "view_presentations"],
        Link::Own,
    ),
    home(
        "iges",
        "segmented_visibility",
        &["drawings", "view_presentations"],
        Link::Own,
    ),
    home("iges", "annotations", &["semantic_annotations"], Link::Own),
    home("iges", "procedural_solids", &["features"], Link::Own),
    home("iges", "boolean_trees", &["features"], Link::Own),
    home("iges", "selected_components", &["features"], Link::Own),
    home("iges", "primitive_solids", &["features"], Link::Own),
    home("iges", "rectangular_arrays", &["features"], Link::Own),
    home("iges", "circular_arrays", &["features"], Link::Own),
    home("iges", "properties", &["attributes"], Link::Own),
    home("iges", "product_properties", &["attributes"], Link::Own),
    home("iges", "connect_points", CONSUMED, Link::Consumed),
    home("rhino", "user_strings", &["attributes"], Link::Own),
    home(
        "rhino",
        "attribute_user_strings",
        &["attributes"],
        Link::Own,
    ),
    home(
        "rhino",
        "legacy_v1_records",
        &["semantic_annotations"],
        Link::Own,
    ),
    home("fcstd", "properties", &["attributes"], Link::Own),
    home("fcstd", "entries", &["appearances"], Link::Own),
    home(
        "f3d",
        "design_sheet_metal_settings",
        &["features"],
        Link::Shared,
    ),
    home(
        "f3d",
        "design_body_recipe_operands",
        &["feature_input_topologies"],
        Link::Shared,
    ),
    home(
        "f3d",
        "design_construction_operand_groups",
        &["feature_input_topologies"],
        Link::Shared,
    ),
    home(
        "f3d",
        "design_construction_operand_identities",
        &["feature_input_topologies"],
        Link::Shared,
    ),
    home(
        "f3d",
        "design_edge_identity_operands",
        &["feature_input_topologies"],
        Link::Shared,
    ),
    home(
        "f3d",
        "design_edge_operands",
        &["feature_input_topologies"],
        Link::Shared,
    ),
    home(
        "f3d",
        "design_edge_treatment_vertex_operands",
        &["feature_input_topologies"],
        Link::Shared,
    ),
    home(
        "f3d",
        "design_entity_selection_operands",
        &["feature_input_topologies"],
        Link::Shared,
    ),
    home(
        "f3d",
        "design_face_operands",
        &["feature_input_topologies"],
        Link::Shared,
    ),
    home("f3d", "creation_timestamps", &["attributes"], Link::Own),
    home(
        "sldprt",
        "sketch_input_entities",
        &["sketch_entities"],
        Link::Own,
    ),
    home("sldprt", "pmi_dimensions", &["pmi"], Link::Own),
    home(
        "sldprt",
        "feature_input_*",
        &["feature_input_topologies"],
        Link::Own,
    ),
    home("inventor", "pm_dc_parameters", &["parameters"], Link::Own),
    home("inventor", "pm_dc_features", &["features"], Link::Own),
    home(
        "inventor",
        "pm_dc_pattern_features",
        &["features"],
        Link::Own,
    ),
    home("inventor", "pm_dc_sketches", &["sketches"], Link::Own),
    home(
        "inventor",
        "pm_dc_sketch_entities",
        &["sketch_entities"],
        Link::Own,
    ),
    home(
        "inventor",
        "pm_dc_sketch_constraints",
        &["sketch_constraints"],
        Link::Own,
    ),
    home("inventor", "pm_dc_expressions", CONSUMED, Link::Consumed),
    home("inventor", "pm_dc_units", CONSUMED, Link::Consumed),
    home("inventor", "pm_dc_transforms", CONSUMED, Link::Consumed),
    home("inventor", "pm_dc_directions", CONSUMED, Link::Consumed),
    home(
        "inventor",
        "pm_dc_feature_terminators",
        CONSUMED,
        Link::Consumed,
    ),
    home(
        "inventor",
        "pm_dc_feature_properties",
        CONSUMED,
        Link::Consumed,
    ),
    home("inventor", "pm_dc_feature_labels", CONSUMED, Link::Consumed),
    home(
        "inventor",
        "pm_dc_entity_style_links",
        CONSUMED,
        Link::Consumed,
    ),
    home(
        "inventor",
        "ufrx_model_states",
        &["configurations"],
        Link::Own,
    ),
    home("inventor", "embedded_references", &["assets"], Link::Own),
    home("catia", "external_references", &["occurrences"], Link::Own),
    home(
        "catia",
        "design_objects",
        &["features", "sketches"],
        Link::Own,
    ),
    home(
        "catia",
        "value_blocks",
        &["parameters", "appearances", "attributes"],
        Link::Own,
    ),
    home(
        "catia",
        "value_schema_selections",
        &["parameters", "appearances", "attributes"],
        Link::Own,
    ),
    home("catia", "consolidated_pcurves", CONSUMED, Link::Consumed),
    home("catia", "consolidated_edge_runs", CONSUMED, Link::Consumed),
    home(
        "catia",
        "consolidated_domain_relations",
        CONSUMED,
        Link::Consumed,
    ),
    home("catia", "sampled_parameter_maps", CONSUMED, Link::Consumed),
    home("catia", "preview_images", &["assets"], Link::Own),
    home("nx", "external_reference*", &["occurrences"], Link::Shared),
    home(
        "nx",
        "display_jt_string_property_atoms",
        &["attributes"],
        Link::Own,
    ),
    home("nx", "material_texture_*", &["appearances"], Link::Own),
    home(
        "nx",
        "feature_construction_named_targets",
        CONSUMED,
        Link::Consumed,
    ),
    home(
        "nx",
        "feature_draft_construction_binary64_lanes",
        CONSUMED,
        Link::Consumed,
    ),
    home(
        "nx",
        "feature_enlarge_construction_envelopes",
        CONSUMED,
        Link::Consumed,
    ),
    home(
        "nx",
        "feature_extract_face_construction_envelopes",
        CONSUMED,
        Link::Consumed,
    ),
    home("nx", "feature_fset_index_lanes", CONSUMED, Link::Consumed),
    home(
        "nx",
        "feature_pattern_construction_binary64_lanes",
        CONSUMED,
        Link::Consumed,
    ),
    home(
        "nx",
        "feature_studio_post_branch_references",
        CONSUMED,
        Link::Consumed,
    ),
];

/// The named home of one native arena, if a codec issue names one.
#[must_use]
pub fn named_home(namespace: &str, arena: &str) -> Option<&'static NamedHome> {
    NAMED_HOMES.iter().find(|row| {
        row.namespace == namespace
            && row
                .arena
                .strip_suffix('*')
                .map_or(row.arena == arena, |prefix| arena.starts_with(prefix))
    })
}

type Reads = BTreeMap<String, Vec<Range<u64>>>;

fn reads(fidelity: &Value, id: &str) -> Reads {
    let mut result = Reads::new();
    let Some(streams) = fidelity
        .pointer("/record_layout")
        .and_then(|layouts| layouts.get(id))
        .and_then(|layout| layout.get("reads"))
        .and_then(Value::as_object)
    else {
        return result;
    };
    for (stream, ranges) in streams {
        let ranges = ranges
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|range| Some(range.get("start")?.as_u64()?..range.get("end")?.as_u64()?))
            .collect();
        result.insert(stream.clone(), ranges);
    }
    result
}

fn intersect(left: &Reads, right: &Reads) -> bool {
    left.iter().any(|(stream, ranges)| {
        right.get(stream).is_some_and(|other| {
            ranges
                .iter()
                .any(|a| other.iter().any(|b| a.start < b.end && b.start < a.end))
        })
    })
}

fn is_b1(fidelity: &Value, id: &str) -> bool {
    fidelity
        .pointer("/record_layout")
        .and_then(|layouts| layouts.get(id))
        .is_some_and(|layout| {
            layout
                .get("reads")
                .and_then(Value::as_object)
                .is_some_and(serde_json::Map::is_empty)
        })
}

/// P-closure findings for one decoded document and its fidelity.
///
/// A record of a named arena passes when a neutral record in its home links
/// it through `native_ref`; when a neutral record's `native_ref` names the
/// record its `owner` names and that neutral record's reads intersect its
/// reads; or when its layout holds no read (B1). No B1u builder exists, so
/// no record passes by a B1u proof.
#[must_use]
pub fn p_closure_findings(document: &Value, fidelity: &Value) -> Vec<String> {
    let mut linking: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
    if let Some(model) = document.get("model").and_then(Value::as_object) {
        for (arena, rows) in model {
            for row in rows.as_array().into_iter().flatten() {
                if let (Some(native), Some(id)) = (
                    row.get("native_ref").and_then(Value::as_str),
                    row.get("id").and_then(Value::as_str),
                ) {
                    linking
                        .entry(native)
                        .or_default()
                        .push((arena.as_str(), id));
                }
            }
        }
    }
    let mut findings = Vec::new();
    let Some(native) = document.get("native").and_then(Value::as_object) else {
        return findings;
    };
    for (namespace, arenas) in native {
        let Some(arenas) = arenas.as_object() else {
            continue;
        };
        for (arena, rows) in arenas {
            let Some(row) = named_home(namespace, arena) else {
                continue;
            };
            let mut used: BTreeMap<&str, &str> = BTreeMap::new();
            for record in rows.as_array().into_iter().flatten() {
                let Some(id) = record.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let direct = linking
                    .get(id)
                    .into_iter()
                    .flatten()
                    .find(|(home, _)| row.link != Link::Consumed && row.homes.contains(home));
                let owned = record
                    .get("owner")
                    .and_then(Value::as_str)
                    .and_then(|owner| linking.get(owner))
                    .into_iter()
                    .flatten()
                    .find(|(_, neutral)| {
                        intersect(&reads(fidelity, neutral), &reads(fidelity, id))
                    });
                let Some(&(_, neutral)) = direct.or(owned) else {
                    if !is_b1(fidelity, id) {
                        findings.push(format!(
                            "native.{namespace}.{arena} {id}: no neutral record in {:?} links it",
                            row.homes
                        ));
                    }
                    continue;
                };
                if row.link == Link::Own {
                    if let Some(previous) = used.insert(neutral, id) {
                        findings.push(format!(
                            "native.{namespace}.{arena} {id}: neutral record {neutral} also links {previous}"
                        ));
                    }
                }
            }
        }
    }
    findings
}

/// One `Native` variant: its neutral arena and its native kind.
pub type NativeKind = (String, String);

/// Every (neutral arena, native kind) pair of the `Native` variants a document holds.
#[must_use]
pub fn native_kinds(document: &Value) -> BTreeSet<NativeKind> {
    fn walk(arena: &str, value: &Value, found: &mut BTreeSet<NativeKind>) {
        match value {
            Value::Object(object) => {
                let tag = |key: &str| {
                    object
                        .get(key)
                        .and_then(Value::as_str)
                        .is_some_and(|tag| matches!(tag, "native" | "native_set"))
                };
                if tag("definition") {
                    let kind = object.get("kind").and_then(Value::as_str).unwrap_or("");
                    found.insert((arena.to_owned(), kind.to_owned()));
                } else if tag("kind") {
                    let kind = object
                        .get("native_kind")
                        .and_then(Value::as_str)
                        .unwrap_or("(reference)");
                    found.insert((arena.to_owned(), kind.to_owned()));
                }
                for child in object.values() {
                    walk(arena, child, found);
                }
            }
            Value::Array(items) => items.iter().for_each(|child| walk(arena, child, found)),
            _ => {}
        }
    }
    let mut found = BTreeSet::new();
    if let Some(model) = document.get("model").and_then(Value::as_object) {
        for (arena, rows) in model {
            walk(arena, rows, &mut found);
        }
    }
    found
}

/// Whether a codec issue names `Native` as the home of this pair.
///
/// #224 §7.11.1 names a `Native` feature for a `FreeCAD` object type outside
/// `App::` and `Gui::`; #225 for an unreferenced sheet-metal settings frame;
/// #228 for CATIA `design_objects` beyond the admitted classes.
#[must_use]
pub fn native_kind_justified(namespace: &str, (arena, kind): (&str, &str)) -> bool {
    match (namespace, arena) {
        ("fcstd", "features") => {
            kind.contains("::") && !kind.starts_with("App::") && !kind.starts_with("Gui::")
        }
        ("f3d", "features") => kind == "sheet_metal_settings",
        ("catia", "features") => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
