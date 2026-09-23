use super::*;

#[test]
fn strength_profile_keeps_base_evidence_and_admits_graph_only_neighbors() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let mut base: Vec<_> = (0..10)
        .map(|i| create(&db, "scope", &format!("anchor {i}")))
        .collect();
    base.sort_by_key(|r| r.record_id);
    let neighbor = create(&db, "scope", "distant evidence");
    link(
        &db,
        &base[0],
        &neighbor,
        MemoryRelationKind::SameEntity,
        "edge",
    );
    let mut q = query("anchor");
    q.ranking_version = GraphRankingVersion::TypedPathStrengthV1;
    let page = db.search_memory_graph("scope", &q).unwrap();
    assert!(ids(&page).contains(&neighbor.record_id));
    // Unlike the earlier base-preserving profile, graph-only evidence can enter.
    // Unlike graph-heavy rank fusion, this single neighbor does not evict rank 5.
    assert!(ids(&page).contains(&base[4].record_id));
    let hit = page
        .hits
        .iter()
        .find(|h| h.record.record_id == neighbor.record_id)
        .unwrap();
    assert!(hit.base.is_none());
    assert_eq!(hit.score, 1.0 / 12.0);
    assert_eq!(hit.graph.as_ref().unwrap().strength, 1.0 / 6.0);
    assert_eq!(page.hits[0].score, 1.0 / 3.0);
    assert!(
        db.search_memory_graph("another-scope", &q)
            .unwrap()
            .hits
            .is_empty()
    );
    let signature = |p: &GraphRetrievalPage| {
        p.hits
            .iter()
            .map(|h| {
                (
                    h.record.record_id,
                    h.score,
                    h.graph.as_ref().map(|g| g.strength),
                )
            })
            .collect::<Vec<_>>()
    };
    link(
        &db,
        &base[0],
        &neighbor,
        MemoryRelationKind::SameEntity,
        "duplicate",
    );
    link(
        &db,
        &neighbor,
        &base[0],
        MemoryRelationKind::SameEntity,
        "cycle",
    );
    assert_eq!(
        signature(&page),
        signature(&db.search_memory_graph("scope", &q).unwrap())
    );
    drop(db);
    let db = open(dir.path());
    assert_eq!(
        signature(&page),
        signature(&db.search_memory_graph("scope", &q).unwrap())
    );
    q.expansion.max_depth = 0;
    let depth_zero = db.search_memory_graph("scope", &q).unwrap();
    assert_eq!(
        depth_zero
            .hits
            .iter()
            .map(|h| h.record.record_id)
            .collect::<Vec<_>>(),
        base.iter().map(|r| r.record_id).collect::<Vec<_>>()
    );
}

#[test]
fn absolute_path_strength_changes_score_even_when_graph_rank_is_unchanged() {
    let evaluate = |version, vector: Vec<f32>| {
        let dir = TempDir::new().unwrap();
        let db = open(dir.path());
        let a = create(&db, "scope", "anchor");
        let b = create(&db, "scope", "neighbor");
        db.apply_memory_embedding(
            "scope",
            &actor(),
            &attach(a.record_id, "a", vec![1.0, 0.0, 0.0]),
        )
        .unwrap();
        db.apply_memory_embedding("scope", &actor(), &attach(b.record_id, "b", vector))
            .unwrap();
        link(&db, &a, &b, MemoryRelationKind::SameEntity, "edge");
        let mut q = query("anchor");
        q.ranking_version = version;
        // Only the lexical match is a seed, but both vectors inform path affinity.
        q.seed = GraphSeedQuery::Hybrid {
            text: "anchor".into(),
            space: space(),
            vector: vec![1.0, 0.0, 0.0],
            ranking_version: HybridRankingVersion::WeightedRrfV1,
            min_score: 1.0,
        };
        let page = db.search_memory_graph("scope", &q).unwrap();
        let hit = page
            .hits
            .iter()
            .find(|h| h.record.record_id == b.record_id)
            .unwrap();
        (
            hit.graph.as_ref().unwrap().rank,
            hit.graph.as_ref().unwrap().strength,
            hit.graph.as_ref().unwrap().contribution,
        )
    };
    let high = vec![0.8, 0.6, 0.0];
    let low = vec![0.0, 1.0, 0.0];
    let old_high = evaluate(GraphRankingVersion::TypedPathBalancedV1, high.clone());
    let old_low = evaluate(GraphRankingVersion::TypedPathBalancedV1, low.clone());
    assert_eq!(old_high.0, old_low.0);
    assert!(old_high.1 > old_low.1);
    assert_eq!(old_high.2, old_low.2);
    let new_high = evaluate(GraphRankingVersion::TypedPathStrengthV1, high);
    let new_low = evaluate(GraphRankingVersion::TypedPathStrengthV1, low);
    assert_eq!(new_high.0, new_low.0);
    assert!(new_high.1 > new_low.1);
    assert!(new_high.2 > new_low.2);
}
