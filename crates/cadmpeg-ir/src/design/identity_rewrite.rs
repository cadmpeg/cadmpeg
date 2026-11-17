// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by the design fields that model records hold.

use super::names::NativeName;
use super::parameter::Deviations;
use super::refs::ConfigurationRef;
use super::selection::{OccurrencePath, PathStep, StepRef};
use super::units::UnitSystem;

rewrite_record!(OccurrencePath, []; (field0));
rewrite_record!(PathStep, []; {occurrence, configuration});
rewrite_enum!(StepRef, []; {
    Occurrence(field0),
    Native(field0),
});
rewrite_scalar!(NativeName);
rewrite_enum!(ConfigurationRef, []; {
    Local(field0),
    Named(field0),
});
rewrite_scalar!(UnitSystem);
rewrite_scalar!(Deviations);
