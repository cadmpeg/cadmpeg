// SPDX-License-Identifier: Apache-2.0
//! Original-source framing without a second parse or semantic read credit.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

use super::*;

fn text<'a>(views: &[View<'a>]) -> Vec<&'a [u8]> {
    views.iter().map(|view| view.window()).collect()
}

fn contains(views: &[View<'_>], range: std::ops::Range<usize>) -> bool {
    views
        .iter()
        .any(|view| view.start() < range.end && range.start < view.end())
}

#[test]
fn existing_gui_tree_exports_only_original_markup_from_a_nonzero_source_window() {
    let xml = "<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Properties Count='1'><Property name='Label' type='App::PropertyString'><String value='a > b'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    let mut bytes = b"prefix".to_vec();
    let start = bytes.len();
    bytes.extend_from_slice(xml.as_bytes());
    let end = bytes.len();
    bytes.extend_from_slice(b"suffix");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let source = root.child(start, end).unwrap();
    let tree = ctx
        .parse_xml(
            std::str::from_utf8(source.window()).unwrap(),
            "existing GUI tree",
        )
        .unwrap();
    let framing = ctx
        .record("navigation", source, || frame(&ctx, source, &tree, true))
        .unwrap();
    assert!(framing.supported());
    assert_eq!(
        text(framing.framing()),
        [
            b"<Document".as_slice(),
            b">",
            b"</Document>",
            b"<ViewProviderData",
            b">",
            b"</ViewProviderData>",
            b"<ViewProvider",
            b">",
            b"</ViewProvider>",
            b"<Properties",
            b">",
            b"</Properties>",
            b"<Property",
            b">",
            b"</Property>",
            b"<String",
            b"/>",
        ]
    );
    assert!(framing.insignificant().is_empty());
    for view in framing.framing() {
        assert_eq!(
            view.location(),
            root.child(view.start(), view.end()).unwrap().location()
        );
        assert_eq!(
            view.window().as_ptr(),
            root.child(view.start(), view.end())
                .unwrap()
                .window()
                .as_ptr()
        );
        assert!(start <= view.start() && view.end() <= end);
    }
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert!(captures[0].reads().is_empty());
}

#[test]
fn namespace_aliases_and_unicode_names_keep_their_actual_source_qnames() {
    let bytes = "<p:Document xmlns:p='urn:f' xmlns:q='urn:f'><q:状态 settings='quoted > text'/></p:Document>".as_bytes();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
    let tree = ctx
        .parse_xml(
            std::str::from_utf8(root.window()).unwrap(),
            "namespaced GUI",
        )
        .unwrap();
    let framing = frame(&ctx, root, &tree, true).unwrap();
    assert!(framing.supported());
    assert_eq!(
        text(framing.framing()),
        [
            b"<p:Document".as_slice(),
            b">",
            b"</p:Document>",
            "<q:状态".as_bytes(),
            b"/>"
        ]
    );
    assert!(framing.insignificant().is_empty());
}

#[test]
fn unknown_intervening_elements_do_not_promote_a_nested_known_provider() {
    let bytes = b"<Document><ViewProviderData><Vendor><ViewProvider name='A'/></Vendor></ViewProviderData></Document>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
    let tree = ctx
        .parse_xml(std::str::from_utf8(root.window()).unwrap(), "GUI tree")
        .unwrap();
    let framing = frame(&ctx, root, &tree, true).unwrap();
    assert!(!framing.supported());
    assert_eq!(
        text(framing.framing()),
        [
            b"<Document".as_slice(),
            b">",
            b"</Document>",
            b"<ViewProviderData",
            b">",
            b"</ViewProviderData>"
        ]
    );
}

#[test]
fn only_plain_inter_element_whitespace_is_insignificant() {
    let xml = "<Document>\n <ViewProviderData>\n <ViewProvider name='A'>\n <Properties>\n <Property name='Text' type='App::PropertyString'><String>  </String></Property>\n <Property name='CData' type='App::PropertyString'><String><![CDATA[  ]]></String></Property>\n <Property name='Entity' type='App::PropertyString'><String>&#32;</String></Property>\n <Property name='Mixed' type='App::PropertyString'><String> <Item/> </String></Property>\n </Properties>\n </ViewProvider>\n </ViewProviderData>\n</Document>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::default()).unwrap();
    let tree = ctx.parse_xml(xml, "existing GUI tree").unwrap();
    let framing = frame(&ctx, root, &tree, true).unwrap();
    assert!(framing.supported());
    assert!(!framing.insignificant().is_empty());
    for view in framing.insignificant() {
        assert!(view.window().iter().all(|byte| xml_space(*byte)));
    }
    for protected in ["  </String>", "<![CDATA[  ]]>", "&#32;", " <Item/> "] {
        let start = xml.find(protected).unwrap();
        assert!(
            !contains(framing.insignificant(), start..start + protected.len()),
            "{protected}"
        );
    }
}

#[test]
fn comments_processing_instructions_and_declarations_are_not_element_framing() {
    let xml = "<?xml version='1.0'?><!-- before --><?outside data?><Document><!-- inside --><?inside data?><Camera settings='source'/></Document>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::default()).unwrap();
    let tree = ctx.parse_xml(xml, "existing GUI tree").unwrap();
    let framing = frame(&ctx, root, &tree, true).unwrap();
    assert!(framing.supported());
    assert_eq!(
        text(framing.framing()),
        [
            b"<Document".as_slice(),
            b">",
            b"</Document>",
            b"<Camera",
            b"/>"
        ]
    );
    assert!(framing.insignificant().is_empty());
}

#[test]
fn document_vocabulary_uses_the_same_original_tree_navigation() {
    for (objects, object, data) in [
        ("Objects", "Object", "ObjectData"),
        ("Features", "Feature", "FeatureData"),
    ] {
        let xml = format!("<Document SchemaVersion='4'><{objects}><{object} name='A' type='App::Feature'/></{objects}><{data}><{object} name='A'><Properties><Property name='Label' type='App::PropertyString'><String value='A'/></Property></Properties></{object}></{data}></Document>");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::default())
                .unwrap();
        let tree = ctx.parse_xml(&xml, "existing Document tree").unwrap();
        let framing = frame(&ctx, root, &tree, false).unwrap();
        assert!(framing.supported());
        assert_eq!(framing.framing().len(), 22);
        assert!(framing.insignificant().is_empty());
        assert!(framing
            .framing()
            .iter()
            .any(|view| view.window() == b"<Property"));
    }
}

