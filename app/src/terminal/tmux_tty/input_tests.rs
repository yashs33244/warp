use std::borrow::Cow;

use warp_terminal::tmux::command;

use super::{TmuxInputSender, TmuxResponder};
use crate::terminal::SizeInfo;
use crate::terminal::tmux_tty::geometry::TmuxGeometry;
use crate::terminal::writeable_pty::Message;
use crate::terminal::writeable_pty::pty_controller::EventLoopSender;

const WINDOW: u32 = 3;
const PANE: u32 = 7;

fn sender() -> (TmuxInputSender, async_channel::Receiver<Vec<u8>>) {
    let (tx, rx) = async_channel::unbounded();
    (
        TmuxInputSender::new(WINDOW, PANE, TmuxGeometry::default(), tx),
        rx,
    )
}

#[test]
fn input_bytes_become_send_keys_hex() {
    let (sender, rx) = sender();
    sender
        .send(Message::Input(Cow::Borrowed(b"ls\n" as &[u8])))
        .expect("send succeeds");
    let sent = rx.try_recv().expect("one command queued");
    assert_eq!(sent, command::send_keys_hex(PANE, b"ls\n").unwrap());
}

#[test]
fn multibyte_input_is_sent_as_code_points_not_utf8_bytes() {
    let (sender, rx) = sender();
    // "é" is U+00E9 (UTF-8 c3 a9) and "😀" is U+1F600 (UTF-8 f0 9f 98 80). Each
    // must reach tmux as its code point, not as its raw bytes, or `send-keys -H`
    // delivers several wrong characters.
    sender
        .send(Message::Input(Cow::Borrowed("é😀".as_bytes())))
        .expect("send succeeds");
    let sent = rx.try_recv().expect("one command queued");
    assert_eq!(sent, b"send-keys -t %7 -H e9 1f600\n".to_vec());
}

#[test]
fn resize_becomes_refresh_client_size() {
    let (sender, rx) = sender();
    let size = SizeInfo::new_without_font_metrics(24, 80);
    sender.send(Message::Resize(size)).expect("send succeeds");
    let sent = rx.try_recv().expect("one command queued");
    assert_eq!(sent, command::refresh_client_window_size(WINDOW, 80, 24));
    // Repeating the same size is dropped, so tmux's answering layout change
    // cannot bounce back as another resize.
    let size = SizeInfo::new_without_font_metrics(24, 80);
    sender.send(Message::Resize(size)).expect("send succeeds");
    assert!(rx.try_recv().is_err());
}

#[test]
fn empty_input_sends_nothing() {
    let (sender, rx) = sender();
    sender
        .send(Message::Input(Cow::Borrowed(b"" as &[u8])))
        .expect("send succeeds");
    assert!(rx.try_recv().is_err());
}

#[test]
fn lifecycle_messages_send_nothing() {
    let (sender, rx) = sender();
    sender.send(Message::Shutdown).expect("send succeeds");
    assert!(rx.try_recv().is_err());
}

#[test]
fn responder_forwards_reply_as_send_keys_hex() {
    let (tx, rx) = async_channel::unbounded();
    let responder = TmuxResponder::new(PANE, tx);
    // A DSR cursor-position reply the emulator would generate.
    responder.send_response(b"\x1b[1;1R");
    let sent = rx.try_recv().expect("one command queued");
    assert_eq!(sent, command::send_keys_hex(PANE, b"\x1b[1;1R").unwrap());
}

#[test]
fn responder_ignores_empty_reply() {
    let (tx, rx) = async_channel::unbounded::<Vec<u8>>();
    let responder = TmuxResponder::new(PANE, tx);
    responder.send_response(b"");
    assert!(rx.try_recv().is_err());
}
