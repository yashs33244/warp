use super::*;

fn out(line: &[u8]) -> TmuxEvent {
    parse_line(line)
}

#[test]
fn begin_end_error_frames() {
    assert_eq!(
        out(b"%begin 1710000000 7 0"),
        TmuxEvent::Begin {
            time: 1710000000,
            num: 7,
            flags: 0
        }
    );
    assert_eq!(
        out(b"%end 1710000001 7 1"),
        TmuxEvent::End {
            time: 1710000001,
            num: 7,
            flags: 1
        }
    );
    assert_eq!(
        out(b"%error 1710000002 8 0"),
        TmuxEvent::Error {
            time: 1710000002,
            num: 8,
            flags: 0
        }
    );
}

#[test]
fn begin_negative_time_and_flags() {
    assert_eq!(
        out(b"%begin -5 12 -3"),
        TmuxEvent::Begin {
            time: -5,
            num: 12,
            flags: -3
        }
    );
}

#[test]
fn output_plain() {
    assert_eq!(
        out(b"%output %1 hello world"),
        TmuxEvent::Output {
            pane: 1,
            data: b"hello world".to_vec()
        }
    );
}

#[test]
fn output_escaped_osc() {
    // \033]0;t\007 -> ESC ] 0 ; t BEL
    assert_eq!(
        out(b"%output %2 \\033]0;t\\007"),
        TmuxEvent::Output {
            pane: 2,
            data: vec![0x1b, b']', b'0', b';', b't', 0x07],
        }
    );
}

#[test]
fn output_lone_high_byte() {
    // escaped \377 -> raw 0xFF (non-UTF-8)
    assert_eq!(
        out(b"%output %5 \\377"),
        TmuxEvent::Output {
            pane: 5,
            data: vec![0xFF],
        }
    );
}

#[test]
fn output_mixed_binary() {
    // "A" + newline(\012) + backslash(\134) + 0xFF(\377) + "Z"
    assert_eq!(
        out(b"%output %10 A\\012\\134\\377Z"),
        TmuxEvent::Output {
            pane: 10,
            data: vec![b'A', 0x0A, b'\\', 0xFF, b'Z'],
        }
    );
}

#[test]
fn output_empty_data() {
    assert_eq!(
        out(b"%output %3 "),
        TmuxEvent::Output {
            pane: 3,
            data: vec![]
        }
    );
    assert_eq!(
        out(b"%output %3"),
        TmuxEvent::Output {
            pane: 3,
            data: vec![]
        }
    );
}

#[test]
fn output_dangling_backslash_passthrough() {
    // a trailing backslash with no 3 octal digits stays literal
    assert_eq!(
        out(b"%output %1 ab\\"),
        TmuxEvent::Output {
            pane: 1,
            data: vec![b'a', b'b', b'\\'],
        }
    );
    // backslash followed by non-octal
    assert_eq!(
        out(b"%output %1 \\9zz"),
        TmuxEvent::Output {
            pane: 1,
            data: vec![b'\\', b'9', b'z', b'z'],
        }
    );
}

#[test]
fn extended_output() {
    assert_eq!(
        out(b"%extended-output %4 250 : \\033[m done"),
        TmuxEvent::ExtendedOutput {
            pane: 4,
            age: 250,
            data: {
                let mut v = vec![0x1b];
                v.extend_from_slice(b"[m done");
                v
            },
        }
    );
}

#[test]
fn extended_output_empty_data() {
    assert_eq!(
        out(b"%extended-output %4 0 : "),
        TmuxEvent::ExtendedOutput {
            pane: 4,
            age: 0,
            data: vec![],
        }
    );
}

#[test]
fn extended_output_data_contains_separator() {
    // only the first " : " separates age from data
    assert_eq!(
        out(b"%extended-output %1 9 : a : b"),
        TmuxEvent::ExtendedOutput {
            pane: 1,
            age: 9,
            data: b"a : b".to_vec(),
        }
    );
}

#[test]
fn pause_continue_pane_mode() {
    assert_eq!(out(b"%pause %6"), TmuxEvent::Pause { pane: 6 });
    assert_eq!(out(b"%continue %6"), TmuxEvent::Continue { pane: 6 });
    assert_eq!(
        out(b"%pane-mode-changed %6"),
        TmuxEvent::PaneModeChanged { pane: 6 }
    );
}

