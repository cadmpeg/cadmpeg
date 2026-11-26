// SPDX-License-Identifier: Apache-2.0
use super::{archive, id, part_bodies, part_studio_bodies, topology, Model, OnshapeLossCode};
use crate::test_support::{fixture, Report};
use serde_json::{json, Value};
use std::io::{Cursor, Read, Write};

/// The revolve fixture's bundle with `states` as its bodydetails, each a list of
/// (part id, part name).
fn with_parts(states: &[(usize, &[(&str, &str)])]) -> Vec<u8> {
    let fixture: &[u8] = &fixture("revolve");
    let mut archive = zip::ZipArchive::new(Cursor::new(fixture)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for i in 0..archive.len() {
        let mut member = archive.by_index(i).unwrap();
        let name = member.name().unwrap().into_owned();
        if name.contains("/bodydetails/") {
            continue;
        }
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        output.start_file(name.as_str(), options).unwrap();
        output.write_all(&bytes).unwrap();
    }
    for (ordinal, parts) in states {
        let bodies: Vec<Value> = parts
            .iter()
            .map(|(id, name)| json!({"id": id, "type": "SOLID", "properties": {"name": name}}))
            .collect();
        output
            .start_file(
                format!("partstudios/studio/bodydetails/{ordinal}.json"),
                options,
            )
            .unwrap();
        output
            .write_all(
                json!({"bodies": bodies, "microversionId": {"theId": "synthetic-microversion"}})
                    .to_string()
                    .as_bytes(),
            )
            .unwrap();
    }
    let indices: Vec<usize> = states.iter().map(|(ordinal, _)| *ordinal).collect();
    output
        .start_file("partstudios/studio/bodydetails/index.json", options)
        .unwrap();
    output
        .write_all(json!(indices).to_string().as_bytes())
        .unwrap();
    output.finish().unwrap().into_inner()
}

fn bound_parts(states: &[(usize, &[(&str, &str)])], creators: &[usize]) -> Vec<usize> {
    let mut report = Report::default();
    let mut model = crate::test_support::read_bytes(
        std::path::Path::new("synthetic.onshape-bundle"),
        &fixture("revolve"),
        &mut report,
    )
    .unwrap();
    model.ir.geometry.bodies[0].name = Some("Part B".to_owned());
    let bytes = with_parts(states);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, view) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .unwrap();
    let bundle = archive::Bundle::open(&ctx, view).unwrap();
    let inputs = topology::Inputs::new(
        &bundle,
        "partstudios/studio",
        "synthetic-microversion",
        &[],
        &mut super::Report::default(),
    );
    let bound = part_bodies(creators, &inputs, &model);
    assert!(bound
        .values()
        .all(|body| *body == model.ir.geometry.bodies[0].id));
    bound.into_keys().collect()
}

#[test]
fn a_part_binds_to_the_feature_that_made_it_by_its_unique_name() {
    // The first feature makes a part the STEP export does not hold; the second makes the
    // STEP body's part.
    let states: [(usize, &[(&str, &str)]); 4] = [
        (0, &[]),
        (1, &[("a", "Part A")]),
        (2, &[("a", "Part A")]),
        (3, &[("a", "Part A"), ("b", "Part B")]),
    ];
    assert_eq!(bound_parts(&states, &[0, 2]), [2]);
    // Without the state after a feature, its part is not guessed.
    assert!(bound_parts(&states[..3], &[0, 2]).is_empty());
    // Two features claiming the one body of that name bind neither.
    let claimed: [(usize, &[(&str, &str)]); 4] = [
        (0, &[]),
        (1, &[("b", "Part B")]),
        (2, &[]),
        (3, &[("c", "Part B")]),
    ];
    assert!(bound_parts(&claimed, &[0, 2]).is_empty());
    // A feature making two parts binds neither.
    let two: [(usize, &[(&str, &str)]); 2] = [(0, &[]), (1, &[("b", "Part B"), ("c", "Part C")])];
    assert!(bound_parts(&two, &[0]).is_empty());
}

