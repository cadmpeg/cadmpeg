// SPDX-License-Identifier: Apache-2.0
//! STEP entity-name string constants shared by the writer and reader.
//!
//! Each constant is byte-identical to the STEP keyword it names. The writer
//! interns emitted entities by these strings and the reader matches record
//! names against them, so the two sides must agree exactly; centralizing the
//! literals here makes that agreement mechanical. Grouped by entity domain,
//! mirroring the `reader/` module split. Schema availability is not encoded
//! here; that remains the responsibility of `StepSchema::supports_*`.

// Boundary-representation topology and shells.
pub(crate) const ADVANCED_BREP_SHAPE_REPRESENTATION: &str = "ADVANCED_BREP_SHAPE_REPRESENTATION";
pub(crate) const ADVANCED_FACE: &str = "ADVANCED_FACE";
pub(crate) const BREP_WITH_VOIDS: &str = "BREP_WITH_VOIDS";
pub(crate) const CLOSED_SHELL: &str = "CLOSED_SHELL";
pub(crate) const CONNECTED_EDGE_SET: &str = "CONNECTED_EDGE_SET";
pub(crate) const EDGE_BASED_WIREFRAME_MODEL: &str = "EDGE_BASED_WIREFRAME_MODEL";
pub(crate) const EDGE_CURVE: &str = "EDGE_CURVE";
pub(crate) const EDGE_LOOP: &str = "EDGE_LOOP";
pub(crate) const FACE_BOUND: &str = "FACE_BOUND";
pub(crate) const FACE_OUTER_BOUND: &str = "FACE_OUTER_BOUND";
pub(crate) const GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION: &str =
    "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION";
pub(crate) const GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION: &str =
    "GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION";
pub(crate) const GEOMETRIC_CURVE_SET: &str = "GEOMETRIC_CURVE_SET";
pub(crate) const GEOMETRIC_SET: &str = "GEOMETRIC_SET";
pub(crate) const MANIFOLD_SOLID_BREP: &str = "MANIFOLD_SOLID_BREP";
pub(crate) const MANIFOLD_SURFACE_SHAPE_REPRESENTATION: &str =
    "MANIFOLD_SURFACE_SHAPE_REPRESENTATION";
pub(crate) const OPEN_SHELL: &str = "OPEN_SHELL";
pub(crate) const ORIENTED_CLOSED_SHELL: &str = "ORIENTED_CLOSED_SHELL";
pub(crate) const ORIENTED_EDGE: &str = "ORIENTED_EDGE";
pub(crate) const ORIENTED_OPEN_SHELL: &str = "ORIENTED_OPEN_SHELL";
pub(crate) const PCURVE: &str = "PCURVE";
pub(crate) const SEAM_CURVE: &str = "SEAM_CURVE";
pub(crate) const SHAPE_REPRESENTATION: &str = "SHAPE_REPRESENTATION";
pub(crate) const SHELL_BASED_SURFACE_MODEL: &str = "SHELL_BASED_SURFACE_MODEL";
pub(crate) const SURFACE_CURVE: &str = "SURFACE_CURVE";
pub(crate) const VERTEX_LOOP: &str = "VERTEX_LOOP";
pub(crate) const VERTEX_POINT: &str = "VERTEX_POINT";

// Geometry carriers: points, curves, surfaces, placements, representations.
pub(crate) const AXIS1_PLACEMENT: &str = "AXIS1_PLACEMENT";
pub(crate) const AXIS2_PLACEMENT_2D: &str = "AXIS2_PLACEMENT_2D";
pub(crate) const AXIS2_PLACEMENT_3D: &str = "AXIS2_PLACEMENT_3D";
pub(crate) const B_SPLINE_CURVE: &str = "B_SPLINE_CURVE";
pub(crate) const B_SPLINE_CURVE_WITH_KNOTS: &str = "B_SPLINE_CURVE_WITH_KNOTS";
pub(crate) const B_SPLINE_SURFACE: &str = "B_SPLINE_SURFACE";
pub(crate) const B_SPLINE_SURFACE_WITH_KNOTS: &str = "B_SPLINE_SURFACE_WITH_KNOTS";
pub(crate) const CARTESIAN_POINT: &str = "CARTESIAN_POINT";
pub(crate) const CARTESIAN_TRANSFORMATION_OPERATOR_3D: &str =
    "CARTESIAN_TRANSFORMATION_OPERATOR_3D";