#[test]
fn window_lifecycle() {
    assert_eq!(out(b"%window-add @5"), TmuxEvent::WindowAdd { window: 5 });
    assert_eq!(
        out(b"%window-close @5"),
        TmuxEvent::WindowClose { window: 5 }
    );
    assert_eq!(
        out(b"%window-renamed @2 my window"),
        TmuxEvent::WindowRenamed {
            window: 2,
            name: "my window".to_string(),
        }
    );
    assert_eq!(
        out(b"%window-pane-changed @1 %2"),
        TmuxEvent::WindowPaneChanged { window: 1, pane: 2 }
    );
}

#[test]
fn layout_change_variants() {
    assert_eq!(
        out(b"%layout-change @1 abcd,80x24,0,0,0"),
        TmuxEvent::LayoutChange {
            window: 1,
            layout: "abcd,80x24,0,0,0".to_string(),
            visible: None,
            flags: None,
        }
    );
    assert_eq!(
        out(b"%layout-change @1 layoutA layoutB *"),
        TmuxEvent::LayoutChange {
            window: 1,
            layout: "layoutA".to_string(),
            visible: Some("layoutB".to_string()),
            flags: Some("*".to_string()),
        }
    );
}

#[test]
fn session_events() {
    assert_eq!(
        out(b"%session-changed $1 main"),
        TmuxEvent::SessionChanged {
            session: 1,
            name: "main".to_string(),
        }
    );
    assert_eq!(out(b"%sessions-changed"), TmuxEvent::SessionsChanged);
    assert_eq!(
        out(b"%session-window-changed $1 @2"),
        TmuxEvent::SessionWindowChanged {
            session: 1,
            window: 2,
        }
    );
    assert_eq!(
        out(b"%session-renamed $3 newname"),
        TmuxEvent::SessionRenamed {
            session: 3,
            name: "newname".to_string(),
        }
    );
}

#[test]
fn client_events() {
    assert_eq!(
        out(b"%client-session-changed client1 $2 main"),
        TmuxEvent::ClientSessionChanged {
            client: "client1".to_string(),
            session: 2,
            name: "main".to_string(),
        }
    );
    assert_eq!(
        out(b"%client-detached client1"),
        TmuxEvent::ClientDetached {
            client: "client1".to_string(),
        }
    );
}

#[test]
fn exit_events() {
    assert_eq!(out(b"%exit"), TmuxEvent::Exit { reason: None });
    assert_eq!(out(b"%exit "), TmuxEvent::Exit { reason: None });
    assert_eq!(
        out(b"%exit server exited"),
        TmuxEvent::Exit {
            reason: Some("server exited".to_string()),
        }
    );
}

#[test]
fn multi_digit_ids() {
    assert_eq!(
        out(b"%output %123456 x"),
        TmuxEvent::Output {
            pane: 123456,
            data: b"x".to_vec(),
        }
    );
    assert_eq!(
        out(b"%window-add @4294967295"),
        TmuxEvent::WindowAdd { window: u32::MAX }
    );
    assert_eq!(
        out(b"%begin 9223372036854775807 18446744073709551615 0"),
        TmuxEvent::Begin {
            time: i64::MAX,
            num: u64::MAX,
            flags: 0,
        }
    );
}

#[test]
fn garbage_and_empty_are_other() {
    assert_eq!(out(b"garbage"), TmuxEvent::Other(b"garbage".to_vec()));
    assert_eq!(out(b""), TmuxEvent::Other(b"".to_vec()));
    assert_eq!(
        out(b"@1 not-a-keyword"),
        TmuxEvent::Other(b"@1 not-a-keyword".to_vec())
    );
}

#[test]
fn malformed_lines_are_other() {
    assert_eq!(
        out(b"%output @1 data"),
        TmuxEvent::Other(b"%output @1 data".to_vec())
    );
    assert_eq!(out(b"%output"), TmuxEvent::Other(b"%output".to_vec()));
    assert_eq!(out(b"%begin 1 2"), TmuxEvent::Other(b"%begin 1 2".to_vec()));
    assert_eq!(out(b"%pause %xx"), TmuxEvent::Other(b"%pause %xx".to_vec()));
    assert_eq!(
        out(b"%totally-unknown x"),
        TmuxEvent::Other(b"%totally-unknown x".to_vec())
    );
    assert_eq!(
        out(b"%pause %4294967296"),
        TmuxEvent::Other(b"%pause %4294967296".to_vec())
    );
    assert_eq!(
        out(b"%extended-output %1 10 data"),
        TmuxEvent::Other(b"%extended-output %1 10 data".to_vec())
    );
    assert_eq!(
        out(b"%sessions-changed x"),
        TmuxEvent::Other(b"%sessions-changed x".to_vec())
    );
    assert_eq!(
        out(b"%layout-change @1 a b c d"),
        TmuxEvent::Other(b"%layout-change @1 a b c d".to_vec())
    );
}

