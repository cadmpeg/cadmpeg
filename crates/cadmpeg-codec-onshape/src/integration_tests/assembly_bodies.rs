// SPDX-License-Identifier: Apache-2.0
//! Body ownership across Part Studio and part definitions.
use crate::test_support::{assembly::*, read_bytes, Report};
use cadmpeg_ir::report::Severity;
use std::collections::BTreeMap;
use std::path::Path;

fn decode(bytes: &[u8]) -> crate::reader::Model {
    let mut report = Report::default();
    read_bytes(Path::new("synthetic.onshape-bundle"), bytes, &mut report).unwrap()
}
fn errors(model: &crate::reader::Model) -> Vec<String> {
    cadmpeg_ir::validate::validate_neutral(&model.ir, Vec::new())
        .unwrap()
        .findings
        .iter()
        .filter(|finding| finding.severity == Severity::Error)
        .map(|finding| format!("{finding:?}"))
        .collect()
}
/// The definitions that list each body.
fn owners(model: &crate::reader::Model) -> BTreeMap<String, Vec<String>> {
    let mut owners: BTreeMap<String, Vec<String>> = model
        .ir
        .geometry
        .bodies
        .iter()
        .map(|body| (body.id.as_str().to_owned(), Vec::new()))
        .collect();
    for definition in &model.ir.model.product_definitions {
        for body in &definition.bodies {
            owners
                .entry(body.as_str().to_owned())
                .or_default()
                .push(definition.id.as_str().to_owned());
        }
    }
    owners
}

/// Every body a Part Studio definition lists is read from that studio's file,
/// so its scope begins with the definition's key and `@` (CADIR §2.2).
fn assert_studio_bodies_scoped(model: &crate::reader::Model) {
    let mut seen = 0;
    for definition in &model.ir.model.product_definitions {
        let cadmpeg_ir::products::Storage::File {
            source: Some(_), ..
        } = &definition.storage
        else {
            continue;
        };
        let (_, key) = definition.id.as_str().split_once('#').unwrap();
        for body in &definition.bodies {
            let scope = body.as_str().split(':').nth(1).unwrap();
            assert!(
                scope.starts_with(&format!("{key}@")),
                "{body:?} under {key}"
            );
            seen += 1;
        }
    }
    assert!(seen > 0, "no studio definition lists a body");
}

#[test]
fn a_bound_body_is_listed_by_its_part_definition_only() {
    let model = decode(&bundle(definition()));
    assert_eq!(errors(&model), Vec::<String>::new());
    let owners = owners(&model);
    assert_eq!(owners.len(), 1);
    for (body, listed) in owners {
        assert_eq!(listed.len(), 1, "{body}: {listed:?}");
        assert!(listed[0].contains("part:"), "{listed:?}");
    }
}

#[test]
fn an_unbound_studio_body_stays_on_its_studio_definition() {
    // The `studio` body is bound by both instances; the `other` studio's body
    // is bound by none.
    let model = decode(&bundle_with_studios(definition(), true));
    assert_eq!(errors(&model), Vec::<String>::new());
    let owners = owners(&model);
    assert_eq!(owners.len(), 2);
    let mut kinds = Vec::new();
    for (body, listed) in &owners {
        assert_eq!(listed.len(), 1, "{body}: {listed:?}");
        kinds.push(listed[0].contains("part:"));
    }
    kinds.sort_unstable();
    assert_eq!(kinds, [false, true], "{owners:?}");
    assert_studio_bodies_scoped(&model);
}

#[test]
fn a_nested_assembly_lists_each_bound_body_once() {
    let model = decode(&bundle(nested()));
    assert_eq!(errors(&model), Vec::<String>::new());
    for (body, listed) in owners(&model) {
        assert_eq!(listed.len(), 1, "{body}: {listed:?}");
    }
}

#[test]
fn an_unbound_part_leaves_the_body_on_the_studio_definition() {
    let mut definition = definition();
    for instance in definition["rootAssembly"]["instances"]
        .as_array_mut()
        .unwrap()
    {
        instance["configuration"] = serde_json::json!("variant");
    }
    let model = decode(&bundle(definition));
    assert_eq!(errors(&model), Vec::<String>::new());
    for (body, listed) in owners(&model) {
        assert_eq!(listed.len(), 1, "{body}: {listed:?}");
        assert!(!listed[0].contains("part:"), "{listed:?}");
    }
    assert_studio_bodies_scoped(&model);
}
