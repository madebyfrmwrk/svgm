# Benchmarks

100 real-world SVG logos exported from Figma, Illustrator, Inkscape, and svgrepo. 902.7 KiB total original size. Same files, same machine, median of 5 runs.

- **svgm 0.2.2**
- **SVGO 4.0.1**
- 100 files
- Apple Silicon

## Summary

| Metric | SVGM | SVGO |
|--------|------|------|
| Speed (median) | 346.9ms | 11,594.5ms |
| Speedup | **33.4x faster** | baseline |
| Compression | **18.5%** | 18.2% |
| Compression gap | **+0.3 pts** | |
| Files where tool wins | **55** | 44 |
| Files tied | 1 | 1 |

## Timing

5 runs each, median taken. All runs on the same machine.

| Run | SVGM | SVGO |
|-----|------|------|
| 1 | 343.1ms | 11,439.1ms |
| 2 | 346.9ms | 11,625.9ms |
| 3 | 360.3ms | 11,594.5ms |
| 4 | 346.0ms | 11,557.9ms |
| 5 | 351.3ms | 11,706.2ms |
| **Median** | **346.9ms** | **11,594.5ms** |

## Where SVGM wins

On 55 files, SVGM produces smaller output than SVGO. The largest win is +4.9 pts on Microsoft Edge.

- Microsoft_Edge_logo_(2019)
- buick
- gg-deals
- xcode
- logo-chatgpt-atlas-38545_logosenvector.com_5
- vivaldi
- Stripe_Logo,_revised_2016
- Figma-logo (1)
- laravel
- tinder-1-logo-svgrepo-com
- Firefox_logo,_2019
- icon-kick
- THQ_Nordic_logo_2016
- vercel
- playtester-studio-logo-inverse
- midwest-games
- discord
- redbull-logo-svgrepo-com
- Samsung_wordmark
- airbnb-2-logo-svgrepo-com
- google
- find-us-on-facebook-logo-svgrepo-com
- cat
- samsung
- coca-cola-logo-svgrepo-com
- amazon
- supabase-logo-wordmark--dark
- stripe
- Remedy_Entertainment_logo_(2023-present)
- netflix
- google-workspace
- supabase-logo-wordmark--light
- Renault_logo
- oreo
- Okta_logo
- kellogg-s-red
- google-play-download-android-app-logo-svgrepo-com
- xx
- Unsplash_Logo_Full_Stacked
- unilever-2
- under-armour-logo-svgrepo-com
- dji-1
- critical-reflex
- rippling-vector-logo
- megabit
- Artboard 3
- perplexity
- id-tokenize
- kofi_symbol
- ping-identity-vector-logo-2023
- joint-chiefs-of-staff-logo-svgrepo-com
- Sina_Weibo
- PlayStation
- forbes-logo-svgrepo-com
- bmw-logo-svgrepo-com

## Where SVGM is close

On files where SVGO wins, the gap is typically under 1 percentage point. Only 1 file exceeds 3 points.

- browserstack-logo
- mirage
- origin-emoji-site-id
- Supercell-logo
- anthropic-icon
- chipotle
- raycast-logo-vector
- Opera_GX_Icon
- obsidian-icon
- Google_Play_Store_badge_EN
- nestle-13
- whatsapp-icon-logo-svgrepo-com
- dia
- jack-daniels-1-logo-svgrepo-com
- Artboard 2
- Bluesky_Logo
- ReLU Games_Logo_Black_KR
- claude
- itchio-textless-black
- Adobe_Corporate_logo (1)
- apidog
- google-play-console
- moonshot-ai
- oxc-dark
- oxc
- Steam_icon_logo (1)
- obsidian
- incident
- statickit
- danone-2
- oxc-icon
- statamic
- frmwrk
- vite
- astro
- id-nissan
- Cloudflare_Logo
- nuxt
- Amazon_logo
- instagram-2016-logo-svgrepo-com
- mcdonalds-5
- Unsplash_Logo_Full
- Epic_Games_logo

## Where SVGM still trails

Larger gaps are concentrated in files where SVGO's fill-rule removal heuristics or specific path rounding choices differ.

- gog

## Per-file breakdown

Compression percentages (higher means more reduction). The gap column shows how much more SVGM compresses than SVGO. Positive gaps mean SVGM wins. Sorted by SVGM advantage.

