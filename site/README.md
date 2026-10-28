# Moeka website

Nuxt + Less static landing page. Keep every translation's feature claims in sync
with the desktop application.

## Commands

```sh
npm ci
npm run dev
npm run generate
npm run preview
```

The site is isolated under this directory and does not participate in the Rust desktop app build.

Builds subset the Noto Serif fonts to the characters used by the page. Source
fonts are downloaded on a cache miss; a first build needs network access.
Review font notices and all artwork provenance before distributing or deploying
the site; see [third-party notices](../THIRD_PARTY_NOTICES.md).
