const path = require('path');
const tiptapPmResolveBase = path.dirname(require.resolve('@tiptap/pm/model'));
const resolveFromTiptapPm = (pkg) =>
  require.resolve(pkg, { paths: [tiptapPmResolveBase] });

/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  // Next 16 writes AGENTS.md and CLAUDE.md into the project on dev-server start.
  // This repository maintains its own CLAUDE.md at the root; a generated one in
  // frontend/ would feed future agent sessions boilerplate instead of it.
  agentRules: false,
  output: 'export',
  images: {
    unoptimized: true,
  },
  // Add basePath configuration
  basePath: '',
  assetPrefix: '/',

  // Add webpack configuration for Tauri
  webpack: (config, { isServer }) => {
    if (!isServer) {
      // Tauri opens its window as soon as the dev server answers `/`, but Next
      // compiles routes on demand. On a cold start — especially while cargo is
      // still using the CPU — the first chunk request can outlast webpack's
      // 120 s default and the window comes up dead with a ChunkLoadError until
      // it is reloaded by hand.
      //
      // Loading BlockNote on demand took roughly a megabyte out of the
      // meeting-details route's initial JavaScript (2256 KB to 1198 KB
      // measured on the production build), which is the bulk of what made
      // that first chunk slow to compile. This ceiling is kept anyway: it
      // costs nothing when compilation is quick, and whether cold starts now
      // finish inside 120 s can only be established by actually running one
      // against a cleared .next, not by reasoning about bundle sizes.
      config.output = {
        ...config.output,
        chunkLoadTimeout: 600000,
      };

      config.resolve.fallback = {
        ...config.resolve.fallback,
        fs: false,
        path: false,
        os: false,
      };

      // Keep ProseMirror single-instanced for BlockNote/Tiptap. The @blocknote
      // packages themselves are not aliased: pnpm already resolves exactly one
      // copy of each, and an alias via require.resolve would pin webpack to
      // their CJS builds, which since 0.54 require ESM-only dependencies.
      config.resolve.alias = {
        ...config.resolve.alias,
        'prosemirror-model': resolveFromTiptapPm('prosemirror-model'),
        'prosemirror-state': resolveFromTiptapPm('prosemirror-state'),
        'prosemirror-view': resolveFromTiptapPm('prosemirror-view'),
        'prosemirror-transform': resolveFromTiptapPm('prosemirror-transform'),
        'prosemirror-tables': resolveFromTiptapPm('prosemirror-tables'),
        'prosemirror-schema-list': resolveFromTiptapPm('prosemirror-schema-list'),
        'prosemirror-keymap': resolveFromTiptapPm('prosemirror-keymap'),
        'prosemirror-commands': resolveFromTiptapPm('prosemirror-commands'),
        'prosemirror-history': resolveFromTiptapPm('prosemirror-history'),
        'prosemirror-inputrules': resolveFromTiptapPm('prosemirror-inputrules'),
        'prosemirror-gapcursor': resolveFromTiptapPm('prosemirror-gapcursor'),
        'prosemirror-dropcursor': resolveFromTiptapPm('prosemirror-dropcursor'),
      };
    }
    return config;
  },
}

module.exports = nextConfig
