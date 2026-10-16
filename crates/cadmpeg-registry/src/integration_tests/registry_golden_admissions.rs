// SPDX-License-Identifier: Apache-2.0
//! Cross-workspace golden admissions against the embedded support registry.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::dialect::{
    Admission, DialectLayers, DialectMatch, FormatIdentity, LayerInstance,
};
use cadmpeg_ir::report::loss::{LossNote, LossTaxonomy, StrictConsequence};
use cadmpeg_parasolid::{Carrier, OwnedSchemaToken, DECLARED_CARRIER, DECLARED_SCHEMA};

use crate::{dialect_table, ReadDisposition};

type RegistryRows = BTreeMap<String, ReadDisposition>;

struct LayerOccurrence {
    snapshot: PathBuf,
    pointer: String,
    layers: DialectLayers,
}

#[derive(Debug, PartialEq, Eq)]
struct AdmissionViolation {
    snapshot: PathBuf,
    pointer: String,
    reason: String,
}

impl AdmissionViolation {
    fn new(snapshot: &Path, pointer: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            snapshot: snapshot.to_owned(),
            pointer: pointer.into(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for AdmissionViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}{}: {}",
            self.snapshot.display(),
            self.pointer,
            self.reason
        )
    }
}

#[test]
fn compiled_read_admissions_match_registry_policy() {
    let registry = registry_rows();
    check_parasolid_capabilities(&registry).unwrap_or_else(|error| panic!("{error}"));

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut snapshots = Vec::new();
    collect_json_files(&root.join("crates"), &mut snapshots);
    let mut layer_set_count = 0usize;
    for path in snapshots {
        if !path
            .components()
            .zip(path.components().skip(1))
            .any(|(parent, child)| parent.as_os_str() == "tests" && child.as_os_str() == "golden")
        {
            continue;
        }
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|error| panic!("cannot parse {}: {error}", path.display()));
        let ctx = cadmpeg_test_support::service_decode_context();
        let count = validate_snapshot(&value, &path, &registry, &ctx)
            .unwrap_or_else(|error| panic!("{error}"));
        layer_set_count = layer_set_count
            .checked_add(count)
            .expect("golden layer-set count fits usize");
    }

    assert!(layer_set_count > 0, "no golden dialect layer sets found");
}

fn registry_rows() -> RegistryRows {
    dialect_table(None)
        .expect("embedded registry formats")
        .into_iter()
        .flat_map(|format| format.rows)
        .map(|row| (row.id.as_str().to_owned(), row.disposition.read))
        .collect()
}

fn validate_snapshot(
    value: &serde_json::Value,
    snapshot: &Path,
    registry: &RegistryRows,
    ctx: &DecodeContext<'_>,
) -> Result<usize, AdmissionViolation> {
    let occurrences = collect_layer_sets(value, snapshot)?;
    check_matching_view_identities(value, snapshot, "")?;
    for occurrence in &occurrences {
        check_layer_set(registry, ctx, occurrence)?;
    }
    check_kernel_loss_counts(value, snapshot, "")?;
    Ok(occurrences.len())
}

fn collect_layer_sets(
    value: &serde_json::Value,
    snapshot: &Path,
) -> Result<Vec<LayerOccurrence>, AdmissionViolation> {
    let mut occurrences = Vec::new();
    let mut match_count = 0usize;
    collect_layer_sets_at(value, snapshot, "", &mut occurrences, &mut match_count)?;

    let typed_layer_count = occurrences
        .iter()
        .map(|occurrence| occurrence.layers.iter().count())
        .sum::<usize>();
    if match_count != typed_layer_count {
        return Err(AdmissionViolation::new(
            snapshot,
            "",
            format!(
                "found {match_count} dialect/admission objects but typed layer sets contain {typed_layer_count}; a layer is orphaned or missing"
            ),
        ));
    }
    Ok(occurrences)
}

