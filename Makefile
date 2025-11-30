.PHONY: build release test bench

build:
	cargo build

release:
	cargo build --release

test:
	cargo test

bench: release
	hyperfine --warmup 2 --runs 10 \
		'bash bench/run_delta_direct.sh' \
		'bash bench/run_delta_xolmis.sh'

