use tracing_subscriber::Registry;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

fn main() -> anyhow::Result<()> {
    let endpoint = std::env::args()
        .nth(1)
        .unwrap_or("http://127.0.0.1:4318".into());
    let (layer, telemetry) = aster_telemetry::layer::<Registry>("aster", &endpoint)?;
    tracing_subscriber::registry().with(layer).init();
    {
        let turn = tracing::info_span!("turn", rounds = 2, calls = 3);
        let _e = turn.enter();
        tracing::info_span!("tool_call", tool = "search_files", barren = true).in_scope(|| {});
        tracing::info_span!("model_request", model = "test", status = 200).in_scope(|| {});
    }
    telemetry.shutdown();
    Ok(())
}
