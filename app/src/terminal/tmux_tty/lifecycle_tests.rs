use warp_terminal::tmux::layout::{LayoutNode, SplitDir};

use super::{diff_pane_ids, pane_ids};

fn leaf(pane: u32) -> LayoutNode {
    LayoutNode::Leaf {
        pane,
        w: 80,
        h: 24,
        x: 0,
        y: 0,
    }
}

fn split(dir: SplitDir, children: Vec<LayoutNode>) -> LayoutNode {
    LayoutNode::Split {
        dir,
        w: 80,
        h: 24,
        x: 0,
        y: 0,
        children,
    }
}

#[test]
fn single_leaf_yields_one_id() {
    assert_eq!(pane_ids(&leaf(7)), vec![7]);
}

#[test]
fn nested_split_collects_all_ids_depth_first() {
    // { 1, [ 2, 3 ], 4 } -> 1,2,3,4 in emission order.
    let root = split(
        SplitDir::Horizontal,
        vec![
            leaf(1),
            split(SplitDir::Vertical, vec![leaf(2), leaf(3)]),
            leaf(4),
        ],
    );
    assert_eq!(pane_ids(&root), vec![1, 2, 3, 4]);
}

#[test]
fn diff_reports_added_and_removed() {
    let delta = diff_pane_ids(&[1, 2, 3], &[2, 3, 4]);
    assert_eq!(delta.added, vec![4]);
    assert_eq!(delta.removed, vec![1]);
    assert!(!delta.is_empty());
}

#[test]
fn diff_of_identical_sets_is_empty() {
    let delta = diff_pane_ids(&[1, 2, 3], &[1, 2, 3]);
    assert!(delta.is_empty());
}

#[test]
fn diff_from_empty_marks_all_added() {
    // First layout for a fresh window: every pane is new.
    let delta = diff_pane_ids(&[], &[5, 6]);
    assert_eq!(delta.added, vec![5, 6]);
    assert!(delta.removed.is_empty());
}

#[test]
fn diff_to_empty_marks_all_removed() {
    let delta = diff_pane_ids(&[5, 6], &[]);
    assert!(delta.added.is_empty());
    assert_eq!(delta.removed, vec![5, 6]);
}
