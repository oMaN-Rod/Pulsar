# Pulsar Themes

A static site for designing Pulsar themes on an emulated Windows 11 desktop, generating them from a color or a wallpaper, and sharing them with the community. It is published at https://oman-rod.github.io/Pulsar/, and Pulsar links to it from _Settings → Theme → More themes_. It is built with SvelteKit and Svelte 5, as a single page with hash routes.

- **Studio** (`#/`): the taskbar, overlay, popups and desktop widget as Pulsar draws them, with live readings. The settings panel covers colors, items, display, popups and the widget. Right-click the taskbar for its alignment, location and transparency. Desktop icons import, export and share themes and profiles. The theme picker sits next to the Night and Day switch.
- **Orbit** (`#/orbit`): generates a theme from a seed color and a harmony, or from colors found in a wallpaper, with night and day previews.

Theme files may carry `author` and `based_on`, which the gallery shows as credit; Pulsar ignores both.

## Layout

- `src/routes`: the layout (app bar, theme menu, toast), the studio (`+page.svelte`) and Orbit (`orbit/+page.svelte`).
- `src/lib`: the logic the pages share, in plain TypeScript (theme files, profiles, colors, the simulated readings, the tile geometry, the generator), the app state in `state.svelte.ts`, and the components under `components/`.
- `community`: shared themes and `index.json`, which lists them.
- `scripts`: `build-catalog.mjs` writes `static/catalog.json` from Pulsar's built-in themes and `community`; `theme-submission.mjs` checks and adds shared themes. Both run on Node 24 without a build step, importing `src/lib/theme.ts` directly, so the files under `src/lib` use only TypeScript that Node can strip.

## Run it locally

```sh
cd site
pnpm install
pnpm dev
```

`pnpm dev` serves the site with hot reloading; run `node scripts/build-catalog.mjs` again after changing a built-in or community theme. The other scripts:

- `pnpm check`: type and Svelte checks.
- `pnpm test`: unit tests for the theme and profile files.
- `pnpm build`: writes the catalog, then the site to `build/`. `pnpm preview` serves that build.

The site is served from the root unless `BASE_PATH` names a subfolder, as GitHub Pages needs: `BASE_PATH=/Pulsar pnpm build`.

## Hosting

The site uses hash routes (`#/`, `#/orbit`), so it needs no rewrite rules on any host.

- **GitHub Pages**: `.github/workflows/site.yml` checks, tests, builds with `BASE_PATH` set to the repository name and deploys on every push to `main` that touches the site or the themes. In the repository settings, set Pages' source to GitHub Actions.
- **Cloudflare Pages**: connect the repository with the root directory `site`, the build command `pnpm install --frozen-lockfile && pnpm build` and the output directory `build`, and set `NODE_VERSION` to `24`.

## Community themes

1. Someone opens an issue with the **Share a theme** form. The studio's Share icon fills it in, crediting the theme's author or, without one, the issue's author.
2. `.github/workflows/theme-submission.yml` checks the file with the same rules Pulsar uses, plus a strict pass that rejects unknown keys, and replies on the issue. Editing the issue checks it again.
3. A maintainer adds the `theme-approved` label. The workflow writes `site/community/<id>.toml`, lists it in `site/community/index.json`, and opens a pull request that closes the issue.

The repository needs the `theme-submission` and `theme-approved` labels, and _Allow GitHub Actions to create and approve pull requests_ turned on under Settings › Actions.
