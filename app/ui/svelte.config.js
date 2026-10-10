import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

// The runner loader reads vite.config.ts without esbuild bundling, which needs
// directory listings above the repo that sandboxed agents cannot access.
export default { preprocess: vitePreprocess({ style: { configLoader: 'runner' } }) };
