// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

mod chamfer;
mod coil;
mod dispatcher;
mod extrude;
mod form;
mod mirror;
mod parameter_cycles;
mod parameters;
mod pattern;
mod pipe;
mod replace_face;
mod sheet_metal;
mod split;
mod surface;
mod timeline;
mod treatments;
