use super::types::{validate_text, *};
use qilbee_core::{Error, Result};
use rocksdb::{DB, Direction, IteratorMode, WriteBatch, WriteOptions};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

const SCHEMA_KEY: &[u8] = b"\0qilbee-learning-schema";
const SCHEMA_VERSION: &[u8] = b"1";

struct Inner {
    db: DB,
    mutation_lock: Mutex<()>,
}

/// Local, durable procedure and evaluation ledger. Clones share one writer lock.
///
/// This API performs blocking disk I/O; use `spawn_blocking` from async servers.
/// The directory must be dedicated to learning data. Every acknowledged write
/// synchronizes the WAL. No LLM execution or network requests happen here.
#[derive(Clone)]
pub struct LearningMemory {
    inner: Arc<Inner>,
}

fn storage_error(error: rocksdb::Error) -> Error {
    Error::Storage(error.to_string())
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|error| Error::Serialization(error.to_string()))
}

fn decode<T: DeserializeOwned>(value: &[u8]) -> Result<T> {
    serde_json::from_slice(value).map_err(|error| Error::DataCorruption(error.to_string()))
}

fn write_options() -> WriteOptions {
    let mut options = WriteOptions::default();
    options.set_sync(true);
    options
}

fn append_component(key: &mut Vec<u8>, component: &str) {
    key.extend_from_slice(&(component.len() as u32).to_be_bytes());
    key.extend_from_slice(component.as_bytes());
}

fn scope_prefix(kind: u8, scope: &LearningScope) -> Vec<u8> {
    let mut key = vec![kind];
    for component in [&scope.tenant, &scope.agent, &scope.environment] {
        append_component(&mut key, component);
    }
    key
}

fn procedure_key(scope: &LearningScope, id: &str) -> Vec<u8> {
    let mut key = scope_prefix(1, scope);
    append_component(&mut key, id);
    key
}

fn evaluation_key(scope: &LearningScope, procedure: &str, case: &str) -> Vec<u8> {
    let mut key = scope_prefix(2, scope);
    append_component(&mut key, procedure);
    append_component(&mut key, case);
    key
}

fn evidence_key(scope: &LearningScope, procedure: &str, reference: &str) -> Vec<u8> {
    let mut key = scope_prefix(3, scope);
    append_component(&mut key, procedure);
    append_component(&mut key, reference);
    key
}