pub(crate) const CIRCLE: &str = "CIRCLE";
pub(crate) const COMPOSITE_CURVE: &str = "COMPOSITE_CURVE";
pub(crate) const COMPOSITE_CURVE_SEGMENT: &str = "COMPOSITE_CURVE_SEGMENT";
pub(crate) const CONICAL_SURFACE: &str = "CONICAL_SURFACE";
pub(crate) const CURVE_BOUNDED_SURFACE: &str = "CURVE_BOUNDED_SURFACE";
pub(crate) const CURVE_REPLICA: &str = "CURVE_REPLICA";
pub(crate) const CYLINDRICAL_SURFACE: &str = "CYLINDRICAL_SURFACE";
pub(crate) const DEFINITIONAL_REPRESENTATION: &str = "DEFINITIONAL_REPRESENTATION";
pub(crate) const DEGENERATE_TOROIDAL_SURFACE: &str = "DEGENERATE_TOROIDAL_SURFACE";
pub(crate) const DIRECTION: &str = "DIRECTION";
pub(crate) const ELLIPSE: &str = "ELLIPSE";
pub(crate) const GEOMETRIC_REPRESENTATION_CONTEXT: &str = "GEOMETRIC_REPRESENTATION_CONTEXT";
pub(crate) const GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT: &str = "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT";
pub(crate) const GLOBAL_UNIT_ASSIGNED_CONTEXT: &str = "GLOBAL_UNIT_ASSIGNED_CONTEXT";
pub(crate) const HYPERBOLA: &str = "HYPERBOLA";
pub(crate) const LINE: &str = "LINE";
pub(crate) const OFFSET_CURVE_2D: &str = "OFFSET_CURVE_2D";
pub(crate) const OFFSET_CURVE_3D: &str = "OFFSET_CURVE_3D";
pub(crate) const OFFSET_SURFACE: &str = "OFFSET_SURFACE";
pub(crate) const OVER_RIDING_STYLED_ITEM: &str = "OVER_RIDING_STYLED_ITEM";
pub(crate) const PARABOLA: &str = "PARABOLA";
pub(crate) const PLANE: &str = "PLANE";
pub(crate) const POLYLINE: &str = "POLYLINE";
pub(crate) const RATIONAL_B_SPLINE_CURVE: &str = "RATIONAL_B_SPLINE_CURVE";
pub(crate) const RATIONAL_B_SPLINE_SURFACE: &str = "RATIONAL_B_SPLINE_SURFACE";
pub(crate) const REPRESENTATION: &str = "REPRESENTATION";
pub(crate) const REPRESENTATION_CONTEXT: &str = "REPRESENTATION_CONTEXT";
pub(crate) const SPHERICAL_SURFACE: &str = "SPHERICAL_SURFACE";
pub(crate) const STYLED_ITEM: &str = "STYLED_ITEM";
pub(crate) const SURFACE_OF_LINEAR_EXTRUSION: &str = "SURFACE_OF_LINEAR_EXTRUSION";
pub(crate) const SURFACE_OF_REVOLUTION: &str = "SURFACE_OF_REVOLUTION";
pub(crate) const SURFACE_REPLICA: &str = "SURFACE_REPLICA";
pub(crate) const TOROIDAL_SURFACE: &str = "TOROIDAL_SURFACE";
pub(crate) const TRIMMED_CURVE: &str = "TRIMMED_CURVE";
pub(crate) const VECTOR: &str = "VECTOR";

// Presentation styling, colors, layers, and draughting.
pub(crate) const COLOUR_RGB: &str = "COLOUR_RGB";
pub(crate) const CURVE_STYLE: &str = "CURVE_STYLE";
pub(crate) const DATUM: &str = "DATUM";
pub(crate) const DATUM_SYSTEM: &str = "DATUM_SYSTEM";
pub(crate) const DRAUGHTING_PRE_DEFINED_COLOUR: &str = "DRAUGHTING_PRE_DEFINED_COLOUR";
pub(crate) const DRAUGHTING_PRE_DEFINED_CURVE_FONT: &str = "DRAUGHTING_PRE_DEFINED_CURVE_FONT";
pub(crate) const FILL_AREA_STYLE: &str = "FILL_AREA_STYLE";
pub(crate) const FILL_AREA_STYLE_COLOUR: &str = "FILL_AREA_STYLE_COLOUR";
pub(crate) const INVISIBILITY: &str = "INVISIBILITY";
pub(crate) const MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION: &str =
    "MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION";
