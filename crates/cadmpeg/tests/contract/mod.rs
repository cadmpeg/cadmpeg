// SPDX-License-Identifier: Apache-2.0
//! Partial-decode and golden data contract checks.
//!
//! In a debug build (`cargo test-fast`) every sweep runs the §9.4 fixed
//! sample: the first golden of each codec in path order. A release build
//! (`cargo test --release -p cadmpeg --test contract`) runs every golden.

use std::path::PathBuf;

use cadmpeg_test_support::data_checks::DecodeGolden;

/// The §9.4 fixed sample of one codec's sweep: every value in release, the
/// first in path order in a debug build. The sample never shrinks.
fn sampled<T>(mut values: Vec<T>) -> Vec<T> {
    if cfg!(debug_assertions) {
        values.truncate(1);
    }
    values
}

/// The §9.4 fixed sample of one codec's golden sweep, in path order.
fn sampled_goldens(mut goldens: Vec<DecodeGolden>) -> Vec<DecodeGolden> {
    goldens.sort_by(|left, right| left.path.cmp(&right.path));
    sampled(goldens)
}

/// The §9.4 fixed sample of one codec's fixture paths, in path order.
fn sampled_paths(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort();
    sampled(paths)
}

mod conformance;
mod golden_data;
mod support;

mod cut_short;
mod damaged_part;
mod no_unexpected_give_ups;

#[cfg(feature = "step")]
mod one_home;

#[cfg(feature = "fcstd")]
mod freecad_one_home;
