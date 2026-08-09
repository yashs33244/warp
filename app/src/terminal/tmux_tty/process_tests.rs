use super::control_args;

#[test]
fn new_session_uses_control_mode_only() {
    assert_eq!(control_args(None), vec!["-CC".to_string()]);
}

#[test]
fn attach_targets_named_session() {
    assert_eq!(
        control_args(Some("work")),
        vec![
            "-CC".to_string(),
            "attach".to_string(),
            "-t".to_string(),
            "work".to_string(),
        ]
    );
}
