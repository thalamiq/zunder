import { defineConfig } from "vite";
import react from "@vitejs/plugin-react-swc";
import tailwindcss from "@tailwindcss/vite";
import path from "path";

const fhirServerUrl =
  process.env.VITE_FHIR_SERVER_URL ?? "http://127.0.0.1:8081";

export default defineConfig({
  base: "/ui/",
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    proxy: {
      "/admin": {
        target: fhirServerUrl,
        changeOrigin: true,
        cookieDomainRewrite: "",
      },
      "/fhir": {
        target: fhirServerUrl,
        changeOrigin: true,
      },
      "/health": {
        target: fhirServerUrl,
        changeOrigin: true,
      },
    },
  },
});
