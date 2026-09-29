// SPDX-License-Identifier: Apache-2.0
//! Copy admitted feature fields under the caller decode resource policy.

use super::{
    ChamferGroup, ChamferSpec, FilletGroup, FullRoundFilletGroup, FullRoundSideSelection, RadiusForm, RadiusSpec, VariableRadii, VariableRadius
};

clone_record_for_decode!(ChamferGroup; { edges, spec });

clone_copy_for_decode!(ChamferSpec);

clone_record_for_decode!(FilletGroup; { edges, radius, tangency_weight });

clone_record_for_decode!(FullRoundFilletGroup; { center, side_one, side_two });

clone_enum_for_decode!(FullRoundSideSelection; {
    Automatic,
    Explicit(field0),
    Unresolved,
});

clone_copy_for_decode!(RadiusForm);

clone_enum_for_decode!(RadiusSpec; {
    Unresolved { form },
    Constant { radius },
    Chordal { chord_length },
    Asymmetric { offset_one, offset_two },
    Variable { points },
});

clone_record_for_decode!(VariableRadii; (field0));

clone_record_for_decode!(VariableRadius<P, L>, [P, L]; { parameter, radius });
