use std::collections::HashMap;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NodeId(pub String);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SessionId(pub String);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeKind {
    Intent,
    Prompt,
    Artifact,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactKind {
    Context,
    Implementation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeStatus {
    Proposed,
    Accepted,
    Rejected,
    Superseded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionState {
    Active,
    Completed,
    Abandoned,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    pub id: NodeId,
    pub label: String,
    pub kind: NodeKind,
    pub status: NodeStatus,
    pub artifact_kind: Option<ArtifactKind>,
    pub parent_id: Option<NodeId>,
    pub children_ids: Vec<NodeId>,
    pub input_node_ids: Vec<NodeId>,
    pub payload_ref: Option<String>,
}

impl Node {
    #[must_use]
    pub fn is_eligible_context(&self) -> bool {
        self.kind == NodeKind::Artifact && self.status == NodeStatus::Accepted
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Session {
    pub session_id: SessionId,
    pub schema_version: String,
    pub intent_node_id: NodeId,
    pub current_node_id: NodeId,
    pub state: SessionState,
    pub nodes: HashMap<NodeId, Node>,
}

impl Session {
    #[must_use]
    pub fn can_attach_child(parent_kind: NodeKind, child_kind: NodeKind) -> bool {
        matches!(
            (parent_kind, child_kind),
            (NodeKind::Intent, NodeKind::Prompt)
                | (NodeKind::Prompt, NodeKind::Prompt)
                | (NodeKind::Prompt, NodeKind::Artifact)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{NodeKind, Session};

    #[test]
    fn prompt_nodes_can_have_prompt_children() {
        assert!(Session::can_attach_child(NodeKind::Prompt, NodeKind::Prompt));
    }

    #[test]
    fn artifact_nodes_cannot_have_children() {
        assert!(!Session::can_attach_child(NodeKind::Artifact, NodeKind::Prompt));
        assert!(!Session::can_attach_child(NodeKind::Artifact, NodeKind::Artifact));
    }
}
