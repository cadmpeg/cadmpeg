// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{Drawing, DrawingKind};

rewrite_record!(Drawing, []; {id, object, kind, runtime_type, order, visible, relationships, template, position, scale, direction, rotation_degrees, sheet, curves, template_geometry, x_direction, parameters, assets, native_ref});
rewrite_enum!(DrawingKind, []; {
    Page,
    Template,
    View,
    Projection,
    Section,
    Detail,
    Dimension,
    Annotation,
    Balloon,
    Symbol,
    Image,
    Leader,
    Other,
});

rewrite_record!(super::DrawingSheet, []; {width_mm, height_mm, fields});
rewrite_record!(super::TitleBlockField, []; {name, value});
rewrite_record!(super::DrawingCurve, []; {geometry, visibility, source, displayed});
rewrite_enum!(super::DrawingCurveGeometry, []; {
    Line {start, end},
    Circle {center, radius},
    Arc {center, radius, start, middle, end},
});
rewrite_enum!(super::CurveVisibility, []; {Visible, Hidden});
rewrite_enum!(super::CurveSource, []; {ProjectedModelEdge, Cosmetic, CenterLine});

rewrite_record!(super::TemplateGeometry, []; {curves, texts});
rewrite_record!(super::TemplateText, []; {text, position, height});
