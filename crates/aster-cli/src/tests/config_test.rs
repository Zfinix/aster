use super::*;

/// A value of the right shape for a key, so the whole table can be exercised.
fn sample(kind: Kind) -> &'static str {
    match kind {
        Kind::Text => "sample",
        Kind::Bool => "true",
        Kind::Number => "1",
        Kind::List => "one, two",
        Kind::Choice(options) => options[0],
    }
}

/// The table is written by hand, so drift is the failure to catch: a key that
/// `Settings` no longer has, or one this module cannot read back.
#[test]
fn every_documented_key_writes_reads_and_parses() {
    for key in KEYS {
        let written = yaml_value(key, sample(key.kind));
        let text = crate::settings::with_key("", key.section(), key.leaf(), &written);
        let settings: Settings = serde_yaml::from_str(&text)
            .unwrap_or_else(|e| panic!("{} wrote {text:?}, which does not parse: {e}", key.name));

        assert!(
            crate::settings::pins(&text, key.section(), key.leaf()),
            "{} was written but does not read as set: {text:?}",
            key.name
        );
        assert!(
            !configured(&settings, key.name).is_null(),
            "{} parsed but reads back as unset",
            key.name
        );
    }
}

#[test]
fn a_key_the_table_does_not_have_is_refused() {
    let e = key("review.modle").unwrap_err().to_string();
    assert!(e.contains("review.model"), "{e}");

    let e = key("nothing.at.all").unwrap_err().to_string();
    assert!(e.contains("aster config list"), "{e}");
}

#[test]
fn the_structured_mcp_sections_point_at_their_own_command() {
    let e = key("mcp.servers.github.command").unwrap_err().to_string();
    assert!(e.contains("aster mcp"), "{e}");
    let e = key("mcp.tools.deny").unwrap_err().to_string();
    assert!(e.contains("aster mcp"), "{e}");
}

#[test]
fn lists_are_written_inline_and_scalars_only_quoted_when_they_must_be() {
    let list = KEYS.iter().find(|k| k.name == "review.analyzers").unwrap();
    assert_eq!(
        yaml_value(list, "semgrep, ast-grep"),
        "[\"semgrep\", \"ast-grep\"]"
    );
    assert_eq!(yaml_value(list, ""), "[]");

    let text = KEYS.iter().find(|k| k.name == "review.model").unwrap();
    assert_eq!(yaml_value(text, "openai/gpt-4o-mini"), "openai/gpt-4o-mini");
    assert_eq!(
        yaml_value(text, "https://x.test/v1"),
        "https://x.test/v1",
        "a URL is a plain scalar"
    );
    assert_eq!(yaml_value(text, "two words"), "\"two words\"");
    assert_eq!(yaml_value(text, "*"), "\"*\"");
    assert_eq!(yaml_value(text, ""), "\"\"");
}

/// The written value has to survive the parser, since that is what the next
/// run reads it with.
#[test]
fn a_written_glob_list_parses_back_to_the_globs_given() {
    let key = KEYS.iter().find(|k| k.name == "permissions.deny").unwrap();
    let written = yaml_value(key, "Bash(npm publish:*), Edit(infra/**)");
    let text = crate::settings::with_key("", key.section(), key.leaf(), &written);
    let settings: Settings = serde_yaml::from_str(&text).expect("parse");
    assert_eq!(
        settings.permissions.deny,
        vec!["Bash(npm publish:*)", "Edit(infra/**)"]
    );
}

#[test]
fn setting_a_list_over_a_block_one_replaces_every_item() {
    let yaml = "review:\n  exclude:\n    - \"docs/**\"\n    - \"web/**\"\n  model: m1\n";
    let out = crate::settings::with_key(yaml, "review", "exclude", "[\"src/**\"]");
    let settings: Settings = serde_yaml::from_str(&out).expect("parse");
    assert_eq!(settings.review.exclude, vec!["src/**"]);
    assert!(out.contains("model: m1"), "{out}");
}

