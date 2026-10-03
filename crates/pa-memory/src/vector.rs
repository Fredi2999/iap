//! Reine Rust-Vektoroperationen; sqlite-vec ist in unserem Bundle nicht
//! verfügbar, für die erwartete Datenmenge (bis niedrige fünfstellige
//! Anzahl Fakten/Chunks) reicht ein Table-Scan über L2-normalisierte
//! Vektoren pro Anfrage aus.

use pa_types::memory::{ItemType, MemoryHit};

/// Cosinus-Ähnlichkeit zweier Vektoren gleicher Länge.
///
/// Für nicht-normalisierte Vektoren wird die Norm im Nenner mit
/// einbezogen. Bei zwei Null-Vektoren gibt es keine sinnvolle
/// Ähnlichkeit; die Funktion liefert dann `0.0`.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(
        a.len(),
        b.len(),
        "Cosinus-Ähnlichkeit verlangt gleiche Dimension"
    );
    let mut dot = 0.0_f32;
    let mut norm_a = 0.0_f32;
    let mut norm_b = 0.0_f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    if norm_a <= 0.0 || norm_b <= 0.0 {
        return 0.0;
    }
    dot / (norm_a.sqrt() * norm_b.sqrt())
}

/// Rankt Kandidaten nach Cosinus-Ähnlichkeit absteigend; liefert bis zu
/// `limit` Elemente.
pub fn top_k_cosine(
    query: &[f32],
    candidates: &[(String, Vec<f32>)],
    limit: usize,
    item_type: ItemType,
) -> Vec<(ItemType, String, f32)> {
    let mut scored: Vec<(ItemType, String, f32)> = candidates
        .iter()
        .map(|(id, vector)| (item_type, id.clone(), cosine_similarity(query, vector)))
        .collect();
    scored.sort_by(|left, right| {
        right
            .2
            .partial_cmp(&left.2)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(limit);
    scored
}

/// Kombiniert zwei Ranglisten via Reciprocal Rank Fusion (Konzept 6.3).
///
/// `k` ist der Dämpfungsparameter aus der Formel `score = Σ 1/(k + rang_i)`;
/// der Konzepttext gibt `60` als Standard vor.
pub fn reciprocal_rank_fusion(
    lists: &[Vec<(ItemType, String)>],
    k: usize,
    limit: usize,
) -> Vec<(ItemType, String, f64)> {
    use std::collections::BTreeMap;
    let mut scores: BTreeMap<(ItemType, String), f64> = BTreeMap::new();
    for list in lists {
        for (rank, (item_type, item_id)) in list.iter().enumerate() {
            let contribution = 1.0 / (k as f64 + (rank + 1) as f64);
            scores
                .entry((*item_type, item_id.clone()))
                .and_modify(|current| *current += contribution)
                .or_insert(contribution);
        }
    }
    let mut ranked: Vec<((ItemType, String), f64)> = scores.into_iter().collect();
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| (a.0).1.cmp(&(b.0).1))
    });
    ranked
        .into_iter()
        .take(limit)
        .map(|((item_type, item_id), score)| (item_type, item_id, score))
        .collect()
}

/// Baut die endgültige Ergebnisliste; `preview_for` liefert für jeden Treffer
/// den anzuzeigenden Textausriss. Beobachter (z. B. Recency-Boost) können
/// den Preview- oder Score-Wert nachträglich anpassen; die Grundordnung
/// bleibt aber die aus der RRF.
pub fn assemble_hits(
    ranked: Vec<(ItemType, String, f64)>,
    mut preview_for: impl FnMut(ItemType, &str) -> String,
) -> Vec<MemoryHit> {
    ranked
        .into_iter()
        .map(|(item_type, item_id, score)| MemoryHit {
            item_type,
            preview: preview_for(item_type, &item_id),
            item_id,
            score,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_vectors_have_cosine_one() {
        let vector = vec![0.1_f32, 0.2, 0.3];
        assert!((cosine_similarity(&vector, &vector) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn orthogonal_vectors_have_cosine_zero() {
        let a = vec![1.0_f32, 0.0, 0.0];
        let b = vec![0.0_f32, 1.0, 0.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn zero_vector_returns_zero_without_panic() {
        let a = vec![0.0_f32; 4];
        let b = vec![1.0_f32; 4];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn rrf_prefers_items_present_in_multiple_lists() {
        let list_a = vec![
            (ItemType::Fact, "shared".to_owned()),
            (ItemType::Fact, "only_a".to_owned()),
        ];
        let list_b = vec![
            (ItemType::Fact, "shared".to_owned()),
            (ItemType::Fact, "only_b".to_owned()),
        ];
        let ranked = reciprocal_rank_fusion(&[list_a, list_b], 60, 10);
        assert_eq!(ranked[0].1, "shared");
    }

    #[test]
    fn top_k_cosine_returns_at_most_limit_items_in_descending_order() {
        let query = vec![1.0_f32, 0.0];
        let candidates = vec![
            ("a".to_owned(), vec![1.0_f32, 0.0]),
            ("b".to_owned(), vec![0.5, 0.5]),
            ("c".to_owned(), vec![0.0, 1.0]),
        ];
        let ranked = top_k_cosine(&query, &candidates, 2, ItemType::Fact);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].1, "a");
        assert!(ranked[0].2 >= ranked[1].2);
    }
}
