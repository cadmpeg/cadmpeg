// SPDX-License-Identifier: Apache-2.0
//! Per-codec truncation and mutation boundaries.

/// What a boundary marks. Header and count positions additionally seed the
/// single-byte mutation spot-checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryKind {
    /// A fixed-position header edge.
    Header,
    /// A record/segment framing signature.
    Record,
    /// A container entry framing signature.
    Entry,
    /// A length or count field, a prime single-byte mutation target.
    Count,
}

/// One named boundary within a fixture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Boundary {
    /// Absolute byte offset.
    pub offset: usize,
    /// What the offset marks.
    pub kind: BoundaryKind,
}

/// Names the record/entry boundaries of one codec's format.
pub trait BoundaryProvider {
    /// The codec id these boundaries belong to.
    fn codec_id(&self) -> &'static str;

    /// The recognized boundaries within `bytes`, sorted and de-duplicated by
    /// `(offset, kind)`.
    fn boundaries(&self, bytes: &[u8]) -> Vec<Boundary>;
}

/// One framing signature and the boundary kind its occurrences mark.
struct Signature {
    bytes: &'static [u8],
    kind: BoundaryKind,
    /// Byte offsets, relative to a match start, of length/count fields to flag
    /// as [`BoundaryKind::Count`] mutation targets.
    count_fields: &'static [usize],
}

/// The structural signature table for one codec.
struct CodecBoundarySpec {
    codec_id: &'static str,
    /// The fixed header length; a [`BoundaryKind::Header`] edge sits at `0` and
    /// at this offset.
    header_len: usize,
    signatures: &'static [Signature],
}

impl BoundaryProvider for CodecBoundarySpec {
    fn codec_id(&self) -> &'static str {
        self.codec_id
    }

    fn boundaries(&self, bytes: &[u8]) -> Vec<Boundary> {
        let mut out = Vec::new();
        out.push(Boundary {
            offset: 0,
            kind: BoundaryKind::Header,
        });
        if self.header_len <= bytes.len() {
            out.push(Boundary {
                offset: self.header_len,
                kind: BoundaryKind::Header,
            });
        }
        for signature in self.signatures {
            for start in find_all(bytes, signature.bytes) {
                out.push(Boundary {
                    offset: start,
                    kind: signature.kind,
                });
                let field_base = start + signature.bytes.len();
                for field in signature.count_fields {
                    let offset = field_base + field;
                    if offset < bytes.len() {
                        out.push(Boundary {
                            offset,
                            kind: BoundaryKind::Count,
                        });
                    }
                }
            }
        }
        out.sort_by_key(|b| (b.offset, kind_rank(b.kind)));
        out.dedup_by_key(|b| (b.offset, kind_rank(b.kind)));
        out
    }
}

fn kind_rank(kind: BoundaryKind) -> u8 {
    match kind {
        BoundaryKind::Header => 0,
        BoundaryKind::Record => 1,
        BoundaryKind::Entry => 2,
        BoundaryKind::Count => 3,
    }
}

