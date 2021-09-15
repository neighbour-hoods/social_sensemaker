# must be run from containing directory
CARGO_TARGET_DIR=$(pwd)/../target wasm-pack build $(pwd)/../crates/frontend --target web --out-dir $(pwd)/pkg
rollup ./main.js --format iife --file ./pkg/bundle.js
