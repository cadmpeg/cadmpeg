#![allow(clippy::unwrap_used)]
use super::bind;
use crate::native::CatiaNative;
use crate::test_support::test_formula::standard_catpart_with_schema_field_roles;

#[test]
fn schema_field_roles_keep_source_declarations_offsets_and_target_class() {
    let native = CatiaNative::decode(&standard_catpart_with_schema_field_roles(
        &[8],
        &[(8, 6)],
        "body",
    ));
    let link = &native.schema_configuration_row_chains[0].links()[0];
    assert_eq!(link.row.entity_id(), 5);
    assert_eq!(
        link.intervening_entities
            .as_ref()
            .expect("ordered interval")[0]
            .entity_id(),
        6
    );
    assert_eq!(link.field_roles.len(), 1);
    let role = &link.field_roles[0];
    assert_eq!(role.target.entity_id(), 6);
    assert_eq!(role.target.class_name(), Some("body"));
    assert_eq!(role.name.as_deref(), Some("_FirstRole"));
    let source = native
        .entity_records
        .iter()
        .find(|e| e.entity_id == 4)
        .expect("source entity");
    let target = native
        .entity_records
        .iter()
        .find(|e| e.entity_id == 6)
        .expect("target entity");
    assert_eq!(role.target.entity(), Some(target.id.as_str()));
    assert_eq!(role.target_object, target.object_record);
    assert_eq!(role.incidences.len(), 1);
    let incidence = &role.incidences[0];
    assert_eq!(incidence.source_entity, source.id);
    assert_eq!(incidence.source_object, source.object_record);
    assert_eq!(incidence.schema_ordinal, 8);
    assert_eq!(
        incidence.schema_entry,
        source.value_schema_selections[0].entry
    );
    assert_eq!(incidence.schema_name, "_FirstRole");
    assert_eq!(incidence.declaration_offsets, [0]);
    assert_eq!(incidence.selector_payload_offset, 0);
    assert_eq!(incidence.target_payload_offset, 5);
}

#[test]
fn schema_field_roles_abstain_without_value_declaration_or_interval_target() {
    for (selectors, assignments, class) in [
        (vec![], vec![(8, 6)], "body"),
        (vec![], vec![(8, 6)], "_FirstRole"),
        (vec![9], vec![(8, 6)], "body"),
        (vec![99], vec![(99, 6)], "body"),
        (vec![8], vec![(8, 7)], "body"),
        (vec![8], vec![(8, 99)], "body"),
        (vec![8], vec![], "_FirstRole"),
    ] {
        let native = CatiaNative::decode(&standard_catpart_with_schema_field_roles(
            &selectors,
            &assignments,
            class,
        ));
        let source = native
            .entity_records
            .iter()
            .find(|e| e.entity_id == 4)
            .expect("source entity");
        assert_eq!(source.definition_schema_selections.len(), 2);
        assert!(source
            .definition_schema_selections
            .iter()
            .all(|s| s.name.as_deref() == Some("_FirstRole")));
        assert!(native.schema_configuration_row_chains[0].links()[0]
            .field_roles
            .is_empty());
    }
}

#[test]
fn schema_field_roles_require_atom_targets() {
    let mut bytes = standard_catpart_with_schema_field_roles(&[8], &[(8, 6)], "body");
    let payload = [0x32, 8, 0, 0, 0, 0x80, 6, 0, 0, 0, 0xfe];
    let at = bytes
        .windows(payload.len())
        .position(|part| part == payload)
        .expect("authored source payload");
    bytes[at + 5] = 0x32;
    let native = CatiaNative::decode(&bytes);
    let source = native
        .object_graphs
        .iter()
        .flat_map(|g| &g.records)
        .find(|o| o.entity_id() == Some(4))
        .expect("source object");
    assert!(matches!(
        source.payload.fields[1],
        crate::object_graph::PayloadField::Reference { value: 6, .. }
    ));
    assert!(native.schema_configuration_row_chains[0].links()[0]
        .field_roles
        .is_empty());
}

#[test]
fn schema_field_roles_keep_duplicate_occurrences_and_declarations() {
    let native = CatiaNative::decode(&standard_catpart_with_schema_field_roles(
        &[8, 8],
        &[(8, 6), (8, 6)],
        "body",
    ));
    let role = &native.schema_configuration_row_chains[0].links()[0].field_roles[0];
    assert_eq!(role.name.as_deref(), Some("_FirstRole"));
    assert_eq!(role.incidences.len(), 2);
    assert_eq!(
        role.incidences
            .iter()
            .map(|i| (i.selector_payload_offset, i.target_payload_offset))
            .collect::<Vec<_>>(),
        [(0, 5), (10, 15)]
    );
    assert!(role
        .incidences
        .iter()
        .all(|i| i.declaration_offsets == [0, 5]));
}

#[test]
fn schema_field_roles_preserve_ambiguous_names_without_selecting_one() {
    let native = CatiaNative::decode(&standard_catpart_with_schema_field_roles(
        &[8, 9],
        &[(8, 6), (9, 6)],
        "body",
    ));
    let role = &native.schema_configuration_row_chains[0].links()[0].field_roles[0];
    assert!(role.name.is_none());
    assert_eq!(role.target.class_name(), Some("body"));
    assert_eq!(
        role.incidences
            .iter()
            .map(|i| i.schema_name.as_str())
            .collect::<Vec<_>>(),
        ["_FirstRole", "_SecondRole"]
    );
    let wire =
        serde_json::to_value(&native.schema_configuration_row_chains[0]).expect("serialize roles");
    assert!(wire["links"][0]["field_roles"][0].get("name").is_none());
}

