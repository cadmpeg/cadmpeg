// SPDX-License-Identifier: Apache-2.0

use super::feature_row_for_aggregate;

#[test]
fn retained_scan_sections_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let bytes = super::build_prt("c", &[("VisibGeom", b"payload".to_vec())]);
    let service =
        crate::decode::with_test_decode_ctx(|ctx| crate::container::scan_bytes(ctx, bytes.clone()))
            .expect("service scan admitted");
    assert_eq!(service.framing.sections.len(), 1);

    let refusal_limit = (0..4096).find(|&limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root image admitted");
        matches!(
            crate::container::scan_bytes(&ctx, bytes.clone()),
            Err(CodecError::ResourceLimit(resource))
                if resource.dimension == ResourceDimension::CollectionItems
                    && resource.operation == "creo retained scan sections"
        )
    });
    assert!(
        refusal_limit.is_some(),
        "one retained section exceeds a collection cap"
    );
}

#[test]
fn section_header_name_refuses_before_retained_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let data = b"\n#Body\nabc";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(data, &arena, &policy).expect("section input is admitted");
    let error =
        super::super::scan_sections(&ctx, data, 0).expect_err("name copy needs retained bytes");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo section header names"));
}

#[test]
fn section_header_hit_refuses_before_vec_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let data = b"\n#Body\nabc";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(data, &arena, &policy).expect("section input is admitted");
    let error =
        super::super::scan_sections(&ctx, data, 0).expect_err("hit needs one collection item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo section header hits"));
}

#[test]
fn scanned_section_refuses_before_output_vec_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let data = b"\n#Body\nabc";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(data, &arena, &policy).expect("section input is admitted");
    let error = super::super::scan_sections(&ctx, data, 0)
        .expect_err("output needs another collection item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo scanned sections"));
}

#[test]
fn scanned_section_succeeds_under_service_policy() {
    let data = b"\n#Body\nabc";
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
        .expect("section input is admitted");
    let sections = super::super::scan_sections(&ctx, data, 0).expect("section is admitted");
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].section.raw_name(), "Body");
}

fn one_toc_section(marker_name: &str, entry_name: &str) -> Vec<u8> {
    let mut data = format!("{:<80}\n", "#UGC_TOC 2 1 81 17").into_bytes();
    let section = format!("#{marker_name}\nabc");
    let offset = 2 * 81;
    data.extend_from_slice(
        format!(
            "{:<80}\n",
            format!("{entry_name} {offset:x} {:x} 0", section.len())
        )
        .as_bytes(),
    );
    assert_eq!(data.len(), offset);
    data.extend_from_slice(section.as_bytes());
    data
}

#[test]
fn toc_section_name_refuses_before_retained_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let data = one_toc_section("Body", "Body");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&data, &arena, &policy).expect("TOC input is admitted");
    let error =
        super::super::toc_sections(&ctx, &data, 0).expect_err("TOC name needs retained bytes");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo TOC section names"));
}

#[test]
fn modelview_toc_name_refuses_before_retained_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let data = one_toc_section("ModelView#1", "ModelView 1");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&data, &arena, &policy).expect("TOC input is admitted");
    let error = super::super::toc_sections(&ctx, &data, 0)
        .expect_err("ModelView name needs retained bytes");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo TOC section names"));
}

#[test]
fn toc_section_refuses_before_vec_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let data = one_toc_section("Body", "Body");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&data, &arena, &policy).expect("TOC input is admitted");
    let error =
        super::super::toc_sections(&ctx, &data, 0).expect_err("one TOC section needs one item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo TOC sections"));
}

#[test]
fn toc_section_succeeds_under_service_policy() {
    let data = one_toc_section("ModelView#1", "ModelView 1");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy)
        .expect("TOC input is admitted");
    let sections = super::super::toc_sections(&ctx, &data, 0).expect("TOC section is admitted");
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].section.raw_name(), "ModelView#1");
}

