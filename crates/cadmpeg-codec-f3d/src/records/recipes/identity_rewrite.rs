// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{ConstructionRecipeDesign, ConstructionRecipeKind, ConstructionRecipeSelector, CreationTimestamp};

rewrite_native_record!(ConstructionRecipeDesign<Id>, [Id]; {id, selector});
rewrite_native_scalar!(ConstructionRecipeKind);
rewrite_native_scalar!(ConstructionRecipeSelector);
rewrite_native_record!(CreationTimestamp, []; {id, target, record_index, unix_microseconds});
