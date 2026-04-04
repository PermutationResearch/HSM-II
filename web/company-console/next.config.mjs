import path from "path";
import { fileURLToPath } from "url";

// Absolute app root so Turbopack does not walk up to e.g. ~/package-lock.json
const __dirname = path.dirname(fileURLToPath(import.meta.url));

/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  turbopack: {
    root: __dirname,
  },
  // `/api/company/*` and `/api/console/*` are proxied by App Route handlers (see app/api/company, app/api/console).
};

export default nextConfig;
