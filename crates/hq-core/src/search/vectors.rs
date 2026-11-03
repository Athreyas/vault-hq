//! Embedding vectors, cached on disk per vault (SPEC 6): keyed by (vault id, rel, chunk
//! hash, model). Only chunks whose text changed are embedded again; a different model
//! drops the vault's vectors, since vectors from two models can't be compared.
//!
//! The cache is the app's own file in its cache dir, never in the vault, and can be lost
//! at any time: a missing or unreadable file just means everything is embedded again.
//!
//! File `<cache>/vectors/<vault id>.bin`, little endian:
//! `HQV1`, model (u32 len + utf-8), dims u32, count u32, then per entry
//! rel (u32 len + utf-8), hash (u32 len + utf-8), dims × f32.

use std::collections::{HashMap, HashSet};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use super::chunk::Chunk;
use crate::error::{Error, Result};
use crate::locks::sha256;
use crate::ollama::{CancelToken, Embedder};

const MAGIC: &[u8; 4] = b"HQV1";
/// Inputs per embedding request: big enough to be quick, small enough to time out rarely.
pub const BATCH: usize = 32;

type Key = (String, String);

#[derive(Debug)]
pub struct VectorStore {
    path: PathBuf,
    model: String,
    dims: usize,
    map: HashMap<Key, Vec<f32>>,
    dirty: bool,
}

impl VectorStore {
    pub fn file_for(cache_dir: &Path, vault: &str) -> PathBuf {
        let safe = if !vault.is_empty()
            && vault
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            vault.to_string()
        } else {
            sha256(vault)
        };
        cache_dir.join("vectors").join(format!("{safe}.bin"))
    }

    /// The vault's cached vectors for `model`; empty if there are none, they are for another
    /// model, or the file is damaged.
    pub fn open(cache_dir: &Path, vault: &str, model: &str) -> VectorStore {
        let path = Self::file_for(cache_dir, vault);
        let mut s = VectorStore {
            path,
            model: model.to_string(),
            dims: 0,
            map: HashMap::new(),
            dirty: false,
        };
        match read_file(&s.path) {
            Ok(Some((m, dims, map))) if m == model => {
                s.dims = dims;
                s.map = map;
            }
            // another model's vectors, or a damaged file: start over and replace it on save
            Ok(Some(_)) | Err(_) => s.dirty = true,
            Ok(None) => {}
        }
        s
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn get(&self, rel: &str, hash: &str) -> Option<&[f32]> {
        self.map
            .get(&(rel.to_string(), hash.to_string()))
            .map(Vec::as_slice)
    }

    pub fn missing<'a>(&self, chunks: &[&'a Chunk]) -> Vec<&'a Chunk> {
        let mut seen = HashSet::new();
        chunks
            .iter()
            .copied()
            .filter(|c| {
                let k = (c.rel.clone(), c.hash.clone());
                !self.map.contains_key(&k) && seen.insert(k)
            })
            .collect()
    }

    pub fn insert(&mut self, rel: &str, hash: &str, v: Vec<f32>) -> Result<()> {
        if self.dims == 0 {
            self.dims = v.len();
        }
        if v.len() != self.dims || v.is_empty() {
            return Err(Error::Internal(format!(
                "The embedding model {} returned vectors of different sizes ({} and {}).",
                self.model,
                self.dims,
                v.len()
            )));
        }
        self.map.insert((rel.to_string(), hash.to_string()), v);
        self.dirty = true;
        Ok(())
    }

    /// Embed the chunks that have no vector yet, in batches. Returns how many were embedded.
    /// Stops between batches when cancelled; what was embedded so far is kept.
    pub fn ensure(
        &mut self,
        chunks: &[&Chunk],
        embedder: &dyn Embedder,
        cancel: Option<&CancelToken>,
    ) -> Result<usize> {
        if embedder.model() != self.model {
            return Err(Error::Internal(format!(
                "Vectors for {} can't be filled by {}.",
                self.model,
                embedder.model()
            )));
        }
        let missing = self.missing(chunks);
        let mut done = 0;
        for batch in missing.chunks(BATCH) {
            if cancel.is_some_and(CancelToken::is_cancelled) {
                break;
            }
            let inputs: Vec<String> = batch.iter().map(|c| c.embed_text()).collect();
            let out = embedder.embed(&inputs)?;
            for (c, v) in batch.iter().zip(out) {
                self.insert(&c.rel, &c.hash, v)?;
                done += 1;
            }
        }
        Ok(done)
    }

    /// Forget vectors of chunks that no longer exist, so the cache doesn't grow forever.
    /// `live` must be every chunk of the vault, not a scoped subset.
    pub fn retain_live<'a>(&mut self, live: impl IntoIterator<Item = &'a Chunk>) {
        let keep: HashSet<Key> = live
            .into_iter()
            .map(|c| (c.rel.clone(), c.hash.clone()))
            .collect();
        let before = self.map.len();
        self.map.retain(|k, _| keep.contains(k));
        if self.map.len() != before {
            self.dirty = true;
        }
    }

    /// Write the file if anything changed (temp file + rename: it is the app's own file).
    pub fn save(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("bin.tmp");
        {
            let mut w = BufWriter::new(std::fs::File::create(&tmp)?);
            w.write_all(MAGIC)?;
            write_str(&mut w, &self.model)?;
            w.write_all(&(self.dims as u32).to_le_bytes())?;
            w.write_all(&(self.map.len() as u32).to_le_bytes())?;
            for ((rel, hash), v) in &self.map {
                write_str(&mut w, rel)?;
                write_str(&mut w, hash)?;
                for x in v {
                    w.write_all(&x.to_le_bytes())?;
                }
            }
            w.flush()?;
        }
        std::fs::rename(&tmp, &self.path)?;
        self.dirty = false;
        Ok(())
    }
}

