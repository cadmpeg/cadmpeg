// SPDX-License-Identifier: Apache-2.0
//! Generate deterministic machine-readable evidence from the public `FCStd` corpus.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use cadmpeg_codec_freecad::{
    FcstdCodec, FcstdDocumentBuilder, FcstdPropertyOwner, FcstdPropertyValue,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_ir::appearance::AppearanceTarget;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::hash::{sha256_hex, DOCUMENT_LOCAL_DIGEST_ATTRIBUTE};
use cadmpeg_ir::{report::Severity, CadIr};
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
    #[serde(serialize_with = "serialize_gates")]
    gates: Vec<Gate>,
    highest_passing_gate: Option<String>,
}

#[derive(Serialize)]
struct Envelope {
    container: &'static str,
    schema_version: u32,
    file_version: u32,
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
    #[serde(flatten)]
    write: FixtureWriteFlags,
    side_entries_preserved: bool,
    #[serde(flatten)]
    semantic_checks: FixtureSemanticChecks,
    #[serde(flatten)]
    native_checks: FixtureNativeChecks,
    entity_counts: BTreeMap<String, usize>,
}

#[derive(Serialize)]
struct FixtureSemanticChecks {
    appearance_ownership_and_precedence: bool,
    design_census_matches_projection: bool,
    design_property_edit_round_trip: bool,
}

#[derive(Serialize)]
struct FixtureNativeChecks {
    native_semantic_equivalence_verified: bool,
    native_recomputed_design_verified: bool,
}

#[derive(Serialize)]
struct FixtureWriteFlags {
    write_deterministic: bool,
    semantic_round_trip: bool,
    typed_edit_round_trip: bool,
}

#[derive(Serialize)]
struct SourceLessWriteProfile {
    #[serde(flatten)]
    generation: SourceLessGenerationFlags,
    object_type: String,
    typed_parameters: BTreeMap<String, String>,
    unsupported_target_rejected: bool,
    independent_native_acceptance: bool,
}

#[derive(Serialize)]
struct SourceLessGenerationFlags {
    generated: bool,
    deterministic: bool,
    decodes_cleanly: bool,
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
    appearance_targets: BTreeSet<String>,
}

struct Gate {
    level: String,
    assertions: Vec<Assertion>,
}

impl Gate {
    fn assertions_passed(&self) -> bool {
        self.assertions.iter().all(|assertion| assertion.passed)
    }
}

