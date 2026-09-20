use super::semantic_tests::{actor, create, input, open};
use super::*;
use tempfile::TempDir;

fn query(text: &str) -> LexicalQuery {
    LexicalQuery {
        text: text.into(),
        limit: 10,
        scan_limit: 10_000,
        scan_bytes_limit: 8_388_608,
        after: None,
        episode_type: None,
        tag: None,
    }
}
#[test]
fn lexical_durable_scoring_uses_scoped_current_corpus_without_embeddings() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "memory memory");
    create(&db, "scope", "different words");
    for i in 0..5 {
        create(&db, "foreign", &format!("memory {i}"));
    }
    let page = db
        .search_memory_lexical("scope", &query("MEMORY memory"))
        .unwrap();
    assert!(page.exhaustive);
    assert_eq!(page.corpus_records, 2);
    assert_eq!(page.matched_records, 1);
    assert_eq!(page.hits[0].record.record_id, a.record_id);
    let expected = 2.0_f64.ln() * 2.0 * 2.2 / (2.0 + 1.2);
    assert!((page.hits[0].score - expected).abs() < 1e-12);
    drop(db);
    let db = open(dir.path());
    assert_eq!(
        db.search_memory_lexical("scope", &query("memory"))
            .unwrap()
            .hits[0]
            .score,
        page.hits[0].score
    );
    assert!(
        db.search_memory_lexical("empty", &query("memory"))
            .unwrap()
            .hits
            .is_empty()
    );
}
#[test]
fn lexical_filters_visibility_before_statistics_and_uses_all_text_fields() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let author = actor();
    let mut record = input("primary");
    record.content.secondary = Some("MEMÓRIA".into());
    record.content.context = Some("東京".into());
    let target = db
        .apply_memory_command(
            "scope",
            &author,
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: "target".into(),
                operation: MemoryOperation::Create {
                    record: record.clone(),
                },
            },
        )
        .unwrap();
    record.valid_until_millis = Some(1);
    db.apply_memory_command(
        "scope",
        &author,
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: "expired".into(),
            operation: MemoryOperation::Create { record },
        },
    )
    .unwrap();
    let deleted = create(&db, "scope", "MEMÓRIA");
    db.apply_memory_command(
        "scope",
        &author,
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: "delete".into(),
            operation: MemoryOperation::Delete {
                record_id: deleted.record_id,
                expected_revision: 1,
            },
        },
    )
    .unwrap();
    let page = db
        .search_memory_lexical("scope", &query("memória 東京"))
        .unwrap();
    assert_eq!(page.scanned_records, 3);
    assert_eq!(page.corpus_records, 1);
    assert_eq!(page.hits[0].record.record_id, target.record_id);
    assert!((page.hits[0].score - 2.0 * (4.0_f64 / 3.0).ln()).abs() < 1e-12);
    let mut filtered = query("primary");
    filtered.tag = Some("absent".into());
    assert_eq!(
        db.search_memory_lexical("scope", &filtered)
            .unwrap()
            .corpus_records,
        0
    );
    filtered.tag = None;
    filtered.episode_type = Some(EpisodeType::Decision);
    assert_eq!(
        db.search_memory_lexical("scope", &filtered)
            .unwrap()
            .corpus_records,
        0
    );
}
#[test]
fn lexical_scan_limits_disclose_local_statistics_and_validate_inputs() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let mut ids = (0..3)
        .map(|i| create(&db, "scope", &format!("term {i}")).record_id)
        .collect::<Vec<_>>();
    ids.sort();
    let mut q = query("term");
    q.scan_limit = 1;
    let page = db.search_memory_lexical("scope", &q).unwrap();
    assert!(!page.exhaustive);
    assert_eq!(page.next_after, Some(ids[0]));
    assert_eq!(page.corpus_records, 1);
    q.scan_limit = 100;
    q.after = page.next_after;
    let rest = db.search_memory_lexical("scope", &q).unwrap();
    assert!(!rest.exhaustive);
    assert!(rest.next_after.is_none());
    assert_eq!(rest.corpus_records, 2);
    q.after = None;
    q.scan_bytes_limit = page.scanned_bytes;
    let bytes = db.search_memory_lexical("scope", &q).unwrap();
    assert_eq!(bytes.scanned_records, 1);
    assert!(!bytes.exhaustive);
    q.scan_bytes_limit = 1;
    assert!(db.search_memory_lexical("scope", &q).is_err());
    for text in ["", "!!!", &"a".repeat(4097)] {
        assert!(db.search_memory_lexical("scope", &query(text)).is_err());
    }
    q = query("term");
    q.limit = 0;
    assert!(db.search_memory_lexical("scope", &q).is_err());
    q = query("term");
    q.scan_limit = 10001;
    assert!(db.search_memory_lexical("scope", &q).is_err());
}
