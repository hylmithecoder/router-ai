import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // Export statically so the Rust binary can serve the dashboard itself.
  output: "export",
  // Emit pages as index.html inside folders (ServeDir-friendly) instead of
  // flat `<page>.html` files.
  trailingSlash: true,
};

export default nextConfig;
