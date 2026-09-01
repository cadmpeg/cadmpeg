// SPDX-License-Identifier: Apache-2.0
//! Global field 23 declarations and their relationship to parser grammars.

use crate::global::GlobalTable;
use crate::IgesVersion;

/// Whether field 23 selected a Global table this codec verified for the version
/// the source declared, and if not, why not.
///
/// Distinct from [`GlobalTable`], which names the grammar actually used. This
/// names the relationship between that grammar and the declaration: a decode
/// can use the shared 5.1--5.3 grammar because the file names one of those
/// versions, because field 23 was unreadable, or because field 23 named a value
/// outside the version table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DialectRecovery<'a> {
    /// Field 23 names a version whose Global table this codec verified against
    /// that version's own specification. The only state that charges no loss.
    Verified,
    /// The Global table was not verified for the source declaration.
    Unverified(UnverifiedDialectRecovery<'a>),
}

/// Why the selected Global table was not verified for the source declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnverifiedDialectRecovery<'a> {
    /// Field 23 does not read as an integer; the specification default stood in.
    UnreadableDeclaration(&'a str),
    /// Field 23 names a value outside the version table, moved by the
    /// postprocessor clamp of IGES 5.3 section 2.2.4.3.23.
    Clamped,
    /// Field 23 names a version whose own specification this codec has not
    /// verified its Global table against.
    UnverifiedVersion,
}

/// One entry in the eleven-value version table selected by Global field 23.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub(crate) enum VersionFlag {
    V1_0 = 1,
    AnsiY1426M1981 = 2,
    V2_0 = 3,
    V3_0 = 4,
    AsmeAnsiY1426M1987 = 5,
    V4_0 = 6,
    AsmeY1426M1989 = 7,
    V5_0 = 8,
    V5_1 = 9,
    V5_2 = 10,
    V5_3 = 11,
}

impl VersionFlag {
    const ALL: [Self; 11] = [
        Self::V1_0,
        Self::AnsiY1426M1981,
        Self::V2_0,
        Self::V3_0,
        Self::AsmeAnsiY1426M1987,
        Self::V4_0,
        Self::AsmeY1426M1989,
        Self::V5_0,
        Self::V5_1,
        Self::V5_2,
        Self::V5_3,
    ];
    const MIN: i64 = Self::V1_0 as i64;
    const MAX: i64 = Self::V5_3 as i64;

    /// Returns the exact table entry, without applying postprocessor recovery.
    pub(crate) const fn exact(value: i64) -> Option<Self> {
        if value < Self::MIN || value > Self::MAX {
            return None;
        }
        Some(Self::ALL[(value - Self::MIN) as usize])
    }

    /// Applies the IGES 5.3 postprocessor clamp to a declared value.
    pub(crate) const fn effective(declared: i64) -> Self {
        match Self::exact(declared) {
            Some(flag) => flag,
            None if declared < Self::MIN => Self::V2_0,
            None => Self::V5_3,
        }
    }

    pub(crate) const fn value(self) -> i64 {
        self as i64
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::V1_0 => "1.0",
            Self::AnsiY1426M1981 => "ANSI-Y14.26M-1981",
            Self::V2_0 => "2.0",
            Self::V3_0 => "3.0",
            Self::AsmeAnsiY1426M1987 => "ASME-ANSI-Y14.26M-1987",
            Self::V4_0 => "4.0",
            Self::AsmeY1426M1989 => "ASME-Y14.26M-1989",
            Self::V5_0 => "5.0",
            Self::V5_1 => "5.1",
            Self::V5_2 => "5.2",
            Self::V5_3 => "5.3",
        }
    }

    pub(crate) const fn global_table(self) -> GlobalTable {
        match self {
            Self::V4_0 => GlobalTable::V4_0,
            Self::V5_0 => GlobalTable::V5_0,
            Self::V5_1 | Self::V5_2 | Self::V5_3 => GlobalTable::V5Later,
            Self::V1_0
            | Self::AnsiY1426M1981
            | Self::V2_0
            | Self::V3_0
            | Self::AsmeAnsiY1426M1987
            | Self::AsmeY1426M1989 => GlobalTable::Legacy,
        }
    }

    pub(crate) const fn verified_version(self) -> Option<IgesVersion> {
        match self {
            Self::V4_0 => Some(IgesVersion::V4_0),
            Self::V5_0 => Some(IgesVersion::V5_0),
            Self::V5_1 => Some(IgesVersion::V5_1),
            Self::V5_2 => Some(IgesVersion::V5_2),
            Self::V5_3 => Some(IgesVersion::V5_3),
            Self::V1_0
            | Self::AnsiY1426M1981
            | Self::V2_0
            | Self::V3_0
            | Self::AsmeAnsiY1426M1987
            | Self::AsmeY1426M1989 => None,
        }
    }

    pub(crate) const fn from_write_version(version: IgesVersion) -> Self {
        match version {
            IgesVersion::V4_0 => Self::V4_0,
            IgesVersion::V5_0 => Self::V5_0,
            IgesVersion::V5_1 => Self::V5_1,
            IgesVersion::V5_2 => Self::V5_2,
            IgesVersion::V5_3 => Self::V5_3,
        }
    }
}