fn one_legacy_toc_section() -> Vec<u8> {
    let mut data = b"#Pro/ENGINEER  TM  Version H-01-21\n@Toc 52 0\n0 52 ->\n\
        @entry 53 10\n1 53 [1]\n"
        .to_vec();
    let section = b"#BasicData\nabc";
    let row_tail = format!(" {:08x} 0 983####\n", section.len());
    let relative_offset = data.len() + b"2 53 BasicData ".len() + 8 + row_tail.len();
    data.extend_from_slice(format!("2 53 BasicData {relative_offset:08x}{row_tail}").as_bytes());
    data.extend_from_slice(section);
    data
}

#[test]
fn legacy_toc_name_refuses_before_retained_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let data = one_legacy_toc_section();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy)
        .expect("legacy TOC input is admitted");
    let error = super::super::legacy_toc_sections(&ctx, &data, 0)
        .expect_err("legacy TOC name needs retained bytes");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo legacy TOC section names"));
}

#[test]
fn legacy_toc_section_refuses_before_vec_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let data = one_legacy_toc_section();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy)
        .expect("legacy TOC input is admitted");
    let error = super::super::legacy_toc_sections(&ctx, &data, 0)
        .expect_err("legacy TOC section needs one item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo legacy TOC sections"));
}

#[test]
fn legacy_toc_section_succeeds_under_service_policy() {
    let data = one_legacy_toc_section();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy)
        .expect("legacy TOC input is admitted");
    let sections =
        super::super::legacy_toc_sections(&ctx, &data, 0).expect("legacy TOC section is admitted");
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].section.raw_name(), "BasicData");
}

#[test]
fn legacy_schema_refuses_before_retained_copy_even_without_banner() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let data = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(data, &arena, &policy).expect("legacy header is admitted");
    let error = super::super::legacy_ascii_framing(&ctx, data)
        .expect_err("schema copy needs retained bytes");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo legacy schema"));
}

#[test]
fn legacy_release_refuses_before_retained_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let data = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n\
        #END_OF_P_OBJECT\n#Pro/ENGINEER  TM  Version H-01-21\n";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(data, &arena, &policy).expect("legacy header is admitted");
    let error = super::super::legacy_ascii_framing(&ctx, data)
        .expect_err("release copy needs retained bytes after schema");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo legacy product release"));
}

#[test]
fn legacy_schema_and_release_succeed_under_service_policy() {
    let data = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n\
        #END_OF_P_OBJECT\n#Pro/ENGINEER  TM  Version H-01-21\n";
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
        .expect("legacy header is admitted");
    let framing = super::super::legacy_ascii_framing(&ctx, data)
        .expect("legacy framing is admitted")
        .expect("complete legacy framing");
    assert_eq!(framing.schema, "6");
    assert_eq!(framing.product_release.as_deref(), Some("H-01-21"));
}

fn one_compressed_section() -> Vec<u8> {
    let mut data = b"#SolidPrimdata\n".to_vec();
    data.extend_from_slice(&[0x1f, 0x9d, 0x10, 0x41, 0x84, 0x0c, 0x01]);
    data
}

#[test]
fn expanded_section_name_refuses_before_retained_copy() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let data = one_compressed_section();
    let section = super::super::Section::scan_for_test(
        "SolidPrimdata".to_string(),
        0,
        data.len(),
        Some(3),
        &data,
    )
    .expect("bounded compressed section");
    let error = crate::test_support::last_refusal_at(
        &data,
        ResourceDimension::RetainedBytes,
        "creo expanded section names",
        |ctx| super::super::expanded_sections(ctx, &data, std::slice::from_ref(&section)),
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo expanded section names"),
        "{error:?}"
    );
}

#[test]
fn expanded_section_record_refuses_before_vec_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let data = one_compressed_section();
    let section = super::super::Section::scan_for_test(
        "SolidPrimdata".to_string(),
        0,
        data.len(),
        Some(3),
        &data,
    )
    .expect("bounded compressed section");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 3 * (1 << 16);
    let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy)
        .expect("compressed input is admitted");
    let error = super::super::expanded_sections(&ctx, &data, std::slice::from_ref(&section))
        .expect_err("expanded record needs another collection item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo expanded sections"));
}

