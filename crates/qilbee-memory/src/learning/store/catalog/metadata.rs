//! Bounded summaries of immutable company policy and context records.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearningMetadataKind {
    Policy,
    Context,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningMetadataQuery {
    pub kind: LearningMetadataKind,
    pub text: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "default_scan")]
    pub max_scanned_records: usize,
    pub cursor: Option<LearningCatalogCursor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningMetadataEntry {
    pub kind: LearningMetadataKind,
    pub id: String,
    pub title: String,
    pub schema_version: u32,
    pub payload_digest: String,
    pub recorded_at_millis: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningMetadataPage {
    pub entries: Vec<LearningMetadataEntry>,
    pub next_cursor: Option<LearningCatalogCursor>,
    pub stop_reason: LearningCatalogStop,
    pub scanned_records: usize,
    pub scanned_record_bytes: usize,
    pub observed_at_millis: i64,
}

impl LearningMemory {
    /// Trusted storage API; HTTP must authenticate metadata read authority first.
    pub fn company_learning_metadata(
        &self,
        company: &str,
        query: &LearningMetadataQuery,
    ) -> Result<LearningMetadataPage> {
        if query
            .text
            .as_ref()
            .is_some_and(|text| text.chars().any(char::is_control))
        {
            return Err(invalid(
                "Metadata text filter must not contain control characters",
            ));
        }
        if let Some(cursor) = &query.cursor {
            let lower_hex = |value: &str| {
                value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            };
            if cursor.position.is_empty()
                || cursor.position.len() > 4096
                || cursor.position.len() % 2 != 0
                || !lower_hex(&cursor.position)
                || cursor.filter_digest.len() != 64
                || !lower_hex(&cursor.filter_digest)
            {
                return Err(invalid("Invalid metadata cursor encoding"));
            }
        }
        let catalog = LearningCatalogQuery {
            kind: match query.kind {
                LearningMetadataKind::Policy => LearningResourceKind::Policy,
                LearningMetadataKind::Context => LearningResourceKind::Context,
            },
            filter: LearningCatalogFilter {
                text: query.text.clone(),
                ..Default::default()
            },
            limit: query.limit,
            max_scanned_records: query.max_scanned_records,
            cursor: query.cursor.clone(),
        };
        let mut entries = Vec::new();
        // Capture summaries under the same serialized observation as traversal.
        // Never unlock and re-read selected records to construct another page.
        let page =
            self.company_learning_catalog_observed(company, &catalog, |details, summary| {
                let (schema_version, payload_digest) = match details {
                    LearningResourceDetails::Policy(record) => {
                        (record.schema_version, &record.payload_digest)
                    }
                    LearningResourceDetails::Context(record) => {
                        (record.schema_version, &record.payload_digest)
                    }
                    _ => return Err(corrupt()),
                };
                entries.push(LearningMetadataEntry {
                    kind: query.kind,
                    id: summary.resource.id.clone(),
                    title: summary.title.clone(),
                    schema_version,
                    payload_digest: payload_digest.clone(),
                    recorded_at_millis: summary.recorded_at_millis,
                });
                Ok(())
            })?;
        Ok(LearningMetadataPage {
            entries,
            next_cursor: page.next_cursor,
            stop_reason: page.stop_reason,
            scanned_records: page.scanned_records,
            scanned_record_bytes: page.scanned_record_bytes,
            observed_at_millis: page.observed_at_millis,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning::{LearningPolicy, PolicyAlgorithm, PolicyDefinition};
    use tempfile::TempDir;

    fn register(store: &LearningMemory, company: &str, id: &str) {
        store
            .register_policy(
                company,
                id,
                PolicyDefinition {
                    algorithm: PolicyAlgorithm::FixedBudgetHoeffdingV1,
                    parameters: LearningPolicy {
                        qualification_trials: 32,
                        evaluator_id: "judge".into(),
                        evaluation_contract: "rubric".into(),
                        ..Default::default()
                    },
                },
                "fixture",
            )
            .unwrap();
    }
    fn query() -> LearningMetadataQuery {
        serde_json::from_value(serde_json::json!({"kind":"policy","limit":1})).unwrap()
    }

    #[test]
    fn metadata_summaries_preserve_receipts_and_scope_across_restart() {
        let dir = TempDir::new().unwrap();
        let store = LearningMemory::open(dir.path()).unwrap();
        for company in ["one", "two"] {
            for id in ["a", "b"] {
                register(&store, company, id);
            }
        }
        let first = store.company_learning_metadata("one", &query()).unwrap();
        assert_eq!(first.entries.len(), 1);
        assert_eq!(first.entries[0].id, "a");
        assert_eq!(first.stop_reason, LearningCatalogStop::EntryLimit);
        let exact = store.policy("one", "a").unwrap().unwrap();
        assert_eq!(first.entries[0].payload_digest, exact.payload_digest);
        assert_eq!(first.entries[0].schema_version, exact.schema_version);
        let mut next = query();
        next.cursor = first.next_cursor;
        assert!(store.company_learning_metadata("two", &next).is_err());
        drop(store);
        let store = LearningMemory::open(dir.path()).unwrap();
        let second = store.company_learning_metadata("one", &next).unwrap();
        assert_eq!(second.entries[0].id, "b");
        assert_eq!(second.stop_reason, LearningCatalogStop::Exhausted);
        assert!(second.next_cursor.is_none());
    }

    #[test]
    fn metadata_empty_filtered_pages_advance_and_bind_filter_and_kind() {
        let dir = TempDir::new().unwrap();
        let store = LearningMemory::open(dir.path()).unwrap();
        for id in ["a", "b"] {
            register(&store, "one", id);
        }
        let mut q = query();
        q.text = Some("b".into());
        q.max_scanned_records = 1;
        let first = store.company_learning_metadata("one", &q).unwrap();
        assert!(first.entries.is_empty());
        assert_eq!(first.scanned_records, 1);
        assert_eq!(first.stop_reason, LearningCatalogStop::ScanLimit);
        q.cursor = first.next_cursor;
        let mut changed = q.clone();
        changed.text = Some("a".into());
        assert!(store.company_learning_metadata("one", &changed).is_err());
        changed = q.clone();
        changed.kind = LearningMetadataKind::Context;
        assert!(store.company_learning_metadata("one", &changed).is_err());
        let second = store.company_learning_metadata("one", &q).unwrap();
        assert_eq!(second.entries[0].id, "b");
        assert!(second.next_cursor.is_none());
        q.cursor.as_mut().unwrap().position = "00".repeat(2049);
        assert!(store.company_learning_metadata("one", &q).is_err());
    }

    #[test]
    fn metadata_text_limits_reject_controls_and_multibyte_overflow() {
        let dir = TempDir::new().unwrap();
        let store = LearningMemory::open(dir.path()).unwrap();
        for text in [
            " ".to_owned(),
            "a\nb".to_owned(),
            "a\u{0085}b".to_owned(),
            "é".repeat(129),
        ] {
            let mut q = query();
            q.text = Some(text);
            assert!(store.company_learning_metadata("one", &q).is_err());
        }
    }

    fn key(company: &str, kind: LearningResourceKind, id: &str) -> Vec<u8> {
        let mut value = kind.prefix(company);
        append_component(&mut value, id);
        value
    }

    #[test]
    fn metadata_corruption_fails_the_whole_page_without_scanning_foreign_payloads() {
        let dir = TempDir::new().unwrap();
        let store = LearningMemory::open(dir.path()).unwrap();
        for id in ["a", "b"] {
            register(&store, "one", id);
        }
        store
            .inner
            .db
            .put(key("two", LearningResourceKind::Policy, "a"), b"corrupt")
            .unwrap();
        let mut q = query();
        q.limit = 50;
        assert_eq!(
            store
                .company_learning_metadata("one", &q)
                .unwrap()
                .entries
                .len(),
            2
        );
        let mut broken = store.policy("one", "b").unwrap().unwrap();
        broken.payload_digest = "0".repeat(64);
        store
            .inner
            .db
            .put(
                key("one", LearningResourceKind::Policy, "b"),
                encode(&broken).unwrap(),
            )
            .unwrap();
        assert!(matches!(
            store.company_learning_metadata("one", &q),
            Err(Error::DataCorruption(_))
        ));
        store
            .inner
            .db
            .put(
                key("one", LearningResourceKind::Policy, "b"),
                vec![0; SCAN_BYTES + 1],
            )
            .unwrap();
        assert!(matches!(
            store.company_learning_metadata("one", &q),
            Err(Error::DataCorruption(_))
        ));
    }

    #[test]
    fn metadata_live_continuation_does_not_claim_a_snapshot_after_inserts() {
        let dir = TempDir::new().unwrap();
        let store = LearningMemory::open(dir.path()).unwrap();
        for id in ["b", "d"] {
            register(&store, "one", id);
        }
        let mut q = query();
        let first = store.company_learning_metadata("one", &q).unwrap();
        assert_eq!(first.entries[0].id, "b");
        q.cursor = first.next_cursor;
        for id in ["a", "c"] {
            register(&store, "one", id);
        }
        let next = store.company_learning_metadata("one", &q).unwrap();
        assert_eq!(next.entries[0].id, "c");
        q.cursor = next.next_cursor;
        let last = store.company_learning_metadata("one", &q).unwrap();
        assert_eq!(last.entries[0].id, "d");
        assert!(last.next_cursor.is_none());
        q.cursor = None;
        q.limit = 50;
        let fresh = store.company_learning_metadata("one", &q).unwrap();
        assert_eq!(
            fresh
                .entries
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c", "d"]
        );
    }

    #[test]
    fn metadata_byte_limit_keeps_bounded_summaries_and_a_progressing_cursor() {
        use crate::learning::EvaluationContext;
        let dir = TempDir::new().unwrap();
        let store = LearningMemory::open(dir.path()).unwrap();
        let context = EvaluationContext {
            task: "Task ".repeat(100),
            baseline_revision: "v1".into(),
            model_provider: "external".into(),
            model_revision: "v1".into(),
            tools: (0..128)
                .map(|i| (format!("{i:03}{}", "x".repeat(509)), "v".repeat(512)))
                .collect(),
            environment_revision: "v1".into(),
            evaluation_contract: "v1".into(),
            dataset_revision: "v1".into(),
            harness_revision: "v1".into(),
            permissions_revision: "v1".into(),
        };
        for i in 0..34 {
            store
                .register_context(
                    "one",
                    &format!("context-{i:03}"),
                    context.clone(),
                    "fixture",
                )
                .unwrap();
        }
        let mut q = query();
        q.kind = LearningMetadataKind::Context;
        q.limit = 50;
        let first = store.company_learning_metadata("one", &q).unwrap();
        assert_eq!(first.stop_reason, LearningCatalogStop::ByteLimit);
        assert!(first.scanned_record_bytes <= SCAN_BYTES);
        assert!(first.entries.iter().all(|e| e.title.chars().count() <= 160));
        assert!(serde_json::to_vec(&first).unwrap().len() < 32 * 1024);
        q.cursor = first.next_cursor;
        let second = store.company_learning_metadata("one", &q).unwrap();
        assert_eq!(first.entries.len() + second.entries.len(), 34);
        assert!(second.next_cursor.is_none());
        assert_ne!(first.entries.last().unwrap().id, second.entries[0].id);
    }

    #[test]
    fn metadata_query_rejects_nonmetadata_kinds_and_scope_fields() {
        for input in [
            serde_json::json!({"kind":"procedure"}),
            serde_json::json!({"kind":"policy","scope":{}}),
            serde_json::json!({"kind":"policy","company_id":"other"}),
        ] {
            assert!(serde_json::from_value::<LearningMetadataQuery>(input).is_err());
        }
    }
}
