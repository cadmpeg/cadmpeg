// SPDX-License-Identifier: Apache-2.0
//! Shared byte-slice search over untrusted decode windows.
//!
//! Empty needles are never a match. That matches the codec helpers this
//! module replaces and avoids `memchr`'s empty-needle-at-zero behavior.

/// First offset of `needle` in `haystack`, or `None` when `needle` is empty.
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    memchr::memmem::find(haystack, needle)
}

/// First offset of `needle` at or after `from`, or `None` when `needle` is empty.
pub fn find_from(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    let tail = haystack.get(from..)?;
    find(tail, needle).map(|relative| from + relative)
}

/// First offset of `needle` in `haystack[start..end]`, returned as an absolute offset.
pub fn find_in(haystack: &[u8], needle: &[u8], start: usize, end: usize) -> Option<usize> {
    let window = haystack.get(start..end)?;
    find(window, needle).map(|relative| start + relative)
}

/// Whether `needle` occurs in `haystack`. Empty needles are absent.
pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    find(haystack, needle).is_some()
}

/// Offsets of every non-overlapping occurrence of `needle`.
///
/// An empty needle yields no offsets.
pub fn find_iter<'a>(haystack: &'a [u8], needle: &'a [u8]) -> impl Iterator<Item = usize> + 'a {
    let search = if needle.is_empty() {
        None
    } else {
        Some(memchr::memmem::find_iter(haystack, needle))
    };
    search.into_iter().flatten()
}

#[cfg(test)]
mod tests {
    use super::{contains, find, find_from, find_in, find_iter};

    #[test]
    fn empty_needle_is_never_a_match() {
        assert_eq!(find(b"abc", b""), None);
        assert_eq!(find_from(b"abc", b"", 1), None);
        assert_eq!(find_in(b"abc", b"", 0, 3), None);
        assert!(!contains(b"abc", b""));
        assert_eq!(
            find_iter(b"abc", b"").collect::<Vec<_>>(),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn finds_absolute_and_ranged_offsets() {
        let haystack = b"xxabcxxabc";
        assert_eq!(find(haystack, b"abc"), Some(2));
        assert_eq!(find_from(haystack, b"abc", 3), Some(7));
        assert_eq!(find_in(haystack, b"abc", 3, 10), Some(7));
        assert_eq!(find_in(haystack, b"abc", 3, 6), None);
        assert!(contains(haystack, b"abc"));
        assert_eq!(find_iter(haystack, b"abc").collect::<Vec<_>>(), [2, 7]);
    }
}
