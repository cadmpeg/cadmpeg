// SPDX-License-Identifier: Apache-2.0
//! Carried sets for the W-closure-cross harness (§5.2, #232).
//!
//! For a writer X:
//! - `A_X`: the arenas X's decoder populates;
//! - `V_X`: the enum variants it constructs, as `Enum::Variant` strings;
//! - `P_X`: the JSON key paths it fills with non-null values, array indices
//!   collapsed.
//!
//! Variants are read syntactically from CADIR JSON through the generated
//! enum registry (`data/variant_registry.json`): externally-tagged values
//! contribute their key or bare spelling, internally and adjacently tagged
//! values contribute their tag value. A bare string attributes only a unit
//! variant, and a single-key object only a variant with a payload: a unit
//! variant never serializes as `{"Variant": ...}`, so a structural map such
//! as an edge carrier `{"curve": <id>}` attributes nothing. The same reading
//! serves observation (what X's decodes hold) and containment (what a record
//! holds), so the two sides can never disagree about a spelling. A tag value
//! claimed by several enums resolves to all of them; the ambiguity is
//! measured by [`ambiguity_sample`].

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde_json::Value;

const REGISTRY_JSON: &str = include_str!("data/variant_registry.json");
const TABLE_JSON: &str = include_str!("data/constructor_table.json");

/// What a writer carries across formats.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Carried {
    /// Registry field names of populated model arenas, plus
    /// `native.<namespace>.<arena>` for populated native arenas.
    pub arenas: BTreeSet<String>,
    /// `Enum::Variant` strings.
    pub variants: BTreeSet<String>,
    /// Collapsed JSON key paths with non-null values (`a.b.[].c`).
    pub paths: BTreeSet<String>,
    /// Supported edge carrier variants, when the writer declares them.
    pub edge_carriers: Option<BTreeSet<EdgeCarrierVariant>>,
}

/// The variants of the edge carrier's untagged wire representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeCarrierVariant {
    /// No curve or endpoints.
    Free,
    /// Endpoints without a curve.
    Endpoints,
    /// A curve without endpoints.
    Curve,
    /// A curve with endpoints.
    Bounded,
}

impl EdgeCarrierVariant {
    /// Identify the variant in an edge record's serialized carrier.
    #[must_use]
    pub fn from_record(record: &Value) -> Option<Self> {
        let carrier = record.get("carrier")?.as_object()?;
        let curve = carrier.get("curve").is_some_and(|value| !value.is_null());
        let range = carrier
            .get("param_range")
            .is_some_and(|value| !value.is_null());
        Some(match (curve, range) {
            (false, false) => Self::Free,
            (false, true) => Self::Endpoints,
            (true, false) => Self::Curve,
            (true, true) => Self::Bounded,
        })
    }
}

impl Carried {
    /// CADIR carries everything (§5.2): every arena and variant counts as
    /// carried, and paths are not projected.
    #[must_use]
    pub fn all() -> Self {
        Self {
            arenas: BTreeSet::from(["*".to_owned()]),
            variants: BTreeSet::from(["*".to_owned()]),
            paths: BTreeSet::from(["*".to_owned()]),
            edge_carriers: None,
        }
    }

    /// Whether the wildcard stands in for every arena, variant or path.
    #[must_use]
    pub fn is_all(&self) -> bool {
        self.arenas.contains("*") && self.variants.contains("*") && self.edge_carriers.is_none()
    }

    /// Whether an arena counts as carried: the wildcard carries all.
    #[must_use]
    pub fn carries_arena(&self, arena: &str) -> bool {
        self.arenas.contains("*") || self.arenas.contains(arena)
    }

    /// Whether a variant counts as carried: the wildcard carries all.
    #[must_use]
    pub fn carries_variant(&self, variant: &str) -> bool {
        self.variants.contains("*") || self.variants.contains(variant)
    }
}

#[derive(Debug, Clone)]
struct VariantMaps {
    /// Bare-string spelling to variants (unit variants).
    units: BTreeMap<String, Vec<String>>,
    /// Single-key object spelling to variants (externally tagged).
    keys: BTreeMap<String, Vec<String>>,
    /// (tag key, tag value) to variants (internally/adjacently tagged).
    tags: BTreeMap<(String, String), Vec<String>>,
}

