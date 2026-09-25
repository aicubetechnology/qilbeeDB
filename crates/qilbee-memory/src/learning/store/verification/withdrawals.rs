//! Audit both authoritative copies without rewriting any historical evidence.
use super::super::experience_withdrawal::{
    ExperienceReuseBudget, StoredWithdrawal, command_key, target_key,
};
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceWithdrawalVerification {
    pub withdrawals: u64,
    pub idempotency_bindings: u64,
}

impl LearningMemory {
    /// Verify exact storage keys, receipt digests, byte-identical reciprocal copies,
    /// and the retained historical event behind every withdrawal. This requires a
    /// stopped store or the caller's retained writer exclusion, like inventory.
    pub fn verify_experience_withdrawals(&self) -> Result<ExperienceWithdrawalVerification> {
        let mut summary = ExperienceWithdrawalVerification {
            withdrawals: 0,
            idempotency_bindings: 0,
        };
        for family in [24u8, 25u8] {
            for item in self
                .inner
                .db
                .iterator(IteratorMode::From(&[family], Direction::Forward))
            {
                let (key, bytes) = item.map_err(storage_error)?;
                if key.first() != Some(&family) {
                    break;
                }
                let stored: StoredWithdrawal = decode(&bytes)?;
                stored.validate(
                    &stored.tenant,
                    &stored.namespace,
                    &stored.command.observation,
                )?;
                let target = target_key(
                    &stored.tenant,
                    &stored.namespace,
                    &stored.command.observation,
                )
                .map_err(|_| withdrawal_corruption())?;
                let command = command_key(
                    &stored.tenant,
                    &stored.namespace,
                    &stored.receipt.actor,
                    &stored.command.idempotency_key,
                )
                .map_err(|_| withdrawal_corruption())?;
                let (expected, counterpart) = if family == 24 {
                    (&target, &command)
                } else {
                    (&command, &target)
                };
                if key.as_ref() != expected.as_slice() {
                    return Err(withdrawal_corruption());
                }
                let reciprocal = self
                    .inner
                    .db
                    .get(counterpart)
                    .map_err(storage_error)?
                    .ok_or_else(withdrawal_corruption)?;
                // Storage commits the same encoded bytes to both authoritative keys.
                // Compare bytes, not merely decoded receipts or selected fields.
                if reciprocal.as_slice() != bytes.as_ref() {
                    return Err(withdrawal_corruption());
                }
                if family == 24 {
                    // Offline audit is exhaustive; the online request budget does not
                    // truncate it. Each target requires only event, attempt and context.
                    let mut budget = ExperienceReuseBudget::new(3, usize::MAX);
                    self.verify_observation_with_budget(
                        &stored.tenant,
                        &stored.namespace,
                        &stored.command.observation,
                        &mut budget,
                    )
                    .map_err(|_| withdrawal_corruption())?;
                    summary.withdrawals = summary
                        .withdrawals
                        .checked_add(1)
                        .ok_or_else(withdrawal_corruption)?;
                } else {
                    summary.idempotency_bindings = summary
                        .idempotency_bindings
                        .checked_add(1)
                        .ok_or_else(withdrawal_corruption)?;
                }
            }
        }
        if summary.withdrawals != summary.idempotency_bindings {
            return Err(withdrawal_corruption());
        }
        Ok(summary)
    }
}

fn withdrawal_corruption() -> Error {
    Error::DataCorruption("Experience withdrawal ledger is inconsistent".into())
}
