use super::semantic_tests::{actor, attach, create, input, open, space};
use super::*;
use tempfile::TempDir;

fn query() -> HybridQuery {
    HybridQuery {
        text: "ZX17".into(),
        space: space(),
        vector: vec![1.0, 0.0, 0.0],
        limit: 10,
        ranking_version: HybridRankingVersion::WeightedRrfV1,
        min_score: -1.0,
        scan_limit: 10_000,
        scan_bytes_limit: 8_388_608,
        after: None,
        episode_type: None,
        tag: None,
    }
}
#[test]
fn hybrid_fuses_complementary_candidates_with_explained_rrf() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let lexical = create(&db, "scope", "error ZX17");
    let dense = create(&db, "scope", "paraphrase");
    db.apply_memory_embedding(
        "scope",
        &actor(),
        &attach(dense.record_id, "dense", vec![1.0, 0.0, 0.0]),
    )
    .unwrap();
    db.apply_memory_embedding(
        "scope",
        &actor(),
        &attach(lexical.record_id, "lexical", vec![0.0, 1.0, 0.0]),
    )
    .unwrap();
    let page = db.search_memory_hybrid("scope", &query()).unwrap();
    assert!(page.exhaustive);
    assert_eq!(page.corpus_records, 2);
    assert_eq!(page.embedded_records, 2);
    assert_eq!(page.hits[0].record.record_id, lexical.record_id);
    let hit = &page.hits[0];
    assert_eq!(hit.lexical.as_ref().unwrap().rank, 1);
    assert_eq!(hit.semantic.as_ref().unwrap().rank, 2);
    assert!((hit.score - (0.5 / 61.0 + 0.5 / 62.0)).abs() < 1e-12);
    assert_eq!(hit.semantic.as_ref().unwrap().score, 0.0);
    assert_eq!(hit.embedding.as_ref().unwrap().record_id, lexical.record_id);
}
#[test]
fn hybrid_missing_or_stale_vectors_preserve_lexical_recall_and_provenance() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let author = actor();
    let source = create(&db, "scope", "ZX17");
    db.apply_memory_embedding(
        "scope",
        &author,
        &attach(source.record_id, "binding", vec![1.0, 0.0, 0.0]),
    )
    .unwrap();
    let snapshot = db.memory_snapshot();
    db.apply_memory_command(
        "scope",
        &author,
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: "update".into(),
            operation: MemoryOperation::Update {
                record_id: source.record_id,
                expected_revision: 1,
                record: input("ZX17 revised"),
            },
        },
    )
    .unwrap();
    let old = snapshot.search_hybrid("scope", &query()).unwrap();
    assert_eq!(old.hits[0].record.revision, 1);
    assert_eq!(old.embedded_records, 1);
    let new = db.search_memory_hybrid("scope", &query()).unwrap();
    assert_eq!(new.hits[0].record.revision, 2);
    assert_eq!(new.embedded_records, 0);
    assert_eq!(new.embedding_coverage, EmbeddingCoverage::Missing);
    assert!(new.hits[0].semantic.is_none());
    assert!(new.hits[0].embedding.is_none());
    assert_eq!(new.hits[0].score, 0.5 / 61.0);
    let mut q = query();
    q.space.revision = "missing".into();
    assert_eq!(
        db.search_memory_hybrid("scope", &q)
            .unwrap()
            .embedded_records,
        0
    );
    assert!(
        db.search_memory_hybrid("foreign", &query())
            .unwrap()
            .hits
            .is_empty()
    );
    q = query();
    q.tag = Some("absent".into());
    assert_eq!(
        db.search_memory_hybrid("scope", &q).unwrap().corpus_records,
        0
    );
}
#[test]
fn hybrid_validates_thresholds_vectors_and_counts_vector_bytes() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    for i in 0..3 {
        let receipt = create(&db, "scope", &format!("ZX17 {i}"));
        db.apply_memory_embedding(
            "scope",
            &actor(),
            &attach(receipt.record_id, &format!("vec{i}"), vec![1.0, 0.0, 0.0]),
        )
        .unwrap();
    }
    let mut q = query();
    q.scan_limit = 1;
    let page = db.search_memory_hybrid("scope", &q).unwrap();
    assert!(!page.exhaustive);
    assert_eq!(page.scanned_records, 1);
    q.scan_limit = 100;
    q.scan_bytes_limit = page.scanned_bytes;
    let bounded = db.search_memory_hybrid("scope", &q).unwrap();
    assert_eq!(bounded.scanned_records, 1);
    assert!(!bounded.exhaustive);
    q = query();
    q.vector = vec![0.0; 3];
    assert!(db.search_memory_hybrid("scope", &q).is_err());
    q = query();
    q.min_score = 1.1;
    assert!(db.search_memory_hybrid("scope", &q).is_err());
}

