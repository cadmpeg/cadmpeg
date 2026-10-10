# ZIP container framing

## 1. Central directory

The central-directory file header is 46 bytes in little-endian order. Its signature is `0x02014b50` at offset 0. The producer version, extraction version, and general flags are 16-bit words at offsets 4, 6, and 8. The 16-bit compression code is at offset 10. Time and date words are at offsets 12 and 14. CRC-32, compressed length, and expanded length occupy offsets 16, 20, and 24. Name, extra-data, and comment lengths are 16-bit words at offsets 28, 30, and 32. Starting disk and internal attributes are words at offsets 34 and 36. External attributes and the local-header position are 32-bit words at offsets 38 and 42. The name, extra data, and comment follow the fixed header, in that order. [PKWARE APPNOTE §4.3.12](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT).

Encryption, compression support, payload checksum, and expansion validity concern a member's readable contents. They do not change its central-directory identity or bounded stored extent. An unreadable member has no admitted expanded payload. ZIP structural extents and unique entry identities must remain available before any member is interpreted.

## 2. Local member frames

The local-file header is 30 bytes in little-endian order. Its signature is `0x04034b50` at offset 0. Extraction version, general flags, and compression code are 16-bit words at offsets 4, 6, and 8. Time and date words are at offsets 10 and 12. CRC-32, compressed length, and expanded length are 32-bit words at offsets 14, 18, and 22. The filename length and extra-data length are 16-bit words at offsets 26 and 28. The filename and extra data follow the fixed header. The stored payload follows the extra data. [PKWARE APPNOTE §4.3.7–4.3.8](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT).

CADIR decision: a central declaration can retain the identity of a member whose local header cannot be admitted. The unreadable member's physical span extends from its declared local-header position to the next declared local-header position, or to the central directory for the last member. It has no expanded payload. A consumer that requires this member must refuse its contents; the container ledger must identify the bounded physical span. Recovery must report the local-header failure.

## 3. End records

The end-of-central-directory record is 22 bytes in little-endian order. Its signature is `0x06054b50` at offset 0. Disk number, directory disk, entries on this disk, and total entries are 16-bit words at offsets 4, 6, 8, and 10. Directory size and directory offset are 32-bit words at offsets 12 and 16. The comment length is a 16-bit word at offset 20. The comment follows the fixed record. Its maximum length is 65535 bytes. [PKWARE APPNOTE §4.3.16](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT).

The ZIP64 end record has a 56-byte fixed prefix. Its signature is `0x06064b50` at offset 0. The 64-bit record size at offset 4 excludes the first 12 bytes. Producer and extraction versions are 16-bit words at offsets 12 and 14. Disk and directory disk are 32-bit words at offsets 16 and 20. Entries on this disk, total entries, directory size, and directory offset are 64-bit words at offsets 24, 32, 40, and 48. Extensible data follows the fixed prefix. The 20-byte ZIP64 locator has signature `0x07064b50` at offset 0, a 32-bit end-record disk at offset 4, a 64-bit end-record offset at offset 8, and a 32-bit disk count at offset 16. [PKWARE APPNOTE §4.3.14–4.3.15](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT).

The directory digital-signature record has a 6-byte fixed prefix. Its signature is `0x05054b50` at offset 0. The 16-bit data length is at offset 4. Signature data follows the fixed prefix. This optional record follows the central file headers. [PKWARE APPNOTE §4.3.12–4.3.13](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT).

The end record and its declared comment terminate the archive. Embedded member archives have their own directory and end records. Comment bytes do not declare member identities. A directory declaration must account for its complete header sequence and declared size. Conflicting terminal directory declarations do not identify one archive namespace. [PKWARE APPNOTE §4.3.1, §4.3.6, §4.3.12–4.3.16](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT).
