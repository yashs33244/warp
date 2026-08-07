use super::*;

#[test]
fn escape_rules_per_byte() {
    for b in 0u16..=255 {
        let b = b as u8;
        let e = escape(&[b]);
        if b < 0x20 || b == b'\\' {
            assert_eq!(e.len(), 4, "byte {b:#04x} should escape to 4 bytes");
            assert_eq!(e[0], b'\\');
            let expected = format!("\\{:03o}", b);
            assert_eq!(e, expected.as_bytes(), "byte {b:#04x} octal form");
        } else {
            assert_eq!(e, vec![b], "byte {b:#04x} should pass through");
        }
    }
}

#[test]
fn roundtrip_every_single_byte() {
    for b in 0u16..=255 {
        let b = b as u8;
        let raw = vec![b];
        assert_eq!(unescape(&escape(&raw)), raw, "roundtrip byte {b:#04x}");
    }
}

#[test]
fn roundtrip_every_pair() {
    for a in 0u16..=255 {
        for c in 0u16..=255 {
            let raw = vec![a as u8, c as u8];
            assert_eq!(unescape(&escape(&raw)), raw);
        }
    }
}

#[test]
fn roundtrip_special_sequences() {
    let all_low: Vec<u8> = (0u8..0x20).collect();
    let all_high: Vec<u8> = (0x80u16..=0xFF).map(|b| b as u8).collect();
    let all_bytes: Vec<u8> = (0u16..=255).map(|b| b as u8).collect();
    let backslashes: Vec<u8> = vec![b'\\'; 10];
    let mixed: Vec<u8> = b"hello\x00\x1b[31mworld\\\xff\x07done".to_vec();
    let cases: Vec<Vec<u8>> = vec![
        vec![],
        all_low,
        all_high,
        all_bytes,
        backslashes,
        mixed,
        b"\\\\\\".to_vec(),
        vec![0x00; 100],
        vec![0xFF; 100],
    ];
    for raw in cases {
        assert_eq!(unescape(&escape(&raw)), raw, "roundtrip failed");
    }
}

#[test]
fn decode_specific_values() {
    assert_eq!(unescape(b"\\012"), vec![0x0A]);
    assert_eq!(unescape(b"\\033"), vec![0x1B]);
    assert_eq!(unescape(b"\\134"), vec![0x5C]);
    assert_eq!(unescape(b"\\000"), vec![0x00]);
    assert_eq!(unescape(b"\\377"), vec![0xFF]);
}

#[test]
fn high_bytes_pass_through() {
    assert_eq!(unescape(&[0x80]), vec![0x80]);
    assert_eq!(unescape(&[0xFF]), vec![0xFF]);
    assert_eq!(escape(&[0x80]), vec![0x80]);
    assert_eq!(escape(&[0xFF]), vec![0xFF]);
}

#[test]
fn out_of_range_octal_is_literal() {
    // \400 = 256 decimal, out of u8 range -> literal
    assert_eq!(unescape(b"\\400"), b"\\400".to_vec());
    assert_eq!(unescape(b"\\777"), b"\\777".to_vec());
}

#[test]
fn truncated_and_malformed_no_panic() {
    // lone trailing backslash
    assert_eq!(unescape(b"\\"), b"\\".to_vec());
    // 2 octal digits only
    assert_eq!(unescape(b"\\01"), b"\\01".to_vec());
    // 1 octal digit
    assert_eq!(unescape(b"\\0"), b"\\0".to_vec());
    // backslash then non-octal
    assert_eq!(unescape(b"\\099"), b"\\099".to_vec());
    assert_eq!(unescape(b"\\8"), b"\\8".to_vec());
    assert_eq!(unescape(b"\\9ab"), b"\\9ab".to_vec());
    // backslash at very end after content
    assert_eq!(unescape(b"abc\\"), b"abc\\".to_vec());
    // backslash-backslash (only 2 chars, needs 3 digits) -> literal
    assert_eq!(unescape(b"\\\\"), b"\\\\".to_vec());
}

