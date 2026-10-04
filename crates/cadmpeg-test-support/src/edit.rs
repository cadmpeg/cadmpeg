// SPDX-License-Identifier: Apache-2.0
//! Reconstruct test fixtures through their production admission constructors.

/// Replace a fixture only when its reconstructed value passes admission.
/// Constructor errors leave the original fixture intact.
pub fn replace<T, E>(value: &mut T, build: impl FnOnce(&T) -> Result<T, E>) -> Result<(), E> {
    *value = build(value)?;
    Ok(())
}

/// Reconstruct a fixture and retain an output computed during the edit.
/// Failed admission preserves the original fixture.
pub fn with_output<T, R, E>(
    value: &mut T,
    build: impl FnOnce(&T) -> Result<(T, R), E>,
) -> Result<R, E> {
    let (candidate, output) = build(value)?;
    *value = candidate;
    Ok(output)
}
