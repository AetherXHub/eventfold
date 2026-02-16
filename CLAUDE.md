# CLAUDE.md — Project Instructions

## Project

eventfold — a lightweight, append-only event log with derived views for Rust.

## Build & Quality Gates

Every change MUST pass all of these before it can be committed:

```bash
cargo build                          # must succeed
cargo clippy -- -D warnings          # zero warnings, treated as errors
cargo test                           # all unit and integration tests pass
```

Do NOT commit code that has clippy warnings. Do NOT commit code with failing tests. Do NOT skip these checks. Run all three in sequence after every meaningful change.

## Testing Requirements

Tests are not optional. Every module, function, and code path requires coverage.

### Unit Tests
- Every public function and method must have corresponding unit tests
- Every error path must be tested (not just the happy path)
- Edge cases: empty inputs, zero values, boundary conditions, unicode, special characters
- Use `tempfile::tempdir()` for all file system tests — never write to fixed paths
- Shared test helpers go in `tests/common/mod.rs`

### Integration Tests
- Every PRD lists specific test cases in its "Test Plan" section — implement ALL of them
- Cross-module interactions must be tested (e.g., append → refresh → verify state)
- Test the public API as a user would use it

### Test Quality
- Tests must assert specific values, not just "didn't panic"
- Each test tests one thing and has a descriptive name
- No `#[ignore]` without a comment explaining why
- No `unwrap()` in production code where an error is plausible — tests may use `unwrap()`

## PRD Workflow

PRDs live in `docs/prds/` and are implemented in order (01 through 09). Each PRD is a self-contained unit of work.

### Implementation Process

1. Read the PRD thoroughly before writing any code
2. Implement the code changes listed in the PRD's "Files" section
3. Implement ALL test cases listed in the PRD's "Test Plan" section
4. Verify all acceptance criteria are met
5. Run the full quality gate: `cargo build && cargo clippy -- -D warnings && cargo test`
6. Mark the PRD as complete: move it from `docs/prds/` to `docs/prds/completed/`
7. Commit with message: `feat: implement PRD-XX — <title>`
8. Push to origin: `git push`
9. Only then proceed to the next PRD

### Commit Rules for PRDs

- Each PRD gets exactly ONE commit (squash if needed during implementation)
- The commit must include: code changes, tests, and the PRD move to `completed/`
- Do NOT start the next PRD until the current one is committed and pushed
- If a PRD requires changes to previously completed work, that's fine — include those changes in the current PRD's commit

### PRD Completion Marker

When moving a PRD to `docs/prds/completed/`, add a completion header at the top:

```markdown
> **Status: COMPLETED** — Implemented and verified on YYYY-MM-DD
```

## Code Style

- Follow standard Rust conventions (rustfmt defaults)
- No `unsafe` unless absolutely necessary and documented
- Prefer returning `io::Result` over panicking
- Keep dependencies minimal — check `docs/plan.md` for the approved dependency list
- No dead code — if clippy says it's unused, remove it or gate it with `#[cfg(test)]`

## Git Conventions

- Commit messages: `feat: implement PRD-XX — <title>` for PRD work
- Commit messages: `fix: <description>` for bug fixes
- Commit messages: `chore: <description>` for non-functional changes
- Always push to `origin main` after committing
- Never force push
