// SPDX-License-Identifier: Apache-2.0
//! Generate deterministic machine-readable evidence from the public FCStd corpus.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use cadmpeg_codec_freecad::{validate_native, FcstdCodec};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::{CadIr, Severity};
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
struct Profile {
    profile_version: u32,
    format: &'static str,
    manifest_sha256: String,
    manifest_verified: bool,
    envelope: Envelope,
    fixtures: Vec<FixtureProfile>,
    observed: Observed,
    gates: Vec<Gate>,
    highest_passing_gate: Option<String>,
}

#[derive(Serialize)]
struct Envelope {
    container: &'static str,
    schema_version: u32,
    file_version: u32,
    native_namespace_version: u32,
    cadir_version: &'static str,
    write_support: bool,
}

#[derive(Serialize)]
struct FixtureProfile {
    filename: String,
    sha256: String,
    canonical_cadir_sha256: String,
    deterministic: bool,
    blocking_losses: usize,
    neutral_errors: usize,
    native_errors: usize,
    exact_byte_coverage: bool,
    entity_counts: BTreeMap<String, usize>,
}

#[derive(Default, Serialize)]
struct Observed {
    shape_forms: BTreeSet<String>,
    curves_2d: BTreeSet<String>,
    curves_3d: BTreeSet<String>,
    surfaces: BTreeSet<String>,
    topology: BTreeSet<String>,
    feature_definitions: BTreeSet<String>,
    feature_operations: BTreeSet<String>,
    sketch_constraint_definitions: BTreeSet<String>,
    application_types: BTreeSet<String>,
    native_arenas: BTreeSet<String>,
    neutral_arenas: BTreeSet<String>,
}

#[derive(Serialize)]
struct Gate {
    level: String,
    passed: bool,
    assertions: Vec<Assertion>,
}

#[derive(Serialize)]
struct Assertion {
    id: &'static str,
    passed: bool,
    observed: String,
    required: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args_os().skip(1);
    let fixture_directory = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: fcstd-profile FIXTURE_DIRECTORY MANIFEST OUTPUT_JSON")?;
    let manifest_path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: fcstd-profile FIXTURE_DIRECTORY MANIFEST OUTPUT_JSON")?;
    let output = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: fcstd-profile FIXTURE_DIRECTORY MANIFEST OUTPUT_JSON")?;
    if arguments.next().is_some() {
        return Err("usage: fcstd-profile FIXTURE_DIRECTORY MANIFEST OUTPUT_JSON".into());
    }

