// SPDX-License-Identifier: Apache-2.0
//! Per-codec boundary providers: each codec names the record and entry
//! boundaries the truncation sweep truncates around and the header/count
//! positions the mutation spot-checks flip.
//!
//! A boundary provider names structural offsets a codec recognizes without
//! re-running its parser: the fixed header length and every occurrence of the
//! format's framing signatures (ZIP local headers, container magics, block and
//! record markers). This is deliberately a structural over-approximation — a
//! marker byte pattern can occur inside payload — which is the correct bias for
//! a sweep: extra truncation points cost time, never coverage.

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

/// A total order over kinds so `dedup_by_key` collapses exact duplicates while
/// keeping distinct kinds at one offset.
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

// The ZIP local-header count fields sit 22 and 24 bytes past the signature
// (filename length and extra-field length); flipping them redirects the entry
// parse. The container magics carry a following big/little-endian length or
// count word, flagged at offset 0 past the header edge.

/// f3d: a ZIP archive. Local file headers are entry boundaries, central
/// directory records are record boundaries, and the end-of-central-directory
/// marks the trailing header.
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

/// sldprt: an 8-byte header, then raw-DEFLATE blocks, cache cells, and
/// directory entries introduced by a shared 6-byte marker. The block header's
/// type, CRC, and size words follow the marker.
const SLDPRT: CodecBoundarySpec = CodecBoundarySpec {
    codec_id: "sldprt",
    header_len: 8,
    signatures: &[Signature {
        bytes: &[0x14, 0x00, 0x06, 0x00, 0x08, 0x00],
        kind: BoundaryKind::Record,
        count_fields: &[0, 4, 8, 12, 16],
    }],
};

/// catia: an 8-byte outer magic, a nested stream-directory magic, and FINJPL
/// named-body markers.
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

/// creo: the `#UGC:2` PSB magic and the ASCII framing lines that delimit the
/// header and table of contents.
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

/// nx: the 8-byte `SPLMSSTR` container magic bounding the header and footer
/// directory regions.
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
