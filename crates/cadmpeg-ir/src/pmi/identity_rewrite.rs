// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{
    DatumReference, DatumReferences, DatumTargetForm, DimensionModifier, FitStandard, FitZone,
    GeometricToleranceKind, LimitsAndFits, PmiAnnotation, PmiDefinition, PmiDimension,
    PmiDimensionKind, PmiMagnitude, PmiQuantity, PmiTarget, PmiValue,
};

rewrite_record!(DatumReference, []; {datum, precedence, common_group, modifiers});
rewrite_record!(DatumReferences, []; (field0));
rewrite_enum!(DatumTargetForm, []; {
    Point,
    Line,
    Rectangle,
    Circle,
    CircularCurve,
    Area,
    Curve,
    Other(field0),
});
rewrite_enum!(PmiDimensionKind, []; {
    Size,
    Location,
    DirectedLocation,
    Angular,
    Diameter,
    Radius,
    SphericalDiameter,
    SphericalRadius,
    CurveLength,
    Other(field0),
});
rewrite_enum!(DimensionModifier, []; {
    Reference,
    Statistical,
    ControlledRadius,
    Other(field0),
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
rewrite_record!(LimitsAndFits, []; {form_variance, zone, grade, source});
rewrite_scalar!(FitZone);
rewrite_enum!(FitStandard, []; {
    Iso286,
    AnsiB41,
    Native(field0),
});
rewrite_record!(PmiAnnotation, []; {id, name, visible, targets, definition, native_ref, native_options, source_settings});
rewrite_enum!(PmiDefinition, []; {
    Datum {identification},
    DatumSystem {references},
    DatumTarget {form, identification, basis},
    GeometricTolerance {tolerance, magnitude, defined_unit, defined_area_unit, defined_area_second_unit, datum_system, modifiers, displacement, maximum_upper, zones},
    Dimension(field0),
    Presentation {text, placement, semantics},
});
rewrite_record!(PmiDimension, []; {dimension, nominal, tolerance, decimals, modifiers});
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