#[test]
fn expanded_section_record_succeeds_under_service_policy() {
    let data = one_compressed_section();
    let section = super::super::Section::scan_for_test(
        "SolidPrimdata".to_string(),
        0,
        data.len(),
        Some(3),
        &data,
    )
    .expect("bounded compressed section");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy)
        .expect("compressed input is admitted");
    let expanded = super::super::expanded_sections(&ctx, &data, std::slice::from_ref(&section))
        .expect("expanded section is admitted");
    assert_eq!(expanded.len(), 1);
    assert_eq!(expanded[0].name, "SolidPrimdata");
    assert_eq!(expanded[0].data, b"ABC");
}

#[test]
fn cmnm_model_name_refuses_before_retained_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let data = b"#UGC:2 PART test \
#- CMNM 00bwidget.prt                                      \
#-END_OF_UGC_HEADER\n";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(data, &arena, &policy).expect("CMNM input is admitted");
    let error =
        super::super::cmnm_model_name(&ctx, data).expect_err("model name needs retained bytes");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo CMNM model name"));
}

#[test]
fn native_model_name_refuses_before_retained_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let data = b"#BasicData\nmodel_name\0widget\0";
    let section =
        super::super::Section::scan_for_test("BasicData".to_string(), 0, data.len(), None, data)
            .expect("bounded native name section");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(data, &arena, &policy)
        .expect("native name input is admitted");
    let error = super::super::native_model_name(&ctx, std::slice::from_ref(&section))
        .expect_err("native name needs retained bytes");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo native model name"));
}

#[test]
fn native_model_name_succeeds_under_service_policy() {
    let data = b"#BasicData\nmodel_name\0widget\0";
    let section =
        super::super::Section::scan_for_test("BasicData".to_string(), 0, data.len(), None, data)
            .expect("bounded native name section");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
        .expect("native name input is admitted");
    let name = super::super::native_model_name(&ctx, std::slice::from_ref(&section))
        .expect("native name is admitted")
        .expect("one native name");
    assert_eq!(name.0, "widget");
}

#[test]
fn legacy_persistence_scopes_refuse_before_counted_vec_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let data = b"0123456789";
    let framing = super::super::LegacyAsciiFraming {
        schema: "6".to_string(),
        product_release: None,
        banner_offset: 0,
        object_offset: 5,
        persistence: crate::legacy::Persistence::default(),
    };
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(data, &arena, &policy)
            .expect("legacy scope input is admitted");
        super::super::legacy_scope_ranges(&ctx, data, &framing, &[])
    };
    assert_eq!(run(1).expect("initial scope admitted"), vec![5..10]);
    let error = run(0).expect_err("initial scope needs one collection item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo legacy persistence scopes"));
}

#[test]
fn section_owner_range_refuses_before_counted_vec_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let row = feature_row_for_aggregate(b"xx");
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&row.body, &arena, &policy)
            .expect("feature row is admitted");
        super::super::section_owner_ranges(&ctx, &[], std::slice::from_ref(&row))
    };
    assert_eq!(run(1).expect("one owner range admitted"), vec![(100, 102)]);
    let error = run(0).expect_err("one owner range needs one item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo section owner ranges"));
}

#[test]
fn complete_legacy_directory_admits_more_than_4096_entries() {
    let count = 4097;
    let mut bytes = format!(
        "#Pro/ENGINEER  TM  Version H-01-21\n@Toc 52 0\n0 52 ->\n@entry 53 10\n1 53 [{count}]\n"
    )
    .into_bytes();
    let row_len = "2 53 BasicData 00000000 0000000e 0 983####\n".len();
    let first_offset = bytes.len() + count * row_len;
    for index in 0..count {
        let offset = first_offset + index * 14;
        bytes.extend_from_slice(
            format!("2 53 BasicData {offset:08x} 0000000e 0 983####\n").as_bytes(),
        );
    }
    for _ in 0..count {
        bytes.extend_from_slice(b"#BasicData\nabc");
    }
    crate::decode::with_test_decode_ctx(|ctx| {
        let sections =
            super::super::legacy_toc_sections(ctx, &bytes, 0).expect("complete directory admitted");
        assert_eq!(sections.len(), count);
    });
}