/// The revolve fixture's model with its body owned by one of two root products, each placed
/// once; `x` moves the second placement.
fn two_part_studio(x: f64) -> Model {
    let mut report = Report::default();
    let model = crate::test_support::read_bytes(
        std::path::Path::new("synthetic.onshape-bundle"),
        &fixture("revolve"),
        &mut report,
    )
    .unwrap();
    let mut ir = model.ir.clone();
    let body = ir.geometry.bodies[0].id.as_str().to_owned();
    // As in a Part Studio's export, the part's name is on its product, not its body.
    ir.geometry.bodies[0].name = None;
    ir.model.product_definitions = cadmpeg_core::serde_erase::json::from_value(json!([
        {"id": "synthetic:test:component#first", "kind": "part", "label": "Part 1", "bodies": [body], "bom_properties": {}, "storage":{"kind":"embedded"}, "occurrences":[], "instance_bodies":[], "representations":[]},
        {"id": "synthetic:test:component#second", "kind": "part", "label": "Part 2", "bodies": [], "bom_properties": {}, "storage":{"kind":"embedded"}, "occurrences":[], "instance_bodies":[], "representations":[]},
    ]))
    .unwrap();
    let placed = |key: u32, definition: &str, x: f64| {
        json!({"id": format!("synthetic:test:occurrence#{key}"),
            "prototype": format!("synthetic:test:component#{definition}"),
            "overrides": [],
            "transform": [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]})
    };
    ir.model.occurrences = cadmpeg_core::serde_erase::json::from_value(json!([
        placed(0, "first", 0.0),
        placed(1, "second", x)
    ]))
    .unwrap();
    ir.finalize();
    Model { ir, ..model }
}

#[test]
fn part_studio_parts_are_one_design_but_moved_parts_stay_placed() {
    let mut model = two_part_studio(0.0);
    part_studio_bodies(&mut model);
    assert!(model.ir.model.occurrences.is_empty());
    assert_eq!(model.ir.model.product_definitions.len(), 2);
    assert!(model.ir.model.occurrences.is_empty());
    assert_eq!(model.ir.geometry.bodies[0].name.as_deref(), Some("Part 1"));

    let mut moved = two_part_studio(30.0);
    part_studio_bodies(&mut moved);
    assert_eq!(moved.ir.model.occurrences.len(), 2);
    assert!(!moved.ir.model.occurrences.is_empty());
}

#[test]
fn a_part_label_names_the_part_body_over_its_solid_name() {
    let mut model = two_part_studio(0.0);
    model.ir.geometry.bodies[0].name = Some("Body1".to_owned());
    part_studio_bodies(&mut model);
    assert_eq!(model.ir.geometry.bodies[0].name.as_deref(), Some("Part 1"));
}

/// The two-product studio with both placements owned by one root file product.
fn rooted_two_part_studio() -> Model {
    let model = two_part_studio(0.0);
    let mut wire = cadmpeg_core::serde_erase::json::to_value(&model.ir).unwrap();
    wire["model"]["product_definitions"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id":"synthetic:test:component#whole", "storage":{"kind":"file", "document":{}},
            "bodies":[], "bom_properties":{},
            "occurrences":["synthetic:test:occurrence#0", "synthetic:test:occurrence#1"],
            "instance_bodies":[], "representations":[]
        }));
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    }
}

#[test]
fn part_studio_flattening_flattens_a_rooted_studio_in_the_unmodified_state() {
    let mut model = rooted_two_part_studio();
    part_studio_bodies(&mut model);
    assert!(model.ir.model.occurrences.is_empty());
    assert!(model.ir.model.product_definitions[2].occurrences.is_empty());
}

#[test]
fn part_studio_flattening_keeps_insertion_state_and_nested_ownership() {
    for state in [
        "override",
        "suppressed",
        "unstated-placement",
        "hidden",
        "representation",
        "nested",
        "shared",
        "external",
    ] {
        let mut model = rooted_two_part_studio();
        let child = model.ir.model.occurrences[0].id.clone();
        match state {
            "override" => {
                model.ir.model.occurrences[0].overrides = cadmpeg_core::serde_erase::json::from_value(json!([
                    {"path":[{"occurrence":{"kind":"occurrence","value":child}}], "visible":false}
                ]))
                .unwrap();
            }
            "suppressed" => model.ir.model.occurrences[0].suppressed = Some(true),
            "unstated-placement" => model.ir.model.occurrences[0].transform = None,
            "hidden" => model.ir.model.occurrences[0].visible = Some(false),
            "representation" => {
                model.ir.model.occurrences[0].representation =
                    cadmpeg_core::serde_erase::json::from_value(json!("stored-view")).unwrap();
            }
            "nested" => {
                model.ir.model.product_definitions[2].occurrences.remove(0);
                model.ir.model.product_definitions[1]
                    .occurrences
                    .push(child);
            }
            "shared" => {
                model.ir.model.occurrences[1].prototype =
                    model.ir.model.occurrences[0].prototype.clone();
            }
            "external" => {
                let child = model.ir.model.occurrences[0].prototype.as_str().to_owned();
                let definition = model
                    .ir
                    .model
                    .product_definitions
                    .iter_mut()
                    .find(|d| d.id.as_str() == child)
                    .unwrap();
                definition.storage = cadmpeg_ir::products::Storage::External(
                    cadmpeg_ir::products::ExternalDocument::default(),
                );
            }
            _ => panic!("unknown synthetic insertion state"),
        }
        let before = model.ir.clone();
        part_studio_bodies(&mut model);
        assert_eq!(model.ir, before, "{state}");
    }
}

