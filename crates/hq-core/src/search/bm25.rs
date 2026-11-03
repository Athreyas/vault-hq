//! BM25 over chunk tokens: exact words, ranked by how rare they are across the vault.

use std::collections::HashMap;

use super::chunk::Chunk;

const K1: f64 = 1.2;
const B: f64 = 0.75;

#[derive(Debug, Default)]
pub struct Bm25 {
    /// Chunks each token appears in.
    df: HashMap<String, u32>,
    /// Term counts per chunk, in the index's chunk order.
    tf: Vec<HashMap<String, u32>>,
    lens: Vec<u32>,
    avg_len: f64,
}

impl Bm25 {
    pub fn build(chunks: &[Chunk]) -> Bm25 {
        let mut df: HashMap<String, u32> = HashMap::new();
        let mut tf = Vec::with_capacity(chunks.len());
        let mut lens = Vec::with_capacity(chunks.len());
        for c in chunks {
            let mut counts: HashMap<String, u32> = HashMap::new();
            for t in &c.tokens {
                *counts.entry(t.clone()).or_default() += 1;
            }
            for t in counts.keys() {
                *df.entry(t.clone()).or_default() += 1;
            }
            lens.push(c.tokens.len() as u32);
            tf.push(counts);
        }
        let avg_len = lens.iter().map(|l| *l as f64).sum::<f64>() / (lens.len().max(1) as f64);
        Bm25 {
            df,
            tf,
            lens,
            avg_len,
        }
    }

    /// The score of chunk `i` for the query tokens (0 when no token matches).
    pub fn score(&self, query: &[String], i: usize) -> f64 {
        let (Some(counts), Some(len)) = (self.tf.get(i), self.lens.get(i)) else {
            return 0.0;
        };
        let n = self.tf.len() as f64;
        let norm = 1.0 - B + B * (*len as f64 / self.avg_len.max(1e-9));
        query
            .iter()
            .filter_map(|q| {
                let f = *counts.get(q)? as f64;
                let df = *self.df.get(q).unwrap_or(&0) as f64;
                let idf = (1.0 + (n - df + 0.5) / (df + 0.5)).ln();
                Some(idf * (f * (K1 + 1.0)) / (f + K1 * norm))
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::super::chunk::{chunk_note, tokenize};
    use super::*;
    use crate::vault::Note;

    fn chunk(rel: &str, body: &str) -> Chunk {
        let n = Note {
            rel: rel.into(),
            stem: rel.trim_end_matches(".md").into(),
            props: Default::default(),
            body: body.into(),
            hash: String::new(),
            mtime: 0,
        };
        chunk_note("v", &n).remove(0)
    }

    #[test]
    fn rare_words_and_frequency_rank_higher() {
        let chunks = vec![
            chunk(
                "a.md",
                "the garden fence needs a drip line and the drip line needs parts",
            ),
            chunk(
                "b.md",
                "the garden has tomatoes and beans growing along the path",
            ),
            chunk(
                "c.md",
                "invoice for the plumber, paid in october, garden unrelated",
            ),
            chunk(
                "d.md",
                "nothing relevant here at all, only filler words to pad it",
            ),
        ];
        let idx = Bm25::build(&chunks);
        let q = tokenize("drip line");
        let s: Vec<f64> = (0..chunks.len()).map(|i| idx.score(&q, i)).collect();
        assert!(s[0] > 0.0);
        assert_eq!(s[1], 0.0);
        assert_eq!(s[3], 0.0);
        // "garden" is in three chunks, "tomatoes" in one: the rare word weighs more
        let g = idx.score(&tokenize("garden"), 1);
        let t = idx.score(&tokenize("tomatoes"), 1);
        assert!(t > g, "{t} > {g}");
        // a word said twice beats once, other things equal
        let two = idx.score(&tokenize("drip"), 0);
        let one = idx.score(&tokenize("parts"), 0);
        assert!(two > one);
        assert_eq!(idx.score(&q, 99), 0.0);
    }
}
