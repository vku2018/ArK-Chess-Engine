# Contributing

Ark welcomes focused changes that improve chess correctness, engine speed, training quality, or
developer reliability. Keep pull requests small enough to review, but complete enough to prove the
behavior they change.

## Quality Bar

- Tie each change to a measurable outcome: legal move coverage, perft speed, search trace accuracy,
  replay integrity, training loss, or benchmark throughput.
- Add tests with the change. A behavior change without a test is unfinished.
- Keep generated outputs out of Git. Do not commit replay chunks, model weights, logs, reports, or
  local tool directories.
- Prefer Rust implementations in the active engine path. Python files may exist in archived
  references, but active move generation, search, self-play, and replay writing belong in Rust.

## Pull Requests

Open a pull request for every change. Include:

- What changed.
- Why it changed.
- The commands used to validate it.
- Any known limitation or follow-up work.

Use direct commit messages such as `Tighten replay validation` or `Add perft regression case`.

Before opening a PR, run:

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release -p ark_cli --locked
```

`scripts/check.sh --release` and `scripts/check.ps1 -Release` run the full local gate, including
repository guard scripts. Maintainers may request additional automated review before merging, but
contributors do not need private tooling to open a useful pull request.

## Rust Standards

- Use Rust 2021.
- Keep `unsafe_code = forbid`.
- Keep interfaces narrow and explicit.
- Avoid global mutable state in engine code.
- Treat deterministic replay hashes and perft counts as compatibility contracts.

## Documentation Style

Write documentation for readers who are seeing Ark for the first time. State facts, commands, and
tradeoffs plainly. Avoid private setup notes, prompt-like instructions, generated prose, hype, and
unverified strength claims.

## AI-Assisted Contributions

AI tools can help draft code, tests, and docs, but contributors own the result. Before submitting:

- Remove prompt artifacts, chat transcripts, planning notes, and local machine assumptions.
- Check names, comments, and prose for generic generated wording.
- Run the same tests expected from a human-authored change.
- Explain the implementation in your own words in the pull request.

If a tool produced a broad rewrite, review the diff file by file before opening the PR. Keep only the
parts that improve the project.
