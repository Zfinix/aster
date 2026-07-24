# Contributing to Aster

Thanks for your interest in Aster. This project is open core: the review
harness stands alone, and contributions that keep it fast, precise, and
self-hostable are very welcome.

## Getting started

Aster is a Rust workspace (edition 2024, stable toolchain).

```bash
git clone https://github.com/zfinix/aster
cd aster
cp .env.example .env      # fill in your model provider settings
cargo build --workspace
cargo test --workspace
```

To run a review locally:

```bash
export ASTER_API_KEY=...
export ASTER_BASE_URL=https://openrouter.ai/api/v1
export ASTER_MODEL=openai/gpt-4o-mini

git diff HEAD~1 > /tmp/change.diff
cargo run -p aster-harness --example run_review -- /tmp/change.diff
```

## Before you open a pull request

CI runs these three checks. Run them locally first so your PR is green:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

## Guidelines

- **Match the surrounding style.** Read the crate you are touching and follow
  its patterns before introducing a new one.
- **Keep the core dependency-light.** No vector DB and no external services in
  the review core. New heavy dependencies need a strong justification.
- **Precision over volume.** A false positive costs more trust than a missed
  finding costs coverage. Changes that raise the false-positive rate need to
  earn it.
- **Prefer small, focused commits** with [Conventional Commit](https://www.conventionalcommits.org)
  messages (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`).
- **Add or update tests** for behavior changes. The harness has an end-to-end
  test against a mock provider (`crates/aster-harness/tests/e2e.rs`); extend it
  when you change the pipeline.

## Reporting bugs and requesting features

Open an issue using the templates. For bugs, include the command you ran, the
model and provider, and the output you expected versus what you got.

## Security

Do not open public issues for security problems. See [SECURITY.md](./SECURITY.md).

## License

By contributing, you agree that your contributions are licensed under the
[Apache-2.0](./LICENSE) license.
