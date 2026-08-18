# Live eval workspace

An Ori workspace whose only feature is a harness that runs `aster --stream`
instead of Ori's own agent, so an eval measures this harness.

The cases are defined in `../src/live.rs` and rendered to `cases.eval.ts`;
nothing here is edited by hand except `features/aster/feature.ts`.

    bun install
    cargo run -p aster-eval -- live --models z-ai/glm-5.2,moonshotai/kimi-k3

Needs `ori` (openrouter.ai/labs/ori), `bun`, and an OpenRouter credential.
Verify the harness itself with `ori harness test --harness aster`.

Both evaluation modes, their metrics, and the limits on what those numbers can
claim are documented in [docs/EVAL.md](../../../docs/EVAL.md).
