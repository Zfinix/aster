use super::*;

fn urls() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("anthropic".into(), "https://api.anthropic.com/v1".into()),
        ("deepseek".into(), "https://api.deepseek.com/v1".into()),
        ("openrouter".into(), "https://openrouter.ai/api/v1".into()),
    ])
}

const DASHSCOPE: &str = "https://dashscope-intl.aliyuncs.com/compatible-mode/v1";

#[test]
fn pick_model_endpoint_switches_to_the_prefixed_provider_with_a_key() {
    let picked = pick_model_endpoint(
        "anthropic/claude-sonnet-5",
        DASHSCOPE,
        &urls(),
        false,
        &|url| url.contains("anthropic"),
    );
    let (base_url, param) = picked.expect("the prefix names a keyed provider");
    assert_eq!(base_url, "https://api.anthropic.com/v1");
    assert_eq!(param, "claude-sonnet-5");
}

#[test]
fn pick_model_endpoint_keeps_openrouter_and_translates_the_slug() {
    let picked = pick_model_endpoint(
        "zai/glm-5.3-flash",
        "https://openrouter.ai/api/v1",
        &urls(),
        false,
        &|_| true,
    );
    let (base_url, param) = picked.expect("openrouter serves prefixed ids");
    assert_eq!(base_url, "https://openrouter.ai/api/v1");
    assert_eq!(param, "z-ai/glm-5.3-flash");
}

#[test]
fn pick_model_endpoint_leaves_an_id_the_current_endpoint_lists() {
    let picked = pick_model_endpoint(
        "moonshotai/kimi-k3",
        "https://integrate.api.nvidia.com/v1",
        &urls(),
        true,
        &|_| true,
    );
    assert_eq!(picked, None);
}

#[test]
fn pick_model_endpoint_falls_back_to_openrouter_for_unknown_prefixes() {
    let picked = pick_model_endpoint("moonshotai/kimi-k3", DASHSCOPE, &urls(), false, &|url| {
        url.contains("openrouter")
    });
    let (base_url, param) = picked.expect("openrouter has a key");
    assert_eq!(base_url, "https://openrouter.ai/api/v1");
    assert_eq!(param, "moonshotai/kimi-k3");
}

#[test]
fn pick_model_endpoint_leaves_bare_and_unresolvable_ids_alone() {
    assert_eq!(
        pick_model_endpoint(
            "deepseek-v4-pro",
            "https://api.deepseek.com/v1",
            &urls(),
            false,
            &|_| true
        ),
        None
    );
    assert_eq!(
        pick_model_endpoint(
            "anthropic/claude-sonnet-5",
            DASHSCOPE,
            &urls(),
            false,
            &|_| false
        ),
        None
    );
}
