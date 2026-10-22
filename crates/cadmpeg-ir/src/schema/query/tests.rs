// SPDX-License-Identifier: Apache-2.0
//! Compiler-bound schema references without name or object-shape inference.

use super::*;

mod first {
    #[derive(schemars::JsonSchema)]
    pub(super) struct Twin;
}

mod second {
    #[derive(schemars::JsonSchema)]
    pub(super) struct Twin;
}

#[derive(schemars::JsonSchema)]
#[serde(transparent)]
struct Transparent(first::Twin);

#[derive(schemars::JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum Imitation {
    Literal(Length),
    Parameter(ParameterId),
}

#[test]
fn query_metadata_leaves_the_entire_existing_generated_schema_unchanged() {
    let (schema, metadata) = cadir();
    assert_eq!(schema, crate::cadir_json_schema());
    assert!(!metadata.references.is_empty());
    for reference in metadata.references.keys() {
        let pointer = reference
            .strip_prefix('#')
            .expect("local generated reference");
        assert!(schema.as_value().pointer(pointer).is_some(), "{reference}");
    }
}

#[test]
fn same_named_types_bind_to_distinct_generator_issued_refs_and_real_ids() {
    let mut generator = SchemaGenerator::default();
    let schema = generator.root_schema_for::<(first::Twin, second::Twin)>();
    let original = generator.definitions().keys().cloned().collect();
    let mut metadata = SchemaTypeMetadata::default();
    register::<first::Twin>(
        &mut generator,
        &original,
        &mut metadata,
        SchemaTypeRole::D10Owner,
    );
    register::<second::Twin>(
        &mut generator,
        &original,
        &mut metadata,
        SchemaTypeRole::D10Owner,
    );
    assert_eq!(metadata.references.len(), 2);
    for (reference, entries) in metadata.references {
        assert_eq!(entries.len(), 1);
        assert!(schema.as_value().pointer(&reference[1..]).is_some());
        let expected = if entries[0].rust_type == type_name::<first::Twin>() {
            first::Twin::schema_id()
        } else {
            assert_eq!(entries[0].rust_type, type_name::<second::Twin>());
            second::Twin::schema_id()
        };
        assert_eq!(entries[0].schema_id.as_str(), expected.as_ref());
    }
    assert_ne!(first::Twin::schema_id(), second::Twin::schema_id());
    assert_eq!(first::Twin::schema_name(), second::Twin::schema_name());
}

#[test]
fn transparent_aliases_preserve_both_actual_rust_bindings_on_the_shared_ref() {
    let mut generator = SchemaGenerator::default();
    generator.root_schema_for::<(first::Twin, Transparent)>();
    let original = generator.definitions().keys().cloned().collect();
    let mut metadata = SchemaTypeMetadata::default();
    register::<first::Twin>(
        &mut generator,
        &original,
        &mut metadata,
        SchemaTypeRole::D10Owner,
    );
    register::<Transparent>(
        &mut generator,
        &original,
        &mut metadata,
        SchemaTypeRole::D10Owner,
    );
    assert_eq!(metadata.references.len(), 1);
    let aliases = metadata
        .references
        .values()
        .next()
        .expect("actual shared reference");
    assert_eq!(aliases.len(), 2);
    assert_eq!(aliases[0].schema_id, aliases[1].schema_id);
    assert_ne!(aliases[0].rust_type, aliases[1].rust_type);
    let value = Transparent(first::Twin);
    let first::Twin = value.0;
}

#[test]
fn registering_a_type_absent_from_the_original_root_never_advertises_it() {
    let mut generator = SchemaGenerator::default();
    let schema = generator.root_schema_for::<u32>();
    let original = generator.definitions().keys().cloned().collect();
    let mut metadata = SchemaTypeMetadata::default();
    dimension::<Length>(&mut generator, &original, &mut metadata);
    assert!(metadata.references.is_empty());
    assert_eq!(schema, schemars::schema_for!(u32));
}

#[test]
fn actual_dimension_generic_identity_is_exported_without_classifying_an_imitation() {
    let mut generator = SchemaGenerator::default();
    generator.root_schema_for::<(Dimension<Length>, Imitation)>();
    let original = generator.definitions().keys().cloned().collect();
    let mut metadata = SchemaTypeMetadata::default();
    dimension::<Length>(&mut generator, &original, &mut metadata);
    assert_eq!(metadata.references.len(), 1);
    let identity = &metadata
        .references
        .values()
        .next()
        .expect("dimension binding")[0];
    assert_eq!(
        identity.schema_id.as_str(),
        Dimension::<Length>::schema_id().as_ref()
    );
    assert_eq!(identity.rust_type, type_name::<Dimension<Length>>());
    assert_eq!(identity.role, SchemaTypeRole::Dimension);
    let literal = Imitation::Literal(Length::new(3.0).expect("finite length"));
    let Imitation::Literal(value) = literal else {
        panic!("literal control");
    };
    assert_eq!(value.get(), 3.0);
    let parameter = Imitation::Parameter(ParameterId::mint("test:parameter:value#0").expect("id"));
    let Imitation::Parameter(id) = parameter else {
        panic!("parameter control");
    };
    assert_eq!(id.as_str(), "test:parameter:value#0");
}

#[test]
fn actual_constraint_and_parameter_bindings_use_their_compiled_trait_ids() {
    let (_, metadata) = cadir();
    for (rust_type, schema_id, role) in [
        (
            type_name::<SketchConstraintDefinition>(),
            SketchConstraintDefinition::schema_id(),
            SchemaTypeRole::D10Owner,
        ),
        (
            type_name::<SpatialSketchConstraintDefinition>(),
            SpatialSketchConstraintDefinition::schema_id(),
            SchemaTypeRole::D10Owner,
        ),
        (
            type_name::<ParameterId>(),
            ParameterId::schema_id(),
            SchemaTypeRole::ParameterId,
        ),
    ] {
        let identity = metadata
            .references
            .values()
            .flatten()
            .find(|identity| identity.rust_type == rust_type)
            .expect("actual reachable compiled type has a binding");
        assert_eq!(identity.schema_id.as_str(), schema_id.as_ref());
        assert_eq!(identity.role, role);
    }
}
