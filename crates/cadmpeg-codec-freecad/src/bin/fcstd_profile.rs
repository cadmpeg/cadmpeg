// SPDX-License-Identifier: Apache-2.0
//! Generate deterministic machine-readable evidence from the public `FCStd` corpus.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use cadmpeg_codec_freecad::{
    validate_native, FcstdCodec, FcstdDocumentBuilder, FcstdPropertyOwner, FcstdPropertyValue,
    FcstdWriteOptions,
};
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions, Encoder};
use cadmpeg_ir::wire::hash::sha256_hex;
use cadmpeg_ir::{CadIr, Severity};
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
struct Profile {
    #[serde(rename = "profile_version")]
    version: u32,
    format: &'static str,
    manifest_sha256: String,
    manifest_verified: bool,
    envelope: Envelope,
    fixtures: Vec<FixtureProfile>,
    observed: Observed,
    source_less_write: SourceLessWriteProfile,
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
#[allow(clippy::struct_excessive_bools)]
struct FixtureProfile {
    filename: String,
    sha256: String,
    canonical_cadir_sha256: String,
    deterministic: bool,
    blocking_losses: usize,
    neutral_errors: usize,
    native_errors: usize,
    exact_byte_coverage: bool,
    write_deterministic: bool,
    semantic_round_trip: bool,
    side_entries_preserved: bool,
    typed_edit_round_trip: bool,
    entity_counts: BTreeMap<String, usize>,
}

#[derive(Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct SourceLessWriteProfile {
    generated: bool,
    deterministic: bool,
    decodes_cleanly: bool,
    object_type: String,
    typed_parameters: BTreeMap<String, String>,
    unsupported_target_rejected: bool,
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
    feature_branches: BTreeSet<String>,
    sketch_constraint_definitions: BTreeSet<String>,
    product_constructs: BTreeSet<String>,
    joint_kinds: BTreeSet<String>,
    document_variants: BTreeSet<String>,
    presentation_constructs: BTreeSet<String>,
    drawing_types: BTreeSet<String>,
    application_constructs: BTreeSet<String>,
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
        let mut first_write = Vec::new();
        FcstdCodec.encode(&first.ir, &mut first_write)?;
        let mut second_write = Vec::new();
        FcstdCodec.encode(&first.ir, &mut second_write)?;
        let written =
            FcstdCodec.decode(&mut Cursor::new(&first_write), &DecodeOptions::default())?;
        let semantic_round_trip =
            semantic_fingerprint(first.ir.clone())? == semantic_fingerprint(written.ir.clone())?;
        let side_entries_preserved =
            logical_side_entries(&first.ir)? == logical_side_entries(&written.ir)?;
        let mut edited_ir = first.ir.clone();
        FcstdCodec.set_property_value_attribute(
            &mut edited_ir,
            FcstdPropertyOwner::Document,
            "Label",
            0,
            "value",
            "cadmpeg L9 edit",
        )?;
        let mut edited_bytes = Vec::new();
        FcstdCodec.encode(&edited_ir, &mut edited_bytes)?;
        let edited =
            FcstdCodec.decode(&mut Cursor::new(edited_bytes), &DecodeOptions::default())?;
        let typed_edit_round_trip =
            property_value_attribute(&edited.ir, "fcstd:native:document#0", "Label", 0, "value")
                == Some("cadmpeg L9 edit".to_owned());
        namespace_version = Some(namespace.version);
        observed.native_arenas.extend(
            namespace
                .arenas
                .iter()
                .filter(|(_, records)| !records.is_empty())
                .map(|(name, _)| name.clone()),
        );
        let has_gui = namespace
            .arenas
            .get("gui_documents")
            .is_some_and(|records| !records.is_empty());
        observed
            .document_variants
            .insert(if has_gui { "gui" } else { "headless" }.to_owned());
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
            write_deterministic: first_write == second_write,
            semantic_round_trip,
            side_entries_preserved,
            typed_edit_round_trip,
            entity_counts: neutral.entity_counts,
        });
    }

    let manifest = fs::read(&manifest_path)?;
    verify_manifest(&manifest, &fixtures)?;
    let source_less_write = source_less_profile()?;
    let gates = gates(&fixtures, &observed, &total_counts, &source_less_write);
    let highest_passing_gate = gates
        .iter()
        .take_while(|gate| gate.passed)
        .last()
        .map(|gate| gate.level.clone());
    let profile = Profile {
        version: 4,
        format: "fcstd",
        manifest_sha256: sha256_hex(&manifest),
        manifest_verified: true,
        envelope: Envelope {
            container: "ZIP-packaged FCStd",
            schema_version: 4,
            file_version: 1,
            native_namespace_version: namespace_version.unwrap_or_default(),
            cadir_version: cadmpeg_ir::document::IR_VERSION,
            write_support: true,
        },
        fixtures,
        observed,
        source_less_write,
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
        if record.fields.get("inert_payload").and_then(Value::as_bool) == Some(true) {
            observed
                .application_constructs
                .insert("inert_payload".into());
        }
        if record
            .fields
            .get("property_records")
            .and_then(Value::as_array)
            .is_some_and(|properties| {
                properties.iter().any(|property| {
                    property
                        .get("payloads")
                        .and_then(Value::as_array)
                        .is_some_and(|payloads| !payloads.is_empty())
                })
            })
        {
            observed
                .application_constructs
                .insert("embedded_payload".into());
        }
    }
    for record in namespace.arenas.get("drawings").into_iter().flatten() {
        insert_string(&record.fields, "kind", &mut observed.drawing_types);
        if record
            .fields
            .get("side_entries")
            .and_then(Value::as_array)
            .is_some_and(|entries| !entries.is_empty())
        {
            observed
                .presentation_constructs
                .insert("drawing_asset".into());
        }
    }
    for record in namespace.arenas.get("gui_documents").into_iter().flatten() {
        if let Some(states) = record.fields.get("states").and_then(Value::as_array) {
            for state in states {
                if let Some(kind) = state.get("kind").and_then(Value::as_str) {
                    observed
                        .presentation_constructs
                        .insert(format!("gui_state:{kind}"));
                }
            }
        }
    }
    for record in namespace
        .arenas
        .get("gui_view_providers")
        .into_iter()
        .flatten()
    {
        if record
            .fields
            .get("expanded")
            .is_some_and(|value| !value.is_null())
        {
            observed.presentation_constructs.insert("tree_state".into());
        }
    }
    for record in namespace.arenas.get("gui_properties").into_iter().flatten() {
        if let Some(name) = record.fields.get("name").and_then(Value::as_str) {
            observed
                .presentation_constructs
                .insert(format!("view_property:{name}"));
        }
    }
    for record in namespace.arenas.get("entries").into_iter().flatten() {
        if record.fields.get("role").and_then(Value::as_str) == Some("thumbnail") {
            observed.presentation_constructs.insert("thumbnail".into());
        }
    }
    let product_nodes = namespace
        .arenas
        .get("product_nodes")
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    for record in &product_nodes {
        if let Some(Value::String(kind)) = record.fields.get("kind") {
            observed.product_constructs.insert(kind.clone());
        }
        if record
            .fields
            .get("element_count")
            .and_then(Value::as_i64)
            .is_some_and(|count| count > 1)
        {
            observed.product_constructs.insert("link_array".into());
        }
        if record
            .fields
            .get("external_document")
            .is_some_and(|value| !value.is_null())
        {
            observed
                .product_constructs
                .insert("external_occurrence".into());
        }
        if record
            .fields
            .get("linked_subelements")
            .and_then(Value::as_array)
            .is_some_and(|values| !values.is_empty())
        {
            observed
                .product_constructs
                .insert("linked_subelements".into());
        }
        let prototype = record.fields.get("prototype").and_then(Value::as_str);
        if prototype.is_some_and(|prototype| {
            product_nodes.iter().any(|candidate| {
                candidate.fields.get("object").and_then(Value::as_str) == Some(prototype)
                    && candidate.fields.get("kind").and_then(Value::as_str) == Some("occurrence")
            })
        }) {
            observed
                .product_constructs
                .insert("nested_occurrence".into());
        }
    }
    for record in namespace.arenas.get("joints").into_iter().flatten() {
        insert_string(&record.fields, "kind", &mut observed.joint_kinds);
        if record
            .fields
            .get("references")
            .and_then(Value::as_array)
            .is_some_and(|references| {
                references.iter().any(|reference| {
                    reference
                        .get("subelements")
                        .and_then(Value::as_array)
                        .is_some_and(|values| !values.is_empty())
                })
            })
        {
            observed
                .product_constructs
                .insert("persistent_joint_operands".into());
        }
        if record
            .fields
            .get("parameters")
            .and_then(Value::as_object)
            .is_some_and(|parameters| {
                parameters.contains_key("AngleMin") && parameters.contains_key("AngleMax")
            })
        {
            observed.product_constructs.insert("joint_limits".into());
        }
    }
    for feature in &ir.model.features {
        if let Ok(Value::Object(definition)) = serde_json::to_value(&feature.definition) {
            insert_string(&definition, "definition", &mut observed.feature_definitions);
            if let Some(Value::Object(operation)) = definition.get("operation") {
                insert_string(operation, "definition", &mut observed.feature_operations);
            }
            collect_feature_branches(
                &Value::Object(definition),
                None,
                "",
                &mut observed.feature_branches,
            );
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

fn collect_feature_branches(
    value: &Value,
    family: Option<&str>,
    path: &str,
    output: &mut BTreeSet<String>,
) {
    let Value::Object(fields) = value else {
        return;
    };
    let family = fields.get("definition").and_then(Value::as_str).or(family);
    for (name, child) in fields {
        if name == "definition" {
            continue;
        }
        let child_path = if path.is_empty() {
            name.clone()
        } else {
            format!("{path}.{name}")
        };
        if child.is_object() {
            collect_feature_branches(child, family, &child_path, output);
            continue;
        }
        if !matches!(
            name.as_str(),
            "kind"
                | "op"
                | "closed"
                | "solid"
                | "ruled"
                | "max_degree"
                | "outward"
                | "mode"
                | "join"
                | "resolve_intersections"
                | "allow_self_intersections"
                | "flip_direction"
                | "count"
                | "transition"
                | "transformation"
                | "x"
                | "y"
                | "z"
        ) {
            continue;
        }
        let Some(family) = family else {
            continue;
        };
        let scalar = match child {
            Value::String(value) => value.clone(),
            Value::Bool(value) => value.to_string(),
            Value::Number(value) => value.to_string(),
            _ => continue,
        };
        output.insert(format!("{family}:{child_path}={scalar}"));
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

fn semantic_fingerprint(mut ir: CadIr) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(source) = &mut ir.source {
        source.attributes.remove("physical_archive_bytes");
        source.attributes.remove("physical_ledger_spans");
    }
    if let Some(namespace) = ir.native.0.get_mut("fcstd") {
        namespace.arenas.remove("physical_ledger");
        namespace.arenas.remove("byte_coverage");
        namespace.arenas.remove("logical_ledger");
    }
    Ok(ir.to_canonical_json()?)
}

fn logical_side_entries(
    ir: &CadIr,
) -> Result<BTreeMap<String, String>, Box<dyn std::error::Error>> {
    let namespace = ir
        .native
        .namespace("fcstd")
        .ok_or("CADIR has no fcstd namespace")?;
    Ok(namespace
        .arenas
        .get("entries")
        .into_iter()
        .flatten()
        .filter_map(|record| {
            let name = record.fields.get("name")?.as_str()?;
            let digest = record.fields.get("sha256")?.as_str()?;
            Some((name.to_owned(), digest.to_owned()))
        })
        .collect())
}

fn property_value_attribute(
    ir: &CadIr,
    owner: &str,
    property_name: &str,
    value_order: usize,
    attribute: &str,
) -> Option<String> {
    ir.native
        .namespace("fcstd")?
        .arenas
        .get("properties")?
        .iter()
        .find(|record| {
            record.fields.get("owner").and_then(Value::as_str) == Some(owner)
                && record.fields.get("name").and_then(Value::as_str) == Some(property_name)
        })?
        .fields
        .get("values")?
        .as_array()?
        .iter()
        .find(|value| value.get("order").and_then(Value::as_u64) == Some(value_order as u64))?
        .get("attributes")?
        .get(attribute)?
        .as_str()
        .map(str::to_owned)
}

fn source_less_profile() -> Result<SourceLessWriteProfile, Box<dyn std::error::Error>> {
    let mut builder = FcstdDocumentBuilder::new("FCStd L9 source-less evidence");
    builder
        .add_object("Box", "Part::Box")?
        .add_property(
            "Box",
            "Length",
            "App::PropertyLength",
            vec![FcstdPropertyValue::attribute("Float", "value", "12.5")],
        )?
        .add_property(
            "Box",
            "Width",
            "App::PropertyLength",
            vec![FcstdPropertyValue::attribute("Float", "value", "7")],
        )?
        .add_property(
            "Box",
            "Height",
            "App::PropertyLength",
            vec![FcstdPropertyValue::attribute("Float", "value", "3")],
        )?;
    let ir = builder.build()?;
    let mut first = Vec::new();
    FcstdCodec.encode(&ir, &mut first)?;
    let mut second = Vec::new();
    FcstdCodec.encode(&ir, &mut second)?;
    let decoded = FcstdCodec.decode(&mut Cursor::new(&first), &DecodeOptions::default())?;
    let namespace = decoded
        .ir
        .native
        .namespace("fcstd")
        .ok_or("generated CADIR has no fcstd namespace")?;
    let object_type = namespace
        .arenas
        .get("objects")
        .into_iter()
        .flatten()
        .find_map(|record| record.fields.get("type_name").and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned();
    let object_owner = "fcstd:native:object#Box";
    let typed_parameters = ["Length", "Width", "Height"]
        .into_iter()
        .filter_map(|name| {
            property_value_attribute(&decoded.ir, object_owner, name, 0, "value")
                .map(|value| (name.to_owned(), value))
        })
        .collect();
    let unsupported_target_rejected = FcstdCodec
        .encode_with_options(
            &ir,
            &mut Vec::new(),
            FcstdWriteOptions {
                schema_version: 3,
                file_version: 1,
            },
        )
        .is_err();
    Ok(SourceLessWriteProfile {
        generated: true,
        deterministic: first == second,
        decodes_cleanly: validate_native(&decoded.ir).is_empty(),
        object_type,
        typed_parameters,
        unsupported_target_rejected,
    })
}

fn gates(
    fixtures: &[FixtureProfile],
    observed: &Observed,
    counts: &BTreeMap<String, usize>,
    source_less_write: &SourceLessWriteProfile,
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
        "shell",
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
    let required_feature_branches = BTreeSet::from([
        "chamfer:spec.kind=two_distances",
        "combine:operation.op=cut",
        "datum_axis:origin.x=2.0",
        "datum_coordinate_system:origin.z=9.0",
        "datum_plane:origin.z=4.0",
        "datum_point:position.z=6.0",
        "draft:operation.outward=true",
        "extrude:extent.kind=symmetric",
        "fillet:radius.kind=constant",
        "hole:operation.bottom.kind=angled",
        "hole:operation.kind.kind=countersink",
        "loft:max_degree=4",
        "loft:ruled=true",
        "mirror_shape:plane_normal.x=1.0",
        "pattern:operation.pattern.count=4",
        "pattern:operation.pattern.direction.z=-1.0",
        "primitive:solid.kind=box",
        "primitive:solid.kind=cylinder",
        "revolve:construction.extent.kind=symmetric_angle",
        "revolve:construction.solid=false",
        "shell:resolve_intersections=true",
        "sweep:orientation.kind=frenet",
    ]);
    let feature_branches = observed
        .feature_branches
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let required_product_constructs = BTreeSet::from([
        "part",
        "group",
        "link_group",
        "occurrence",
        "nested_occurrence",
        "link_array",
        "external_occurrence",
        "persistent_joint_operands",
        "joint_limits",
    ]);
    let product_constructs = observed
        .product_constructs
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let required_joint_kinds = BTreeSet::from(["grounded", "Revolute"]);
    let joint_kinds = observed
        .joint_kinds
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let required_document_variants = BTreeSet::from(["gui", "headless"]);
    let document_variants = observed
        .document_variants
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let required_presentation_constructs = BTreeSet::from([
        "drawing_asset",
        "gui_state:Camera",
        "thumbnail",
        "tree_state",
        "view_property:DisplayMode",
        "view_property:SelectionStyle",
        "view_property:Visibility",
    ]);
    let presentation_constructs = observed
        .presentation_constructs
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let required_drawing_types = BTreeSet::from([
        "TechDraw::DrawPage",
        "TechDraw::DrawSVGTemplate",
        "TechDraw::DrawViewAnnotation",
        "TechDraw::DrawViewDimension",
        "TechDraw::DrawViewPart",
        "TechDraw::DrawViewSymbol",
    ]);
    let drawing_types = observed
        .drawing_types
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let required_application_types = BTreeSet::from([
        "App::FeaturePython",
        "Fem::ConstraintTemperature",
        "Fem::FemAnalysis",
        "Mesh::Feature",
        "Path::Feature",
        "Points::Feature",
        "Spreadsheet::Sheet",
    ]);
    let application_types = observed
        .application_types
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let required_application_constructs = BTreeSet::from(["embedded_payload", "inert_payload"]);
    let application_constructs = observed
        .application_constructs
        .iter()
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
                    format!("{carriers:?}"),
                    format!("{required_carriers:?}"),
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
                    format!("{topology:?}"),
                    format!("{required_topology:?}"),
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
                    required_feature_branches.is_subset(&feature_branches),
                    format!("{feature_branches:?}"),
                    format!("{required_feature_branches:?}"),
                ),
            ],
        ),
        gate(
            "L7",
            vec![
                assertion(
                    "product_structure_matrix",
                    count("components") > 0
                        && count("occurrences") > 0
                        && required_product_constructs.is_subset(&product_constructs),
                    format!(
                        "components={}, occurrences={}, constructs={product_constructs:?}",
                        count("components"),
                        count("occurrences")
                    ),
                    format!("{required_product_constructs:?}"),
                ),
                assertion(
                    "assembly_joint_matrix",
                    count("assembly_joints") > 0 && required_joint_kinds.is_subset(&joint_kinds),
                    format!("joints={}, kinds={joint_kinds:?}", count("assembly_joints")),
                    format!("{required_joint_kinds:?}"),
                ),
            ],
        ),
        gate(
            "L8",
            vec![
                assertion(
                    "presentation_variants",
                    required_document_variants.is_subset(&document_variants)
                        && count("appearance_bindings") > 0
                        && required_presentation_constructs.is_subset(&presentation_constructs),
                    format!(
                        "documents={document_variants:?}, appearances={}, constructs={presentation_constructs:?}",
                        count("appearance_bindings")
                    ),
                    format!(
                        "documents={required_document_variants:?}, constructs={required_presentation_constructs:?}, appearances"
                    ),
                ),
                assertion(
                    "drawing_annotation_matrix",
                    count("drawings") > 0
                        && count("semantic_annotations") > 0
                        && required_drawing_types.is_subset(&drawing_types),
                    format!(
                        "drawings={}, semantic_annotations={}, types={drawing_types:?}",
                        count("drawings"),
                        count("semantic_annotations")
                    ),
                    format!("types={required_drawing_types:?}, drawings and annotations"),
                ),
                assertion(
                    "application_retention",
                    observed.native_arenas.contains("applications")
                        && required_application_types.is_subset(&application_types)
                        && required_application_constructs.is_subset(&application_constructs)
                        && exact,
                    format!(
                        "types={application_types:?}, constructs={application_constructs:?}, exact={exact}"
                    ),
                    format!(
                        "types={required_application_types:?}, constructs={required_application_constructs:?}, exact byte coverage"
                    ),
                ),
            ],
        ),
        gate(
            "L9",
            vec![
                assertion(
                    "semantic_native_round_trip",
                    fixtures.iter().all(|fixture| {
                        fixture.write_deterministic
                            && fixture.semantic_round_trip
                            && fixture.typed_edit_round_trip
                    }),
                    format!(
                        "{} of {} fixtures deterministic, semantically equivalent, and editable",
                        fixtures
                            .iter()
                            .filter(|fixture| fixture.write_deterministic
                                && fixture.semantic_round_trip
                                && fixture.typed_edit_round_trip)
                            .count(),
                        fixtures.len()
                    ),
                    "every primary-envelope fixture writes deterministically, round-trips semantically, and accepts a typed edit",
                ),
                assertion(
                    "unsupported_record_survival",
                    fixtures
                        .iter()
                        .all(|fixture| fixture.side_entries_preserved),
                    format!(
                        "{} of {} fixture entry sets preserved",
                        fixtures
                            .iter()
                            .filter(|fixture| fixture.side_entries_preserved)
                            .count(),
                        fixtures.len()
                    ),
                    "every named logical entry retains identity and digest",
                ),
                assertion(
                    "source_less_and_target_selection",
                    source_less_write.generated
                        && source_less_write.deterministic
                        && source_less_write.decodes_cleanly
                        && source_less_write.object_type == "Part::Box"
                        && source_less_write.typed_parameters.len() == 3
                        && source_less_write.unsupported_target_rejected,
                    format!(
                        "generated={}, deterministic={}, clean={}, type={}, parameters={:?}, unsupported_target_rejected={}",
                        source_less_write.generated,
                        source_less_write.deterministic,
                        source_less_write.decodes_cleanly,
                        source_less_write.object_type,
                        source_less_write.typed_parameters,
                        source_less_write.unsupported_target_rejected
                    ),
                    "deterministic clean source-less Part::Box with three typed dimensions and explicit unsupported-target rejection",
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

#[allow(clippy::needless_pass_by_value)]
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