impl LearningMemory {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut options = rocksdb::Options::default();
        options.create_if_missing(true);
        let db = DB::open(&options, path).map_err(storage_error)?;
        match db.get(SCHEMA_KEY).map_err(storage_error)? {
            Some(version) if version.as_slice() == SCHEMA_VERSION => {}
            Some(_) => {
                return Err(Error::Configuration(
                    "Unsupported learning schema version".into(),
                ));
            }
            None => {
                if let Some(entry) = db.iterator(IteratorMode::Start).next() {
                    entry.map_err(storage_error)?;
                    return Err(Error::Configuration(
                        "Learning storage requires a dedicated database".into(),
                    ));
                }
                db.put_opt(SCHEMA_KEY, SCHEMA_VERSION, &write_options())
                    .map_err(storage_error)?;
            }
        }
        Ok(Self {
            inner: Arc::new(Inner {
                db,
                mutation_lock: Mutex::new(()),
            }),
        })
    }

    /// Register a candidate. Repeating the same proposal is idempotent; changing
    /// content, provenance, baseline or policy under an existing ID is rejected.
    pub fn propose(
        &self,
        scope: &LearningScope,
        proposal: ProcedureProposal,
    ) -> Result<ProcedureRecord> {
        scope.validate()?;
        proposal.validate()?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        if let Some(existing) = self.get(scope, &proposal.id)? {
            return if existing.proposal == proposal {
                Ok(existing)
            } else {
                Err(Error::ConstraintViolation(
                    "Procedure revisions are immutable; use a new ID".into(),
                ))
            };
        }
        let key = procedure_key(scope, &proposal.id);
        let record = new_procedure_record(scope, proposal);
        self.inner
            .db
            .put_opt(key, encode(&record)?, &write_options())
            .map_err(storage_error)?;
        Ok(record)
    }

    pub fn get(&self, scope: &LearningScope, id: &str) -> Result<Option<ProcedureRecord>> {
        scope.validate()?;
        validate_text(id, "procedure ID", 512)?;
        self.inner
            .db
            .get(procedure_key(scope, id))
            .map_err(storage_error)?
            .map(|bytes| decode(&bytes))
            .transpose()
    }

    /// Persist evidence and any state transition in one atomic write batch.
    /// Retries return the original receipt; conflicting reuses of a case fail.
    pub fn record_evaluation(
        &self,
        scope: &LearningScope,
        id: &str,
        evaluation: PairedEvaluation,
    ) -> Result<EvaluationResult> {
        scope.validate()?;
        validate_text(id, "procedure ID", 512)?;
        evaluation.validate()?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let (result, batch) = self.prepare_evaluation(scope, id, evaluation)?;
        if let Some(batch) = batch {
            self.inner
                .db
                .write_opt(batch, &write_options())
                .map_err(storage_error)?;
        }
        Ok(result)
    }

    // Caller holds mutation_lock; admission adds its receipt to this same batch.
    fn prepare_evaluation(
        &self,
        scope: &LearningScope,
        id: &str,
        evaluation: PairedEvaluation,
    ) -> Result<(EvaluationResult, Option<WriteBatch>)> {
        scope.validate()?;
        validate_text(id, "procedure ID", 512)?;
        evaluation.validate()?;
        let key = evaluation_key(scope, id, &evaluation.case_id);
        if let Some(bytes) = self.inner.db.get(&key).map_err(storage_error)? {
            let receipt: EvaluationReceipt = decode(&bytes)?;
            if receipt.evaluation != evaluation {
                return Err(Error::ConstraintViolation(
                    "Case ID already has different evidence".into(),
                ));
            }
            return Ok((
                EvaluationResult {
                    receipt,
                    duplicate: true,
                },
                None,
            ));
        }

        let mut record = self
            .get(scope, id)?
            .ok_or_else(|| Error::KeyNotFound("Procedure not found in this scope".into()))?;
        let policy = &record.proposal.policy;
        if evaluation.evaluator_id != policy.evaluator_id
            || evaluation.evaluation_contract != policy.evaluation_contract
        {
            return Err(Error::ValidationError(
                "Evaluation does not match the registered evaluator and contract".into(),
            ));
        }
        if record
            .proposal
            .source_refs
            .contains(&evaluation.evidence_ref)
        {
            return Err(Error::ValidationError(
                "Proposal evidence cannot also be held-out evaluation evidence".into(),
            ));
        }
        let source_key = evidence_key(scope, id, &evaluation.evidence_ref);
        if self
            .inner
            .db
            .get(&source_key)
            .map_err(storage_error)?
            .is_some()
        {
            return Err(Error::ConstraintViolation(
                "Evidence reference already used by another case".into(),
            ));
        }
        let phase_matches = matches!(
            (record.state, evaluation.phase),
            (ProcedureState::Candidate, EvaluationPhase::Qualification)
                | (ProcedureState::Active, EvaluationPhase::Monitoring)
        );
        if !phase_matches {
            return Err(Error::MemoryOperation(
                "Evaluation phase does not match procedure state".into(),
            ));
        }

        let now = chrono::Utc::now().timestamp_millis();
        record.apply(&evaluation, now)?;
        let receipt = EvaluationReceipt {
            evaluation,
            state_after: record.state,
            recorded_at_millis: now,
        };
        let mut batch = WriteBatch::default();
        batch.put(procedure_key(scope, id), encode(&record)?);
        batch.put(key, encode(&receipt)?);
        batch.put(source_key, receipt.evaluation.case_id.as_bytes());
        Ok((
            EvaluationResult {
                receipt,
                duplicate: false,
            },
            Some(batch),
        ))
    }

    pub fn evaluation(
        &self,
        scope: &LearningScope,
        procedure: &str,
        case: &str,
    ) -> Result<Option<EvaluationReceipt>> {
        scope.validate()?;
        validate_text(procedure, "procedure ID", 512)?;
        validate_text(case, "case ID", 512)?;
        self.inner
            .db
            .get(evaluation_key(scope, procedure, case))
            .map_err(storage_error)?
            .map(|bytes| decode(&bytes))
            .transpose()
    }

    /// Select an active procedure evaluated against the caller's exact baseline.
    /// None means use that baseline. Budget counts UTF-8 instruction bytes, not
    /// model tokens. Selection scans this scope; no scale claim is implied.
    pub fn select(
        &self,
        scope: &LearningScope,
        task: &str,
        baseline_revision: &str,
        evaluation_contract: &str,
        max_instruction_bytes: usize,
    ) -> Result<Option<ProcedureRecord>> {
        scope.validate()?;
        validate_text(task, "task", 512)?;
        validate_text(baseline_revision, "baseline revision", 512)?;
        validate_text(evaluation_contract, "evaluation contract", 512)?;
        let prefix = scope_prefix(1, scope);
        let mut best: Option<ProcedureRecord> = None;
        for item in self
            .inner
            .db
            .iterator(IteratorMode::From(&prefix, Direction::Forward))
        {
            let (key, bytes) = item.map_err(storage_error)?;
            if !key.starts_with(&prefix) {
                break;
            }
            let record: ProcedureRecord = decode(&bytes)?;
            if record.state != ProcedureState::Active
                || record.proposal.task != task
                || record.proposal.baseline_revision != baseline_revision
                || record.proposal.policy.evaluation_contract != evaluation_contract
                || record.proposal.instructions.len() > max_instruction_bytes
            {
                continue;
            }
            // Equal bounds resolve deterministically by procedure ID.
            let replace = best.as_ref().is_none_or(|current| {
                record.lower_improvement_bound > current.lower_improvement_bound
                    || (record.lower_improvement_bound == current.lower_improvement_bound
                        && record.proposal.id < current.proposal.id)
            });
            if replace {
                best = Some(record);
            }
        }
        Ok(best)
    }
}

