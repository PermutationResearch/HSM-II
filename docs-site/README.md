# HSM-II documentation site (VitePress)

This folder builds the **project-owned** documentation site for HSM-II, Company OS, and operator agent-chat — intended to **host your own** docs (for example on GitHub Pages or any static host) instead of relying on third-party product handbooks.

The default theme is extended with **Paperclip-adjacent** visual language (see [paperclip.ing tokens](https://fontofweb.com/tokens/paperclip.ing)): **Inter** body, **Instrument Serif** headlines, warm stone background (`#efefee`), **Cod Gray / Dorado** text, **Westar** hairlines, soft card shadow, pill search control, and a **light-only** layout (`appearance: false`). Tweak colors and type in **`.vitepress/theme/custom.css`**.

## Develop

```bash
cd docs-site
npm install
npm run dev
```

Open the URL VitePress prints (usually `http://localhost:5173`).

`npm run dev` / `npm run build` automatically run **`sync-docs.mjs`**, which reads **`sync-manifest.json`** and:

1. Copies the operator handbook into **`guide/operator-handbook.md`** (with frontmatter).
2. Copies each listed doc into **`guide/reference/<slug>.md`** as a normal VitePress page.
3. Writes the **same markdown bytes** (no frontmatter) to **`public/llm/<name>.md`** — served as **`/llm/<name>.md`** after build (use for LLM `GET` / curl).
4. Writes **`public/route-to-llm.json`** (route → `/llm/…`) plus **`public/llm/docs-site-*.md`** exports for site-only pages (home, guide index, reference index).

The theme adds **Copy page (MD)** and **Open .md** in the nav bar: one click copies the full page markdown to the clipboard; crawlers can fetch the same file from **`/llm/…`**.

**Edit canonical markdown under `docs/` in the repo**; then run `npm run sync` or any dev/build to refresh generated files.

## Build for production

```bash
npm run build
```

Static output: **`docs-site/.vitepress/dist/`**

## Bundle into the marketing site (`web_interface/` on :4242)

From `docs-site/`:

```bash
npm run build:web
```

This builds with **`VITEPRESS_BASE=/docs/`**, **`VITEPRESS_WEB_BUNDLE=1`** (`.html` URLs for Python’s static server), and copies **`.vitepress/dist/`** to **`../web_interface/docs/`**. Serve **`web_interface/`** (e.g. `python3 -m http.server 4242`) and open **http://127.0.0.1:4242/docs/**.

## Deploy examples

- **GitHub Pages**: point the Pages “folder” to `/ (root)` or `/docs` of the `gh-pages` branch and upload the contents of `.vitepress/dist`, or use an Actions workflow that runs `npm ci && npm run build` and publishes `dist`.
- **Vercel / Netlify**: set root directory to `docs-site`, build command `npm run build`, publish directory `.vitepress/dist`.

## Replace an external docs URL

Point your DNS or hosting (e.g. `docs.yourcompany.com`) at this build output and update any links in apps or READMEs to your new origin. The handbook content stays versioned with the repo under `docs/company-os/`.