#[test]
fn assembly_begin_body_end() {
    let mut p = ControlParser::new();
    assert_eq!(p.feed_line(b"%begin 1000 7 0"), None);
    assert_eq!(p.feed_line(b"line one"), None);
    assert_eq!(p.feed_line(b"line two"), None);
    assert_eq!(
        p.feed_line(b"%end 1001 7 0"),
        Some(ControlMessage::Reply(CommandReply {
            num: 7,
            success: true,
            body: vec![b"line one".to_vec(), b"line two".to_vec()],
        }))
    );
}

#[test]
fn assembly_error_is_failure() {
    let mut p = ControlParser::new();
    assert_eq!(p.feed_line(b"%begin 1 3 0"), None);
    assert_eq!(p.feed_line(b"can't find pane"), None);
    assert_eq!(
        p.feed_line(b"%error 2 3 0"),
        Some(ControlMessage::Reply(CommandReply {
            num: 3,
            success: false,
            body: vec![b"can't find pane".to_vec()],
        }))
    );
}

#[test]
fn assembly_empty_body() {
    let mut p = ControlParser::new();
    assert_eq!(p.feed_line(b"%begin 5 42 0"), None);
    assert_eq!(
        p.feed_line(b"%end 6 42 0"),
        Some(ControlMessage::Reply(CommandReply {
            num: 42,
            success: true,
            body: vec![],
        }))
    );
}

#[test]
fn assembly_notifications_outside_block() {
    let mut p = ControlParser::new();
    assert_eq!(
        p.feed_line(b"%window-add @5"),
        Some(ControlMessage::Notification(TmuxEvent::WindowAdd {
            window: 5
        }))
    );
    assert_eq!(
        p.feed_line(b"garbage"),
        Some(ControlMessage::Notification(TmuxEvent::Other(
            b"garbage".to_vec()
        )))
    );
}

#[test]
fn assembly_body_lines_kept_raw_even_if_notification_like() {
    // defensive: %-lines inside a block are body, not notifications
    let mut p = ControlParser::new();
    assert_eq!(p.feed_line(b"%begin 1 9 0"), None);
    assert_eq!(p.feed_line(b"%output %1 hi"), None);
    assert_eq!(
        p.feed_line(b"%end 1 9 0"),
        Some(ControlMessage::Reply(CommandReply {
            num: 9,
            success: true,
            body: vec![b"%output %1 hi".to_vec()],
        }))
    );
}

#[test]
fn assembly_non_matching_num_end_is_body() {
    // %end with a different command number does not close the block
    let mut p = ControlParser::new();
    assert_eq!(p.feed_line(b"%begin 1 7 0"), None);
    assert_eq!(p.feed_line(b"%end 1 99 0"), None); // wrong num -> body
    assert_eq!(
        p.feed_line(b"%end 2 7 0"),
        Some(ControlMessage::Reply(CommandReply {
            num: 7,
            success: true,
            body: vec![b"%end 1 99 0".to_vec()],
        }))
    );
}

#[test]
fn assembly_sequential_blocks() {
    let mut p = ControlParser::new();
    assert_eq!(p.feed_line(b"%begin 1 1 0"), None);
    assert_eq!(
        p.feed_line(b"%end 1 1 0"),
        Some(ControlMessage::Reply(CommandReply {
            num: 1,
            success: true,
            body: vec![],
        }))
    );
    assert_eq!(
        p.feed_line(b"%sessions-changed"),
        Some(ControlMessage::Notification(TmuxEvent::SessionsChanged))
    );
    assert_eq!(p.feed_line(b"%begin 2 2 0"), None);
    assert_eq!(p.feed_line(b"body"), None);
    assert_eq!(
        p.feed_line(b"%error 2 2 0"),
        Some(ControlMessage::Reply(CommandReply {
            num: 2,
            success: false,
            body: vec![b"body".to_vec()],
        }))
    );
}

