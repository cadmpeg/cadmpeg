// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{DeclaredMeasure, DeclaredQuantity, DeclaredTarget, MeasureBasis};

rewrite_enum!(DeclaredTarget, []; {
    Definition {definition},
    Bodies {bodies},
    State(field0),
    Unresolved,
});
rewrite_enum!(DeclaredQuantity, []; {
    Volume {value},
    SolidArea {value},
    WettedArea {value},
    SolidCentroid {value},
    SheetArea {value},
    SheetCentroid {value},
});
rewrite_enum!(MeasureBasis, []; {Validation, Cache});
rewrite_record!(DeclaredMeasure, []; {id, target, quantity, label, basis, precision, native_ref});