#[test]
fn hybrid_does_not_hide_source_corruption_as_a_missing_candidate() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let source = create(&db, "scope", "ZX17");
    db.db
        .delete_cf(
            db.cf(super::super::cf::EPISODE_INDEX).unwrap(),
            record_key(0x11, "scope", source.record_id),
        )
        .unwrap();
    assert!(matches!(
        db.search_memory_hybrid("scope", &query()),
        Err(Error::DataCorruption(_))
    ));
}

#[test]
fn hybrid_rejects_request_owned_weights_and_unknown_ranking_versions() {
    let mut body = serde_json::to_value(query()).unwrap();
    body["semantic_weight"] = serde_json::json!(0.9);
    assert!(serde_json::from_value::<HybridQuery>(body).is_err());
    let mut body = serde_json::to_value(query()).unwrap();
    body["ranking_version"] = serde_json::json!("unknown_v1");
    assert!(serde_json::from_value::<HybridQuery>(body).is_err());
    let mut body = serde_json::to_value(query()).unwrap();
    body["candidate_limit"] = serde_json::json!(1);
    assert!(serde_json::from_value::<HybridQuery>(body).is_err());
}

#[test]
fn hybrid_server_profile_caps_candidates_and_exposes_missing_bindings() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    for i in 0..101 {
        create(&db, "scope", &format!("ZX17 record {i}"));
    }
    let page = db.search_memory_hybrid("scope", &query()).unwrap();
    assert!(page.exhaustive);
    assert!(page.candidates_truncated);
    assert_eq!(page.lexical_matches, 101);
    assert_eq!(page.lexical_candidates, 100);
    assert_eq!(page.ranking.version, HybridRankingVersion::WeightedRrfV1);
    assert_eq!(page.ranking.candidate_limit, 100);
    assert_eq!(page.ranking.semantic_weight, 0.5);
    assert_eq!(page.embedding_coverage, EmbeddingCoverage::Missing);
}

#[test]
fn hybrid_v2_is_explicit_explained_and_leaves_v1_unchanged() {
    assert_eq!(
        serde_json::to_value(HybridRankingVersion::WeightedRrfV1.profile()).unwrap(),
        serde_json::json!({"version":"weighted_rrf_v1","method":"weighted_rrf","lexical_version":"bm25_v1","semantic_version":"cosine_exact_v1","candidate_limit":100,"lexical_weight":0.5,"semantic_weight":0.5,"rank_constant":60,"experimental":true})
    );
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    for (i, text) in ["ZX17 recovery", "ZX17", "retry budget"]
        .into_iter()
        .enumerate()
    {
        let receipt = create(&db, "scope", text);
        db.apply_memory_embedding(
            "scope",
            &actor(),
            &attach(
                receipt.record_id,
                &format!("v2-binding-{i}"),
                vec![i as f32, 1.0, 0.0],
            ),
        )
        .unwrap();
    }
    let original =
        serde_json::to_value(db.search_memory_hybrid("scope", &query()).unwrap()).unwrap();
    let mut request = query();
    request.ranking_version = HybridRankingVersion::WeightedRrfV2;
    let result = db.search_memory_hybrid("scope", &request).unwrap();
    assert_eq!(result.ranking.version, HybridRankingVersion::WeightedRrfV2);
    assert!(result.ranking.experimental);
    assert_ne!(
        serde_json::to_value(&result.ranking).unwrap(),
        serde_json::to_value(HybridRankingVersion::WeightedRrfV1.profile()).unwrap()
    );
    assert_eq!(result.rank_constant, result.ranking.rank_constant);
    for hit in result.hits {
        let mut sum = 0.0;
        for (contribution, weight) in [
            (&hit.lexical, result.ranking.lexical_weight),
            (&hit.semantic, result.ranking.semantic_weight),
        ] {
            if let Some(value) = contribution {
                let expected = weight / (result.ranking.rank_constant + value.rank) as f64;
                assert!((value.contribution - expected).abs() < 1e-12);
                sum += expected;
            }
        }
        assert!((hit.score - sum).abs() < 1e-12);
    }
    assert_eq!(
        serde_json::to_value(db.search_memory_hybrid("scope", &query()).unwrap()).unwrap(),
        original
    );
}

