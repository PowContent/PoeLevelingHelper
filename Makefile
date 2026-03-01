.PHONY: build release run clean

# Debug build
build:
	cargo build

# Optimized release build
release:
	cargo build --release

# Run debug build
run:
	cargo run

# Run release build
run-release:
	cargo run --release

# Clean build artifacts
clean:
	cargo clean