#[test]
fn unescape_roundtrips_all_bytes() {
    // every byte escapes to \nnn (control/backslash) or passes through, and back
    for b in 0u16..=255 {
        let byte = b as u8;
        let esc: Vec<u8> = if byte < 0x20 || byte == b'\\' {
            format!("\\{byte:03o}").into_bytes()
        } else {
            vec![byte]
        };
        let mut line = b"%output %1 ".to_vec();
        line.extend_from_slice(&esc);
        match parse_line(&line) {
            TmuxEvent::Output { pane, data } => {
                assert_eq!(pane, 1);
                assert_eq!(data, vec![byte], "byte {byte} did not round-trip");
            }
            e => panic!("expected Output, got {e:?}"),
        }
    }
}

#[test]
fn fuzz_never_panics() {
    for b in 0u16..=255 {
        let line = [b as u8];
        let _ = parse_line(&line);
    }
    let keywords: &[&[u8]] = &[
        b"%begin",
        b"%end",
        b"%error",
        b"%output",
        b"%extended-output",
        b"%pause",
        b"%continue",
        b"%window-add",
        b"%window-close",
        b"%window-renamed",
        b"%window-pane-changed",
        b"%layout-change",
        b"%session-changed",
        b"%sessions-changed",
        b"%session-window-changed",
        b"%session-renamed",
        b"%pane-mode-changed",
        b"%client-session-changed",
        b"%client-detached",
        b"%exit",
        b"%",
        b"%x",
        b"",
    ];
    let fillers: &[&[u8]] = &[
        b"",
        b" ",
        b"  ",
        b" %1",
        b" @1",
        b" $1",
        b" %1 ",
        b" 1 2 3",
        b" \\",
        b" \\3",
        b" \\377",
        b" : ",
        b" %1 0 : \\",
        b" \xff\xfe\x00",
        b" -1 -2 -3",
        b" %99999999999999999999",
    ];
    for kw in keywords {
        for f in fillers {
            let mut line = kw.to_vec();
            line.extend_from_slice(f);
            let _ = parse_line(&line);
            let mut p = ControlParser::new();
            let _ = p.feed_line(&line);
            let _ = p.feed_line(f);
            let _ = p.feed_line(kw);
        }
    }
    for b in 0u16..=255 {
        let byte = b as u8;
        let mut p = ControlParser::new();
        let _ = p.feed_line(b"%begin 1 1 0");
        let _ = p.feed_line(&[byte]);
        let _ = p.feed_line(b"%end 1 1 0");

        let mut line = b"%output %1 ".to_vec();
        line.push(b'\\');
        line.push(byte);
        let _ = parse_line(&line);

        let mut line2 = b"%output %1 ".to_vec();
        line2.push(byte);
        let _ = parse_line(&line2);
    }
    for tail in [
        &b"\\"[..],
        b"\\0",
        b"\\00",
        b"\\000",
        b"\\777",
        b"\\800",
        b"\\\\\\",
        b"\\37",
        b"\\\\377",
    ] {
        let mut line = b"%output %1 ".to_vec();
        line.extend_from_slice(tail);
        let _ = parse_line(&line);
    }
}

// ===========================================================================
// Hostile / adversarial tests written by an independent verifier.
// Goal: break the parser on boundaries, malformed input, non-UTF-8,
// overflow, adversarial escaping, and reply-block edge cases.
// ===========================================================================

fn ev(line: &[u8]) -> TmuxEvent {
    parse_line(line)
}
fn other(line: &[u8]) -> TmuxEvent {
    TmuxEvent::Other(line.to_vec())
}

// ---------- numeric boundaries ----------

#[test]
fn begin_i64_min_time_parses() {
    assert_eq!(
        ev(b"%begin -9223372036854775808 1 0"),
        TmuxEvent::Begin {
            time: i64::MIN,
            num: 1,
            flags: 0
        }
    );
}

#[test]
fn begin_i64_min_flags_parses() {
    assert_eq!(
        ev(b"%begin 0 1 -9223372036854775808"),
        TmuxEvent::Begin {
            time: 0,
            num: 1,
            flags: i64::MIN
        }
    );
}

