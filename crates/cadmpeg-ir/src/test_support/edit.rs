// SPDX-License-Identifier: Apache-2.0
//! Reconstruct test fixtures through their production admission constructors.

/// Replace a fixture only when its reconstructed value passes admission.
/// Constructor errors leave the original fixture intact.
pub(crate) fn replace<T, E>(
    value: &mut T,
    build: impl FnOnce(&T) -> Result<T, E>,
) -> Result<(), E> {
    *value = build(value)?;
    Ok(())
}
