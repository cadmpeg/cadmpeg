// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{ReferenceSelection, ReferenceTarget};

rewrite_record!(ReferenceSelection, []; {target, subelements});
rewrite_enum!(ReferenceTarget, []; {
    Null,
    Local(field0),
    External {document, object},
});