#[test]
fn begin_time_overflow_is_other() {
    // i64::MAX + 1
    let l = b"%begin 9223372036854775808 1 0";
    assert_eq!(ev(l), other(l));
}

#[test]
fn begin_time_min_overflow_is_other() {
    // i64::MIN - 1
    let l = b"%begin -9223372036854775809 1 0";
    assert_eq!(ev(l), other(l));
}

#[test]
fn begin_num_u64_max_ok_and_overflow_other() {
    assert_eq!(
        ev(b"%begin 0 18446744073709551615 0"),
        TmuxEvent::Begin {
            time: 0,
            num: u64::MAX,
            flags: 0
        }
    );
    let l = b"%begin 0 18446744073709551616 0";
    assert_eq!(ev(l), other(l));
}

#[test]
fn output_pane_u32_boundary() {
    assert_eq!(
        ev(b"%output %4294967295 x"),
        TmuxEvent::Output {
            pane: u32::MAX,
            data: b"x".to_vec()
        }
    );
    let l = b"%output %4294967296 x";
    assert_eq!(ev(l), other(l));
}

#[test]
fn extended_output_age_overflow_is_other() {
    let l = b"%extended-output %1 18446744073709551616 : x";
    assert_eq!(ev(l), other(l));
}

#[test]
fn extended_output_pane_overflow_is_other() {
    let l = b"%extended-output %4294967296 0 : x";
    assert_eq!(ev(l), other(l));
}

#[test]
fn many_leading_zeros_in_id() {
    assert_eq!(
        ev(b"%output %000007 x"),
        TmuxEvent::Output {
            pane: 7,
            data: b"x".to_vec()
        }
    );
    // huge run of zeros then value fitting in u32 still parses (no overflow)
    let mut l = b"%window-add @".to_vec();
    l.extend(std::iter::repeat_n(b'0', 40));
    l.extend_from_slice(b"5");
    assert_eq!(parse_line(&l), TmuxEvent::WindowAdd { window: 5 });
}

// ---------- single-space / whitespace semantics ----------

#[test]
fn output_preserves_leading_space_in_data() {
    // only ONE space after the pane token is the separator; the rest is data
    assert_eq!(
        ev(b"%output %1  hi"),
        TmuxEvent::Output {
            pane: 1,
            data: b" hi".to_vec()
        }
    );
}

#[test]
fn output_data_is_only_spaces() {
    assert_eq!(
        ev(b"%output %1    "),
        TmuxEvent::Output {
            pane: 1,
            data: b"   ".to_vec()
        }
    );
}

#[test]
fn begin_double_space_is_other() {
    let l = b"%begin  1 2 3";
    assert_eq!(ev(l), other(l));
}

#[test]
fn extended_output_double_space_before_age_is_other() {
    // "%1  250 : x" -> age token is " 250" (leading space) -> not numeric
    let l = b"%extended-output %1  250 : x";
    assert_eq!(ev(l), other(l));
}

// ---------- adversarial escaping ----------

#[test]
fn escaped_backslash_then_octal() {
    // bytes: backslash, backslash, '3','7','7'
    // first '\' has non-octal next byte -> literal '\'; then \377 -> 0xFF
    assert_eq!(
        ev(b"%output %1 \\\\377"),
        TmuxEvent::Output {
            pane: 1,
            data: vec![b'\\', 0xFF],
        }
    );
}

#[test]
fn octal_out_of_byte_range_passes_through() {
    // \400 = 256 > 255 -> not decoded, emitted literally
    assert_eq!(
        ev(b"%output %1 \\400"),
        TmuxEvent::Output {
            pane: 1,
            data: b"\\400".to_vec(),
        }
    );
    // \777 = 511 -> literal
    assert_eq!(
        ev(b"%output %1 \\777"),
        TmuxEvent::Output {
            pane: 1,
            data: b"\\777".to_vec(),
        }
    );
}

#[test]
fn truncated_octal_at_end_is_literal() {
    assert_eq!(
        ev(b"%output %1 \\37"),
        TmuxEvent::Output {
            pane: 1,
            data: b"\\37".to_vec(),
        }
    );
    assert_eq!(
        ev(b"%output %1 \\0"),
        TmuxEvent::Output {
            pane: 1,
            data: b"\\0".to_vec(),
        }
    );
    assert_eq!(
        ev(b"%output %1 \\"),
        TmuxEvent::Output {
            pane: 1,
            data: b"\\".to_vec(),
        }
    );
}

