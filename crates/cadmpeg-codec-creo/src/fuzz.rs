//! Feature-gated entry points for focused parser fuzzing.

/// Exercise Creo datum plane decoders.
pub fn datum(data: &[u8]) {
    let _ = crate::datum::planes(data);
    let _ = crate::datum::named_plane(data);
}
