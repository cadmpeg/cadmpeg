// SPDX-License-Identifier: Apache-2.0
//! The schema feature generates a schema for every sketch, datum, and shared
//! operation type.

use schemars::{schema_for, JsonSchema};

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

fn generates<T: JsonSchema>() {
    let schema = serde_json::to_value(schema_for!(T)).unwrap();
    assert!(schema.is_object(), "{}", T::schema_name());
}

fn generates_in<S>()
where
    S: SketchSpace + JsonSchema,
    S::Sketch: JsonSchema,
    S::Entity: JsonSchema,
    S::Constraint: JsonSchema,
    S::Direction: JsonSchema,
    S::Curve: JsonSchema,
{
    generates::<SketchEntity<S>>();
    generates::<EntityDefinition<S>>();
    generates::<EntityForm<S>>();
    generates::<EntityRole<S>>();
    generates::<InterpolatedSpline<S>>();
    generates::<SplineHandle<S>>();
    generates::<SketchText<S>>();
    generates::<TextLayout<S>>();
    generates::<SketchConstraint<S>>();
    generates::<Operand<S>>();
    generates::<SketchRelation<S>>();
    generates::<Contact<S>>();
    generates::<DistanceAxis<S>>();
    generates::<AngleOperands<S>>();
    generates::<SketchOffset<S>>();
    generates::<SketchPolygon<S>>();
    generates::<SketchSlot<S>>();
    generates::<SketchPattern<S>>();
    generates::<SketchPatternLayout<S>>();
    generates::<SketchPatternAxis<S>>();
    generates::<SketchPatternDirection<S>>();
    generates::<SketchPatternInstance<S>>();
}

#[test]
fn every_sketch_type_has_a_schema() {
    generates::<Sketch>();
    generates::<SketchPlane>();
    generates::<SpatialSketch>();
    generates::<SketchSolve>();
    generates::<SolveState>();
    generates::<Planar>();
    generates::<Spatial>();
    generates::<DefiningRole>();
    generates::<PlanarCurve>();
    generates::<SpatialCurve>();
    generates::<UnitVector2>();
    generates::<Derivation>();
    generates::<DerivationMode>();
    generates::<TextContent>();
    generates::<TextAlign>();
    generates::<TextHorizontalBox>();
    generates::<TextVerticalBox>();
    generates::<BlockInstance>();
    generates::<AngleSector>();
    generates::<RadialMeasure>();
    generates::<SlotKind>();
    generates_in::<Planar>();
    generates_in::<Spatial>();
}

#[test]
fn every_datum_and_shared_type_has_a_schema() {
    generates::<Datum>();
    generates::<DatumElement>();
    generates::<RunSides<Length>>();
    generates::<RunSides<Angle>>();
    generates::<Spacing<Length>>();
    generates::<Spacing<Angle>>();
    generates::<UpToFill<Length>>();
    generates::<UpToFill<Angle>>();
    generates::<Locus>();
}
