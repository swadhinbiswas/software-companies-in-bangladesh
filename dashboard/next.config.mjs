/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  // Vercel + Cloudflare both support edge + static export via next-on-pages
  // For Cloudflare: build with `npx @cloudflare/next-on-pages`
  output: process.env.CF_PAGES ? undefined : undefined,
  experimental: {
    optimizePackageImports: ['duckdb-wasm'],
  },
  async headers() {
    return [
      {
        source: '/(.*)',
        headers: [
          { key: 'X-Dashboard', value: 'bd-jobs' },
        ],
      },
    ];
  },
};

export default nextConfig;
