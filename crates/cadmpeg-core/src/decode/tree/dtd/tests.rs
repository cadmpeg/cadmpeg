// SPDX-License-Identifier: Apache-2.0

use super::{locate, reference, walk, Declared};
use crate::decode::tree::{XmlNote, XmlUnread};
use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, LimitScope, ResourceDimension};
use crate::scan::{ManualClock, ScanBudget, ScanLimits, StopReason};
use crate::CodecError;

const BILLION_LAUGHS: &str = "<?xml version='1.0'?><!DOCTYPE lolz [\
    <!ENTITY lol 'lol'>\
    <!ENTITY lol1 '&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;'>\
    <!ENTITY lol2 '&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;'>\
    <!ENTITY lol3 '&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;'>\
    <!ENTITY lol4 '&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;'>\
    <!ENTITY lol5 '&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;'>\
    <!ENTITY lol6 '&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;'>\
    <!ENTITY lol7 '&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;'>\
    <!ENTITY lol8 '&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;'>\
    <!ENTITY lol9 '&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;'>\
    ]><lolz>&lol9;</lolz>";

fn with_session<T>(run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::default()).expect("empty root");
    run(&ctx)
}

fn names(text: &str) -> Vec<(String, Option<String>)> {
    let mut declared = Vec::new();
    let start = locate(text.as_bytes()).expect("a DOCTYPE");
    walk(text.as_bytes(), start, |Declared { name, value }| {
        declared.push((
            String::from_utf8_lossy(name).into_owned(),
            value.map(|range| text[range].to_owned()),
        ));
    })
    .expect("a DOCTYPE that closes");
    declared
}

#[test]
fn locate_finds_the_doctype_roxmltree_reads() {
    for (text, at) in [
        ("<!DOCTYPE r><r/>", Some(0)),
        ("\u{feff}<!DOCTYPE r><r/>", Some(3)),
        (
            "<?xml version='1.0' encoding=\"a?>b\"?>\n<!-- c --><?p q?> <!DOCTYPE r><r/>",
            Some(56),
        ),
        ("<r><!DOCTYPE r></r>", None),
        ("<r/>", None),
        ("<!-- open", None),
    ] {
        assert_eq!(locate(text.as_bytes()), at, "{text}");
    }
}

#[test]
fn walk_reads_the_internal_subset_as_roxmltree_does() {
    let text = "<!DOCTYPE r PUBLIC '-//p//EN' \"r.dtd\" [\n  <!ENTITY a \"x>]y\">\
                <!-- <!ENTITY b 'no'> --><?p <!ENTITY c 'no'>?>\
                <!ENTITY % d 'p'><!ENTITY e SYSTEM 'e.xml'><!ENTITY f PUBLIC 'f' 'f.xml' NDATA g>\
                <!ELEMENT r ANY><!ATTLIST r h CDATA #IMPLIED><!NOTATION g SYSTEM 'g'>\
                <!ENTITY a 'second'> ] ><r/>";
    assert_eq!(
        names(text),
        [
            ("a".to_owned(), Some("x>]y".to_owned())),
            ("d".to_owned(), Some("p".to_owned())),
            ("e".to_owned(), None),
            ("f".to_owned(), None),
            ("a".to_owned(), Some("second".to_owned())),
        ]
    );
    let (end, unread) = walk(text.as_bytes(), 0, |_| {}).expect("a DOCTYPE that closes");
    assert_eq!(&text[end..], "<r/>");
    assert!(unread.external_subset);
    assert!(!unread.attribute_defaults);
    let (_, unread) =
        walk(b"<!DOCTYPE r [<!ATTLIST r a CDATA 'd'>]>", 0, |_| {}).expect("a DOCTYPE that closes");
    assert!(unread.attribute_defaults && !unread.external_subset);
}

#[test]
fn walk_refuses_a_doctype_that_does_not_close() {
    for text in [
        "<!DOCTYPE r [<!ENTITY a 'x'>",
        "<!DOCTYPE r [<!ENTITY a 'x>]><r/>",
        "<!DOCTYPE r [<!ENTITY a x>]><r/>",
        "<!DOCTYPE r [<!FOO>]><r/>",
        "<!DOCTYPE r SYSTEM><r/>",
    ] {
        assert!(walk(text.as_bytes(), 0, |_| {}).is_err(), "{text}");
        with_session(|ctx| {
            assert!(
                matches!(ctx.parse_xml(text, "XML"), Err(CodecError::Malformed(_))),
                "{text}"
            );
        });
    }
}

#[test]
fn reference_names_general_entities_only() {
    for (tail, name) in [
        (&b"&a;"[..], Some(&b"a"[..])),
        (b"&a.b-c;rest", Some(b"a.b-c")),
        (b"&amp;", None),
        (b"&lt;", None),
        (b"&#60;", None),
        (b"&#x3c;", None),
        (b"&a b;", None),
        (b"&a", None),
        (b"&;", None),
        (b"a;", None),
    ] {
        assert_eq!(reference(tail), name, "{}", String::from_utf8_lossy(tail));
    }
}

