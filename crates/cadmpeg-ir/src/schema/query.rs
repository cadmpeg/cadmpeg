// SPDX-License-Identifier: Apache-2.0
//! Compiler-bound identities for dimension and D10 schema-query references.

mod native_links;
pub use native_links::visit_native_references;

use std::any::type_name;
use std::collections::{BTreeMap, BTreeSet};

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::Serialize;

use crate::features::{Dimension, Expression, ParameterId, PolygonSideCount};
use crate::scalar::{
    Angle, FiniteReal, Fraction, InteriorAngle, Length, NonNegativeLength, NonZeroLength,
    NonZeroReal, PositiveAngle, PositiveLength, PositiveReal, SlopeAngle,
};
use crate::sketches::{
    SketchCircularPattern, SketchConstraintDefinition, SketchConstraintDefinitionInput,
    SketchPatternDirection, SpatialSketchConstraintDefinition,
    SpatialSketchConstraintDefinitionInput,
};

/// The typed declaration that contributes a reference binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaTypeRole {
    /// The actual `Dimension<Q>` generic, for its compiled quantity type.
    Dimension,
    /// A typed constraint or pattern definition that can own D10 fields.
    D10Owner,
    /// The actual neutral design parameter identity type.
    ParameterId,
}

/// A Rust type associated with its generator-issued schema reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchemaTypeIdentity {
    /// The actual `JsonSchema::schema_id()` for this compiled type.
    pub schema_id: String,
    /// The full Rust type name, including concrete generic arguments.
    pub rust_type: String,
    /// The typed declaration's role in dimension path classification.
    pub role: SchemaTypeRole,
}

/// The finite typed bindings exported for a schema query.
#[derive(Debug, Default, Serialize)]
pub struct SchemaTypeMetadata {
    /// Generator-issued references mapped to every registered Rust alias.
    pub references: BTreeMap<String, Vec<SchemaTypeIdentity>>,
}

fn register<T: JsonSchema>(
    generator: &mut SchemaGenerator,
    original: &BTreeSet<String>,
    metadata: &mut SchemaTypeMetadata,
    role: SchemaTypeRole,
) {
    let schema = generator.subschema_for::<T>();
    let Some(reference) = schema.get("$ref").and_then(serde_json::Value::as_str) else {
        return;
    };
    let Some(name) = reference.strip_prefix("#/$defs/") else {
        return;
    };
    let name = name.replace("~1", "/").replace("~0", "~");
    if !original.contains(&name) {
        return;
    }
    let identity = SchemaTypeIdentity {
        schema_id: T::schema_id().into_owned(),
        rust_type: type_name::<T>().to_owned(),
        role,
    };
    let entries = metadata.references.entry(reference.to_owned()).or_default();
    if !entries.contains(&identity) {
        entries.push(identity);
    }
}

fn dimension<Q: JsonSchema>(
    generator: &mut SchemaGenerator,
    original: &BTreeSet<String>,
    metadata: &mut SchemaTypeMetadata,
) {
    register::<Dimension<Q>>(generator, original, metadata, SchemaTypeRole::Dimension);
}

/// Generate the unchanged CADIR schema and compiler-bound dimension/D10 identities.
///
/// Bindings cover the registered types only. An absent binding proves nothing
/// about a reference's Rust identity or dimension semantics.
#[must_use]
pub fn cadir() -> (Schema, SchemaTypeMetadata) {
    let mut generator = SchemaGenerator::default();
    let schema = generator.root_schema_for::<crate::CadIr>();
    let original: BTreeSet<String> = generator.definitions().keys().cloned().collect();
    let mut metadata = SchemaTypeMetadata::default();
    macro_rules! dimensions {
        ($($quantity:ty),+ $(,)?) => {$(
            dimension::<$quantity>(&mut generator, &original, &mut metadata);
        )+};
    }
    dimensions!(
        Angle,
        Expression,
        FiniteReal,
        Fraction,
        InteriorAngle,
        Length,
        NonNegativeLength,
        NonZeroLength,
        NonZeroReal,
        PositiveAngle,
        PositiveLength,
        PositiveReal,
        SlopeAngle,
        PolygonSideCount,
        crate::pmi::PmiValue,
        std::num::NonZeroU32,
        u32,
    );
    macro_rules! owners {
        ($($owner:ty),+ $(,)?) => {$(
            register::<$owner>(
                &mut generator, &original, &mut metadata, SchemaTypeRole::D10Owner,
            );
        )+};
    }
    owners!(
        SketchConstraintDefinition,
        SketchConstraintDefinitionInput,
        SpatialSketchConstraintDefinition,
        SpatialSketchConstraintDefinitionInput,
        SpatialSketchConstraintDefinitionInput<crate::units::UnitVector3, NonZeroLength>,
        SketchPatternDirection,
        SketchCircularPattern,
    );
    register::<ParameterId>(
        &mut generator,
        &original,
        &mut metadata,
        SchemaTypeRole::ParameterId,
    );
    (schema, metadata)
}

#[cfg(test)]
mod tests;
