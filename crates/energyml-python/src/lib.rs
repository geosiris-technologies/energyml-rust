//! The Python bridge - **conversion, and nothing else**.
//!
//! The library has to be usable from Python. This first increment covers only
//! reading and writing an EPC and its `.h5` files; operations and the rest will
//! be added as they come.
//!
//! # Nothing here decides anything
//!
//! It is the WebAssembly front end's rule, and it holds for the same reason: **what
//! can be wrong lives in [`session`], plain Rust that `cargo test` exercises.**
//! A pyo3 binding only runs after a `maturin develop`; code written at the
//! boundary is code the suite never sees.
//!
//! # Arrays cross into numpy **by copy**
//!
//! Decided at the start, and not as a performance detail. A view would share
//! the memory of the block that was read, so its lifetime would be the
//! package's - and an array Python keeps after closing its package would read
//! freed memory. Python has no way to prevent that, and the defect would not
//! show at the write but weeks later, as wrong numbers or a crash with no
//! traceback.
//!
//! The copy costs one pass; a view would cost the trust. If it ever becomes
//! necessary on very large arrays, it will arrive as a method that **says so**
//! in its name, never by changing this one.
//!
//! Usage: `docs/python.md`.

pub mod session;

// **The binding itself sits behind the `python` feature, and the default is
// off.** A workspace member that requires an interpreter and a C library
// requires them for every other member: through feature unification, `cargo
// build --workspace` would go looking for `libpython` and HDF5 to compile the
// lint or the wasm pass. maturin turns `python` on (see `pyproject.toml`);
// nobody else needs it, and `session` - where everything that can be wrong
// lives - compiles and tests without any of it.
#[cfg(feature = "python")]
pub mod bindings;
