project := "wicked-estate"
build_dir := "target"

build:
    cargo build --workspace

test: build
    cargo test --workspace

check: build
    cargo clippy --workspace --all-targets -- -D warnings
    cargo fmt --all -- --check

release: check test
    cargo build --workspace --release

clean:
    cargo clean
    rm -rf {{build_dir}}
