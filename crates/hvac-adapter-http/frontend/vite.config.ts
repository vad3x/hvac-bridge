import { defineConfig } from 'vite';
import { viteSingleFile } from 'vite-plugin-singlefile';
import solidPlugin from 'vite-plugin-solid';

export default defineConfig({
  plugins: [solidPlugin(), viteSingleFile()],
  resolve: {
    dedupe: ['solid-js', 'solid-js/web', 'solid-js/store'],
    alias: {
      '~': new URL('./src', import.meta.url).pathname,
    },
  },
  build: {
    outDir: '../assets',
    emptyOutDir: false,
    cssCodeSplit: false,
    assetsInlineLimit: 100000000,
  },
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://e8f60af415a8.local:8080',
        changeOrigin: true,
      },
    },
  },
});
