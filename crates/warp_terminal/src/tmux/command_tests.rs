use super::*;

fn ends_with_single_newline(buf: &[u8]) -> bool {
    buf.last() == Some(&b'\n') && buf.iter().filter(|&&b| b == b'\n').count() == 1
}

fn well_formed(buf: &[u8]) {
    assert!(!buf.is_empty());
    assert!(ends_with_single_newline(buf));
    assert_ne!(buf, b"\n");
}

#[test]
fn write_command_basic() {
    assert_eq!(Command::write_command("foo").unwrap(), b"foo\n");
}

#[test]
fn write_command_trims() {
    assert_eq!(Command::write_command("  foo  ").unwrap(), b"foo\n");
    assert_eq!(Command::write_command("\tfoo\n").unwrap(), b"foo\n");
}

#[test]
fn write_command_rejects_empty() {
    assert_eq!(Command::write_command(""), Err(CommandError));
    assert_eq!(Command::write_command("   "), Err(CommandError));
    assert_eq!(Command::write_command("\n"), Err(CommandError));
    assert_eq!(Command::write_command("\t \r\n"), Err(CommandError));
}

#[test]
fn write_command_rejects_embedded_control() {
    // An embedded newline/CR would smuggle a second command past the
    // one-command-per-line framing; other control bytes are refused too.
    assert_eq!(
        Command::write_command("kill-server\ndetach"),
        Err(CommandError)
    );
    assert_eq!(Command::write_command("a\rb"), Err(CommandError));
    assert_eq!(Command::write_command("a\tb"), Err(CommandError));
    // Surrounding whitespace is still trimmed rather than rejected.
    assert_eq!(Command::write_command("\tfoo\n").unwrap(), b"foo\n");
}

#[test]
fn send_keys_hex_rejects_empty() {
    assert_eq!(Command::send_keys_hex(0, &[]), Err(CommandError));
    assert_eq!(Command::send_keys_hex(42, b""), Err(CommandError));
}

#[test]
fn send_keys_hex_special_bytes() {
    // 0x00, 0x0a (newline), 0x3b (';'), 0xff
    let out = Command::send_keys_hex(3, &[0x00, 0x0a, 0x3b, 0xff]).unwrap();
    assert_eq!(out, b"send-keys -t %3 -H 00 0a 3b ff\n".to_vec());
    well_formed(&out);
    // The payload (everything before the trailing newline) must contain
    // no raw newline and no raw ';'.
    let payload = &out[..out.len() - 1];
    assert!(!payload.contains(&b'\n'));
    assert!(!payload.contains(&b';'));
}