fn collect_layer_sets_at(
    value: &serde_json::Value,
    snapshot: &Path,
    pointer: &str,
    occurrences: &mut Vec<LayerOccurrence>,
    match_count: &mut usize,
) -> Result<(), AdmissionViolation> {
    match value {
        serde_json::Value::Object(object) => {
            if object
                .get("classification")
                .and_then(serde_json::Value::as_str)
                == Some("classified")
                && pointer.rsplit('/').next() == Some("identity")
                && !object.contains_key("dialects")
            {
                return Err(AdmissionViolation::new(
                    snapshot,
                    pointer_child(pointer, "dialects"),
                    "classified identity is missing its dialect layer set",
                ));
            }

            if object.contains_key("dialect") && object.contains_key("admission") {
                serde_json::from_value::<DialectMatch>(value.clone()).map_err(|error| {
                    AdmissionViolation::new(
                        snapshot,
                        pointer,
                        format!("invalid dialect layer: {error}"),
                    )
                })?;
                *match_count = (*match_count).checked_add(1).ok_or_else(|| {
                    AdmissionViolation::new(snapshot, pointer, "dialect layer count overflow")
                })?;
            }

            if let Some(dialects) = object.get("dialects") {
                let layers =
                    serde_json::from_value::<DialectLayers>(dialects.clone()).map_err(|error| {
                        AdmissionViolation::new(
                            snapshot,
                            pointer_child(pointer, "dialects"),
                            format!("invalid dialect layer set: {error}"),
                        )
                    })?;
                occurrences.push(LayerOccurrence {
                    snapshot: snapshot.to_owned(),
                    pointer: pointer_child(pointer, "dialects"),
                    layers,
                });
            }

            for (key, child) in object {
                collect_layer_sets_at(
                    child,
                    snapshot,
                    &pointer_child(pointer, key),
                    occurrences,
                    match_count,
                )?;
            }
        }
        serde_json::Value::Array(array) => {
            for (index, child) in array.iter().enumerate() {
                collect_layer_sets_at(
                    child,
                    snapshot,
                    &format!("{pointer}/{index}"),
                    occurrences,
                    match_count,
                )?;
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
    Ok(())
}

fn check_matching_view_identities(
    value: &serde_json::Value,
    snapshot: &Path,
    pointer: &str,
) -> Result<(), AdmissionViolation> {
    if let serde_json::Value::Object(object) = value {
        let is_decode_report = (pointer.is_empty() || pointer.ends_with("/report"))
            && object.contains_key("transfer")
            && object.contains_key("losses")
            && object.contains_key("notes");
        let is_container_summary = (pointer.is_empty() || pointer.ends_with("/inspect"))
            && object.contains_key("container_kind")
            && object.contains_key("entries")
            && object.contains_key("notes");
        let is_source_meta = pointer.ends_with("/ir/source");
        let identity_owner = if is_decode_report {
            Some("decode report")
        } else if is_container_summary {
            Some("container summary")
        } else if is_source_meta {
            Some("source metadata")
        } else {
            None
        };
        if let Some(owner) = identity_owner {
            let identity_pointer = pointer_child(pointer, "identity");
            let identity = object.get("identity").ok_or_else(|| {
                AdmissionViolation::new(
                    snapshot,
                    &identity_pointer,
                    format!("successful {owner} is missing its typed identity"),
                )
            })?;
            serde_json::from_value::<FormatIdentity<DialectLayers>>(identity.clone()).map_err(
                |error| {
                    AdmissionViolation::new(
                        snapshot,
                        &identity_pointer,
                        format!("invalid typed {owner} identity: {error}"),
                    )
                },
            )?;
        }

        if let (Some(ir), Some(report)) = (object.get("ir"), object.get("report")) {
            let source_identity = ir
                .get("source")
                .and_then(|source| source.get("identity"))
                .and_then(|identity| identity.get("dialects"));
            let report_identity = report
                .get("identity")
                .and_then(|identity| identity.get("dialects"));
            check_identity_pair(
                source_identity,
                report_identity,
                snapshot,
                &pointer_child(
                    &pointer_child(
                        &pointer_child(&pointer_child(pointer, "ir"), "source"),
                        "identity",
                    ),
                    "dialects",
                ),
                &pointer_child(
                    &pointer_child(&pointer_child(pointer, "report"), "identity"),
                    "dialects",
                ),
            )?;
        }

        if let (Some(decode), Some(inspect)) = (object.get("decode"), object.get("inspect")) {
            let report_identity = decode
                .get("report")
                .and_then(|report| report.get("identity"))
                .and_then(|identity| identity.get("dialects"));
            let inspect_identity = inspect
                .get("identity")
                .and_then(|identity| identity.get("dialects"));
            check_identity_pair(
                report_identity,
                inspect_identity,
                snapshot,
                &pointer_child(
                    &pointer_child(
                        &pointer_child(&pointer_child(pointer, "decode"), "report"),
                        "identity",
                    ),
                    "dialects",
                ),
                &pointer_child(
                    &pointer_child(&pointer_child(pointer, "inspect"), "identity"),
                    "dialects",
                ),
            )?;
        }

        for (key, child) in object {
            check_matching_view_identities(child, snapshot, &pointer_child(pointer, key))?;
        }
    } else if let serde_json::Value::Array(array) = value {
        for (index, child) in array.iter().enumerate() {
            check_matching_view_identities(child, snapshot, &format!("{pointer}/{index}"))?;
        }
    }
    Ok(())
}

fn check_identity_pair(
    first: Option<&serde_json::Value>,
    second: Option<&serde_json::Value>,
    snapshot: &Path,
    first_pointer: &str,
    second_pointer: &str,
) -> Result<(), AdmissionViolation> {
    match (first, second) {
        (None, None) => Ok(()),
        (Some(_), None) => Err(AdmissionViolation::new(
            snapshot,
            second_pointer,
            format!("matching identity is missing; expected the layer set at {first_pointer}"),
        )),
        (None, Some(_)) => Err(AdmissionViolation::new(
            snapshot,
            first_pointer,
            format!("matching identity is missing; expected the layer set at {second_pointer}"),
        )),
        (Some(first), Some(second)) => {
            let first: DialectLayers = serde_json::from_value(first.clone()).map_err(|error| {
                AdmissionViolation::new(
                    snapshot,
                    first_pointer,
                    format!("invalid dialect layer set: {error}"),
                )
            })?;
            let second: DialectLayers =
                serde_json::from_value(second.clone()).map_err(|error| {
                    AdmissionViolation::new(
                        snapshot,
                        second_pointer,
                        format!("invalid dialect layer set: {error}"),
                    )
                })?;
            if first != second {
                return Err(AdmissionViolation::new(
                    snapshot,
                    second_pointer,
                    format!("layer identity differs from {first_pointer}"),
                ));
            }
            Ok(())
        }
    }
}

fn check_layer_set(
    registry: &RegistryRows,
    ctx: &DecodeContext<'_>,
    occurrence: &LayerOccurrence,
) -> Result<(), AdmissionViolation> {
    let primary = occurrence.layers.primary();
    let primary_pointer = pointer_child(&occurrence.pointer, "primary");
    let primary_read = registered_read(registry, primary, &occurrence.snapshot, &primary_pointer)?;
    if !native_admission_compatible(primary_read, primary.admission()) {
        return Err(AdmissionViolation::new(
            &occurrence.snapshot,
            primary_pointer,
            format!(
                "primary {} admission {:?} contradicts registry read {primary_read}",
                primary.dialect(),
                primary.admission()
            ),
        ));
    }

    for (index, layer) in occurrence.layers.iter().skip(1).enumerate() {
        let layer_pointer = pointer_child(
            &pointer_child(&occurrence.pointer, "extra"),
            &index.to_string(),
        );
        let read = registered_read(registry, layer, &occurrence.snapshot, &layer_pointer)?;
        if layer.format() == "parasolid" {
            let expected = classify_parasolid_layer(
                ctx,
                primary,
                layer,
                &occurrence.snapshot,
                &layer_pointer,
            )?;
            if layer.dialect() != expected.dialect() || layer.admission() != expected.admission() {
                return Err(AdmissionViolation::new(
                    &occurrence.snapshot,
                    layer_pointer,
                    format!(
                        "host {} classifies kernel {} as {:?}; observed {:?}",
                        primary.dialect(),
                        expected.dialect(),
                        expected.admission(),
                        layer.admission()
                    ),
                ));
            }
        } else if !native_admission_compatible(read, layer.admission()) {
            return Err(AdmissionViolation::new(
                &occurrence.snapshot,
                layer_pointer,
                format!(
                    "secondary {} admission {:?} contradicts registry read {read}",
                    layer.dialect(),
                    layer.admission()
                ),
            ));
        }
    }
    Ok(())
}

fn registered_read(
    registry: &RegistryRows,
    layer: &DialectMatch,
    snapshot: &Path,
    pointer: &str,
) -> Result<ReadDisposition, AdmissionViolation> {
    registry
        .get(layer.dialect().as_str())
        .copied()
        .ok_or_else(|| {
            AdmissionViolation::new(
                snapshot,
                pointer,
                format!("{} has no registry support row", layer.dialect()),
            )
        })
}

fn classify_parasolid_layer(
    ctx: &DecodeContext<'_>,
    primary: &DialectMatch,
    layer: &DialectMatch,
    snapshot: &Path,
    pointer: &str,
) -> Result<DialectMatch, AdmissionViolation> {
    let verified = match primary.format() {
        "nx" => cadmpeg_codec_nx::NxCodec::VERIFIED_PARASOLID_GRAMMARS,
        "sldprt" => cadmpeg_codec_sldprt::SldprtCodec::VERIFIED_PARASOLID_GRAMMARS,
        _ => {
            return Err(AdmissionViolation::new(
                snapshot,
                pointer,
                format!(
                    "host {} has no Parasolid verification policy for {}",
                    primary.dialect(),
                    layer.dialect()
                ),
            ));
        }
    };
    let schema = layer.declared().get(DECLARED_SCHEMA).ok_or_else(|| {
        AdmissionViolation::new(
            snapshot,
            pointer,
            format!("{} is missing its schema declaration", layer.dialect()),
        )
    })?;
    let carrier = layer.declared().get(DECLARED_CARRIER).ok_or_else(|| {
        AdmissionViolation::new(
            snapshot,
            pointer,
            format!("{} is missing its carrier declaration", layer.dialect()),
        )
    })?;
    let schema = OwnedSchemaToken::try_from(schema.clone()).map_err(|error| {
        AdmissionViolation::new(
            snapshot,
            pointer,
            format!(
                "{} has an invalid schema declaration: {error}",
                layer.dialect()
            ),
        )
    })?;
    let classified = cadmpeg_parasolid::classify_layer(
        ctx,
        schema,
        Carrier::new(carrier.clone()),
        LayerInstance::Sole,
        verified,
    )
    .map_err(|error| {
        AdmissionViolation::new(
            snapshot,
            pointer,
            format!("{} could not be classified: {error}", layer.dialect()),
        )
    })?;
    Ok(classified.into_matched())
}

fn native_admission_compatible(read: ReadDisposition, admission: &Admission) -> bool {
    match admission {
        Admission::Admitted => {
            matches!(read, ReadDisposition::Level(_) | ReadDisposition::Detected)
        }
        Admission::Unverified { .. } | Admission::Residual => {
            matches!(read, ReadDisposition::UnclassifiedRecovered)
        }
        Admission::Refused => matches!(read, ReadDisposition::Refused),
    }
}

fn check_parasolid_capabilities(registry: &RegistryRows) -> Result<(), AdmissionViolation> {
    let registry_path = Path::new("<embedded registry>");
    for (host, verified) in [
        ("nx", cadmpeg_codec_nx::NxCodec::VERIFIED_PARASOLID_GRAMMARS),
        (
            "sldprt",
            cadmpeg_codec_sldprt::SldprtCodec::VERIFIED_PARASOLID_GRAMMARS,
        ),
    ] {
        for dialect in verified {
            let pointer = format!("/support/{}", dialect.as_str());
            let read = registry.get(dialect.as_str()).ok_or_else(|| {
                AdmissionViolation::new(
                    registry_path,
                    &pointer,
                    format!("{host} verifies {dialect} without a registry row"),
                )
            })?;
            if !matches!(read, ReadDisposition::Level(_) | ReadDisposition::Detected) {
                return Err(AdmissionViolation::new(
                    registry_path,
                    pointer,
                    format!("{host} verifies {dialect} but its global read cell is {read}"),
                ));
            }
        }
    }

    match registry.get("parasolid:unknown") {
        Some(ReadDisposition::UnclassifiedRecovered) => Ok(()),
        Some(read) => Err(AdmissionViolation::new(
            registry_path,
            "/support/parasolid:unknown",
            format!("the unknown Parasolid row must remain unclassified-recovered, found {read}"),
        )),
        None => Err(AdmissionViolation::new(
            registry_path,
            "/support/parasolid:unknown",
            "the unknown Parasolid row has no registry support row",
        )),
    }
}

fn check_kernel_loss_counts(
    value: &serde_json::Value,
    snapshot: &Path,
    pointer: &str,
) -> Result<(), AdmissionViolation> {
    match value {
        serde_json::Value::Object(object) => {
            let identity = object
                .get("identity")
                .and_then(serde_json::Value::as_object);
            let is_view = object.contains_key("losses")
                || pointer.is_empty()
                || pointer.ends_with("/report")
                || pointer.ends_with("/inspect");
            if is_view {
                if let Some(dialects) = identity.and_then(|identity| identity.get("dialects")) {
                    let layers: DialectLayers =
                        serde_json::from_value(dialects.clone()).map_err(|error| {
                            AdmissionViolation::new(
                                snapshot,
                                pointer_child(&pointer_child(pointer, "identity"), "dialects"),
                                format!("invalid dialect layer set: {error}"),
                            )
                        })?;
                    let host = layers.primary().format();
                    let expected = layers
                        .iter()
                        .skip(1)
                        .filter(|layer| layer.format() == "parasolid")
                        .filter(|layer| {
                            matches!(
                                layer.admission(),
                                Admission::Unverified { .. } | Admission::Residual
                            )
                        })
                        .count();
                    let losses = match object.get("losses") {
                        Some(serde_json::Value::Array(losses)) => losses.as_slice(),
                        Some(_) => {
                            return Err(AdmissionViolation::new(
                                snapshot,
                                pointer_child(pointer, "losses"),
                                "view losses must be an array",
                            ));
                        }
                        None => &[],
                    };
                    let mut observed = 0usize;
                    for (index, loss) in losses.iter().enumerate() {
                        let loss_pointer =
                            pointer_child(&pointer_child(pointer, "losses"), &index.to_string());
                        let loss: LossNote =
                            serde_json::from_value(loss.clone()).map_err(|error| {
                                AdmissionViolation::new(
                                    snapshot,
                                    &loss_pointer,
                                    format!("invalid typed loss note: {error}"),
                                )
                            })?;
                        if loss.code.local_code() != "source.kernel-dialect-unverified" {
                            continue;
                        }
                        if loss.code.namespace() != host
                            || loss.code.taxonomy() != LossTaxonomy::SourceDialectUnverified
                        {
                            return Err(AdmissionViolation::new(
                                snapshot,
                                loss_pointer,
                                format!(
                                    "kernel loss must use the {host} namespaced source_dialect_unverified code"
                                ),
                            ));
                        }
                        if loss.strict_consequence() != StrictConsequence::Reject {
                            return Err(AdmissionViolation::new(
                                snapshot,
                                loss_pointer,
                                "kernel loss must reject strict mode",
                            ));
                        }
                        observed = observed.checked_add(1).ok_or_else(|| {
                            AdmissionViolation::new(
                                snapshot,
                                pointer_child(pointer, "losses"),
                                "kernel loss count overflow",
                            )
                        })?;
                    }
                    if observed != expected {
                        return Err(AdmissionViolation::new(
                            snapshot,
                            pointer_child(pointer, "losses"),
                            format!(
                                "{} has {expected} residual Parasolid layers but {observed} typed kernel losses",
                                layers.primary().dialect()
                            ),
                        ));
                    }
                }
            }

            for (key, child) in object {
                check_kernel_loss_counts(child, snapshot, &pointer_child(pointer, key))?;
            }
        }
        serde_json::Value::Array(array) => {
            for (index, child) in array.iter().enumerate() {
                check_kernel_loss_counts(child, snapshot, &format!("{pointer}/{index}"))?;
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
    Ok(())
}

fn pointer_child(pointer: &str, segment: &str) -> String {
    let escaped = segment.replace('~', "~0").replace('/', "~1");
    format!("{pointer}/{escaped}")
}

fn collect_json_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("read directory entry").path();
        if path.is_dir() {
            collect_json_files(&path, files);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            files.push(path);
        }
    }
}

#[cfg(test)]
mod tests;
