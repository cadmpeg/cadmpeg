// SPDX-License-Identifier: Apache-2.0
//! The schema feature generates a schema for every sketch, datum, and shared
//! operation type.

use schemars::{JsonSchema, SchemaGenerator};

use crate::design::ops::datum::{Datum, DatumElement};
use crate::design::ops::shared::{Locus, RunSides, Spacing, UpToFill};
use crate::design::ops::sketch::{
    AngleOperands, AngleSector, BlockInstance, Contact, DefiningRole, Derivation, DerivationMode,
    DistanceAxis, EntityDefinition, EntityForm, EntityRole, InterpolatedSpline, Operand, Planar,
    PlanarCurve, RadialMeasure, Sketch, SketchConstraint, SketchEntity, SketchOffset,
    SketchPattern, SketchPatternAxis, SketchPatternDirection, SketchPatternInstance,
    SketchPatternLayout, SketchPlane, SketchPolygon, SketchRelation, SketchSlot, SketchSolve,
    SketchSpace, SketchText, SlotKind, SolveState, Spatial, SpatialCurve, SpatialSketch,
    SplineHandle, TextAlign, TextContent, TextHorizontalBox, TextLayout, TextVerticalBox,
};
use crate::scalar::{Angle, Length};
use crate::units::UnitVector2;

fn generates<T: JsonSchema>(generator: &mut SchemaGenerator) {
    let schema = T::json_schema(generator);
    assert!(schema.as_object().is_some(), "{}", T::schema_name());
}

fn generates_in<S>(generator: &mut SchemaGenerator)
where
    S: SketchSpace + JsonSchema,
    S::Sketch: JsonSchema,
    S::Entity: JsonSchema,
    S::Constraint: JsonSchema,
    S::Direction: JsonSchema,
    S::Curve: JsonSchema,
{
    generates::<SketchEntity<S>>(generator);
    generates::<EntityDefinition<S>>(generator);
    generates::<EntityForm<S>>(generator);
    generates::<EntityRole<S>>(generator);
    generates::<InterpolatedSpline<S>>(generator);
    generates::<SplineHandle<S>>(generator);
    generates::<SketchText<S>>(generator);
    generates::<TextLayout<S>>(generator);
    generates::<SketchConstraint<S>>(generator);
    generates::<Operand<S>>(generator);
    generates::<SketchRelation<S>>(generator);
    generates::<Contact<S>>(generator);
    generates::<DistanceAxis<S>>(generator);
    generates::<AngleOperands<S>>(generator);
    generates::<SketchOffset<S>>(generator);
    generates::<SketchPolygon<S>>(generator);
    generates::<SketchSlot<S>>(generator);
    generates::<SketchPattern<S>>(generator);
    generates::<SketchPatternLayout<S>>(generator);
    generates::<SketchPatternAxis<S>>(generator);
    generates::<SketchPatternDirection<S>>(generator);
    generates::<SketchPatternInstance<S>>(generator);
}

#[test]
fn every_sketch_type_has_a_schema() {
    let mut generator = SchemaGenerator::default();
    generates::<Sketch>(&mut generator);
    generates::<SketchPlane>(&mut generator);
    generates::<SpatialSketch>(&mut generator);
    generates::<SketchSolve>(&mut generator);
    generates::<SolveState>(&mut generator);
    generates::<Planar>(&mut generator);
    generates::<Spatial>(&mut generator);
    generates::<DefiningRole>(&mut generator);
    generates::<PlanarCurve>(&mut generator);
    generates::<SpatialCurve>(&mut generator);
    generates::<UnitVector2>(&mut generator);
    generates::<Derivation>(&mut generator);
    generates::<DerivationMode>(&mut generator);
    generates::<TextContent>(&mut generator);
    generates::<TextAlign>(&mut generator);
    generates::<TextHorizontalBox>(&mut generator);
    generates::<TextVerticalBox>(&mut generator);
    generates::<BlockInstance>(&mut generator);
    generates::<AngleSector>(&mut generator);
    generates::<RadialMeasure>(&mut generator);
    generates::<SlotKind>(&mut generator);
    generates_in::<Planar>(&mut generator);
    generates_in::<Spatial>(&mut generator);
}

#[test]
fn every_datum_and_shared_type_has_a_schema() {
    let mut generator = SchemaGenerator::default();
    generates::<Datum>(&mut generator);
    generates::<DatumElement>(&mut generator);
    generates::<RunSides<Length>>(&mut generator);
    generates::<RunSides<Angle>>(&mut generator);
    generates::<Spacing<Length>>(&mut generator);
    generates::<Spacing<Angle>>(&mut generator);
    generates::<UpToFill<Length>>(&mut generator);
    generates::<UpToFill<Angle>>(&mut generator);
    generates::<Locus>(&mut generator);
}
