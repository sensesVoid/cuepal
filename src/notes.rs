//! M1 hardcoded notes. The editor and notes.json land in M2; until then the
//! overlay runs against this static set.

#![allow(dead_code)]

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Topic {
    pub id: &'static str,
    pub title: &'static str,
    pub cues: &'static [&'static str],
    pub bullets: &'static [&'static str],
    pub closing_cues: &'static [&'static str],
}

pub type CueIndex = &'static [Topic];

pub static M1_NOTES: CueIndex = &[
    Topic {
        id: "t_01",
        title: "System architecture",
        cues: &["on system architecture", "how i designed the system"],
        bullets: &[
            "Event-driven services",
            "Queue between stages",
            "Cut p99 latency 40%",
            "Horizontal scale",
        ],
        closing_cues: &["that's the architecture"],
    },
    Topic {
        id: "t_02",
        title: "Biggest challenge",
        cues: &["biggest challenge", "hardest problem"],
        bullets: &[
            "Migrated live traffic",
            "Zero downtime cutover",
            "Rollback in 30s",
            "Lessons learned",
        ],
        closing_cues: &[],
    },
    Topic {
        id: "t_03",
        title: "Project you're proud of",
        cues: &["project you're proud of", "tell me about a project"],
        bullets: &[
            "Shipped in 6 weeks",
            "Owned end to end",
            "Measurable impact",
            "Team of four",
        ],
        closing_cues: &[],
    },
    Topic {
        id: "t_04",
        title: "Why this company",
        cues: &["why this company", "why us"],
        bullets: &[
            "Mission alignment",
            "Scale of the problem",
            "Engineering culture",
            "Growth path",
        ],
        closing_cues: &["moving on"],
    },
];

/// Vosk grammar: every cue only, then `[unk]` (M0 behavior).
pub fn build_grammar_json(topics: &[Topic]) -> String {
    let mut phrases: Vec<&str> = Vec::new();
    for t in topics {
        phrases.extend(t.cues);
    }
    phrases.push("[unk]");
    serde_json::to_string(&phrases).expect("grammar phrases are valid json strings")
}

/// Vosk grammar: every cue + closing cue, then `[unk]`.
pub fn build_grammar_json_from_topics(topics: &[Topic]) -> String {
    let mut phrases: Vec<&str> = Vec::new();
    for t in topics {
        phrases.extend(t.cues);
        phrases.extend(t.closing_cues);
    }
    phrases.push("[unk]");
    serde_json::to_string(&phrases).expect("grammar phrases are valid json strings")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m1_notes_shape() {
        assert!(M1_NOTES.len() >= 3 && M1_NOTES.len() <= 6);
        for t in M1_NOTES {
            assert!(!t.id.is_empty() && !t.title.is_empty());
            assert!(!t.cues.is_empty() && t.cues.len() <= 3);
            assert!(t.bullets.len() >= 3 && t.bullets.len() <= 5);
            for b in t.bullets {
                assert!(b.len() <= 40, "bullet too long: {b}");
            }
        }
    }

    #[test]
    fn grammar_json_contains_every_cue_and_unk() {
        let json = build_grammar_json(M1_NOTES);
        for t in M1_NOTES {
            for c in t.cues {
                assert!(json.contains(c), "missing cue: {c}");
            }
        }
        assert!(json.ends_with(r#","[unk]"]"#));
    }

    #[test]
    fn grammar_json_from_topics_contains_all_cues_and_closing() {
        let json = build_grammar_json_from_topics(M1_NOTES);
        for t in M1_NOTES {
            for c in t.cues {
                assert!(json.contains(c), "missing cue: {c}");
            }
            for c in t.closing_cues {
                assert!(json.contains(c), "missing closing cue: {c}");
            }
        }
        assert!(json.ends_with(r#","[unk]"]"#));
    }

    #[test]
    fn topic_ids_are_unique() {
        let mut ids: Vec<&str> = M1_NOTES.iter().map(|t| t.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), M1_NOTES.len());
    }
}
