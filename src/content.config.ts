// Where the pages come from.
//
// Starlight's own loader reads `src/content/docs/`. This points the same
// collection at `docs/` instead, so the Markdown stays where somebody browsing
// the repository will find it and the site is built from exactly those files
// rather than from a copy that can drift.
import { defineCollection } from 'astro:content';
import { glob } from 'astro/loaders';
import { docsSchema } from '@astrojs/starlight/schema';

export const collections = {
  docs: defineCollection({
    loader: glob({ pattern: '**/*.md', base: './docs' }),
    schema: docsSchema(),
  }),
};
