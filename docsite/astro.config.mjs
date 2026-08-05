// The documentation site.
//
// The Markdown is not in here. It stays in the repository's `docs/`, and
// `src/content.config.ts` points this site's collection at it, so the same
// files serve two audiences: this site, and anybody reading the repo on GitHub.
//
// Everything Node needs lives in this directory so the repository root stays a
// Rust project. Run the site from here, not from the root.
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
  site: 'https://pgprox.github.io',
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
          href: 'https://github.com/pgprox/pgprox',
        },
      ],
      // Ordered by what a reader wants first rather than alphabetically:
      // run it, configure it, operate it, then understand it.
      sidebar: [
        { label: 'Getting started', link: '/getting-started/' },
        { label: 'Configuration', link: '/configuration/' },
        { label: 'Operations', link: '/operations/' },
        { label: 'Architecture', link: '/architecture/' },
        { label: 'Performance', link: '/performance/' },
      ],
      editLink: {
        baseUrl: 'https://github.com/pgprox/pgprox/edit/main/',
      },
      lastUpdated: true,
    }),
  ],
});