#[test]
fn foreign_detached_and_mismatched_tree_sources_cannot_claim_framing() {
    let bytes = b"<Document/>";
    let arena = DecodeArena::new();
    let other_arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
    let (other, _) =
        DecodeContext::from_root_bytes(bytes, &other_arena, &DecodePolicy::default()).unwrap();
    let tree = ctx
        .parse_xml(std::str::from_utf8(root.window()).unwrap(), "original tree")
        .unwrap();
    assert!(frame(&other, root, &tree, true).is_err());
    assert!(frame(&ctx, View::over_retained(bytes), &tree, true).is_err());
    let (mismatch, source) =
        DecodeContext::from_root_bytes(b"<Xocument/>", &arena, &DecodePolicy::default()).unwrap();
    assert!(frame(&mismatch, source, &tree, true).is_err());
}

#[test]
fn xml_framing_uses_the_same_retained_budget_and_preserves_the_session_fuse() {
    let bytes = b"<Document><Camera/></Document>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
    let tree = ctx
        .parse_xml(std::str::from_utf8(root.window()).unwrap(), "original tree")
        .unwrap();
    ctx.retain_bytes(ctx.retained_bytes_available(), "exhaust framing storage")
        .unwrap();
    assert!(matches!(
        frame(&ctx, root, &tree, true),
        Err(CodecError::ResourceLimit(_))
    ));
    assert!(ctx.finish_session().is_err());
}

