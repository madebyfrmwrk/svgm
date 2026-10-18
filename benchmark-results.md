# Benchmarks

100 real-world SVG logos exported from Figma, Illustrator, Inkscape, and svgrepo. 902.8 KiB total original size. Same files, same machine (Apple Silicon). Measured with `scripts/benchmark.py` (batch: median of 5 runs; per file: median of 3 runs).

- **svgm 0.4.0 + unreleased fixes** (main after #28)
- **SVGO 4.0.1**
- 100 files

## Summary

| Metric | SVGM | SVGO |
|--------|------|------|
| Compression | 18.1% | **18.2%** |
| Output bytes | 756,985 | 755,986 |
| Time, one process per file (median) | **633ms** | 12,126ms |
| Time, one process for the folder (median) | **120ms** | 293ms |
| Speedup | **19x** per file / **2.4x** batch | baseline |
| Files where tool wins | 32 | **66** |
| Files tied | 2 | 2 |

For reference, svgm 0.3.9 reached 18.5% on this corpus and won 51 files. 0.4.0 gave that back deliberately: it now refuses every optimisation SVGO refuses as unsafe (see CHANGELOG 0.4.0), and almost all of the loss is in path data (0.3.9 rounded past the configured precision; 0.4.0 follows SVGO's strongRound rule), plus an absolute/relative segment choice that is tracked for the next release.

Since 0.4.0 (unreleased, #28) svgm keeps `data-*` attributes such as Illustrator's `data-name` layer names, as SVGO does; 0.4.0 stripped them. That adds 184 bytes across the 8 files that carry them: 4 of them (`buick`, `Artboard 3`, `origin-emoji-site-id`, `xx`) stay ahead of SVGO, while `gog`, `PlayStation`, `megabit` and `Unsplash_Logo_Full_Stacked` drop to a tie or slightly behind, which is why the win count moved from 36 to 32. Compression is unchanged at one decimal.

Per-file timings are now taken by `scripts/benchmark.py`, which starts each process from Python. That adds the same per-process overhead to both tools, so they are not directly comparable with the 0.4.0 figures (476ms / 11,186ms, measured with a shell loop); the ratio is what to compare.

## Timing

Batch mode (`svgm -r dir -o out` vs `svgo -f dir -o out`), 5 runs each:

| Run | SVGM | SVGO |
|-----|------|------|
| 1 | 121.3ms | 341.2ms |
| 2 | 119.8ms | 301.7ms |
| 3 | 119.3ms | 292.9ms |
| 4 | 120.9ms | 287.3ms |
| 5 | 119.1ms | 292.6ms |
| **Median** | **119.8ms** | **292.9ms** |

Per-file mode (one process per file): svgm 633ms, SVGO 12,126ms (median of 3 runs).

## Per-file breakdown

Sorted by compression gap (svgm minus SVGO, percentage points).

| File | SVGM | SVGO | Gap | |
| --- | --- | --- | --- | --- |
| buick | 11.9% | 7.7% | +4.2 | ahead |
| xcode | 47.4% | 43.8% | +3.6 | ahead |
| logo-chatgpt-atlas-38545_logosenvector.com_5 | 21.5% | 18.2% | +3.4 | ahead |
| Figma-logo (1) | 28.0% | 24.7% | +3.3 | ahead |
| vercel | 66.4% | 63.5% | +2.9 | ahead |
| vivaldi | 2.7% | 0.3% | +2.5 | ahead |
| Stripe_Logo,_revised_2016 | 34.4% | 32.1% | +2.2 | ahead |
| Okta_logo | 33.5% | 31.6% | +1.9 | ahead |
| cat | 41.6% | 40.2% | +1.4 | ahead |
| origin-emoji-site-id | 9.4% | 8.1% | +1.4 | ahead |
| google-workspace | 64.3% | 63.1% | +1.3 | ahead |
| Firefox_logo,_2019 | 3.4% | 2.1% | +1.2 | ahead |
| discord | 40.9% | 39.8% | +1.1 | ahead |
| Artboard 3 | 11.4% | 10.3% | +1.1 | ahead |
| oreo | 2.4% | 1.5% | +0.9 | ahead |
| dji-1 | 12.6% | 11.9% | +0.7 | ahead |
| THQ_Nordic_logo_2016 | 37.8% | 37.0% | +0.7 | ahead |
| Artboard 2 | 6.5% | 5.8% | +0.7 | ahead |
| xx | 7.6% | 6.9% | +0.6 | ahead |
| Renault_logo | 26.3% | 25.7% | +0.6 | ahead |
| supabase-logo-wordmark--dark | 31.1% | 30.6% | +0.4 | ahead |
| supabase-logo-wordmark--light | 31.2% | 30.8% | +0.4 | ahead |
| Cloudflare_Logo | 17.8% | 17.4% | +0.4 | ahead |
| browserstack-logo | 33.3% | 33.0% | +0.4 | ahead |
| Remedy_Entertainment_logo_(2023-present) | 24.0% | 23.7% | +0.3 | ahead |
| perplexity | 59.7% | 59.4% | +0.3 | ahead |
| redbull-logo-svgrepo-com | 4.2% | 3.9% | +0.3 | ahead |
| airbnb-2-logo-svgrepo-com | 4.4% | 4.2% | +0.2 | ahead |
| Adobe_Corporate_logo (1) | 48.9% | 48.7% | +0.1 | ahead |
| astro | 51.0% | 50.9% | +0.1 | ahead |
| statickit | 22.8% | 22.8% | +0.0 | tied |
| obsidian | 53.8% | 53.8% | +0.0 | tied |
| icon-kick | 1.8% | 1.8% | +0.0 | tied |
| id-tokenize | 2.9% | 2.9% | +0.0 | tied |
| PlayStation | 0.0% | 0.0% | -0.0 | tied |
| Samsung_wordmark | 38.6% | 38.6% | -0.0 | tied |
| gog | 19.1% | 19.2% | -0.0 | tied |
| Epic_Games_logo | 53.9% | 53.9% | -0.0 | tied |
| instagram-2016-logo-svgrepo-com | 2.8% | 2.9% | -0.0 | tied |
| Microsoft_Edge_logo_(2019) | 0.2% | 0.3% | -0.1 | behind |
| forbes-logo-svgrepo-com | 0.1% | 0.2% | -0.1 | behind |
| samsung | 2.5% | 2.6% | -0.1 | behind |
| tinder-1-logo-svgrepo-com | 2.3% | 2.4% | -0.1 | behind |
| mcdonalds-5 | 0.0% | 0.1% | -0.1 | behind |
| midwest-games | 13.6% | 13.7% | -0.1 | behind |
| amazon | 1.6% | 1.8% | -0.1 | behind |
| Unsplash_Logo_Full | 0.3% | 0.4% | -0.2 | behind |
| critical-reflex | 0.0% | 0.2% | -0.2 | behind |
| Unsplash_Logo_Full_Stacked | 0.3% | 0.5% | -0.2 | behind |
| kofi_symbol | 21.4% | 21.6% | -0.2 | behind |
| Amazon_logo | 35.6% | 35.8% | -0.2 | behind |
| nuxt | 26.2% | 26.4% | -0.2 | behind |
| coca-cola-logo-svgrepo-com | 2.0% | 2.2% | -0.3 | behind |
| megabit | 14.5% | 14.8% | -0.3 | behind |
| google-play-console | 60.6% | 60.9% | -0.3 | behind |
| under-armour-logo-svgrepo-com | 3.8% | 4.1% | -0.3 | behind |
| ping-identity-vector-logo-2023 | 20.5% | 20.8% | -0.3 | behind |
| bmw-logo-svgrepo-com | 1.5% | 1.9% | -0.3 | behind |
| kellogg-s-red | 0.1% | 0.5% | -0.4 | behind |
| dunkin-donuts-1 | 4.2% | 4.6% | -0.4 | behind |
| rippling-vector-logo | 24.1% | 24.5% | -0.4 | behind |
| moonshot-ai | 61.1% | 61.6% | -0.5 | behind |
| unilever-2 | 0.1% | 0.6% | -0.5 | behind |
| danone-2 | 1.9% | 2.4% | -0.5 | behind |
| find-us-on-facebook-logo-svgrepo-com | 5.8% | 6.3% | -0.5 | behind |
| google-play-download-android-app-logo-svgrepo-com | 2.7% | 3.3% | -0.5 | behind |
| vite | 25.1% | 25.7% | -0.6 | behind |
| incident | 54.1% | 54.7% | -0.6 | behind |
| itchio-textless-black | 1.7% | 2.3% | -0.6 | behind |
| claude | 52.4% | 53.1% | -0.7 | behind |
| apidog | 55.2% | 55.9% | -0.7 | behind |
| oxc-icon | 24.4% | 25.1% | -0.7 | behind |
| dia | 1.9% | 2.6% | -0.7 | behind |
| id-nissan | 5.9% | 6.6% | -0.8 | behind |
| google | 0.6% | 1.4% | -0.8 | behind |
| Sina_Weibo | 25.7% | 26.5% | -0.8 | behind |
| joint-chiefs-of-staff-logo-svgrepo-com | 4.1% | 4.9% | -0.8 | behind |
| oxc-dark | 24.6% | 25.5% | -0.9 | behind |
| oxc | 24.6% | 25.5% | -0.9 | behind |
| playtester-studio-logo-inverse | 2.3% | 3.3% | -1.0 | behind |
| statamic | 1.5% | 2.5% | -1.0 | behind |
| Steam_icon_logo (1) | 1.1% | 2.3% | -1.1 | behind |
| ReLU Games_Logo_Black_KR | 16.3% | 17.5% | -1.2 | behind |
| laravel | 16.3% | 17.6% | -1.2 | behind |
| obsidian-icon | 44.0% | 45.3% | -1.3 | behind |
| gg-deals | 7.9% | 9.2% | -1.3 | behind |
| netflix | 2.7% | 4.0% | -1.3 | behind |
| Bluesky_Logo | 10.1% | 11.4% | -1.3 | behind |
| anthropic-icon | 73.3% | 74.8% | -1.5 | behind |
| Opera_GX_Icon | 23.9% | 25.5% | -1.6 | behind |
| nestle-13 | 9.0% | 10.6% | -1.7 | behind |
| stripe | 0.5% | 2.2% | -1.7 | behind |
| jack-daniels-1-logo-svgrepo-com | 2.5% | 4.4% | -1.8 | behind |
| chipotle | 1.8% | 3.9% | -2.0 | behind |
| raycast-logo-vector | 19.1% | 21.6% | -2.5 | behind |
| Supercell-logo | 31.3% | 33.8% | -2.6 | behind |
| Google_Play_Store_badge_EN | 30.5% | 33.1% | -2.6 | behind |
| whatsapp-icon-logo-svgrepo-com | 4.6% | 7.5% | -2.8 | behind |
| frmwrk | 1.1% | 4.0% | -2.9 | behind |
| mirage | 4.9% | 8.1% | -3.2 | behind |
