// SPDX-License-Identifier: Apache-2.0
//! "What is this file?", answered at inspection depth.

use std::io::SeekFrom;

use cadmpeg_container::compound::read_detection_prefix;
use cadmpeg_core::decode::InspectOptions;
use cadmpeg_core::{CodecError, ReadSeek};
use cadmpeg_ir::codec::{Confidence, FormatId};
use cadmpeg_ir::ContainerSummary;

use crate::{
    DetectionOutcome, ForcedInput, InputCatalog, ResolveSourceError, ResolvedSource, Selection,
};

/// Leading byte window offered to prefix detection.
///
/// The same window the CLI reads before it loads a file: detection that saw
/// less than the loader does could name a different codec than the loader
/// then picks.
pub const DETECTION_PREFIX_LEN: usize = 128 * 1024;

/// Identification of one file before semantic decode.
#[derive(Debug)]
pub enum Identification {
    /// No codec recognized the input.
    None,
    /// Neutral CADIR JSON without a native container.
    Cadir,
    /// One native codec and its inspection result.
    Native {
        /// Stable format id of the selected codec.
        format: FormatId,
        /// Confidence of prefix detection.
        confidence: Confidence,
        /// Container summary or typed inspection failure.
        inspection: Result<Box<ContainerSummary>, CodecError>,
    },
    /// Native codecs tied at the strongest confidence.
    Ambiguous {
        /// Shared strongest confidence.
        confidence: Confidence,
        /// Candidate format ids in detection order.
        candidates: Vec<FormatId>,
    },
}

/// A successfully inspected source: the selected codec's format, how it was
/// selected, and its complete summary.
#[derive(Debug)]
pub struct Inspected {
    /// Stable format id of the selected codec.
    pub format: FormatId,
    /// How the codec was chosen.
    pub selection: Selection,
    /// The codec's complete container summary.
    pub summary: ContainerSummary,
}

