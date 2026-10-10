// SPDX-License-Identifier: Apache-2.0
//! Fixed-width byte fields of Design record frames.

/// The `N` bytes at `at`, when the frame holds them. Comparing the result with
/// a literal reads at most `N` bytes.
pub(in crate::design::decode) fn bytes_at<const N: usize>(
    bytes: &[u8],
    at: usize,
) -> Option<&[u8; N]> {
    bytes.get(at..)?.first_chunk::<N>()
}

/// Whether the `N` bytes at `at` are all zero.
pub(in crate::design::decode) fn zeros_at<const N: usize>(bytes: &[u8], at: usize) -> bool {
    bytes_at::<N>(bytes, at) == Some(&[0; N])
}
