import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  output: "export",
  trailingSlash: true,
  experimental: {
    reactCompiler: true
  }
};

export default nextConfig;
