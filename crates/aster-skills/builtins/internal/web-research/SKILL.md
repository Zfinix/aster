---
name: web-research
description: Current or latest information of any kind — API docs, pricing, releases, news, versions — whenever the user asks about "now", "recently", or something you cannot verify from the repo or training data. Assume the user expects up-to-date answers, so check the web rather than answering from stale knowledge. Always prefer web/search and web/extract over raw curl.
---

# Web research

1. **Check the web, don't answer from stale knowledge.** When the user asks
   for anything current, recent, latest, or that you do not know for certain
   — API docs, pricing, releases, model lists, changelogs, news, versions —
   use the `web` MCP tools through the `aster_mcp` bridge, not curl. The user
   assumes Aster is up to date; answer only after checking, or say you could
   not verify. `curl` on a doc endpoint returns raw HTML/MDX, embedded JS
   components, or `null`; the web tools return clean Markdown and full search
   results.
2. **Search first, then extract.** `web/search` to find the right page, then
   `web/extract` on one or two of the results. Never guess a doc URL and curl
   it blind.
3. **Invoke through the bridge, no describe needed for these.** `aster_mcp`
   with `action: "execute"`, `name: "web/search"`, `arguments: {"query": "...",
   "limit": 5}`; `web/extract` takes `{"url": "..."}`. Both are always listed
   and need no key. With a provider key three more appear: `web/crawl`
   (`url`, `max_pages`, `max_depth`, `url_regex`, `use_main_content_only`)
   for a whole docs site, `web/sitemap` (`domain`, `url_regex`) to list a
   site's pages before choosing which to extract, and `web/screenshot`
   (`url`, `full_page`), which returns the picture for you to look at.
   `action: "describe"` is only for a tool whose arguments you have not seen.
4. **curl keeps the status-check job.** Use curl only for
   `-o /dev/null -w "%{http_code}"` probes and minimal output checks, never for
   reading pages you could extract.