/// Why [`resolve_and_inspect_with`] produced no summary.
#[derive(Debug, thiserror::Error)]
pub enum InspectError {
    /// Reading or repositioning the source failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The source could not be resolved to one codec.
    #[error(transparent)]
    Unresolved(#[from] ResolveSourceError),
    /// The source begins as CADIR JSON, which has no native container.
    #[error("CADIR JSON has no native container to inspect")]
    Cadir,
    /// No registered codec recognized the source.
    #[error("no registered codec recognized the input")]
    Unrecognized,
    /// The selected codec recognized the prefix but its inspection failed.
    #[error("{format} inspection failed: {error}")]
    Codec {
        /// Stable format id of the selected codec.
        format: FormatId,
        /// How the codec was chosen.
        selection: Selection,
        /// The codec's typed failure.
        error: CodecError,
    },
}

/// Identifies a source and inspects a sole native winner under the supplied limits.
pub fn identify(
    source: &mut dyn ReadSeek,
    options: &InspectOptions,
) -> std::io::Result<Identification> {
    let catalog = InputCatalog::with_builtins();
    identify_with(&catalog, source, options)
}

/// [`identify`], against a caller-held catalog.
///
/// Building the catalog constructs every codec this build carries. A caller
/// identifying many files holds one catalog and reuses it.
pub fn identify_with(
    catalog: &InputCatalog,
    source: &mut dyn ReadSeek,
    options: &InspectOptions,
) -> std::io::Result<Identification> {
    let prefix = read_prefix(source, options)?;
    match catalog.detect(&prefix) {
        DetectionOutcome::None if crate::catalog::is_cadir_prefix(&prefix) => {
            Ok(Identification::Cadir)
        }
        DetectionOutcome::None => Ok(Identification::None),
        DetectionOutcome::Detected { codec, confidence } => {
            let inspection = inspect_codec(codec, source, options)?.map(Box::new);
            Ok(Identification::Native {
                format: codec.id(),
                confidence,
                inspection,
            })
        }
        DetectionOutcome::Ambiguous {
            confidence,
            candidates,
        } => Ok(Identification::Ambiguous {
            confidence,
            candidates,
        }),
    }
}

/// Resolves and inspects exactly one source against a caller-held catalog.
///
/// Unlike [`identify_with`], equal-confidence ambiguity is a resolution error,
/// and an explicit [`ForcedInput`] is accepted. Every way of not producing a
/// summary is one [`InspectError`] variant; a selected codec's failure keeps
/// the format identity and selection it established.
pub fn resolve_and_inspect_with(
    catalog: &InputCatalog,
    source: &mut dyn ReadSeek,
    forced: Option<ForcedInput>,
    options: &InspectOptions,
) -> Result<Inspected, InspectError> {
    let prefix = read_prefix(source, options)?;
    match catalog.resolve_source(&prefix, forced)? {
        ResolvedSource::Native { codec, selection } => {
            match inspect_codec(codec, source, options)? {
                Ok(summary) => Ok(Inspected {
                    format: codec.id(),
                    selection,
                    summary,
                }),
                Err(error) => Err(InspectError::Codec {
                    format: codec.id(),
                    selection,
                    error,
                }),
            }
        }
        ResolvedSource::Cadir => Err(InspectError::Cadir),
        ResolvedSource::Unrecognized => Err(InspectError::Unrecognized),
    }
}

fn inspect_codec(
    codec: &dyn cadmpeg_ir::codec::Codec,
    source: &mut dyn ReadSeek,
    options: &InspectOptions,
) -> std::io::Result<Result<ContainerSummary, CodecError>> {
    source.seek(SeekFrom::Start(0))?;
    Ok(codec.inspect(source, options))
}

/// Reads the detection window and leaves `source` at the start.
///
/// Bounded by the inspection's own input limit as well as the window: a
/// caller that capped the input has capped what detection may look at too.
fn read_prefix(source: &mut dyn ReadSeek, options: &InspectOptions) -> std::io::Result<Vec<u8>> {
    source.seek(SeekFrom::Start(0))?;
    let prefix =
        read_detection_prefix(source, DETECTION_PREFIX_LEN, options.limits.max_input_bytes)?;
    source.seek(SeekFrom::Start(0))?;
    Ok(prefix)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use cadmpeg_core::decode::ResourceLimits;

    fn inspection() -> InspectOptions {
        InspectOptions {
            limits: ResourceLimits::desktop(),
        }
    }

    fn run(bytes: &[u8], options: &InspectOptions) -> Identification {
        let mut reader = Cursor::new(bytes.to_vec());
        identify(&mut reader, options).expect("a Cursor neither fails to read nor to seek")
    }

    #[cfg(feature = "nx")]
    #[test]
    fn compound_detection_under_a_small_cap_keeps_prefix_candidates() {
        let mut bytes = nx_compound_prefix();
        let cap = bytes.len();
        bytes.resize(cap + 1024, 0x5a);
        let options = InspectOptions {
            limits: ResourceLimits {
                max_input_bytes: cap as u64,
                ..ResourceLimits::desktop()
            },
        };
        let mut reader = Cursor::new(bytes);

        let found = identify(&mut reader, &options).expect("the cap bounds CFB detection");

        assert!(match found {
            Identification::Native { format, .. } => format == FormatId::new("nx"),
            Identification::Ambiguous { candidates, .. } =>
                candidates.contains(&FormatId::new("nx")),
            Identification::None | Identification::Cadir => false,
        });
    }

    #[cfg(feature = "nx")]
    fn nx_compound_prefix() -> Vec<u8> {
        const SECTOR: usize = 512;
        const END: u32 = 0xffff_fffe;
        const FREE: u32 = 0xffff_ffff;
        const FAT: u32 = 0xffff_fffd;
        let mut file = vec![0_u8; SECTOR * 3];
        file[..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
        put_u16(&mut file, 24, 0x003e);
        put_u16(&mut file, 26, 3);
        put_u16(&mut file, 28, 0xfffe);
        put_u16(&mut file, 30, 9);
        put_u16(&mut file, 32, 6);
        put_u32(&mut file, 44, 1);
        put_u32(&mut file, 48, 0);
        put_u32(&mut file, 56, 4096);
        put_u32(&mut file, 60, END);
        put_u32(&mut file, 68, END);
        for index in 0..109 {
            put_u32(&mut file, 76 + index * 4, FREE);
        }
        put_u32(&mut file, 76, 1);

        directory_entry(&mut file[SECTOR..SECTOR * 2], 0, "Root Entry", 5, FREE, 1);
        directory_entry(&mut file[SECTOR..SECTOR * 2], 1, "UG_PART", 1, FREE, 2);
        directory_entry(&mut file[SECTOR..SECTOR * 2], 2, "UG_PART", 2, END, FREE);
        file[SECTOR * 2..].fill(0xff);
        put_u32(&mut file, SECTOR * 2, END);
        put_u32(&mut file, SECTOR * 2 + 4, FAT);
        file
    }

    #[cfg(feature = "nx")]
    fn directory_entry(
        directory: &mut [u8],
        index: usize,
        name: &str,
        kind: u8,
        start: u32,
        child: u32,
    ) {
        const FREE: u32 = 0xffff_ffff;
        let entry = &mut directory[index * 128..(index + 1) * 128];
        let mut encoded = name.encode_utf16().collect::<Vec<_>>();
        encoded.push(0);
        for (offset, word) in encoded.iter().enumerate() {
            put_u16(entry, offset * 2, *word);
        }
        put_u16(entry, 64, (encoded.len() * 2) as u16);
        entry[66] = kind;
        entry[67] = 1;
        put_u32(entry, 68, FREE);
        put_u32(entry, 72, FREE);
        put_u32(entry, 76, child);
        put_u32(entry, 116, start);
    }

    #[cfg(feature = "nx")]
    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    #[cfg(feature = "nx")]
    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    /// The per-format fixture table and the case it drives.
    ///
    /// Gated once, on the features that contribute a fixture, so a build
    /// carrying none of them compiles without an empty table to walk.
    #[cfg(any(
        feature = "rhino",
        feature = "iges",
        feature = "step",
        feature = "f3d",
        feature = "fcstd",
        feature = "sldprt",
        feature = "catia",
        feature = "creo",
    ))]
    mod declared {
        use cadmpeg_core::dialect::DialectId;

        use super::*;

        /// One fixture and the dialect its codec classifies.
        struct Case {
            bytes: &'static [u8],
            format: &'static str,
            dialect: &'static str,
        }

        fn cases() -> Vec<Case> {
            vec![
                #[cfg(feature = "rhino")]
                Case {
                    bytes: include_bytes!(
                        "../../cadmpeg-codec-rhino/tests/golden/fixtures/arc.3dm"
                    ),
                    format: "rhino",
                    dialect: "rhino:archive-50",
                },
                #[cfg(feature = "iges")]
                Case {
                    bytes: include_bytes!(
                        "../../cadmpeg-codec-iges/tests/golden/fixtures/analytic_cone_surface.igs"
                    ),
                    format: "iges",
                    dialect: "iges:5.3-fixed-ascii",
                },
                #[cfg(feature = "step")]
                Case {
                    bytes: include_bytes!(
                        "../../cadmpeg-codec-step/tests/fixtures/ap203_sheet.p21"
                    ),
                    format: "step",
                    dialect: "step:ap203-e1",
                },
                #[cfg(feature = "f3d")]
                Case {
                    bytes: include_bytes!(
                        "../../cadmpeg-codec-f3d/tests/golden/fixtures/attributes.f3d"
                    ),
                    format: "f3d",
                    dialect: "f3d:manifest-3-2-0-0",
                },
                #[cfg(feature = "fcstd")]
                Case {
                    bytes: include_bytes!(
                        "../../../corpus/freecad_fcstd/fixtures/application_payloads.FCStd"
                    ),
                    format: "fcstd",
                    dialect: "fcstd:schema-4",
                },
                #[cfg(feature = "sldprt")]
                Case {
                    bytes: include_bytes!(
                        "../../cadmpeg-codec-sldprt/tests/golden/fixtures/analytic_cylinder.sldprt"
                    ),
                    format: "sldprt",
                    dialect: "sldprt:unknown",
                },
                #[cfg(feature = "catia")]
                Case {
                    bytes: include_bytes!(
                    "../../cadmpeg-codec-catia/tests/golden/fixtures/fbb_only_fallthrough.catpart"
                    ),
                    format: "catia",
                    dialect: "catia:fbb-only",
                },
                #[cfg(feature = "creo")]
                Case {
                    bytes: include_bytes!(
                        "../../cadmpeg-codec-creo/tests/golden/fixtures/depdb_recipe_history.prt"
                    ),
                    format: "creo",
                    dialect: "creo:depdb",
                },
            ]
        }

        /// One fixture per format: identification names the resolver's winner
        /// and settles its dialect.
        #[test]
        fn a_fixture_of_each_format_identifies_to_its_codec_dialect() {
            let cases = cases();
            assert!(
                !cases.is_empty(),
                "the feature gate on this test must match the table it walks"
            );
            for case in cases {
                let found = run(case.bytes, &inspection());
                let Identification::Native {
                    format,
                    confidence,
                    inspection,
                } = &found
                else {
                    panic!("{}: no native candidate: {found:?}", case.format);
                };
                assert_eq!(format.as_str(), case.format, "{found:?}");
                let catalog = InputCatalog::with_builtins();
                let resolved = catalog
                    .resolve_source(case.bytes, None)
                    .unwrap_or_else(|error| panic!("{}: {error}", case.format));
                let crate::ResolvedSource::Native { codec, selection } = resolved else {
                    panic!("{}: resolver did not select a native codec", case.format);
                };
                assert_eq!(*format, codec.id(), "{}: resolver winner", case.format);
                assert_eq!(
                    Some(*confidence),
                    selection.confidence(),
                    "{}: resolver confidence",
                    case.format
                );
                let summary = inspection
                    .as_ref()
                    .unwrap_or_else(|_| panic!("{}: inspection did not classify", case.format));
                assert_eq!(
                    summary
                        .dialects()
                        .map(cadmpeg_core::dialect::DialectLayers::primary)
                        .map(cadmpeg_core::dialect::DialectMatch::dialect)
                        .map(DialectId::as_str),
                    Some(case.dialect),
                    "{}: dialect",
                    case.format
                );
            }
        }
    }

    /// A ZIP with no format marker is several formats at once, and the answer
    /// says so instead of picking one.
    ///
    /// Every ZIP-based codec reports [`Confidence::Low`] for it. Candidate
    /// detection retains the equal-confidence ambiguity, so no container is
    /// opened and no dialect is claimed.
    #[cfg(all(feature = "fcstd", feature = "f3d"))]
    #[test]
    fn a_markerless_zip_identifies_as_several_formats_with_no_dialect() {
        let found = run(b"PK\x03\x04 markerless", &inspection());
        let Identification::Ambiguous {
            confidence,
            candidates,
        } = &found
        else {
            panic!("expected ambiguity: {found:?}");
        };
        assert!(candidates.len() > 1, "{found:?}");
        assert_eq!(*confidence, Confidence::Low);
        let formats = candidates
            .iter()
            .map(|format| format.as_str())
            .collect::<Vec<_>>();
        assert!(
            formats.contains(&"fcstd") && formats.contains(&"f3d"),
            "{formats:?}"
        );
    }

    /// A budget too small to rebuild the container keeps the prefix evidence
    /// and reports no dialect.
    ///
    /// The prefix still names the format at high confidence; only the
    /// reconstruction that would settle the dialect is refused. Reporting a
    /// dialect here would be reporting one nothing read.
    #[cfg(feature = "rhino")]
    #[test]
    fn an_exhausted_budget_keeps_the_format_and_reports_the_failure() {
        let bytes = include_bytes!("../../cadmpeg-codec-rhino/tests/golden/fixtures/arc.3dm");
        let starved = InspectOptions {
            limits: ResourceLimits {
                // Enough for the 32-byte archive header the prefix detector
                // reads, far short of the whole file the inspection acquires.
                max_input_bytes: 64,
                ..ResourceLimits::desktop()
            },
        };
        assert!(bytes.len() > 64);

        let found = run(bytes, &starved);
        let Identification::Native {
            format,
            confidence,
            inspection: Err(error),
        } = &found
        else {
            panic!("expected a typed inspection failure: {found:?}");
        };
        assert_eq!(*format, FormatId::new("rhino"));
        assert_eq!(*confidence, Confidence::High);
        assert!(matches!(error, CodecError::ResourceLimit(_)), "{error}");

        let catalog = InputCatalog::with_builtins();
        let mut source = Cursor::new(bytes);
        let Err(InspectError::Codec {
            format,
            selection,
            error,
        }) = resolve_and_inspect_with(&catalog, &mut source, None, &starved)
        else {
            panic!("resolved inspection must retain the selected codec failure");
        };
        assert_eq!(format, FormatId::new("rhino"));
        assert_eq!(
            selection,
            Selection::Detected {
                confidence: Confidence::High
            }
        );
        assert!(matches!(error, CodecError::ResourceLimit(_)), "{error}");

        // The same bytes under the default budget do settle the dialect, so
        // the difference above is the budget and not the fixture.
        assert!(matches!(
            run(bytes, &inspection()),
            Identification::Native {
                inspection: Ok(_),
                ..
            }
        ));
    }

    #[test]
    fn cadir_json_is_identified_without_container_inspection() {
        for bytes in [
            b" \n{\"ir_version\": 1}".as_slice(),
            b"\xef\xbb\xbf\t{\"ir_version\": 1}".as_slice(),
        ] {
            let found = run(bytes, &inspection());
            assert!(matches!(found, Identification::Cadir));
        }
    }

    /// Bytes no codec recognizes produce no candidates at all.
    #[test]
    fn an_unrecognized_prefix_names_nothing() {
        assert!(matches!(
            run(b"not a CAD file at all\n", &inspection()),
            Identification::None
        ));
    }
}