/// Every start offset of `needle` within `haystack`.
fn find_all(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    if needle.is_empty() || needle.len() > haystack.len() {
        return out;
    }
    let mut i = 0;
    while i + needle.len() <= haystack.len() {
        if &haystack[i..i + needle.len()] == needle {
            out.push(i);
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

/// F3D ZIP boundaries.
const F3D: CodecBoundarySpec = CodecBoundarySpec {
    codec_id: "f3d",
    header_len: 4,
    signatures: &[
        Signature {
            bytes: b"PK\x03\x04",
            kind: BoundaryKind::Entry,
            count_fields: &[22, 24],
        },
        Signature {
            bytes: b"PK\x01\x02",
            kind: BoundaryKind::Record,
            count_fields: &[24, 28],
        },
        Signature {
            bytes: b"PK\x05\x06",
            kind: BoundaryKind::Header,
            count_fields: &[8, 10],
        },
    ],
};

/// SLDPRT container boundaries.
const SLDPRT: CodecBoundarySpec = CodecBoundarySpec {
    codec_id: "sldprt",
    header_len: 8,
    signatures: &[Signature {
        bytes: &[0x14, 0x00, 0x06, 0x00, 0x08, 0x00],
        kind: BoundaryKind::Record,
        count_fields: &[0, 4, 8, 12, 16],
    }],
};

/// CATIA container boundaries.
const CATIA: CodecBoundarySpec = CodecBoundarySpec {
    codec_id: "catia",
    header_len: 8,
    signatures: &[
        Signature {
            bytes: b"V5_CFV2\0",
            kind: BoundaryKind::Header,
            count_fields: &[0, 4],
        },
        Signature {
            bytes: b"CATIA_V5 CB0001\0",
            kind: BoundaryKind::Record,
            count_fields: &[0, 4],
        },
        Signature {
            bytes: b"FINJPL  ",
            kind: BoundaryKind::Entry,
            count_fields: &[0, 4],
        },
    ],
};

/// Creo PSB boundaries.
const CREO: CodecBoundarySpec = CodecBoundarySpec {
    codec_id: "creo",
    header_len: 6,
    signatures: &[
        Signature {
            bytes: b"#UGC:2",
            kind: BoundaryKind::Header,
            count_fields: &[],
        },
        Signature {
            bytes: b"#-END_OF_UGC_HEADER",
            kind: BoundaryKind::Record,
            count_fields: &[],
        },
        Signature {
            bytes: b"#UGC_TOC",
            kind: BoundaryKind::Record,
            count_fields: &[],
        },
        Signature {
            bytes: b"#END_OF_TOC_HEADER",
            kind: BoundaryKind::Record,
            count_fields: &[],
        },
    ],
};

/// NX SPLMSSTR boundaries.
const NX: CodecBoundarySpec = CodecBoundarySpec {
    codec_id: "nx",
    header_len: 8,
    signatures: &[Signature {
        bytes: b"SPLMSSTR",
        kind: BoundaryKind::Record,
        count_fields: &[0, 4, 8],
    }],
};

/// rhino: the 24-byte `3D Geometry File Format ` header prefix; chunk framing
/// beyond the header is length-prefixed and not statically discoverable without
/// the parser, so this names the header edge only.
const RHINO: CodecBoundarySpec = CodecBoundarySpec {
    codec_id: "rhino",
    header_len: 24,
    signatures: &[Signature {
        bytes: b"3D Geometry File Format ",
        kind: BoundaryKind::Header,
        count_fields: &[8, 12],
    }],
};

/// The boundary provider for `codec_id`, or `None` for an unknown codec.
pub fn provider_for(codec_id: &str) -> Option<&'static dyn BoundaryProvider> {
    let spec: &'static CodecBoundarySpec = match codec_id {
        "f3d" => &F3D,
        "sldprt" => &SLDPRT,
        "catia" => &CATIA,
        "creo" => &CREO,
        "nx" => &NX,
        "rhino" => &RHINO,
        _ => return None,
    };
    Some(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_all_non_overlapping() {
        assert_eq!(find_all(b"ababab", b"ab"), vec![0, 2, 4]);
        assert_eq!(find_all(b"aaa", b"aa"), vec![0]);
        assert_eq!(find_all(b"abc", b"z"), Vec::<usize>::new());
    }

    #[test]
    fn zip_local_header_is_an_entry_boundary() {
        let provider = provider_for("f3d").expect("f3d provider");
        let bytes = b"PK\x03\x04rest-of-header-padding-bytes";
        let boundaries = provider.boundaries(bytes);
        assert!(boundaries
            .iter()
            .any(|b| b.offset == 0 && b.kind == BoundaryKind::Entry));
    }

    #[test]
    fn header_edge_always_present() {
        for codec in crate::execute::CODEC_IDS {
            let provider = provider_for(codec).expect("provider");
            let boundaries = provider.boundaries(&[0u8; 64]);
            assert!(boundaries
                .iter()
                .any(|b| b.offset == 0 && b.kind == BoundaryKind::Header));
        }
    }
}