#[test]
fn schema_field_roles_require_unique_same_graph_paired_records() {
    for control in [
        "source-object-missing",
        "target-object-missing",
        "source-graph",
        "target-graph",
        "duplicate-source-entity",
        "duplicate-target-entity",
        "duplicate-source-object",
        "duplicate-target-object",
        "source-pair-mismatch",
        "target-pair-mismatch",
    ] {
        let mut native = CatiaNative::decode(&standard_catpart_with_schema_field_roles(
            &[8],
            &[(8, 6)],
            "body",
        ));
        match control {
            "source-object-missing" => native
                .entity_records
                .iter_mut()
                .find(|e| e.entity_id == 4)
                .expect("source")
                .object_record
                .push_str(":missing"),
            "target-object-missing" => native
                .entity_records
                .iter_mut()
                .find(|e| e.entity_id == 6)
                .expect("target")
                .object_record
                .push_str(":missing"),
            "source-graph" => native
                .entity_records
                .iter_mut()
                .find(|e| e.entity_id == 4)
                .expect("source")
                .object_graph
                .push_str(":other"),
            "target-graph" => native
                .entity_records
                .iter_mut()
                .find(|e| e.entity_id == 6)
                .expect("target")
                .object_graph
                .push_str(":other"),
            "duplicate-source-entity" | "duplicate-target-entity" => {
                let identity = if control == "duplicate-source-entity" {
                    4
                } else {
                    6
                };
                let duplicate = native
                    .entity_records
                    .iter()
                    .find(|e| e.entity_id == identity)
                    .expect("entity")
                    .clone();
                native.entity_records.push(duplicate);
            }
            "duplicate-source-object" | "duplicate-target-object" => {
                let identity = if control == "duplicate-source-object" {
                    4
                } else {
                    6
                };
                let graph = native
                    .object_graphs
                    .iter_mut()
                    .find(|g| g.records.iter().any(|o| o.entity_id() == Some(identity)))
                    .expect("graph");
                let duplicate = graph
                    .records
                    .iter()
                    .find(|o| o.entity_id() == Some(identity))
                    .expect("object")
                    .clone();
                graph.records.push(duplicate);
            }
            "source-pair-mismatch" | "target-pair-mismatch" => {
                let identity = if control == "source-pair-mismatch" {
                    4
                } else {
                    6
                };
                let other = if identity == 4 { 6 } else { 4 };
                let wrong = native
                    .entity_records
                    .iter()
                    .find(|e| e.entity_id == other)
                    .expect("other")
                    .object_record
                    .clone();
                native
                    .entity_records
                    .iter_mut()
                    .find(|e| e.entity_id == identity)
                    .expect("entity")
                    .object_record = wrong;
            }
            _ => unreachable!("named control"),
        }
        crate::test_support::with_service_context(|ctx| {
            bind(
                ctx,
                &mut native.schema_configuration_row_chains,
                &native.entity_records,
                native.object_graphs.iter().flat_map(|g| &g.records),
            )
        })
        .expect("invalid ownership abstains");
        assert!(
            native.schema_configuration_row_chains[0].links()[0]
                .field_roles
                .is_empty(),
            "{control}"
        );
    }
}

#[test]
fn schema_field_roles_roundtrip_and_reject_forged_derived_metadata() {
    let native = CatiaNative::decode(&standard_catpart_with_schema_field_roles(
        &[8],
        &[(8, 6)],
        "body",
    ));
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store complete roles");
    let loaded = CatiaNative::load(&namespace).expect("load canonical roles");
    assert_eq!(
        loaded.schema_configuration_row_chains,
        native.schema_configuration_row_chains
    );
    for control in ["name", "incidence-offset", "target-object", "omit-roles"] {
        let mut changed = native.clone();
        let link = &mut changed.schema_configuration_row_chains[0].links_mut()[0];
        match control {
            "name" => link.field_roles[0].name = Some("forged".to_owned()),
            "incidence-offset" => link.field_roles[0].incidences[0].selector_payload_offset += 1,
            "target-object" => link.field_roles[0].target_object.push_str(":wrong"),
            "omit-roles" => link.field_roles.clear(),
            _ => unreachable!("named control"),
        }
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        changed
            .store(&mut namespace)
            .expect("store forged derived view for validation");
        assert!(CatiaNative::load(&namespace).is_err(), "{control}");
    }
}

#[test]
fn schema_field_roles_charge_retained_metadata_before_copying() {
    let mut native = CatiaNative::decode(&standard_catpart_with_schema_field_roles(
        &[8],
        &[(8, 6)],
        "body",
    ));
    let error = crate::test_support::with_retained_limit(0, |ctx| {
        bind(
            ctx,
            &mut native.schema_configuration_row_chains,
            &native.entity_records,
            native.object_graphs.iter().flat_map(|g| &g.records),
        )
    })
    .expect_err("role metadata requires retained space");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes && limit.operation == "retain CATIA schema-configuration field roles")
    );
}

#[test]
fn field_role_name_has_one_absence_spelling() {
    let native = CatiaNative::decode(&standard_catpart_with_schema_field_roles(
        &[8],
        &[(8, 6)],
        "body",
    ));
    let role = &native.schema_configuration_row_chains[0].links()[0].field_roles[0];
    let mut value = serde_json::to_value(role).unwrap();
    assert!(
        serde_json::from_value::<super::CatiaSchemaConfigurationFieldRole>(value.clone()).is_ok()
    );
    value.as_object_mut().unwrap().remove("name");
    let absent: super::CatiaSchemaConfigurationFieldRole =
        serde_json::from_value(value.clone()).unwrap();
    assert!(absent.name.is_none());
    value["name"] = serde_json::Value::Null;
    let error =
        serde_json::from_value::<super::CatiaSchemaConfigurationFieldRole>(value).unwrap_err();
    assert!(error.to_string().contains("name"));
}
