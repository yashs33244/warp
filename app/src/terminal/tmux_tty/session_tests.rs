use super::drain_complete_lines;
use super::parse_window_layout_line;
use super::{LifecycleEffect, TmuxSession};

// Valid tmux 3.5a layout strings (checksum + body) reused from the protocol
// core's vectors: V2 has panes {1, 2}, V1 has panes {1, 2, 3}.
const V1: &str = "e123,204x53,0,0{102x53,0,0,1,101x53,103,0[101x26,103,0,2,101x26,103,27,3]}";
const V2: &str = "ee3e,204x53,0,0{102x53,0,0,1,101x53,103,0,2}";

fn layout_change_line(window: u32, layout: &str) -> Vec<u8> {
    format!("%layout-change @{window} {layout}\n").into_bytes()
}

#[test]
fn drains_only_complete_lines() {
    let mut buf = b"%begin 1 0\n%output %1 hi".to_vec();
    let lines = drain_complete_lines(&mut buf);
    assert_eq!(lines, vec![b"%begin 1 0".to_vec()]);
    assert_eq!(buf, b"%output %1 hi".to_vec());
}

#[test]
fn strips_carriage_returns_across_multiple_lines() {
    let mut buf = b"a\r\nb\r\n".to_vec();
    let lines = drain_complete_lines(&mut buf);
    assert_eq!(lines, vec![b"a".to_vec(), b"b".to_vec()]);
    assert!(buf.is_empty());
}

#[test]
fn buffers_a_partial_line() {
    let mut buf = b"partial".to_vec();
    let lines = drain_complete_lines(&mut buf);
    assert!(lines.is_empty());
    assert_eq!(buf, b"partial".to_vec());
}

#[test]
fn preserves_empty_lines() {
    let mut buf = b"\n\n".to_vec();
    let lines = drain_complete_lines(&mut buf);
    assert_eq!(lines, vec![Vec::<u8>::new(), Vec::<u8>::new()]);
    assert!(buf.is_empty());
}

#[test]
fn first_layout_change_marks_every_pane_added() {
    let mut session = TmuxSession::new();
    let effects = session.feed_control_bytes(&layout_change_line(1, V2));
    match effects.as_slice() {
        [LifecycleEffect::WindowLayout { window, delta, .. }] => {
            assert_eq!(*window, 1);
            assert_eq!(delta.added, vec![1, 2]);
            assert!(delta.removed.is_empty());
        }
        other => panic!("expected one WindowLayout effect, got {other:?}"),
    }
}

#[test]
fn second_layout_change_diffs_against_prior_pane_set() {
    let mut session = TmuxSession::new();
    let _ = session.feed_control_bytes(&layout_change_line(1, V2)); // panes {1,2}
    let effects = session.feed_control_bytes(&layout_change_line(1, V1)); // panes {1,2,3}
    match effects.as_slice() {
        [LifecycleEffect::WindowLayout { delta, .. }] => {
            assert_eq!(delta.added, vec![3]);
            assert!(delta.removed.is_empty());
        }
        other => panic!("expected one WindowLayout effect, got {other:?}"),
    }
}

#[test]
fn malformed_layout_change_produces_no_effect() {
    let mut session = TmuxSession::new();
    // Wrong checksum: parse_layout rejects it, so no state change and no effect.
    let effects = session.feed_control_bytes(&layout_change_line(1, "0000,80x24,0,0,1"));
    assert!(effects.is_empty());
}

#[test]
fn window_close_and_exit_are_reported() {
    let mut session = TmuxSession::new();
    let _ = session.feed_control_bytes(&layout_change_line(2, V2));

    let closed = session.feed_control_bytes(b"%window-close @2\n");
    assert_eq!(closed, vec![LifecycleEffect::WindowClosed { window: 2 }]);

    let exited = session.feed_control_bytes(b"%exit shutting down\n");
    assert_eq!(
        exited,
        vec![LifecycleEffect::Exit {
            reason: Some("shutting down".to_string()),
        }]
    );
}

#[test]
fn output_for_unknown_pane_yields_no_effect() {
    let mut session = TmuxSession::new();
    // No sink registered for pane 9: the output is dropped, no effect emitted.
    let effects = session.feed_control_bytes(b"%output %9 hello\n");
    assert!(effects.is_empty());
}

