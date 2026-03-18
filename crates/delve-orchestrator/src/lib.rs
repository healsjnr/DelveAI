#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::error::Error;
use std::fmt;

use delve_domain::{
    ArtifactKind, NodeId, NodeKind, NodeStatus, SessionNode, SessionTree, ValidationError,
};
use delve_providers::{CompletionProvider, ProviderError, ProviderRequest, ProviderResponse};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextEntry {
    pub node_id: NodeId,
    pub text: String,
    pub estimated_tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPack {
    pub entries: Vec<ContextEntry>,
    pub used_tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptPackage {
    pub prompt: String,
    pub context: ContextPack,
    pub rendered_prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactProposal {
    pub artifact_kind: ArtifactKind,
    pub body: String,
}

pub trait ArtifactProposalGenerator {
    fn propose_artifacts(
        &self,
        prompt_package: &PromptPackage,
    ) -> Result<Vec<ArtifactProposal>, OrchestrationError>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewRubric {
    pub required_keywords: Vec<String>,
    pub confidence_threshold: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewResult {
    pub confidence: f32,
    pub matched_keywords: Vec<String>,
    pub missing_keywords: Vec<String>,
    pub accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrchestrationError {
    DomainValidation(ValidationError),
    InvalidTokenBudget,
    UnknownInputNode(NodeId),
    IneligibleInputNode(NodeId),
    InvalidReviewRubric(String),
}

impl fmt::Display for OrchestrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DomainValidation(err) => write!(f, "domain validation error: {err:?}"),
            Self::InvalidTokenBudget => f.write_str("token budget must be greater than zero"),
            Self::UnknownInputNode(node_id) => {
                write!(f, "input context node '{node_id}' was not found")
            }
            Self::IneligibleInputNode(node_id) => {
                write!(
                    f,
                    "input context node '{node_id}' is not an accepted artifact"
                )
            }
            Self::InvalidReviewRubric(reason) => write!(f, "invalid review rubric: {reason}"),
        }
    }
}

impl Error for OrchestrationError {}

impl From<ValidationError> for OrchestrationError {
    fn from(value: ValidationError) -> Self {
        Self::DomainValidation(value)
    }
}

pub fn resolve_context_node_ids(
    session: &SessionTree,
    input_node_ids: &[NodeId],
) -> Result<Vec<NodeId>, OrchestrationError> {
    let mut seen = HashSet::new();
    let mut resolved = Vec::new();

    for input_node_id in input_node_ids {
        let Some(node) = find_node(session, input_node_id) else {
            return Err(OrchestrationError::UnknownInputNode(input_node_id.clone()));
        };

        if !is_context_eligible(node) {
            return Err(OrchestrationError::IneligibleInputNode(
                input_node_id.clone(),
            ));
        }

        if seen.insert(node.id.clone()) {
            resolved.push(node.id.clone());
        }
    }

    for lineage_context_node_id in session.resolve_eligible_context_node_ids()? {
        if seen.insert(lineage_context_node_id.clone()) {
            resolved.push(lineage_context_node_id);
        }
    }

    Ok(resolved)
}

pub fn pack_context(
    session: &SessionTree,
    context_node_ids: &[NodeId],
    token_budget: usize,
) -> Result<ContextPack, OrchestrationError> {
    if token_budget == 0 {
        return Err(OrchestrationError::InvalidTokenBudget);
    }

    let mut entries = Vec::new();
    let mut used_tokens = 0;

    for context_node_id in context_node_ids {
        let Some(node) = find_node(session, context_node_id) else {
            return Err(OrchestrationError::UnknownInputNode(
                context_node_id.clone(),
            ));
        };

        // Exclude rejected/superseded artifacts from packed context even when explicitly referenced.
        if !is_context_eligible(node) {
            continue;
        }

        let text = format!("[{}] {}", node.id, node.label);
        let estimated_tokens = estimate_tokens(&text);
        if used_tokens + estimated_tokens > token_budget {
            continue;
        }

        entries.push(ContextEntry {
            node_id: node.id.clone(),
            text,
            estimated_tokens,
        });
        used_tokens += estimated_tokens;
    }

    Ok(ContextPack {
        entries,
        used_tokens,
    })
}

pub fn build_prompt_package(
    session: &SessionTree,
    prompt: impl Into<String>,
    input_node_ids: &[NodeId],
    token_budget: usize,
) -> Result<PromptPackage, OrchestrationError> {
    let prompt = prompt.into();
    let context_node_ids = resolve_context_node_ids(session, input_node_ids)?;
    let context = pack_context(session, &context_node_ids, token_budget)?;
    let rendered_prompt = render_prompt_with_context(&prompt, &context);

    Ok(PromptPackage {
        prompt,
        context,
        rendered_prompt,
    })
}

pub fn parse_review_rubric(rubric_json: &str) -> Result<ReviewRubric, OrchestrationError> {
    let rubric: ReviewRubric = serde_json::from_str(rubric_json)
        .map_err(|err| OrchestrationError::InvalidReviewRubric(err.to_string()))?;

    if !(0.0..=1.0).contains(&rubric.confidence_threshold) {
        return Err(OrchestrationError::InvalidReviewRubric(String::from(
            "confidence_threshold must be between 0.0 and 1.0",
        )));
    }

    Ok(rubric)
}

#[must_use]
pub fn execute_review(rubric: &ReviewRubric, artifact_body: &str) -> ReviewResult {
    let artifact_body_lower = artifact_body.to_lowercase();
    let mut matched_keywords = Vec::new();
    let mut missing_keywords = Vec::new();

    for keyword in &rubric.required_keywords {
        if artifact_body_lower.contains(&keyword.to_lowercase()) {
            matched_keywords.push(keyword.clone());
        } else {
            missing_keywords.push(keyword.clone());
        }
    }

    let confidence = if rubric.required_keywords.is_empty() {
        1.0
    } else {
        matched_keywords.len() as f32 / rubric.required_keywords.len() as f32
    };

    ReviewResult {
        confidence,
        matched_keywords,
        missing_keywords,
        accepted: passes_confidence_threshold(confidence, rubric.confidence_threshold),
    }
}

#[must_use]
pub fn passes_confidence_threshold(confidence: f32, threshold: f32) -> bool {
    confidence >= threshold
}

pub fn generate_artifact<P>(
    provider: &P,
    prompt: impl Into<String>,
) -> Result<ProviderResponse, ProviderError>
where
    P: CompletionProvider + ?Sized,
{
    let request = ProviderRequest {
        prompt: prompt.into(),
        thread_id: None,
    };
    provider.generate(&request)
}

pub fn generate_artifact_with_thread<P>(
    provider: &P,
    prompt: impl Into<String>,
    thread_id: impl Into<String>,
) -> Result<ProviderResponse, ProviderError>
where
    P: CompletionProvider + ?Sized,
{
    let request = ProviderRequest {
        prompt: prompt.into(),
        thread_id: Some(thread_id.into()),
    };
    provider.generate(&request)
}

pub fn generate_artifact_streaming<P>(
    provider: &P,
    prompt: impl Into<String>,
    on_chunk: &mut dyn FnMut(&str),
) -> Result<ProviderResponse, ProviderError>
where
    P: CompletionProvider + ?Sized,
{
    let request = ProviderRequest {
        prompt: prompt.into(),
        thread_id: None,
    };
    provider.generate_streaming(&request, on_chunk)
}

pub fn generate_artifact_streaming_with_thread<P>(
    provider: &P,
    prompt: impl Into<String>,
    thread_id: impl Into<String>,
    on_chunk: &mut dyn FnMut(&str),
) -> Result<ProviderResponse, ProviderError>
where
    P: CompletionProvider + ?Sized,
{
    let request = ProviderRequest {
        prompt: prompt.into(),
        thread_id: Some(thread_id.into()),
    };
    provider.generate_streaming(&request, on_chunk)
}

fn find_node<'a>(session: &'a SessionTree, node_id: &NodeId) -> Option<&'a SessionNode> {
    session.nodes.iter().find(|node| node.id == *node_id)
}

fn is_context_eligible(node: &SessionNode) -> bool {
    node.kind == NodeKind::Artifact && node.status == NodeStatus::Accepted
}

fn estimate_tokens(text: &str) -> usize {
    text.split_whitespace().count().max(1)
}

fn render_prompt_with_context(prompt: &str, context: &ContextPack) -> String {
    if context.entries.is_empty() {
        return prompt.to_string();
    }

    let context_lines = context
        .entries
        .iter()
        .map(|entry| format!("- {}", entry.text))
        .collect::<Vec<_>>()
        .join("\n");

    format!("Context:\n{context_lines}\n\nPrompt:\n{prompt}")
}

#[cfg(test)]
mod tests {
    use delve_domain::{NodeId, NodeStatus, SessionId, SessionNode, SessionTree};
    use delve_providers::{CompletionProvider, ProviderError, ProviderKind};

    use super::{
        build_prompt_package, execute_review, generate_artifact, generate_artifact_streaming,
        parse_review_rubric, resolve_context_node_ids, ArtifactKind, OrchestrationError,
        ReviewRubric,
    };

    #[test]
    fn context_resolution_merges_inputs_with_lineage_context() {
        let session = session_fixture();

        let resolved =
            resolve_context_node_ids(&session, &[NodeId::from("artifact-sibling-accepted")])
                .expect("context should resolve");

        assert_eq!(
            resolved,
            vec![
                NodeId::from("artifact-sibling-accepted"),
                NodeId::from("artifact-accepted"),
            ]
        );
    }

    #[test]
    fn context_packer_respects_budget_and_excludes_rejected_nodes() {
        let session = session_fixture();

        let package = build_prompt_package(
            &session,
            "Continue work",
            &[NodeId::from("artifact-sibling-accepted")],
            4,
        )
        .expect("prompt package should build");

        assert_eq!(package.context.entries.len(), 1);
        assert_eq!(
            package.context.entries[0].node_id,
            NodeId::from("artifact-sibling-accepted")
        );
        assert!(
            !package
                .rendered_prompt
                .contains("artifact-sibling-rejected"),
            "rejected artifacts should be excluded from packed context"
        );
    }

    #[test]
    fn review_pipeline_parses_rubric_and_evaluates_confidence() {
        let rubric = parse_review_rubric(
            r#"{"required_keywords":["cli","tests"],"confidence_threshold":0.5}"#,
        )
        .expect("rubric should parse");

        let result = execute_review(&rubric, "This artifact adds CLI behavior and tests.");

        assert!(result.accepted);
        assert_eq!(result.missing_keywords.len(), 0);
        assert!(result.confidence >= 1.0);
    }

    #[test]
    fn end_to_end_mock_provider_flow_works_with_prompt_package_and_review() {
        let session = session_fixture();
        let package = build_prompt_package(
            &session,
            "Create implementation artifact",
            &[NodeId::from("artifact-sibling-accepted")],
            64,
        )
        .expect("prompt package should build");

        let provider = MockProvider {
            output: String::from("Implemented cli tests and updated docs"),
        };
        let response = generate_artifact(&provider, &package.rendered_prompt)
            .expect("provider should succeed");

        let rubric = ReviewRubric {
            required_keywords: vec![String::from("cli"), String::from("tests")],
            confidence_threshold: 0.8,
        };
        let review = execute_review(&rubric, &response.output);

        assert!(review.accepted);
    }

    #[test]
    fn provider_errors_are_propagated_for_non_streaming_and_streaming() {
        let provider = FailingProvider;

        let non_streaming = generate_artifact(&provider, "prompt");
        assert!(non_streaming.is_err());

        let mut seen_chunks = String::new();
        let streaming = generate_artifact_streaming(&provider, "prompt", &mut |chunk| {
            seen_chunks.push_str(chunk);
        });
        assert!(streaming.is_err());
        assert!(seen_chunks.is_empty());
    }

    #[test]
    fn invalid_rubric_threshold_is_rejected() {
        let result =
            parse_review_rubric(r#"{"required_keywords":["foo"],"confidence_threshold":1.1}"#);

        assert!(matches!(
            result,
            Err(OrchestrationError::InvalidReviewRubric(_))
        ));
    }

    fn session_fixture() -> SessionTree {
        let mut session = SessionTree::new("Intent");
        session.session_id = SessionId::from("session-orchestrator");

        let intent = session
            .nodes
            .iter_mut()
            .find(|node| node.id == "intent-root")
            .expect("intent should exist");
        intent.children_ids.push(NodeId::from("prompt-1"));

        session.nodes.push(SessionNode {
            id: NodeId::from("prompt-1"),
            label: String::from("prompt 1"),
            kind: delve_domain::NodeKind::Prompt,
            artifact_kind: None,
            status: NodeStatus::Accepted,
            parent_id: Some(NodeId::from("intent-root")),
            children_ids: vec![
                NodeId::from("prompt-2"),
                NodeId::from("artifact-accepted"),
                NodeId::from("artifact-sibling-accepted"),
                NodeId::from("artifact-sibling-rejected"),
            ],
            input_node_ids: Vec::new(),
            payload_ref: None,
        });

        session.nodes.push(SessionNode {
            id: NodeId::from("prompt-2"),
            label: String::from("prompt 2"),
            kind: delve_domain::NodeKind::Prompt,
            artifact_kind: None,
            status: NodeStatus::Accepted,
            parent_id: Some(NodeId::from("prompt-1")),
            children_ids: Vec::new(),
            input_node_ids: vec![NodeId::from("artifact-sibling-accepted")],
            payload_ref: None,
        });

        session.nodes.push(SessionNode {
            id: NodeId::from("artifact-accepted"),
            label: String::from("artifact accepted"),
            kind: delve_domain::NodeKind::Artifact,
            artifact_kind: Some(ArtifactKind::Implementation),
            status: NodeStatus::Accepted,
            parent_id: Some(NodeId::from("prompt-1")),
            children_ids: Vec::new(),
            input_node_ids: Vec::new(),
            payload_ref: None,
        });

        session.nodes.push(SessionNode {
            id: NodeId::from("artifact-sibling-accepted"),
            label: String::from("sibling accepted"),
            kind: delve_domain::NodeKind::Artifact,
            artifact_kind: Some(ArtifactKind::Context),
            status: NodeStatus::Accepted,
            parent_id: Some(NodeId::from("prompt-1")),
            children_ids: Vec::new(),
            input_node_ids: Vec::new(),
            payload_ref: None,
        });

        session.nodes.push(SessionNode {
            id: NodeId::from("artifact-sibling-rejected"),
            label: String::from("sibling rejected"),
            kind: delve_domain::NodeKind::Artifact,
            artifact_kind: Some(ArtifactKind::Context),
            status: NodeStatus::Rejected,
            parent_id: Some(NodeId::from("prompt-1")),
            children_ids: Vec::new(),
            input_node_ids: Vec::new(),
            payload_ref: None,
        });

        session.current_node_id = NodeId::from("prompt-2");
        session
            .validate_tree_invariants()
            .expect("fixture should be valid");
        session
    }

    struct MockProvider {
        output: String,
    }

    impl CompletionProvider for MockProvider {
        fn generate(
            &self,
            _request: &delve_providers::ProviderRequest,
        ) -> Result<delve_providers::ProviderResponse, ProviderError> {
            Ok(delve_providers::ProviderResponse {
                output: self.output.clone(),
                thread_id: None,
            })
        }
    }

    struct FailingProvider;

    impl CompletionProvider for FailingProvider {
        fn generate(
            &self,
            _request: &delve_providers::ProviderRequest,
        ) -> Result<delve_providers::ProviderResponse, ProviderError> {
            Err(ProviderError::CommandExecutionFailed {
                provider: ProviderKind::Echo,
                error_message: String::from("timeout"),
            })
        }
    }
}
