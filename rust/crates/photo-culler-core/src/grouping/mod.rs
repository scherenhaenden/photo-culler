//! Deterministic temporal and perceptual grouping for photo review.
//!
//! This module is intentionally standalone until the core crate registers it.

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame<'a> {
    /// Existing catalog identity. Grouping only borrows this value and never rewrites it.
    pub photo_id: &'a str,
    pub session_id: Option<&'a str>,
    /// Capture time in UTC milliseconds, when known.
    pub captured_at_ms: Option<i64>,
    /// 64-bit difference hash, when available.
    pub dhash: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub burst_gap_ms: i64,
    pub perceptual_window_ms: i64,
    pub max_hamming_distance: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            burst_gap_ms: 1_500,
            perceptual_window_ms: 10 * 60 * 1_000,
            max_hamming_distance: 8,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Evidence {
    Temporal { gap_ms: i64 },
    Perceptual { hamming_distance: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// Stable for the same sorted member identities.
    pub group_id: String,
    /// Sorted by capture time (unknown times last), then by catalog identity.
    pub photo_ids: Vec<String>,
    /// Human-readable, stable pairwise reasons for connected members.
    pub evidence: Vec<(String, String, Evidence)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grouping {
    pub groups: Vec<Group>,
}

pub fn group(frames: &[Frame<'_>], config: Config) -> Grouping {
    assert!(config.burst_gap_ms >= 0 && config.perceptual_window_ms >= 0);
    let mut ordered: Vec<usize> = (0..frames.len()).collect();
    ordered.sort_by_key(|&i| {
        (
            frames[i].session_id.unwrap_or(""),
            frames[i].captured_at_ms.unwrap_or(i64::MAX),
            frames[i].photo_id,
        )
    });
    let mut parent: Vec<usize> = (0..frames.len()).collect();
    let mut evidence: Vec<(usize, usize, Evidence)> = Vec::new();
    for pos in 0..ordered.len() {
        let i = ordered[pos];
        for &j in &ordered[..pos] {
            if frames[i].session_id != frames[j].session_id {
                continue;
            }
            if let (Some(a), Some(b)) = (frames[i].captured_at_ms, frames[j].captured_at_ms) {
                let gap = a.abs_diff(b).min(i64::MAX as u64) as i64;
                if gap <= config.burst_gap_ms {
                    union(&mut parent, i, j);
                    evidence.push((i, j, Evidence::Temporal { gap_ms: gap }));
                } else if gap <= config.perceptual_window_ms {
                    if let (Some(x), Some(y)) = (frames[i].dhash, frames[j].dhash) {
                        let distance = (x ^ y).count_ones();
                        if distance <= config.max_hamming_distance {
                            union(&mut parent, i, j);
                            evidence.push((
                                i,
                                j,
                                Evidence::Perceptual {
                                    hamming_distance: distance,
                                },
                            ));
                        }
                    }
                }
            }
        }
    }
    let mut members: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..frames.len() {
        let root = find(&mut parent, i);
        members.entry(root).or_default().push(i);
    }
    let mut groups = Vec::new();
    for indexes in members.values().filter(|v| v.len() > 1) {
        let mut ids: Vec<String> = indexes
            .iter()
            .map(|&i| frames[i].photo_id.to_owned())
            .collect();
        ids.sort();
        let id = stable_id(&ids);
        let mut chronological = indexes.clone();
        chronological.sort_by_key(|&i| {
            (
                frames[i].captured_at_ms.unwrap_or(i64::MAX),
                frames[i].photo_id,
            )
        });
        let photo_ids = chronological
            .iter()
            .map(|&i| frames[i].photo_id.to_owned())
            .collect();
        let mut reasons: Vec<_> = evidence
            .iter()
            .filter(|(a, b, _)| indexes.contains(a) && indexes.contains(b))
            .map(|(a, b, e)| {
                let (left, right) = if frames[*a].photo_id <= frames[*b].photo_id {
                    (
                        frames[*a].photo_id.to_owned(),
                        frames[*b].photo_id.to_owned(),
                    )
                } else {
                    (
                        frames[*b].photo_id.to_owned(),
                        frames[*a].photo_id.to_owned(),
                    )
                };
                (left, right, e.clone())
            })
            .collect();
        reasons.sort();
        groups.push(Group {
            group_id: id,
            photo_ids,
            evidence: reasons,
        });
    }
    groups.sort_by(|a, b| a.group_id.cmp(&b.group_id));
    Grouping { groups }
}

fn find(parent: &mut [usize], x: usize) -> usize {
    if parent[x] != x {
        parent[x] = find(parent, parent[x]);
    }
    parent[x]
}
fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
        parent[hi] = lo;
    }
}
fn stable_id(sorted_ids: &[String]) -> String {
    // FNV-1a over length-prefixed IDs avoids delimiter ambiguity and external dependencies.
    let mut hash = 0xcbf29ce484222325u64;
    for id in sorted_ids {
        for byte in (id.len() as u64).to_le_bytes().iter().chain(id.as_bytes()) {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("group-{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<Frame<'static>> {
        vec![
            Frame {
                photo_id: "a",
                session_id: Some("s1"),
                captured_at_ms: Some(1000),
                dhash: Some(0),
            },
            Frame {
                photo_id: "b",
                session_id: Some("s1"),
                captured_at_ms: Some(1500),
                dhash: Some(0xffff),
            },
            Frame {
                photo_id: "c",
                session_id: Some("s1"),
                captured_at_ms: Some(10_000),
                dhash: Some(1),
            },
            Frame {
                photo_id: "d",
                session_id: Some("s1"),
                captured_at_ms: Some(10_500),
                dhash: Some(u64::MAX),
            },
            Frame {
                photo_id: "e",
                session_id: Some("s2"),
                captured_at_ms: Some(11_000),
                dhash: Some(0),
            },
            Frame {
                photo_id: "f",
                session_id: Some("s1"),
                captured_at_ms: Some(900_000),
                dhash: Some(0),
            },
        ]
    }
    #[test]
    fn permutation_does_not_change_groups_or_reasons() {
        let input = fixture();
        let expected = group(&input, Config::default());
        for shift in 0..input.len() {
            let mut p = input.clone();
            p.rotate_left(shift);
            assert_eq!(group(&p, Config::default()), expected);
        }
        let reversed: Vec<_> = input.iter().rev().cloned().collect();
        assert_eq!(group(&reversed, Config::default()), expected);
    }
    #[test]
    fn identity_and_session_boundaries_are_preserved() {
        let input = fixture();
        let before: Vec<_> = input
            .iter()
            .map(|f| (f.photo_id, f.session_id, f.captured_at_ms))
            .collect();
        let result = group(&input, Config::default());
        assert_eq!(
            before,
            input
                .iter()
                .map(|f| (f.photo_id, f.session_id, f.captured_at_ms))
                .collect::<Vec<_>>()
        );
        assert!(result
            .groups
            .iter()
            .all(|g| !g.photo_ids.contains(&"e".to_string())
                || g.photo_ids.iter().all(|id| id == "e")));
        assert!(result
            .groups
            .iter()
            .all(|g| !g.photo_ids.contains(&"f".to_string())));
    }
}