pub fn cosine(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0f64, 0f64, 0f64);
    for (x, y) in a.iter().zip(b) {
        let (x, y) = (*x as f64, *y as f64);
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    let d = (na * nb).sqrt();
    if d == 0.0 {
        0.0
    } else {
        dot / d
    }
}

fn write_str(w: &mut impl Write, s: &str) -> std::io::Result<()> {
    w.write_all(&(s.len() as u32).to_le_bytes())?;
    w.write_all(s.as_bytes())
}

fn read_u32(r: &mut impl Read) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_str(r: &mut impl Read) -> std::io::Result<String> {
    let n = read_u32(r)? as usize;
    if n > 1 << 16 {
        return Err(std::io::Error::other("string too long"));
    }
    let mut b = vec![0u8; n];
    r.read_exact(&mut b)?;
    String::from_utf8(b).map_err(std::io::Error::other)
}

type Loaded = (String, usize, HashMap<Key, Vec<f32>>);

fn read_file(path: &Path) -> std::io::Result<Option<Loaded>> {
    let f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut r = BufReader::new(f);
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(std::io::Error::other("not a vector cache"));
    }
    let model = read_str(&mut r)?;
    let dims = read_u32(&mut r)? as usize;
    let count = read_u32(&mut r)? as usize;
    if dims > 1 << 16 {
        return Err(std::io::Error::other("implausible dimensions"));
    }
    let mut map = HashMap::with_capacity(count.min(1 << 20));
    let mut buf = vec![0u8; dims * 4];
    for _ in 0..count {
        let rel = read_str(&mut r)?;
        let hash = read_str(&mut r)?;
        r.read_exact(&mut buf)?;
        let v = buf
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        map.insert((rel, hash), v);
    }
    Ok(Some((model, dims, map)))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::search::chunk::chunk_note;
    use crate::vault::Note;
    use std::cell::RefCell;

    /// Counts what it is asked to embed; a vector per input from its letters.
    pub(crate) struct FakeEmbedder {
        pub model: String,
        pub calls: RefCell<Vec<usize>>,
    }

    impl FakeEmbedder {
        pub fn new(model: &str) -> Self {
            FakeEmbedder {
                model: model.into(),
                calls: RefCell::new(vec![]),
            }
        }
    }

    /// A crude bag of letters: texts sharing words point the same way.
    pub(crate) fn letters(s: &str) -> Vec<f32> {
        let mut v = vec![0f32; 26];
        for c in s.to_lowercase().bytes().filter(u8::is_ascii_lowercase) {
            v[(c - b'a') as usize] += 1.0;
        }
        v
    }

    impl Embedder for FakeEmbedder {
        fn model(&self) -> &str {
            &self.model
        }
        fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
            self.calls.borrow_mut().push(inputs.len());
            Ok(inputs.iter().map(|s| letters(s)).collect())
        }
    }

    fn chunks(body: &str) -> Vec<Chunk> {
        let n = Note {
            rel: "01-Projects/garden.md".into(),
            stem: "garden".into(),
            props: Default::default(),
            body: body.into(),
            hash: String::new(),
            mtime: 0,
        };
        chunk_note("v", &n)
    }

    const BODY: &str = "## One\nthe first section, about the fence line\n## Two\nthe second section, about tomatoes and beans\n";

    #[test]
    fn only_changed_chunks_are_embedded_again() {
        let dir = tempfile::tempdir().unwrap();
        let e = FakeEmbedder::new("nomic-embed-text");
        let cs = chunks(BODY);
        let refs: Vec<&Chunk> = cs.iter().collect();
        let mut s = VectorStore::open(dir.path(), "vault1", "nomic-embed-text");
        assert_eq!(s.ensure(&refs, &e, None).unwrap(), 2);
        s.save().unwrap();

        // reopened from disk: nothing to do
        let mut s = VectorStore::open(dir.path(), "vault1", "nomic-embed-text");
        assert_eq!(s.len(), 2);
        assert_eq!(s.ensure(&refs, &e, None).unwrap(), 0);
        assert_eq!(
            s.get(&cs[0].rel, &cs[0].hash).unwrap(),
            letters(&cs[0].embed_text())
        );

        // one section edited: one embedding, and the stale vector is dropped
        let edited = chunks(&BODY.replace("tomatoes", "peppers"));
        let refs: Vec<&Chunk> = edited.iter().collect();
        assert_eq!(s.ensure(&refs, &e, None).unwrap(), 1);
        s.retain_live(edited.iter());
        assert_eq!(s.len(), 2);
        assert!(s.get(&cs[1].rel, &cs[1].hash).is_none());
        assert_eq!(*e.calls.borrow(), vec![2, 1]);
    }

    #[test]
    fn a_new_model_drops_the_vectors() {
        let dir = tempfile::tempdir().unwrap();
        let cs = chunks(BODY);
        let refs: Vec<&Chunk> = cs.iter().collect();
        let mut s = VectorStore::open(dir.path(), "vault1", "model-a");
        s.ensure(&refs, &FakeEmbedder::new("model-a"), None)
            .unwrap();
        s.save().unwrap();
        let mut s = VectorStore::open(dir.path(), "vault1", "model-b");
        assert!(s.is_empty());
        s.save().unwrap();
        // the file now belongs to model-b, so model-a starts over too
        assert!(VectorStore::open(dir.path(), "vault1", "model-a").is_empty());
        // and a mismatched embedder is refused rather than mixing vectors
        let mut s = VectorStore::open(dir.path(), "vault1", "model-b");
        assert!(s
            .ensure(&refs, &FakeEmbedder::new("model-a"), None)
            .is_err());
    }

    #[test]
    fn damaged_files_and_odd_ids_are_survivable() {
        let dir = tempfile::tempdir().unwrap();
        let p = VectorStore::file_for(dir.path(), "../../etc");
        assert!(p.starts_with(dir.path().join("vectors")));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(VectorStore::file_for(dir.path(), "v"), b"HQV1garbage").unwrap();
        let s = VectorStore::open(dir.path(), "v", "m");
        assert!(s.is_empty());
    }

    #[test]
    fn cancel_stops_between_batches() {
        let dir = tempfile::tempdir().unwrap();
        let body: String = (0..(BATCH + 5))
            .map(|i| {
                format!("## Section {i}\nsome words that make section number {i} long enough\n")
            })
            .collect();
        let cs = chunks(&body);
        let refs: Vec<&Chunk> = cs.iter().collect();
        let cancel = CancelToken::new();
        cancel.cancel();
        let mut s = VectorStore::open(dir.path(), "v", "m");
        assert_eq!(
            s.ensure(&refs, &FakeEmbedder::new("m"), Some(&cancel))
                .unwrap(),
            0
        );
        assert_eq!(
            s.ensure(&refs, &FakeEmbedder::new("m"), None).unwrap(),
            BATCH + 5
        );
    }

    #[test]
    fn cosine_basics() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-9);
        assert_eq!(cosine(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 2.0]), 0.0);
    }
}
