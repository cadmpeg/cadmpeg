// SPDX-License-Identifier: Apache-2.0
//! Bounded samples of one immutable parameterized function.

use std::{cell::RefCell, collections::HashMap};

const MAX_CACHED_PARAMETERS: usize = 32_768;

pub(super) fn memoize<T: Copy>(evaluate: impl Fn(f64) -> Option<T>) -> impl Fn(f64) -> Option<T> {
    let samples = RefCell::new(HashMap::new());
    move |parameter| {
        let key = parameter.to_bits();
        if let Some(&value) = samples.borrow().get(&key) {
            return value;
        }
        let value = evaluate(parameter);
        let mut samples = samples.borrow_mut();
        if samples.len() < MAX_CACHED_PARAMETERS {
            samples.insert(key, value);
        }
        value
    }
}