fn serialize_gates<S: serde::Serializer>(gates: &[Gate], serializer: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;

    #[derive(Serialize)]
    struct GateWire<'a> {
        level: &'a str,
        passed: bool,
        assertions: &'a [Assertion],
    }

    let mut sequence = serializer.serialize_seq(Some(gates.len()))?;
    let mut reached = true;
    for gate in gates {
        reached &= gate.assertions_passed();
        sequence.serialize_element(&GateWire {
            level: &gate.level,
            passed: reached,
            assertions: &gate.assertions,
        })?;
    }
    sequence.end()
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

    let native_report = env::var_os("CADMPEG_FCSTD_NATIVE_REPORT")
        .map(fs::read)
        .transpose()?
        .map(|bytes| serde_json::from_slice::<Value>(&bytes))
        .transpose()?;
    let mut fixtures = Vec::new();
    let mut observed = Observed::default();
    let mut total_counts = BTreeMap::<String, usize>::new();
    for path in paths {
        let bytes = fs::read(&path)?;
        let first = FcstdCodec.decode(&mut Cursor::new(&bytes), &DecodeOptions::default())?;
        let second = FcstdCodec.decode(&mut Cursor::new(&bytes), &DecodeOptions::default())?;
        let canonical = first.ir().to_canonical_json()?;
        let deterministic = canonical == second.ir().to_canonical_json()?;
        let neutral = cadmpeg_ir::validate_neutral(first.ir(), Vec::new())?;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())?;
        let native = FcstdCodec.validate_native(&ctx, first.ir(), None)?;
        let namespace = first
            .ir()
            .native
            .namespace("fcstd")
            .ok_or("decoded fixture has no fcstd namespace")?;
        let mut first_write = Vec::new();
        FcstdCodec
            .plan(EncodeInput::new(first.ir(), None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut first_write))?;
        let mut second_write = Vec::new();
        FcstdCodec
            .plan(EncodeInput::new(first.ir(), None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut second_write))?;
        let written =
            FcstdCodec.decode(&mut Cursor::new(&first_write), &DecodeOptions::default())?;
        let semantic_round_trip = semantic_fingerprint(first.ir().clone())?
            == semantic_fingerprint(written.ir().clone())?;
        let side_entries_preserved =
            logical_side_entries(first.ir())? == logical_side_entries(written.ir())?;
        let mut edited_ir = first.ir().clone();
        FcstdCodec.set_property_value_attribute(
            &mut edited_ir,
            FcstdPropertyOwner::Document,
            "Label",
            0,
            "value",
            "cadmpeg L9 edit",
        )?;
        let mut edited_bytes = Vec::new();
        FcstdCodec
            .plan(EncodeInput::new(&edited_ir, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut edited_bytes))?;
        let edited =
            FcstdCodec.decode(&mut Cursor::new(edited_bytes), &DecodeOptions::default())?;
        let typed_edit_round_trip =
            property_value_attribute(edited.ir(), "fcstd:native:document#0", "Label", 0, "value")
                == Some("cadmpeg L9 edit".to_owned());
        observed.native_arenas.extend(
            namespace
                .arenas()
                .iter()
                .filter(|(_, records)| !records.is_empty())
                .map(|(name, _)| name.clone()),
        );
        let has_gui = namespace
            .arenas()
            .get("gui_documents")
            .is_some_and(|records| !records.is_empty());
        observed
            .document_variants
            .insert(if has_gui { "gui" } else { "headless" }.to_owned());
        collect_native_observations(first.ir(), &mut observed)?;
        let appearance_ownership_and_precedence = appearance_checks(first.ir(), &mut observed)?;
        let design_census_matches_projection = design_census_matches_projection(first.ir())?;
        let design_property_edit_round_trip = design_property_edit_round_trip(first.ir())?;
        for (name, count) in &neutral.entity_counts {
            *total_counts.entry(name.to_string()).or_default() += count;
            if *count > 0 && !name.as_str().starts_with("native.") {
                observed.neutral_arenas.insert(name.to_string());
            }
        }
        fixtures.push(FixtureProfile {
            filename: file_name(&path)?,
            sha256: sha256_hex(&bytes),
            canonical_cadir_sha256: sha256_hex(canonical.as_bytes()),
            deterministic,
            blocking_losses: first
                .report()
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
            exact_byte_coverage: exact_byte_coverage(first.ir()),
            write: FixtureWriteFlags {
                write_deterministic: first_write == second_write,
                semantic_round_trip,
                typed_edit_round_trip,
            },
            side_entries_preserved,
            semantic_checks: FixtureSemanticChecks {
                appearance_ownership_and_precedence,
                design_census_matches_projection,
                design_property_edit_round_trip,
            },
            native_checks: FixtureNativeChecks {
                native_recomputed_design_verified: native_report.as_ref().is_some_and(|report| {
                    native_design_census_matches(
                        report,
                        first.ir(),
                        &sha256_hex(&bytes),
                        &sha256_hex(&first_write),
                    )
                }),
                native_semantic_equivalence_verified: native_report.as_ref().is_some_and(
                    |report| {
                        native_acceptance_matches(
                            report,
                            &sha256_hex(&bytes),
                            &sha256_hex(&first_write),
                        )
                    },
                ),
            },
            entity_counts: neutral
                .entity_counts
                .iter()
                .map(|(name, count)| (name.to_string(), *count))
                .collect(),
        });
    }

    let manifest = fs::read(&manifest_path)?;
    verify_manifest(&manifest, &fixtures)?;
    let source_less_write = source_less_profile(native_report.as_ref())?;
    let gates = gates(&fixtures, &observed, &total_counts, &source_less_write);
    let highest_passing_gate = gates
        .iter()
        .take_while(|gate| gate.assertions_passed())
        .last()
        .map(|gate| gate.level.clone());
    let profile = Profile {
        version: 5,
        format: "fcstd",
        manifest_sha256: sha256_hex(&manifest),
        manifest_verified: true,
        envelope: Envelope {
            container: "ZIP-packaged FCStd",
            schema_version: 4,
            file_version: 1,
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

fn collect_native_observations(
    ir: &CadIr,
    observed: &mut Observed,
) -> Result<(), cadmpeg_ir::native::NativeConvertError> {
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(());
    };
    for record in namespace
        .arenas()
        .get("carrier_census")
        .into_iter()
        .flatten()
    {
        let fields = &record.fields()?;
        insert_string(fields, "form", &mut observed.shape_forms);
        insert_map_keys(fields, "curves_2d", &mut observed.curves_2d);
        insert_map_keys(fields, "curves_3d", &mut observed.curves_3d);
        insert_map_keys(fields, "surfaces", &mut observed.surfaces);
        insert_map_keys(fields, "topology", &mut observed.topology);
    }
    for record in namespace.arenas().get("applications").into_iter().flatten() {
        let fields = &record.fields()?;
        insert_string(fields, "type_name", &mut observed.application_types);
        if fields.get("inert_payload").and_then(Value::as_bool) == Some(true) {
            observed
                .application_constructs
                .insert("inert_payload".into());
        }
        if fields
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
    for record in namespace.arenas().get("drawings").into_iter().flatten() {
        let fields = &record.fields()?;
        insert_string(fields, "kind", &mut observed.drawing_types);
        if fields
            .get("side_entries")
            .and_then(Value::as_array)
            .is_some_and(|entries| !entries.is_empty())
        {
            observed
                .presentation_constructs
                .insert("drawing_asset".into());
        }
    }
    for record in namespace
        .arenas()
        .get("gui_documents")
        .into_iter()
        .flatten()
    {
        if let Some(states) = record.field("states").as_ref().and_then(Value::as_array) {
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
        .arenas()
        .get("gui_view_providers")
        .into_iter()
        .flatten()
    {
        if record
            .field("expanded")
            .is_some_and(|value| !value.is_null())
        {
            observed.presentation_constructs.insert("tree_state".into());
        }
    }
    for record in namespace
        .arenas()
        .get("gui_properties")
        .into_iter()
        .flatten()
    {
        if let Some(Value::String(name)) = record.field("name") {
            observed
                .presentation_constructs
                .insert(format!("view_property:{name}"));
        }
    }
    for record in namespace.arenas().get("entries").into_iter().flatten() {
        if record.field("role").as_ref().and_then(Value::as_str) == Some("thumbnail") {
            observed.presentation_constructs.insert("thumbnail".into());
        }
    }
    let product_nodes = namespace
        .arenas()
        .get("product_nodes")
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let product_node_fields = product_nodes
        .iter()
        .map(|record| record.fields())
        .collect::<Result<Vec<_>, _>>()?;
    for fields in &product_node_fields {
        if let Some(Value::String(kind)) = fields.get("kind") {
            observed.product_constructs.insert(kind.clone());
        }
        if fields
            .get("element_count")
            .and_then(Value::as_i64)
            .is_some_and(|count| count > 1)
        {
            observed.product_constructs.insert("link_array".into());
        }
        if fields
            .get("external_document")
            .is_some_and(|value| !value.is_null())
        {
            observed
                .product_constructs
                .insert("external_occurrence".into());
        }
        if fields
            .get("linked_subelements")
            .and_then(Value::as_array)
            .is_some_and(|values| !values.is_empty())
        {
            observed
                .product_constructs
                .insert("linked_subelements".into());
        }
        let prototype = fields.get("prototype").and_then(Value::as_str);
        if prototype.is_some_and(|prototype| {
            product_node_fields.iter().any(|candidate| {
                candidate.get("object").and_then(Value::as_str) == Some(prototype)
                    && candidate.get("kind").and_then(Value::as_str) == Some("occurrence")
            })
        }) {
            observed
                .product_constructs
                .insert("nested_occurrence".into());
        }
    }
    for record in namespace.arenas().get("joints").into_iter().flatten() {
        let fields = &record.fields()?;
        insert_string(fields, "kind", &mut observed.joint_kinds);
        if fields
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
        if fields
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
        if let Ok(Value::Object(definition)) = serde_json::to_value(feature.evaluation.definition())
        {
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
    Ok(())
}

/// Record every non-default branch a feature definition selects, keyed by
/// operation family.
///
/// A repeated operand carries the same branch taxonomy as a single operand, so
/// a sequence field contributes its element branches under the enclosing path
/// and neither the sequence field name nor an element index appears in the
/// recorded name.
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
        if let Value::Array(items) = child {
            for item in items {
                collect_feature_branches(item, family, path, output);
            }
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
        .and_then(|namespace| namespace.arenas().get("byte_coverage"))
        .is_some_and(|records| {
            records.len() == 1
                && records[0].field("exact").as_ref().and_then(Value::as_bool) == Some(true)
        })
}

fn native_acceptance_matches(report: &Value, original_digest: &str, written_digest: &str) -> bool {
    report
        .get("failures")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
        && report
            .get("files")
            .and_then(Value::as_array)
            .is_some_and(|files| {
                files.iter().any(|file| {
                    file.get("reference_sha256").and_then(Value::as_str) == Some(original_digest)
                        && file.get("input_sha256").and_then(Value::as_str) == Some(written_digest)
                        && file
                            .get("semantic_equivalence_accepted")
                            .and_then(Value::as_bool)
                            == Some(true)
                        && file.get("native_resave_accepted").and_then(Value::as_bool) == Some(true)
                })
            })
}

fn native_source_less_matches(report: &Value, written_digest: &str) -> bool {
    const VOLUME_ABSOLUTE_TOLERANCE: f64 = 1e-7;
    let Some(source_less) = report.get("source_less") else {
        return false;
    };
    source_less.get("source_sha256").and_then(Value::as_str) == Some(written_digest)
        && source_less
            .get("native_recompute_accepted")
            .and_then(Value::as_bool)
            == Some(true)
        && source_less
            .get("dimensions_mm")
            .and_then(Value::as_array)
            .is_some_and(|dimensions| {
                dimensions.len() == 3
                    && dimensions
                        .iter()
                        .zip([12.5, 7.0, 3.0])
                        .all(|(value, expected)| value.as_f64() == Some(expected))
            })
        && source_less.get("solids").and_then(Value::as_u64) == Some(1)
        && source_less.get("faces").and_then(Value::as_u64) == Some(6)
        && source_less
            .get("volume_mm3")
            .and_then(Value::as_f64)
            .is_some_and(|volume| (volume - 262.5).abs() <= VOLUME_ABSOLUTE_TOLERANCE)
}

fn native_design_census_matches(
    report: &Value,
    ir: &CadIr,
    original_digest: &str,
    written_digest: &str,
) -> bool {
    if !native_acceptance_matches(report, original_digest, written_digest) {
        return false;
    }
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return false;
    };
    let Some(files) = report.get("files").and_then(Value::as_array) else {
        return false;
    };
    let Some(file) = files.iter().find(|file| {
        file.get("reference_sha256").and_then(Value::as_str) == Some(original_digest)
            && file.get("input_sha256").and_then(Value::as_str) == Some(written_digest)
    }) else {
        return false;
    };
    let Some(objects) = file
        .get("native_snapshot")
        .and_then(|snapshot| snapshot.get("objects"))
        .and_then(Value::as_object)
    else {
        return false;
    };
    ir.model.features.iter().all(|feature| {
        let Some(owner) = feature.native_ref.as_deref() else {
            return false;
        };
        let Some(record) = namespace
            .arenas()
            .get("objects")
            .into_iter()
            .flatten()
            .find(|record| record.id() == owner)
        else {
            return false;
        };
        let Some(Value::String(name)) = record.field("name") else {
            return false;
        };
        objects
            .get(&name)
            .and_then(|object| object.get("type"))
            .cloned()
            == record.field("type_name")
    })
}

fn typed_operation(
    feature: &cadmpeg_ir::features::Feature,
) -> Result<Option<Value>, serde_json::Error> {
    let operation = match feature.evaluation.definition() {
        FeatureDefinition::PostProcess { operation, .. }
        | FeatureDefinition::Operation(operation) => operation,
    };
    if matches!(
        operation,
        cadmpeg_ir::features::FeatureOperation::Native { .. }
            | cadmpeg_ir::features::FeatureOperation::Unresolved { .. }
    ) {
        return Ok(None);
    }
    serde_json::to_value(operation).map(Some)
}

fn appearance_checks(
    ir: &CadIr,
    observed: &mut Observed,
) -> Result<bool, Box<dyn std::error::Error>> {
    let namespace = ir
        .native
        .namespace("fcstd")
        .ok_or("missing fcstd namespace")?;
    let properties = namespace
        .arenas()
        .get("properties")
        .into_iter()
        .flatten()
        .map(|record| record.fields().map(|fields| (record.id(), fields)))
        .collect::<Result<Vec<_>, _>>()?;
    let payloads = namespace
        .arenas()
        .get("shape_payloads")
        .into_iter()
        .flatten()
        .map(|record| record.fields().map(|fields| (record.id(), fields)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut assignments = BTreeSet::new();
    for binding in &ir.model.appearance_bindings {
        let Some(asset) = ir
            .model
            .appearances
            .iter()
            .find(|asset| asset.id == binding.appearance)
        else {
            return Ok(false);
        };
        let (kind, target, precedence) = match &binding.target {
            AppearanceTarget::Body(id) => ("body", id.as_str(), None),
            AppearanceTarget::Face(id) => ("face", id.as_str(), Some("face_over_object")),
            AppearanceTarget::Edge(id) => (
                "edge",
                id.as_str(),
                Some(if asset.schema.as_deref() == Some("FCStd LineColorArray") {
                    "edge_array_over_line"
                } else {
                    "edge_over_object"
                }),
            ),
            AppearanceTarget::Vertex(id) => (
                "vertex",
                id.as_str(),
                Some(
                    if asset.schema.as_deref() == Some("FCStd PointColorArray") {
                        "vertex_array_over_point"
                    } else {
                        "vertex_over_object"
                    },
                ),
            ),
            _ => return Ok(false),
        };
        observed.appearance_targets.insert(kind.to_owned());
        let Some(owner) = binding.source_entity_id.as_deref() else {
            return Ok(false);
        };
        let owned = payloads.iter().any(|(payload_id, payload)| {
            properties.iter().any(|(property_id, property)| {
                property.get("owner").and_then(Value::as_str) == Some(owner)
                    && property.get("name").and_then(Value::as_str) == Some("Shape")
                    && Some(*property_id) == payload.get("property").and_then(Value::as_str)
            }) && {
                let key = payload_id
                    .split_once('#')
                    .map_or(*payload_id, |(_, key)| key);
                let target_key = target.split_once('#').map_or(target, |(_, key)| key);
                target_key
                    .strip_prefix(key)
                    .is_some_and(|suffix| suffix.starts_with(':'))
            }
        });
        if !owned
            || binding.channels.get("precedence").map(String::as_str) != precedence
            || !assignments.insert((owner, kind, target))
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn design_census_matches_projection(ir: &CadIr) -> Result<bool, Box<dyn std::error::Error>> {
    let namespace = ir
        .native
        .namespace("fcstd")
        .ok_or("missing fcstd namespace")?;
    let records = namespace
        .arenas()
        .get("design_census")
        .into_iter()
        .flatten()
        .map(cadmpeg_ir::NativeRecord::fields)
        .collect::<Result<Vec<_>, _>>()?;
    let projected = ir
        .model
        .features
        .iter()
        .filter(|feature| feature.native_ref.is_some())
        .collect::<Vec<_>>();
    if records.len() != projected.len() {
        return Ok(false);
    }
    for feature in projected {
        let matching = records
            .iter()
            .filter(|record| {
                record.get("feature").and_then(Value::as_str) == Some(feature.id.as_str())
                    && record.get("object").and_then(Value::as_str) == feature.native_ref.as_deref()
            })
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Ok(false);
        }
        let (operation, post_processed) = match feature.evaluation.definition() {
            FeatureDefinition::PostProcess { operation, .. } => {
                (serde_json::to_value(operation)?, true)
            }
            FeatureDefinition::Operation(operation) => (serde_json::to_value(operation)?, false),
        };
        let semantic_kind = operation.get("definition").and_then(Value::as_str);
        if matching[0].get("semantic_kind").and_then(Value::as_str) != semantic_kind
            || matching[0].get("post_processed").and_then(Value::as_bool) != Some(post_processed)
            || matching[0].get("neutral").and_then(Value::as_bool)
                != Some(semantic_kind != Some("native"))
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn design_property_edit_round_trip(ir: &CadIr) -> Result<bool, Box<dyn std::error::Error>> {
    let namespace = ir
        .native
        .namespace("fcstd")
        .ok_or("missing fcstd namespace")?;
    for property in namespace.arenas().get("properties").into_iter().flatten() {
        let fields = property.fields()?;
        let Some(owner) = fields.get("owner").and_then(Value::as_str) else {
            continue;
        };
        let Some(feature) = ir
            .model
            .features
            .iter()
            .find(|feature| feature.native_ref.as_deref() == Some(owner))
        else {
            continue;
        };
        let Some(original_operation) = typed_operation(feature)? else {
            continue;
        };
        let Some(name) = fields.get("name").and_then(Value::as_str) else {
            continue;
        };
        if !["Length", "Width", "Height", "Radius", "Angle"].contains(&name) {
            continue;
        }
        let Some(old) = property_value_attribute(ir, owner, name, 0, "value")
            .and_then(|value| value.parse::<f64>().ok())
        else {
            continue;
        };
        let new = (old + 1.0).to_string();
        let object_name = namespace
            .arenas()
            .get("objects")
            .into_iter()
            .flatten()
            .find(|record| record.id() == owner)
            .and_then(|record| record.field("name"))
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or("design property owner has no object name")?;
        let mut edited = ir.clone();
        FcstdCodec.set_property_value_attribute(
            &mut edited,
            FcstdPropertyOwner::Object(&object_name),
            name,
            0,
            "value",
            &new,
        )?;
        let mut bytes = Vec::new();
        FcstdCodec
            .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)?
            .write_to(&mut bytes)?;
        let decoded = FcstdCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default())?;
        if property_value_attribute(decoded.ir(), owner, name, 0, "value") == Some(new)
            && decoded
                .ir()
                .model
                .features
                .iter()
                .find(|candidate| candidate.id == feature.id)
                .map(typed_operation)
                .transpose()?
                .flatten()
                .is_some_and(|operation| operation != original_operation)
            && design_census_matches_projection(decoded.ir())?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn semantic_fingerprint(mut ir: CadIr) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(source) = &mut ir.source {
        source.attributes.remove("physical_archive_bytes");
        source.attributes.remove("physical_ledger_spans");
        source.attributes.remove(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE);
    }
    if let Some(namespace) = ir.native.0.get_mut("fcstd") {
        namespace.arenas_mut().remove("physical_ledger");
        namespace.arenas_mut().remove("byte_coverage");
        namespace.arenas_mut().remove("logical_ledger");
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
    let mut entries = BTreeMap::new();
    for record in namespace.arenas().get("entries").into_iter().flatten() {
        let fields = record.fields()?;
        if let (Some(name), Some(digest)) = (
            fields.get("name").and_then(Value::as_str),
            fields.get("sha256").and_then(Value::as_str),
        ) {
            entries.insert(name.to_owned(), digest.to_owned());
        }
    }
    Ok(entries)
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
        .arenas()
        .get("properties")?
        .iter()
        .find(|record| {
            record.field("owner").as_ref().and_then(Value::as_str) == Some(owner)
                && record.field("name").as_ref().and_then(Value::as_str) == Some(property_name)
        })?
        .field("values")?
        .as_array()?
        .iter()
        .find(|value| {
            value.get("order").and_then(Value::as_u64)
                == Some(cadmpeg_core::decode::u64_from_index(value_order))
        })?
        .get("attributes")?
        .get(attribute)?
        .as_str()
        .map(str::to_owned)
}

fn source_less_profile(
    native_report: Option<&Value>,
) -> Result<SourceLessWriteProfile, Box<dyn std::error::Error>> {
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
    FcstdCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut first))?;
    let mut second = Vec::new();
    FcstdCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut second))?;
    let decoded = FcstdCodec.decode(&mut Cursor::new(&first), &DecodeOptions::default())?;
    let namespace = decoded
        .ir()
        .native
        .namespace("fcstd")
        .ok_or("generated CADIR has no fcstd namespace")?;
    let object_type = namespace
        .arenas()
        .get("objects")
        .into_iter()
        .flatten()
        .find_map(|record| match record.field("type_name") {
            Some(Value::String(value)) => Some(value),
            _ => None,
        })
        .unwrap_or_default();
    let object_owner = "fcstd:native:object#Box";
    let typed_parameters = ["Length", "Width", "Height"]
        .into_iter()
        .filter_map(|name| {
            property_value_attribute(decoded.ir(), object_owner, name, 0, "value")
                .map(|value| (name.to_owned(), value))
        })
        .collect();
    let unsupported_target_rejected = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-3"),
        )
        .is_err();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())?;
    Ok(SourceLessWriteProfile {
        generation: SourceLessGenerationFlags {
            generated: true,
            deterministic: first == second,
            decodes_cleanly: FcstdCodec
                .validate_native(&ctx, decoded.ir(), None)?
                .is_empty(),
        },
        object_type,
        typed_parameters,
        unsupported_target_rejected,
        independent_native_acceptance: native_report
            .is_some_and(|report| native_source_less_matches(report, &sha256_hex(&first))),
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
        "datum_coordinate_system:frame.origin.z=9.0",
        "datum_plane:frame.origin.z=4.0",
        "datum_point:position.z=6.0",
        "draft:operation.outward=true",
        "extrude:extent.kind=symmetric",
        "fillet:radius.kind=constant",
        "hole:operation.bottom.kind=angled",
        "hole:operation.shape.construction.kind.kind=countersink",
        "loft:max_degree=4",
        "loft:ruled=true",
        "mirror_shape:plane_normal.x=1.0",
        "pattern:operation.pattern.count=4",
        "pattern:operation.pattern.direction.z=-1.0",
        "primitive:solid.kind=box",
        "primitive:solid.kind=cylinder",
        "revolve:construction.extent.kind=symmetric",
        "revolve:construction.extent.termination.kind=angle",
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
    vec![
        gate(
            "L0",
            vec![
                assertion("clean_decode", clean, &clean, &"all fixtures decode deterministically and validate",),
                assertion("physical_coverage", exact, &exact, &"exact physical and logical byte partitions",),
            ],
        ),
        gate(
            "L1",
            vec![
                assertion("application_graph", observed.native_arenas.contains("objects")
                        && observed.native_arenas.contains("properties"), &format!("{:?}", observed.native_arenas), &"objects and properties arenas",),
                assertion("asset_inventory", observed.native_arenas.contains("entries"), &format!("{:?}", observed.native_arenas), &"entries arena",),
            ],
        ),
        gate(
            "L2",
            vec![
                assertion("text_and_binary_shapes", observed.shape_forms.contains("text")
                        && observed.shape_forms.contains("binary"), &format!("{:?}", observed.shape_forms), &"text and binary",),
                assertion("carrier_families", required_carriers.is_subset(&carriers), &format!("{carriers:?}"), &format!("{required_carriers:?}"),),
                assertion("application_geometry", count("tessellations") > 0 && count("points") > 0, &format!(
                        "tessellations={}, points={}",
                        count("tessellations"),
                        count("points")
                    ), &"mesh tessellation and point data",),
            ],
        ),
        gate(
            "L3",
            vec![
                assertion("connected_topology", required_topology.is_subset(&topology), &format!("{topology:?}"), &format!("{required_topology:?}"),),
                assertion("persistent_names", observed.native_arenas.contains("element_maps"), &format!("{:?}", observed.native_arenas), &"element_maps arena",),
            ],
        ),
        gate(
            "L4",
            vec![
                assertion("design_records", count("features") > 0
                        && count("sketches") > 0
                        && count("sketch_constraints") > 0
                        && count("parameters") > 0, &format!(
                        "features={}, sketches={}, constraints={}, parameters={}",
                        count("features"),
                        count("sketches"),
                        count("sketch_constraints"),
                        count("parameters")
                    ), &"nonempty feature, sketch, constraint, and parameter graphs",),
                assertion("operation_semantics", observed.feature_operations.contains("extrude")
                        && (observed.feature_operations.contains("fillet")
                            || observed.feature_operations.contains("combine")), &format!("{:?}", observed.feature_operations), &"extrusion plus subtractive or dress-up operation",),
            ],
        ),
        gate(
            "L5",
            vec![assertion(
                "appearance_ownership_and_precedence",
                ["body", "face", "edge", "vertex"].into_iter().all(|kind| observed.appearance_targets.contains(kind))
                    && fixtures.iter().all(|fixture| fixture.semantic_checks.appearance_ownership_and_precedence),
                &format!("targets={:?}, all bindings checked={}", observed.appearance_targets,
                    fixtures.iter().all(|fixture| fixture.semantic_checks.appearance_ownership_and_precedence)),
                &"body, face, edge, and vertex targets with valid source ownership, assets, and override precedence",
            )],
        ),
        gate(
            "L6",
            vec![
                assertion("persisted_projection_census", fixtures.iter().all(|fixture| fixture.semantic_checks.design_census_matches_projection),
                    &format!("{} of {} fixture censuses agree with every projected design feature", fixtures.iter().filter(|fixture| fixture.semantic_checks.design_census_matches_projection).count(), fixtures.len()),
                    &"native object, feature identity, semantic family, and post-process state match every projected feature"),
                assertion("independent_recomputed_design", fixtures.iter().filter(|fixture| fixture.entity_counts.get("features").copied().unwrap_or_default() > 0).all(|fixture| fixture.native_checks.native_recomputed_design_verified),
                    &format!("{} fixtures have SHA-bound native recompute comparisons and projected owner/type matches", fixtures.iter().filter(|fixture| fixture.native_checks.native_recomputed_design_verified).count()),
                    &"every design fixture matches the independent original after native recomputation and save/reopen; every projected owner and runtime type matches the native census"),
                assertion("design_parameter_edit", fixtures.iter().any(|fixture| fixture.semantic_checks.design_property_edit_round_trip),
                    &fixtures.iter().filter(|fixture| fixture.semantic_checks.design_property_edit_round_trip).count(),
                    &"a typed design parameter edit changes projected operation semantics after a write and decode"),
                assertion("complete_constraint_matrix", required_constraints.is_subset(&constraint_definitions)
                        && !constraint_definitions.contains("native"), &format!("{constraint_definitions:?}"), &format!("{required_constraints:?}; no native fallback"),),
                assertion("core_operation_families", required_design_definitions.is_subset(&design_definitions), &format!("{design_definitions:?}"), &format!("{required_design_definitions:?}"),),
                assertion("non_default_operation_branches", required_feature_branches.is_subset(&feature_branches), &format!("{feature_branches:?}"), &format!("{required_feature_branches:?}"),),
            ],
        ),
        gate(
            "L7",
            vec![
                assertion("product_structure_matrix", count("product_definitions") > 0
                        && count("occurrences") > 0
                        && required_product_constructs.is_subset(&product_constructs), &format!(
                        "product_definitions={}, occurrences={}, constructs={product_constructs:?}",
                        count("product_definitions"),
                        count("occurrences")
                    ), &format!("{required_product_constructs:?}"),),
                assertion("assembly_joint_matrix", count("assembly_joints") > 0 && required_joint_kinds.is_subset(&joint_kinds), &format!("joints={}, kinds={joint_kinds:?}", count("assembly_joints")), &format!("{required_joint_kinds:?}"),),
            ],
        ),
        gate(
            "L8",
            vec![
                assertion("presentation_variants", required_document_variants.is_subset(&document_variants)
                        && count("appearance_bindings") > 0
                        && required_presentation_constructs.is_subset(&presentation_constructs), &format!(
                        "documents={document_variants:?}, appearances={}, constructs={presentation_constructs:?}",
                        count("appearance_bindings")
                    ), &format!(
                        "documents={required_document_variants:?}, constructs={required_presentation_constructs:?}, appearances"
                    ),),
                assertion("drawing_annotation_matrix", count("drawings") > 0
                        && count("semantic_annotations") > 0
                        && required_drawing_types.is_subset(&drawing_types), &format!(
                        "drawings={}, semantic_annotations={}, types={drawing_types:?}",
                        count("drawings"),
                        count("semantic_annotations")
                    ), &format!("types={required_drawing_types:?}, drawings and annotations"),),
                assertion("application_retention", observed.native_arenas.contains("applications")
                        && required_application_types.is_subset(&application_types)
                        && required_application_constructs.is_subset(&application_constructs)
                        && exact, &format!(
                        "types={application_types:?}, constructs={application_constructs:?}, exact={exact}"
                    ), &format!(
                        "types={required_application_types:?}, constructs={required_application_constructs:?}, exact byte coverage"
                    ),),
            ],
        ),
        gate(
            "L9",
            vec![
                assertion("independent_native_geometry", fixtures.iter().all(|fixture| fixture.native_checks.native_semantic_equivalence_verified),
                    &format!("{} of {} native comparisons verified by original and output digests", fixtures.iter().filter(|fixture| fixture.native_checks.native_semantic_equivalence_verified).count(), fixtures.len()),
                    &"every written fixture matches its independently opened native original before and after recompute/save/reopen"),
                assertion("cadir_semantic_round_trip", fixtures.iter().all(|fixture| {
                        fixture.write.write_deterministic
                            && fixture.write.semantic_round_trip
                            && fixture.write.typed_edit_round_trip
                    }), &format!(
                        "{} of {} fixtures deterministic, semantically equivalent, and editable",
                        fixtures
                            .iter()
                            .filter(|fixture| fixture.write.write_deterministic
                                && fixture.write.semantic_round_trip
                                && fixture.write.typed_edit_round_trip)
                            .count(),
                        fixtures.len()
                    ), &"every primary-envelope fixture writes deterministically, round-trips semantically, and accepts a typed edit",),
                assertion("unsupported_record_survival", fixtures
                        .iter()
                        .all(|fixture| fixture.side_entries_preserved), &format!(
                        "{} of {} fixture entry sets preserved",
                        fixtures
                            .iter()
                            .filter(|fixture| fixture.side_entries_preserved)
                            .count(),
                        fixtures.len()
                    ), &"every named logical entry retains identity and digest",),
                assertion("independent_source_less_acceptance", source_less_write.independent_native_acceptance,
                    &source_less_write.independent_native_acceptance,
                    &"SHA-bound native recompute of the generated Part::Box retains dimensions 12.5 by 7 by 3 mm, one solid, six faces, and volume 262.5 cubic mm"),
                assertion("source_less_and_target_selection", source_less_write.generation.generated
                        && source_less_write.generation.deterministic
                        && source_less_write.generation.decodes_cleanly
                        && source_less_write.object_type == "Part::Box"
                        && source_less_write.typed_parameters.len() == 3
                        && source_less_write.unsupported_target_rejected, &format!(
                        "generated={}, deterministic={}, clean={}, type={}, parameters={:?}, unsupported_target_rejected={}",
                        source_less_write.generation.generated,
                        source_less_write.generation.deterministic,
                        source_less_write.generation.decodes_cleanly,
                        source_less_write.object_type,
                        source_less_write.typed_parameters,
                        source_less_write.unsupported_target_rejected
                    ), &"deterministic clean source-less Part::Box with three typed dimensions and explicit unsupported-target rejection",),
            ],
        ),
    ]
}

fn assertion(
    id: &'static str,
    passed: bool,
    observed: &impl ToString,
    required: &impl ToString,
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
        assertions,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        appearance_checks, assertion, design_census_matches_projection,
        design_property_edit_round_trip, gate, native_acceptance_matches,
        native_design_census_matches, native_source_less_matches, semantic_fingerprint,
        serialize_gates, Observed,
    };
    use cadmpeg_codec_freecad::{FcstdCodec, FcstdPropertyOwner};
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;

    #[test]
    fn appearance_gate_checks_owner_and_override_precedence() {
        let bytes =
            include_bytes!("../../../../corpus/freecad_fcstd/fixtures/gui_appearance.FCStd");
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode appearance fixture");
        let mut observed = Observed::default();
        assert!(appearance_checks(decoded.ir(), &mut observed).expect("appearance checks"));
        let uniform = FcstdCodec
            .decode(
                &mut Cursor::new(include_bytes!(
                    "../../../../corpus/freecad_fcstd/fixtures/uniform_appearance.FCStd"
                )),
                &DecodeOptions::default(),
            )
            .expect("decode uniform appearance fixture");
        assert!(appearance_checks(uniform.ir(), &mut observed).expect("uniform appearance checks"));
        for kind in ["body", "face", "edge", "vertex"] {
            assert!(observed.appearance_targets.contains(kind), "missing {kind}");
        }
        let mut ir = decoded.ir().clone();
        ir.model.appearance_bindings[0].source_entity_id = Some("fcstd:native:object#other".into());
        assert!(!appearance_checks(&ir, &mut Observed::default()).expect("wrong owner"));
        let mut ir = decoded.ir().clone();
        let binding = ir
            .model
            .appearance_bindings
            .iter_mut()
            .find(|binding| !binding.channels.is_empty())
            .expect("override binding");
        binding.channels.clear();
        assert!(!appearance_checks(&ir, &mut Observed::default()).expect("missing precedence"));
    }

    #[test]
    fn design_gate_checks_census_and_a_real_operation_parameter_edit() {
        let bytes =
            include_bytes!("../../../../corpus/freecad_fcstd/fixtures/core_operations.FCStd");
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode design fixture");
        assert!(design_census_matches_projection(decoded.ir()).expect("design census"));
        assert!(design_property_edit_round_trip(decoded.ir()).expect("design parameter edit"));
        let mut native_only = decoded.ir().clone();
        for feature in &mut native_only.model.features {
            feature.evaluation.set_definition(
                serde_json::from_value(serde_json::json!({
                    "definition":"native", "kind":"retained_operation", "parameters":{}
                }))
                .expect("native definition"),
            );
        }
        assert!(!design_property_edit_round_trip(&native_only)
            .expect("native parameters are not typed semantics"));
        let mut ir = decoded.ir().clone();
        ir.model.features[0].native_ref = Some("fcstd:native:object#other".into());
        assert!(!design_census_matches_projection(&ir).expect("changed design owner"));
    }

    #[test]
    fn recomputed_design_gate_requires_native_owner_and_runtime_type_matches() {
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(include_bytes!(
                    "../../../../corpus/freecad_fcstd/fixtures/core_operations.FCStd"
                )),
                &DecodeOptions::default(),
            )
            .expect("decode design fixture");
        let mut objects = serde_json::Map::new();
        for record in decoded
            .ir()
            .native
            .namespace("fcstd")
            .expect("native namespace")
            .arenas()
            .get("objects")
            .expect("object census")
        {
            let name = record
                .field("name")
                .and_then(|value| value.as_str().map(str::to_owned))
                .expect("object name");
            objects.insert(name, serde_json::json!({"type":record.field("type_name")}));
        }
        let mut report = serde_json::json!({"files":[{
            "reference_sha256":"original", "input_sha256":"written",
            "semantic_equivalence_accepted":true,"native_resave_accepted":true,
            "native_snapshot":{"objects":objects}
        }], "failures":[]});
        assert!(native_design_census_matches(
            &report,
            decoded.ir(),
            "original",
            "written"
        ));
        report["files"][0]["native_snapshot"]["objects"] = serde_json::json!({});
        assert!(!native_design_census_matches(
            &report,
            decoded.ir(),
            "original",
            "written"
        ));
        assert!(!native_design_census_matches(
            &serde_json::json!({}),
            decoded.ir(),
            "original",
            "written"
        ));
    }

    #[test]
    fn source_less_native_gate_requires_the_generated_bytes_and_primitive_oracle() {
        let mut report = serde_json::json!({"source_less":{
            "source_sha256":"generated", "native_recompute_accepted":true,
            "dimensions_mm":[12.5,7,3],"solids":1,"faces":6,"volume_mm3":262.5
        }});
        assert!(native_source_less_matches(&report, "generated"));
        assert!(!native_source_less_matches(&report, "other"));
        report["source_less"]["volume_mm3"] = serde_json::json!(260.0);
        assert!(!native_source_less_matches(&report, "generated"));
        assert!(!native_source_less_matches(
            &serde_json::json!({}),
            "generated"
        ));
    }

    #[test]
    fn native_gate_requires_original_and_written_digests_and_semantic_checks() {
        let report = serde_json::json!({"files":[{
            "reference_sha256":"original", "input_sha256":"written",
            "semantic_equivalence_accepted":true,"native_resave_accepted":true
        }], "failures":[]});
        assert!(native_acceptance_matches(&report, "original", "written"));
        assert!(!native_acceptance_matches(&report, "different", "written"));
        assert!(!native_acceptance_matches(&report, "original", "different"));
        let mut open_only = report.clone();
        open_only["files"][0]["semantic_equivalence_accepted"] = serde_json::json!(false);
        assert!(!native_acceptance_matches(
            &open_only, "original", "written"
        ));
        let mut failed = report;
        failed["failures"] = serde_json::json!([{"filename":"other.FCStd","error":"null shape"}]);
        assert!(!native_acceptance_matches(&failed, "original", "written"));
    }

    #[test]
    fn semantic_fingerprint_ignores_repacking_but_retains_property_edits() {
        let bytes =
            include_bytes!("../../../../corpus/freecad_fcstd/fixtures/binary_exact_shape.FCStd");
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode fixture");
        let original = semantic_fingerprint(decoded.ir().clone()).expect("fingerprint fixture");
        let mut written = Vec::new();
        FcstdCodec
            .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut written))
            .expect("write fixture");
        let restored = FcstdCodec
            .decode(&mut Cursor::new(written), &DecodeOptions::default())
            .expect("decode written fixture");
        assert_eq!(
            original,
            semantic_fingerprint(restored.ir().clone()).expect("fingerprint written fixture")
        );

        let mut edited = restored.ir().clone();
        FcstdCodec
            .set_property_value_attribute(
                &mut edited,
                FcstdPropertyOwner::Document,
                "Label",
                0,
                "value",
                "Changed document label",
            )
            .expect("edit document label");
        assert_ne!(
            original,
            semantic_fingerprint(edited).expect("fingerprint edited fixture")
        );
    }

    #[test]
    fn gate_wire_passed_is_cumulative_without_changing_assertions() {
        let gates = vec![
            gate("L0", vec![assertion("first", true, &"yes", &"yes")]),
            gate("L1", vec![assertion("second", false, &"no", &"yes")]),
            gate("L2", vec![assertion("third", true, &"yes", &"yes")]),
        ];
        assert!(gates[2].assertions_passed());
        let wire =
            serialize_gates(&gates, serde_json::value::Serializer).expect("serialize gate ladder");
        assert_eq!(
            wire,
            serde_json::json!([
                {"level":"L0", "passed":true, "assertions":[{"id":"first", "passed":true, "observed":"yes", "required":"yes"}]},
                {"level":"L1", "passed":false, "assertions":[{"id":"second", "passed":false, "observed":"no", "required":"yes"}]},
                {"level":"L2", "passed":false, "assertions":[{"id":"third", "passed":true, "observed":"yes", "required":"yes"}]}
            ])
        );
    }
}
