# Copilot Instructions for WhisperMe

You are writing code for a maintainable, production-quality codebase.
Prioritize clarity, simplicity, and functional programming idioms.
Fail early, fail fast - detect problems immediately and exit.
Leverage the type system to encode invariants - this reduces the need for rigorous runtime checks and unit tests.
Follow the guidelines below strictly.

---

## Before You Code

- Read ARCHITECTURE.md to understand the system design
- Review `messages.rs` before defining new message types
- Check `common/` for existing utility functions before creating new ones
- Review Cargo.toml before adding new dependencies
- Look at existing component implementations and follow the same patterns
- Do not add new threads or communication patterns without updating ARCHITECTURE.md

---

## Error Handling

- Use `anyhow` crate with `?` operator and a top-level handler that exits on error
- No error recovery or retry logic

---

## Code Style

- Functional programming style preferred
- Pure functions - separate logic from side effects
- Prefer higher-order functions: `map`, `filter`, `fold`, `iter`, `collect`, `flat_map`
- Avoid `for` loops - use iterator methods instead
- Use `for_each` for side-effectful iteration
- Prefer iterator combinators over recursion (unless data is naturally recursive with bounded depth)
- Minimize branching - prefer combinators and transformations
- Prefer `match` over `if`/`if let` when branching is necessary
- Do not add special-case handling when the general case handles it correctly
- Avoid object-oriented patterns
- Use `impl` for constructors and accessors; prefer free functions for complex behavior
- Use existing Rust macros; do not create new ones
- Avoid `unsafe` code when possible

---

## Code Reuse

- Extract small, pure utility functions to `common/` when reusable
- Avoid duplicating existing functionality
- Only generalize when there is clear reuse potential

---

## Ownership

- Think carefully about ownership
- Prefer `Arc<T>` for data shared between threads
- Use owned types in channel messages
- Prefer moving data over borrowing across thread boundaries

---

## Async Patterns

- Use `spawn_blocking` for CPU-heavy operations (e.g., Whisper inference)
- Use `tokio::select!` for waiting on multiple channels
- Prefer `tokio::sync::mpsc` for thread communication

---

## Testing

- Test properties, not specific cases
- Focus on places where things can go wrong
- Property-based testing where applicable
- No compiler warnings or dead code must be present in the code.
- Never use compiler annotation to silence compiler warnings

---

## Build System

- Use Makefile for all build operations
- Required targets: `build`, `clean`, `test`, `run`
- Always use `make <target>` - do not invoke `cargo` directly
- Use proper Makefile dependency declarations
- Include targets for third-party libraries (e.g., whisper.cpp)
