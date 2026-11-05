.PHONY: build check doctor help push run test

help:
	@echo "Targets: help check doctor run test build push"

check:
	@cargo fmt --all -- --check
	@cargo clippy --all-targets --all-features -- -D warnings
	@cargo test --all-targets --all-features
	@cargo build --release
	@shellcheck scripts/*.sh
	@./scripts/doctor.sh

doctor:
	@./scripts/doctor.sh

run:
	@cargo run -- play

test:
	@cargo test --all-targets --all-features

build:
	@cargo build --release

push:
	git push github HEAD:main
