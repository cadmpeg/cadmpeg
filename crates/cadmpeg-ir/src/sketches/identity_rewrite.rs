// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{NativeOperandField, OffsetParameter, OrderedMajorRadius, ReferenceLineDirection, SeededMembers, Sketch, SketchAxis, SketchCircularPattern, SketchCircularPatternInstance, SketchConstraint, SketchConstraintDefinition, SketchConstraintDefinitionInput, SketchCoordinateAxis, SketchDistanceMeasurement, SketchDistancePair, SketchEntity, SketchEntityUse, SketchFontWeight, SketchGeometry, SketchGeometryDefinition, SketchInternalAlignment, SketchLabelValue, SketchLocus, SketchNativeOperand, SketchOffsetPair, SketchPatternDirection, SketchPatternDistance, SketchPatternInstance, SketchPlacement, SketchPlaneFrame, SketchPolygon, SketchProfiles, SketchRectangularPattern, SketchSameCoordinate, SketchTextHorizontalAlignment, SketchTextVerticalAlignment, SpatialSketch, SpatialSketchConstraint, SpatialSketchConstraintDefinition, SpatialSketchConstraintDefinitionInput, SpatialSketchEntity, SpatialSketchEntityPair, SpatialSketchEntityUse, SpatialSketchGeometry, SpatialSketchGeometryDefinition, SpatialSketchNurbsCurve, SpatialSketchProfile, TextPlacement};