fn variant_maps() -> &'static VariantMaps {
    static MAPS: OnceLock<VariantMaps> = OnceLock::new();
    MAPS.get_or_init(|| {
        let registry: serde_json::Value =
            serde_json::from_str(REGISTRY_JSON).expect("checked-in variant registry parses");
        let mut maps = VariantMaps {
            units: BTreeMap::new(),
            keys: BTreeMap::new(),
            tags: BTreeMap::new(),
        };
        let enums = registry
            .get("enums")
            .and_then(Value::as_object)
            .expect("registry holds enums");
        for (name, definition) in enums {
            let style = definition
                .get("style")
                .and_then(Value::as_str)
                .unwrap_or("external");
            let tag = definition.get("tag").and_then(Value::as_str);
            let variants = definition
                .get("variants")
                .and_then(Value::as_object)
                .expect("enum holds variants");
            let objects: Option<BTreeSet<String>> =
                definition
                    .get("object_variants")
                    .and_then(Value::as_array)
                    .map(|names| {
                        names
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    });
            for (variant, spelling) in variants {
                let spelling = spelling.as_str().unwrap_or(variant);
                let qualified = format!("{name}::{variant}");
                match style {
                    "untagged" => {}
                    "internal" | "adjacent" => {
                        if let Some(tag) = tag {
                            maps.tags
                                .entry((tag.to_owned(), spelling.to_owned()))
                                .or_default()
                                .push(qualified.clone());
                        }
                    }
                    _ => {
                        // Without the generated payload list (a stale
                        // registry), keep the legacy reading: every
                        // spelling attributes both forms.
                        let object = objects.as_ref().is_none_or(|set| set.contains(variant));
                        if objects.is_none() || !object {
                            maps.units
                                .entry(spelling.to_owned())
                                .or_default()
                                .push(qualified.clone());
                        }
                        if objects.is_none() || object {
                            maps.keys
                                .entry(spelling.to_owned())
                                .or_default()
                                .push(qualified);
                        }
                    }
                }
            }
        }
        maps
    })
}

/// Every `Enum::Variant` the JSON value exhibits through the registry.
#[must_use]
pub fn json_variants(value: &Value) -> BTreeSet<String> {
    let maps = variant_maps();
    let mut found = BTreeSet::new();
    walk_variants(value, maps, &mut found);
    found
}

fn walk_variants(value: &Value, maps: &VariantMaps, found: &mut BTreeSet<String>) {
    match value {
        Value::String(text) => {
            if let Some(variants) = maps.units.get(text) {
                found.extend(variants.iter().cloned());
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_variants(item, maps, found);
            }
        }
        Value::Object(fields) => {
            if fields.len() == 1 {
                if let Some((key, _)) = fields.iter().next() {
                    if let Some(variants) = maps.keys.get(key) {
                        found.extend(variants.iter().cloned());
                    }
                }
            }
            for (key, child) in fields {
                if let Value::String(text) = child {
                    if let Some(variants) = maps.tags.get(&(key.clone(), text.clone())) {
                        found.extend(variants.iter().cloned());
                    }
                }
                walk_variants(child, maps, found);
            }
        }
        _ => {}
    }
}

/// Registry entries whose spelling resolves to several variants, over one
/// document: evidence for the documented over-approximation.
#[must_use]
pub fn ambiguity_sample(document: &Value) -> BTreeMap<String, usize> {
    let maps = variant_maps();
    let mut ambiguous = BTreeMap::new();
    let mut check = |spelling: &str, count: usize| {
        if count > 1 {
            *ambiguous.entry(spelling.to_owned()).or_default() += 1;
        }
    };
    fn walk(value: &Value, maps: &VariantMaps, check: &mut dyn FnMut(&str, usize)) {
        match value {
            Value::String(text) => {
                if let Some(variants) = maps.units.get(text) {
                    check(text, variants.len());
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk(item, maps, check);
                }
            }
            Value::Object(fields) => {
                if fields.len() == 1 {
                    if let Some((key, _)) = fields.iter().next() {
                        if let Some(variants) = maps.keys.get(key) {
                            check(key, variants.len());
                        }
                    }
                }
                for (key, child) in fields {
                    if let Value::String(text) = child {
                        if let Some(variants) = maps.tags.get(&(key.clone(), text.clone())) {
                            check(&format!("{key}={text}"), variants.len());
                        }
                    }
                    walk(child, maps, check);
                }
            }
            _ => {}
        }
    }
    walk(document, maps, &mut check);
    ambiguous
}

/// Model arenas with at least one row, as registry field names, plus
/// `native.<namespace>.<arena>` for populated native arenas.
#[must_use]
pub fn observe_arenas(document: &Value) -> BTreeSet<String> {
    let mut arenas = BTreeSet::new();
    if let Some(model) = document.get("model").and_then(Value::as_object) {
        for (arena, rows) in model {
            if rows.as_array().is_some_and(|rows| !rows.is_empty()) {
                arenas.insert(arena.clone());
            }
        }
    }
    if let Some(native) = document.get("native").and_then(Value::as_object) {
        for (namespace, spaces) in native {
            for (arena, rows) in spaces.as_object().into_iter().flatten() {
                if rows.as_array().is_some_and(|rows| !rows.is_empty()) {
                    arenas.insert(format!("native.{namespace}.{arena}"));
                }
            }
        }
    }
    arenas
}

/// JSON key paths with non-null values, array indices collapsed to `[]`.
#[must_use]
pub fn observe_paths(document: &Value) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    walk_paths(document, &mut String::new(), &mut paths);
    paths
}

