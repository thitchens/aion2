# AION 2 — Flight Log

A lightweight GitHub Pages checklist for AION 2 dailies, weeklies, and shop/craft limits.

Checks are stored in **localStorage** in your browser. They clear automatically when the Global server resets:

| Period | Reset |
| --- | --- |
| Daily | Every day at **07:00 UTC** |
| Weekly (and Buy & Craft) | **Wednesday** at **07:00 UTC** |

Notes persist across resets.

## Preview locally

Open `index.html` in a browser, or serve the folder:

```bash
python3 -m http.server 8080
```

Then visit `http://localhost:8080`.

## Publish on GitHub Pages

1. Create a GitHub repo and push this project.
2. **Settings → Pages → Build and deployment**
3. Source: **Deploy from a branch**
4. Branch: `main` (or `master`), folder: `/ (root)`
5. Save — the site will be at `https://<user>.github.io/<repo>/`

If the repo is named `<user>.github.io`, it will be available at the root of that domain.

## Custom tasks

Use **Add** under each section for personal tasks. Custom items can be removed with ✕. Built-in tasks stay in the list so you can uncheck them after the next reset.
