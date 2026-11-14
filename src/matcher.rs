#![allow(dead_code)]

use crate::notes::Topic;
use crate::speech::grammar::normalize;

const COOLDOWN_MS: u64 = 1500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatcherAction {
    None,
    Open(usize),
    Close,
}

pub struct Matcher {
    topics: Vec<(Vec<String>, Vec<String>)>, // (cues, closing_cues) per topic
    open: Option<usize>,
    last_trigger_ms: Option<u64>,
}

impl Matcher {
    pub fn new(topics: Vec<(Vec<&str>, Vec<&str>)>) -> Self {
        Self {
            topics: topics
                .into_iter()
                .map(|(c, cl)| {
                    (
                        c.into_iter().map(String::from).collect(),
                        cl.into_iter().map(String::from).collect(),
                    )
                })
                .collect(),
            open: None,
            last_trigger_ms: None,
        }
    }

    pub fn process(&mut self, text: &str, now_ms: u64) -> MatcherAction {
        let norm = normalize(text);

        // Closing cue of the open card -> Close.
        if let Some(open) = self.open {
            let closing = &self.topics[open].1;
            if closing.iter().any(|c| norm.contains(&normalize(c))) {
                self.open = None;
                self.last_trigger_ms = None;
                return MatcherAction::Close;
            }
        }

        // Longest contained cue wins.
        let mut best: Option<(usize, usize)> = None; // (topic, cue len)
        for (ti, (cues, _)) in self.topics.iter().enumerate() {
            for c in cues {
                let nc = normalize(c);
                if !nc.is_empty() && norm.contains(&nc) {
                    match best {
                        Some((_, len)) if len >= nc.len() => {}
                        _ => best = Some((ti, nc.len())),
                    }
                }
            }
        }
        let Some((topic, _)) = best else {
            return MatcherAction::None;
        };

        // Card already open ignores its own cue.
        if self.open == Some(topic) {
            return MatcherAction::None;
        }

        // Cooldown.
        if !cooldown_elapsed(self.last_trigger_ms, now_ms) {
            return MatcherAction::None;
        }

        self.open = Some(topic);
        self.last_trigger_ms = Some(now_ms);
        MatcherAction::Open(topic)
    }
}

pub fn cooldown_elapsed(last_ms: Option<u64>, now_ms: u64) -> bool {
    match last_ms {
        None => true,
        Some(l) => now_ms.saturating_sub(l) >= COOLDOWN_MS,
    }
}

/// Pure helper: longest contained cue -> topic index, or None.
pub fn detect_cue(text: &str, topics: &[Topic], open: Option<usize>) -> Option<usize> {
    let norm = normalize(text);
    let mut best: Option<(usize, usize)> = None;
    for (ti, t) in topics.iter().enumerate() {
        if Some(ti) == open {
            continue; // open card ignores its own cue
        }
        for c in t.cues {
            let nc = normalize(c);
            if !nc.is_empty() && norm.contains(&nc) {
                match best {
                    Some((_, len)) if len >= nc.len() => {}
                    _ => best = Some((ti, nc.len())),
                }
            }
        }
    }
    best.map(|(ti, _)| ti)
}

/// Pure helper: does text contain any closing cue?
pub fn detect_closing(text: &str, closing_cues: &[String]) -> bool {
    let norm = normalize(text);
    closing_cues.iter().any(|c| norm.contains(&normalize(c)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn topics() -> Vec<(Vec<&'static str>, Vec<&'static str>)> {
        vec![
            (
                vec!["on system architecture", "how i designed the system"],
                vec!["that's the architecture"],
            ),
            (vec!["biggest challenge", "hardest problem"], vec![]),
        ]
    }

    #[test]
    fn longest_cue_wins() {
        let mut m = Matcher::new(topics());
        assert_eq!(
            m.process("i want to talk on system architecture", 0),
            MatcherAction::Open(0)
        );
    }

    #[test]
    fn cooldown_blocks_retrigger() {
        let mut m = Matcher::new(topics());
        assert_eq!(
            m.process("on system architecture", 0),
            MatcherAction::Open(0)
        );
        // within 1.5 s -> blocked (distinct cue, still on cooldown)
        assert_eq!(m.process("biggest challenge", 1000), MatcherAction::None);
        // after 1.5 s -> allowed
        assert_eq!(m.process("biggest challenge", 2000), MatcherAction::Open(1));
    }

    #[test]
    fn open_card_ignores_its_own_cue() {
        let mut m = Matcher::new(topics());
        assert_eq!(
            m.process("on system architecture", 0),
            MatcherAction::Open(0)
        );
        assert_eq!(
            m.process("on system architecture", 5000),
            MatcherAction::None
        );
    }

    #[test]
    fn closing_cue_returns_close() {
        let mut m = Matcher::new(topics());
        assert_eq!(
            m.process("on system architecture", 0),
            MatcherAction::Open(0)
        );
        assert_eq!(
            m.process("that's the architecture", 1000),
            MatcherAction::Close
        );
    }

    #[test]
    fn no_match_returns_none() {
        let mut m = Matcher::new(topics());
        assert_eq!(m.process("i enjoy hiking", 0), MatcherAction::None);
    }

    #[test]
    fn cooldown_elapsed_boundary() {
        assert!(!cooldown_elapsed(Some(0), 1499));
        assert!(cooldown_elapsed(Some(0), 1500));
        assert!(cooldown_elapsed(None, 0));
    }

    #[test]
    fn detect_cue_picks_longest() {
        let t = [Topic {
            id: "t0",
            title: "t",
            cues: &["tell me about a project", "project"],
            bullets: &[],
            closing_cues: &[],
        }];
        assert_eq!(detect_cue("so tell me about a project", &t, None), Some(0));
    }

    #[test]
    fn detect_closing_works() {
        assert!(detect_closing(
            "that's the architecture",
            &["that's the architecture".to_string()]
        ));
        assert!(!detect_closing(
            "hello",
            &["that's the architecture".to_string()]
        ));
    }
}
