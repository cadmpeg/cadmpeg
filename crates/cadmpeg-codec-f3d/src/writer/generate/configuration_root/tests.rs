// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::ProductDefinitionId;

use super::{plan, ID_WIDTH};
use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};
use crate::test_support::configuration_table::table_document;

const TABLE: &str = "FusionAssetName[Active]/DesignConfigurationTable.BlobParts/DesignConfigurationTable.synthetic.dsgcfg";
const SECOND: &str = "FusionAssetName[Active]/DesignConfigurationTable.BlobParts/DesignConfigurationTable.second.dsgcfg";
const THIRD: &str = "FusionAssetName[Active]/DesignConfigurationTable.BlobParts/DesignConfigurationTable.third.dsgcfg";

fn table(entry: &str, table_id: &str, rows: &[&str]) -> DesignConfiguration {
    let names = rows
        .iter()
        .map(|row| format!("Row {row}"))
        .collect::<Vec<_>>();
    let payload = table_document(
        table_id,
        &[],
        &rows
            .iter()
            .zip(&names)
            .map(|(row, name)| (*row, name.as_str(), Vec::new()))
            .collect::<Vec<_>>(),
        rows,
    );
    DesignConfiguration::try_new(
        entry.into(),
        DesignConfigurationKind::Table,
        payload.as_object().unwrap().clone(),
    )
    .unwrap()
}

fn documents() -> Vec<DesignConfiguration> {
    vec![table(TABLE, "1", &["small", "large"])]
}

fn two_documents() -> Vec<DesignConfiguration> {
    vec![
        table(TABLE, "1", &["small", "large"]),
        table(SECOND, "2", &["a", "b"]),
    ]
}

fn id(row: &str) -> cadmpeg_ir::features::ConfigurationId {
    crate::ids::neutral_configuration_id(TABLE, row)
}

fn second(row: &str) -> cadmpeg_ir::features::ConfigurationId {
    crate::ids::neutral_configuration_id(SECOND, row)
}

/// A document over `documents` whose earlier arena states `flagged` active
/// and whose design scope states `scope` active.
fn document(
    documents: &[DesignConfiguration],
    flagged: &[cadmpeg_ir::features::ConfigurationId],
    scope: Option<cadmpeg_ir::features::ConfigurationId>,
) -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.configurations = documents
        .chunks(1)
        .flat_map(|document| {
            crate::design::configurations::project_configurations(
                document,
                &crate::design::configurations::ConfigurationTargets {
                    parameters: &[],
                    owners: &[],
                    scopes: &[],
                    neutral_parameters: &[],
                    neutral_features: &[],
                },
            )
            .unwrap()
        })
        .collect();
    for configuration in &mut ir.model.configurations {
        configuration.active = flagged.contains(&configuration.id);
    }
    ir.design.scopes.push(DesignScope {
        definition: ProductDefinitionId::mint("test:design:product#root").unwrap(),
        roots: Vec::new(),
        active_configuration: scope,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: Vec::new(),
        unproduced: BTreeSet::new(),
    });
    ir
}

/// Two texts: a table id and a row id, or a loss code and its message.
type Pair = (String, String);

/// The table id and row id the root record of `ir` names, and the code and
/// message of each loss.
fn planned(ir: &CadIr, documents: &[DesignConfiguration]) -> (Option<Pair>, Vec<Pair>) {
    let plan = plan(ir, documents);
    let losses = plan
        .losses
        .iter()
        .map(|loss| (loss.code.to_string(), loss.message.clone()))
        .collect();
    let root = plan
        .root
        .map(|root| (root.table_id.written, root.row_id.written));
    (root, losses)
}

fn row(table: &str, row: &str) -> Pair {
    (table.to_owned(), row.to_owned())
}

#[test]
fn a_table_without_an_active_scope_row_takes_its_flagged_row() {
    assert_eq!(
        planned(&document(&documents(), &[id("large")], None), &documents()),
        (Some(row("1", "large")), Vec::new())
    );
    assert_eq!(
        planned(&document(&documents(), &[], None), &documents()),
        (None, Vec::new())
    );
}

#[test]
fn a_scope_row_wins_over_the_flagged_row_of_its_table_with_a_charge() {
    let (root, losses) = planned(
        &document(&documents(), &[id("large")], Some(id("small"))),
        &documents(),
    );
    assert_eq!(root, Some(row("1", "small")));
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].0, "export/value.unwritable");
    assert!(losses[0].1.contains(id("large").as_str()));
    assert!(!losses[0].1.contains(id("small").as_str()));
}

