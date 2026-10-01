// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{CounterdrillDiameters, HoleBottom, HoleForm, HoleKind, HoleProfileFilter, HoleThreadDepth, PartialPair, ThreadHand};

rewrite_scalar!(CounterdrillDiameters);
rewrite_scalar!(HoleBottom);
rewrite_scalar!(HoleForm);
rewrite_scalar!(HoleKind);
rewrite_scalar!(HoleProfileFilter);
rewrite_scalar!(HoleThreadDepth);
rewrite_enum!(PartialPair<A, B>, [A, B]; {
    First(field0),
    Second(field0),
});
rewrite_scalar!(ThreadHand);
