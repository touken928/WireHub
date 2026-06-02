import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, 'src'),
    },
  },
  build: {
    outDir: path.resolve(__dirname, '../internal/static/dist'),
    emptyOutDir: true,
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [
            {
              name: 'fluent-icons',
              test: /[\\/]node_modules[\\/]@fluentui[\\/]react-icons[\\/]/,
            },
            {
              name: 'fluent-ui',
              test: /[\\/]node_modules[\\/]@fluentui[\\/]/,
            },
            {
              name: 'xyflow',
              test: /[\\/]node_modules[\\/]@xyflow[\\/]/,
            },
          ],
        },
      },
    },
  },
  server: {
    proxy: {
      '/api': 'http://localhost:8080',
    },
  },
})
