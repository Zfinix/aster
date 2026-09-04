use super::*;

fn entry(slug: &str, coding: f64, agentic: f64, price_per_m: f64) -> Entry {
    Entry {
        slug: slug.to_string(),
        coding_index: coding,
        agentic_index: agentic,
        blended_price_per_m: price_per_m,
    }
}

#[test]
fn tier_parse_accepts_exact_and_mixed_case() {
    assert_eq!(Tier::parse("cheap"), Some(Tier::Cheap));
    assert_eq!(Tier::parse(" Strong "), Some(Tier::Strong));
    assert_eq!(Tier::parse("fastest"), None);
}

#[test]
fn strong_prefers_capability_over_price() {
    let entries = [
        entry("cheap/model", 60.0, 50.0, 0.05),
        entry("strong/model", 85.0, 70.0, 5.0),
    ];
    let pick = pick_from_entries(&entries, Tier::Strong, false).unwrap();
    assert_eq!(pick.model, "strong/model");
}

#[test]
fn cheap_respects_the_price_ceiling() {
    let entries = [
        entry("cheap/model", 60.0, 50.0, 0.10),
        entry("strong/model", 85.0, 70.0, 5.0),
    ];
    let pick = pick_from_entries(&entries, Tier::Cheap, false).unwrap();
    assert_eq!(pick.model, "cheap/model");
}

#[test]
fn cheap_falls_through_when_everything_is_priced_out() {
    let entries = [entry("strong/model", 85.0, 70.0, 5.0)];
    assert!(pick_from_entries(&entries, Tier::Cheap, false).is_none());
}

#[test]
fn balanced_prefers_capability_within_the_mid_price_band() {
    let entries = [
        entry("free/model", 40.0, 30.0, 0.0),
        entry("paid/model", 80.0, 60.0, 0.50),
        entry("strong/model", 90.0, 80.0, 5.0),
    ];
    let pick = pick_from_entries(&entries, Tier::Balanced, false).unwrap();
    assert_eq!(pick.model, "paid/model");
}

#[test]
fn rows_without_coding_or_pricing_are_skipped() {
    let row = BenchmarkRow {
        model_permaslug: "no/pricing".to_string(),
        coding_index: Some(70.0),
        agentic_index: None,
        pricing: None,
    };
    assert!(row_to_entry(row).is_none());
}

#[test]
fn pricing_strings_convert_to_blended_per_million() {
    let row = BenchmarkRow {
        model_permaslug: "a/model".to_string(),
        coding_index: Some(70.0),
        agentic_index: Some(50.0),
        pricing: Some(Pricing {
            prompt: Some("0.000005".to_string()),
            completion: Some("0.000025".to_string()),
        }),
    };
    let entry = row_to_entry(row).unwrap();
    // (3*5 + 25)/4 = 10 $/M.
    assert!((entry.blended_price_per_m - 10.0).abs() < 1e-9);
}

#[test]
fn cache_round_trips_and_honors_the_ttl() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("model-rankings.json");
    let entries = [entry("cheap/model", 60.0, 50.0, 0.10)];
    write_cache(&cache, &entries).unwrap();

    let pick = pick_from_cache(&cache, Tier::Cheap).unwrap();
    assert_eq!(pick.model, "cheap/model");
    assert!(pick.from_cache);

    // A stale timestamp is treated as a miss, not an error.
    let stale = Cache {
        fetched_at_secs: 0,
        entries: entries.to_vec(),
    };
    std::fs::write(&cache, serde_json::to_string(&stale).unwrap()).unwrap();
    assert!(pick_from_cache(&cache, Tier::Cheap).is_none());
}

#[test]
fn resolve_auto_falls_back_when_the_cache_is_empty_and_the_fetch_fails() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("model-rankings.json");
    let pick = resolve_auto("not-a-real-key", Tier::Strong, &cache, "fallback/model").unwrap();
    assert_eq!(pick.model, "fallback/model");
}
