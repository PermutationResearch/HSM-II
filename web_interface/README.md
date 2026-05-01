# HSM-II marketing site (static)

Served locally on **port 4242** (for example `python3 -m http.server 4242` from this directory). Open **http://127.0.0.1:4242/**.

## Documentation under `/docs/`

The VitePress site in **`docs-site/`** can be built with base path **`/docs/`** and copied here so the same origin serves the handbook and reference:

```bash
cd docs-site && npm install && npm run build:web
cd ../web_interface && python3 -m http.server 4242
```

Then open **http://127.0.0.1:4242/docs/** (nav **Documentation** and hero **Read the Docs** point there).

The **`docs/`** folder is gitignored; run **`npm run build:web`** in `docs-site` whenever you want an updated bundle on the marketing server.
