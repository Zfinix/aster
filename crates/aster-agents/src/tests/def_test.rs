use super::*;

#[test]
fn parses_full_frontmatter() {
    let raw = "---\nname: scout\ndescription: Finds things.\nmodel: openai/gpt-4o-mini\ntools: [read_file, search_files]\nmax_rounds: 4\nverify: true\n---\nBe terse.";
    let def = parse_agent_md(raw, "dir", AgentSource::BuiltIn(raw)).unwrap();
    assert_eq!(def.name, "scout");
    assert_eq!(def.model.as_deref(), Some("openai/gpt-4o-mini"));
    assert_eq!(
        def.tools.as_deref(),
        Some(&["read_file".to_string(), "search_files".to_string()][..])
    );
    assert_eq!(def.max_rounds, Some(4));
    assert!(def.verify);
    assert_eq!(def.load_body().unwrap(), "Be terse.");
}

#[test]
fn name_falls_back_to_directory() {
    let raw = "---\ndescription: A helper.\n---\nbody";
    let def = parse_agent_md(raw, "helper", AgentSource::BuiltIn(raw)).unwrap();
    assert_eq!(def.name, "helper");
    assert!(!def.verify);
    assert!(def.tools.is_none());
}

#[test]
fn rejects_missing_description() {
    let raw = "---\nname: broken\n---\nbody";
    assert!(parse_agent_md(raw, "broken", AgentSource::BuiltIn(raw)).is_err());
}

#[test]
fn ignores_unknown_frontmatter_keys() {
    let raw = "---\ndescription: Fine.\ncolor: purple\n---\nbody";
    assert!(parse_agent_md(raw, "ok", AgentSource::BuiltIn(raw)).is_ok());
}

#[test]
fn rejects_bad_name() {
    let raw = "---\nname: Not Kebab\ndescription: x.\n---\nbody";
    assert!(parse_agent_md(raw, "d", AgentSource::BuiltIn(raw)).is_err());
}

#[test]
fn bullet_led_body_survives() {
    let raw = "---\ndescription: A helper.\n---\n- item one\n- item two\n";
    let def = parse_agent_md(raw, "helper", AgentSource::BuiltIn(raw)).unwrap();
    assert_eq!(def.load_body().unwrap(), "- item one\n- item two");
}

#[test]
fn hrule_in_body_survives() {
    let raw = "---\ndescription: A helper.\n---\nSome text\n\n---\n\nMore text\n";
    let def = parse_agent_md(raw, "helper", AgentSource::BuiltIn(raw)).unwrap();
    assert_eq!(def.load_body().unwrap(), "Some text\n\n---\n\nMore text");
}

#[test]
fn missing_fence_is_whole_body() {
    let raw = "No frontmatter here.\nJust a paragraph.\n";
    assert_eq!(strip_frontmatter(raw), raw);
}

#[test]
fn crlf_fence_works() {
    let raw = "---\r\ndescription: A helper.\r\n---\r\nbody line\r\n";
    let def = parse_agent_md(raw, "helper", AgentSource::BuiltIn(raw)).unwrap();
    assert_eq!(def.load_body().unwrap(), "body line");
}

#[test]
fn no_leading_newline_after_fence() {
    let raw = "---\ndescription: A helper.\n---";
    let def = parse_agent_md(raw, "helper", AgentSource::BuiltIn(raw)).unwrap();
    assert_eq!(def.load_body().unwrap(), "");
}

#[test]
fn body_with_leading_dashes_not_eaten() {
    let raw = "---\ndescription: A helper.\n---\n- bullet\n- bullet\n";
    let def = parse_agent_md(raw, "helper", AgentSource::BuiltIn(raw)).unwrap();
    assert_eq!(def.load_body().unwrap(), "- bullet\n- bullet");
}
