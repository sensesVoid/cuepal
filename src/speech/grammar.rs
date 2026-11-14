#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammar_json_shape() {
        assert_eq!(
            build_grammar_json(&[
                "on system architecture".to_string(),
                "why this".to_string()
            ]),
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
