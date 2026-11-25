// SPDX-License-Identifier: Apache-2.0
//! Part 10, Scope and linkage: decode sets (§2.2, §3.1, §8.1).

use crate::document::CadIr;
use crate::hash::digest::Sha256Digest;
use crate::ids::{OccurrenceId, ProductDefinitionId};
use crate::products::{ExternalDocument, Occurrence, ProductDefinition, Storage};
use crate::report::check::Finding;
use crate::report::Severity;
use crate::source_fidelity::SourceFidelity;

fn source() -> crate::document::SourceMeta {
    crate::document::SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect_id!("test:root"),
        )),
        std::collections::BTreeMap::new(),
    )
}

fn definition(id: &str) -> ProductDefinitionId {
    id.try_into().unwrap()
}

/// A root file that places the component file `pin`, whose definition owns
/// occurrence `occurrence` of an embedded part of that file.
fn decode_set(occurrence: &str) -> CadIr {
    let mut ir = CadIr::decoded(source());
    let mut root = ProductDefinition::root(
        definition("test:model:product-definition#root"),
        ExternalDocument::default(),
    );
    root.occurrences
        .push("test:model:occurrence#pin".try_into().unwrap());
    let mut pin = ProductDefinition::new(
        definition("test:model:product-definition#pin"),
        Storage::File {
            document: ExternalDocument::default(),
            source: Some(source()),
        },
    );
    pin.occurrences.push(occurrence.try_into().unwrap());
    let part = ProductDefinition::new(
        definition("test:pin@model:product-definition#part"),
        Storage::Embedded,
    );
    ir.model.product_definitions = vec![pin, root, part];
    ir.model.occurrences = vec![
        Occurrence::new(
            OccurrenceId::try_from(occurrence.to_owned()).unwrap(),
            definition("test:pin@model:product-definition#part"),
            None,
        ),
        Occurrence::new(
            "test:model:occurrence#pin".try_into().unwrap(),
            definition("test:model:product-definition#pin"),
            None,
        ),
    ];
    ir.model
        .occurrences
        .sort_by(|left, right| left.id.cmp(&right.id));
    ir
}

fn messages(ir: &CadIr, fragment: &str) -> Vec<(Severity, Option<String>)> {
    crate::validate::validate_neutral(ir, Vec::new())
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| finding.message.contains(fragment))
        .map(|finding| (finding.severity, finding.entity))
        .collect()
}

#[test]
fn a_qualified_scope_begins_with_the_key_of_a_file_definition() {
    const FRAGMENT: &str = "the key of no `File` definition";
    assert_eq!(
        messages(&decode_set("test:pin@model:occurrence#a"), FRAGMENT),
        []
    );
    for key in ["bolt", "root"] {
        let id = format!("test:{key}@model:occurrence#a");
        let mut ir = decode_set("test:pin@model:occurrence#a");
        ir.model.occurrences[0].id = id.as_str().try_into().unwrap();
        assert_eq!(messages(&ir, FRAGMENT), [(Severity::Error, Some(id))]);
    }
}

#[test]
fn records_of_a_component_file_carry_its_key() {
    const FRAGMENT: &str = "does not begin its scope with that file's key `pin@`";
    assert_eq!(
        messages(&decode_set("test:pin@model:occurrence#a"), FRAGMENT),
        []
    );
    assert_eq!(
        messages(&decode_set("test:model:occurrence#a"), FRAGMENT),
        [(Severity::Error, Some("test:model:occurrence#a".to_owned()))]
    );
}

/// `decode_set` with a design scope for `pin` whose timeline holds
/// `feature`, and a body of `pin` with region `region`.
fn with_design_and_topology(feature: &str, region: &str) -> CadIr {
    use crate::design::container::{Container, OrderedKind};
    use crate::design::refs::Member;
    use crate::design::scope::DesignScope;

    let mut ir = decode_set("test:pin@model:occurrence#a");
    let pin = definition("test:model:product-definition#pin");
    let timeline = "test:pin@design:container#timeline";
    ir.design.scopes.push(DesignScope {
        definition: pin.clone(),
        roots: vec![timeline.try_into().unwrap()],
        active_configuration: None,
        active_arrangements: std::collections::BTreeSet::new(),
        occurrences: std::collections::BTreeMap::new(),
        current: Vec::new(),
        unproduced: std::collections::BTreeSet::new(),
    });
    ir.design.containers.push(Container::Ordered {
        id: timeline.try_into().unwrap(),
        kind: OrderedKind::Timeline,
        members: vec![Member::Feature(feature.try_into().unwrap())],
        folders: Vec::new(),
        rollback: None,
        placement: None,
    });
    let body: crate::ids::BodyId = "test:pin@model:body#b".try_into().unwrap();
    ir.model
        .product_definitions
        .iter_mut()
        .find(|row| row.id == pin)
        .unwrap()
        .bodies
        .push(body.clone());
    ir.geometry.regions.push(crate::topology::Region::new(
        region.try_into().unwrap(),
        body,
        Vec::new(),
    ));
    ir
}

#[test]
fn features_and_topology_of_a_component_file_carry_its_key() {
    const FRAGMENT: &str = "does not begin its scope with that file's key `pin@`";
    let qualified =
        with_design_and_topology("test:pin@design:feature#301", "test:pin@model:region#r");
    assert_eq!(messages(&qualified, FRAGMENT), []);
    let unqualified = with_design_and_topology("test:design:feature#301", "test:model:region#r");
    assert_eq!(
        messages(&unqualified, FRAGMENT),
        [
            (Severity::Error, Some("test:design:feature#301".to_owned())),
            (Severity::Error, Some("test:model:region#r".to_owned())),
        ]
    );
}

fn digest_findings(ir: &CadIr, key: &str) -> Vec<Finding> {
    let mut fidelity = SourceFidelity::new(Sha256Digest::digest(&[]));
    fidelity
        .file_digests
        .insert(definition(key), Sha256Digest::digest(&[1]));
    let mut findings = Vec::new();
    super::check_file_digests(ir, &fidelity, &mut findings);
    findings
}

#[test]
fn file_digests_name_component_files_only() {
    let ir = decode_set("test:pin@model:occurrence#a");
    assert!(digest_findings(&ir, "test:model:product-definition#pin").is_empty());
    for (key, fragment) in [
        (
            "test:model:product-definition#root",
            "for the root definition",
        ),
        (
            "test:pin@model:product-definition#part",
            "read from no file of its own",
        ),
        (
            "test:model:product-definition#missing",
            "for no product definition",
        ),
    ] {
        let findings = digest_findings(&ir, key);
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert_eq!(findings[0].severity, Severity::Error);
        assert_eq!(findings[0].entity.as_deref(), Some(key));
        assert!(findings[0].message.contains(fragment), "{findings:#?}");
    }
}