#[test]
fn consecutive_nul_escapes() {
    assert_eq!(
        ev(b"%output %1 \\000\\000\\000"),
        TmuxEvent::Output {
            pane: 1,
            data: vec![0, 0, 0],
        }
    );
}

#[test]
fn escape_immediately_followed_by_more_escape() {
    // \033\033 -> ESC ESC
    assert_eq!(
        ev(b"%output %1 \\033\\033"),
        TmuxEvent::Output {
            pane: 1,
            data: vec![0x1b, 0x1b],
        }
    );
}

#[test]
fn raw_high_bytes_passthrough_unescaped() {
    // a raw 0xFF that was NOT escaped (defensive) still survives
    let mut line = b"%output %1 ".to_vec();
    line.extend_from_slice(&[0xFF, 0xFE, b'a']);
    assert_eq!(
        parse_line(&line),
        TmuxEvent::Output {
            pane: 1,
            data: vec![0xFF, 0xFE, b'a'],
        }
    );
}

// ---------- extended-output separator adversary ----------

#[test]
fn extended_output_first_separator_wins_with_binary() {
    // data contains its own " : " and a non-UTF-8 byte
    let mut expect = b"a : ".to_vec();
    expect.push(0xFF);
    assert_eq!(
        ev(b"%extended-output %2 42 : a : \\377"),
        TmuxEvent::ExtendedOutput {
            pane: 2,
            age: 42,
            data: expect,
        }
    );
}

#[test]
fn extended_output_missing_separator_is_other() {
    let l = b"%extended-output %1 10 x";
    assert_eq!(ev(l), other(l));
    let l2 = b"%extended-output %1 10";
    assert_eq!(ev(l2), other(l2));
    let l3 = b"%extended-output %1";
    assert_eq!(ev(l3), other(l3));
}

#[test]
fn extended_output_colon_without_spaces_is_other() {
    // "10:x" is not the literal " : " separator
    let l = b"%extended-output %1 10:x";
    assert_eq!(ev(l), other(l));
}

// ---------- non-UTF-8 in ASCII string fields -> lossy ----------

#[test]
fn window_renamed_non_utf8_name_is_lossy() {
    let mut line = b"%window-renamed @1 ".to_vec();
    line.push(0xFF);
    match parse_line(&line) {
        TmuxEvent::WindowRenamed { window, name } => {
            assert_eq!(window, 1);
            assert_eq!(name, "\u{FFFD}");
        }
        e => panic!("expected WindowRenamed, got {e:?}"),
    }
}

#[test]
fn window_renamed_empty_name_trailing_space() {
    assert_eq!(
        ev(b"%window-renamed @1 "),
        TmuxEvent::WindowRenamed {
            window: 1,
            name: String::new(),
        }
    );
}

#[test]
fn window_renamed_missing_name_is_other() {
    let l = b"%window-renamed @1";
    assert_eq!(ev(l), other(l));
}

#[test]
fn exit_reason_non_utf8_lossy() {
    let mut line = b"%exit ".to_vec();
    line.push(0xFF);
    match parse_line(&line) {
        TmuxEvent::Exit { reason: Some(r) } => assert_eq!(r, "\u{FFFD}"),
        e => panic!("expected Exit(Some), got {e:?}"),
    }
}

// ---------- wrong-sigil / malformed sigils ----------

#[test]
fn wrong_sigils_are_other() {
    for l in [
        &b"%window-add %5"[..], // pane sigil for window
        b"%window-add $5",      // session sigil for window
        b"%window-add 5",       // no sigil
        b"%session-changed @1 x",
        b"%session-changed %1 x",
        b"%pause @1",
        b"%pause $1",
        b"%output @1 x",
        b"%output $1 x",
        b"%pane-mode-changed @1",
    ] {
        assert_eq!(ev(l), other(l), "line {l:?} should be Other");
    }
}

#[test]
fn empty_after_sigil_is_other() {
    for l in [
        &b"%window-add @"[..],
        b"%pause %",
        b"%output % x",
        b"%session-changed $ name",
    ] {
        assert_eq!(ev(l), other(l), "line {l:?} should be Other");
    }
}

