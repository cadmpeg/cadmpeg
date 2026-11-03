// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
use cadmpeg_ir::{Annotations, CadIr};

pub(super) fn validate(
    ir: &CadIr,
    annotations: &Annotations,
    sections: &[(String, Vec<u8>)],
) -> Result<(), CodecError> {
    for attribute in super::metadata_attributes(ir)
        .into_iter()
        .filter(|attribute| attribute.name == "configuration_manager")
    {
        let valid = attribute.target == AttributeTarget::Document
            && annotations
                .provenance
                .get(attribute.id.as_str())
                .is_some_and(|provenance| {
                    let Ok(offset) = usize::try_from(provenance.offset) else {
                        return false;
                    };
                    sections
                        .iter()
                        .find(|(name, _)| name == provenance.stream())
                        .and_then(|(_, payload)| {
                            crate::metadata::configuration_manager::values(payload, offset)
                        })
                        .is_some_and(|values| {
                            attribute.values == values.map(AttributeValue::Integer)
                        })
                });
        if !valid {
            return Err(CodecError::NotImplemented(
                "SLDPRT configuration-manager edits and generation require an owned version-specific serializer and proven scalar roles".into(),
            ));
        }
    }
    Ok(())
}
