// SPDX-License-Identifier: Apache-2.0
//! Copy admitted feature fields under the caller decode resource policy.

use super::{
    HoleBottom, HoleConstruction, HoleKind, HolePlacement, HoleProfileFilter, HoleShape, HoleSpecification, HoleThreadDepth, ThreadHand
};

clone_copy_for_decode!(HoleBottom);

clone_enum_for_decode!(HoleConstruction; {
    Form { kind, specification },
    NativeThread { major_diameter, thread_depth, pitch, drill_point_angle },
});

clone_copy_for_decode!(HoleKind);

clone_enum_for_decode!(HolePlacement; {
    Directed { position, direction },
    Axis { origin, axis },
});

clone_copy_for_decode!(HoleProfileFilter);

clone_record_for_decode!(HoleShape; { construction, exit_kind, diameter });

clone_enum_for_decode!(HoleSpecification; {
    Clearance { standard, designation, fit, modeled, cosmetic, hand, depth, clearance },
    Threaded { standard, designation, class, modeled, cosmetic, pitch, major_diameter, hand, depth, clearance },
});

clone_copy_for_decode!(HoleThreadDepth);

clone_copy_for_decode!(ThreadHand);
