use std::borrow::Cow;

use warp_terminal::tmux::command;

use super::TmuxInputSender;
use crate::terminal::SizeInfo;
use crate::terminal::writeable_pty::Message;
use crate::terminal::writeable_pty::pty_controller::EventLoopSender;

const PANE: u32 = 7;

fn sender() -> (TmuxInputSender, async_channel::Receiver<Vec<u8>>) {
    let (tx, rx) = async_channel::unbounded();
    (TmuxInputSender::new(PANE, tx), rx)
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
fn resize_becomes_refresh_client_size() {
    let (sender, rx) = sender();
    let size = SizeInfo::new_without_font_metrics(24, 80);
    sender.send(Message::Resize(size)).expect("send succeeds");
    let sent = rx.try_recv().expect("one command queued");
    assert_eq!(sent, command::refresh_client_size(80, 24));
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
