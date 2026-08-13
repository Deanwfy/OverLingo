import { fileURLToPath, URL } from 'node:url';
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
    root: 'src',
    plugins: [svelte({ configFile: false })],
    server: {
        host: '127.0.0.1',
        port: 1420,
        strictPort: true,
    },
    build: {
        outDir: '../dist',
        emptyOutDir: true,
        rollupOptions: {
            input: {
                main: fileURLToPath(new URL('./src/index.html', import.meta.url)),
                overlay: fileURLToPath(new URL('./src/overlay.html', import.meta.url)),
            },
        },
    },
});
