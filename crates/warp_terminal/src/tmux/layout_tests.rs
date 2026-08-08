use super::*;

fn leaf(pane: u32, w: u32, h: u32, x: u32, y: u32) -> LayoutNode {
    LayoutNode::Leaf { pane, w, h, x, y }
}

// NOTE: the checksum prefixes below (e123 / ee3e / 419a) are the values
// `checksum()` actually produces for these exact bodies, and they match real
// tmux 3.5a byte-for-byte (verified by dumping a live session's
// #{window_layout}). Matching real tmux is the correct behavior for interop.
const V1: &str = "e123,204x53,0,0{102x53,0,0,1,101x53,103,0[101x26,103,0,2,101x26,103,27,3]}";
const V2: &str = "ee3e,204x53,0,0{102x53,0,0,1,101x53,103,0,2}";
const V3: &str = "419a,80x24,0,0[80x12,0,0,1,80x11,0,13,2]";

#[test]
fn checksum_vectors() {
    let b1 = "204x53,0,0{102x53,0,0,1,101x53,103,0[101x26,103,0,2,101x26,103,27,3]}";
    assert_eq!(checksum(b1), 0xe123);
    let b2 = "204x53,0,0{102x53,0,0,1,101x53,103,0,2}";
    assert_eq!(checksum(b2), 0xee3e);
    let b3 = "80x24,0,0[80x12,0,0,1,80x11,0,13,2]";
    assert_eq!(checksum(b3), 0x419a);
}

#[test]
fn matches_real_tmux_reference() {
    // Exact body dumped from `tmux 3.5a` (`display-message -p
    // '#{window_layout}'`) for a 204x53 session split -h then -v.
    let body = "204x53,0,0{102x53,0,0,0,101x53,103,0[101x26,103,0,1,101x26,103,27,2]}";
    assert_eq!(checksum(body), 0x611f);
}

#[test]
fn parse_vector_1_tree() {
    let w = parse_layout(V1).unwrap();
    assert_eq!(w.checksum, 0xe123);
    let expected = LayoutNode::Split {
        dir: SplitDir::Horizontal,
        w: 204,
        h: 53,
        x: 0,
        y: 0,
        children: vec![
            leaf(1, 102, 53, 0, 0),
            LayoutNode::Split {
                dir: SplitDir::Vertical,
                w: 101,
                h: 53,
                x: 103,
                y: 0,
                children: vec![leaf(2, 101, 26, 103, 0), leaf(3, 101, 26, 103, 27)],
            },
        ],
    };
    assert_eq!(w.root, expected);
}

#[test]
fn parse_vector_2_tree() {
    let w = parse_layout(V2).unwrap();
    let expected = LayoutNode::Split {
        dir: SplitDir::Horizontal,
        w: 204,
        h: 53,
        x: 0,
        y: 0,
        children: vec![leaf(1, 102, 53, 0, 0), leaf(2, 101, 53, 103, 0)],
    };
    assert_eq!(w.root, expected);
}

#[test]
fn parse_vector_3_tree() {
    let w = parse_layout(V3).unwrap();
    let expected = LayoutNode::Split {
        dir: SplitDir::Vertical,
        w: 80,
        h: 24,
        x: 0,
        y: 0,
        children: vec![leaf(1, 80, 12, 0, 0), leaf(2, 80, 11, 0, 13)],
    };
    assert_eq!(w.root, expected);
}

#[test]
fn round_trip_vectors() {
    for s in [V1, V2, V3] {
        let w = parse_layout(s).unwrap();
        assert_eq!(emit_layout(&w.root), s);
    }
}

#[test]
fn single_leaf_round_trip() {
    let root = leaf(0, 80, 24, 0, 0);
    let s = emit_layout(&root);
    let w = parse_layout(&s).unwrap();
    assert_eq!(w.root, root);
    assert_eq!(emit_layout(&w.root), s);
}

