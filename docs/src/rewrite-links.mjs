// Makes the same Markdown work in two places.
//
// The pages under `docs/` are read both by this site and by anybody browsing
// the repository on GitHub, so their links are written the way GitHub needs:
// `configuration.md` for a sibling page, `internal/product/perf/x.md` for a
// file that is not one. Neither resolves once the pages are routes.
//
// This rewrites both at build time, so the source stays GitHub-correct and the
// built site has no dead links. Doing it the other way round, writing site
// routes into the source, would break the copy far more people will read first.
//
// Two rules, and nothing else is touched:
//
//   sibling.md             -> <base>/sibling/
//   anything/with/a/slash  -> the file on GitHub
//
// The second rule is not a list of directories, and that is deliberate. Every
// page is a direct child of `docs/`, which the collection's glob and
// `scripts/gates/m41-complete.sh` both depend on, so a Markdown link containing a
// slash cannot be a page on this site whatever it names. That was `../` when
// the design record lived at the repository root and is `internal/` now, and
// this did not have to change for the move.
//
// Anything already absolute, already a route, or an anchor is left alone.
import { visit } from 'unist-util-visit';

const REPO = 'https://github.com/huyz0/pgprox/blob/main';

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

      const from = path.replace(/^\.\//, '');

      // Not a sibling, so not a page: it names a file in the repository. A
      // `../` is repository-root relative once stripped; anything else is
      // relative to `docs/`, which is where these pages live.
      if (from.includes('/')) {
        const inRepo = from.startsWith('../')
          ? from.replace(/^(\.\.\/)+/, '')
          : `docs/${from}`;
        node.properties.href = `${REPO}/${inRepo}${fragment}`;
        return;
      }

      // A sibling page. `index.md` is the site root rather than `/index/`.
      const page = from.replace(/\.md$/, '');
      if (page === from) return; // not a Markdown link, leave it
      node.properties.href =
        page === 'index' ? `${prefix}/${fragment}` : `${prefix}/${page}/${fragment}`;
    });
  };
}