fn walk_paths(value: &Value, prefix: &mut String, paths: &mut BTreeSet<String>) {
    match value {
        Value::Null => {}
        Value::Array(items) => {
            let base = prefix.len();
            prefix.push_str("[]");
            for item in items {
                walk_paths(item, prefix, paths);
            }
            prefix.truncate(base);
        }
        Value::Object(fields) => {
            for (key, child) in fields {
                let base = prefix.len();
                if !prefix.is_empty() {
                    prefix.push('.');
                }
                prefix.push_str(key);
                if !matches!(child, Value::Null) {
                    paths.insert(prefix.clone());
                }
                walk_paths(child, prefix, paths);
                prefix.truncate(base);
            }
        }
        _ => {}
    }
}

/// Observed carried sets of decoded documents: arenas and paths from JSON,
/// variants from JSON through the registry.
#[must_use]
pub fn observe_carried(documents: &[Value]) -> Carried {
    let mut carried = Carried::default();
    for document in documents {
        carried.arenas.extend(observe_arenas(document));
        carried.paths.extend(observe_paths(document));
        carried.variants.extend(json_variants(document));
    }
    carried
}

/// The generated constructor table: constructor path to variants it builds.
#[must_use]
pub fn constructor_table() -> BTreeMap<String, Vec<String>> {
    serde_json::from_str(TABLE_JSON).expect("checked-in constructor table parses")
}

/// One codec crate's scan output, as checked in by the §5.2 scan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CrateScan {
    /// `Enum::Variant` strings the crate constructs outside the encode path.
    pub variants: BTreeSet<String>,
    /// Arena registry fields whose record types the crate constructs.
    pub arenas: BTreeSet<String>,
}

/// Reads a `scan_<package>.json` artifact written by the §5.2 scan.
pub fn crate_scan(scan_json: &str) -> CrateScan {
    let parsed: Value = serde_json::from_str(scan_json).expect("crate scan parses");
    let strings = |key: &str| {
        parsed
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    };
    CrateScan {
        variants: strings("variants"),
        arenas: strings("arenas"),
    }
}

#[cfg(test)]
mod tests {
    use super::{ambiguity_sample, json_variants, observe_arenas, observe_paths};
    use serde_json::json;

    #[test]
    fn registry_reads_all_three_tag_styles() {
        let document = json!({
            "model": {
                "faces": [{"id": "a", "sense": "forward"}],
                "surfaces": [{"id": "b", "geometry": {"kind": "plane"}}],
                "bodies": [{"id": "c", "kind": {"solid": {}}}],
            }
        });
        let variants = json_variants(&document);
        assert!(
            variants.contains("Sense::Forward")
                || variants
                    .iter()
                    .any(|variant| variant.ends_with("::Forward")),
            "{variants:?}"
        );
        assert!(
            variants.contains("SolvedSurfaceGeometry::Plane"),
            "{variants:?}"
        );
    }

    #[test]
    fn arenas_and_paths_observe_populated_rows_only() {
        let document = json!({
            "model": {
                "faces": [{"id": "a", "loop": serde_json::Value::Null}],
                "edges": [],
            },
            "native": {"step": {"drawing_targets": [{"id": "b"}]}},
        });
        let arenas = observe_arenas(&document);
        assert!(arenas.contains("faces"), "{arenas:?}");
        assert!(arenas.contains("native.step.drawing_targets"), "{arenas:?}");
        assert!(!arenas.contains("edges"), "{arenas:?}");
        let paths = observe_paths(&document);
        assert!(paths.contains("model.faces[].id"), "{paths:?}");
        assert!(!paths.iter().any(|path| path.contains("loop")), "{paths:?}");
    }

    #[test]
    fn ambiguity_reports_shared_spellings() {
        let _ = ambiguity_sample(&json!({"kind": "plane"}));
    }

    #[test]
    fn structural_single_key_maps_attribute_no_variant() {
        // An edge carrier is a struct, not an externally-tagged enum. The
        // release cross-closure misread `{"curve": <id>}` as
        // `VariableBlendSupportKind::Curve` (a unit variant, which never
        // serializes as a single-key object) and dropped every carried edge
        // from the required set: `step:data:edge#19` in ap203_sheet.p21,
        // `iges:model:edge#D55:D35:1` in explicit_tetrahedron_solid.igs, and
        // the coedges that need them.
        let carrier = json!({"carrier": {"curve": "step:data:curve#17"}});
        assert!(json_variants(&carrier).is_empty(), "{carrier:?}");
        let projected = json!({
            "carrier": {"curve": "iges:model:curve#D21"},
            "end": "iges:model:vertex#D55:D33:2",
            "id": "iges:model:edge#D55:D35:1",
            "start": "iges:model:vertex#D55:D33:1",
        });
        assert!(json_variants(&projected).is_empty(), "{projected:?}");
    }

    #[test]
    fn bare_unit_spellings_still_attribute() {
        assert!(json_variants(&json!("curve")).contains("VariableBlendSupportKind::Curve"));
        assert!(json_variants(&json!("forward")).contains("Sense::Forward"));
    }

    #[test]
    fn payload_single_key_objects_still_attribute() {
        // `ModelEvaluationIdentity::Curve` carries a payload, so its
        // single-key object form is genuine.
        let identity = json!({"Curve": {"lane": "model"}});
        assert!(
            json_variants(&identity).contains("ModelEvaluationIdentity::Curve"),
            "{identity:?}"
        );
    }
}
