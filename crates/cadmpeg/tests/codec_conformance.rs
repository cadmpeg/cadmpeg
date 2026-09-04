// SPDX-License-Identifier: Apache-2.0
//! Cross-codec conformance contracts that drive the codec crates in-process.
//!
//! Both module trees share one harness binary. Tests that only spawn the CLI
//! belong in `cli.rs`.

/// Cross-codec resource-budget classification contracts.
mod budget {
    use std::io::Cursor;

    use cadmpeg_codec_catia::CatiaCodec;
    use cadmpeg_codec_creo::CreoCodec;
    use cadmpeg_codec_freecad::FcstdCodec;
    use cadmpeg_codec_nx::NxCodec;
    use cadmpeg_codec_sldprt::SldprtCodec;
    use cadmpeg_core::decode::{
        DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceLimits,
    };
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    const CATIA: &[u8] = include_bytes!("../../cadmpeg-fuzz/seeds/catia_container/standard_nested");
    const CATIA_WORK: &[u8] = include_bytes!("fixtures/catia_tetrahedron_topology.CATPart");
    const CREO: &[u8] = include_bytes!("../../cadmpeg-fuzz/seeds/creo_container/with_surface_rows");
    const FREECAD: &[u8] =
        include_bytes!("../../cadmpeg-fuzz/seeds/fcstd_container/core_design_product.FCStd");
    const NX: &[u8] = include_bytes!("fixtures/nx_topology_part.prt");
    const SLDPRT: &[u8] = include_bytes!("fixtures/sldprt_triangle_body.sldprt");

    #[derive(Clone, Copy)]
    enum Starvation {
        Entities,
        CollectionItems,
        RecursionDepth,
        WorkUnits,
        DecompressedBytes,
        RetainedBytes,
    }

    impl Starvation {
        fn apply(self, limits: &mut ResourceLimits) {
            match self {
                Self::Entities => limits.max_entities = 0,
                Self::CollectionItems => limits.max_collection_items = 0,
                Self::RecursionDepth => limits.max_recursion_depth = 0,
                Self::WorkUnits => limits.max_work_units = 0,
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
                Self::CollectionItems => ResourceDimension::CollectionItems,
                Self::RecursionDepth => ResourceDimension::RecursionDepth,
                Self::WorkUnits => ResourceDimension::WorkUnits,
                Self::DecompressedBytes => ResourceDimension::DecompressedBytes,
                Self::RetainedBytes => ResourceDimension::RetainedBytes,
            }
        }
    }

    struct Case {
        name: &'static str,
        codec: &'static dyn Codec,
        bytes: &'static [u8],
        starvation: Starvation,
    }

    static CATIA_CODEC: CatiaCodec = CatiaCodec;
    static CREO_CODEC: CreoCodec = CreoCodec;
    static FREECAD_CODEC: FcstdCodec = FcstdCodec;
    static NX_CODEC: NxCodec = NxCodec;
    static SLDPRT_CODEC: SldprtCodec = SldprtCodec;

    fn cases() -> [Case; 10] {
        [
            Case {
                name: "catia/entities",
                codec: &CATIA_CODEC,
                bytes: CATIA,
                starvation: Starvation::Entities,
            },
            Case {
                name: "catia/work_units",
                codec: &CATIA_CODEC,
                bytes: CATIA_WORK,
                starvation: Starvation::WorkUnits,
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
                name: "freecad/collection_items",
                codec: &FREECAD_CODEC,
                bytes: FREECAD,
                starvation: Starvation::CollectionItems,
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
                            container_only: false,
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

/// Cross-codec product-structure regression tests.
mod product_roundtrip {
    #![allow(clippy::unwrap_used)]

    use std::collections::{BTreeMap, HashMap, HashSet};
    use std::io::Cursor;

    use cadmpeg_codec_freecad::FcstdCodec;
    use cadmpeg_codec_step::StepCodec;
    use cadmpeg_ir::codec::write::EncodeInput;
    use cadmpeg_ir::codec::write::Encoder;
    use cadmpeg_ir::codec::write::TargetRequest;
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
                    .get(definition.0.as_str())
                    .expect("local prototype resolves"),
                _ => panic!("round-trip fixture contains only local prototypes"),
            };
            let segment = format!("{}:{definition}", occurrence.ordinal);
            let resolved = match &occurrence.parent {
                OccurrenceParent::Root => segment,
                OccurrenceParent::Occurrence { occurrence: parent } => format!(
                    "{}/{}",
                    path(
                        occurrences
                            .get(parent.0.as_str())
                            .expect("parent occurrence resolves"),
                        occurrences,
                        definitions,
                        memo,
                    ),
                    segment
                ),
            };
            memo.insert(occurrence.id.0.clone(), resolved.clone());
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
                    graph
                        .resolved_transform(&occurrence.id)
                        .expect("resolved transform")
                        .rows,
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
                matches!(occurrence.parent, OccurrenceParent::Root)
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
