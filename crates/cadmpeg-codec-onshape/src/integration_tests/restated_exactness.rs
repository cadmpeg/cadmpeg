// SPDX-License-Identifier: Apache-2.0
//! The exactness of a design feature names its own fields (§10), and claims
//! no more than the decoder read.

use cadmpeg_ir::design::projection::fields::{FeatureField, Field};
use cadmpeg_ir::Exactness;

use super::projection::stored_thin;
use crate::reader::id;

const SIDE: &str = "definition.operation.value.output.body.thickness.value.side";

/// The thin plate's design feature, decoded from `bytes`: its fields'
/// exactness and its wire record.
fn plate(bytes: &[u8]) -> (Vec<(String, Exactness)>, serde_json::Value) {
    let result = crate::test_support::review::decode(bytes);
    let plate = id("feature", "Plate");
    let fields = result.source_fidelity().annotations.exactness()[plate.as_str()]
        .fields()
        .iter()
        .map(|(field, exactness)| (field.to_string(), *exactness))
        .collect();
    let wire = crate::test_support::review::wire(&result);
    let record = wire["design"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|feature| feature["id"] == plate.as_str())
        .expect("the plate has a design feature")
        .clone();
    (fields, record)
}

fn resolves(record: &serde_json::Value, path: &str) -> bool {
    record
        .pointer(&format!("/{}", path.replace('.', "/")))
        .is_some()
}

/// The thin extrusion's design definition takes the derived `definition`
/// mark of its feature; the model `dependencies` and `outputs` describe the
/// model record and leave. The source states no suppression, so the design
/// feature states none (§5.10), and the model's inferred `suppressed` leaves
/// with it.
#[test]
fn stored_thin_design_feature_takes_the_exactness_of_its_model_feature() {
    let (fields, record) = plate(&stored_thin(true, false, false));
    assert_eq!(fields, vec![("definition".to_owned(), Exactness::Derived)]);
    assert!(record.get("suppressed").is_none(), "{record}");
    for (field, _) in &fields {
        let top = field.split('.').next().unwrap_or_default();
        assert!(
            FeatureField::parse(top).is_some() && resolves(&record, field),
            "`{field}` names no field of the design feature"
        );
    }
}

/// A one-sided wall over a line whose stored interval is reversed takes its
/// side from an unresolved convention (ONSHAPE-007). Only the side is
/// inferred; the rest of the design `definition` stays derived.
#[test]
fn an_assumed_thin_side_is_inferred() {
    let bytes =
        crate::test_support::review::edit(&stored_thin(true, false, false), |path, response| {
            if path.ends_with("features.json") {
                let line = &mut response["features"][0]["entities"][0];
                let start = line["startParam"].clone();
                line["startParam"] = line["endParam"].clone();
                line["endParam"] = start;
            }
        });
    let (fields, record) = plate(&bytes);
    assert_eq!(
        fields,
        vec![
            ("definition".to_owned(), Exactness::Derived),
            (SIDE.to_owned(), Exactness::Inferred)
        ]
    );
    assert!(resolves(&record, SIDE));
}

/// A timeline member the design layer restates natively (the profile sketch)
/// states suppression only where the source holds a `suppressed` or
/// `parentSuppressed` key (§5.10): the model and design features state none
/// without one, and neither takes a mark for it.
#[test]
fn a_timeline_member_states_suppression_only_where_the_source_does() {
    let sketch = id("feature", "Base sketch");
    for key in [None, Some("suppressed"), Some("parentSuppressed")] {
        let bytes = crate::test_support::review::edit(
            &stored_thin(true, false, false),
            |path, response| {
                if let (true, Some(key)) = (path.ends_with("features.json"), key) {
                    response["features"][0][key] = cadmpeg_core::json!(false);
                }
            },
        );
        let result = crate::test_support::review::decode(&bytes);
        let model = result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.id.as_str() == sketch)
            .expect("the sketch has a model feature");
        assert_eq!(model.suppressed, key.map(|_| false), "{key:?}");
        let wire = crate::test_support::review::wire(&result);
        let design = wire["design"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|feature| feature["id"] == sketch.as_str())
            .expect("the sketch has a design feature");
        assert_eq!(
            design.get("suppressed"),
            key.map(|_| cadmpeg_core::json!({"kind":"literal","value":false}))
                .as_ref(),
            "{key:?}"
        );
        assert!(
            result.source_fidelity().annotations.exactness()[sketch.as_str()]
                .fields()
                .keys()
                .all(|field| field.to_string() != "suppressed"),
            "{key:?}"
        );
    }
}
