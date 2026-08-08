use warp_terminal::tmux::layout::parse_layout;

use super::{TmuxPaneTree, map_window, map_window_to};
use crate::app_state::{LeafContents, LeafSnapshot, PaneNodeSnapshot, SplitDirection};

/// tmux `list-windows` layout for a 204x53 session split left-right, with the
/// right column then split top-bottom: pane 1 on the left, panes 2 (top) and
/// 3 (bottom) stacked on the right.
const LAYOUT: &str = "e123,204x53,0,0{102x53,0,0,1,101x53,103,0[101x26,103,0,2,101x26,103,27,3]}";

/// Test leaf builder: encodes the tmux pane id in `custom_vertical_tabs_title`
/// so assertions can recover which pane landed where. Uses a field-less
/// `LeafContents` variant to keep the shape assertions decoupled from the
/// terminal pane snapshot's fields.
fn leaf_for(pane: u32) -> LeafSnapshot {
    LeafSnapshot {
        is_focused: false,
        custom_vertical_tabs_title: Some(pane.to_string()),
        contents: LeafContents::NetworkLog,
    }
}

fn pane_id(node: &PaneNodeSnapshot) -> u32 {
    match node {
        PaneNodeSnapshot::Leaf(leaf) => leaf
            .custom_vertical_tabs_title
            .as_deref()
            .expect("leaf carries a pane id")
            .parse()
            .expect("pane id parses"),
        PaneNodeSnapshot::Branch(_) => panic!("expected a leaf, found a branch"),
    }
}

fn branch(node: &PaneNodeSnapshot) -> &crate::app_state::BranchSnapshot {
    match node {
        PaneNodeSnapshot::Branch(b) => b,
        PaneNodeSnapshot::Leaf(_) => panic!("expected a branch, found a leaf"),
    }
}

#[test]
fn maps_left_pane_and_stacked_right_panes() {
    let window = parse_layout(LAYOUT).expect("layout parses");
    let tree = map_window(&window, &leaf_for);

    // Root is a left-right split with two children.
    let root = branch(&tree);
    assert_eq!(root.direction, SplitDirection::Horizontal);
    assert_eq!(root.children.len(), 2);

    // Left child is the single pane 1.
    assert_eq!(pane_id(&root.children[0].1), 1);

    // Right child is a top-bottom split of pane 2 (top) over pane 3 (bottom).
    let right = branch(&root.children[1].1);
    assert_eq!(right.direction, SplitDirection::Vertical);
    assert_eq!(right.children.len(), 2);
    assert_eq!(pane_id(&right.children[0].1), 2);
    assert_eq!(pane_id(&right.children[1].1), 3);
}

#[test]
fn flex_weights_follow_the_split_axis_extent() {
    let window = parse_layout(LAYOUT).expect("layout parses");
    let root = match map_window(&window, &leaf_for) {
        PaneNodeSnapshot::Branch(b) => b,
        PaneNodeSnapshot::Leaf(_) => panic!("root should be a branch"),
    };

    // Left-right split weights by width: 102 vs 101 columns.
    assert_eq!(root.children[0].0.0, 102.0);
    assert_eq!(root.children[1].0.0, 101.0);

    // Top-bottom split weights by height: 26 vs 26 rows.
    let right = branch(&root.children[1].1);
    assert_eq!(right.children[0].0.0, 26.0);
    assert_eq!(right.children[1].0.0, 26.0);
}

#[test]
fn single_pane_layout_maps_to_a_leaf() {
    let window =
        parse_layout("ee3e,204x53,0,0{102x53,0,0,1,101x53,103,0,2}").expect("layout parses");
    let tree = map_window(&window, &leaf_for);
    let root = branch(&tree);
    assert_eq!(root.direction, SplitDirection::Horizontal);
    assert_eq!(pane_id(&root.children[0].1), 1);
    assert_eq!(pane_id(&root.children[1].1), 2);
}

fn generic_pane_id(node: &TmuxPaneTree<u32>) -> u32 {
    match node {
        TmuxPaneTree::Leaf(pane) => *pane,
        TmuxPaneTree::Branch { .. } => panic!("expected a leaf, found a branch"),
    }
}

fn generic_branch(
    node: &TmuxPaneTree<u32>,
) -> (
    &SplitDirection,
    &[(crate::app_state::PaneFlex, TmuxPaneTree<u32>)],
) {
    match node {
        TmuxPaneTree::Branch {
            direction,
            children,
        } => (direction, children),
        TmuxPaneTree::Leaf(_) => panic!("expected a branch, found a leaf"),
    }
}

#[test]
fn generic_mapper_preserves_shape_direction_and_flex() {
    let window = parse_layout(LAYOUT).expect("layout parses");
    let tree = map_window_to(&window, &mut |pane, _cols, _rows| pane);

    let (dir, children) = generic_branch(&tree);
    assert_eq!(*dir, SplitDirection::Horizontal);
    assert_eq!(children.len(), 2);

    // Left child is pane 1 with width-based flex 102.
    assert_eq!(children[0].0.0, 102.0);
    assert_eq!(generic_pane_id(&children[0].1), 1);

    // Right child is a top-bottom split of panes 2 over 3, flex by width 101.
    assert_eq!(children[1].0.0, 101.0);
    let (right_dir, right_children) = generic_branch(&children[1].1);
    assert_eq!(*right_dir, SplitDirection::Vertical);
    assert_eq!(generic_pane_id(&right_children[0].1), 2);
    assert_eq!(generic_pane_id(&right_children[1].1), 3);
    assert_eq!(right_children[0].0.0, 26.0);
    assert_eq!(right_children[1].0.0, 26.0);
}

#[test]
fn generic_mapper_visits_leaves_depth_first_left_to_right() {
    let window = parse_layout(LAYOUT).expect("layout parses");
    let mut visited = Vec::new();
    let _ = map_window_to(&window, &mut |pane, _cols, _rows| {
        visited.push(pane);
        pane
    });
    assert_eq!(visited, vec![1, 2, 3]);
}
