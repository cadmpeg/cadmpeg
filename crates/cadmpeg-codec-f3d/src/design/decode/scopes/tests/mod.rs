// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]

pub(super) use super::*;

mod assembly;
mod combine;
mod component_insert;
mod derived_instance;
mod existing;
mod extrude_class_296;
mod extrude_coil;
mod extrude_extent;
mod fixed_kind_operations;
mod fixed_kind_tail;
mod flange;
mod hem;
mod history_admission;
mod legacy_class_397;
mod mirror;
mod named_empty_label;
mod named_variable_tail;
mod pattern;
mod prelude;
mod scale;
mod surfaces;
mod thicken;
mod thread;
mod work_point;