pub(crate) const NULL_STYLE: &str = "NULL_STYLE";
pub(crate) const POINT_STYLE: &str = "POINT_STYLE";
pub(crate) const PRESENTATION_LAYER_ASSIGNMENT: &str = "PRESENTATION_LAYER_ASSIGNMENT";
pub(crate) const PRESENTATION_STYLE_ASSIGNMENT: &str = "PRESENTATION_STYLE_ASSIGNMENT";
pub(crate) const SURFACE_SIDE_STYLE: &str = "SURFACE_SIDE_STYLE";
pub(crate) const SURFACE_STYLE: &str = "SURFACE_STYLE";
pub(crate) const SURFACE_STYLE_FILL_AREA: &str = "SURFACE_STYLE_FILL_AREA";
pub(crate) const SURFACE_STYLE_USAGE: &str = "SURFACE_STYLE_USAGE";

// Product structure, occurrences, and assembly relationships.
pub(crate) const APPLICATION_CONTEXT: &str = "APPLICATION_CONTEXT";
pub(crate) const APPLICATION_PROTOCOL_DEFINITION: &str = "APPLICATION_PROTOCOL_DEFINITION";
pub(crate) const CONTEXT_DEPENDENT_SHAPE_REPRESENTATION: &str =
    "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION";
pub(crate) const ITEM_DEFINED_TRANSFORMATION: &str = "ITEM_DEFINED_TRANSFORMATION";
pub(crate) const MAPPED_ITEM: &str = "MAPPED_ITEM";
pub(crate) const NEXT_ASSEMBLY_USAGE_OCCURRENCE: &str = "NEXT_ASSEMBLY_USAGE_OCCURRENCE";
pub(crate) const PRODUCT: &str = "PRODUCT";
pub(crate) const PRODUCT_CONTEXT: &str = "PRODUCT_CONTEXT";
pub(crate) const PRODUCT_DEFINITION: &str = "PRODUCT_DEFINITION";
pub(crate) const PRODUCT_DEFINITION_CONTEXT: &str = "PRODUCT_DEFINITION_CONTEXT";
pub(crate) const PRODUCT_DEFINITION_FORMATION: &str = "PRODUCT_DEFINITION_FORMATION";
pub(crate) const PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE: &str =
    "PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE";
pub(crate) const PRODUCT_DEFINITION_SHAPE: &str = "PRODUCT_DEFINITION_SHAPE";
pub(crate) const REPRESENTATION_MAP: &str = "REPRESENTATION_MAP";
pub(crate) const REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION: &str =
    "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION";
pub(crate) const SHAPE_DEFINITION_REPRESENTATION: &str = "SHAPE_DEFINITION_REPRESENTATION";

// Representation-context units and measures.
pub(crate) const CONVERSION_BASED_UNIT: &str = "CONVERSION_BASED_UNIT";
pub(crate) const LENGTH_MEASURE_WITH_UNIT: &str = "LENGTH_MEASURE_WITH_UNIT";
pub(crate) const LENGTH_UNIT: &str = "LENGTH_UNIT";
pub(crate) const MEASURE_WITH_UNIT: &str = "MEASURE_WITH_UNIT";
pub(crate) const NAMED_UNIT: &str = "NAMED_UNIT";
pub(crate) const PLANE_ANGLE_MEASURE_WITH_UNIT: &str = "PLANE_ANGLE_MEASURE_WITH_UNIT";
pub(crate) const PLANE_ANGLE_UNIT: &str = "PLANE_ANGLE_UNIT";
pub(crate) const RATIO_UNIT: &str = "RATIO_UNIT";
pub(crate) const SI_UNIT: &str = "SI_UNIT";
pub(crate) const SOLID_ANGLE_UNIT: &str = "SOLID_ANGLE_UNIT";
pub(crate) const UNCERTAINTY_MEASURE_WITH_UNIT: &str = "UNCERTAINTY_MEASURE_WITH_UNIT";