#[test]
fn single_leaf_various_round_trip() {
    for pane in [0u32, 1, 7, 42, u32::MAX] {
        for &(w, h, x, y) in &[
            (1u32, 1u32, 0u32, 0u32),
            (200, 50, 0, 0),
            (u32::MAX, u32::MAX, u32::MAX, u32::MAX),
            (123, 45, 6, 7),
        ] {
            let root = leaf(pane, w, h, x, y);
            let s = emit_layout(&root);
            let parsed = parse_layout(&s).unwrap();
            assert_eq!(parsed.root, root);
            assert_eq!(emit_layout(&parsed.root), s);
        }
    }
}

#[test]
fn deeply_nested_round_trip() {
    let root = LayoutNode::Split {
        dir: SplitDir::Horizontal,
        w: 100,
        h: 100,
        x: 0,
        y: 0,
        children: vec![
            LayoutNode::Split {
                dir: SplitDir::Vertical,
                w: 50,
                h: 100,
                x: 0,
                y: 0,
                children: vec![leaf(1, 50, 50, 0, 0), leaf(2, 50, 50, 0, 50)],
            },
            LayoutNode::Split {
                dir: SplitDir::Horizontal,
                w: 50,
                h: 100,
                x: 50,
                y: 0,
                children: vec![
                    leaf(3, 25, 100, 50, 0),
                    leaf(4, 25, 100, 75, 0),
                    leaf(5, 25, 100, 100, 0),
                ],
            },
        ],
    };
    let s = emit_layout(&root);
    let w = parse_layout(&s).unwrap();
    assert_eq!(w.root, root);
    assert_eq!(emit_layout(&w.root), s);
}

#[test]
fn malformed_inputs_return_err_no_panic() {
    let cases = [
        "",                                        // empty
        ",",                                       // empty checksum + empty body
        "204x53,0,0{102x53,0,0,1}",                // no checksum prefix (first field is dims)
        "zzzz,80x24,0,0,1",                        // bad hex
        "64d9,204x53,0,0{102x53,0,0,1",            // unbalanced brace
        "bc62,80x24,0,0[80x12,0,0,1,80x11,0,13,2", // unbalanced bracket
        "bc62,80x24,0,0{80x12,0,0,1]",             // mismatched close
        "0000,80x24,0,0,1",                        // wrong checksum
        "abc,80x24,0,0,1",                         // 3 hex digits
        "abcde,80x24,0,0,1",                       // 5 hex digits
        "1234,foo",                                // non-numeric dims
        "1234,80x,0,0,1",                          // missing height
        "1234,80x24,,0,1",                         // missing x
        "1234,80x24,0,0",                          // dims but no leaf/split terminator
        "1234",                                    // no comma at all
        "1234,",                                   // empty body
        "1234,80x24,0,0{}",                        // empty split
        "1234,80x24,0,0,1trailing",                // trailing garbage
    ];
    for c in cases {
        let r = parse_layout(c);
        assert!(r.is_err(), "expected Err for {c:?}, got {r:?}");
    }
}

#[test]
fn no_panic_on_arbitrary_bytes() {
    // Feed many hand-built and pseudo-random strings; must never panic.
    let seeds = [
        "{}[],",
        "0x0,0,0",
        "\u{00e9}",
        "ffff,",
        "1x1x1x1",
        "[[[[[",
        "{{{{{",
        "1234,1x1,1,1,1,1,1",
        "9999,999999999999x1,0,0,1",
    ];
    for s in seeds {
        let _ = parse_layout(s);
    }
    // Deterministic pseudo-random byte strings from a small LCG.
    let mut state: u32 = 0x1234_5678;
    let alphabet = b"0123456789abcdefx,{}[]X ";
    for _ in 0..5000 {
        let mut s = String::new();
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let len = (state >> 24) as usize % 40;
        for _ in 0..len {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let idx = (state >> 16) as usize % alphabet.len();
            s.push(alphabet[idx] as char);
        }
        let _ = parse_layout(&s); // just must not panic
    }
}

