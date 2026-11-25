// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::ProductDefinitionId;

use super::plan;
use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};
use crate::test_support::configuration_table::table_document;

const TABLE: &str = "FusionAssetName[Active]/DesignConfigurationTable.BlobParts/DesignConfigurationTable.synthetic.dsgcfg";

fn documents() -> Vec<DesignConfiguration> {
    let payload = table_document(
        "1",
        &[],
        &[
            ("small", "Small", Vec::new()),
            ("large", "Large", Vec::new()),
        ],
        &["small", "large"],
    );
    vec![DesignConfiguration::try_new(
        TABLE.into(),
        DesignConfigurationKind::Table,
        payload.as_object().unwrap().clone(),
    )
    .unwrap()]
}

fn id(row: &str) -> cadmpeg_ir::features::ConfigurationId {
    crate::ids::neutral_configuration_id(TABLE, row)
}

/// A document whose earlier arena states row `legacy` active and whose
/// design scope states `scope` active.
fn document(legacy: Option<&str>, scope: Option<&str>) -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.configurations = crate::design::configurations::project_configurations(
        &documents(),
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
        configuration.active = legacy.is_some_and(|row| configuration.id == id(row));
    }
    ir.design.scopes.push(DesignScope {
        definition: ProductDefinitionId::mint("test:design:product#root").unwrap(),
        roots: Vec::new(),
        active_configuration: scope.map(id),
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: Vec::new(),
        unproduced: BTreeSet::new(),
    });
    ir
}

fn planned_row(ir: &CadIr) -> Option<(String, String)> {
    let (root, losses) = plan(ir, &documents());
    assert!(losses.is_empty(), "{losses:#?}");
    root.map(|root| (root.table_id, root.row_id))
}

#[test]
fn the_root_record_names_the_active_configuration_of_the_design_scope() {
    let row = |row: &str| Some(("1".to_owned(), row.to_owned()));
    assert_eq!(
        planned_row(&document(Some("large"), Some("small"))),
        row("small")
    );
    assert_eq!(planned_row(&document(Some("large"), None)), row("large"));
    assert_eq!(planned_row(&document(None, None)), None);
}

#[test]
fn an_active_configuration_with_no_written_row_is_charged() {
    let mut ir = document(None, None);
    ir.design.scopes[0].active_configuration = Some(id("absent"));
    let (root, losses) = plan(&ir, &documents());
    assert!(root.is_none());
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.to_string(), "export/value.unwritable");
    assert!(losses[0].message.contains(id("absent").as_str()));
}
