// SPDX-License-Identifier: Apache-2.0
//! Array occurrence ordinal text admission.

#[test]
fn product_element_decimal_ordinal_is_scoped_and_keeps_identity() {
    use crate::native::{LinkArray, LinkOccurrence, ProductNode, ProductNodeRecord};
    let record = ProductNodeRecord {
        id: "fcstd:native:product#Link".into(), object: "fcstd:native:object#Link".into(),
        node: ProductNode::Occurrence(Box::new(LinkOccurrence {
            members: Vec::new(), prototype: None, external_document: None,
            local_transform: None, placement_property: None,
            array: LinkArray::try_new(Some(2), Vec::new(), Vec::new(), Vec::new(), Vec::new()).unwrap(),
            link_transform: None, linked_subelements: Vec::new(), claim_child: None,
            copy_on_change: None, scale: None,
        })),
    };
    crate::test_support::materialized_refusal_at("fcstd product occurrence ordinal", |ctx| {
        crate::product::transfer_neutral(ctx, std::slice::from_ref(&record), &[], &[], &[], &[], &[])
    });
    crate::test_support::with_service_context(&[], |ctx| {
        let (_, occurrences) = crate::product::transfer_neutral(ctx,
            std::slice::from_ref(&record), &[], &[], &[], &[], &[]).unwrap();
        assert_eq!(occurrences.len(), 2);
        assert_eq!(occurrences[0].id.as_str(), "fcstd:model:occurrence#Link:0");
        assert_eq!(occurrences[1].id.as_str(), "fcstd:model:occurrence#Link:1");
    });
}
