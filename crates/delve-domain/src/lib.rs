#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub String);

impl NodeId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for NodeId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl From<String> for NodeId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl PartialEq<str> for NodeId {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for NodeId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for SessionId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl From<String> for SessionId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    Intent,
    Prompt,
    Artifact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactKind {
    Context,
    Implementation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeStatus {
    Proposed,
    Accepted,
    Rejected,
    Superseded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    Active,
    Completed,
    Abandoned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    InvalidParentChild {
        parent_kind: NodeKind,
        child_kind: NodeKind,
    },
    InvalidStatusTransition {
        from: NodeStatus,
        to: NodeStatus,
    },
    DuplicateNodeId {
        node_id: NodeId,
    },
    MissingIntentNode {
        intent_node_id: NodeId,
    },
    IntentNodeMustBeIntent {
        node_id: NodeId,
        found_kind: NodeKind,
    },
    UnexpectedIntentNode {
        node_id: NodeId,
    },
    MissingCurrentNode {
        current_node_id: NodeId,
    },
    InvalidCurrentNodeKind {
        node_id: NodeId,
        kind: NodeKind,
    },
    InvalidCurrentNodeStatus {
        node_id: NodeId,
        status: NodeStatus,
    },
    NodeMissingParent {
        node_id: NodeId,
        kind: NodeKind,
    },
    MissingParentNode {
        node_id: NodeId,
        parent_id: NodeId,
    },
    MissingChildNode {
        parent_id: NodeId,
        child_id: NodeId,
    },
    ParentMissingChildReference {
        parent_id: NodeId,
        child_id: NodeId,
    },
    ChildMissingParentReference {
        parent_id: NodeId,
        child_id: NodeId,
        observed_parent_id: Option<NodeId>,
    },
    InvalidParentChildLink {
        parent_id: NodeId,
        parent_kind: NodeKind,
        child_id: NodeId,
        child_kind: NodeKind,
    },
}

impl NodeStatus {
    #[must_use]
    pub fn can_transition_to(self, next: Self) -> bool {
        self == next
            || matches!(
                (self, next),
                (Self::Proposed, Self::Accepted)
                    | (Self::Proposed, Self::Rejected)
                    | (Self::Accepted, Self::Superseded)
            )
    }

    pub fn validate_transition(self, next: Self) -> Result<(), ValidationError> {
        if self.can_transition_to(next) {
            Ok(())
        } else {
            Err(ValidationError::InvalidStatusTransition {
                from: self,
                to: next,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionNode {
    pub id: NodeId,
    pub label: String,
    pub kind: NodeKind,
    pub artifact_kind: Option<ArtifactKind>,
    pub status: NodeStatus,
    pub parent_id: Option<NodeId>,
    pub children_ids: Vec<NodeId>,
    pub input_node_ids: Vec<NodeId>,
    pub payload_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTree {
    pub schema_version: u32,
    pub session_id: SessionId,
    pub intent_node_id: NodeId,
    pub current_node_id: NodeId,
    pub state: SessionState,
    pub nodes: Vec<SessionNode>,
}

impl SessionTree {
    #[must_use]
    pub fn new(intent_label: impl Into<String>) -> Self {
        let intent_id = NodeId::from("intent-root");
        let intent_node = SessionNode {
            id: intent_id.clone(),
            label: intent_label.into(),
            kind: NodeKind::Intent,
            artifact_kind: None,
            status: NodeStatus::Accepted,
            parent_id: None,
            children_ids: Vec::new(),
            input_node_ids: Vec::new(),
            payload_ref: Some(String::from("intent.md")),
        };

        Self {
            schema_version: 1,
            session_id: SessionId::from("session-local"),
            intent_node_id: intent_id.clone(),
            current_node_id: intent_id,
            state: SessionState::Active,
            nodes: vec![intent_node],
        }
    }

    pub fn validate_tree_invariants(&self) -> Result<(), ValidationError> {
        let mut node_map: HashMap<&str, &SessionNode> = HashMap::with_capacity(self.nodes.len());
        for node in &self.nodes {
            if node_map.insert(node.id.as_str(), node).is_some() {
                return Err(ValidationError::DuplicateNodeId {
                    node_id: node.id.clone(),
                });
            }
        }

        let Some(intent_node) = node_map.get(self.intent_node_id.as_str()) else {
            return Err(ValidationError::MissingIntentNode {
                intent_node_id: self.intent_node_id.clone(),
            });
        };

        if intent_node.kind != NodeKind::Intent {
            return Err(ValidationError::IntentNodeMustBeIntent {
                node_id: self.intent_node_id.clone(),
                found_kind: intent_node.kind,
            });
        }

        let Some(current_node) = node_map.get(self.current_node_id.as_str()) else {
            return Err(ValidationError::MissingCurrentNode {
                current_node_id: self.current_node_id.clone(),
            });
        };

        validate_current_node_candidate(current_node)?;

        for node in &self.nodes {
            if node.kind == NodeKind::Intent && node.id != self.intent_node_id {
                return Err(ValidationError::UnexpectedIntentNode {
                    node_id: node.id.clone(),
                });
            }

            if node.kind == NodeKind::Intent {
                continue;
            }

            let Some(parent_id) = node.parent_id.as_ref() else {
                return Err(ValidationError::NodeMissingParent {
                    node_id: node.id.clone(),
                    kind: node.kind,
                });
            };

            let Some(parent) = node_map.get(parent_id.as_str()) else {
                return Err(ValidationError::MissingParentNode {
                    node_id: node.id.clone(),
                    parent_id: parent_id.clone(),
                });
            };

            if !can_attach_child(parent.kind, node.kind) {
                return Err(ValidationError::InvalidParentChildLink {
                    parent_id: parent_id.clone(),
                    parent_kind: parent.kind,
                    child_id: node.id.clone(),
                    child_kind: node.kind,
                });
            }

            if !parent.children_ids.contains(&node.id) {
                return Err(ValidationError::ParentMissingChildReference {
                    parent_id: parent_id.clone(),
                    child_id: node.id.clone(),
                });
            }
        }

        for parent in &self.nodes {
            for child_id in &parent.children_ids {
                let Some(child) = node_map.get(child_id.as_str()) else {
                    return Err(ValidationError::MissingChildNode {
                        parent_id: parent.id.clone(),
                        child_id: child_id.clone(),
                    });
                };

                if child.parent_id.as_ref() != Some(&parent.id) {
                    return Err(ValidationError::ChildMissingParentReference {
                        parent_id: parent.id.clone(),
                        child_id: child_id.clone(),
                        observed_parent_id: child.parent_id.clone(),
                    });
                }

                if !can_attach_child(parent.kind, child.kind) {
                    return Err(ValidationError::InvalidParentChildLink {
                        parent_id: parent.id.clone(),
                        parent_kind: parent.kind,
                        child_id: child_id.clone(),
                        child_kind: child.kind,
                    });
                }
            }
        }

        Ok(())
    }

    pub fn set_current_node(&mut self, next_node_id: NodeId) -> Result<(), ValidationError> {
        let Some(next_node) = self.nodes.iter().find(|node| node.id == next_node_id) else {
            return Err(ValidationError::MissingCurrentNode {
                current_node_id: next_node_id,
            });
        };

        validate_current_node_candidate(next_node)?;
        self.current_node_id = next_node.id.clone();
        Ok(())
    }

    pub fn active_lineage_node_ids(&self) -> Result<Vec<NodeId>, ValidationError> {
        let node_map: HashMap<&str, &SessionNode> = self
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), node))
            .collect();

        let mut lineage_reversed = Vec::new();
        let mut cursor_id = self.current_node_id.clone();

        loop {
            let Some(cursor_node) = node_map.get(cursor_id.as_str()) else {
                return Err(ValidationError::MissingCurrentNode {
                    current_node_id: cursor_id,
                });
            };

            lineage_reversed.push(cursor_node.id.clone());

            let Some(parent_id) = cursor_node.parent_id.as_ref() else {
                break;
            };

            if !node_map.contains_key(parent_id.as_str()) {
                return Err(ValidationError::MissingParentNode {
                    node_id: cursor_node.id.clone(),
                    parent_id: parent_id.clone(),
                });
            }

            cursor_id = parent_id.clone();
        }

        lineage_reversed.reverse();
        Ok(lineage_reversed)
    }

    pub fn resolve_eligible_context_node_ids(&self) -> Result<Vec<NodeId>, ValidationError> {
        let node_map: HashMap<&str, &SessionNode> = self
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), node))
            .collect();
        let lineage = self.active_lineage_node_ids()?;
        let current_node = node_map.get(self.current_node_id.as_str()).ok_or_else(|| {
            ValidationError::MissingCurrentNode {
                current_node_id: self.current_node_id.clone(),
            }
        })?;

        let mut seen = HashSet::new();
        let mut eligible = Vec::new();

        for input_node_id in &current_node.input_node_ids {
            if let Some(input_node) = node_map.get(input_node_id.as_str()) {
                push_context_candidate(input_node, &mut seen, &mut eligible);
            }
        }

        for lineage_node_id in lineage {
            let lineage_node = node_map.get(lineage_node_id.as_str()).ok_or_else(|| {
                ValidationError::MissingCurrentNode {
                    current_node_id: lineage_node_id.clone(),
                }
            })?;

            if lineage_node.kind != NodeKind::Prompt {
                continue;
            }

            for child_id in &lineage_node.children_ids {
                if let Some(child_node) = node_map.get(child_id.as_str()) {
                    push_context_candidate(child_node, &mut seen, &mut eligible);
                }
            }
        }

        Ok(eligible)
    }
}

fn validate_current_node_candidate(node: &SessionNode) -> Result<(), ValidationError> {
    if !matches!(node.kind, NodeKind::Intent | NodeKind::Prompt) {
        return Err(ValidationError::InvalidCurrentNodeKind {
            node_id: node.id.clone(),
            kind: node.kind,
        });
    }

    if matches!(node.status, NodeStatus::Rejected | NodeStatus::Superseded) {
        return Err(ValidationError::InvalidCurrentNodeStatus {
            node_id: node.id.clone(),
            status: node.status,
        });
    }

    Ok(())
}

fn push_context_candidate(
    node: &SessionNode,
    seen: &mut HashSet<NodeId>,
    eligible: &mut Vec<NodeId>,
) {
    if node.kind != NodeKind::Artifact || node.status != NodeStatus::Accepted {
        return;
    }

    if seen.insert(node.id.clone()) {
        eligible.push(node.id.clone());
    }
}

#[must_use]
pub fn can_attach_child(parent_kind: NodeKind, child_kind: NodeKind) -> bool {
    matches!(
        (parent_kind, child_kind),
        (NodeKind::Intent, NodeKind::Prompt)
            | (NodeKind::Prompt, NodeKind::Prompt)
            | (NodeKind::Prompt, NodeKind::Artifact)
    )
}

pub fn validate_parent_child(
    parent_kind: NodeKind,
    child_kind: NodeKind,
) -> Result<(), ValidationError> {
    if can_attach_child(parent_kind, child_kind) {
        Ok(())
    } else {
        Err(ValidationError::InvalidParentChild {
            parent_kind,
            child_kind,
        })
    }
}

#[must_use]
pub fn is_valid_status_transition(from: NodeStatus, to: NodeStatus) -> bool {
    from.can_transition_to(to)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{
        can_attach_child, is_valid_status_transition, validate_parent_child, ArtifactKind, NodeId,
        NodeKind, NodeStatus, SessionId, SessionNode, SessionState, SessionTree, ValidationError,
    };

    fn node(id: &str, kind: NodeKind, parent_id: Option<&str>, children: &[&str]) -> SessionNode {
        SessionNode {
            id: id.into(),
            label: id.to_string(),
            kind,
            artifact_kind: (kind == NodeKind::Artifact).then_some(ArtifactKind::Implementation),
            status: NodeStatus::Proposed,
            parent_id: parent_id.map(NodeId::from),
            children_ids: children.iter().map(|child| NodeId::from(*child)).collect(),
            input_node_ids: Vec::new(),
            payload_ref: None,
        }
    }

    fn valid_session_tree() -> SessionTree {
        SessionTree {
            schema_version: 1,
            session_id: SessionId::from("session-1"),
            intent_node_id: NodeId::from("intent-1"),
            current_node_id: NodeId::from("prompt-2"),
            state: SessionState::Active,
            nodes: vec![
                node("intent-1", NodeKind::Intent, None, &["prompt-1"]),
                node(
                    "prompt-1",
                    NodeKind::Prompt,
                    Some("intent-1"),
                    &["prompt-2", "artifact-1"],
                ),
                node("prompt-2", NodeKind::Prompt, Some("prompt-1"), &[]),
                node("artifact-1", NodeKind::Artifact, Some("prompt-1"), &[]),
            ],
        }
    }

    #[test]
    fn allows_prompt_to_prompt_children() {
        assert!(can_attach_child(NodeKind::Prompt, NodeKind::Prompt));
        assert!(validate_parent_child(NodeKind::Prompt, NodeKind::Prompt).is_ok());
    }

    #[test]
    fn rejects_artifact_as_parent() {
        assert!(!can_attach_child(NodeKind::Artifact, NodeKind::Prompt));

        assert_eq!(
            validate_parent_child(NodeKind::Artifact, NodeKind::Prompt),
            Err(ValidationError::InvalidParentChild {
                parent_kind: NodeKind::Artifact,
                child_kind: NodeKind::Prompt,
            })
        );
    }

    #[test]
    fn validates_tree_with_prompt_to_prompt_path() {
        let session = valid_session_tree();

        assert!(session.validate_tree_invariants().is_ok());
    }

    #[test]
    fn rejects_tree_with_illegal_parent_child_link() {
        let mut session = valid_session_tree();

        {
            let prompt_2 = session
                .nodes
                .iter_mut()
                .find(|node| node.id == "prompt-2")
                .expect("prompt-2 should exist");
            prompt_2.parent_id = Some(NodeId::from("artifact-1"));
        }

        {
            let artifact_1 = session
                .nodes
                .iter_mut()
                .find(|node| node.id == "artifact-1")
                .expect("artifact-1 should exist");
            artifact_1.children_ids.push(NodeId::from("prompt-2"));
        }

        assert_eq!(
            session.validate_tree_invariants(),
            Err(ValidationError::InvalidParentChildLink {
                parent_id: NodeId::from("artifact-1"),
                parent_kind: NodeKind::Artifact,
                child_id: NodeId::from("prompt-2"),
                child_kind: NodeKind::Prompt,
            })
        );
    }

    #[test]
    fn rejects_tree_with_missing_reciprocal_parent_reference() {
        let mut session = valid_session_tree();

        let intent = session
            .nodes
            .iter_mut()
            .find(|node| node.id == "intent-1")
            .expect("intent should exist");
        intent.children_ids.clear();

        assert_eq!(
            session.validate_tree_invariants(),
            Err(ValidationError::ParentMissingChildReference {
                parent_id: NodeId::from("intent-1"),
                child_id: NodeId::from("prompt-1"),
            })
        );
    }

    #[test]
    fn allows_expected_status_transitions() {
        assert!(is_valid_status_transition(
            NodeStatus::Proposed,
            NodeStatus::Accepted
        ));
        assert!(is_valid_status_transition(
            NodeStatus::Accepted,
            NodeStatus::Superseded
        ));
        assert!(NodeStatus::Rejected
            .validate_transition(NodeStatus::Rejected)
            .is_ok());
    }

    #[test]
    fn rejects_invalid_status_transition() {
        assert!(!is_valid_status_transition(
            NodeStatus::Rejected,
            NodeStatus::Accepted
        ));

        assert_eq!(
            NodeStatus::Rejected.validate_transition(NodeStatus::Accepted),
            Err(ValidationError::InvalidStatusTransition {
                from: NodeStatus::Rejected,
                to: NodeStatus::Accepted,
            })
        );
        assert_eq!(
            NodeStatus::Proposed.validate_transition(NodeStatus::Superseded),
            Err(ValidationError::InvalidStatusTransition {
                from: NodeStatus::Proposed,
                to: NodeStatus::Superseded,
            })
        );
    }

    #[test]
    fn updates_current_node_when_next_node_is_prompt() {
        let mut session = valid_session_tree();

        session
            .set_current_node(NodeId::from("prompt-1"))
            .expect("prompt nodes should be valid current nodes");

        assert_eq!(session.current_node_id, NodeId::from("prompt-1"));
    }

    #[test]
    fn rejects_artifact_as_current_node() {
        let mut session = valid_session_tree();

        let err = session
            .set_current_node(NodeId::from("artifact-1"))
            .expect_err("artifacts should not be valid current nodes");

        assert_eq!(
            err,
            ValidationError::InvalidCurrentNodeKind {
                node_id: NodeId::from("artifact-1"),
                kind: NodeKind::Artifact,
            }
        );
    }

    #[test]
    fn computes_active_lineage_from_intent_to_current() {
        let session = valid_session_tree();

        let lineage = session
            .active_lineage_node_ids()
            .expect("lineage should be resolved");

        assert_eq!(
            lineage,
            vec![
                NodeId::from("intent-1"),
                NodeId::from("prompt-1"),
                NodeId::from("prompt-2"),
            ]
        );
    }

    #[test]
    fn resolves_eligible_context_nodes_with_accepted_sibling_artifacts() {
        let mut session = valid_session_tree();

        let prompt_2 = session
            .nodes
            .iter_mut()
            .find(|node| node.id == "prompt-2")
            .expect("prompt-2 should exist");
        prompt_2.parent_id = Some(NodeId::from("prompt-1"));
        prompt_2
            .input_node_ids
            .push(NodeId::from("artifact-sibling-accepted"));
        prompt_2
            .input_node_ids
            .push(NodeId::from("artifact-sibling-rejected"));

        let prompt_1 = session
            .nodes
            .iter_mut()
            .find(|node| node.id == "prompt-1")
            .expect("prompt-1 should exist");
        prompt_1
            .children_ids
            .push(NodeId::from("artifact-sibling-accepted"));
        prompt_1
            .children_ids
            .push(NodeId::from("artifact-sibling-rejected"));

        let artifact_1 = session
            .nodes
            .iter_mut()
            .find(|node| node.id == "artifact-1")
            .expect("artifact-1 should exist");
        artifact_1.status = NodeStatus::Accepted;

        session.nodes.push(SessionNode {
            id: NodeId::from("artifact-sibling-accepted"),
            label: String::from("artifact-sibling-accepted"),
            kind: NodeKind::Artifact,
            artifact_kind: Some(ArtifactKind::Context),
            status: NodeStatus::Accepted,
            parent_id: Some(NodeId::from("prompt-1")),
            children_ids: Vec::new(),
            input_node_ids: Vec::new(),
            payload_ref: None,
        });
        session.nodes.push(SessionNode {
            id: NodeId::from("artifact-sibling-rejected"),
            label: String::from("artifact-sibling-rejected"),
            kind: NodeKind::Artifact,
            artifact_kind: Some(ArtifactKind::Context),
            status: NodeStatus::Rejected,
            parent_id: Some(NodeId::from("prompt-1")),
            children_ids: Vec::new(),
            input_node_ids: Vec::new(),
            payload_ref: None,
        });

        let context_ids = session
            .resolve_eligible_context_node_ids()
            .expect("eligible context should resolve");

        assert_eq!(
            context_ids,
            vec![
                NodeId::from("artifact-sibling-accepted"),
                NodeId::from("artifact-1"),
            ]
        );
    }

    proptest! {
        #[test]
        fn property_valid_prompt_chains_satisfy_invariants(depth in 1usize..8) {
            let mut session = SessionTree::new("intent");
            session.session_id = SessionId::from("session-property");

            let mut parent_id = session.intent_node_id.clone();
            for index in 0..depth {
                let prompt_id = NodeId::from(format!("prompt-{index}"));
                let artifact_id = NodeId::from(format!("artifact-{index}"));

                let parent = session
                    .nodes
                    .iter_mut()
                    .find(|node| node.id == parent_id)
                    .expect("parent should exist");
                parent.children_ids.push(prompt_id.clone());

                session.nodes.push(SessionNode {
                    id: prompt_id.clone(),
                    label: format!("prompt-{index}"),
                    kind: NodeKind::Prompt,
                    artifact_kind: None,
                    status: NodeStatus::Accepted,
                    parent_id: Some(parent_id.clone()),
                    children_ids: vec![artifact_id.clone()],
                    input_node_ids: Vec::new(),
                    payload_ref: None,
                });
                session.nodes.push(SessionNode {
                    id: artifact_id,
                    label: format!("artifact-{index}"),
                    kind: NodeKind::Artifact,
                    artifact_kind: Some(ArtifactKind::Implementation),
                    status: NodeStatus::Accepted,
                    parent_id: Some(prompt_id.clone()),
                    children_ids: Vec::new(),
                    input_node_ids: Vec::new(),
                    payload_ref: None,
                });

                parent_id = prompt_id;
            }

            session.current_node_id = parent_id;
            prop_assert!(session.validate_tree_invariants().is_ok());
        }
    }
}
