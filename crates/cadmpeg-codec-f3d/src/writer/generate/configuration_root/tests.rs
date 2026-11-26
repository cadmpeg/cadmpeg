// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::ProductDefinitionId;

use super::{plan, ID_WIDTH};
use crate::records::configuration::{
    DesignConfiguration, DesignConfigurationActiveRow, DesignConfigurationKind,
};
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

/// `document` with a second table of id `table_id` and no rows after its
/// table.
fn with_twin(document: &DesignConfiguration, table_id: &str) -> DesignConfiguration {
    let mut payload = document.payload().clone();
    let twin = table_document(table_id, &[], &[], &[])["tables"][0].clone();
    payload["tables"].as_array_mut().unwrap().push(twin);
    DesignConfiguration::try_new(
        document.entry_name().clone(),
        DesignConfigurationKind::Table,
        payload,
    )
    .unwrap()
}

fn id(row: &str) -> cadmpeg_ir::features::ConfigurationId {
    crate::ids::neutral_configuration_id(TABLE, row)
}

/// A document over `documents` whose earlier arena states `flagged` active
/// and whose design scopes state each of `scopes` active.
fn document(
    documents: &[DesignConfiguration],
    flagged: &[cadmpeg_ir::features::ConfigurationId],
    scopes: &[cadmpeg_ir::features::ConfigurationId],
) -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.configurations = crate::design::configurations::project_configurations(
        documents,
        &crate::design::configurations::ConfigurationTargets {
            parameters: &[],
            owners: &[],
            scopes: &[],
            neutral_parameters: &[],
            neutral_features: &[],
        },
    )
    .unwrap();
    for configuration in &mut ir.model.configurations {
        configuration.active = flagged.contains(&configuration.id);
    }
    for (ordinal, scope) in scopes.iter().enumerate() {
        ir.design.scopes.push(DesignScope {
            definition: ProductDefinitionId::mint(format!("test:design:product#{ordinal}"))
                .unwrap(),
            roots: Vec::new(),
            active_configuration: Some(scope.clone()),
            active_arrangements: BTreeSet::new(),
            occurrences: BTreeMap::new(),
            current: Vec::new(),
            unproduced: BTreeSet::new(),
        });
    }
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
    let root = plan.root.map(|root| (root.table_id, root.row_id));
    (root, losses)
}

fn row(table: &str, row: &str) -> Pair {
    (table.to_owned(), row.to_owned())
}

#[test]
fn a_table_without_an_active_scope_row_takes_its_flagged_row() {
    assert_eq!(
        planned(&document(&documents(), &[id("large")], &[]), &documents()),
        (Some(row("1", "large")), Vec::new())
    );
    assert_eq!(
        planned(&document(&documents(), &[], &[]), &documents()),
        (None, Vec::new())
    );
}

#[test]
fn a_scope_row_wins_over_the_flagged_row_of_its_table_with_a_charge() {
    let (root, losses) = planned(
        &document(&documents(), &[id("large")], &[id("small")]),
        &documents(),
    );
    assert_eq!(root, Some(row("1", "small")));
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].0, "export/value.unwritable");
    assert!(losses[0].1.contains(id("large").as_str()));
    assert!(!losses[0].1.contains(id("small").as_str()));
}

#[test]
fn a_scope_row_outside_the_table_keeps_its_flagged_row() {
    let (root, losses) = planned(
        &document(&documents(), &[id("large")], &[id("absent")]),
        &documents(),
    );
    assert_eq!(root, Some(row("1", "large")));
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert!(losses[0].1.contains(id("absent").as_str()));
}

#[test]
fn a_second_scope_row_is_written_inactive_with_a_charge() {
    let (root, losses) = planned(
        &document(&documents(), &[], &[id("large"), id("small")]),
        &documents(),
    );
    assert_eq!(root, Some(row("1", "small")));
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].0, "export/value.unwritable");
    assert!(losses[0].1.contains("one DesignConfigurationRoot record"));
    assert!(losses[0].1.contains(id("large").as_str()));
}

#[test]
fn an_active_configuration_with_no_written_row_is_charged() {
    let (root, losses) = planned(&document(&documents(), &[], &[id("absent")]), &documents());
    assert!(root.is_none());
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].0, "export/value.unwritable");
    assert!(losses[0].1.contains(id("absent").as_str()));
}

