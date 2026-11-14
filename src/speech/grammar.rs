/// Vosk recognizer grammar: a JSON array of phrase strings, `[unk]` last.
/// Consumed by Task 2's integration test (build_grammar_json) and Task 4's
/// engine; the allow holds until those land.
pub fn build_grammar_json(cues: &[String]) -> String {
    let mut phrases: Vec<&str> = cues.iter().map(|s| s.as_str()).collect();
    phrases.push("[unk]");
    serde_json::to_string(&phrases).expect("grammar phrases are valid json strings")
}

/// Lowercase, strip everything that is not alphanumeric to a space,
/// collapse runs of spaces, trim. (Filler-word dropping is M1.)
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_space = true;
    for c in text.chars() {
        let mapped = if c.is_ascii_alphanumeric() {
            c.to_ascii_lowercase()
        } else {
            ' '
        };
        if mapped == ' ' {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(mapped);
            prev_space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Match on normalized text; among cues contained in it, longest wins
/// (ties go to the first). None when nothing matches.
pub fn detect_match(text: &str, cues: &[String]) -> Option<usize> {
    let normalized = normalize(text);
    let mut best: Option<(usize, usize)> = None; // (index, normalized cue len)
    for (i, cue) in cues.iter().enumerate() {
        let c = normalize(cue);
        if c.is_empty() {
            continue;
        }
        if normalized.contains(&c) {
            match best {
                Some((_, len)) if len >= c.len() => {}
                _ => best = Some((i, c.len())),
            }
        }
    }
    best.map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammar_json_shape() {
        assert_eq!(
            build_grammar_json(&["on system architecture".to_string(), "why this".to_string()]),
            r#"["on system architecture","why this","[unk]"]"#
        );
    }

    #[test]
    fn normalize_strips_case_and_punct() {
        assert_eq!(normalize("WHY This, company?!"), "why this company");
    }

    #[test]
    fn longest_cue_wins_over_prefix_overlap() {
        let cues = vec!["tell me about a project".to_string(), "project".to_string()];
        assert_eq!(detect_match("so tell me about a project", &cues), Some(0));
    }

    #[test]
    fn no_match_returns_none() {
        let cues = vec!["on system architecture".to_string()];
        assert_eq!(detect_match("i enjoy hiking", &cues), None);
    }

    #[test]
    fn punctuated_shouting_still_matches() {
        let cues = vec!["why this company".to_string()];
        assert_eq!(detect_match("WHY THIS COMPANY?!", &cues), Some(0));
    }
}
