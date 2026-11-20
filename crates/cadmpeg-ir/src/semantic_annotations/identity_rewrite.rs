// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{SemanticAnnotation, SemanticAnnotationKind};

rewrite_record!(SemanticAnnotation, []; {id, object, kind, runtime_type, order, text, references, value, reference_anchors, text_style, format, number_format, position, dimension_kind, text_override, upper_tolerance, lower_tolerance, measured_on, basic, reflex, negated, line_directions, parameters, assets, native_ref});
rewrite_enum!(SemanticAnnotationKind, []; {
    Dimension,
    Text,
    Balloon,
    Leader,
    Symbol,
    Other,
});

rewrite_enum!(super::AnnotationDimensionKind, []; {
    Distance,
    HorizontalDistance,
    VerticalDistance,
    Radius,
    Diameter,
    Angle,
    ThreePointAngle,
    Area {holes_excluded},
    Extent {axis},
    Other {native_type},
});
rewrite_scalar!(super::ViewAxis);
rewrite_scalar!(super::MeasuredOn);
rewrite_scalar!(super::LineDirections);
rewrite_record!(super::NumberFormat, []; {prefix, suffix, decimals, trim_zeros, round_to, unit_shown});

rewrite_record!(super::AnnotationTextStyle, []; {height, font_family});
