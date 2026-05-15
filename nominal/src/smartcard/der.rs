// DER encoding for ECDSA signatures.
//
// PKCS#11 CKM_ECDSA* returns raw r || s (each component is key-size bytes,
// zero-padded to a fixed width). TLS requires a DER-encoded
// SEQUENCE { INTEGER r, INTEGER s } as defined in RFC 3279 §2.2.3.

/// Convert a raw PKCS#11 ECDSA signature to the DER form required by TLS.
///
/// `raw` must be exactly 2 * (key-size) bytes: the first half is `r`, the
/// second is `s`, each left-padded with zeros to the key size. Supports
/// P-256 (64 bytes) and P-384 (96 bytes).
pub(super) fn ecdsa_raw_to_der(raw: &[u8]) -> Vec<u8> {
    let half = raw.len() / 2;
    let r = encode_integer(&raw[..half]);
    let s = encode_integer(&raw[half..]);
    let body_len = r.len() + s.len();
    let mut out = Vec::with_capacity(4 + body_len);
    out.push(0x30); // SEQUENCE tag
    push_len(&mut out, body_len);
    out.extend_from_slice(&r);
    out.extend_from_slice(&s);
    out
}

/// Encode `n` as a DER INTEGER (tag + length + value).
///
/// Strips leading zero bytes (keeping at least one byte) and prepends a `0x00`
/// byte when the most-significant bit is set, ensuring the value is treated as
/// a positive integer.
fn encode_integer(n: &[u8]) -> Vec<u8> {
    // Strip leading zeros, keeping at least the last byte.
    let n = match n.iter().position(|&b| b != 0) {
        Some(i) => &n[i..],
        None => &n[n.len() - 1..],
    };
    let pad = n[0] & 0x80 != 0;
    let content_len = n.len() + usize::from(pad);
    let mut out = Vec::with_capacity(2 + content_len);
    out.push(0x02); // INTEGER tag
    push_len(&mut out, content_len);
    if pad {
        out.push(0x00);
    }
    out.extend_from_slice(n);
    out
}

