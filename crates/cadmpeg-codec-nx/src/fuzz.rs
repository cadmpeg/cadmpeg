//! Feature-gated entry points for focused parser fuzzing.

/// Exercise the NX deltas walker.
pub fn deltas(data: &[u8]) {
    let _ = crate::deltas::walk(data);
}

/// Exercise NX object-model indexed section framing.
pub fn om(data: &[u8]) {
    for section in crate::om::indexed_sections(data) {
        let _ = section.numeric_expressions();
    }
}
