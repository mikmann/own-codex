use super::*;
use pretty_assertions::assert_eq;

fn thread_id(suffix: u128) -> ThreadId {
    ThreadId::from_string(&format!("00000000-0000-0000-0000-{suffix:012}"))
        .expect("valid thread id")
}

fn open_edge(parent_thread_id: ThreadId) -> InMemorySpawnEdge {
    InMemorySpawnEdge {
        parent_thread_id,
        status: ThreadSpawnEdgeStatus::Open,
    }
}

#[test]
fn spawn_path_requires_an_open_chain_to_the_expected_root() {
    let root = thread_id(/*suffix*/ 1);
    let child = thread_id(/*suffix*/ 2);
    let grandchild = thread_id(/*suffix*/ 3);
    let mut edges = HashMap::from([(child, open_edge(root)), (grandchild, open_edge(child))]);

    assert!(spawn_path_is_open(&edges, root, grandchild));
    edges.get_mut(&child).expect("child edge").status = ThreadSpawnEdgeStatus::Closing;
    assert!(!spawn_path_is_open(&edges, root, grandchild));
    assert!(!spawn_path_is_open(
        &edges,
        thread_id(/*suffix*/ 9),
        grandchild
    ));
}

#[test]
fn close_authorization_requires_open_intermediate_edges() {
    let root = thread_id(/*suffix*/ 10);
    let child = thread_id(/*suffix*/ 11);
    let grandchild = thread_id(/*suffix*/ 12);
    let leaf = thread_id(/*suffix*/ 13);
    let mut edges = HashMap::from([
        (child, open_edge(root)),
        (grandchild, open_edge(child)),
        (leaf, open_edge(grandchild)),
    ]);

    assert!(target_is_authorized_descendant(&edges, child, leaf));
    edges.get_mut(&grandchild).expect("grandchild edge").status = ThreadSpawnEdgeStatus::Closed;
    assert!(!target_is_authorized_descendant(&edges, child, leaf));
    assert!(target_is_authorized_descendant(&edges, child, grandchild));
}

#[test]
fn subtree_order_is_deepest_first_and_contains_target_once() {
    let root = thread_id(/*suffix*/ 20);
    let child = thread_id(/*suffix*/ 21);
    let left = thread_id(/*suffix*/ 22);
    let right = thread_id(/*suffix*/ 23);
    let leaf = thread_id(/*suffix*/ 24);
    let edges = HashMap::from([
        (child, open_edge(root)),
        (left, open_edge(child)),
        (right, open_edge(child)),
        (leaf, open_edge(left)),
    ]);

    assert_eq!(
        subtree_thread_ids_deepest_first(&edges, child),
        vec![leaf, left, right, child]
    );
}