pub(super) fn push_len(buf: &mut Vec<u8>, len: usize) {
    if len < 0x80 {
        buf.push(len as u8);
    } else if len < 0x100 {
        buf.extend_from_slice(&[0x81, len as u8]);
    } else {
        buf.extend_from_slice(&[0x82, (len >> 8) as u8, len as u8]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- DER parsing helpers (test-only) ------------------------------------

    /// Read a DER length field. Returns (value, bytes consumed by the length field).
    fn read_len(bytes: &[u8]) -> (usize, usize) {
        if bytes[0] < 0x80 {
            (bytes[0] as usize, 1)
        } else if bytes[0] == 0x81 {
            (bytes[1] as usize, 2)
        } else if bytes[0] == 0x82 {
            ((bytes[1] as usize) << 8 | bytes[2] as usize, 3)
        } else {
            panic!("unsupported DER length encoding: {:#x}", bytes[0])
        }
    }

    /// Parse a DER INTEGER from the start of `encoded`. Returns the integer
    /// value bytes (including any sign-extension `0x00` prefix) and the total
    /// number of bytes consumed from `encoded`.
    fn parse_integer(encoded: &[u8]) -> (&[u8], usize) {
        assert_eq!(encoded[0], 0x02, "expected INTEGER tag 0x02");
        let (content_len, len_size) = read_len(&encoded[1..]);
        let value_start = 1 + len_size;
        let value = &encoded[value_start..value_start + content_len];
        (value, value_start + content_len)
    }

    /// Parse a full DER ECDSA signature. Returns (r_value_bytes, s_value_bytes).
    fn parse_ecdsa_der(der: &[u8]) -> (&[u8], &[u8]) {
        assert_eq!(der[0], 0x30, "expected SEQUENCE tag 0x30");
        let (body_len, len_size) = read_len(&der[1..]);
        let body = &der[1 + len_size..1 + len_size + body_len];
        let (r, r_consumed) = parse_integer(body);
        let (s, _) = parse_integer(&body[r_consumed..]);
        (r, s)
    }

    // --- Structure tests ----------------------------------------------------

    #[test]
    fn p256_output_has_correct_sequence_structure() {
        // P-256: 32 + 32 = 64-byte raw input, neither component has high bit set.
        let r_raw = [0x12u8; 32];
        let s_raw = [0x34u8; 32];
        let mut raw = r_raw.to_vec();
        raw.extend_from_slice(&s_raw);

        let der = ecdsa_raw_to_der(&raw);

        assert_eq!(der[0], 0x30, "outer SEQUENCE tag");
        let (body_len, _) = read_len(&der[1..]);
        // r: tag(1) + len(1) + 32 = 34; s: tag(1) + len(1) + 32 = 34; total = 68
        assert_eq!(body_len, 68);
    }

    #[test]
    fn p384_output_has_correct_sequence_structure() {
        // P-384: 48 + 48 = 96-byte raw input.
        let r_raw = [0x12u8; 48];
        let s_raw = [0x34u8; 48];
        let mut raw = r_raw.to_vec();
        raw.extend_from_slice(&s_raw);

        let der = ecdsa_raw_to_der(&raw);

        assert_eq!(der[0], 0x30);
        let (body_len, _) = read_len(&der[1..]);
        // r: 1 + 1 + 48 = 50; s: 1 + 1 + 48 = 50; total = 100
        assert_eq!(body_len, 100);
    }

    // --- Correctness tests: parsed values match the original numbers ---------

    #[test]
    fn integer_values_round_trip_without_high_bit() {
        // r and s both have 0x12 as first byte — high bit clear, no padding needed.
        let r_raw = [0x12u8; 32];
        let s_raw = [0x34u8; 32];
        let mut raw = r_raw.to_vec();
        raw.extend_from_slice(&s_raw);

        let der = ecdsa_raw_to_der(&raw);
        let (r_der, s_der) = parse_ecdsa_der(&der);

        assert_eq!(r_der, &r_raw, "r must round-trip exactly");
        assert_eq!(s_der, &s_raw, "s must round-trip exactly");
    }

    #[test]
    fn known_p256_byte_sequence() {
        // r = [0x01, 0x02, ..., 0x20], s = [0x21, ..., 0x40]
        // Neither starts with 0x00 or has a high bit, so no stripping or padding.
        // Expected: 30 44 02 20 01..20 02 20 21..40
        let r_raw: Vec<u8> = (1u8..=32).collect();
        let s_raw: Vec<u8> = (33u8..=64).collect();
        let mut raw = r_raw.clone();
        raw.extend_from_slice(&s_raw);

        let der = ecdsa_raw_to_der(&raw);

        assert_eq!(der[0], 0x30);
        assert_eq!(der[1], 0x44); // body = 68 bytes
        assert_eq!(der[2], 0x02); // r INTEGER tag
        assert_eq!(der[3], 0x20); // r length = 32
        assert_eq!(&der[4..36], r_raw.as_slice());
        assert_eq!(der[36], 0x02); // s INTEGER tag
        assert_eq!(der[37], 0x20); // s length = 32
        assert_eq!(&der[38..70], s_raw.as_slice());
    }

    // --- Leading-zero stripping tests ---------------------------------------

    #[test]
    fn leading_zeros_stripped_and_value_preserved() {
        // r = [0x00, 0x00, 0x42, 0x00, ...] (leading zeros then a non-zero byte)
        let mut r_raw = [0x00u8; 32];
        r_raw[2] = 0x42;
        let s_raw = [0x01u8; 32];
        let mut raw = r_raw.to_vec();
        raw.extend_from_slice(&s_raw);

        let der = ecdsa_raw_to_der(&raw);
        let (r_der, _) = parse_ecdsa_der(&der);

        // DER INTEGER for r should be [0x42, 0x00, ..., 0x00] (30 bytes)
        assert_eq!(r_der[0], 0x42, "first byte after stripping");
        assert_eq!(r_der.len(), 30, "30 bytes remain after stripping 2 zeros");
        assert!(r_der.iter().skip(1).all(|&b| b == 0x00));
    }

    #[test]
    fn all_zero_r_becomes_single_zero_byte() {
        // r = all zeros → mathematically 0, DER INTEGER value should be [0x00]
        let r_raw = [0x00u8; 32];
        let s_raw = [0x01u8; 32];
        let mut raw = r_raw.to_vec();
        raw.extend_from_slice(&s_raw);

        let der = ecdsa_raw_to_der(&raw);
        let (r_der, _) = parse_ecdsa_der(&der);

        assert_eq!(r_der, &[0x00], "all-zero r encodes as a single 0x00 byte");
    }

    // --- High-bit padding tests --------------------------------------------

    #[test]
    fn high_bit_r_gets_zero_pad() {
        // r starts with 0x80 (high bit set → DER INTEGER needs 0x00 prefix)
        let mut r_raw = [0x01u8; 32];
        r_raw[0] = 0x80;
        let s_raw = [0x01u8; 32];
        let mut raw = r_raw.to_vec();
        raw.extend_from_slice(&s_raw);

        let der = ecdsa_raw_to_der(&raw);
        let (r_der, _) = parse_ecdsa_der(&der);

        assert_eq!(r_der.len(), 33, "r is 33 bytes: 0x00 prefix + 32 raw bytes");
        assert_eq!(r_der[0], 0x00, "sign-extension zero");
        assert_eq!(r_der[1], 0x80, "original first byte follows");
    }

    #[test]
    fn high_bit_s_gets_zero_pad() {
        let r_raw = [0x01u8; 32];
        let mut s_raw = [0x01u8; 32];
        s_raw[0] = 0xff; // high bit and more
        let mut raw = r_raw.to_vec();
        raw.extend_from_slice(&s_raw);

        let der = ecdsa_raw_to_der(&raw);
        let (_, s_der) = parse_ecdsa_der(&der);

        assert_eq!(s_der.len(), 33);
        assert_eq!(s_der[0], 0x00);
        assert_eq!(s_der[1], 0xff);
    }

    // --- Both conditions simultaneously ------------------------------------

    #[test]
    fn r_has_leading_zeros_s_has_high_bit() {
        let mut r_raw = [0x00u8; 32];
        r_raw[4] = 0x7f; // after 4 leading zeros
        let mut s_raw = [0x00u8; 32];
        s_raw[0] = 0x90; // high bit set

        let mut raw = r_raw.to_vec();
        raw.extend_from_slice(&s_raw);

        let der = ecdsa_raw_to_der(&raw);
        let (r_der, s_der) = parse_ecdsa_der(&der);

        assert_eq!(r_der[0], 0x7f, "r: leading zeros stripped");
        assert_eq!(r_der.len(), 28, "r: 4 zeros stripped from 32-byte field");
        assert_eq!(s_der[0], 0x00, "s: sign-extension zero added");
        assert_eq!(s_der[1], 0x90, "s: original byte follows");
    }
}