#[test]
fn a_doctype_without_a_subset_decodes_without_damage() {
    with_session(|ctx| {
        let document = ctx
            .parse_xml(
                "<?xml version='1.0'?>\n<!DOCTYPE boost_serialization>\n<boost_serialization \
                 version='19'><count>3</count></boost_serialization>",
                "XML",
            )
            .expect("DOCTYPE without a subset");
        assert_eq!(
            document.root_element().tag_name().name(),
            "boost_serialization"
        );
        assert!(ctx.take_xml_notes().is_empty());
        let document = ctx
            .parse_xml(
                "<!DOCTYPE svg PUBLIC '-//W3C//DTD SVG 1.1//EN' \
                 'http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd'>\
                 <svg xmlns='http://www.w3.org/2000/svg'><g/></svg>",
                "XML",
            )
            .expect("DOCTYPE with an external subset");
        assert_eq!(document.root_element().children().count(), 1);
        assert_eq!(
            ctx.take_xml_notes(),
            [XmlNote {
                operation: "XML",
                unread: XmlUnread::ExternalSubset,
                documents: 1,
                references: 0,
            }]
        );
    });
}

#[test]
fn internal_entities_expand_in_text_and_values() {
    with_session(|ctx| {
        let document = ctx
            .parse_xml(
                "<!DOCTYPE r [<!ENTITY who 'World'><!ENTITY greet 'Hello, &who;!'>\
                 <!ENTITY el '<b q=\"&who;\">bold</b>'><!ENTITY lt2 'a<b'><!ENTITY who 'no'>]>\
                 <r a='&greet;' c='&lt2;'>&greet; &el;</r>",
                "XML",
            )
            .expect("internal entities");
        let root = document.root_element();
        assert_eq!(root.attribute("a"), Some("Hello, World!"));
        assert_eq!(root.attribute("c"), Some("a<b"));
        assert_eq!(root.text(), Some("Hello, World! "));
        let bold = root
            .first_element_child()
            .expect("an element from an entity");
        assert_eq!(
            (bold.tag_name().name(), bold.attribute("q"), bold.text()),
            ("b", Some("World"), Some("bold"))
        );
        assert!(ctx.take_xml_notes().is_empty());
    });
}

#[test]
fn literal_markup_in_an_entity_does_not_create_a_cycle() {
    with_session(|ctx| {
        let document = ctx
            .parse_xml(
                "<!DOCTYPE r [<!ENTITY a '<!-- &a; -->ok'>]><r>&a;</r>",
                "XML",
            )
            .expect("the comment contains a literal ampersand");
        let root = document.root_element();
        let comment = root.first_child().expect("expanded comment");
        assert!(comment.is_comment());
        assert_eq!(comment.text(), Some(" &a; "));
        assert_eq!(
            comment.next_sibling().and_then(|node| node.text()),
            Some("ok")
        );
        assert!(ctx.take_xml_notes().is_empty());
    });
}

#[test]
fn external_references_follow_the_context_of_each_expansion() {
    with_session(|ctx| {
        let document = ctx
            .parse_xml(
                "<!DOCTYPE r [<!ENTITY ext SYSTEM 'https://example.invalid/entity'>\
                 <!ENTITY fragment '<!-- &ext; --><![CDATA[&ext;]]><?tag &ext;?>'>\
                 <!ENTITY leaf '&ext;'><!ENTITY element '<a v=\"&leaf;\"/>'>]>\
                 <r attr='&fragment;'>&fragment;&element;</r>",
                "XML",
            )
            .expect("both uses of the replacement");
        let root = document.root_element();
        assert_eq!(
            root.attribute("attr"),
            Some("<!--  --><![CDATA[]]><?tag ?>")
        );
        let comment = root.first_child().expect("expanded comment");
        assert_eq!(comment.text(), Some(" &ext; "));
        let cdata = comment.next_sibling().expect("expanded CDATA");
        assert_eq!(cdata.text(), Some("&ext;"));
        assert_eq!(
            cdata
                .next_sibling()
                .and_then(|node| node.pi())
                .and_then(|pi| pi.value),
            Some("&ext;")
        );
        assert_eq!(
            root.first_element_child()
                .and_then(|node| node.attribute("v")),
            Some("")
        );
        assert_eq!(
            ctx.take_xml_notes(),
            [XmlNote {
                operation: "XML",
                unread: XmlUnread::ExternalEntity,
                documents: 1,
                references: 4,
            }]
        );
    });
}

#[test]
fn billion_laughs_is_refused_before_it_expands() {
    with_session(|ctx| {
        let (result, peak) =
            crate::decode::tree::tests::heap::peak(|| ctx.parse_xml(BILLION_LAUGHS, "XML"));
        match result {
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, ResourceDimension::DecompressedBytes);
                assert_eq!(limit.scope, LimitScope::Session);
            }
            other => panic!("expected a resource refusal: {other:?}"),
        }
        assert!(peak < 1 << 20, "peak {peak}");
    });
    let clock = ManualClock::default();
    let budget = ScanBudget::new(ScanLimits::default(), &clock);
    assert_eq!(
        budget.parse_xml(BILLION_LAUGHS).expect_err("refused"),
        StopReason::RetainedBytes
    );
}