impl ProcedureRecord {
    fn transition(
        &mut self,
        to: ProcedureState,
        reason: &str,
        evaluation: &PairedEvaluation,
        now: i64,
    ) {
        self.decisions.push(LearningDecision {
            from: self.state,
            to,
            reason: reason.into(),
            triggering_case: evaluation.case_id.clone(),
            recorded_at_millis: now,
        });
        self.state = to;
    }

    fn apply(&mut self, evaluation: &PairedEvaluation, now: i64) -> Result<()> {
        let policy = &self.proposal.policy;
        let within_budget = evaluation.candidate_cost_units <= policy.max_cost_units
            && evaluation.candidate_latency_ms <= policy.max_latency_ms;
        let improvement = evaluation.candidate_utility - evaluation.baseline_utility;
        match evaluation.phase {
            EvaluationPhase::Qualification => {
                self.qualification_count += 1;
                let n = f64::from(self.qualification_count);
                self.mean_improvement += (improvement - self.mean_improvement) / n;
                self.mean_candidate_utility +=
                    (evaluation.candidate_utility - self.mean_candidate_utility) / n;
                self.budget_violations += u32::from(!within_budget);
                if self.qualification_count == policy.qualification_trials {
                    // Paired differences lie in [-1, 1]. One fixed, one-sided
                    // Hoeffding bound: radius = sqrt(2 * ln(1/delta) / n).
                    // Assumes independent held-out cases. No early acceptance.
                    let lower =
                        self.mean_improvement - (-2.0 * policy.confidence_delta.ln() / n).sqrt();
                    self.lower_improvement_bound = Some(lower.max(-1.0));
                    let accepted = lower >= policy.min_improvement
                        && self.mean_candidate_utility >= policy.min_candidate_utility
                        && self.budget_violations == 0;
                    let (state, reason) = if accepted {
                        (
                            ProcedureState::Active,
                            "Fixed evaluation budget passed utility, improvement and resource gates",
                        )
                    } else {
                        (
                            ProcedureState::Rejected,
                            "Fixed evaluation budget failed one or more promotion gates",
                        )
                    };
                    self.transition(state, reason, evaluation, now);
                }
            }
            EvaluationPhase::Monitoring => {
                self.monitoring_count = self
                    .monitoring_count
                    .checked_add(1)
                    .ok_or_else(|| Error::MemoryOperation("Monitoring counter exhausted".into()))?;
                let good = within_budget
                    && evaluation.candidate_utility >= policy.min_candidate_utility
                    && improvement >= 0.0;
                self.failure_streak = if good { 0 } else { self.failure_streak + 1 };
                if self.failure_streak >= policy.max_failure_streak {
                    self.transition(
                        ProcedureState::Suspended,
                        "Monitoring failure streak reached policy limit; fall back to baseline",
                        evaluation,
                        now,
                    );
                }
            }
        }
        Ok(())
    }
}

pub mod bound;
pub mod registry;

fn new_procedure_record(scope: &LearningScope, proposal: ProcedureProposal) -> ProcedureRecord {
    ProcedureRecord {
        scope: scope.clone(),
        proposal,
        state: ProcedureState::Candidate,
        qualification_count: 0,
        mean_improvement: 0.0,
        mean_candidate_utility: 0.0,
        budget_violations: 0,
        lower_improvement_bound: None,
        monitoring_count: 0,
        failure_streak: 0,
        decisions: Vec::new(),
        created_at_millis: chrono::Utc::now().timestamp_millis(),
    }
}
pub mod admission;
pub mod tools;
