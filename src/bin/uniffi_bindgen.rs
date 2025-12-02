//! UniFFI bindgen CLI tool for generating language bindings.
//!
//! This binary allows generating Swift, Kotlin, Python, and Ruby bindings
//! from the cooklang-find library.
//!
//! ## Usage
//!
//! Generate Swift bindings:
//! ```bash
//! cargo run --bin uniffi-bindgen generate --library target/release/libcooklang_find.so --language swift --out-dir ./bindings
//! ```
//!
//! Generate Kotlin bindings:
//! ```bash
//! cargo run --bin uniffi-bindgen generate --library target/release/libcooklang_find.so --language kotlin --out-dir ./bindings
//! ```

fn main() {
    uniffi::uniffi_bindgen_main()
}
