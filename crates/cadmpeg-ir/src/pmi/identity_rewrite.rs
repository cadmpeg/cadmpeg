// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{DatumReference, DatumReferences, DatumTargetForm, DimensionKind, DimensionTolerance, GeometricToleranceKind, LimitsAndFits, PmiAnnotation, PmiDefinition, PmiDimension, PmiMagnitude, PmiQuantity, PmiTarget, PmiValue};

rewrite_record!(DatumReference, []; {datum, precedence, common_group, modifiers});
rewrite_record!(DatumReferences, []; (field0));
rewrite_enum!(DatumTargetForm, []; {
    Point,
    Line,
    Rectangle,
    Circle,
    CircularCurve,
    Other(field0),
});
rewrite_enum!(DimensionKind, []; {
    Size,
    Location,
    Angular,
    Diameter,
    Radius,
    Other(field0),
});
rewrite_enum!(DimensionTolerance, []; {
    PlusMinus {lower, upper},
    Fit {fit},
    PlusMinusFit {lower, upper, fit},
});
rewrite_enum!(GeometricToleranceKind, []; {
    Straightness,
    Flatness,
    Roundness,
    Cylindricity,
    Coaxiality,
    LineProfile,
    SurfaceProfile,
    Angularity,
    Perpendicularity,
    Parallelism,
    Position,
    Concentricity,
    Symmetry,
    CircularRunout,
    TotalRunout,
    Other(field0),
});
rewrite_record!(LimitsAndFits, []; {form_variance, zone_variance, grade, source});
rewrite_record!(PmiAnnotation, []; {id, name, visible, targets, definition});
rewrite_enum!(PmiDefinition, []; {
    Datum {identification},
    DatumSystem {references},
    DatumTarget {form, identification, basis},
    GeometricTolerance {tolerance, magnitude, defined_unit, defined_area_unit, defined_area_second_unit, datum_system, modifiers},
    Dimension(field0),
    Presentation {text, placement, semantics},
});
rewrite_record!(PmiDimension, []; {kind, nominal, tolerance});
rewrite_scalar!(PmiMagnitude);
rewrite_scalar!(PmiQuantity);
rewrite_enum!(PmiTarget, []; {
    Body {body},
    Face {face},
    Edge {edge},
    Vertex {vertex},
    Point {point},
    Curve {curve},
    Product {product},
    Occurrence {occurrence},
    ShapeAspect {source_id},
});
rewrite_scalar!(PmiValue);
