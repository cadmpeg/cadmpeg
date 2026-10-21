// SPDX-License-Identifier: Apache-2.0
//! Active source slots obey native identities without inventing source indices.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::features::{ConfigurationId, DesignConfiguration};
use std::collections::BTreeMap;

fn configuration(
    name: &str,
    ordinal: u32,
    id: Option<u32>,
    source_index: Option<u32>,
) -> DesignConfiguration {
    DesignConfiguration {
        id: ConfigurationId::mint(format!("test:model:configuration#{name}")).unwrap(),
        ordinal,
        active: false,
        source_index,
        name: Some(name.into()),
        material: None,
        properties: id
            .map(|id| BTreeMap::from([(cadmpeg_core::nonblank_literal!("id"), id.to_string())]))
            .unwrap_or_default(),
        bodies: None,
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: Some(format!("native:{name}")),
    }
}

fn activate(configurations: Vec<DesignConfiguration>) -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.configurations = configurations;
    ir.source = Some(cadmpeg_ir::document::SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect_id!("sldprt:test"),
        )),
        BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("active_parasolid_block"),
            "Contents/Config-0-Partition".into(),
        )]),
    ));
    crate::decode::mark_active_configuration(&mut ir);
    ir
}

#[test]
fn unique_native_slot_selects_active_configuration_without_fabricating_source_index() {
    let ir = activate(vec![
        configuration("Large", 1, None, None),
        configuration("Default", 0, None, None),
    ]);
    // An ordinal alone is not a source-slot statement. Preserve that exact
    // input as the unproven contrast to the explicitly identified slot below.
    assert!(ir
        .model
        .configurations
        .iter()
        .all(|configuration| !configuration.active));
    let ir = activate(vec![
        configuration("Large", 1, None, None),
        configuration("Default", 0, Some(0), None),
    ]);
    assert!(!ir.model.configurations[0].active);
    assert!(ir.model.configurations[1].active);
    assert!(ir
        .model
        .configurations
        .iter()
        .all(|configuration| configuration.source_index.is_none()));
}

#[test]
fn explicit_native_id_precedes_ordinal_and_disagreement_remains_unresolved() {
    let ir = activate(vec![
        configuration("Ordinal", 0, None, None),
        configuration("Explicit", 7, Some(0), None),
    ]);
    assert!(!ir.model.configurations[0].active);
    assert!(ir.model.configurations[1].active);
    let ir = activate(vec![
        configuration("Ordinal", 0, None, Some(0)),
        configuration("Explicit", 7, Some(0), None),
    ]);
    assert!(ir
        .model
        .configurations
        .iter()
        .all(|configuration| !configuration.active));
    let ir = activate(vec![configuration("Disagrees", 0, None, Some(9))]);
    assert!(!ir.model.configurations[0].active);
}

#[test]
fn ambiguous_native_slots_and_duplicate_source_indices_are_not_selected() {
    for configurations in [
        vec![
            configuration("First", 0, None, None),
            configuration("Second", 0, None, None),
        ],
        vec![
            configuration("First", 1, Some(0), None),
            configuration("Second", 2, Some(0), None),
        ],
        vec![
            configuration("First", 0, None, Some(0)),
            configuration("Second", 1, None, Some(0)),
        ],
    ] {
        let ir = activate(configurations);
        assert!(ir
            .model
            .configurations
            .iter()
            .all(|configuration| !configuration.active));
    }
}
