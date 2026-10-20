// SPDX-License-Identifier: Apache-2.0
//! Cross-codec conformance contracts that drive the codec crates in-process.
//!
//! Both module trees share one harness binary. Tests that only spawn the CLI
//! belong in `cli.rs`.

/// Cross-codec resource-budget classification contracts.
mod budget {
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    use std::io::Cursor;

    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    use cadmpeg_codec_catia::CatiaCodec;
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    use cadmpeg_codec_creo::CreoCodec;
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    use cadmpeg_codec_freecad::FcstdCodec;
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    use cadmpeg_codec_nx::NxCodec;
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    use cadmpeg_codec_sldprt::SldprtCodec;
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    use cadmpeg_core::decode::ResourceLimits;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    const CATIA: &[u8] = include_bytes!("../../cadmpeg-fuzz/seeds/catia_container/standard_nested");
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    const CREO: &[u8] = include_bytes!("../../cadmpeg-fuzz/seeds/creo_container/with_surface_rows");
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    const FREECAD: &[u8] =
        include_bytes!("../../cadmpeg-fuzz/seeds/fcstd_container/core_design_product.FCStd");
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    const NX: &[u8] = include_bytes!("fixtures/nx_topology_part.prt");
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    const SLDPRT: &[u8] = include_bytes!("fixtures/sldprt_triangle_body.sldprt");

    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    #[derive(Clone, Copy)]
    enum Starvation {
        Entities,
        RecursionDepth,
        DecompressedBytes,
        RetainedBytes,
    }

    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    impl Starvation {
        fn apply(self, limits: &mut ResourceLimits) {
            match self {
                Self::Entities => limits.max_entities = 0,
                Self::RecursionDepth => limits.max_recursion_depth = 0,
                Self::DecompressedBytes => {
                    limits.max_decompressed_bytes_total = 0;
                    limits.max_decompressed_bytes_per_expand = 0;
                }
                Self::RetainedBytes => limits.max_retained_bytes = 0,
            }
        }

        const fn dimension(self) -> ResourceDimension {
            match self {
                Self::Entities => ResourceDimension::Entities,
                Self::RecursionDepth => ResourceDimension::RecursionDepth,
                Self::DecompressedBytes => ResourceDimension::DecompressedBytes,
                Self::RetainedBytes => ResourceDimension::RetainedBytes,
            }
        }
    }

    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    struct Case {
        name: &'static str,
        codec: &'static dyn Codec,
        bytes: &'static [u8],
        starvation: Starvation,
    }

    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    static CATIA_CODEC: CatiaCodec = CatiaCodec;
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    static CREO_CODEC: CreoCodec = CreoCodec;
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    static FREECAD_CODEC: FcstdCodec = FcstdCodec;
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    static NX_CODEC: NxCodec = NxCodec;
    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    static SLDPRT_CODEC: SldprtCodec = SldprtCodec;

    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    fn cases() -> [Case; 8] {
        [
            Case {
                name: "catia/entities",
                codec: &CATIA_CODEC,
                bytes: CATIA,
                starvation: Starvation::Entities,
            },
            Case {
                name: "creo/entities",
                codec: &CREO_CODEC,
                bytes: CREO,
                starvation: Starvation::Entities,
            },
            Case {
                name: "freecad/entities",
                codec: &FREECAD_CODEC,
                bytes: FREECAD,
                starvation: Starvation::Entities,
            },
            Case {
                name: "freecad/recursion_depth",
                codec: &FREECAD_CODEC,
                bytes: FREECAD,
                starvation: Starvation::RecursionDepth,
            },
            Case {
                name: "freecad/retained_bytes",
                codec: &FREECAD_CODEC,
                bytes: FREECAD,
                starvation: Starvation::RetainedBytes,
            },
            Case {
                name: "nx/entities",
                codec: &NX_CODEC,
                bytes: NX,
                starvation: Starvation::Entities,
            },
            Case {
                name: "nx/decompressed_bytes",
                codec: &NX_CODEC,
                bytes: NX,
                starvation: Starvation::DecompressedBytes,
            },
            Case {
                name: "sldprt/entities",
                codec: &SLDPRT_CODEC,
                bytes: SLDPRT,
                starvation: Starvation::Entities,
            },
        ]
    }

