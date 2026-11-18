//! Content fingerprints and conservative file identity reconciliation.
//!
//! This module deliberately has no crate dependencies so it can be developed
//! and tested before the server crate and shared import harness are registered.

use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const DEFAULT_QUICK_HASH_CHUNK_SIZE: usize = 65_536;
pub const DEFAULT_FULL_HASH_BUFFER_SIZE: usize = 1_048_576;

/// SHA-256 of the file size encoded as decimal UTF-8 followed by the first
/// `chunk_size` bytes. This matches the Python importer's sparse quick hash.
pub fn compute_quick_hash<R: Read + Seek>(
    reader: &mut R,
    file_size: u64,
    chunk_size: usize,
) -> io::Result<String> {
    reader.seek(SeekFrom::Start(0))?;
    let mut hasher = Sha256::new();
    hasher.update(file_size.to_string().as_bytes());
    let mut remaining = chunk_size;
    let mut buffer = [0u8; 8192];
    while remaining > 0 {
        let requested = remaining.min(buffer.len());
        let count = reader.read(&mut buffer[..requested])?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        remaining -= count;
    }
    Ok(hasher.finish_hex())
}

/// SHA-256 of the complete stream, read in bounded chunks.
pub fn compute_full_hash<R: Read>(reader: &mut R) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; DEFAULT_FULL_HASH_BUFFER_SIZE];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finish_hex())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownFile {
    pub path: PathBuf,
    pub file_id: String,
    pub photo_id: String,
    pub quick_hash: String,
    pub full_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedFile {
    pub path: PathBuf,
    pub quick_hash: String,
    pub full_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Unchanged {
        path: PathBuf,
        file_id: String,
        photo_id: String,
    },
    Modified {
        path: PathBuf,
        file_id: String,
        photo_id: String,
        previous_full_hash: String,
    },
    Moved {
        from: PathBuf,
        to: PathBuf,
        file_id: String,
        photo_id: String,
    },
    Added {
        path: PathBuf,
    },
    Missing {
        path: PathBuf,
        file_id: String,
        photo_id: String,
    },
}

/// Match current observations against known files. A move is inferred only
/// when one unmatched old file and one unmatched observation share the same
/// full hash. Ambiguous duplicates remain separate Added/Missing records.
///
/// Exact-path matches are resolved first. This is what preserves a copy at a
/// second path as a new file when the identical original is still present.
pub fn reconcile(known: &[KnownFile], observed: &[ObservedFile]) -> Vec<Change> {
    let mut changes = Vec::new();
    let mut old_used = vec![false; known.len()];
    let mut new_used = vec![false; observed.len()];

    for (new_index, current) in observed.iter().enumerate() {
        if let Some((old_index, previous)) = known
            .iter()
            .enumerate()
            .find(|(index, previous)| !old_used[*index] && previous.path == current.path)
        {
            old_used[old_index] = true;
            new_used[new_index] = true;
            if previous.full_hash == current.full_hash {
                changes.push(Change::Unchanged {
                    path: current.path.clone(),
                    file_id: previous.file_id.clone(),
                    photo_id: previous.photo_id.clone(),
                });
            } else {
                changes.push(Change::Modified {
                    path: current.path.clone(),
                    file_id: previous.file_id.clone(),
                    photo_id: previous.photo_id.clone(),
                    previous_full_hash: previous.full_hash.clone(),
                });
            }
        }
    }

    let mut old_hashes = std::collections::BTreeMap::<&str, Vec<usize>>::new();
    let mut new_hashes = std::collections::BTreeMap::<&str, Vec<usize>>::new();
    for (index, file) in known
        .iter()
        .enumerate()
        .filter(|(index, _)| !old_used[*index])
    {
        old_hashes.entry(&file.full_hash).or_default().push(index);
    }
    for (index, file) in observed
        .iter()
        .enumerate()
        .filter(|(index, _)| !new_used[*index])
    {
        new_hashes.entry(&file.full_hash).or_default().push(index);
    }

    for (hash, old_indices) in old_hashes {
        let Some(new_indices) = new_hashes.get(hash) else {
            continue;
        };
        if old_indices.len() == 1 && new_indices.len() == 1 {
            let old_index = old_indices[0];
            let new_index = new_indices[0];
            old_used[old_index] = true;
            new_used[new_index] = true;
            changes.push(Change::Moved {
                from: known[old_index].path.clone(),
                to: observed[new_index].path.clone(),
                file_id: known[old_index].file_id.clone(),
                photo_id: known[old_index].photo_id.clone(),
            });
        }
    }

    for (index, file) in observed.iter().enumerate() {
        if !new_used[index] {
            changes.push(Change::Added {
                path: file.path.clone(),
            });
        }
    }
    for (index, file) in known.iter().enumerate() {
        if !old_used[index] {
            changes.push(Change::Missing {
                path: file.path.clone(),
                file_id: file.file_id.clone(),
                photo_id: file.photo_id.clone(),
            });
        }
    }
    changes.sort_by(|left, right| change_path(left).cmp(change_path(right)));
    changes
}

fn change_path(change: &Change) -> &Path {
    match change {
        Change::Unchanged { path, .. }
        | Change::Modified { path, .. }
        | Change::Added { path }
        | Change::Missing { path, .. } => path,
        Change::Moved { to, .. } => to,
    }
}

// Small self-contained SHA-256 implementation keeps the module buildable
// before the future server manifest can declare a shared hashing dependency.
struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffered: usize,
    bit_len: u64,
}