#[test]
fn escaped_backslash_literal_in_stream() {
    // \134 is backslash; ensure a following \134 decodes independently
    assert_eq!(unescape(b"\\134\\134"), vec![0x5C, 0x5C]);
    // bytes: '\','\','0','1','2'. i=0: window is '\','0','1' -> '\' not
    // octal -> literal '\'. Then "\\012" (i=1) -> 0x0A.
    assert_eq!(unescape(b"\\\\012"), vec![b'\\', 0x0A]);
}

#[test]
fn no_panic_on_arbitrary_backslash_positions() {
    // 3-byte inputs (backslash + 2 trailing): must not panic and stay literal
    for a in 0u16..=255 {
        for b in 0u16..=255 {
            let input = [b'\\', a as u8, b as u8];
            let out = unescape(&input);
            assert_eq!(out, input.to_vec());
        }
    }
    // Full 4-byte windows over ASCII digit space
    for d0 in b'0'..=b'9' {
        for d1 in b'0'..=b'9' {
            for d2 in b'0'..=b'9' {
                let input = [b'\\', d0, d1, d2];
                let _ = unescape(&input);
            }
        }
    }
}

#[test]
fn escape_output_only_uses_octal_digits() {
    for b in 0u16..=255 {
        let e = escape(&[b as u8]);
        if e.len() == 4 {
            assert_eq!(e[0], b'\\');
            for &d in &e[1..] {
                assert!((b'0'..=b'7').contains(&d));
            }
        }
    }
}

// ---- adversary-added hostile tests ----

// Every value 256..=511 (octal \400..\777) is out of u8 range and MUST be
// emitted literally, never truncated/masked into a byte.
#[test]
fn adv_all_out_of_range_octals_literal() {
    for v in 256u16..=511 {
        let d0 = b'0' + (v >> 6) as u8;
        let d1 = b'0' + ((v >> 3) & 7) as u8;
        let d2 = b'0' + (v & 7) as u8;
        let input = [b'\\', d0, d1, d2];
        assert_eq!(
            unescape(&input),
            input.to_vec(),
            "value {v} (\\{d0}{d1}{d2} as chars) must stay literal"
        );
    }
}

// Every in-range octal escape \000..\377 must decode to exactly that byte.
#[test]
fn adv_all_in_range_octals_decode() {
    for v in 0u16..=255 {
        let d0 = b'0' + (v >> 6) as u8;
        let d1 = b'0' + ((v >> 3) & 7) as u8;
        let d2 = b'0' + (v & 7) as u8;
        let input = [b'\\', d0, d1, d2];
        assert_eq!(unescape(&input), vec![v as u8], "\\{d0}{d1}{d2}");
    }
}

// Exhaustive: backslash followed by 0, 1, and 2 arbitrary bytes must never
// panic and must be returned verbatim (not enough room for 3 digits).
#[test]
fn adv_no_panic_short_windows_exhaustive() {
    for a in 0u16..=255 {
        let one = [b'\\', a as u8];
        assert_eq!(unescape(&one), one.to_vec());
        for b in 0u16..=255 {
            let two = [b'\\', a as u8, b as u8];
            assert_eq!(unescape(&two), two.to_vec());
        }
    }
    assert_eq!(unescape(b"\\"), b"\\".to_vec());
    assert_eq!(unescape(b""), Vec::<u8>::new());
}

// A valid escape sitting at the very end of the buffer must decode (guards
// the i+3<n boundary against being off-by-one too strict).
#[test]
fn adv_escape_at_buffer_end_decodes() {
    assert_eq!(unescape(b"\\012"), vec![0x0A]);
    assert_eq!(unescape(b"x\\012"), vec![b'x', 0x0A]);
    assert_eq!(unescape(b"xy\\377"), vec![b'x', b'y', 0xFF]);
    // exactly 4 bytes, nothing after
    assert_eq!(unescape(b"\\000"), vec![0x00]);
}

