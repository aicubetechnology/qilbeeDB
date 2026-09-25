//! Read-only audit of the derived strategy locator projection.
use super::super::experience_withdrawal::ExperienceReuseBudget;
use super::super::strategies::{
    STRATEGY_LOCATOR_MARKER, StrategyCandidateReceipt, StrategyLocator, locator_key,
};
use super::*;

impl LearningMemory {
    /// Exhaustive offline audit, without the online request's work limits. Legacy
    /// unmarked stores remain valid only when they contain no new locator entries.
    pub fn verify_strategy_locators(&self) -> Result<u64> {
        let marker = self
            .inner
            .db
            .get(STRATEGY_LOCATOR_MARKER)
            .map_err(storage_error)?;
        if let Some(bytes) = &marker {
            if decode::<u32>(bytes)? != 1 {
                return Err(locator_corruption());
            }
        }
        if self
            .inner
            .db
            .get(super::super::strategies::STRATEGY_LOCATOR_PROGRESS)
            .map_err(storage_error)?
            .is_some()
        {
            return Err(Error::DataCorruption(
                "Strategy locator migration is incomplete".into(),
            ));
        }
        let mut locators = 0u64;
        for item in self
            .inner
            .db
            .iterator(IteratorMode::From(&[26], Direction::Forward))
        {
            let (key, bytes) = item.map_err(storage_error)?;
            if key.first() != Some(&26) {
                break;
            }
            if marker.is_none() {
                return Err(locator_corruption());
            }
            let locator: StrategyLocator = decode(&bytes)?;
            if locator.schema_version != 1
                || key.as_ref() != locator_key(&locator.scope, &locator.strategy_id).as_slice()
            {
                return Err(locator_corruption());
            }
            let mut budget = ExperienceReuseBudget::new(usize::MAX, usize::MAX);
            let receipt = self
                .strategy_candidate_budgeted(
                    &locator.tenant,
                    &locator.namespace,
                    &locator.strategy_id,
                    &mut budget,
                )
                .map_err(|_| locator_corruption())?
                .ok_or_else(locator_corruption)?;
            if receipt.locator() != locator {
                return Err(locator_corruption());
            }
            locators = locators.checked_add(1).ok_or_else(locator_corruption)?;
        }
        if marker.is_none() {
            return Ok(0);
        }
        let mut receipts = 0u64;
        for item in self
            .inner
            .db
            .iterator(IteratorMode::From(&[15], Direction::Forward))
        {
            let (key, bytes) = item.map_err(storage_error)?;
            if key.first() != Some(&15) {
                break;
            }
            let raw: StrategyCandidateReceipt = decode(&bytes)?;
            let expected_key = super::super::tools::tool_key(
                15,
                &raw.proposal.tenant,
                &raw.proposal.namespace,
                &raw.request.id,
            )
            .map_err(|_| locator_corruption())?;
            if key.as_ref() != expected_key.as_slice() {
                return Err(locator_corruption());
            }
            let mut budget = ExperienceReuseBudget::new(usize::MAX, usize::MAX);
            let receipt = self
                .strategy_candidate_budgeted(
                    &raw.proposal.tenant,
                    &raw.proposal.namespace,
                    &raw.request.id,
                    &mut budget,
                )
                .map_err(|_| locator_corruption())?
                .ok_or_else(locator_corruption)?;
            let expected = receipt.locator();
            let bytes = self
                .inner
                .db
                .get(locator_key(&expected.scope, &expected.strategy_id))
                .map_err(storage_error)?
                .ok_or_else(locator_corruption)?;
            if decode::<StrategyLocator>(&bytes)? != expected {
                return Err(locator_corruption());
            }
            receipts = receipts.checked_add(1).ok_or_else(locator_corruption)?;
        }
        if receipts != locators {
            return Err(locator_corruption());
        }
        Ok(locators)
    }
}

fn locator_corruption() -> Error {
    Error::DataCorruption("Strategy locator projection is inconsistent".into())
}