#[test]
fn hybrid_v1_swapped_ranks_change_winner_with_reimported_ids() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let mut expected = Vec::new();
    for (namespace, semantic_first) in [("import-a", true), ("import-b", false)] {
        let mut ids = [
            create(&db, namespace, "placeholder-a").record_id,
            create(&db, namespace, "placeholder-b").record_id,
        ];
        ids.sort();
        let semantic_winner = ids[usize::from(!semantic_first)];
        for id in ids {
            let is_semantic_winner = id == semantic_winner;
            let text = if is_semantic_winner {
                "ZX17 supporting context words"
            } else {
                "ZX17 ZX17"
            };
            db.apply_memory_command(
                namespace,
                &actor(),
                &MemoryCommand {
                    contract_version: 1,
                    idempotency_key: format!("payload-{id}"),
                    operation: MemoryOperation::Update {
                        record_id: id,
                        expected_revision: 1,
                        record: input(text),
                    },
                },
            )
            .unwrap();
            let vector = if is_semantic_winner {
                vec![0.75, 0.6614378, 0.0]
            } else {
                vec![0.5, 0.8660254, 0.0]
            };
            let mut binding = attach(id, &format!("vector-{id}"), vector);
            binding.record_revision = 2;
            db.apply_memory_embedding(namespace, &actor(), &binding)
                .unwrap();
        }
        let page = db.search_memory_hybrid(namespace, &query()).unwrap();
        assert!(page.exhaustive);
        assert_eq!(page.hits.len(), 2);
        let winner = page
            .hits
            .iter()
            .find(|h| h.record.record_id == semantic_winner)
            .unwrap();
        assert_eq!(winner.semantic.as_ref().unwrap().rank, 1);
        assert_eq!(winner.lexical.as_ref().unwrap().rank, 2);
        assert!(winner.semantic.as_ref().unwrap().score > 0.74);
        assert_eq!(page.hits[0].score, page.hits[1].score);
        assert_eq!(page.hits[0].record.record_id, ids[0]);
        assert_eq!(
            page.hits[0].record.record_id == semantic_winner,
            semantic_first
        );
        // Different IDs can select a different winner without any change in v1's formula.
        expected.push((namespace, ids[0]));
        let mut v2 = query();
        v2.ranking_version = HybridRankingVersion::WeightedRrfV2;
        assert_eq!(
            db.search_memory_hybrid(namespace, &v2).unwrap().hits[0]
                .record
                .record_id,
            semantic_winner
        );
    }
    drop(db);
    let db = open(dir.path());
    for (namespace, winner) in expected {
        assert_eq!(
            db.search_memory_hybrid(namespace, &query()).unwrap().hits[0]
                .record
                .record_id,
            winner
        );
    }
}

#[test]
fn hybrid_v1_can_drop_the_first_semantic_candidate_without_any_fused_tie() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let target = create(&db, "scope", "external paraphrase");
    db.apply_memory_embedding(
        "scope",
        &actor(),
        &attach(target.record_id, "target-vector", vec![1.0, 0.0, 0.0]),
    )
    .unwrap();
    for n in 0..12 {
        let row = db
            .apply_memory_command(
                "scope",
                &actor(),
                &MemoryCommand {
                    contract_version: 1,
                    idempotency_key: format!("distractor-{n}"),
                    operation: MemoryOperation::Create {
                        record: input("ZX17"),
                    },
                },
            )
            .unwrap();
        db.apply_memory_embedding(
            "scope",
            &actor(),
            &attach(
                row.record_id,
                &format!("vector-{n}"),
                vec![0.9, 0.4358899, 0.0],
            ),
        )
        .unwrap();
    }
    let semantic = db
        .search_memory_semantic("scope", &super::semantic_tests::search())
        .unwrap();
    assert_eq!(semantic.hits[0].record.record_id, target.record_id);
    let page = db.search_memory_hybrid("scope", &query()).unwrap();
    assert!(page.exhaustive);
    assert!(!page.candidates_truncated);
    assert_eq!(page.embedded_records, 13);
    assert_eq!(page.hits.len(), 10);
    assert!(
        page.hits
            .iter()
            .all(|h| h.record.record_id != target.record_id)
    );
    assert!(
        page.hits
            .windows(2)
            .all(|pair| pair[0].score > pair[1].score)
    );
    let mut expanded = query();
    expanded.limit = 13;
    let all = db.search_memory_hybrid("scope", &expanded).unwrap();
    let hit = all.hits.last().unwrap();
    assert_eq!(hit.record.record_id, target.record_id);
    assert_eq!(hit.semantic.as_ref().unwrap().rank, 1);
    assert!(hit.lexical.is_none());
    assert!(all.hits[..12].iter().all(|h| h.score > hit.score));
}