impl Sha256 {
    const INITIAL: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    fn new() -> Self {
        Self {
            state: Self::INITIAL,
            buffer: [0; 64],
            buffered: 0,
            bit_len: 0,
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.bit_len = self
            .bit_len
            .wrapping_add((data.len() as u64).wrapping_mul(8));
        if self.buffered > 0 {
            let take = (64 - self.buffered).min(data.len());
            self.buffer[self.buffered..self.buffered + take].copy_from_slice(&data[..take]);
            self.buffered += take;
            data = &data[take..];
            if self.buffered == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffered = 0;
            } else {
                return;
            }
        }
        while data.len() >= 64 {
            let (block, rest) = data.split_at(64);
            self.compress(block);
            data = rest;
        }
        self.buffer[..data.len()].copy_from_slice(data);
        self.buffered = data.len();
    }

    fn finish_hex(mut self) -> String {
        let original_bit_len = self.bit_len;
        self.buffer[self.buffered] = 0x80;
        self.buffered += 1;
        if self.buffered > 56 {
            self.buffer[self.buffered..].fill(0);
            let block = self.buffer;
            self.compress(&block);
            self.buffer = [0; 64];
        } else {
            self.buffer[self.buffered..56].fill(0);
        }
        self.buffer[56..64].copy_from_slice(&original_bit_len.to_be_bytes());
        let block = self.buffer;
        self.compress(&block);
        let mut output = String::with_capacity(64);
        for word in self.state {
            use std::fmt::Write;
            let _ = write!(output, "{word:08x}");
        }
        output
    }

    fn compress(&mut self, block: &[u8]) {
        let mut words = [0u32; 64];
        for (index, bytes) in block.chunks_exact(4).enumerate() {
            words[index] = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        }
        for index in 16..64 {
            let x = words[index - 15];
            let y = words[index - 2];
            let s0 = x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3);
            let s1 = y.rotate_right(17) ^ y.rotate_right(19) ^ (y >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for index in 0..64 {
            let sum1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ ((!e) & g);
            let t1 = h
                .wrapping_add(sum1)
                .wrapping_add(choose)
                .wrapping_add(Self::K[index])
                .wrapping_add(words[index]);
            let sum0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = sum0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (state, value) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *state = state.wrapping_add(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn known(path: &str, file_id: &str, photo_id: &str, hash: &str) -> KnownFile {
        KnownFile {
            path: path.into(),
            file_id: file_id.into(),
            photo_id: photo_id.into(),
            quick_hash: "q".into(),
            full_hash: hash.into(),
        }
    }
    fn seen(path: &str, hash: &str) -> ObservedFile {
        ObservedFile {
            path: path.into(),
            quick_hash: "q".into(),
            full_hash: hash.into(),
        }
    }

    #[test]
    fn full_hash_matches_sha256_compatibility_vectors() {
        assert_eq!(
            compute_full_hash(&mut Cursor::new(b"abc")).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            compute_full_hash(&mut Cursor::new(b"")).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn quick_hash_matches_python_sparse_hash_vector() {
        assert_eq!(
            compute_quick_hash(&mut Cursor::new(b"abc"), 3, 65_536).unwrap(),
            "7b0393befdc28ff2a7c5e08fa8a38a4c845da5c600476eb01ae7b865fa344f18"
        );
    }

    #[test]
    fn unique_content_match_reuses_identity_for_a_move() {
        let changes = reconcile(
            &[known("/old/a.raw", "file-1", "photo-1", "hash-a")],
            &[seen("/new/a.raw", "hash-a")],
        );
        assert_eq!(
            changes,
            vec![Change::Moved {
                from: "/old/a.raw".into(),
                to: "/new/a.raw".into(),
                file_id: "file-1".into(),
                photo_id: "photo-1".into()
            }]
        );
    }

    #[test]
    fn same_path_with_new_content_is_modified() {
        let changes = reconcile(
            &[known("/photos/a.jpg", "file-1", "photo-1", "old")],
            &[seen("/photos/a.jpg", "new")],
        );
        assert_eq!(
            changes,
            vec![Change::Modified {
                path: "/photos/a.jpg".into(),
                file_id: "file-1".into(),
                photo_id: "photo-1".into(),
                previous_full_hash: "old".into()
            }]
        );
    }

    #[test]
    fn identical_copy_at_second_present_path_keeps_two_file_records() {
        let changes = reconcile(
            &[known("/photos/a.jpg", "file-1", "photo-1", "same")],
            &[seen("/photos/a.jpg", "same"), seen("/backup/a.jpg", "same")],
        );
        assert!(changes.iter().any(|change| matches!(change, Change::Unchanged { path, file_id, .. } if path == Path::new("/photos/a.jpg") && file_id == "file-1")));
        assert!(changes.iter().any(
            |change| matches!(change, Change::Added { path } if path == Path::new("/backup/a.jpg"))
        ));
        assert_eq!(changes.len(), 2);
    }

    #[test]
    fn ambiguous_duplicate_hashes_are_not_guessed_as_moves() {
        let known = [
            known("/old/a.jpg", "f1", "p1", "same"),
            known("/old/b.jpg", "f2", "p2", "same"),
        ];
        let observed = [seen("/new/a.jpg", "same"), seen("/new/b.jpg", "same")];
        let changes = reconcile(&known, &observed);
        assert_eq!(
            changes
                .iter()
                .filter(|change| matches!(change, Change::Added { .. }))
                .count(),
            2
        );
        assert_eq!(
            changes
                .iter()
                .filter(|change| matches!(change, Change::Missing { .. }))
                .count(),
            2
        );
        assert!(!changes
            .iter()
            .any(|change| matches!(change, Change::Moved { .. })));
    }
}