#[test]
fn pause_requests_a_resume() {
    let mut session = TmuxSession::new();
    let effects = session.feed_control_bytes(b"%pause %4\n");
    assert_eq!(effects, vec![LifecycleEffect::ResumePane { pane: 4 }]);
    // The continue notification just clears bookkeeping; no effect is emitted.
    let resumed = session.feed_control_bytes(b"%continue %4\n");
    assert!(resumed.is_empty());
}

#[test]
fn strips_dcs_intro_so_first_line_parses() {
    let mut session = TmuxSession::new();
    // tmux prefixes the stream with the DCS intro glued to the first control
    // line; without stripping it the first line would fail to parse.
    let mut bytes = b"\x1bP1000p".to_vec();
    bytes.extend_from_slice(&layout_change_line(1, V2));
    match session.feed_control_bytes(&bytes).as_slice() {
        [LifecycleEffect::WindowLayout { window, .. }] => assert_eq!(*window, 1),
        other => panic!("expected the first line to parse, got {other:?}"),
    }
}

#[test]
fn strips_dcs_intro_split_across_chunks() {
    let mut session = TmuxSession::new();
    // The 7-byte intro can straddle a read boundary; stripping must defer until
    // enough bytes have arrived to match it.
    assert!(session.feed_control_bytes(b"\x1bP10").is_empty());
    let mut rest = b"00p".to_vec();
    rest.extend_from_slice(&layout_change_line(1, V2));
    match session.feed_control_bytes(&rest).as_slice() {
        [LifecycleEffect::WindowLayout { window, .. }] => assert_eq!(*window, 1),
        other => panic!("expected the first line to parse, got {other:?}"),
    }
}

#[test]
fn list_windows_reply_bootstraps_a_window() {
    let mut session = TmuxSession::new();
    // tmux emits no `%layout-change` on attach; the tab is seeded from the
    // `list-windows` reply body instead.
    let reply = format!("%begin 100 5 1\n@1 {V2}\n%end 100 5 1\n").into_bytes();
    match session.feed_control_bytes(&reply).as_slice() {
        [LifecycleEffect::WindowLayout { window, delta, .. }] => {
            assert_eq!(*window, 1);
            assert_eq!(delta.added, vec![1, 2]);
        }
        other => panic!("expected one WindowLayout effect, got {other:?}"),
    }
}

#[test]
fn list_windows_reply_bootstraps_every_window() {
    let mut session = TmuxSession::new();
    let reply = format!("%begin 1 1 1\n@1 {V2}\n@2 {V1}\n%end 1 1 1\n").into_bytes();
    match session.feed_control_bytes(&reply).as_slice() {
        [
            LifecycleEffect::WindowLayout { window: w1, .. },
            LifecycleEffect::WindowLayout { window: w2, .. },
        ] => {
            assert_eq!(*w1, 1);
            assert_eq!(*w2, 2);
        }
        other => panic!("expected two WindowLayout effects, got {other:?}"),
    }
}

#[test]
fn failed_or_empty_reply_yields_no_effect() {
    let mut session = TmuxSession::new();
    // An empty-bodied success (e.g. a send-keys ack) produces nothing.
    assert!(
        session
            .feed_control_bytes(b"%begin 2 3 1\n%end 2 3 1\n")
            .is_empty()
    );
    // A failed command (`%error`) is ignored even if its body looks like a layout.
    let err = format!("%begin 3 4 1\n@1 {V2}\n%error 3 4 1\n").into_bytes();
    assert!(session.feed_control_bytes(&err).is_empty());
}

#[test]
fn parse_window_layout_line_accepts_well_formed_lines() {
    assert_eq!(
        parse_window_layout_line(b"@0 aafd,120x40,0,0,0"),
        Some((0, "aafd,120x40,0,0,0".to_string()))
    );
    assert_eq!(
        parse_window_layout_line(b"@12 layout"),
        Some((12, "layout".to_string()))
    );
}

#[test]
fn parse_window_layout_line_rejects_malformed_lines() {
    assert_eq!(parse_window_layout_line(b"no-space"), None);
    assert_eq!(parse_window_layout_line(b"0 missing-at"), None);
    assert_eq!(parse_window_layout_line(b"@ empty-id"), None);
    assert_eq!(parse_window_layout_line(b"@1x non-digit"), None);
    assert_eq!(parse_window_layout_line(b"@1 "), None);
}