#[test]
fn a_table_no_scope_names_keeps_its_flagged_row() {
    let documents = two_documents();
    let (root, losses) = planned(
        &document(&documents, &[second("a")], Some(id("absent"))),
        &documents,
    );
    assert_eq!(root, Some(row("2", "a")));
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert!(losses[0].1.contains(id("absent").as_str()));
}

#[test]
fn a_second_active_configuration_is_written_inactive_with_a_charge() {
    let documents = two_documents();
    let (root, losses) = planned(
        &document(&documents, &[id("large"), second("a")], None),
        &documents,
    );
    assert_eq!(root, Some(row("1", "large")));
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].0, "export/value.unwritable");
    assert!(losses[0].1.contains("one DesignConfigurationRoot record"));
    assert!(losses[0].1.contains(second("a").as_str()));

    let (root, losses) = planned(
        &document(&documents, &[id("large")], Some(second("b"))),
        &documents,
    );
    assert_eq!(root, Some(row("2", "b")));
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert!(losses[0].1.contains(id("large").as_str()));
}

#[test]
fn an_active_configuration_with_no_written_row_is_charged() {
    let (root, losses) = planned(
        &document(&documents(), &[], Some(id("absent"))),
        &documents(),
    );
    assert!(root.is_none());
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].0, "export/value.unwritable");
    assert!(losses[0].1.contains(id("absent").as_str()));
}

#[test]
fn an_empty_row_id_drops_the_root_record_with_a_charge() {
    let documents = vec![table(TABLE, "1", &["", "large"])];
    let (root, losses) = planned(&document(&documents, &[id("")], None), &documents);
    assert!(root.is_none());
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].0, "export/value.unwritable");
    assert!(losses[0].1.contains(id("").as_str()));
}

#[test]
fn an_id_cut_that_names_another_row_drops_the_root_record_with_a_charge() {
    let long = "r".repeat(ID_WIDTH + 1);
    let cut = "r".repeat(ID_WIDTH);
    let documents = vec![table(TABLE, "1", &[&long, &cut])];
    let (root, losses) = planned(&document(&documents, &[id(&long)], None), &documents);
    assert!(root.is_none());
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert!(losses[0].1.contains("another table or row"));
}

#[test]
fn every_capacity_field_is_cut_at_a_character_boundary() {
    for field in crate::writer::CAPACITY_FIELDS {
        let widest = "\u{1F600}".repeat(field.width / 2);
        assert!(!field.fit(&widest).was_cut(), "{field:?}");
        let wider = format!("{}\u{1F600}", "a".repeat(field.width - 1));
        let fitted = field.fit(&wider);
        assert!(fitted.was_cut(), "{field:?}");
        assert_eq!(fitted.text, "a".repeat(field.width - 1));
    }
}

#[test]
fn the_generated_type_guid_is_derived_from_the_record_key() {
    assert!(super::ROOT_TYPE.guid.eq_ignore_ascii_case(
        &crate::writer::generate::presentation::deterministic_guid(
            "type",
            "DesignConfigurationRoot"
        )
    ));
}

#[test]
fn a_table_id_another_document_holds_is_written_unique_with_a_charge() {
    let documents = vec![
        table(TABLE, "1", &["small", "large"]),
        table(SECOND, "1", &["a"]),
    ];
    let ir = document(&documents, &[id("large")], None);
    let (root, losses) = planned(&ir, &documents);
    assert_eq!(root, Some(row("1-1", "large")));
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].0, "export/value.unwritable");
    assert!(losses[0].1.contains(id("large").as_str()));

    let plan = plan(&ir, &documents);
    let renamed = DesignConfiguration::try_new(
        TABLE.into(),
        DesignConfigurationKind::Table,
        cadmpeg_core::serde_erase::json::from_slice(&plan.payload(0, &documents[0]).unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(renamed.tables()[0].id, "1-1");
    let untouched = DesignConfiguration::try_new(
        SECOND.into(),
        DesignConfigurationKind::Table,
        cadmpeg_core::serde_erase::json::from_slice(&plan.payload(1, &documents[1]).unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(untouched.tables()[0].id, "1");
}

#[test]
fn a_unique_table_id_avoids_every_other_table_id() {
    let documents = vec![
        table(TABLE, "1", &["large"]),
        table(SECOND, "1", &["a"]),
        table(THIRD, "1-1", &["b"]),
    ];
    let (root, _) = planned(&document(&documents, &[id("large")], None), &documents);
    assert_eq!(root, Some(row("1-2", "large")));
}
