// SPDX-License-Identifier: Apache-2.0
//! Copy admitted feature fields under the caller decode resource policy.

use super::{
    ChamferGroup, FilletGroup, FullRoundFilletGroup, FullRoundSideSelection, RadiusSpec,
    VariableRadii, VariableRadius,
};

rewrite_record!(ChamferGroup, []; { edges, spec });

rewrite_record!(FilletGroup, []; { edges, radius, tangency_weight });

rewrite_record!(FullRoundFilletGroup, []; { center, side_one, side_two });

rewrite_enum!(FullRoundSideSelection, []; {
    Automatic,
    Explicit(field0),
    Unresolved,
});

rewrite_enum!(RadiusSpec, []; {
    Unresolved { form },
    Constant { radius },
    Chordal { chord_length },
    Asymmetric { offset_one, offset_two },
    Variable { points },
});

rewrite_record!(VariableRadii, []; (field0));

rewrite_record!(VariableRadius<P, L>, [P, L]; { parameter, radius });
