use super::{FlowControl, MAX_PANE_BUFFER_BYTES, OutputRoute};

#[test]
fn live_pane_delivers_output() {
    let mut flow = FlowControl::default();
    assert!(!flow.is_paused(1));
    assert_eq!(flow.route_output(1, b"hi"), OutputRoute::Deliver);
}

#[test]
fn pause_marks_pane_and_returns_newly_paused() {
    let mut flow = FlowControl::default();
    assert!(flow.pause(1));
    assert!(flow.is_paused(1));
    // A redundant pause reports it was already paused.
    assert!(!flow.pause(1));
}

#[test]
fn paused_pane_buffers_output_and_resume_drains_in_order() {
    let mut flow = FlowControl::default();
    flow.pause(1);
    assert_eq!(flow.route_output(1, b"a"), OutputRoute::Buffered);
    assert_eq!(flow.route_output(1, b"b"), OutputRoute::Buffered);

    let drained = flow.resume(1);
    assert_eq!(drained, vec![b"a".to_vec(), b"b".to_vec()]);
    // After resume the pane is live again and its buffer is empty.
    assert!(!flow.is_paused(1));
    assert!(flow.resume(1).is_empty());
}

#[test]
fn buffering_is_bounded_by_dropping_oldest_chunks() {
    let mut flow = FlowControl::default();
    flow.pause(1);
    // Two chunks each just over half the cap: the first is evicted when the
    // second pushes the total past the limit.
    let chunk = vec![b'x'; MAX_PANE_BUFFER_BYTES / 2 + 1];
    flow.route_output(1, &chunk);
    flow.route_output(1, &chunk);
    flow.route_output(1, b"tail");

    let drained = flow.resume(1);
    // Only the most recent chunk plus the tail survive the cap.
    assert_eq!(drained.len(), 2);
    assert_eq!(drained[1], b"tail".to_vec());
}

#[test]
fn output_for_a_different_paused_pane_is_independent() {
    let mut flow = FlowControl::default();
    flow.pause(1);
    assert_eq!(flow.route_output(1, b"a"), OutputRoute::Buffered);
    // Pane 2 was never paused, so its output is delivered immediately.
    assert_eq!(flow.route_output(2, b"b"), OutputRoute::Deliver);
}

#[test]
fn forget_clears_pause_and_buffer() {
    let mut flow = FlowControl::default();
    flow.pause(1);
    flow.route_output(1, b"a");
    flow.forget(1);
    assert!(!flow.is_paused(1));
    assert!(flow.resume(1).is_empty());
}
