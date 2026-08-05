// Where the pages come from.
//
// Starlight's own loader reads `src/content/docs/`. This points the same
// collection at this directory, which is where the Markdown already is, so the
// site is built from exactly the files somebody browsing the repository reads
// rather than from a copy that can drift.
//
// The pattern is deliberately not `**/*.md`. The site's own `node_modules` and
// `dist` live in here, and a recursive glob would walk both: a build that reads
// every Markdown file in a dependency tree is slow at best and picks up
// somebody's README at worst. Every page is a direct child of this directory,
// which is also what `scripts/m41-complete.sh` looks at when it checks for a
// title and a navigation entry, so the two agree by construction. A page in a
// subdirectory would need both to change.
import { defineCollection } from 'astro:content';
import { glob } from 'astro/loaders';
import { docsSchema } from '@astrojs/starlight/schema';

export const collections = {
  docs: defineCollection({
    loader: glob({ pattern: '*.md', base: '.' }),
    schema: docsSchema(),
  }),
};
