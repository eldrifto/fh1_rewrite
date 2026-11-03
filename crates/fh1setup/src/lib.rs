//! The reusable half of `fh1setup`, exposed so the `examples/` probes can read the same data the importer does
//! instead of re-deriving it (plan v8 P1/P2: a probe that feeds a shader different inputs from the importer proves
//! nothing about the game).
//!
//! **Why a lib as well as a bin.** `fh1setup` is a binary crate: its modules live in `src/main.rs` as `mod x;`, which
//! means nothing outside the binary can call them. `fm2_uv` is the importer's answer to "which texture coordinate
//! does each vertex give each sampler, and through which atlas rectangle" — the one piece the GPU probe cannot get
//! any other way. It has no dependencies on the rest of the crate, so it is exposed here as is.
//!
//! **One copy per target, on purpose.** `src/main.rs` keeps its own `mod fm2_uv;` and this file declares
//! `pub mod fm2_uv;`; the same source file is compiled once into the binary and once into the library. That is how
//! a file can serve both targets. Nothing is passed between the two copies, so no type has to be shared, and the
//! binary's behaviour is unchanged by this file existing.
#![recursion_limit = "256"]

pub mod fm2_uv;