#[test]
fn clearing_the_last_key_takes_the_empty_section_with_it() {
    let yaml = "review:\n  model: m1\npermissions:\n  mode: manual\n";
    let out = crate::settings::without_key(yaml, "review", "model").expect("removed");
    assert!(!out.contains("review:"), "{out}");
    // A bare `review:` would parse as null and fail the next load outright.
    serde_yaml::from_str::<Settings>(&out).expect("parse");
    assert!(out.contains("mode: manual"), "{out}");
}

#[test]
fn clearing_one_key_leaves_the_rest_of_the_block_alone() {
    let yaml = "# mine\nreview:\n  model: m1  # picked long ago\n  effort: high\n";
    let out = crate::settings::without_key(yaml, "review", "model").expect("removed");
    assert!(!out.contains("m1"), "{out}");
    assert!(out.contains("effort: high"), "{out}");
    assert!(out.contains("# mine"), "{out}");
}

#[test]
fn clearing_a_key_no_file_sets_reports_nothing_to_do() {
    assert!(crate::settings::without_key("review:\n  effort: high\n", "review", "model").is_none());
    assert!(crate::settings::without_key("", "agent", "max_tool_rounds").is_none());
}

#[test]
fn an_unset_key_renders_as_its_documented_default() {
    let key = KEYS
        .iter()
        .find(|k| k.name == "agent.max_tool_rounds")
        .unwrap();
    assert_eq!(render(&Value::Null, key), "60");
    assert_eq!(render(&json!(20), key), "20");

    let list = KEYS.iter().find(|k| k.name == "review.include").unwrap();
    assert_eq!(render(&json!([]), list), "[]");
    assert_eq!(
        render(&json!(["src/**", "docs/**"]), list),
        "src/**, docs/**"
    );
}

#[test]
fn the_environment_outranks_the_file_and_says_so() {
    let key = KEYS.iter().find(|k| k.name == "review.model").unwrap();
    let layer = Layer {
        path: PathBuf::from("/tmp/aster.yaml"),
        label: "/tmp/aster.yaml".into(),
        text: "review:\n  model: from-file\n".into(),
    };
    let settings: Settings = serde_yaml::from_str(&layer.text).expect("parse");
    let layers = std::slice::from_ref(&layer);

    let resolved = resolve_from(key, &settings, layers, None);
    assert_eq!(resolved.value, json!("from-file"));
    assert_eq!(resolved.source, "/tmp/aster.yaml");
    assert!(resolved.shadowed.is_none());

    let shell = Some(("ASTER_MODEL", "from-shell".to_string()));
    let resolved = resolve_from(key, &settings, layers, shell);
    assert_eq!(resolved.value, json!("from-shell"));
    assert_eq!(resolved.source, "env ASTER_MODEL");
    assert_eq!(resolved.shadowed, Some("ASTER_MODEL"));
}

#[test]
fn a_key_no_file_sets_resolves_to_the_default() {
    let key = KEYS.iter().find(|k| k.name == "permissions.mode").unwrap();
    let layer = Layer {
        path: PathBuf::from("/tmp/aster.yaml"),
        label: "/tmp/aster.yaml".into(),
        text: "review:\n  model: m1\n".into(),
    };
    let settings: Settings = serde_yaml::from_str(&layer.text).expect("parse");
    let resolved = resolve_from(key, &settings, std::slice::from_ref(&layer), None);

    assert!(resolved.value.is_null());
    assert_eq!(resolved.source, "default");
    assert_eq!(render(&resolved.value, key), "edit");
}

#[test]
fn a_value_the_type_rejects_never_reaches_the_file() {
    let key = KEYS.iter().find(|k| k.name == "permissions.mode").unwrap();
    let text = crate::settings::with_key("", "permissions", "mode", &yaml_value(key, "sudo"));
    assert!(check(&text).is_err(), "{text}");

    let key = KEYS
        .iter()
        .find(|k| k.name == "agent.max_tool_rounds")
        .unwrap();
    let text = crate::settings::with_key("", "agent", "max_tool_rounds", &yaml_value(key, "lots"));
    assert!(check(&text).is_err(), "{text}");
}

