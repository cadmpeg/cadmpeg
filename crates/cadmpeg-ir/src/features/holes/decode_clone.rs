// SPDX-License-Identifier: Apache-2.0
//! Copy admitted feature fields under the caller decode resource policy.

use super::{HoleConstruction, HolePlacement, HoleShape, HoleSpecification};

rewrite_enum!(HoleConstruction, []; {
    Form { kind, specification },
    NativeThread { major_diameter, thread_depth, pitch, drill_point_angle },
});

rewrite_enum!(HolePlacement, []; {
    Directed { position, direction },
    Axis { origin, axis },
});

rewrite_record!(HoleShape, []; { construction, exit_kind, diameter });

rewrite_enum!(HoleSpecification, []; {
    Clearance { standard, designation, fit, modeled, cosmetic, hand, depth, clearance },
    Threaded { standard, designation, class, modeled, cosmetic, pitch, major_diameter, hand, depth, clearance },
});