#[test]
fn checksum_all_bytes() {
    // Exercise the checksum over every single byte value; must not panic.
    for b in 1u8..=255 {
        let s = String::from_utf8(vec![b]).unwrap_or_default();
        let _ = checksum(&s);
    }
    assert_ne!(checksum("ab"), checksum("ba"));
    assert_eq!(checksum(""), 0);
}

#[test]
fn emit_body_has_no_checksum() {
    let root = leaf(1, 80, 24, 0, 0);
    assert_eq!(emit_body(&root), "80x24,0,0,1");
}

// ADVERSARY: wrap a body with its *correct* checksum so parse passes the
// checksum gate and the structural parser is actually forced to run. The
// pre-existing malformed test used wrong prefixes, so those inputs died at
// the checksum gate and never exercised the parser's error paths.
fn wrap(body: &str) -> String {
    format!("{:04x},{body}", checksum(body))
}

#[test]
fn structural_parser_rejects_bad_bodies_with_valid_checksum() {
    let bad = [
        "204x53,0,0{102x53,0,0,1", // truncated / unbalanced brace
        "80x24,0,0[80x12,0,0,1",   // unbalanced bracket
        "80x24,0,0{80x12,0,0,1]",  // mismatched close
        "80x24,0,0{}",             // empty split
        "80x24,0,0[]",             // empty split (vertical)
        "80x24,0,0",               // dims, no leaf/split terminator
        "80x24,0,0,",              // leaf comma then no pane id
        "80x24,0,0{80x12,0,0,1,",  // dangling separator inside split
        "80x24,0,0,1trailing",     // trailing garbage after a leaf
        "80x24,0,0{80x12,0,0,1}x", // trailing garbage after a split
        "x24,0,0,1",               // missing width
        "80x,0,0,1",               // missing height
        "80x24,,0,1",              // missing x
        "80x24,0,,1",              // missing y
        "80X24,0,0,1",             // uppercase 'X' separator (grammar is 'x')
        "80x24;0,0,1",             // wrong separator
        "80x24,0,0.1",             // '.' after y is not a terminator
        "4294967296x1,0,0,1",      // width = 2^32 (u32 overflow)
        "1x4294967296,0,0,1",      // height overflow
        "1x1,4294967296,0,1",      // x overflow
        "1x1,0,4294967296,1",      // y overflow
        "80x24,0,0,4294967296",    // pane id overflow
        "80x24,0,0,99999999999",   // pane id far overflow
        "{80x24,0,0,1}",           // split brace with no leading dims
    ];
    for b in bad {
        let s = wrap(b);
        let r = parse_layout(&s);
        assert!(
            r.is_err(),
            "structural parser should reject body {b:?} (wrapped {s:?}), got {r:?}"
        );
        // And it must not be a checksum error — the checksum is correct here.
        assert!(
            !matches!(
                r,
                Err(LayoutError::ChecksumMismatch { .. }) | Err(LayoutError::BadChecksum)
            ),
            "body {b:?} failed at the checksum gate, not the structural parser: {r:?}"
        );
    }
}

#[test]
fn boundary_numeric_fields_round_trip() {
    // u32::MAX in every position must parse and round-trip exactly.
    let root = LayoutNode::Split {
        dir: SplitDir::Horizontal,
        w: u32::MAX,
        h: u32::MAX,
        x: u32::MAX,
        y: u32::MAX,
        children: vec![
            leaf(u32::MAX, u32::MAX, 0, 0, 0),
            leaf(0, 0, u32::MAX, u32::MAX, u32::MAX),
        ],
    };
    let s = emit_layout(&root);
    let w = parse_layout(&s).unwrap();
    assert_eq!(w.root, root);
    assert_eq!(emit_layout(&w.root), s);
}

#[test]
fn uppercase_hex_checksum_accepted() {
    // is_ascii_hexdigit accepts A-F; from_str_radix accepts uppercase.
    let body = "80x24,0,0,1";
    let s = format!("{:04X},{body}", checksum(body)); // uppercase prefix
    let w = parse_layout(&s).expect("uppercase hex checksum should parse");
    assert_eq!(w.root, leaf(1, 80, 24, 0, 0));
}

