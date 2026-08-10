use super::drain_complete_lines;

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
