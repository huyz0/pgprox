// The documentation site, and the pages it is built from.
//
// Both live in this directory. The Markdown beside this file is the product and
// is what somebody browsing the repository reads; everything else here is how
// that same Markdown becomes a site. One source, two audiences, and no copy
// between them that can drift.
//
// Everything Node needs is under `docs/` so the repository root stays a Rust
// project. Run the site from here, not from the root.
//
// `site` and `base` are what GitHub Pages needs to build correct links for a
// project page, which is served under /<repo>/ rather than at a domain root.
// Getting `base` wrong produces a site whose every internal link 404s, and it
// is not visible locally because `astro dev` serves from the root.
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import { rewriteLinks } from './src/rewrite-links.mjs';

const BASE = '/pgprox';

export default defineConfig({
  site: 'https://huyz0.github.io',
  base: BASE,
  // The pages link the way GitHub needs. See ./src/rewrite-links.mjs.
  markdown: { rehypePlugins: [rewriteLinks({ base: BASE })] },
  integrations: [
    starlight({
      title: 'pgprox',
      description:
        'A multitenant connection pooler for Postgres. Clients authenticate with a JWT and the proxy multiplexes them onto a capped pool of upstream connections.',
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/huyz0/pgprox',
        },
      ],
      // Ordered by what a reader wants first rather than alphabetically:
      // run it, know what it does, configure it, operate it, satisfy whoever
      // has to sign off on it, then understand how it works.
      sidebar: [
        { label: 'Getting started', link: '/getting-started/' },
        { label: 'Going to production', link: '/going-to-production/' },
        { label: 'Features and limits', link: '/features/' },
        { label: 'Multitenancy', link: '/multitenancy/' },
        { label: 'Read routing', link: '/read-routing/' },
        { label: 'Configuration', link: '/configuration/' },
        { label: 'Operations', link: '/operations/' },
        { label: 'Clustering and deployment', link: '/clustering/' },
        { label: 'Admin and management', link: '/admin/' },
        { label: 'Security', link: '/security/' },
        { label: 'FIPS builds', link: '/fips/' },
        { label: 'Architecture', link: '/architecture/' },
        { label: 'Request flow', link: '/request-flow/' },
        { label: 'Performance', link: '/performance/' },
        { label: 'Optimizations', link: '/optimizations/' },
      ],
      // Starlight resolves a page's edit URL with `new URL(path, baseUrl)`,
      // against the path the content collection stored. So this base has to
      // name the directory those paths are relative to, which is this one.
      //
      // Worth stating because it was wrong for two milestones and could not be
      // seen from here. While the collection read `../docs`, every path began
      // `../`, resolution spent it on the base rather than the path, and
      // `edit/main/` became `edit/`: fourteen pages linking to a branch called
      // `docs`. The link points at GitHub either way and nothing local follows
      // it, so only the built output ever showed it.
      // `scripts/gates/m44-complete.sh` resolves one and checks where it lands.
      editLink: {
        baseUrl: 'https://github.com/huyz0/pgprox/edit/main/docs/',
      },
      lastUpdated: true,
    }),
  ],
});