#[test]
fn section_scan_refuses_name_normalization_before_classifying() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo section name normalization",
        |ctx| super::super::Section::scan(ctx, "ND:0:VisibGeom:1".to_string(), 0, 0, None, &[]),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo section name normalization")
    );
}

#[test]
fn section_scan_normalizes_decorated_names_once() {
    for (raw, name, role) in [
        (
            "ND:0:VisibGeom:1",
            "VisibGeom",
            super::super::SectionRole::PsbGeometry,
        ),
        (
            "ND:0:AllFeatur",
            "AllFeatur",
            super::super::SectionRole::ModelData,
        ),
        (
            "ModelView#3",
            "ModelView",
            super::super::SectionRole::Opaque,
        ),
        ("ND:Body", "ND:Body", super::super::SectionRole::Opaque),
        (
            "BasicData",
            "BasicData",
            super::super::SectionRole::ModelData,
        ),
    ] {
        let section = super::super::Section::scan_for_test(raw.to_string(), 0, 0, None, &[])
            .expect("empty section extent")
            .section;
        assert_eq!(section.raw_name(), raw);
        assert_eq!(section.name(), name);
        assert_eq!(section.role(), role);
    }
}

#[test]
fn toc_section_deduplication_refuses_work() {
let mut data = one_toc_section("Body", "Body");
// A second directory repeats the original bounded section.
data.extend_from_slice(b"\n");
data.extend_from_slice(format!("{:<80}\n", "#UGC_TOC 2 1 81 17").as_bytes());
data.extend_from_slice(format!("{:<80}\n", "Body a2 9 0").as_bytes());
let sections = crate::decode::with_test_decode_ctx(|ctx|
super::super::toc_sections(ctx, &data, 0)).expect("duplicate directory witness");
assert_eq!(sections.len(), 1);
assert_eq!(sections[0].section.raw_name(), "Body");
assert_eq!(sections[0].section.offset(), 162);
assert_eq!(sections[0].region, b"#Body\nabc");
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo toc sections sections deduplication",
        |ctx| super::super::toc_sections(ctx, &data, 0),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo toc sections sections deduplication")
    );
}

#[test]
fn legacy_toc_section_deduplication_refuses_work() {
let mut data = b"#Pro/ENGINEER  TM  Version H-01-21\n@Toc 52 0\n0 52 ->\n\
@entry 53 10\n1 53 [2]\n".to_vec();
let row_len = b"2 53 BasicData 00000000 0000000e 0 983####\n".len();
let offset = data.len() + 2 * row_len;
let row = format!("2 53 BasicData {offset:08x} 0000000e 0 983####\n");
data.extend_from_slice(row.as_bytes());
data.extend_from_slice(row.as_bytes());
assert_eq!(data.len(), offset);
data.extend_from_slice(b"#BasicData\nabc");
let sections = crate::decode::with_test_decode_ctx(|ctx|
super::super::legacy_toc_sections(ctx, &data, 0)).expect("duplicate legacy witness");
assert_eq!(sections.len(), 1);
assert_eq!(sections[0].section.raw_name(), "BasicData");
assert_eq!(sections[0].section.offset(), offset);
assert_eq!(sections[0].region, b"#BasicData\nabc");
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo legacy toc sections sections deduplication",
        |ctx| super::super::legacy_toc_sections(ctx, &data, 0),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo legacy toc sections sections deduplication")
    );
}

mod allocation_order;
mod selection_lifetime;
