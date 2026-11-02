// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{DeclaredMeasure, DeclaredQuantity, DeclaredTarget};

rewrite_enum!(DeclaredTarget, []; {
    Definition {definition},
    Bodies {bodies},
    Unresolved {},
});
rewrite_enum!(DeclaredQuantity, []; {
    Volume {value},
    SolidArea {value},
    WettedArea {value},
    SolidCentroid {value},
    SheetArea {value},
    SheetCentroid {value},
});
rewrite_record!(DeclaredMeasure, []; {id, target, quantity, label, native_ref});
