// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{SemanticAnnotation, SemanticAnnotationKind};

rewrite_record!(SemanticAnnotation, []; {id, object, kind, runtime_type, order, text, references, value, format, position, parameters, assets, native_ref});
rewrite_enum!(SemanticAnnotationKind, []; {
    Dimension,
    Text,
    Balloon,
    Leader,
    Symbol,
    Other,
});