#[test]
fn equal_bytes_in_a_different_dom_buffer_do_not_claim_the_original_view() {
    let bytes = b"<Document><Camera settings='same'/></Document>";
    let duplicate = bytes.to_vec();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
    let different_tree = ctx
        .parse_xml(
            std::str::from_utf8(&duplicate).unwrap(),
            "separate equal DOM",
        )
        .unwrap();
    assert_eq!(different_tree.input_text().as_bytes(), root.window());
    assert_ne!(different_tree.input_text().as_ptr(), root.window().as_ptr());
    assert!(frame(&ctx, root, &different_tree, true).is_err());
    let original_tree = ctx
        .parse_xml(
            std::str::from_utf8(root.window()).unwrap(),
            "original source DOM",
        )
        .unwrap();
    assert!(frame(&ctx, root, &original_tree, true).unwrap().supported());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn one_original_element_exposes_only_markup_in_a_nonzero_window() {
    let bytes =
        b"prefix<!--before--><Document marker='>'><Properties/></Document><!--after-->suffix";
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let source = root.child(6, bytes.len() - 6).unwrap();
    let text = std::str::from_utf8(source.window()).unwrap();
    let xml = ctx.parse_xml(text, "original framing control").unwrap();
    let (extent, fields) = element_fields(&ctx, source, xml.root_element()).unwrap();
    assert_eq!(
        extent.window(),
        b"<Document marker='>'><Properties/></Document>"
    );
    let actual = fields
        .iter()
        .map(|range| {
            &bytes[usize::try_from(range.start).unwrap()..usize::try_from(range.end).unwrap()]
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            b"<Document".as_slice(),
            b">".as_slice(),
            b"</Document>".as_slice()
        ]
    );
    let copied = text.to_owned();
    let other_dom = roxmltree::Document::parse(&copied).unwrap();
    assert!(element_fields(&ctx, source, other_dom.root_element()).is_err());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[derive(Clone, Copy)]
enum Role {
    Document,
    Objects,
    Deps,
    Dep,
    Declaration,
    Data,
    Object,
    Extensions,
    Extension,
    Properties,
    Property,
    Value,
    ValueElement,
    Hasher,
    Providers,
    Provider,
    State,
    StateElement,
}

impl Role {
    fn structural(self) -> bool {
        matches!(
            self,
            Self::Document
                | Self::Objects
                | Self::Deps
                | Self::Data
                | Self::Object
                | Self::Extensions
                | Self::Extension
                | Self::Properties
                | Self::Property
                | Self::Providers
                | Self::Provider
        )
    }
}

fn role(gui: bool, name: &str, parent: Option<Role>) -> Option<Role> {
    use Role::{
        Data, Declaration, Dep, Deps, Document, Extension, Extensions, Hasher, Object, Objects,
        Properties, Property, Provider, Providers, State, StateElement, Value, ValueElement,
    };
    if gui {
        Some(match (parent, name) {
            (None, "Document") => Document,
            (Some(Document), "ViewProviderData") => Providers,
            (Some(Document), _) => State,
            (Some(State | StateElement), _) => StateElement,
            (Some(Providers), "ViewProvider") => Provider,
            (Some(Provider), "Extensions") => Extensions,
            (Some(Extensions), "Extension") => Extension,
            (Some(Provider), "Properties") => Properties,
            (Some(Properties), "Property") => Property,
            (Some(Property), _) => Value,
            (Some(Value | ValueElement), _) => ValueElement,
            _ => return None,
        })
    } else {
        Some(match (parent, name) {
            (None, "Document") => Document,
            (Some(Document), "StringHasher" | "StringHasher2") => Hasher,
            (Some(Document), "Objects" | "Features") => Objects,
            (Some(Objects), "ObjectDeps") => Deps,
            (Some(Deps), "Dep") => Dep,
            (Some(Objects), "Object" | "Feature") => Declaration,
            (Some(Document), "ObjectData" | "FeatureData") => Data,
            (Some(Data), "Object" | "Feature") => Object,
            (Some(Object), "Extensions") => Extensions,
            (Some(Extensions), "Extension") => Extension,
            (Some(Document | Object | Extension), "Properties") => Properties,
            (Some(Properties), "Property" | "_Property") => Property,
            (Some(Property), _) => Value,
            (Some(Value | ValueElement), _) => ValueElement,
            _ => return None,
        })
    }
}

// SPDX-License-Identifier: Apache-2.0

#[test]
fn damaged_child_does_not_invalidate_closed_parent_or_following_peer() {
    let bytes = b"<Document><Properties><Property name='bad'><Broken></Property><Property name='good'/></Properties></Document>";
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = XmlSource::new(&ctx, view).unwrap();
    assert!(source.validate(0).is_ok());
    let properties = source.children(0).next().unwrap();
    let children: Vec<_> = source.children(properties).collect();
    assert_eq!(children.len(), 2);
    assert_eq!(source.name(children[1]).unwrap(), "Property");
    assert!(ctx
        .parse_xml(
            std::str::from_utf8(source.extent(children[0]).unwrap().window()).unwrap(),
            "actual damaged XML leaf"
        )
        .is_err());
    assert!(ctx
        .parse_xml(
            std::str::from_utf8(source.extent(children[1]).unwrap().window()).unwrap(),
            "actual independent XML leaf"
        )
        .is_ok());
    let broken = source.children(children[0]).next().unwrap();
    assert!(source.validate(broken).is_err());
    assert_eq!(
        source.extent(children[1]).unwrap().window(),
        b"<Property name='good'/>"
    );
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn selected_attribute_records_actual_token_without_crediting_navigation_fields() {
    let bytes = b"<Document Count='9' Name = \"A&amp;B&#x9;\"/>";
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = XmlSource::new(&ctx, view).unwrap();
    assert_eq!(source.peek_attributes(&ctx, 0).unwrap()["Count"], "9");
    ctx.record("kept", view, || {
        assert_eq!(source.attribute(&ctx, 0, "Name")?, Some("A&B\t".to_owned()));
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .unwrap();
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    let ranges: Vec<_> = captures[0].reads().values().flatten().cloned().collect();
    let start = bytes.windows(4).position(|bytes| bytes == b"Name").unwrap();
    assert_eq!(
        ranges,
        vec![
            u64::try_from(start).unwrap()..u64::try_from(start + 4).unwrap(),
            u64::try_from(start + 8).unwrap()..u64::try_from(bytes.len() - 3).unwrap(),
        ]
    );
}

#[test]
fn text_keeps_entities_cdata_and_direct_text_without_descendant_content() {
    let bytes =
        b"<Value>a&amp;b<![CDATA[<raw>]]><!--ignored--><Child>elsewhere</Child>tail&#13;</Value>";
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = XmlSource::new(&ctx, view).unwrap();
    ctx.record("value", view, || {
        assert_eq!(source.text(&ctx, 0)?, "a&b<raw>tail\r");
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .unwrap();
    let captures = ctx.take_record_captures().unwrap();
    let ranges: Vec<_> = captures[0].reads().values().flatten().cloned().collect();
    for range in ranges {
        let start = usize::try_from(range.start).unwrap();
        let end = usize::try_from(range.end).unwrap();
        assert!(!bytes[start..end]
            .windows(9)
            .any(|bytes| bytes == b"elsewhere"));
        assert!(!bytes[start..end]
            .windows(7)
            .any(|bytes| bytes == b"ignored"));
    }
}
struct Framing<'a> {
    fields: Vec<View<'a>>,
    spaces: Vec<View<'a>>,
    complete: bool,
}
impl<'a> Framing<'a> {
    fn framing(&self) -> &[View<'a>] {
        &self.fields
    }
    fn insignificant(&self) -> &[View<'a>] {
        &self.spaces
    }
    fn supported(&self) -> bool {
        self.complete
    }
}

fn frame<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    tree: &roxmltree::Document<'_>,
    gui: bool,
) -> Result<Framing<'a>, CodecError> {
    let scan = XmlSource::new(ctx, source)?;
    if tree.input_text().as_ptr() != source.window().as_ptr()
        || tree.input_text().len() != source.window().len()
    {
        return Err(CodecError::malformed("DOM does not borrow framing source"));
    }
    let mut roles = Vec::new();
    let mut fields = Vec::new();
    let mut complete = true;
    for (index, element) in scan.elements().iter().enumerate() {
        let parent = element.parent().and_then(|parent| roles[parent]);
        let role = if element.parent().is_some() && parent.is_none() {
            None
        } else {
            role(gui, scan.name(index)?.rsplit(':').next().unwrap(), parent)
        };
        complete &= role.is_some();
        roles.push(role);
        if role.is_some() {
            fields.extend(element.framing());
        }
    }
    let spaces = scan
        .insignificant(ctx)?
        .into_iter()
        .filter(|space| {
            scan.elements()
                .iter()
                .enumerate()
                .filter(|(_, element)| {
                    element.extent().start() <= space.start()
                        && element.extent().end() >= space.end()
                })
                .last()
                .is_some_and(|(index, _)| roles[index].is_some_and(Role::structural))
        })
        .collect();
    Ok(Framing {
        fields,
        spaces,
        complete,
    })
}

fn element_fields<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    node: roxmltree::Node<'_, '_>,
) -> Result<(View<'a>, Vec<std::ops::Range<u64>>), CodecError> {
    if node.document().input_text().as_ptr() != source.window().as_ptr()
        || node.document().input_text().len() != source.window().len()
    {
        return Err(CodecError::malformed("DOM does not borrow element source"));
    }
    let scan = XmlSource::new(ctx, source)?;
    let start = source.start() + node.range().start;
    let index = scan
        .elements()
        .iter()
        .position(|element| element.extent().start() == start)
        .ok_or_else(|| CodecError::malformed("element source boundary absent"))?;
    let fields = scan.elements()[index]
        .framing()
        .map(|field| u64::try_from(field.start()).unwrap()..u64::try_from(field.end()).unwrap())
        .collect();
    Ok((scan.extent(index)?, fields))
}
