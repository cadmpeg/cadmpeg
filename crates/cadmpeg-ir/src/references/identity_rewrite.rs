// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{InstanceTarget, ReferenceEntity, ReferenceSelection, ReferenceTarget};

rewrite_record!(InstanceTarget<E>, [E]; {entity, path, state});
rewrite_record!(ReferenceSelection, []; {target, native});
rewrite_enum!(ReferenceTarget, []; {
    Null,
    Entity(field0),
    Feature(field0),
    External {document, object},
    Unresolved,
});
rewrite_enum!(ReferenceEntity, []; {
    Topology(field0),
    Definition(field0),
    Occurrence(field0),
});
