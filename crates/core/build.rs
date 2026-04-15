// build.rs — compile sqlite-vec from vendored C sources.
//
// Requires the `sqlite3ext.h` header from rusqlite's bundled libsqlite3-sys.
// The `DEP_SQLITE3_INCLUDE` env var is set automatically by the
// `libsqlite3-sys` build script when the `bundled` feature is active.

fn main() {
    // libsqlite3-sys exports the include directory via DEP_SQLITE3_INCLUDE.
    let sqlite3_include = std::env::var("DEP_SQLITE3_INCLUDE")
        .expect("DEP_SQLITE3_INCLUDE not set — ensure rusqlite has the `bundled` feature");

    cc::Build::new()
        .file("sqlite-vec/sqlite-vec.c")
        .include(&sqlite3_include)
        .include("sqlite-vec") // for sqlite-vec.h, sqlite-vec-diskann.c, sqlite-vec-rescore.c
        .define("SQLITE_CORE", None)
        .warnings(false)
        .compile("sqlite_vec");

    println!("cargo:rerun-if-changed=sqlite-vec/sqlite-vec.c");
    println!("cargo:rerun-if-changed=sqlite-vec/sqlite-vec.h");
    println!("cargo:rerun-if-changed=sqlite-vec/sqlite-vec-diskann.c");
    println!("cargo:rerun-if-changed=sqlite-vec/sqlite-vec-rescore.c");
}
