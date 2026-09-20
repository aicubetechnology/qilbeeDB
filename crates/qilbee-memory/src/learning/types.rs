//! Durable contracts for outcome-driven procedural memory.

use qilbee_core::{Error, Result};
use serde::{Deserialize, Serialize};

/// Exact namespace. Environment must identify model, tools and task version.
/// These labels partition records; callers must enforce authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearningScope {
    pub tenant: String,
    pub agent: String,
    pub environment: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LearningPolicy {
    /// One fixed sample size. No early acceptance or repeated significance tests.
    pub qualification_trials: u32,
    /// One-sided Hoeffding error budget for this proposal only.
    pub confidence_delta: f64,
    pub min_improvement: f64,
    pub min_candidate_utility: f64,
    /// Hard per-trial limits, also checked during monitoring.
    pub max_cost_units: u64,
    pub max_latency_ms: u64,
    /// Consecutive bad monitoring trials trigger suspension (operational rule).
    pub max_failure_streak: u32,
    /// Identity must be supplied by a trusted evaluator integration.
    pub evaluator_id: String,
    /// Version of dataset, rubric, model and evaluation protocol.
    pub evaluation_contract: String,
}

impl Default for LearningPolicy {
    fn default() -> Self {
        Self {
            qualification_trials: 128,
            confidence_delta: 0.01,
            min_improvement: 0.05,
            min_candidate_utility: 0.7,
            max_cost_units: 10_000,
            max_latency_ms: 60_000,
            max_failure_streak: 3,
            evaluator_id: String::new(),
            evaluation_contract: String::new(),
        }
    }
}

/// Immutable proposal. A revision requires a new ID and new evaluation cases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcedureProposal {
    pub id: String,
    pub task: String,
    pub baseline_revision: String,
    pub instructions: String,
    /// Evidence identifiers resolved and authenticated by the host application.
    pub source_refs: Vec<String>,
    pub policy: LearningPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcedureState {
    Candidate,
    Active,
    Rejected,
    Suspended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvaluationPhase {
    Qualification,
    Monitoring,
}

/// Both scores must come from the same held-out case, rubric and environment.
/// Score truth and independence cannot be established by a database alone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairedEvaluation {
    /// Unique case ID, shared across phases to prevent replay as new evidence.
    pub case_id: String,
    pub phase: EvaluationPhase,
    pub evaluator_id: String,
    pub evaluation_contract: String,
    pub evidence_ref: String,
    pub baseline_utility: f64,
    pub candidate_utility: f64,
    pub candidate_cost_units: u64,
    pub candidate_latency_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LearningDecision {
    pub from: ProcedureState,
    pub to: ProcedureState,
    pub reason: String,
    pub triggering_case: String,
    pub recorded_at_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcedureRecord {
    pub scope: LearningScope,
    pub proposal: ProcedureProposal,
    pub state: ProcedureState,
    pub qualification_count: u32,
    pub mean_improvement: f64,
    pub mean_candidate_utility: f64,
    pub budget_violations: u32,
    pub lower_improvement_bound: Option<f64>,
    pub monitoring_count: u64,
    pub failure_streak: u32,
    pub decisions: Vec<LearningDecision>,
    pub created_at_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationReceipt {
    pub evaluation: PairedEvaluation,
    pub state_after: ProcedureState,
    pub recorded_at_millis: i64,
}

#[derive(Debug, Clone)]
pub struct EvaluationResult {
    /// Stable receipt from the original write, including on duplicate delivery.
    pub receipt: EvaluationReceipt,
    pub duplicate: bool,
}

pub(super) fn validate_text(value: &str, name: &str, limit: usize) -> Result<()> {
    if value.trim().is_empty() || value.len() > limit {
        return Err(Error::ValidationError(format!(
            "{name} must contain 1..={limit} bytes"
        )));
    }
    Ok(())
}

pub(super) fn unit_interval(value: f64, name: &str) -> Result<()> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(Error::ValidationError(format!(
            "{name} must be finite and in [0, 1]"
        )));
    }
    Ok(())
}

impl LearningScope {
    pub(super) fn validate(&self) -> Result<()> {
        validate_text(&self.tenant, "tenant", 512)?;
        validate_text(&self.agent, "agent", 512)?;
        validate_text(&self.environment, "environment", 512)
    }
}

impl ProcedureProposal {
    pub(super) fn validate(&self) -> Result<()> {
        validate_text(&self.id, "procedure ID", 512)?;
        validate_text(&self.task, "task", 512)?;
        validate_text(&self.baseline_revision, "baseline revision", 512)?;
        validate_text(&self.instructions, "instructions", 64 * 1024)?;
        if self.source_refs.is_empty() || self.source_refs.len() > 128 {
            return Err(Error::ValidationError(
                "Supply 1..=128 source references".into(),
            ));
        }
        for reference in &self.source_refs {
            validate_text(reference, "source reference", 2048)?;
        }
        let policy = &self.policy;
        if !(2..=1_000_000).contains(&policy.qualification_trials)
            || policy.max_failure_streak == 0
            || policy.max_failure_streak > 1_000_000
        {
            return Err(Error::ValidationError(
                "Invalid evaluation or failure budget".into(),
            ));
        }
        unit_interval(policy.confidence_delta, "confidence_delta")?;
        if policy.confidence_delta <= 0.0 || policy.confidence_delta >= 0.5 {
            return Err(Error::ValidationError(
                "confidence_delta must be in (0, 0.5)".into(),
            ));
        }
        unit_interval(policy.min_improvement, "min_improvement")?;
        unit_interval(policy.min_candidate_utility, "min_candidate_utility")?;
        validate_text(&policy.evaluator_id, "evaluator ID", 512)?;
        validate_text(&policy.evaluation_contract, "evaluation contract", 512)
    }
}

impl PairedEvaluation {
    pub(super) fn validate(&self) -> Result<()> {
        validate_text(&self.case_id, "case ID", 512)?;
        validate_text(&self.evaluator_id, "evaluator ID", 512)?;
        validate_text(&self.evaluation_contract, "evaluation contract", 512)?;
        validate_text(&self.evidence_ref, "evidence reference", 2048)?;
        unit_interval(self.baseline_utility, "baseline utility")?;
        unit_interval(self.candidate_utility, "candidate utility")
    }
}
