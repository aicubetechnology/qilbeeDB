use super::*;
use std::cmp::Ordering;

// Keep one strongest path at each exact depth and endpoint. Factors are in
// (0, 1], with strict hop decay, so cycling cannot improve a global best path.
#[derive(Clone)]
struct Path {
    strength: f64,
    anchor: MemorySourceRef,
    anchor_rank: usize,
    steps: Vec<GraphPathStep>,
    missing: Vec<MemorySourceRef>,
}
fn order(a: &Path, b: &Path) -> Ordering {
    b.strength
        .total_cmp(&a.strength)
        .then_with(|| a.steps.len().cmp(&b.steps.len()))
        .then_with(|| a.anchor_rank.cmp(&b.anchor_rank))
        .then_with(|| {
            a.steps
                .iter()
                .map(|s| (s.relation_id, s.direction))
                .cmp(b.steps.iter().map(|s| (s.relation_id, s.direction)))
        })
}
fn keep(map: &mut BTreeMap<Uuid, Path>, id: Uuid, path: Path) {
    if map.get(&id).is_none_or(|old| order(&path, old).is_lt()) {
        map.insert(id, path);
    }
}
pub(super) fn rank_paths(
    profile: &GraphRankingProfile,
    anchors: &[MemorySourceRef],
    graph: &TypedMemoryGraph,
    affinities: &BTreeMap<Uuid, GraphAffinity>,
    use_affinity: bool,
) -> Vec<(Uuid, GraphPathContribution)> {
    let nodes: BTreeSet<_> = graph.nodes.iter().map(|n| n.record.record_id).collect();
    let weights: BTreeMap<_, _> = profile
        .relation_weights
        .iter()
        .map(|w| (w.kind, w.weight))
        .collect();
    let mut frontier = BTreeMap::new();
    for (i, anchor) in anchors.iter().enumerate() {
        if nodes.contains(&anchor.record_id) {
            frontier.insert(
                anchor.record_id,
                Path {
                    strength: 1.0 / (profile.rank_constant + i + 1) as f64,
                    anchor: anchor.clone(),
                    anchor_rank: i + 1,
                    steps: vec![],
                    missing: vec![],
                },
            );
        }
    }
    let mut best = frontier.clone();
    for _ in 0..graph.max_depth {
        let mut next = BTreeMap::new();
        for relation in &graph.edges {
            let weight = weights[&relation.input.kind];
            if weight == 0.0 {
                continue;
            }
            for direction in [GraphStepDirection::Outgoing, GraphStepDirection::Incoming] {
                if matches!(
                    (graph.direction, direction),
                    (TypedGraphDirection::Incoming, GraphStepDirection::Outgoing)
                        | (TypedGraphDirection::Outgoing, GraphStepDirection::Incoming)
                ) {
                    continue;
                }
                let (from, to) = match direction {
                    GraphStepDirection::Outgoing => {
                        (&relation.input.source, &relation.input.target)
                    }
                    GraphStepDirection::Incoming => {
                        (&relation.input.target, &relation.input.source)
                    }
                };
                let Some(previous) = frontier.get(&from.record_id) else {
                    continue;
                };
                let mut path = previous.clone();
                let affinity = if use_affinity {
                    if let Some(a) = affinities.get(&to.record_id) {
                        profile.cosine_affinity_floor
                            + profile.cosine_affinity_weight * a.cosine.max(0.0)
                    } else {
                        path.missing.push(to.clone());
                        profile.missing_embedding_affinity
                    }
                } else {
                    1.0
                };
                path.strength *= profile.hop_decay * weight * affinity;
                path.steps.push(GraphPathStep {
                    relation_id: relation.relation_id,
                    relation_revision: relation.revision,
                    direction,
                    from: from.clone(),
                    to: to.clone(),
                });
                keep(&mut next, to.record_id, path);
            }
        }
        for (id, path) in &next {
            keep(&mut best, *id, path.clone());
        }
        frontier = next;
        if frontier.is_empty() {
            break;
        }
    }
    let mut paths: Vec<_> = best.into_iter().collect();
    // Relation UUIDs break proof ties only, never ranking ties. Additional equal
    // parallel assertions therefore cannot inflate a memory's rank or strength.
    paths.sort_by(|(a_id, a), (b_id, b)| {
        b.strength
            .total_cmp(&a.strength)
            .then_with(|| a.steps.len().cmp(&b.steps.len()))
            .then_with(|| a.anchor_rank.cmp(&b.anchor_rank))
            .then_with(|| a_id.cmp(b_id))
    });
    paths
        .into_iter()
        .enumerate()
        .map(|(index, (id, path))| {
            let rank = index + 1;
            (
                id,
                GraphPathContribution {
                    rank,
                    strength: path.strength,
                    contribution: if profile.version == GraphRankingVersion::TypedPathStrengthV1 {
                        profile.graph_weight * path.strength
                    } else {
                        profile.graph_weight / (profile.rank_constant + rank) as f64
                    },
                    anchor: path.anchor,
                    anchor_rank: path.anchor_rank,
                    steps: path.steps,
                    missing_affinity: path.missing,
                },
            )
        })
        .collect()
}