| File                                             |    SVGM |    SVGO |     Gap |         |
| ---                                              |     --- |     --- |     --- |     --- |
| Microsoft_Edge_logo_(2019)                       |    5.2% |    0.3% |   +4.9 |   ahead |
| buick                                            |   12.1% |    7.7% |   +4.4 |   ahead |
| gg-deals                                         |   13.2% |    9.2% |   +4.0 |   ahead |
| xcode                                            |   47.6% |   43.8% |   +3.8 |   ahead |
| logo-chatgpt-atlas-38545_logosenvector.com_5     |   21.8% |   18.2% |   +3.6 |   ahead |
| vivaldi                                          |    3.8% |    0.3% |   +3.5 |   ahead |
| Stripe_Logo,_revised_2016                        |   35.5% |   32.1% |   +3.4 |   ahead |
| Figma-logo (1)                                   |   28.0% |   24.7% |   +3.3 |   ahead |
| laravel                                          |   20.8% |   17.6% |   +3.2 |   ahead |
| tinder-1-logo-svgrepo-com                        |    5.5% |    2.4% |   +3.1 |   ahead |
| Firefox_logo,_2019                               |    4.8% |    2.1% |   +2.7 |   ahead |
| icon-kick                                        |    4.3% |    1.8% |   +2.5 |   ahead |
| THQ_Nordic_logo_2016                             |   39.4% |   37.0% |   +2.4 |   ahead |
| vercel                                           |   65.7% |   63.5% |   +2.2 |   ahead |
| playtester-studio-logo-inverse                   |    5.3% |    3.3% |   +2.0 |   ahead |
| midwest-games                                    |   15.5% |   13.7% |   +1.8 |   ahead |
| discord                                          |   41.6% |   39.8% |   +1.8 |   ahead |
| redbull-logo-svgrepo-com                         |    5.5% |    3.9% |   +1.6 |   ahead |
| Samsung_wordmark                                 |   40.1% |   38.6% |   +1.5 |   ahead |
| airbnb-2-logo-svgrepo-com                        |    5.6% |    4.2% |   +1.4 |   ahead |
| google                                           |    2.7% |    1.4% |   +1.3 |   ahead |
| find-us-on-facebook-logo-svgrepo-com             |    7.5% |    6.3% |   +1.2 |   ahead |
| cat                                              |   41.4% |   40.2% |   +1.2 |   ahead |
| samsung                                          |    3.7% |    2.6% |   +1.1 |   ahead |
| coca-cola-logo-svgrepo-com                       |    3.3% |    2.2% |   +1.1 |   ahead |
| amazon                                           |    2.9% |    1.8% |   +1.1 |   ahead |
| supabase-logo-wordmark--dark                     |   31.6% |   30.6% |   +1.0 |   ahead |
| stripe                                           |    3.2% |    2.2% |   +1.0 |   ahead |
| Remedy_Entertainment_logo_(2023-present)         |   24.7% |   23.7% |   +1.0 |   ahead |
| netflix                                          |    5.0% |    4.0% |   +1.0 |   ahead |
| google-workspace                                 |   64.1% |   63.1% |   +1.0 |   ahead |
| supabase-logo-wordmark--light                    |   31.7% |   30.8% |   +0.9 |   ahead |
| Renault_logo                                     |   26.6% |   25.7% |   +0.9 |   ahead |
| oreo                                             |    2.4% |    1.5% |   +0.9 |   ahead |
| Okta_logo                                        |   32.5% |   31.6% |   +0.9 |   ahead |
| kellogg-s-red                                    |    1.4% |    0.5% |   +0.9 |   ahead |
| google-play-download-android-app-logo-svgrepo-com |    4.2% |    3.3% |   +0.9 |   ahead |
| xx                                               |    7.7% |    6.9% |   +0.8 |   ahead |
| Unsplash_Logo_Full_Stacked                       |    1.3% |    0.5% |   +0.8 |   ahead |
| unilever-2                                       |    1.4% |    0.6% |   +0.8 |   ahead |
| under-armour-logo-svgrepo-com                    |    4.8% |    4.1% |   +0.7 |   ahead |
| dji-1                                            |   12.6% |   11.9% |   +0.7 |   ahead |
| critical-reflex                                  |    0.9% |    0.2% |   +0.7 |   ahead |
| rippling-vector-logo                             |   25.1% |   24.5% |   +0.6 |   ahead |
| megabit                                          |   15.4% |   14.8% |   +0.6 |   ahead |
| Artboard 3                                       |   10.9% |   10.3% |   +0.6 |   ahead |
| perplexity                                       |   59.8% |   59.4% |   +0.4 |   ahead |
| id-tokenize                                      |    3.3% |    2.9% |   +0.4 |   ahead |
| kofi_symbol                                      |   21.9% |   21.6% |   +0.3 |   ahead |
| ping-identity-vector-logo-2023                   |   21.0% |   20.8% |   +0.2 |   ahead |
| joint-chiefs-of-staff-logo-svgrepo-com           |    5.1% |    4.9% |   +0.2 |   ahead |
| Sina_Weibo                                       |   26.6% |   26.5% |   +0.1 |   ahead |
| PlayStation                                      |    0.1% |    0.0% |   +0.1 |   ahead |
| forbes-logo-svgrepo-com                          |    0.3% |    0.2% |   +0.1 |   ahead |
| Epic_Games_logo                                  |   53.9% |   53.9% |   +0.0 |  trails |
| dunkin-donuts-1                                  |    4.6% |    4.6% |   +0.0 |    tied |
| bmw-logo-svgrepo-com                             |    1.9% |    1.9% |   +0.0 |   ahead |
| Unsplash_Logo_Full                               |    0.3% |    0.4% |   -0.1 |  trails |
| mcdonalds-5                                      |    0.0% |    0.1% |   -0.1 |  trails |
| instagram-2016-logo-svgrepo-com                  |    2.8% |    2.9% |   -0.1 |  trails |
| Amazon_logo                                      |   35.6% |   35.8% |   -0.2 |  trails |
| nuxt                                             |   26.1% |   26.4% |   -0.3 |  trails |
| Cloudflare_Logo                                  |   17.1% |   17.4% |   -0.3 |  trails |
| id-nissan                                        |    6.2% |    6.6% |   -0.4 |  trails |
| astro                                            |   50.5% |   50.9% |   -0.4 |  trails |
| vite                                             |   25.2% |   25.7% |   -0.5 |  trails |
| frmwrk                                           |    3.5% |    4.0% |   -0.5 |  trails |
| statamic                                         |    1.9% |    2.5% |   -0.6 |  trails |
| oxc-icon                                         |   24.5% |   25.1% |   -0.6 |  trails |
| danone-2                                         |    1.8% |    2.4% |   -0.6 |  trails |
| statickit                                        |   22.1% |   22.8% |   -0.7 |  trails |
| incident                                         |   54.0% |   54.7% |   -0.7 |  trails |
| obsidian                                         |   53.0% |   53.8% |   -0.8 |  trails |
| Steam_icon_logo (1)                              |    1.4% |    2.3% |   -0.9 |  trails |
| oxc                                              |   24.6% |   25.5% |   -0.9 |  trails |
| oxc-dark                                         |   24.6% |   25.5% |   -0.9 |  trails |
| moonshot-ai                                      |   60.7% |   61.6% |   -0.9 |  trails |
| google-play-console                              |   60.0% |   60.9% |   -0.9 |  trails |
| apidog                                           |   55.0% |   55.9% |   -0.9 |  trails |
| Adobe_Corporate_logo (1)                         |   47.7% |   48.7% |   -1.0 |  trails |
| itchio-textless-black                            |    1.2% |    2.3% |   -1.1 |  trails |
| claude                                           |   52.0% |   53.1% |   -1.1 |  trails |
| ReLU Games_Logo_Black_KR                         |   16.3% |   17.5% |   -1.2 |  trails |
| Bluesky_Logo                                     |   10.2% |   11.4% |   -1.2 |  trails |
| Artboard 2                                       |    4.6% |    5.8% |   -1.2 |  trails |
| jack-daniels-1-logo-svgrepo-com                  |    3.1% |    4.4% |   -1.3 |  trails |
| dia                                              |    1.2% |    2.6% |   -1.4 |  trails |
| whatsapp-icon-logo-svgrepo-com                   |    6.0% |    7.5% |   -1.5 |  trails |
| nestle-13                                        |    9.1% |   10.6% |   -1.5 |  trails |
| Google_Play_Store_badge_EN                       |   31.5% |   33.1% |   -1.6 |  trails |
| obsidian-icon                                    |   43.6% |   45.3% |   -1.7 |  trails |
| Opera_GX_Icon                                    |   23.6% |   25.5% |   -1.9 |  trails |
| raycast-logo-vector                              |   19.6% |   21.6% |   -2.0 |  trails |
| chipotle                                         |    1.8% |    3.9% |   -2.1 |  trails |
| anthropic-icon                                   |   72.7% |   74.8% |   -2.1 |  trails |
| Supercell-logo                                   |   31.3% |   33.8% |   -2.5 |  trails |
| origin-emoji-site-id                             |    5.4% |    8.1% |   -2.7 |  trails |
| mirage                                           |    5.4% |    8.1% |   -2.7 |  trails |
| browserstack-logo                                |   30.2% |   33.0% |   -2.8 |  trails |
| gog                                              |   10.6% |   19.2% |   -8.6 |  trails |