#[test]
fn send_keys_hex_all_bytes_lowercase_two_digits() {
    let all: Vec<u8> = (0u16..=255).map(|b| b as u8).collect();
    let out = Command::send_keys_hex(7, &all).unwrap();
    well_formed(&out);
    let payload = &out[..out.len() - 1];
    // No injection bytes survive.
    assert!(!payload.contains(&b';'));
    // Exactly one newline total (the terminator).
    assert_eq!(out.iter().filter(|&&b| b == b'\n').count(), 1);

    let text = std::str::from_utf8(&out).unwrap();
    let prefix = "send-keys -t %7 -H";
    assert!(text.starts_with(prefix));
    let hex_part = text[prefix.len()..].trim_end_matches('\n');
    let tokens: Vec<&str> = hex_part.split_whitespace().collect();
    assert_eq!(tokens.len(), 256);
    for (i, tok) in tokens.iter().enumerate() {
        assert_eq!(tok.len(), 2);
        assert_eq!(*tok, format!("{i:02x}").as_str());
        assert!(
            tok.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }
}

#[test]
fn send_keys_hex_single_byte_pane_edges() {
    assert_eq!(
        Command::send_keys_hex(0, &[0x41]).unwrap(),
        b"send-keys -t %0 -H 41\n"
    );
    assert_eq!(
        Command::send_keys_hex(u32::MAX, &[0xde, 0xad]).unwrap(),
        format!("send-keys -t %{} -H de ad\n", u32::MAX).into_bytes()
    );
}

#[test]
fn resize_pane_format() {
    assert_eq!(
        Command::resize_pane(1, 80, 24),
        b"resize-pane -t %1 -x 80 -y 24\n"
    );
}

#[test]
fn split_window_format() {
    assert_eq!(Command::split_window(2, true), b"split-window -t %2 -h\n");
    assert_eq!(Command::split_window(2, false), b"split-window -t %2 -v\n");
}

#[test]
fn kill_pane_format() {
    assert_eq!(Command::kill_pane(9), b"kill-pane -t %9\n");
}

#[test]
fn new_window_format() {
    assert_eq!(Command::new_window(), b"new-window\n");
}

#[test]
fn kill_window_format() {
    assert_eq!(Command::kill_window(4), b"kill-window -t @4\n");
}

#[test]
fn select_layout_format() {
    assert_eq!(
        Command::select_layout(5, "main-vertical"),
        b"select-layout -t @5 main-vertical\n"
    );
}

#[test]
fn refresh_client_size_format() {
    assert_eq!(
        Command::refresh_client_size(120, 40),
        b"refresh-client -C 120,40\n"
    );
}

#[test]
fn refresh_client_pause_after_format() {
    assert_eq!(
        Command::refresh_client_pause_after(3),
        b"refresh-client -f pause-after=3\n"
    );
}

#[test]
fn refresh_client_continue_format() {
    assert_eq!(
        Command::refresh_client_continue(6),
        b"refresh-client -A '%6:continue'\n"
    );
}

#[test]
fn capture_pane_format() {
    assert_eq!(
        Command::capture_pane(8, None),
        b"capture-pane -p -e -J -t %8\n"
    );
    assert_eq!(
        Command::capture_pane(8, Some(-100)),
        b"capture-pane -p -e -J -t %8 -S -100\n"
    );
    assert_eq!(
        Command::capture_pane(8, Some(0)),
        b"capture-pane -p -e -J -t %8 -S 0\n"
    );
}

#[test]
fn list_windows_format() {
    assert_eq!(
        Command::list_windows(),
        b"list-windows -F '#{window_layout}'\n"
    );
}

#[test]
fn detach_client_format() {
    assert_eq!(Command::detach_client(), b"detach-client\n");
}

#[test]
fn property_all_helpers_well_formed() {
    // Representative args across every helper.
    let panes = [0u32, 1, 42, u32::MAX];
    let dims = [0u32, 1, 80, 24, u32::MAX];

    for &p in &panes {
        well_formed(&Command::send_keys_hex(p, &[0u8]).unwrap());
        well_formed(&Command::send_keys_hex(p, &[0x0a, 0x3b, 0xff]).unwrap());
        well_formed(&Command::kill_pane(p));
        well_formed(&Command::kill_window(p));
        well_formed(&Command::refresh_client_continue(p));
        well_formed(&Command::split_window(p, true));
        well_formed(&Command::split_window(p, false));
        well_formed(&Command::select_layout(p, "even-horizontal"));
        well_formed(&Command::capture_pane(p, None));
        for s in [i64::MIN, -1, 0, 1, i64::MAX] {
            well_formed(&Command::capture_pane(p, Some(s)));
        }
    }
    for &w in &dims {
        for &h in &dims {
            well_formed(&Command::resize_pane(0, w, h));
            well_formed(&Command::refresh_client_size(w, h));
        }
        well_formed(&Command::refresh_client_pause_after(w));
    }
    well_formed(&Command::new_window());
    well_formed(&Command::list_windows());
    well_formed(&Command::detach_client());
}

#[test]
fn property_send_keys_no_injection_bytes() {
    let patterns: Vec<Vec<u8>> = vec![
        vec![0x0a],
        vec![0x3b],
        vec![0x0a, 0x0a, 0x0a],
        vec![0x3b, 0x3b],
        (0u16..=255).map(|b| b as u8).collect(),
        b"detach-client\n".to_vec(),
        b"; kill-server\n".to_vec(),
    ];
    for pat in &patterns {
        let out = Command::send_keys_hex(1, pat).unwrap();
        well_formed(&out);
        let payload = &out[..out.len() - 1];
        assert!(!payload.contains(&b'\n'));
        assert!(!payload.contains(&b';'));
    }
}

mod adversarial {
    use super::super::*;

    fn ends_with_single_newline(buf: &[u8]) -> bool {
        buf.last() == Some(&b'\n') && buf.iter().filter(|&&b| b == b'\n').count() == 1
    }

    fn well_formed(buf: &[u8]) {
        assert!(!buf.is_empty(), "buffer must be non-empty");
        assert!(
            ends_with_single_newline(buf),
            "must end in exactly one newline: {buf:?}"
        );
        assert_ne!(
            buf, b"\n",
            "must never be a bare newline (detaches session)"
        );
    }

    // Reconstruct the byte sequence from a `send-keys ... -H hh hh ...` line and
    // assert it round-trips exactly, for arbitrary (incl. non-UTF-8) input.
    fn parse_send_keys(out: &[u8], pane: u32) -> Vec<u8> {
        let text = std::str::from_utf8(out).expect("send-keys output is ASCII");
        let prefix = format!("send-keys -t %{pane} -H");
        assert!(text.starts_with(&prefix), "wrong prefix: {text}");
        let hex = text[prefix.len()..].trim_end_matches('\n');
        hex.split_whitespace()
            .map(|tok| {
                assert_eq!(tok.len(), 2, "each token is 2 hex digits: {tok}");
                assert!(
                    tok.chars()
                        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                    "lowercase hex only: {tok}"
                );
                u8::from_str_radix(tok, 16).unwrap()
            })
            .collect()
    }

    // ---- send_keys_hex: exhaustive round-trip over every single byte ----
    #[test]
    fn send_keys_hex_every_single_byte_roundtrips() {
        for b in 0u16..=255 {
            let byte = b as u8;
            let out = Command::send_keys_hex(11, &[byte]).unwrap();
            well_formed(&out);
            let payload = &out[..out.len() - 1];
            // No injection byte ever appears literally in the payload.
            assert!(!payload.contains(&b'\n'));
            assert!(!payload.contains(&b';'));
            // Round-trips exactly.
            assert_eq!(parse_send_keys(&out, 11), vec![byte]);
        }
    }

    // ---- send_keys_hex: non-UTF-8 / control-heavy sequences round-trip ----
    #[test]
    fn send_keys_hex_non_utf8_roundtrips() {
        let patterns: Vec<Vec<u8>> = vec![
            vec![0xff, 0xfe, 0xfd, 0xc0, 0x80], // invalid UTF-8
            vec![0x80, 0x81, 0x82],             // lone continuation bytes
            vec![0x1b, 0x5b, 0x41],             // ESC [ A
            vec![0x0d, 0x0a],                   // CRLF
            vec![0x00; 64],                     // many NULs
            vec![0x3b; 64],                     // many semicolons
            vec![0x0a; 64],                     // many newlines
            (0u16..=255).rev().map(|b| b as u8).collect(),
        ];
        for pat in &patterns {
            let out = Command::send_keys_hex(0, pat).unwrap();
            well_formed(&out);
            let payload = &out[..out.len() - 1];
            assert!(!payload.contains(&b'\n'));
            assert!(!payload.contains(&b';'));
            assert_eq!(&parse_send_keys(&out, 0), pat);
        }
    }

    // A payload crafted to look like a full detach command must be neutralized.
    #[test]
    fn send_keys_hex_neutralizes_embedded_commands() {
        let evil = b"\ndetach-client\nkill-server\n; rm -rf /\n";
        let out = Command::send_keys_hex(5, evil).unwrap();
        well_formed(&out);
        let payload = &out[..out.len() - 1];
        assert!(!payload.contains(&b'\n'));
        assert!(!payload.contains(&b';'));
        // Not a single literal substring of the dangerous words survives as bytes:
        // everything is hex, so "detach" never appears contiguously.
        assert!(!payload.windows(6).any(|w| w == b"detach"));
        assert_eq!(&parse_send_keys(&out, 5), evil);
    }

    // ---- pane / dimension boundary values ----
    #[test]
    fn numeric_boundaries_exact() {
        assert_eq!(
            Command::send_keys_hex(u32::MAX, &[0x00]).unwrap(),
            format!("send-keys -t %{} -H 00\n", u32::MAX).into_bytes()
        );
        assert_eq!(
            Command::resize_pane(0, 0, 0),
            b"resize-pane -t %0 -x 0 -y 0\n"
        );
        assert_eq!(
            Command::resize_pane(u32::MAX, u32::MAX, u32::MAX),
            format!("resize-pane -t %{m} -x {m} -y {m}\n", m = u32::MAX).into_bytes()
        );
        assert_eq!(
            Command::refresh_client_pause_after(u32::MAX),
            format!("refresh-client -f pause-after={}\n", u32::MAX).into_bytes()
        );
    }

    // ---- capture_pane: i64 extremes format exactly and stay well-formed ----
    #[test]
    fn capture_pane_i64_extremes() {
        assert_eq!(
            Command::capture_pane(0, Some(i64::MIN)),
            format!("capture-pane -p -e -J -t %0 -S {}\n", i64::MIN).into_bytes()
        );
        assert_eq!(
            Command::capture_pane(0, Some(i64::MAX)),
            format!("capture-pane -p -e -J -t %0 -S {}\n", i64::MAX).into_bytes()
        );
        well_formed(&Command::capture_pane(u32::MAX, Some(i64::MIN)));
        well_formed(&Command::capture_pane(u32::MAX, Some(i64::MAX)));
    }

    // ---- write_command: unicode-whitespace-only strings are rejected ----
    #[test]
    fn write_command_rejects_unicode_whitespace() {
        // NBSP, ideographic space, various unicode spaces, vertical tab, FF.
        for s in [
            "\u{00a0}",
            "\u{3000}",
            "\u{2028}",
            "\u{2029}",
            "\u{0b}\u{0c}",
            "\u{2003}\u{2009}",
        ] {
            assert_eq!(
                Command::write_command(s),
                Err(CommandError),
                "should reject {s:?}"
            );
        }
    }

    // A non-whitespace command survives trimming and terminates in one newline.
    #[test]
    fn write_command_preserves_internal_content() {
        // Internal spaces preserved; only ends trimmed.
        assert_eq!(Command::write_command("  a b c  ").unwrap(), b"a b c\n");
        // Single non-whitespace char is valid.
        assert_eq!(Command::write_command("x").unwrap(), b"x\n");
        // Content that already looks like a command still gets exactly one \n.
        assert_eq!(
            Command::write_command("detach-client\n\n\n").unwrap(),
            b"detach-client\n"
        );
    }

    // ---- exhaustive well-formedness sweep, dense arg grid ----
    #[test]
    fn dense_property_sweep_well_formed() {
        let panes = [0u32, 1, 2, 7, 255, 65535, u32::MAX - 1, u32::MAX];
        for &p in &panes {
            for len in [1usize, 2, 3, 17, 256] {
                let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + p as usize) as u8).collect();
                let out = Command::send_keys_hex(p, &bytes).unwrap();
                well_formed(&out);
                assert_eq!(parse_send_keys(&out, p), bytes);
            }
            well_formed(&Command::kill_pane(p));
            well_formed(&Command::kill_window(p));
            well_formed(&Command::refresh_client_continue(p));
            well_formed(&Command::split_window(p, true));
            well_formed(&Command::split_window(p, false));
            well_formed(&Command::select_layout(p, "tiled"));
            well_formed(&Command::capture_pane(p, None));
            for s in [i64::MIN, i64::MIN + 1, -1, 0, 1, i64::MAX - 1, i64::MAX] {
                well_formed(&Command::capture_pane(p, Some(s)));
            }
            well_formed(&Command::resize_pane(p, p, p));
            well_formed(&Command::refresh_client_size(p, p));
            well_formed(&Command::refresh_client_pause_after(p));
        }
        well_formed(&Command::new_window());
        well_formed(&Command::list_windows());
        well_formed(&Command::detach_client());
    }

    // ---- helpers use @ for windows and % for panes, never confused ----
    #[test]
    fn window_vs_pane_sigils() {
        assert!(Command::kill_window(3).starts_with(b"kill-window -t @3"));
        assert!(Command::select_layout(3, "even-vertical").starts_with(b"select-layout -t @3 "));
        assert!(Command::kill_pane(3).starts_with(b"kill-pane -t %3"));
        assert!(
            Command::send_keys_hex(3, &[0])
                .unwrap()
                .starts_with(b"send-keys -t %3 ")
        );
    }

    // ---- CommandError is a proper error value ----
    #[test]
    fn command_error_traits() {
        let e = CommandError;
        assert_eq!(e, CommandError);
        assert!(!format!("{e}").is_empty());
        let _: &dyn std::error::Error = &e;
    }
}
