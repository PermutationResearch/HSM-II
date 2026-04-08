import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  async rewrites() {
    return [
      {
        source: "/hsm-api/:path*",
        destination: `${process.env.HSM_API_URL ?? "http://127.0.0.1:3000"}/api/:path*`,
      },
    ];
  },
};

export default nextConfig;