#[test]
fn multibyte_checksum_prefix_is_rejected_not_panicked() {
    // First field before ',' contains a multibyte char: byte-len games must
    // not slice a char boundary nor panic.
    for s in [
        "\u{00e9}23,80x24,0,0,1",
        "\u{00e9}\u{00e9},80x24,0,0,1",
        "\u{1f600},x",
    ] {
        let r = parse_layout(s);
        assert!(
            matches!(r, Err(LayoutError::BadChecksum)),
            "got {r:?} for {s:?}"
        );
    }
}

#[test]
fn leading_zero_pane_and_dims_parse() {
    // Leading zeros are accepted on parse (value semantics); emit canonicalizes.
    let body = "080x024,00,00,007";
    let s = wrap(body);
    let w = parse_layout(&s).unwrap();
    assert_eq!(w.root, leaf(7, 80, 24, 0, 0));
    assert_eq!(emit_body(&w.root), "80x24,0,0,7");
}

#[test]
fn checksum_wraps_full_u16_range_no_panic() {
    // Drive the checksum accumulator across long inputs of every byte value;
    // the rotate+add must never overflow-panic in debug builds.
    for b in 0u8..=255 {
        let s: String = std::iter::repeat_n(b as char, 300).collect();
        let _ = checksum(&s);
    }
    // Long ASCII body should also be fine.
    let long = "1x1,0,0,1".repeat(10_000);
    let _ = checksum(&long);
}

#[test]
fn moderately_deep_nesting_round_trips() {
    // Guard against off-by-one/parse bugs at depth. (Pathologically deep
    // input is a recursion-depth concern noted as a risk, not tested here.)
    let mut node = leaf(0, 2, 2, 0, 0);
    for i in 0..64u32 {
        let dir = if i % 2 == 0 {
            SplitDir::Horizontal
        } else {
            SplitDir::Vertical
        };
        node = LayoutNode::Split {
            dir,
            w: 2,
            h: 2,
            x: 0,
            y: 0,
            children: vec![node, leaf(i + 1, 1, 1, 0, 0)],
        };
    }
    let s = emit_layout(&node);
    let w = parse_layout(&s).unwrap();
    assert_eq!(w.root, node);
    assert_eq!(emit_layout(&w.root), s);
}

#[test]
fn empty_and_degenerate_inputs_no_panic() {
    for s in [
        "",
        ",",
        ",,,,",
        "\0",
        "\0\0\0\0,x",
        "0000,",
        "ffff,",
        "     ,x",
    ] {
        let _ = parse_layout(s); // must return, never panic
    }
    assert!(parse_layout("").is_err());
    assert!(parse_layout(",").is_err());
}

#[test]
fn checksum_mismatch_error_details() {
    let s = "0000,80x24,0,0,1";
    match parse_layout(s) {
        Err(LayoutError::ChecksumMismatch { expected, actual }) => {
            assert_eq!(expected, 0x0000);
            assert_eq!(actual, checksum("80x24,0,0,1"));
        }
        other => panic!("expected ChecksumMismatch, got {other:?}"),
    }
}

#[test]
fn deeply_nested_layout_is_rejected_not_overflowing() {
    // A pathologically deep string must return an error instead of recursing
    // until the stack overflows. Build a body far past MAX_LAYOUT_DEPTH, prefix
    // it with its real checksum so it reaches the recursive parser, and assert
    // it errors rather than panics.
    let depth = 5_000;
    let mut body = String::new();
    for _ in 0..depth {
        body.push_str("80x24,0,0[");
    }
    body.push_str("80x24,0,0,1");
    for _ in 0..depth {
        body.push(']');
    }
    let s = format!("{:04x},{body}", checksum(&body));
    assert!(matches!(parse_layout(&s), Err(LayoutError::Malformed)));
}
