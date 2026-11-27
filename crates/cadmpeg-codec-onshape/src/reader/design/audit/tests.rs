// SPDX-License-Identifier: Apache-2.0
use std::collections::{BTreeMap, BTreeSet};

use super::{destinations, dormant, loss_message, Source, TABLE};
use crate::reader::id;
use crate::test_support::{fixture, review};

#[test]
fn the_parameter_table_has_a_source_and_one_disposition_per_declared_slot() {
    let mut seen = BTreeSet::new();
    for line in TABLE.lines().filter(|line| !line.starts_with('#')) {
        let fields: Vec<_> = line.split('\t').collect();
        let [kind, key, upstream, destination] = fields.as_slice() else {
            panic!("invalid parameter table row: {line}");
        };
        assert!(!upstream.is_empty(), "{line}");
        assert!(seen.insert((*kind, *key)), "{line}");
        assert!(
            destination.starts_with('/')
                || matches!(
                    *destination,
                    "parameter"
                        | "constraint"
                        | "inactive"
                        | "sketch.placement"
                        | "history.parameter-not-projected"
                ),
            "{line}"
        );
    }
}

#[test]
fn every_stored_fixture_parameter_is_projected_or_named_by_a_loss() {
    for fixture_name in [
        "plate-fillet",
        "revolve",
        "hole",
        "hole-forms",
        "unsupported",
        "default-plane-regions",
        "derived",
    ] {
        let bytes = fixture(fixture_name);
        let result = review::decode(&bytes);
        let mut stored_history = BTreeMap::new();
        review::edit(&bytes, |path, response| {
            if path.ends_with("/features.json") {
                stored_history.insert(
                    path.to_owned(),
                    response["features"].as_array().unwrap().clone(),
                );
            }
        });
        for (path, history) in stored_history {
            let (prefix, _) = path.split_once("partstudios/").unwrap();
            let nested = (!prefix.is_empty()).then(|| {
                let bundle = review::members(&bytes, |members| {
                    *members = std::mem::take(members)
                        .into_iter()
                        .filter_map(|(name, bytes)| {
                            name.strip_prefix(prefix)
                                .map(|name| (name.to_owned(), bytes))
                        })
                        .collect();
                });
                review::decode(&bundle)
            });
            let result = nested.as_ref().unwrap_or(&result);
            let wire = review::wire(result);
            for (ordinal, feature) in history.iter().enumerate() {
                let kind = feature["featureType"].as_str().unwrap();
                let Some(destinations) = destinations(kind) else {
                    continue;
                };
                let native = feature["featureId"].as_str().unwrap();
                let label = feature["name"].as_str().unwrap_or(native);
                let source =
                    Source::read(feature, ordinal, None, &result.ir().model, &BTreeSet::new());
                let operation = review::design_operation(result.ir(), native);
                for row in feature["parameters"].as_array().unwrap() {
                    let key = row["parameterId"].as_str().unwrap();
                    let target = destinations
                        .get(key)
                        .unwrap_or_else(|| panic!("{fixture_name}: {kind}.{key}"));
                    if !dormant(&source, key, row) {
                        let consumed =
                            *target == "inactive"
                                || *target == "sketch.placement" && operation["kind"] == "sketch"
                                || *target == "parameter"
                                    && wire["design"]["parameters"].as_array().unwrap().iter().any(
                                        |parameter| parameter["id"] == id("parameter", native),
                                    )
                                || target.split('|').any(|path| {
                                    path.starts_with('/') && operation.pointer(path).is_some()
                                });
                        let charged = result.report().losses.iter().any(|loss| {
                            loss.code.local_code() == "history.parameter-not-projected"
                                && loss.message == loss_message(key, label)
                        });
                        assert!(
                            consumed || charged,
                            "{fixture_name}: {kind}.{key} => {target}"
                        );
                        assert!(
                            !(consumed && charged),
                            "{fixture_name}: {kind}.{key} both projected and charged"
                        );
                    }
                }
                if kind == "newSketch" {
                    for constraint in feature["constraints"].as_array().unwrap() {
                        let key = match constraint["constraintType"].as_str() {
                            Some("LENGTH" | "DIAMETER") => "length",
                            Some("DISTANCE") => "distance",
                            Some("RADIUS") => "radius",
                            Some("ANGLE") => "angle",
                            _ => continue,
                        };
                        let chart = destinations_for_dimensions();
                        let constraint_id = constraint["entityId"].as_str().unwrap();
                        let parameter_id = id("parameter", &format!("{native}/{constraint_id}"));
                        assert!(constraint["parameters"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .all(|row| {
                                row["parameterId"]
                                    .as_str()
                                    .is_some_and(|key| chart.contains_key(key))
                            }));
                        assert!(constraint["parameters"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|row| row["parameterId"] == key));
                        let parameters: &[_] = wire["design"]["parameters"]
                            .as_array()
                            .map_or(&[], Vec::as_slice);
                        assert!(
                            parameters
                                .iter()
                                .any(|parameter| parameter["id"] == parameter_id),
                            "{fixture_name}: {parameter_id}"
                        );
                    }
                }
            }
        }
    }
}

fn destinations_for_dimensions() -> &'static BTreeMap<&'static str, &'static str> {
    destinations("sketchDimension").unwrap()
}
