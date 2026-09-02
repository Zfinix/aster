#![cfg(test)]

use super::*;

#[test]
fn a_name_is_uppercased_and_checked() {
    assert_eq!(normalize("firecrawl_api_key").unwrap(), "FIRECRAWL_API_KEY");
    assert_eq!(normalize("  EXA_API_KEY  ").unwrap(), "EXA_API_KEY");

    for bad in ["", "9LIVES", "has-a-dash", "has space", "$HOME"] {
        assert!(normalize(bad).is_err(), "{bad} should be refused");
    }
}

#[test]
fn every_web_provider_var_is_known() {
    // The catalog `aster key list` reads is the one aster-web resolves from, so
    // a provider cannot gain a key without turning up here.
    for (_, var, _) in aster_web::KEY_VARS {
        assert!(known(var), "{var} should be listed");
    }
    assert!(known(SHARED_KEY_VAR));
    assert!(known("OPENAI_API_KEY"), "catalog vars count as known");
    assert!(!known("NOT_A_REAL_KEY"));
}

#[test]
fn firecrawl_is_among_the_web_vars() {
    let vars: Vec<&str> = aster_web::KEY_VARS.iter().map(|(_, v, _)| *v).collect();
    assert!(vars.contains(&"FIRECRAWL_API_KEY"), "{vars:?}");
}

#[test]
fn an_assignment_is_read_back_with_or_without_quotes() {
    assert_eq!(assignment("FOO=bar", "FOO"), Some("bar"));
    assert_eq!(assignment("  FOO=bar  ", "FOO"), Some("bar"));
    assert_eq!(assignment("FOO=\"bar\"", "FOO"), Some("bar"));
    assert_eq!(assignment("FOO='bar'", "FOO"), Some("bar"));
    assert_eq!(assignment("FOO=", "FOO"), Some(""));

    // A different var, and a name that merely starts the same, are not matches.
    assert_eq!(assignment("BAR=baz", "FOO"), None);
    assert_eq!(assignment("FOOBAR=baz", "FOO"), None);
    assert_eq!(assignment("# FOO=bar", "FOO"), None);
}

#[test]
fn the_last_assignment_in_a_file_is_the_one_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(".env");
    std::fs::write(&path, "FOO=first\nOTHER=x\nFOO=second\n").expect("write");
    assert_eq!(file_value(Some(&path), "FOO").as_deref(), Some("second"));
    assert_eq!(file_value(Some(&path), "MISSING"), None);
    assert_eq!(file_value(None, "FOO"), None);
}

#[test]
fn a_written_key_reads_back_through_the_same_parser() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(".env");
    crate::init::set_env_key(&path, "FIRECRAWL_API_KEY", "fc-abc123").expect("write");
    assert_eq!(
        file_value(Some(&path), "FIRECRAWL_API_KEY").as_deref(),
        Some("fc-abc123")
    );

    // Setting it again replaces rather than appends, so no stale duplicate is
    // left for the parser to prefer.
    crate::init::set_env_key(&path, "FIRECRAWL_API_KEY", "fc-xyz789").expect("rewrite");
    let text = std::fs::read_to_string(&path).expect("read");
    assert_eq!(text.matches("FIRECRAWL_API_KEY").count(), 1, "{text}");
    assert_eq!(
        file_value(Some(&path), "FIRECRAWL_API_KEY").as_deref(),
        Some("fc-xyz789")
    );
}

#[test]
fn setting_a_duplicated_key_leaves_only_the_new_value() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(".env");
    std::fs::write(&path, "FOO=first\nOTHER=x\nFOO=second\n").expect("write");

    crate::init::set_env_key(&path, "FOO", "third").expect("rewrite");
    let text = std::fs::read_to_string(&path).expect("read");
    assert_eq!(text.matches("FOO=").count(), 1, "{text}");
    assert!(text.contains("OTHER=x"), "{text}");
    assert_eq!(file_value(Some(&path), "FOO").as_deref(), Some("third"));
}

#[test]
fn a_masked_key_shows_its_tail_and_nothing_more() {
    assert_eq!(mask_tail("sk-or-v1-abcdef1234"), "…1234");
    assert_eq!(mask_tail("fc-1234abcd"), "…abcd");

    // Short keys hide entirely: a four-char tail of an eight-char key would be
    // half the secret.
    for short in ["x", "12345678"] {
        assert_eq!(mask_tail(short), "••••");
    }
}

#[test]
fn source_labels_never_print_the_key() {
    for source in [Source::Shell, Source::Local, Source::Global, Source::Unset] {
        assert!(!source.label().is_empty());
        assert!(!source.as_str().is_empty());
    }
}

#[test]
fn the_shadow_note_warns_only_about_a_layer_that_still_wins() {
    // A shell export outranks both files, whichever one was written.
    for local in [true, false] {
        let note = shadow_note("EXA_API_KEY", Source::Shell, local).expect("shell shadows");
        assert!(note.contains("shell"), "{note}");
    }

    // Writing the global file while the repo's sets the same key: the repo wins.
    let note = shadow_note("EXA_API_KEY", Source::Local, false).expect("repo .env shadows");
    assert!(note.contains("unset --local"), "{note}");

    // Nothing to warn about: the write replaced the layer that was winning, or
    // the key was not set at all.
    assert!(shadow_note("EXA_API_KEY", Source::Local, true).is_none());
    assert!(shadow_note("EXA_API_KEY", Source::Global, false).is_none());
    assert!(shadow_note("EXA_API_KEY", Source::Global, true).is_none());
    assert!(shadow_note("EXA_API_KEY", Source::Unset, false).is_none());
    assert!(shadow_note("EXA_API_KEY", Source::Unset, true).is_none());
}

#[test]
fn web_providers_groups_the_vars_a_provider_needs() {
    let grouped = web_providers();

    // Every var in the catalog survives grouping, in the same order.
    let flat: Vec<&str> = grouped
        .iter()
        .flat_map(|(_, vars)| vars.iter().map(|(var, _)| *var))
        .collect();
    let catalog: Vec<&str> = aster_web::KEY_VARS.iter().map(|(_, var, _)| *var).collect();
    assert_eq!(flat, catalog);

    // Cloudflare needs two, so it is one provider holding both vars rather than
    // two providers the wizard would ask about separately.
    let cloudflare = grouped
        .iter()
        .find(|(name, _)| name.starts_with("Cloudflare"))
        .expect("Cloudflare is in the catalog");
    assert_eq!(cloudflare.1.len(), 2, "{:?}", cloudflare.1);

    // Firecrawl is the single-var case the prompt phrases differently.
    let firecrawl = grouped
        .iter()
        .find(|(name, _)| *name == "Firecrawl")
        .expect("Firecrawl is in the catalog");
    assert_eq!(firecrawl.1.len(), 1);
    assert_eq!(firecrawl.1[0].0, "FIRECRAWL_API_KEY");

    // No provider is listed twice, which grouping consecutive entries would do
    // if the catalog ever interleaved them.
    let mut names: Vec<&str> = grouped.iter().map(|(name, _)| *name).collect();
    let before = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), before, "a provider is split across groups");
}