    #[cfg(all(
        feature = "catia",
        feature = "creo",
        feature = "fcstd",
        feature = "nx",
        feature = "sldprt"
    ))]
    #[test]
    fn migrated_codec_paths_classify_starvation_identically_across_profiles() {
        for case in cases() {
            case.codec
                .decode(&mut Cursor::new(case.bytes), &DecodeOptions::default())
                .unwrap_or_else(|error| panic!("{} baseline failed: {error}", case.name));

            let mut observed = None;
            for mut policy in [DecodePolicy::desktop(), DecodePolicy::service()] {
                case.starvation.apply(&mut policy.limits);
                let error = case
                    .codec
                    .decode(
                        &mut Cursor::new(case.bytes),
                        &DecodeOptions {
                            depth: cadmpeg_core::decode::DecodeDepth::Full,
                            policy,
                        },
                    )
                    .expect_err("starved decode must fail");
                let cadmpeg_ir::DecodeFailure::Codec(CodecError::ResourceLimit(limit)) = error
                else {
                    panic!(
                        "{} returned the wrong error classification: {error}",
                        case.name
                    );
                };
                assert_eq!(
                    limit.dimension,
                    case.starvation.dimension(),
                    "{}",
                    case.name
                );
                assert_eq!(observed.get_or_insert(limit.dimension), &limit.dimension);
            }
        }
    }

    #[test]
    fn a_refusal_permanently_fuses_the_decode_session() {
        for policy in [DecodePolicy::desktop(), DecodePolicy::service()] {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(b"root", &arena, &policy)
                .expect("context construction");
            let requested = policy.limits.max_entities.saturating_add(1);
            assert!(matches!(
                ctx.charge_entities(requested, "force refusal"),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::Entities
            ));
            assert!(matches!(
                ctx.charge_entities(0, "post-refusal charge"),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::Entities
            ));
            assert!(matches!(
                ctx.finish_session(),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::Entities
            ));
        }
    }
}

/// Cross-codec local-cap contracts: a local work cap skips one item, reports a
/// loss naming its constant, and never refuses the session.
#[cfg(feature = "fcstd")]
mod local_cap {
    use std::fmt::Write as _;
    use std::io::{Cursor, Write as _};

