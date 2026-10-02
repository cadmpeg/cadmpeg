// SPDX-License-Identifier: Apache-2.0
pub fn copy<T: Clone>(value: &T) -> T {
    value.clone()
}
pub fn fixed(bytes: &[u8]) -> usize {
    bytes.len()
}
pub fn with<F: FnOnce() -> usize>(callback: F) -> usize {
    callback()
}
