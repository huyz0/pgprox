// Makes the same Markdown work in two places.
//
// The pages under `docs/` are read both by this site and by anybody browsing
// the repository on GitHub, so their links are written the way GitHub needs:
// `configuration.md` for a sibling page, `../product/perf/x.md` for a file
// outside the docs. Neither resolves once the pages are routes.
//
// This rewrites both at build time, so the source stays GitHub-correct and the
// built site has no dead links. Doing it the other way round, writing site
// routes into the source, would break the copy far more people will read first.
//
// Two rules, and nothing else is touched:
//
//   sibling.md            -> <base>/sibling/
//   ../anything/else.md   -> the file on GitHub
//
// Anything already absolute, already a route, or an anchor is left alone.
import { visit } from 'unist-util-visit';

const REPO = 'https://github.com/pgprox/pgprox/blob/main';

export function rewriteLinks({ base = '' } = {}) {
  const prefix = base.endsWith('/') ? base.slice(0, -1) : base;

  return () => (tree) => {
    visit(tree, 'element', (node) => {
      if (node.tagName !== 'a') return;
      const href = node.properties?.href;
      if (typeof href !== 'string' || href === '') return;

      // Absolute, protocol-relative, anchors and mail: not ours to touch.
      if (/^([a-z]+:|\/\/|\/|#)/i.test(href)) return;

      const [path, hash = ''] = href.split('#');
      const fragment = hash ? `#${hash}` : '';

      // Escapes the docs directory, so it names a file in the repository
      // rather than a page on this site.
      if (path.startsWith('../')) {
        node.properties.href = `${REPO}/${path.replace(/^(\.\.\/)+/, '')}${fragment}`;
        return;
      }

      // A sibling page. `index.md` is the site root rather than `/index/`.
      const page = path.replace(/^\.\//, '').replace(/\.md$/, '');
      if (page === path) return; // not a Markdown link, leave it
      node.properties.href =
        page === 'index' ? `${prefix}/${fragment}` : `${prefix}/${page}/${fragment}`;
    });
  };
}
