# ZIP container framing

## 1. Central directory

The central-directory file header is 46 bytes in little-endian order. Its signature is `0x02014b50` at offset 0. The producer version, extraction version, and general flags are 16-bit words at offsets 4, 6, and 8. The 16-bit compression code is at offset 10. Time and date words are at offsets 12 and 14. CRC-32, compressed length, and expanded length occupy offsets 16, 20, and 24. Name, extra-data, and comment lengths are 16-bit words at offsets 28, 30, and 32. Starting disk and internal attributes are words at offsets 34 and 36. External attributes and the local-header position are 32-bit words at offsets 38 and 42. The name, extra data, and comment follow the fixed header, in that order. [PKWARE APPNOTE §4.3.12](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT).

Encryption, compression support, payload checksum, and expansion validity concern a member's readable contents. They do not change its central-directory identity or bounded stored extent. An unreadable member has no admitted expanded payload. ZIP structural extents and unique entry identities must remain available before any member is interpreted.
