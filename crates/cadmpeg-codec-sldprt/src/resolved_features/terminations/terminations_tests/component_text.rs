//! Component selection text.

use crate::resolved_features::terminations::compact_surface_selection_value;

#[test]
fn surface_component_text_matches_native_text() {
    let entries = [None, Some(0), Some(u32::MAX)].map(|local_id| {
        crate::records::FeatureInputComponentPathEntry {
            instance: None,
            type_signature: [0; 12].into(),
            local_id,
        }
    });
    assert_eq!(
        compact_surface_selection_value(&entries),
        "sldprt:feature-input:surface-component-ids:_,0,4294967295"
    );
}