// Only 3 digits are consumed; a 4th digit is passed through.
#[test]
fn adv_extra_digit_not_consumed() {
    assert_eq!(unescape(b"\\0000"), vec![0x00, b'0']);
    assert_eq!(unescape(b"\\1234"), vec![0o123, b'4']);
    assert_eq!(unescape(b"\\3777"), vec![0xFF, b'7']);
}

// Adjacent escapes decode independently, with high/binary bytes interleaved.
#[test]
fn adv_dense_adjacent_escapes() {
    assert_eq!(
        unescape(b"\\000\\377\\134\\012"),
        vec![0x00, 0xFF, 0x5C, 0x0A]
    );
    // interleaved raw high bytes pass through untouched
    let mut input = Vec::new();
    input.extend_from_slice(b"\\033");
    input.push(0x80);
    input.push(0xFF);
    input.extend_from_slice(b"\\007");
    assert_eq!(unescape(&input), vec![0x1B, 0x80, 0xFF, 0x07]);
}

// Odd runs of backslashes: N backslashes then 3 octal digits. The parser
// greedily pairs from the left; verify it never panics and stays consistent.
#[test]
fn adv_backslash_runs_before_octal() {
    for n_bs in 0u32..8 {
        let mut input = vec![b'\\'; n_bs as usize];
        input.extend_from_slice(b"012");
        let out = unescape(&input);
        // whatever it is, re-escaping the decode then unescaping is stable
        let re = unescape(&escape(&out));
        assert_eq!(re, out, "instability for {n_bs} leading backslashes");
    }
}

// Large all-out-of-range buffer: no reallocation panic, exact literal echo.
#[test]
fn adv_large_literal_buffer() {
    let mut input = Vec::new();
    for _ in 0..1000 {
        input.extend_from_slice(b"\\777");
    }
    assert_eq!(unescape(&input), input);
}

// Deterministic pseudo-random fuzz of the full pipeline and of raw decode.
#[test]
fn adv_deterministic_fuzz() {
    // xorshift-ish deterministic generator, no external deps
    let mut state: u64 = 0x9E3779B97F4A7C15;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..2000 {
        let len = (next() % 64) as usize;
        let raw: Vec<u8> = (0..len).map(|_| (next() & 0xFF) as u8).collect();
        // full round-trip identity
        assert_eq!(unescape(&escape(&raw)), raw, "roundtrip fuzz {raw:?}");
        // raw decode of arbitrary bytes must never panic
        let arb: Vec<u8> = (0..len).map(|_| (next() & 0xFF) as u8).collect();
        let _ = unescape(&arb);
    }
}

// Escape must NEVER emit a bare backslash that isn't the start of a \NNN
// triple, otherwise decode would be ambiguous. Verify structurally.
#[test]
fn adv_escape_is_unambiguous() {
    let all: Vec<u8> = (0u16..=255).map(|b| b as u8).collect();
    let enc = escape(&all);
    let mut i = 0;
    while i < enc.len() {
        if enc[i] == b'\\' {
            // must be followed by exactly 3 octal digits within bounds
            assert!(i + 4 <= enc.len(), "dangling backslash in encoder output");
            for &d in &enc[i + 1..i + 4] {
                assert!((b'0'..=b'7').contains(&d));
            }
            i += 4;
        } else {
            assert!(enc[i] >= 0x20 && enc[i] != b'\\');
            i += 1;
        }
    }
}

// Multi-kilobyte mixed stream round-trips and decodes without panic.
#[test]
fn adv_big_mixed_roundtrip() {
    let mut raw = Vec::new();
    for b in 0u16..=255 {
        for _ in 0..16 {
            raw.push(b as u8);
        }
    }
    assert_eq!(unescape(&escape(&raw)), raw);
}
