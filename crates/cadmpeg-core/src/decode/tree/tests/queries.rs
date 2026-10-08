// SPDX-License-Identifier: Apache-2.0
use crate::decode::{DecodeArena, DecodeContext, DecodePolicy};
use crate::CodecError;

#[test]
fn xml_queries_preserve_document_attribute_and_local_name_semantics() {
    let doc = roxmltree::Document::parse(
        "<!--before--><e xmlns:n='uri' n:a='first' a='second' b='é'>text<c/></e>",
    )
    .expect("document");
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let node = ctx.xml_root_element(&doc, "root").expect("root");
    assert_eq!(node, doc.root_element());
    for name in ["e", "other", ""] {
        assert_eq!(
            ctx.xml_has_tag_name(node, name, "tag").expect("tag"),
            node.has_tag_name(name)
        );
    }
    for name in ["a", "b", "absent", ""] {
        assert_eq!(
            ctx.xml_attribute(node, name, "attribute")
                .expect("attribute"),
            node.attribute(name)
        );
    }
    assert!(!ctx
        .xml_has_tag_name(doc.root(), "e", "root node")
        .expect("non-element"));
    assert_eq!(
        ctx.xml_attribute(doc.root(), "a", "root attributes")
            .expect("empty attributes"),
        None
    );
}

#[test]
fn xml_queries_refuse_on_the_exact_receiver_and_preserve_refusal() {
    let doc = roxmltree::Document::parse("<e a='1' b='2'/>").expect("document");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One attribute step and both one-byte names permit the first comparison.
    policy.limits.max_work_units = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let CodecError::ResourceLimit(first) = ctx
        .xml_attribute(doc.root_element(), "b", "attribute")
        .expect_err("second attribute step")
    else {
        panic!("resource refusal")
    };
    assert_eq!(first.used, 3);
    assert_eq!(first.additional, 1);
    assert_eq!(first.operation, "attribute");
    let CodecError::ResourceLimit(repeated) = ctx
        .xml_has_tag_name(doc.root_element(), "e", "later")
        .expect_err("original refusal")
    else {
        panic!("resource refusal")
    };
    assert_eq!(first, repeated);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        ctx.xml_root_element(&doc, "root"),
        Err(CodecError::ResourceLimit(_))
    ));
}