#[test]
fn negative_id_is_other() {
    for l in [&b"%window-add @-1"[..], b"%pause %-1", b"%output %-1 x"] {
        assert_eq!(ev(l), other(l), "line {l:?} should be Other");
    }
}

// ---------- layout-change token counts ----------

#[test]
fn layout_change_two_tokens_layout_only() {
    assert_eq!(
        ev(b"%layout-change @9 checksum,80x24,0,0,1"),
        TmuxEvent::LayoutChange {
            window: 9,
            layout: "checksum,80x24,0,0,1".to_string(),
            visible: None,
            flags: None,
        }
    );
}

#[test]
fn layout_change_three_tokens() {
    assert_eq!(
        ev(b"%layout-change @9 A B"),
        TmuxEvent::LayoutChange {
            window: 9,
            layout: "A".to_string(),
            visible: Some("B".to_string()),
            flags: None,
        }
    );
}

#[test]
fn layout_change_one_or_five_tokens_is_other() {
    let l1 = b"%layout-change @9";
    assert_eq!(ev(l1), other(l1));
    let l5 = b"%layout-change @9 a b c d";
    assert_eq!(ev(l5), other(l5));
}

// ---------- keyword edge cases ----------

#[test]
fn bare_percent_and_unknown_keywords() {
    assert_eq!(ev(b"%"), other(b"%"));
    assert_eq!(ev(b"% "), other(b"% "));
    assert_eq!(ev(b"%unknown"), other(b"%unknown"));
    assert_eq!(ev(b"%OUTPUT %1 x"), other(b"%OUTPUT %1 x")); // case-sensitive
    assert_eq!(ev(b"%output-x %1 x"), other(b"%output-x %1 x"));
}

#[test]
fn non_utf8_keyword_is_other() {
    let l = &[b'%', 0xFF, 0xFE][..];
    assert_eq!(ev(l), other(l));
}

#[test]
fn line_not_starting_with_percent_is_other() {
    for l in [
        &b" %output %1 x"[..], // leading space
        b"output %1 x",
        b"\x00%output",
        b"@1 x",
        b"$1",
    ] {
        assert_eq!(ev(l), other(l), "line {l:?} should be Other");
    }
}

// ---------- client events ----------

#[test]
fn client_session_changed_missing_name_is_other() {
    let l = b"%client-session-changed c1 $2";
    assert_eq!(ev(l), other(l));
}

#[test]
fn client_session_changed_missing_session_is_other() {
    let l = b"%client-session-changed c1";
    assert_eq!(ev(l), other(l));
}

#[test]
fn client_detached_missing_client_is_other() {
    assert_eq!(ev(b"%client-detached"), other(b"%client-detached"));
    assert_eq!(ev(b"%client-detached "), other(b"%client-detached "));
}

// ---------- ControlParser adversarial ----------

#[test]
fn stray_end_outside_block_is_notification() {
    let mut p = ControlParser::new();
    assert_eq!(
        p.feed_line(b"%end 1 2 0"),
        Some(ControlMessage::Notification(TmuxEvent::End {
            time: 1,
            num: 2,
            flags: 0
        }))
    );
}

#[test]
fn stray_error_outside_block_is_notification() {
    let mut p = ControlParser::new();
    assert_eq!(
        p.feed_line(b"%error 1 2 0"),
        Some(ControlMessage::Notification(TmuxEvent::Error {
            time: 1,
            num: 2,
            flags: 0
        }))
    );
}

#[test]
fn unclosed_block_never_emits_and_never_panics() {
    let mut p = ControlParser::new();
    assert_eq!(p.feed_line(b"%begin 1 1 0"), None);
    for i in 0..1000 {
        assert_eq!(p.feed_line(format!("body {i}").as_bytes()), None);
    }
    // still open; feeding a non-matching end keeps it open
    assert_eq!(p.feed_line(b"%end 1 999 0"), None);
}

#[test]
fn nested_begin_is_collected_as_body() {
    let mut p = ControlParser::new();
    assert_eq!(p.feed_line(b"%begin 1 1 0"), None);
    assert_eq!(p.feed_line(b"%begin 1 2 0"), None); // nested -> body
    assert_eq!(p.feed_line(b"%end 1 2 0"), None); // wrong num -> body
    assert_eq!(
        p.feed_line(b"%end 1 1 0"),
        Some(ControlMessage::Reply(CommandReply {
            num: 1,
            success: true,
            body: vec![b"%begin 1 2 0".to_vec(), b"%end 1 2 0".to_vec()],
        }))
    );
}

