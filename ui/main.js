import init, { run_app } from './pkg/frontend.js';
async function main() {
   await init('./pkg/frontend_bg.wasm');
   run_app();
}
main()
