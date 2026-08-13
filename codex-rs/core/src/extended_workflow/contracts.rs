use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowEvidence {
    pub file: String,
    pub symbol: String,
    pub explanation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpertResult {
    pub subject: String,
    pub summary: String,
    pub findings: Vec<String>,
    pub recommendations: Vec<String>,
    pub evidence: Vec<WorkflowEvidence>,
    pub uncertainties: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowTicket {
    pub title: String,
    pub goal: String,
    pub context: String,
    pub requirements: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub dependencies: Vec<String>,
    pub test_and_validation: Vec<String>,
    pub risks_or_open_questions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowConcept {
    pub title: String,
    pub objective: String,
    pub context: String,
    pub decisions: Vec<String>,
    pub alternatives: Vec<String>,
    pub risks_or_open_questions: Vec<String>,
    pub tickets: Vec<WorkflowTicket>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Pass,
    NeedsChanges,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FindingSeverity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingImpact {
    Local,
    Material,
    SystemWide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingLikelihood {
    Unlikely,
    Plausible,
    Expected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingRecoverability {
    Easy,
    Costly,
    Irreversible,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewerFinding {
    pub severity: FindingSeverity,
    pub impact: FindingImpact,
    pub likelihood: FindingLikelihood,
    pub recoverability: FindingRecoverability,
    pub rationale: String,
    pub file: String,
    pub symbol: String,
    pub recommended_fix: String,
    pub missing_tests: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewerResult {
    pub subject: String,
    pub verdict: ReviewVerdict,
    pub summary: String,
    pub findings: Vec<ReviewerFinding>,
    pub evidence: Vec<WorkflowEvidence>,
    pub missing_tests: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractValidation {
    pub valid: bool,
    pub errors: Vec<String>,
    pub has_high_or_critical: bool,
}

pub fn severity_for_dimensions(
    impact: FindingImpact,
    likelihood: FindingLikelihood,
    recoverability: FindingRecoverability,
) -> FindingSeverity {
    let score = enum_score(impact) + enum_score(likelihood) + enum_score(recoverability);
    match score {
        0..=2 => FindingSeverity::Low,
        3..=4 => FindingSeverity::Medium,
        5..=6 => FindingSeverity::High,
        _ => FindingSeverity::Critical,
    }
}

fn enum_score(value: impl IntoSeverityScore) -> u8 {
    value.severity_score()
}

trait IntoSeverityScore {
    fn severity_score(self) -> u8;
}

impl IntoSeverityScore for FindingImpact {
    fn severity_score(self) -> u8 {
        match self {
            Self::Local => 0,
            Self::Material => 2,
            Self::SystemWide => 3,
        }
    }
}

impl IntoSeverityScore for FindingLikelihood {
    fn severity_score(self) -> u8 {
        match self {
            Self::Unlikely => 0,
            Self::Plausible => 1,
            Self::Expected => 3,
        }
    }
}

impl IntoSeverityScore for FindingRecoverability {
    fn severity_score(self) -> u8 {
        match self {
            Self::Easy => 0,
            Self::Costly => 1,
            Self::Irreversible => 3,
        }
    }
}

pub fn validate_expert_result(result: &ExpertResult, expected_subject: &str) -> ContractValidation {
    let mut errors = Vec::new();
    validate_subject(&mut errors, &result.subject, expected_subject);
    require_text(&mut errors, "summary", &result.summary);
    require_text_items(&mut errors, "findings", &result.findings);
    require_text_items(&mut errors, "recommendations", &result.recommendations);
    require_evidence(&mut errors, &result.evidence);
    require_text_items(&mut errors, "uncertainties", &result.uncertainties);
    reject_contradictions(&mut errors, &result.summary, &result.findings);
    ContractValidation {
        valid: errors.is_empty(),
        errors,
        has_high_or_critical: false,
    }
}

pub fn validate_reviewer_result(
    result: &ReviewerResult,
    expected_subject: &str,
) -> ContractValidation {
    let mut errors = Vec::new();
    validate_subject(&mut errors, &result.subject, expected_subject);
    require_text(&mut errors, "summary", &result.summary);
    if result.findings.is_empty() && result.verdict != ReviewVerdict::Pass {
        errors.push("a non-pass verdict must contain at least one finding".to_string());
    }
    for (index, finding) in result.findings.iter().enumerate() {
        require_text(
            &mut errors,
            &format!("findings[{index}].rationale"),
            &finding.rationale,
        );
        require_text(
            &mut errors,
            &format!("findings[{index}].file"),
            &finding.file,
        );
        require_text(
            &mut errors,
            &format!("findings[{index}].symbol"),
            &finding.symbol,
        );
        require_text(
            &mut errors,
            &format!("findings[{index}].recommended_fix"),
            &finding.recommended_fix,
        );
        validate_optional_text_items(
            &mut errors,
            &format!("findings[{index}].missing_tests"),
            &finding.missing_tests,
        );
        let expected =
            severity_for_dimensions(finding.impact, finding.likelihood, finding.recoverability);
        if finding.severity != expected {
            errors.push(format!(
                "findings[{index}].severity must be {expected:?} for the supplied impact, likelihood, and recoverability dimensions"
            ));
        }
    }
    require_evidence(&mut errors, &result.evidence);
    validate_optional_text_items(&mut errors, "missing_tests", &result.missing_tests);
    reject_contradictions(
        &mut errors,
        &result.summary,
        &result
            .findings
            .iter()
            .map(|finding| finding.rationale.clone())
            .collect::<Vec<_>>(),
    );
    let has_high_or_critical = result
        .findings
        .iter()
        .any(|finding| finding.severity >= FindingSeverity::High);
    if result.verdict == ReviewVerdict::Pass && has_high_or_critical {
        errors.push(
            "a pass verdict cannot contain High or Critical findings; use needs_changes or blocked"
                .to_string(),
        );
    }
    ContractValidation {
        valid: errors.is_empty(),
        errors,
        has_high_or_critical,
    }
}

pub fn validate_ticket(ticket: &WorkflowTicket, expected_subject: &str) -> ContractValidation {
    let mut errors = Vec::new();
    require_text(&mut errors, "title", &ticket.title);
    require_text(&mut errors, "goal", &ticket.goal);
    require_text(&mut errors, "context", &ticket.context);
    require_text_items(&mut errors, "requirements", &ticket.requirements);
    require_text_items(
        &mut errors,
        "acceptance_criteria",
        &ticket.acceptance_criteria,
    );
    validate_subject(
        &mut errors,
        &format!("{} {}", ticket.title, ticket.goal),
        expected_subject,
    );
    require_text_items(
        &mut errors,
        "test_and_validation",
        &ticket.test_and_validation,
    );
    require_text_items(&mut errors, "dependencies", &ticket.dependencies);
    require_text_items(
        &mut errors,
        "risks_or_open_questions",
        &ticket.risks_or_open_questions,
    );
    ContractValidation {
        valid: errors.is_empty(),
        errors,
        has_high_or_critical: false,
    }
}

pub fn validate_concept(concept: &WorkflowConcept, expected_subject: &str) -> ContractValidation {
    let mut errors = Vec::new();
    require_text(&mut errors, "title", &concept.title);
    require_text(&mut errors, "objective", &concept.objective);
    require_text(&mut errors, "context", &concept.context);
    require_text_items(&mut errors, "decisions", &concept.decisions);
    require_text_items(&mut errors, "alternatives", &concept.alternatives);
    require_text_items(
        &mut errors,
        "risks_or_open_questions",
        &concept.risks_or_open_questions,
    );
    validate_subject(
        &mut errors,
        &format!("{} {}", concept.title, concept.objective),
        expected_subject,
    );
    for (index, ticket) in concept.tickets.iter().enumerate() {
        let validation = validate_ticket(ticket, expected_subject);
        errors.extend(
            validation
                .errors
                .into_iter()
                .map(|error| format!("tickets[{index}].{error}")),
        );
    }
    ContractValidation {
        valid: errors.is_empty(),
        errors,
        has_high_or_critical: false,
    }
}

fn validate_subject(errors: &mut Vec<String>, subject: &str, expected_subject: &str) {
    require_text(errors, "subject", subject);
    let subject = subject.trim();
    if !subject.is_empty() && subject.chars().count() < 3 {
        errors.push("subject must contain at least three characters".to_string());
        return;
    }
    if !expected_subject.trim().is_empty() {
        let subject_terms = relevance_terms(subject);
        let expected_terms = relevance_terms(expected_subject);
        if subject_terms.is_empty()
            || expected_terms.is_empty()
            || subject_terms.is_disjoint(&expected_terms)
        {
            errors.push("subject is not relevant to the assigned workflow task".to_string());
        }
    }
}

fn relevance_terms(value: &str) -> std::collections::HashSet<String> {
    const STOP_WORDS: &[&str] = &[
        "and", "the", "for", "with", "und", "der", "die", "das", "ein", "eine", "this", "that",
        "from", "return", "result", "contract", "json", "object",
    ];
    value
        .split(|character: char| {
            !character.is_alphanumeric() && character != '_' && character != '-'
        })
        .map(str::to_lowercase)
        .filter(|term| term.chars().count() >= 3)
        .filter(|term| !STOP_WORDS.contains(&term.as_str()))
        .collect()
}

fn require_text(errors: &mut Vec<String>, field: &str, value: &str) {
    if value.trim().is_empty() {
        errors.push(format!("{field} must not be empty"));
    }
}

fn require_text_items(errors: &mut Vec<String>, field: &str, values: &[String]) {
    if values.is_empty() {
        errors.push(format!("{field} must contain at least one item"));
    }
    validate_optional_text_items(errors, field, values);
}

fn validate_optional_text_items(errors: &mut Vec<String>, field: &str, values: &[String]) {
    for (index, value) in values.iter().enumerate() {
        require_text(errors, &format!("{field}[{index}]"), value);
    }
}

fn require_evidence(errors: &mut Vec<String>, evidence: &[WorkflowEvidence]) {
    if evidence.is_empty() {
        errors.push("evidence must contain at least one file and symbol reference".to_string());
    }
    for (index, item) in evidence.iter().enumerate() {
        require_text(errors, &format!("evidence[{index}].file"), &item.file);
        require_text(errors, &format!("evidence[{index}].symbol"), &item.symbol);
        require_text(
            errors,
            &format!("evidence[{index}].explanation"),
            &item.explanation,
        );
    }
}

fn reject_contradictions(errors: &mut Vec<String>, summary: &str, details: &[String]) {
    let summary = summary.to_lowercase();
    let details = details.join(" ").to_lowercase();
    let claims_no_issue = summary.contains("no finding")
        || summary.contains("keine findings")
        || summary.contains("no issue");
    let claims_issue = details.contains("critical")
        || details.contains("blocker")
        || details.contains("incorrect")
        || details.contains("fehler");
    if claims_no_issue && claims_issue {
        errors.push("summary contradicts the detailed findings".to_string());
    }
}