    use cadmpeg_codec_freecad::FcstdCodec;
    use cadmpeg_core::decode::{DecodeDepth, DecodePolicy};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    fn fcstd(document: &str) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut bytes);
        zip.start_file(
            "Document.xml",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .expect("start entry");
        zip.write_all(document.as_bytes()).expect("write entry");
        zip.finish().expect("finish ZIP");
        bytes.into_inner()
    }

    /// Two sketches: 4096 coincident segments whose endpoint matching needs
    /// 2 * 4096² candidates, above the per-sketch
    /// `MAX_PROFILE_ENDPOINT_CANDIDATES`, then a closed triangle.
    fn dense_then_triangle() -> Vec<u8> {
        let dense_count = 4096;
        let dense = r#"<Geometry type="Part::GeomLineSegment"><LineSegment StartX="1" StartY="1" EndX="2" EndY="1"/></Geometry>"#
            .repeat(dense_count);
        let mut document = String::from(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Sketcher::SketchObject" name="Dense" id="1"/><Object type="Sketcher::SketchObject" name="Triangle" id="2"/></Objects>
<ObjectData Count="2">"#,
        );
        write!(
            document,
            r#"<Object name="Dense"><Properties Count="1">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="{dense_count}">{dense}</GeometryList></Property>
</Properties></Object>
<Object name="Triangle"><Properties Count="1">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="3">
<Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="1" EndY="0"/></Geometry>
<Geometry type="Part::GeomLineSegment"><LineSegment StartX="1" StartY="0" EndX="0" EndY="1"/></Geometry>
<Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="1" EndX="0" EndY="0"/></Geometry>
</GeometryList></Property>
</Properties></Object></ObjectData></Document>"#
        )
        .expect("format document");
        fcstd(&document)
    }

    #[test]
    fn an_exhausted_local_cap_skips_one_item_under_every_profile() {
        let bytes = dense_then_triangle();
        for (profile, policy) in [
            ("desktop", DecodePolicy::desktop()),
            ("service", DecodePolicy::service()),
        ] {
            let result = FcstdCodec
                .decode(
                    &mut Cursor::new(&bytes),
                    &DecodeOptions {
                        depth: DecodeDepth::Full,
                        policy,
                    },
                )
                .unwrap_or_else(|error| panic!("{profile}: a local cap refused: {error}"));
            let sketch = |name: &str| {
                result
                    .ir()
                    .model
                    .sketches
                    .iter()
                    .find(|sketch| sketch.name.as_deref() == Some(name))
                    .unwrap_or_else(|| panic!("{profile}: sketch {name} was dropped"))
            };
            assert!(sketch("Dense").profiles.is_empty(), "{profile}");
            assert_eq!(sketch("Triangle").profiles.as_slice().len(), 1, "{profile}");
            let bounded = result
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code.local_code() == "sketch.profile-matching-bounded")
                .collect::<Vec<_>>();
            assert_eq!(bounded.len(), 1, "{profile}: {bounded:#?}");
            assert!(
                bounded[0]
                    .message
                    .contains("MAX_PROFILE_ENDPOINT_CANDIDATES"),
                "{profile}: {}",
                bounded[0].message
            );
        }
    }
}

/// Cross-codec product-structure regression tests.
#[cfg(all(feature = "fcstd", feature = "step"))]
mod product_roundtrip {
    #![allow(clippy::unwrap_used)]

    use cadmpeg_test_support::assembly;
    use std::collections::{BTreeMap, HashMap, HashSet};
    use std::io::Cursor;

    use cadmpeg_codec_freecad::FcstdCodec;
    use cadmpeg_codec_step::StepCodec;
    use cadmpeg_ir::codec::write::target::TargetRequest;
    use cadmpeg_ir::codec::write::EncodeInput;
    use cadmpeg_ir::codec::write::Encoder;
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::products::{AssemblyGraph, Occurrence, OccurrenceParent, PrototypeReference};
    use cadmpeg_ir::CadIr;

