// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{
    CounterdrillDiameters, HoleBottom, HoleForm, HoleKind, HoleProfileFilter, HoleThreadDepth,
    PartialPair, ThreadHand,
};

rewrite_record!(CounterdrillDiameters, []; { diameter, entry_diameter });
rewrite_enum!(HoleBottom, []; {
    Flat,
    Angled { included_angle, depth_to_tip },
});
rewrite_scalar!(HoleForm);
rewrite_enum!(HoleKind, []; {
    Unresolved(field0),
    PartialCounterbore(field0),
    PartialCountersink(field0),
    Simple,
    Chamfer { diameter, angle },
    SimpleDrilled { drill_point_angle },
    Counterbore { diameter, depth },
    CounterboreDrilled { diameter, depth, drill_point_angle },
    Countersink { diameter, angle },
    Counterdrill { diameters, depth, angle },
});
rewrite_scalar!(HoleProfileFilter);
rewrite_enum!(HoleThreadDepth, []; {
    HoleDepth,
    Blind { depth },
    TappedStandard,
});
rewrite_enum!(PartialPair<A, B>, [A, B]; {
    First(field0),
    Second(field0),
});
rewrite_scalar!(ThreadHand);