rewrite_record!(NativeOperandField, []; {name, role});
rewrite_record!(OffsetParameter, []; {id, negated});
rewrite_scalar!(OrderedMajorRadius);
rewrite_scalar!(ReferenceLineDirection);
rewrite_record!(SeededMembers<T>, [T]; {members});
rewrite_record!(Sketch, []; {id, name, configuration, visible, placement, profiles, native_ref});
rewrite_scalar!(SketchAxis);
rewrite_record!(SketchCircularPattern, []; {center, angle, angle_parameter, count_parameter, seed, instances});
rewrite_record!(SketchCircularPatternInstance, []; {angle, entities});
rewrite_record!(SketchConstraint, []; {id, sketch, definition, name, driving, active, virtual_space, visible, orientation, label_distance, label_position, metadata, native_ref});
rewrite_record!(SketchConstraintDefinition, []; (field0));
rewrite_enum!(SketchConstraintDefinitionInput, []; {
    Disabled {},
    Coincident {entities},
    Polygon {polygon},
    SplineGroup {entities},
    RectangularPattern {pattern},
    CircularPattern {pattern},
    TextFrame {text, frame},
    TextPath {text, path, glyph_transforms},
    CoincidentLoci {loci},
    SameCoordinate {relation},
    PointOnObject {point, entity},
    Midpoint {point, entity},
    PointCoordinateValues {point, values},
    MidpointCoordinate {first, second, axis, value},
    Offset {pairs, distance, parameter},
    ProjectedCopy {source, result},
    AtIntersection {point, first, second},
    Concentric {first, second},
    Coradial {first, second},
    Collinear {first, second},
    Symmetric {first, second, axis},
    PointSymmetric {first, second, center},
    Horizontal {entity},
    Vertical {entity},
    Parallel {first, second},
    Perpendicular {first, second},
    Tangent {first, second},
    TangentLoci {first, second},
    Curvature {first, second},
    Equal {first, second},
    Fixed {entity},
    ArcAngle {entity, angle},
    EllipseAngle {entity, angle},
    Distance {entities, parameter},
    DistanceLoci {first, second, parameter},
    DistanceLociValue {first, second, distance, parameter},
    PolarDistance {first, second, distance, angle, distance_parameter},
    AngleDifference {first, second, difference, value},
    ScalarEquality {first, second},
    EqualDistance {first, second},
    HorizontalDistance {first, second, parameter},
    VerticalDistance {first, second, parameter},
    RepeatedDistance {measurements, parameter},
    RepeatedLength {entities, parameter},
    ParallelLineSetDistance {first, second, parameter},
    Angle {first, second, parameter},
    AngleToAxis {entity, axis, parameter},
    Radius {entity, parameter},
    RepeatedRadius {entities, parameter},
    Diameter {entity, parameter},
    RepeatedDiameter {entities, parameter},
    SnellsLaw {incident, refracted, interface, parameter},
    Weight {entity, parameter},
    InternalAlignment {helper, parent, alignment},
    Group {elements},
    Text {elements, text, font, is_text_height},
    Native {native_kind, native_state, native_flags, native_properties, entities, parameter, operands},
});
rewrite_scalar!(SketchCoordinateAxis);
rewrite_enum!(SketchDistanceMeasurement, []; {
    Distance {first, second},
    Horizontal {first, second},
    Vertical {first, second},
});
rewrite_record!(SketchDistancePair, []; {first, second});
rewrite_record!(SketchEntity, []; {id, sketch, construction, native_ref, geometry_ref, endpoint_refs, geometry});
rewrite_record!(SketchEntityUse, []; {entity, reversed});
rewrite_scalar!(SketchFontWeight);
rewrite_record!(SketchGeometry, []; (field0));
rewrite_enum!(SketchGeometryDefinition<P, L, R, W, D, M>, [P, L, R, W, D, M]; {
    Point {position},
    Line {start, end},
    ReferenceLine {origin, direction},
    Circle {center, radius},
    Arc {center, radius, start_angle, end_angle},
    Ellipse {center, major_angle, radii, bounds},
    Hyperbola {center, major_angle, major_radius, minor_radius, bounds},
    Parabola {vertex, axis_angle, focal_length, bounds},
    Nurbs {curve},
    Text {text, font_family, font_weight, height, width_factor, placement, horizontal_alignment, vertical_alignment},
    ExternalReference {document, object, subelements},
    Native {native_kind},
});
rewrite_scalar!(SketchInternalAlignment);
rewrite_scalar!(SketchLabelValue);
rewrite_enum!(SketchLocus, []; {
    Entity(field0),
    Start(field0),
    End(field0),
    Center(field0),
});
rewrite_record!(SketchNativeOperand, []; {native_kind, field, object_index, native_ref});
rewrite_record!(SketchOffsetPair, []; {source, result, source_reversed});
rewrite_record!(SketchPatternDirection, []; {direction, spacing, distance, count_parameter});
rewrite_enum!(SketchPatternDistance, []; {
    Spacing {parameter},
    Span {parameter},
});
rewrite_record!(SketchPatternInstance, []; {entities});
rewrite_scalar!(SketchPlacement);
rewrite_scalar!(SketchPlaneFrame);
rewrite_record!(SketchPolygon, []; {entities});
rewrite_record!(SketchProfiles, []; (field0));
rewrite_record!(SketchRectangularPattern, []; {directions, rows});
rewrite_record!(SketchSameCoordinate, []; {first, second, axis});
rewrite_scalar!(SketchTextHorizontalAlignment);
rewrite_scalar!(SketchTextVerticalAlignment);
rewrite_record!(SpatialSketch, []; {id, name, configuration, visible, profiles, native_ref});
rewrite_record!(SpatialSketchConstraint, []; {id, sketch, definition, native_ref});
rewrite_record!(SpatialSketchConstraintDefinition, []; (field0));
rewrite_enum!(SpatialSketchConstraintDefinitionInput<U, L>, [U, L]; {
    Native {native_kind, native_state, parameter, operands},
    Coincident {first, second},
    Symmetric {first, second, axis},
    PointOnSurface {point, surface},
    Midpoint {point, entity},
    Tangent {first, second},
    PointDistance {first, second, parameter},
    PointLineDistance {point, line, parameter},
    LineLength {entity, parameter},
    RepeatedLineLength {entities, parameter},
    ParallelLineDistance {first, second, parameter},
    RepeatedParallelLineDistance {pairs, parameter},
    ParallelLineSetDistance {first, second, parameter},
    Offset {sources, results, normal, distance, parameter},
    ParallelToDirection {entity, direction},
    SplineGroup {entities},
});
rewrite_record!(SpatialSketchEntity, []; {id, sketch, construction, native_ref, geometry_ref, endpoint_refs, geometry});
rewrite_record!(SpatialSketchEntityPair, []; {first, second});
rewrite_record!(SpatialSketchEntityUse, []; {entity, reversed});
rewrite_record!(SpatialSketchGeometry, []; (field0));
rewrite_enum!(SpatialSketchGeometryDefinition<P, V, L>, [P, V, L]; {
    Point {position},
    Line {start, end},
    Circle {center, normal, reference_direction, radius},
    Arc {center, normal, reference_direction, radius, start_angle, end_angle},
    Nurbs {curve},
    NurbsSurface {surface},
    Native {native_kind},
});
rewrite_record!(SpatialSketchNurbsCurve, []; (field0));
rewrite_record!(SpatialSketchProfile, []; {origin, normal, u_axis, boundary});
rewrite_record!(TextPlacement<P>, [P]; {anchor, rotation});
