// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{CameraState, PresentationDocument, PresentationItem, PresentationLayer, PresentationState, PresentationStateKind, ViewPresentation};

rewrite_record!(CameraState, []; {position, orientation, properties});
rewrite_record!(PresentationDocument, []; {id, schema_version, active_view, states, native_ref});
rewrite_enum!(PresentationItem, []; {
    Body {body},
    Face {face},
    Edge {edge},
    Vertex {vertex},
    Point {point},
    Curve {curve},
    Surface {surface},
    Product {product},
    Occurrence {occurrence},
    Pmi {annotation},
    Tessellation {tessellation},
    Source {source_id},
});
rewrite_record!(PresentationLayer, []; {id, name, description, visible, items});
rewrite_record!(PresentationState, []; {kind, order, attributes, assets});
rewrite_enum!(PresentationStateKind, []; {
    Camera(field0),
    Native(field0),
});
rewrite_record!(ViewPresentation, []; {id, object, order, expanded, visible, display_mode, selection_style, line_width, point_size, properties, native_ref});