/// The form shows labels, so two rows reading the same in one group would be
/// two rows nobody can tell apart.
#[test]
fn every_group_labels_its_settings_distinctly() {
    for group in Group::ALL {
        let labels: Vec<&str> = group.keys().map(|(_, k)| k.label).collect();
        assert!(!labels.is_empty(), "{} has no settings", group.title());
        for (i, label) in labels.iter().enumerate() {
            assert!(!label.is_empty(), "a key in {} has no label", group.title());
            assert!(
                !labels[i + 1..].contains(label),
                "{} labels two settings {label:?}",
                group.title()
            );
        }
    }
    assert_eq!(
        Group::ALL.iter().map(|g| g.keys().count()).sum::<usize>(),
        KEYS.len(),
        "a key belongs to no group"
    );
}

/// A unit is a reading aid, so it lands on a number and never on a default
/// written in words.
#[test]
fn numbers_carry_their_unit_and_words_do_not() {
    let timeout = KEYS
        .iter()
        .find(|k| k.name == "agent.command_timeout_secs")
        .unwrap();
    assert_eq!(display(&json!(600), timeout), "600s");
    assert_eq!(display(&Value::Null, timeout), "300s");

    let compact = KEYS
        .iter()
        .find(|k| k.name == "agent.compact_budget_chars")
        .unwrap();
    assert_eq!(display(&json!(192_000), compact), "192k chars");

    let collector = KEYS
        .iter()
        .find(|k| k.name == "agents.collector_model")
        .unwrap();
    assert_eq!(display(&Value::Null, collector), "the main model");

    // An empty list reads as what empty means for that key, not as "[]".
    let include = KEYS.iter().find(|k| k.name == "review.include").unwrap();
    assert_eq!(display(&json!([]), include), "everything");
    assert_eq!(display(&json!(["src/**"]), include), "src/**");
}

/// What a prompt prefills and what `get` prints is the file's own value, never
/// the decorated one, or the next write would save "600s" as a number.
#[test]
fn the_written_value_never_carries_the_unit() {
    let timeout = KEYS
        .iter()
        .find(|k| k.name == "agent.command_timeout_secs")
        .unwrap();
    assert_eq!(render(&json!(600), timeout), "600");
}

/// A layer reports only what it sets itself. `permissions.mode` is the trap:
/// parsing a file that omits it still yields the default, so a settings editor
/// asking "does this file set the mode?" would be told yes by every file.
#[test]
fn a_layer_reports_only_the_keys_it_sets() {
    let layer = Layer {
        path: PathBuf::from("aster.yaml"),
        label: "aster.yaml".into(),
        text: "review:\n  model: openai/gpt-4o-mini\n".to_string(),
    };

    assert_eq!(
        in_layer(key("review.model").unwrap(), &layer),
        json!("openai/gpt-4o-mini")
    );
    assert!(in_layer(key("review.effort").unwrap(), &layer).is_null());
    assert!(in_layer(key("permissions.mode").unwrap(), &layer).is_null());
}

/// Each file answers for itself, so an editor can write back to the scope the
/// user picked rather than to whichever file happened to win.
#[test]
fn each_scope_keeps_its_own_value() {
    let global = PathBuf::from("/home/u/.aster/aster.yaml");
    let layers = vec![
        Layer {
            path: global.clone(),
            label: "~/.aster/aster.yaml".into(),
            text: "review:\n  model: global/model\n  effort: low\n".to_string(),
        },
        Layer {
            path: PathBuf::from("/repo/aster.yaml"),
            label: "aster.yaml".into(),
            text: "review:\n  model: repo/model\n".to_string(),
        },
    ];

    let model = scoped(key("review.model").unwrap(), &layers, &global);
    assert_eq!(model["global"], json!("global/model"));
    assert_eq!(model["local"], json!("repo/model"));

    let effort = scoped(key("review.effort").unwrap(), &layers, &global);
    assert_eq!(effort["global"], json!("low"));
    assert!(effort["local"].is_null());
}