#[test]
fn an_empty_row_id_drops_the_root_record_with_a_charge() {
    let documents = vec![table(TABLE, "1", &["", "large"])];
    let (root, losses) = planned(&document(&documents, &[id("")], &[]), &documents);
    assert!(root.is_none());
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].0, "export/value.unwritable");
    assert!(losses[0].1.contains(id("").as_str()));
}

#[test]
fn a_row_id_cut_that_names_another_row_is_written_unique_with_a_charge() {
    let long = "r".repeat(ID_WIDTH + 1);
    let cut = "r".repeat(ID_WIDTH);
    let documents = vec![table(TABLE, "1", &[&long, &cut])];
    let ir = document(&documents, &[id(&long)], &[]);
    let (root, losses) = planned(&ir, &documents);
    let unique = format!("{}-1", "r".repeat(ID_WIDTH - 2));
    assert_eq!(root, Some(row("1", &unique)));
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].0, "export/value.unwritable");
    assert!(losses[0].1.contains("names another row"));
    assert!(losses[0].1.contains(id(&long).as_str()));

    let written = plan(&ir, &documents).payload(0, &documents[0]).unwrap();
    let written = DesignConfiguration::try_new(
        TABLE.into(),
        DesignConfigurationKind::Table,
        cadmpeg_core::serde_erase::json::from_slice(&written).unwrap(),
    )
    .unwrap();
    let rows = written.tables()[0]
        .rows
        .iter()
        .map(|row| row.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(rows, [unique.as_str(), cut.as_str()]);
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

/// The document `configuration` at `index` as `plan` writes it.
fn written(
    plan: &super::Plan,
    index: usize,
    configuration: &DesignConfiguration,
) -> DesignConfiguration {
    DesignConfiguration::try_new(
        configuration.entry_name().clone(),
        DesignConfigurationKind::Table,
        cadmpeg_core::serde_erase::json::from_slice(&plan.payload(index, configuration).unwrap())
            .unwrap(),
    )
    .unwrap()
}

fn table_ids(document: &DesignConfiguration) -> Vec<&str> {
    document
        .tables()
        .iter()
        .map(|table| table.id.as_str())
        .collect()
}

#[test]
fn a_table_id_another_document_holds_is_written_unique_with_a_charge() {
    let documents = vec![
        table(TABLE, "1", &["small", "large"]),
        table(SECOND, "1", &[]),
    ];
    let ir = document(&documents, &[id("large")], &[]);
    let (root, losses) = planned(&ir, &documents);
    assert_eq!(root, Some(row("1-1", "large")));
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].0, "export/value.unwritable");
    assert!(losses[0].1.contains(id("large").as_str()));
    assert!(losses[0].1.contains(&documents[1].id()));

    let plan = plan(&ir, &documents);
    assert_eq!(table_ids(&written(&plan, 0, &documents[0])), ["1-1"]);
    assert_eq!(table_ids(&written(&plan, 1, &documents[1])), ["1"]);
}

#[test]
fn a_same_document_twin_of_the_table_id_is_not_renamed() {
    let documents = vec![with_twin(&table(TABLE, "1", &["large"]), "1")];
    let ir = document(&documents, &[id("large")], &[]);
    assert_eq!(
        planned(&ir, &documents),
        (Some(row("1", "large")), Vec::new())
    );

    let documents = vec![documents[0].clone(), table(SECOND, "1", &[])];
    let ir = document(&documents, &[id("large")], &[]);
    let (root, losses) = planned(&ir, &documents);
    assert_eq!(root, Some(row("1-1", "large")));
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert!(losses[0].1.contains(&documents[1].id()));
    assert!(!losses[0].1.contains(&documents[0].id()));
    let plan = plan(&ir, &documents);
    assert_eq!(table_ids(&written(&plan, 0, &documents[0])), ["1-1", "1"]);
}