#[test]
fn thin_scope_requires_direct_body_ownership_without_child_insertions() {
    let mut model = two_part_studio(0.0);
    model.ir.model.product_definitions.truncate(1);
    model.ir.model.occurrences.clear();
    let definition = model.ir.model.product_definitions[0].id.clone();
    assert_eq!(super::thin::owner(&model.ir).unwrap(), definition);

    model.ir.model.product_definitions[0]
        .occurrences
        .push(cadmpeg_ir::ids::OccurrenceId::mint("synthetic:test:occurrence#child").unwrap());
    assert!(super::thin::owner(&model.ir).is_err());
    model.ir.model.product_definitions[0].occurrences.clear();
    model.ir.model.product_definitions[0].bodies.clear();
    assert!(super::thin::owner(&model.ir).is_err());
}

#[test]
fn failed_thin_scope_restores_design_and_downgrades_wall_losses() {
    let mut report = Report::default();
    let mut model = crate::test_support::read_bytes(
        std::path::Path::new("synthetic.onshape-bundle"),
        &fixture("plate-fillet"),
        &mut report,
    )
    .unwrap();
    let mut report = super::Report::default();
    let original = model.ir.design.clone();
    let feature = &model.ir.model.features[1];
    super::push(&mut model.ir.design.features, json!({"id":feature.id,"definition":{"operation":{"kind":"native","value":{"kind":"synthetic:thin","native_options":{},"source_settings":{}}}}})).unwrap();
    report.notes.push(
        crate::loss::OnshapeLossCode::ThinModelNotSupported
            .note("design construction carries the proved wall operands"),
    );
    let notes = report.notes.len();
    model.ir.model.product_definitions.clear();
    super::install_thin_scope(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        &mut model.ir,
        &mut report,
        original.clone(),
        &super::RollbackBar::default(),
    )
    .unwrap();
    assert_eq!(model.ir.design, original);
    assert_eq!(report.notes.len(), notes);
    assert!(report
        .notes
        .iter()
        .any(|note| crate::loss::OnshapeLossCode::ThinNotSupported.matches(note)));
    assert!(!report
        .notes
        .iter()
        .any(
            |note| crate::loss::OnshapeLossCode::ThinModelNotSupported.matches(note)
                || crate::loss::OnshapeLossCode::DesignOperationNative.matches(note)
        ));
}

#[test]
fn omitted_feature_has_no_placement_or_suppression_annotation() {
    let mut model = crate::test_support::read_bytes(
        std::path::Path::new("synthetic.onshape-bundle"),
        &fixture("default-plane-regions"),
        &mut Report::default(),
    )
    .unwrap();
    model.ir.model.features.clear();
    model.source_fidelity.annotations = cadmpeg_ir::annotations::Annotations::default();
    let source = json!({"featureId":"drawn-profile","featureType":"newSketch"});
    super::annotate(&mut model, &[source]).unwrap();
    for key in [
        id("sketch", "drawn-profile"),
        id("feature", "drawn-profile"),
    ] {
        assert!(!model
            .source_fidelity
            .annotations
            .exactness()
            .contains_key(&key));
    }
}

#[test]
fn sweep_losses_distinguish_support_from_malformed_records_and_budgets() {
    let mut feature = json!({"featureType":"sweep", "parameters":[
        {"parameterId":"bodyType","value":"SOLID"},
        {"parameterId":"operationType","value":"NEW"},
        {"parameterId":"profiles","queries":[]},
        {"parameterId":"path","queries":[]}
    ]});
    let support = super::mapping_loss_code(&feature, false, true, "unsupported control");
    assert!(OnshapeLossCode::SweepNotSupported.matches(&support.note("support")));
    for (budget, valid_suppression, reason) in [
        (true, true, "region budget exhausted"),
        (false, false, "malformed suppression"),
        (false, true, super::sweep::PATH_BUDGET_EXHAUSTED),
    ] {
        let code = super::mapping_loss_code(&feature, budget, valid_suppression, reason);
        assert!(OnshapeLossCode::HistoryNotSupported.matches(&code.note("history")));
    }
    feature["parameters"] = json!(null);
    let malformed = super::mapping_loss_code(&feature, false, true, "malformed parameters");
    assert!(OnshapeLossCode::HistoryNotSupported.matches(&malformed.note("record")));
}
