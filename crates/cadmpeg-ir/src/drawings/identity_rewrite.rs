// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{Drawing, DrawingKind};

rewrite_record!(Drawing, []; {id, object, kind, runtime_type, order, visible, relationships, template, position, scale, direction, rotation_degrees, parameters, assets, native_ref});
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