#[test]
fn block_num_zero_and_max() {
    let mut p = ControlParser::new();
    assert_eq!(p.feed_line(b"%begin 1 0 0"), None);
    assert_eq!(
        p.feed_line(b"%end 2 0 0"),
        Some(ControlMessage::Reply(CommandReply {
            num: 0,
            success: true,
            body: vec![],
        }))
    );
    assert_eq!(p.feed_line(b"%begin 1 18446744073709551615 0"), None);
    assert_eq!(
        p.feed_line(b"%error 2 18446744073709551615 0"),
        Some(ControlMessage::Reply(CommandReply {
            num: u64::MAX,
            success: false,
            body: vec![],
        }))
    );
}

#[test]
fn body_preserves_non_utf8_and_empty_lines() {
    let mut p = ControlParser::new();
    assert_eq!(p.feed_line(b"%begin 1 5 0"), None);
    assert_eq!(p.feed_line(b""), None); // empty body line kept
    assert_eq!(p.feed_line(&[0xFF, 0x00, 0xFE]), None);
    match p.feed_line(b"%end 2 5 0") {
        Some(ControlMessage::Reply(r)) => {
            assert_eq!(r.num, 5);
            assert!(r.success);
            assert_eq!(r.body, vec![b"".to_vec(), vec![0xFF, 0x00, 0xFE]]);
        }
        x => panic!("expected reply, got {x:?}"),
    }
}

#[test]
fn malformed_begin_inside_block_and_a_valid_begin_line_as_body() {
    // "%begin 1 2" (too few tokens) parses as Other -> body, not a nested block
    let mut p = ControlParser::new();
    assert_eq!(p.feed_line(b"%begin 1 7 0"), None);
    assert_eq!(p.feed_line(b"%begin 1 2"), None); // malformed -> Other -> body
    assert_eq!(
        p.feed_line(b"%end 9 7 0"),
        Some(ControlMessage::Reply(CommandReply {
            num: 7,
            success: true,
            body: vec![b"%begin 1 2".to_vec()],
        }))
    );
}

// ---------- exhaustive no-panic fuzz over structured mutations ----------

#[test]
fn exhaustive_no_panic_two_byte_and_three_byte_suffixes() {
    let prefixes: &[&[u8]] = &[
        b"%output %1 ",
        b"%output %1 \\",
        b"%extended-output %1 0 : ",
        b"%extended-output %1 ",
        b"%begin ",
        b"%window-renamed @1 ",
        b"%client-session-changed c ",
        b"%layout-change @1 ",
        b"%exit ",
        b"%",
        b"",
    ];
    for p in prefixes {
        for a in 0u16..=255 {
            let mut line = p.to_vec();
            line.push(a as u8);
            let _ = parse_line(&line);
            for b in [0u8, b' ', b':', b'\\', b'%', b'@', b'$', 0xFF, b'7', b'-'] {
                let mut l2 = line.clone();
                l2.push(b);
                let _ = parse_line(&l2);
                let mut cp = ControlParser::new();
                let _ = cp.feed_line(b"%begin 1 1 0");
                let _ = cp.feed_line(&l2);
                let _ = cp.feed_line(b"%end 1 1 0");
            }
        }
    }
}

#[test]
fn exhaustive_output_backslash_triples_no_panic() {
    // all backslash + 3 arbitrary bytes into %output data
    for a in 0u16..=255 {
        for &b in &[b'0', b'3', b'7', b'8', b'9', b'x', b'\\', 0xFF] {
            for &c in &[b'0', b'7', b'8', 0xFF, b' '] {
                let mut line = b"%output %1 ".to_vec();
                line.push(b'\\');
                line.push(a as u8);
                line.push(b);
                line.push(c);
                match parse_line(&line) {
                    TmuxEvent::Output { .. } => {}
                    e => panic!("expected Output, got {e:?} for {line:?}"),
                }
            }
        }
    }
}

#[test]
fn all_bytes_as_whole_line_and_as_single_field() {
    for a in 0u16..=255 {
        let byte = a as u8;
        let _ = parse_line(&[byte]);
        let _ = parse_line(&[b'%', byte]);
        let mut cp = ControlParser::new();
        let _ = cp.feed_line(&[byte]);
    }
}