// AP242 indexed tessellation.
pub(crate) const COMPLEX_TRIANGULATED_FACE: &str = "COMPLEX_TRIANGULATED_FACE";
pub(crate) const COMPLEX_TRIANGULATED_SURFACE_SET: &str = "COMPLEX_TRIANGULATED_SURFACE_SET";
pub(crate) const COORDINATES_LIST: &str = "COORDINATES_LIST";
pub(crate) const TESSELLATED_SHAPE_REPRESENTATION: &str = "TESSELLATED_SHAPE_REPRESENTATION";
pub(crate) const TESSELLATED_SHELL: &str = "TESSELLATED_SHELL";
pub(crate) const TESSELLATED_SOLID: &str = "TESSELLATED_SOLID";
pub(crate) const TRIANGULATED_FACE: &str = "TRIANGULATED_FACE";
pub(crate) const TRIANGULATED_SURFACE_SET: &str = "TRIANGULATED_SURFACE_SET";

// Semantic and draughting product-manufacturing information.
pub(crate) const ANGULARITY_TOLERANCE: &str = "ANGULARITY_TOLERANCE";
pub(crate) const ANGULAR_LOCATION: &str = "ANGULAR_LOCATION";
pub(crate) const ANGULAR_SIZE: &str = "ANGULAR_SIZE";
pub(crate) const ANNOTATION_PLANE: &str = "ANNOTATION_PLANE";
pub(crate) const ANNOTATION_TEXT_OCCURRENCE: &str = "ANNOTATION_TEXT_OCCURRENCE";
pub(crate) const CIRCULAR_RUNOUT_TOLERANCE: &str = "CIRCULAR_RUNOUT_TOLERANCE";
pub(crate) const COMMON_DATUM_LIST: &str = "COMMON_DATUM_LIST";
pub(crate) const CONCENTRICITY_TOLERANCE: &str = "CONCENTRICITY_TOLERANCE";
pub(crate) const CYLINDRICITY_TOLERANCE: &str = "CYLINDRICITY_TOLERANCE";
pub(crate) const DATUM_FEATURE: &str = "DATUM_FEATURE";
pub(crate) const DATUM_REFERENCE_COMPARTMENT: &str = "DATUM_REFERENCE_COMPARTMENT";
pub(crate) const DATUM_REFERENCE_ELEMENT: &str = "DATUM_REFERENCE_ELEMENT";
pub(crate) const DATUM_REFERENCE_MODIFIER_WITH_VALUE: &str = "DATUM_REFERENCE_MODIFIER_WITH_VALUE";
pub(crate) const DIAMETER_SIZE: &str = "DIAMETER_SIZE";
pub(crate) const DIMENSIONAL_CHARACTERISTIC_REPRESENTATION: &str =
    "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION";
pub(crate) const DIMENSIONAL_LOCATION: &str = "DIMENSIONAL_LOCATION";
pub(crate) const DIMENSIONAL_SIZE: &str = "DIMENSIONAL_SIZE";
pub(crate) const DIMENSIONAL_SIZE_WITH_DATUM_FEATURE: &str = "DIMENSIONAL_SIZE_WITH_DATUM_FEATURE";
pub(crate) const DRAUGHTING_CALLOUT: &str = "DRAUGHTING_CALLOUT";
pub(crate) const DRAUGHTING_MODEL: &str = "DRAUGHTING_MODEL";
pub(crate) const DRAUGHTING_MODEL_ITEM_ASSOCIATION: &str = "DRAUGHTING_MODEL_ITEM_ASSOCIATION";
pub(crate) const EXTERNALLY_DEFINED_TEXT_FONT: &str = "EXTERNALLY_DEFINED_TEXT_FONT";
pub(crate) const FLATNESS_TOLERANCE: &str = "FLATNESS_TOLERANCE";
pub(crate) const GEOMETRIC_TOLERANCE: &str = "GEOMETRIC_TOLERANCE";
pub(crate) const GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE: &str =
    "GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE";
