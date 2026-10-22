// SPDX-License-Identifier: Apache-2.0
//! Resolve native-source controls for projection assertions.

pub(crate) fn source_value<Q: cadmpeg_ir::features::DimensionValue>(
    dimension: &cadmpeg_ir::Dimension<Q>,
    features: &[crate::records::Feature],
) -> Q {
    crate::history::project::source_dimension_value(
        dimension,
        &[crate::records::FeatureHistory {
            id: "source-dimension-test".into(),
            part_name: None,
            properties: std::collections::BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: features.to_vec(),
        }],
    )
    .expect("dimension resolves from the source parameter")
}
