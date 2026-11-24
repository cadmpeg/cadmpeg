// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{DrawingPart, InstanceTarget, ReferenceEntity, ReferenceSelection, ReferenceTarget};

rewrite_record!(InstanceTarget<E>, [E]; {entity, path, state});
rewrite_record!(ReferenceSelection, []; {target, native});
rewrite_enum!(ReferenceTarget, []; {
    Null,
    Entity(field0),
    Feature(field0),
    Drawing {drawing, part},
    Annotation(field0),
    Pmi(field0),
    External {document, object},
    Unresolved,
});
rewrite_enum!(DrawingPart, []; {
    Curve(field0),
    Unread,
});
rewrite_enum!(ReferenceEntity, []; {
    Topology(field0),
    Definition(field0),
    Occurrence(field0),
});
