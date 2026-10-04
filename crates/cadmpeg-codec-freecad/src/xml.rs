// SPDX-License-Identifier: Apache-2.0
//! XML document and attribute searches under the decode work budget.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

pub(crate) fn document_element<'a, 'input>(
    ctx: &DecodeContext<'_>,
    document: &'a roxmltree::Document<'input>,
) -> Result<roxmltree::Node<'a, 'input>, CodecError> {
    let mut next = document.root().first_child();
    while let Some(node) = next {
        ctx.charge_work(1, "FreeCAD XML document element search")?;
        if node.is_element() {
            return Ok(node);
        }
        next = node.next_sibling();
    }
    Err(CodecError::Malformed("XML document has no root element".into()))
}

pub(crate) fn attribute<'a, 'input>(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'a, 'input>,
    name: &str,
) -> Result<Option<&'a str>, CodecError> {
    let mut attributes = node.attributes();
    while attributes.len() != 0 {
        ctx.charge_work(1, "FreeCAD XML attribute search")?;
        let Some(attribute) = attributes.next() else {
            break;
        };
        if ctx.equal(attribute.name(), name, "FreeCAD XML attribute name")? {
            return Ok(Some(attribute.value()));
        }
    }
    Ok(None)
}

pub(crate) fn children<'ctx, 'a, 'input>(
    ctx: &'ctx DecodeContext<'_>,
    node: roxmltree::Node<'a, 'input>,
    operation: &'static str,
) -> Result<(Vec<roxmltree::Node<'a, 'input>>, ScopedReservation<'ctx>), CodecError> {
    ctx.with_scoped_storage(operation, || ctx.collect_vec(node.children(), operation))
}

pub(crate) fn descendants<'ctx, 'a, 'input>(
    ctx: &'ctx DecodeContext<'_>,
    node: roxmltree::Node<'a, 'input>,
    operation: &'static str,
) -> Result<(Vec<roxmltree::Node<'a, 'input>>, ScopedReservation<'ctx>), CodecError> {
    ctx.with_scoped_storage(operation, || ctx.collect_vec(node.descendants(), operation))
}

pub(crate) fn has_tag_name(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
    name: &str,
) -> Result<bool, CodecError> {
    Ok(node.is_element()
        && ctx.equal(node.tag_name().name(), name, "FreeCAD XML element name")?)
}

#[cfg(test)]
mod tests {
    #[test]
    fn xml_searches_preserve_document_and_local_attribute_selection() {
        for source in [
            "<!-- before --><?meta value?><root a='value'/>",
            "<root xmlns:n='urn:n' n:a='first' a='second'/>",
            "<root xmlns:n='urn:n' a='first' n:a='second'/>",
            "<root α='unicode'/>",
        ] {
            let document = roxmltree::Document::parse(source).expect("XML");
            crate::test_support::with_service_context(&[], |ctx| {
                let root = super::document_element(ctx, &document).expect("document element");
                assert_eq!(root, document.root_element());
                for name in ["a", "α", "missing"] {
                    assert_eq!(super::attribute(ctx, root, name).expect("attribute"), root.attribute(name));
                }
            });
        }
    }

    #[test]
    fn xml_node_lists_preserve_source_order_and_charge_scoped_storage() {
        let document = roxmltree::Document::parse("<root>text<!-- comment --><a/><b><c/></b></root>").expect("XML");
        crate::test_support::with_service_context(&[], |ctx| {
            let root = document.root_element();
            let (children, _children_storage) = super::children(ctx, root, "XML children").expect("children");
            assert_eq!(children, root.children().collect::<Vec<_>>());
            let (descendants, _descendants_storage) = super::descendants(ctx, root, "XML descendants").expect("descendants");
            assert_eq!(descendants, root.descendants().collect::<Vec<_>>());
        });
        for descendants in [false, true] {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_materialized_bytes = 0;
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let result = if descendants {
                super::descendants(&ctx, document.root_element(), "XML descendants")
            } else {
                super::children(&ctx, document.root_element(), "XML children")
            };
            assert!(matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                    && Some(limit) == ctx.resource_refusal()));
        }
    }

    #[test]
    fn xml_element_comparison_preserves_local_name_and_node_kind() {
        for source in ["<root/>", "<n:root xmlns:n='urn:n'/>", "<α/>"] {
            let document = roxmltree::Document::parse(source).expect("XML");
            crate::test_support::with_service_context(&[], |ctx| {
                for node in document.descendants() {
                    for name in ["root", "α", "", "missing"] {
                        assert_eq!(super::has_tag_name(ctx, node, name).expect("element name"), node.has_tag_name(name));
                    }
                }
            });
        }
        let document = roxmltree::Document::parse("<α/>").expect("XML");
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert!(matches!(super::has_tag_name(&ctx, document.root_element(), "α"),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "FreeCAD XML element name" && Some(limit) == ctx.resource_refusal()));
    }

    #[test]
    fn xml_searches_propagate_work_refusals_before_each_step() {
        let document = roxmltree::Document::parse("<!-- before --><root a='value'/>").expect("XML");
        for attribute in [false, true] {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_work_units = 0;
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("empty root");
            let error = if attribute {
                super::attribute(&ctx, document.root_element(), "a").expect_err("attribute refusal")
            } else {
                super::document_element(&ctx, &document).expect_err("root refusal")
            };
            let operation = if attribute { "FreeCAD XML attribute search" } else { "FreeCAD XML document element search" };
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == operation && Some(limit) == ctx.resource_refusal()));
        }
    }

    #[test]
    fn xml_attribute_comparison_propagates_its_work_refusal() {
        let document = roxmltree::Document::parse("<root α='unicode'/>").expect("XML");
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root");
        assert!(matches!(super::attribute(&ctx, document.root_element(), "α"),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "FreeCAD XML attribute name" && Some(limit) == ctx.resource_refusal()));
    }
}
