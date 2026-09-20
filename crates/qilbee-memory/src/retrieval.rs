//! Deterministic lexical retrieval over valid episodes.

use crate::episode::Episode;
use std::collections::{BTreeSet, HashMap};

/// BM25 score is a lexical ranking signal, not a probability.
#[derive(Debug, Clone)]
pub struct KeywordSearchResult {
    pub episode: Episode,
    pub score: f64,
}

fn terms(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect()
}

pub(crate) fn rank_episodes(
    episodes: Vec<Episode>,
    query: &str,
    limit: usize,
) -> Vec<KeywordSearchResult> {
    let query: BTreeSet<String> = terms(query).into_iter().collect();
    if limit == 0 || query.is_empty() {
        return Vec::new();
    }
    let documents: Vec<_> = episodes
        .into_iter()
        .filter(Episode::is_valid)
        .map(|episode| {
            let text = format!(
                "{} {} {}",
                episode.content.primary,
                episode.content.secondary.as_deref().unwrap_or(""),
                episode.content.context.as_deref().unwrap_or("")
            );
            let tokens = terms(&text);
            let length = tokens.len();
            let mut frequencies = HashMap::<String, usize>::new();
            for token in tokens {
                *frequencies.entry(token).or_default() += 1;
            }
            (episode, length, frequencies)
        })
        .collect();
    if documents.is_empty() {
        return Vec::new();
    }
    let count = documents.len() as f64;
    let average_length = documents
        .iter()
        .map(|(_, length, _)| *length as f64)
        .sum::<f64>()
        / count;
    if average_length == 0.0 {
        return Vec::new();
    }
    let idfs: Vec<_> = query
        .into_iter()
        .map(|term| {
            let frequency = documents
                .iter()
                .filter(|(_, _, terms)| terms.contains_key(&term))
                .count() as f64;
            let idf = (1.0 + (count - frequency + 0.5) / (frequency + 0.5)).ln();
            (term, idf)
        })
        .collect();
    let mut results = Vec::new();
    for (episode, length, frequencies) in documents {
        let mut score = 0.0;
        for (term, idf) in &idfs {
            let tf = *frequencies.get(term).unwrap_or(&0) as f64;
            let denominator = tf + 1.2 * (0.25 + 0.75 * length as f64 / average_length);
            score += idf * tf * 2.2 / denominator;
        }
        if score > 0.0 {
            results.push(KeywordSearchResult { episode, score });
        }
    }
    results.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.episode.id.as_uuid().cmp(&b.episode.id.as_uuid()))
    });
    results.truncate(limit);
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::episode::EpisodeId;

    #[test]
    fn retrieval_bm25_ranks_term_frequency_and_length_independent_of_input_order() {
        let focused = Episode::observation("a", "memory memory");
        let verbose = Episode::observation(
            "a",
            "memory plus many irrelevant words that dilute the signal",
        );
        let irrelevant = Episode::observation("a", "unrelated words");
        let documents = vec![verbose, irrelevant, focused.clone()];
        let first = rank_episodes(documents.clone(), "memory", 10);
        let second = rank_episodes(documents.into_iter().rev().collect(), "MEMORY memory", 10);
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].episode.id, focused.id);
        assert_eq!(
            first
                .iter()
                .map(|r| (r.episode.id, r.score))
                .collect::<Vec<_>>(),
            second
                .iter()
                .map(|r| (r.episode.id, r.score))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn retrieval_bm25_uses_unicode_tokens_secondary_context_and_filters_invalid_records() {
        let mut valid = Episode::conversation("a", "", "MEMÓRIA");
        valid.content.context = Some("東京".into());
        let mut invalid = valid.clone();
        invalid.invalidate();
        assert_eq!(
            rank_episodes(vec![valid.clone(), invalid], "memória 東京", 10).len(),
            1
        );
        assert!(rank_episodes(vec![valid.clone()], "mem", 10).is_empty());
        assert!(rank_episodes(vec![valid.clone()], "!!!", 10).is_empty());
        assert!(rank_episodes(vec![valid], "東京", 0).is_empty());
    }

    #[test]
    fn retrieval_bm25_single_document_matches_formula_and_ties_are_stable() {
        let mut one = Episode::observation("a", "memory");
        one.id = EpisodeId::from_uuid(uuid::Uuid::from_u128(1));
        let result = rank_episodes(vec![one.clone()], "memory", 1);
        assert!((result[0].score - (4.0_f64 / 3.0).ln()).abs() < 1e-12);
        let mut two = one.clone();
        two.id = EpisodeId::from_uuid(uuid::Uuid::from_u128(2));
        assert_eq!(
            rank_episodes(vec![two, one.clone()], "memory", 1)[0]
                .episode
                .id,
            one.id
        );
    }
}
