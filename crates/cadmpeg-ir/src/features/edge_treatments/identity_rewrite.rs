// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{ChamferForm, ChamferSpec, RadiusForm};

rewrite_scalar!(ChamferForm);
rewrite_enum!(ChamferSpec, []; {
    Unresolved { form },
    Distance { distance },
    TwoDistances { first, second },
    DistanceAngle { distance, angle },
});
rewrite_scalar!(RadiusForm);