#[test]
fn a_table_id_cut_that_another_document_holds_is_written_unique() {
    let long = "t".repeat(ID_WIDTH + 1);
    let cut = "t".repeat(ID_WIDTH);
    let documents = vec![table(TABLE, &long, &["large"]), table(SECOND, &cut, &[])];
    let (root, losses) = planned(&document(&documents, &[id("large")], &[]), &documents);
    let unique = format!("{}-1", "t".repeat(ID_WIDTH - 2));
    assert_eq!(root, Some(row(&unique, "large")));
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert!(losses[0].1.contains("renamed"));

    let documents = vec![with_twin(&table(TABLE, &long, &["large"]), &cut)];
    let (root, losses) = planned(&document(&documents, &[id("large")], &[]), &documents);
    assert_eq!(root, Some(row(&cut, "large")));
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert!(losses[0].1.contains("cut at a character boundary"));
}

#[test]
fn a_unique_table_id_avoids_every_other_table_id() {
    let documents = vec![
        table(TABLE, "1", &["large"]),
        table(SECOND, "1", &[]),
        table(THIRD, "1-1", &[]),
    ];
    let (root, _) = planned(&document(&documents, &[id("large")], &[]), &documents);
    assert_eq!(root, Some(row("1-2", "large")));
}

/// `document` with the stated active row `row_id` of table `table_id`.
fn stating(mut document: DesignConfiguration, table_id: &str, row_id: &str) -> DesignConfiguration {
    document
        .set_active_row(DesignConfigurationActiveRow {
            record_index: 7,
            table_id: table_id.into(),
            row_id: row_id.into(),
        })
        .unwrap();
    document
}

/// The document of [`documents`] with an empty table `2` that states row
/// `x` of table `2` active.
fn stating_an_empty_table() -> Vec<DesignConfiguration> {
    vec![stating(with_twin(&documents()[0], "2"), "2", "x")]
}

#[test]
fn a_stated_row_of_an_empty_table_is_written_as_stated() {
    let documents = stating_an_empty_table();
    assert_eq!(
        planned(&document(&documents, &[], &[]), &documents),
        (Some(row("2", "x")), Vec::new())
    );
    let documents = vec![
        table(TABLE, "1", &["small", "large"]),
        stating(table(SECOND, "2", &[]), "2", "x"),
    ];
    assert_eq!(
        planned(&document(&documents, &[], &[]), &documents),
        (Some(row("2", "x")), Vec::new())
    );
}

#[test]
fn a_stated_row_beside_a_scope_row_of_its_document_is_charged_as_replaced() {
    let documents = stating_an_empty_table();
    let (root, losses) = planned(&document(&documents, &[], &[id("large")]), &documents);
    assert_eq!(root, Some(row("1", "large")));
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].0, "export/value.unwritable");
    assert!(losses[0]
        .1
        .contains(&format!("{} table id 2 row id x", documents[0].id())));
    assert!(losses[0]
        .1
        .contains("holds one DesignConfigurationRoot record, which names another row"));
    assert!(!losses[0].1.contains("writes no active row"));
}

#[test]
fn a_stated_row_missing_from_its_populated_table_is_charged() {
    let documents = vec![stating(table(TABLE, "1", &["small", "large"]), "1", "x")];
    let (root, losses) = planned(&document(&documents, &[], &[]), &documents);
    assert!(root.is_none());
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert!(losses[0]
        .1
        .contains(&format!("{} table id 1 row id x", documents[0].id())));
    assert!(losses[0]
        .1
        .contains("a table of its table id holds other rows"));
}

#[test]
fn a_stated_row_the_reader_binds_elsewhere_is_charged() {
    let documents = vec![
        stating(table(TABLE, "1", &[]), "9", "x"),
        table(SECOND, "2", &[]),
    ];
    let (root, losses) = planned(&document(&documents, &[], &[]), &documents);
    assert!(root.is_none());
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert!(losses[0]
        .1
        .contains("does not read back as the row of the document"));
}

#[test]
fn a_unique_id_is_found_when_every_earlier_candidate_is_taken() {
    let taken = ["t-1", "t-2", "t-3"].into_iter().collect::<BTreeSet<_>>();
    assert_eq!(super::unique(super::TABLE_ID, "t", &taken), "t-4");
    let taken = ["t-1", "t-3", "x"].into_iter().collect::<BTreeSet<_>>();
    assert_eq!(super::unique(super::TABLE_ID, "t", &taken), "t-2");
}