    const CORE_DESIGN_PRODUCT: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/core_design_product.FCStd"
    ));

    fn definition_names(ir: &CadIr) -> HashMap<&str, String> {
        ir.model
            .product_definitions
            .iter()
            .map(|definition| {
                (
                    definition.id.as_str(),
                    definition
                        .part_number
                        .as_ref()
                        .or(definition.source_name.as_ref())
                        .or(definition.label.as_ref())
                        .expect("definition has a stable product name")
                        .clone(),
                )
            })
            .collect()
    }

    fn occurrence_paths(ir: &CadIr) -> BTreeMap<String, [[f64; 4]; 4]> {
        fn path(
            occurrence: &Occurrence,
            occurrences: &HashMap<&str, &Occurrence>,
            definitions: &HashMap<&str, String>,
            memo: &mut HashMap<String, String>,
        ) -> String {
            if let Some(path) = memo.get(occurrence.id.as_str()) {
                return path.clone();
            }
            let definition = match &occurrence.prototype {
                PrototypeReference::Local { definition } => definitions
                    .get(definition.as_str())
                    .expect("local prototype resolves"),
                _ => panic!("round-trip fixture contains only local prototypes"),
            };
            let segment = format!("{}:{definition}", occurrence.ordinal);
            let resolved = match &occurrence.parent {
                OccurrenceParent::Root {} => segment,
                OccurrenceParent::Occurrence { occurrence: parent } => format!(
                    "{}/{}",
                    path(
                        occurrences
                            .get(parent.as_str())
                            .expect("parent occurrence resolves"),
                        occurrences,
                        definitions,
                        memo,
                    ),
                    segment
                ),
            };
            memo.insert(occurrence.id.as_str().to_owned(), resolved.clone());
            resolved
        }

        let definitions = definition_names(ir);
        let occurrences = ir
            .model
            .occurrences
            .iter()
            .map(|occurrence| (occurrence.id.as_str(), occurrence))
            .collect::<HashMap<_, _>>();
        let graph = AssemblyGraph::new(&ir.model.occurrences).expect("valid assembly graph");
        let mut memo = HashMap::new();
        ir.model
            .occurrences
            .iter()
            .map(|occurrence| {
                (
                    path(occurrence, &occurrences, &definitions, &mut memo),
                    assembly::resolved_transform(&graph, &occurrence.id)
                        .expect("resolved transform")
                        .rows(),
                )
            })
            .collect()
    }

    #[test]
    fn fcstd_assembly_round_trips_through_step_without_losing_its_tree() {
        let mut source = FcstdCodec
            .decode(
                &mut Cursor::new(CORE_DESIGN_PRODUCT),
                &DecodeOptions::default(),
            )
            .expect("decode FCStd assembly")
            .into_parts()
            .0;

        let assembly_root = source
            .model
            .occurrences
            .iter()
            .find(|occurrence| {
                matches!(occurrence.parent, OccurrenceParent::Root {})
                    && matches!(
                        &occurrence.prototype,
                        PrototypeReference::Local { definition }
                            if source.model.product_definitions.iter().any(|candidate| {
                                candidate.id == *definition
                                    && candidate.source_name.as_deref() == Some("Product")
                            })
                    )
            })
            .expect("Product assembly root")
            .id
            .clone();
        let mut retained = HashSet::from([assembly_root.clone()]);
        loop {
            let before = retained.len();
            for occurrence in &source.model.occurrences {
                if matches!(
                    &occurrence.parent,
                    OccurrenceParent::Occurrence { occurrence: parent }
                        if retained.contains(parent)
                ) {
                    retained.insert(occurrence.id.clone());
                }
            }
            if retained.len() == before {
                break;
            }
        }
        source
            .model
            .occurrences
            .retain(|occurrence| retained.contains(&occurrence.id));
        source
            .model
            .occurrences
            .iter_mut()
            .find(|occurrence| occurrence.id == assembly_root)
            .expect("retained root")
            .ordinal = 0;

        assert_eq!(
            source
                .model
                .product_definitions
                .iter()
                .map(|definition| definition.bodies.len())
                .sum::<usize>(),
            source.model.bodies.len(),
            "each FCStd body belongs to its source product definition"
        );
        let expected_definitions = definition_names(&source)
            .into_values()
            .collect::<HashSet<_>>();
        let expected_occurrences = occurrence_paths(&source);
        assert_eq!(expected_definitions.len(), 6);
        assert_eq!(expected_occurrences.len(), 6);

        let mut step = Vec::new();
        StepCodec::default()
            .plan(EncodeInput::new(&source, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut step))
            .expect("write STEP assembly");
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(step), &DecodeOptions::default())
            .expect("decode STEP assembly")
            .into_parts()
            .0;

        assert_eq!(
            definition_names(&decoded)
                .into_values()
                .collect::<HashSet<_>>(),
            expected_definitions
        );
        let actual_occurrences = occurrence_paths(&decoded);
        assert_eq!(
            actual_occurrences.keys().collect::<Vec<_>>(),
            expected_occurrences.keys().collect::<Vec<_>>()
        );
        for (path, expected) in expected_occurrences {
            let actual = actual_occurrences.get(&path).expect("same occurrence path");
            for row in 0..4 {
                for column in 0..4 {
                    assert!(
                        (actual[row][column] - expected[row][column]).abs() <= 1.0e-9,
                        "resolved transform differs at {path}[{row}][{column}]: expected {}, got {}",
                        expected[row][column],
                        actual[row][column]
                    );
                }
            }
        }
    }
}