#[test]
fn expansions_past_roxmltree_limits_are_not_implemented() {
    let sixteen = |name: &str| format!("&{name};").repeat(16);
    let text = format!(
        "<!DOCTYPE r [<!ENTITY a 'x'><!ENTITY b '{}'><!ENTITY c '{}'>]><r>&c;</r>",
        sixteen("a"),
        sixteen("b")
    );
    with_session(|ctx| {
        assert!(matches!(
            ctx.parse_xml(&text, "XML"),
            Err(CodecError::NotImplemented(_))
        ));
    });
}

#[test]
fn self_reference_and_content_that_does_not_nest_are_malformed() {
    for text in [
        "<!DOCTYPE r [<!ENTITY a '&b;'><!ENTITY b 'x&a;'>]><r>&a;</r>",
        "<!DOCTYPE r [<!ENTITY a '&a;'>]><r a='&a;'/>",
        "<!DOCTYPE r [<!ENTITY a '<b>'>]><r>&a;</r>",
        "<!DOCTYPE r [<!ENTITY a '</r>'>]><r>&a;</r>",
    ] {
        with_session(|ctx| {
            assert!(
                matches!(ctx.parse_xml(text, "XML"), Err(CodecError::Malformed(_))),
                "{text}"
            );
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
    with_session(|ctx| {
        let document = ctx
            .parse_xml(
                "<!DOCTYPE r [<!ENTITY a '&a;'><!ENTITY b '<b>'>]><r>&amp;<!-- &a; --></r>",
                "XML",
            )
            .expect("entities that no reference expands");
        assert_eq!(document.root_element().text(), Some("&"));
    });
}

#[test]
fn external_entities_expand_to_empty_text_and_are_noted() {
    with_session(|ctx| {
        let document = ctx
            .parse_xml(
                "<!DOCTYPE r [<!ENTITY ext SYSTEM 'file:///etc/passwd'>\
                 <!ENTITY pub PUBLIC 'p' 'https://example.invalid/p.xml'>\
                 <!ENTITY both 'b&ext;'>]><r v='&pub;v'>a&ext;&both;&pub;c</r>",
                "read XML",
            )
            .expect("external entities");
        let root = document.root_element();
        assert_eq!(root.text(), Some("abc"));
        assert_eq!(root.attribute("v"), Some("v"));
        assert_eq!(
            ctx.take_xml_notes(),
            [XmlNote {
                operation: "read XML",
                unread: XmlUnread::ExternalEntity,
                documents: 1,
                references: 4,
            }]
        );
    });
}

#[test]
fn notes_reach_the_session_through_task_traces_and_arenas() {
    let text = "<!DOCTYPE r [<!ENTITY x SYSTEM 'x'>]><r>&x;</r>";
    let note = XmlNote {
        operation: "XML",
        unread: XmlUnread::ExternalEntity,
        documents: 1,
        references: 1,
    };
    with_session(|ctx| {
        let arena = DecodeArena::new();
        let task = ctx.checkpoint().resume(&arena);
        task.parse_xml(text, "XML").expect("task parse");
        let trace = task.take_trace();
        assert!(ctx.take_xml_notes().is_empty());
        assert!(ctx.accept_trace(&trace));
        let local = DecodeArena::new();
        ctx.with_arena(&local, |inner| inner.parse_xml(text, "XML").map(|_| ()))
            .expect("arena parse");
        assert_eq!(
            ctx.take_xml_notes(),
            [XmlNote {
                documents: 2,
                references: 2,
                ..note
            }]
        );
    });
}

#[test]
fn an_entity_from_an_unread_external_subset_keeps_independent_markup() {
    for (text, original) in [
        ("<!DOCTYPE r SYSTEM 'labels.dtd'><r><label>Plate</label>&units;</r>", "<label>Plate</label>"),
        ("<!DOCTYPE r SYSTEM 'labels.dtd' [<!ENTITY label 'Plate'>]><r><label>&label;</label>&units;</r>", "<label>&label;</label>"),
    ] {
        with_session(|ctx| {
            let document = ctx.parse_xml(text, "XML").expect("readable local markup");
            let node = document.root_element().first_element_child().expect("label");
            assert_eq!(node.text(), Some("Plate"));
            assert_eq!(&document.source()[document.source_range(node.range())], original);
            assert_eq!(ctx.take_xml_notes(), [
                XmlNote { operation: "XML", unread: XmlUnread::ExternalSubset, documents: 1, references: 0 },
                XmlNote { operation: "XML", unread: XmlUnread::ExternalEntity, documents: 1, references: 1 },
            ]);
        });
    }
}

#[test]
fn missing_entities_still_refuse_when_the_declaration_is_required() {
    with_session(|ctx| {
        for text in [
            "<?xml version='1.0' standalone='yes'?><!DOCTYPE r SYSTEM 'labels.dtd'><r>&units;</r>",
            "<!DOCTYPE r><r>&units;</r>",
        ] {
            assert!(matches!(
                ctx.parse_xml(text, "XML"),
                Err(CodecError::Malformed(_))
            ));
        }
        assert!(ctx.take_xml_notes().is_empty());
    });
}