    let mut paths = fs::read_dir(&fixture_directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("fcstd"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    if paths.is_empty() {
        return Err("fixture directory contains no FCStd files".into());
    }

    let mut fixtures = Vec::new();
    let mut observed = Observed::default();
    let mut total_counts = BTreeMap::<String, usize>::new();
    let mut namespace_version = None;
    for path in paths {
        let bytes = fs::read(&path)?;
        let first = FcstdCodec.decode(&mut Cursor::new(&bytes), &DecodeOptions::default())?;
        let second = FcstdCodec.decode(&mut Cursor::new(&bytes), &DecodeOptions::default())?;
        let canonical = first.ir.to_canonical_json()?;
        let deterministic = canonical == second.ir.to_canonical_json()?;
        let neutral = cadmpeg_ir::validate(&first.ir, Vec::new());
        let native = validate_native(&first.ir);
        let namespace = first
            .ir
            .native
            .namespace("fcstd")
            .ok_or("decoded fixture has no fcstd namespace")?;
        namespace_version = Some(namespace.version);
        observed.native_arenas.extend(
            namespace
                .arenas
                .iter()
                .filter(|(_, records)| !records.is_empty())
                .map(|(name, _)| name.clone()),
        );
        collect_native_observations(&first.ir, &mut observed);
        for (name, count) in &neutral.entity_counts {
            *total_counts.entry(name.clone()).or_default() += count;
            if *count > 0 && !name.starts_with("native.") {
                observed.neutral_arenas.insert(name.clone());
            }
        }
        fixtures.push(FixtureProfile {
            filename: file_name(&path)?,
            sha256: sha256_hex(&bytes),
            canonical_cadir_sha256: sha256_hex(canonical.as_bytes()),
            deterministic,
            blocking_losses: first
                .report
                .losses
                .iter()
                .filter(|loss| loss.severity >= Severity::Blocking)
                .count(),
            neutral_errors: neutral
                .findings
                .iter()
                .filter(|finding| finding.severity >= Severity::Error)
                .count(),
            native_errors: native
                .iter()
                .filter(|finding| finding.severity >= Severity::Error)
                .count(),
            exact_byte_coverage: exact_byte_coverage(&first.ir),
            entity_counts: neutral.entity_counts,
        });
    }

    let manifest = fs::read(&manifest_path)?;
    verify_manifest(&manifest, &fixtures)?;
    let gates = gates(&fixtures, &observed, &total_counts);
    let highest_passing_gate = gates
        .iter()
        .take_while(|gate| gate.passed)
        .last()
        .map(|gate| gate.level.clone());
    let profile = Profile {
        profile_version: 2,
        format: "fcstd",
        manifest_sha256: sha256_hex(&manifest),
        manifest_verified: true,
        envelope: Envelope {
            container: "ZIP-packaged FCStd",
            schema_version: 4,
            file_version: 1,
            native_namespace_version: namespace_version.unwrap_or_default(),
            cadir_version: cadmpeg_ir::document::IR_VERSION,
            write_support: false,
        },
        fixtures,
        observed,
        gates,
        highest_passing_gate,
    };
    let mut json = serde_json::to_string_pretty(&profile)?;
    json.push('\n');
    fs::write(output, json)?;
    Ok(())
}

fn verify_manifest(
    manifest: &[u8],
    fixtures: &[FixtureProfile],
) -> Result<(), Box<dyn std::error::Error>> {
    let text = std::str::from_utf8(manifest)?;
    let mut entries = Vec::<(String, String, String)>::new();
    let mut current = BTreeMap::<&str, String>::new();
    for line in text.lines().map(str::trim) {
        if line == "[[file]]" {
            retain_fcstd_manifest_entry(&mut entries, &current);
            current.clear();
            continue;
        }
        for name in ["filename", "format", "sha256"] {
            let prefix = format!("{name} = \"");
            if let Some(value) = line
                .strip_prefix(&prefix)
                .and_then(|value| value.strip_suffix('"'))
            {
                current.insert(name, value.to_owned());
            }
        }
    }
    retain_fcstd_manifest_entry(&mut entries, &current);
    entries.sort();
    let mut actual = fixtures
        .iter()
        .map(|fixture| {
            (
                fixture.filename.clone(),
                fixture.sha256.clone(),
                "fcstd".to_owned(),
            )
        })
        .collect::<Vec<_>>();
    actual.sort();
    if entries != actual {
        return Err(format!(
            "FCStd manifest entries do not match fixture bytes\nmanifest: {entries:#?}\nactual: {actual:#?}"
        )
        .into());
    }
    Ok(())
}

fn retain_fcstd_manifest_entry(
    entries: &mut Vec<(String, String, String)>,
    fields: &BTreeMap<&str, String>,
) {
    if fields.get("format").map(String::as_str) != Some("fcstd") {
        return;
    }
    let Some(filename) = fields.get("filename") else {
        return;
    };
    let Some(sha256) = fields.get("sha256") else {
        return;
    };
    let basename = Path::new(filename)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(filename)
        .to_owned();
    entries.push((basename, sha256.clone(), "fcstd".to_owned()));
}

fn file_name(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    Ok(path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("fixture path has no UTF-8 filename")?
        .to_owned())
}

fn collect_native_observations(ir: &CadIr, observed: &mut Observed) {
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return;
    };
    for record in namespace.arenas.get("carrier_census").into_iter().flatten() {
        insert_string(&record.fields, "form", &mut observed.shape_forms);
        insert_map_keys(&record.fields, "curves_2d", &mut observed.curves_2d);
        insert_map_keys(&record.fields, "curves_3d", &mut observed.curves_3d);
        insert_map_keys(&record.fields, "surfaces", &mut observed.surfaces);
        insert_map_keys(&record.fields, "topology", &mut observed.topology);
    }
    for record in namespace.arenas.get("applications").into_iter().flatten() {
        insert_string(&record.fields, "type_name", &mut observed.application_types);
    }
    for feature in &ir.model.features {
        if let Ok(Value::Object(definition)) = serde_json::to_value(&feature.definition) {
            insert_string(&definition, "definition", &mut observed.feature_definitions);
            if let Some(Value::Object(operation)) = definition.get("operation") {
                insert_string(operation, "definition", &mut observed.feature_operations);
            }
        }
    }
    for constraint in &ir.model.sketch_constraints {
        if let Ok(Value::Object(definition)) = serde_json::to_value(&constraint.definition) {
            insert_string(
                &definition,
                "kind",
                &mut observed.sketch_constraint_definitions,
            );
        }
    }
}

fn insert_string(
    fields: &serde_json::Map<String, Value>,
    name: &str,
    output: &mut BTreeSet<String>,
) {
    if let Some(Value::String(value)) = fields.get(name) {
        output.insert(value.clone());
    }
}

fn insert_map_keys(
    fields: &serde_json::Map<String, Value>,
    name: &str,
    output: &mut BTreeSet<String>,
) {
    if let Some(Value::Object(values)) = fields.get(name) {
        output.extend(values.keys().cloned());
    }
}

fn exact_byte_coverage(ir: &CadIr) -> bool {
    ir.native
        .namespace("fcstd")
        .and_then(|namespace| namespace.arenas.get("byte_coverage"))
        .is_some_and(|records| {
            records.len() == 1
                && records[0].fields.get("exact").and_then(Value::as_bool) == Some(true)
        })
}

fn gates(
    fixtures: &[FixtureProfile],
    observed: &Observed,
    counts: &BTreeMap<String, usize>,
) -> Vec<Gate> {
    let clean = fixtures.iter().all(|fixture| {
        fixture.deterministic
            && fixture.blocking_losses == 0
            && fixture.neutral_errors == 0
            && fixture.native_errors == 0
    });
    let exact = fixtures.iter().all(|fixture| fixture.exact_byte_coverage);
    let count = |name: &str| counts.get(name).copied().unwrap_or_default();
    let required_carriers = BTreeSet::from([
        "circle", "ellipse", "line", "nurbs", "plane", "cylinder", "cone", "sphere", "torus",
        "trimmed",
    ]);
    let carriers = observed
        .curves_3d
        .union(&observed.surfaces)
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let required_topology = BTreeSet::from([
        "compound", "solid", "shell", "face", "wire", "edge", "vertex",
    ]);
    let topology = observed
        .topology
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let required_constraints = BTreeSet::from([
        "disabled",
        "coincident_loci",
        "point_on_object",
        "horizontal",
        "vertical",
        "parallel",
        "perpendicular",
        "tangent",
        "equal",
        "fixed",
        "distance",
        "distance_loci",
        "horizontal_distance",
        "vertical_distance",
        "angle",
        "radius",
        "diameter",
        "snells_law",
        "weight",
        "symmetric",
        "internal_alignment",
        "group",
        "text",
    ]);
    let constraint_definitions = observed
        .sketch_constraint_definitions
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let required_design_definitions = BTreeSet::from([
        "sketch",
        "datum_plane",
        "datum_axis",
        "datum_point",
        "datum_coordinate_system",
        "primitive",
        "extrude",
        "revolve",
        "sweep",
        "loft",
        "fillet",
        "chamfer",
        "thicken",
        "draft",
        "combine",
        "hole",
        "pattern",
        "mirror_shape",
    ]);
    let design_definitions = observed
        .feature_definitions
        .union(&observed.feature_operations)
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut gates = vec![
        gate(
            "L0",
            vec![
                assertion(
                    "clean_decode",
                    clean,
                    clean,
                    "all fixtures decode deterministically and validate",
                ),
                assertion(
                    "physical_coverage",
                    exact,
                    exact,
                    "exact physical and logical byte partitions",
                ),
            ],
        ),
        gate(
            "L1",
            vec![
                assertion(
                    "application_graph",
                    observed.native_arenas.contains("objects")
                        && observed.native_arenas.contains("properties"),
                    format!("{:?}", observed.native_arenas),
                    "objects and properties arenas",
                ),
                assertion(
                    "asset_inventory",
                    observed.native_arenas.contains("entries"),
                    format!("{:?}", observed.native_arenas),
                    "entries arena",
                ),
            ],
        ),
        gate(
            "L2",
            vec![
                assertion(
                    "text_and_binary_shapes",
                    observed.shape_forms.contains("text")
                        && observed.shape_forms.contains("binary"),
                    format!("{:?}", observed.shape_forms),
                    "text and binary",
                ),
                assertion(
                    "carrier_families",
                    required_carriers.is_subset(&carriers),
                    format!("{:?}", carriers),
                    format!("{:?}", required_carriers),
                ),
                assertion(
                    "application_geometry",
                    count("tessellations") > 0 && count("points") > 0,
                    format!(
                        "tessellations={}, points={}",
                        count("tessellations"),
                        count("points")
                    ),
                    "mesh tessellation and point data",
                ),
            ],
        ),
        gate(
            "L3",
            vec![
                assertion(
                    "connected_topology",
                    required_topology.is_subset(&topology),
                    format!("{:?}", topology),
                    format!("{:?}", required_topology),
                ),
                assertion(
                    "persistent_names",
                    observed.native_arenas.contains("element_maps"),
                    format!("{:?}", observed.native_arenas),
                    "element_maps arena",
                ),
            ],
        ),
        gate(
            "L4",
            vec![
                assertion(
                    "design_records",
                    count("features") > 0
                        && count("sketches") > 0
                        && count("sketch_constraints") > 0
                        && count("parameters") > 0,
                    format!(
                        "features={}, sketches={}, constraints={}, parameters={}",
                        count("features"),
                        count("sketches"),
                        count("sketch_constraints"),
                        count("parameters")
                    ),
                    "nonempty feature, sketch, constraint, and parameter graphs",
                ),
                assertion(
                    "operation_semantics",
                    observed.feature_operations.contains("extrude")
                        && (observed.feature_operations.contains("fillet")
                            || observed.feature_operations.contains("combine")),
                    format!("{:?}", observed.feature_operations),
                    "extrusion plus subtractive or dress-up operation",
                ),
            ],
        ),
        gate(
            "L5",
            vec![assertion(
                "appearance_coverage",
                count("appearance_bindings") > 0,
                count("appearance_bindings"),
                "object and subshape appearance bindings",
            )],
        ),
        gate(
            "L6",
            vec![
                assertion(
                    "complete_constraint_matrix",
                    required_constraints.is_subset(&constraint_definitions)
                        && !constraint_definitions.contains("native"),
                    format!("{constraint_definitions:?}"),
                    format!("{required_constraints:?}; no native fallback"),
                ),
                assertion(
                    "core_operation_families",
                    required_design_definitions.is_subset(&design_definitions),
                    format!("{design_definitions:?}"),
                    format!("{required_design_definitions:?}"),
                ),
                assertion(
                    "non_default_operation_branches",
                    false,
                    "public operation-branch matrix incomplete",
                    "each supported core operation family's non-default semantic branches",
                ),
            ],
        ),
        gate(
            "L7",
            vec![assertion(
                "product_and_joints",
                count("components") > 0 && count("occurrences") > 0 && count("assembly_joints") > 0,
                format!(
                    "components={}, occurrences={}, joints={}",
                    count("components"),
                    count("occurrences"),
                    count("assembly_joints")
                ),
                "components, occurrences, and joints",
            )],
        ),
        gate(
            "L8",
            vec![
                assertion(
                    "presentation",
                    count("drawings") > 0 && count("semantic_annotations") > 0,
                    format!(
                        "drawings={}, semantic_annotations={}",
                        count("drawings"),
                        count("semantic_annotations")
                    ),
                    "drawing and semantic annotation graphs",
                ),
                assertion(
                    "gui_state",
                    observed.native_arenas.contains("gui_documents"),
                    format!("{:?}", observed.native_arenas),
                    "GUI document state",
                ),
                assertion(
                    "application_retention",
                    observed.native_arenas.contains("applications") && exact,
                    format!(
                        "application types={:?}, exact={exact}",
                        observed.application_types
                    ),
                    "application records and exact byte coverage",
                ),
            ],
        ),
    ];
    let mut cumulative = true;
    for gate in &mut gates {
        cumulative &= gate.passed;
        gate.passed = cumulative;
    }
    gates
}

fn assertion(
    id: &'static str,
    passed: bool,
    observed: impl ToString,
    required: impl ToString,
) -> Assertion {
    Assertion {
        id,
        passed,
        observed: observed.to_string(),
        required: required.to_string(),
    }
}

fn gate(level: &str, assertions: Vec<Assertion>) -> Gate {
    Gate {
        level: level.to_owned(),
        passed: assertions.iter().all(|assertion| assertion.passed),
        assertions,
    }
}
