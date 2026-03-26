import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // Standalone output for Docker deployment
  output: "standalone",

  // Privacy: disable image optimization CDN, serve all assets locally
  images: {
    unoptimized: true,
  },

  // Proxy API requests to the Rust DAL server
  async rewrites() {
    const apiUrl = process.env.DAL_API_URL || "http://localhost:8550";
    return [
      {
        source: "/api/:path*",
        destination: `${apiUrl}/api/:path*`,
      },
    ];
  },

  // Privacy: CSP headers to restrict external resource loading
  async headers() {
    return [
      {
        source: "/(.*)",
        headers: [
          {
            key: "Content-Security-Policy",
            value: "default-src 'self'; script-src 'self' 'unsafe-inline' 'unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self' ws: wss: http: https:;",
          },
        ],
      },
    ];
  },
};

export default nextConfig;