pub(crate) const LEADER_CURVE: &str = "LEADER_CURVE";
pub(crate) const LEADER_DIRECTED_CALLOUT: &str = "LEADER_DIRECTED_CALLOUT";
pub(crate) const LEADER_DIRECTED_DIMENSION: &str = "LEADER_DIRECTED_DIMENSION";
pub(crate) const LENGTH_MEASURE: &str = "LENGTH_MEASURE";
pub(crate) const LIMITS_AND_FITS: &str = "LIMITS_AND_FITS";
pub(crate) const LINE_PROFILE_TOLERANCE: &str = "LINE_PROFILE_TOLERANCE";
pub(crate) const MEASURE_REPRESENTATION_ITEM: &str = "MEASURE_REPRESENTATION_ITEM";
pub(crate) const PARALLELISM_TOLERANCE: &str = "PARALLELISM_TOLERANCE";
pub(crate) const PERPENDICULARITY_TOLERANCE: &str = "PERPENDICULARITY_TOLERANCE";
pub(crate) const PLANE_ANGLE_MEASURE: &str = "PLANE_ANGLE_MEASURE";
pub(crate) const PLUS_MINUS_TOLERANCE: &str = "PLUS_MINUS_TOLERANCE";
pub(crate) const POSITION_TOLERANCE: &str = "POSITION_TOLERANCE";
pub(crate) const RADIUS_SIZE: &str = "RADIUS_SIZE";
pub(crate) const RATIO_MEASURE: &str = "RATIO_MEASURE";
pub(crate) const ROUNDNESS_TOLERANCE: &str = "ROUNDNESS_TOLERANCE";
pub(crate) const SHAPE_ASPECT: &str = "SHAPE_ASPECT";
pub(crate) const SHAPE_DIMENSION_REPRESENTATION: &str = "SHAPE_DIMENSION_REPRESENTATION";
pub(crate) const STRAIGHTNESS_TOLERANCE: &str = "STRAIGHTNESS_TOLERANCE";
pub(crate) const SURFACE_PROFILE_TOLERANCE: &str = "SURFACE_PROFILE_TOLERANCE";
pub(crate) const SYMMETRY_TOLERANCE: &str = "SYMMETRY_TOLERANCE";
pub(crate) const TESSELLATED_ANNOTATION_OCCURRENCE: &str = "TESSELLATED_ANNOTATION_OCCURRENCE";
pub(crate) const TEXT_LITERAL: &str = "TEXT_LITERAL";
pub(crate) const TEXT_LITERAL_WITH_ASSOCIATED_CURVES: &str = "TEXT_LITERAL_WITH_ASSOCIATED_CURVES";
pub(crate) const TOLERANCE_VALUE: &str = "TOLERANCE_VALUE";
pub(crate) const TOTAL_RUNOUT_TOLERANCE: &str = "TOTAL_RUNOUT_TOLERANCE";

// External documents, sources, and references.
pub(crate) const APPLIED_DOCUMENT_REFERENCE: &str = "APPLIED_DOCUMENT_REFERENCE";
pub(crate) const DOCUMENT: &str = "DOCUMENT";
pub(crate) const DOCUMENT_FILE: &str = "DOCUMENT_FILE";
pub(crate) const DOCUMENT_REFERENCE: &str = "DOCUMENT_REFERENCE";
pub(crate) const EXTERNALLY_DEFINED_ITEM: &str = "EXTERNALLY_DEFINED_ITEM";
pub(crate) const EXTERNAL_SOURCE: &str = "EXTERNAL_SOURCE";

// Validation properties and derived measures.
pub(crate) const AREA_MEASURE: &str = "AREA_MEASURE";
pub(crate) const DERIVED_UNIT: &str = "DERIVED_UNIT";
pub(crate) const PROPERTY_DEFINITION: &str = "PROPERTY_DEFINITION";
pub(crate) const PROPERTY_DEFINITION_REPRESENTATION: &str = "PROPERTY_DEFINITION_REPRESENTATION";
pub(crate) const VOLUME_MEASURE: &str = "VOLUME_MEASURE";

// Exchange-structure header records.
pub(crate) const FILE_SCHEMA: &str = "FILE_SCHEMA";
