// Standalone HTTP server serving static-ish feed fixtures for the Playwright e2e suite. Started
// as a Playwright `webServer` entry (see ../../playwright.config.ts), independent of the backend.
//
// The backend drops ingested items older than `ingest.max_item_age_days` (default 1, see
// backend/src/ingest/store.rs:49,81-83), and the bundled backend/tests/fixtures/* carry stale
// June-2025 dates. So every route below injects the *current* timestamp into the feed body at
// request time, keeping items inside the age cutoff regardless of when the suite runs.
//
// Also imported (for its title constants only) by ../support/api.ts, which playwright.config.ts
// pulls in while *loading the config* - i.e. in the Playwright CLI process, not the child process
// the `webServer` entry spawns. The listen() call below is guarded to only run when this file is
// executed directly, so that config-load import doesn't itself bind the port.
import http from "node:http";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const PORT = Number(process.env.FIXTURE_PORT ?? 8098);

// How many /flaky requests fail before it starts succeeding, restored on /_control/reset.
const DEFAULT_FLAKY_FAILURES = 2;

// Stable, known titles the specs assert on (must match web/e2e/support/api.ts FIXTURE.*).
export const RSS_FEED_TITLE = "E2E Fixture RSS Feed";
export const RSS_ITEM_TITLE = "E2E Fixture RSS Item One";
export const ATOM_FEED_TITLE = "E2E Fixture Atom Feed";
export const ATOM_ITEM_TITLE = "E2E Fixture Atom Item One";
export const JSON_FEED_TITLE = "E2E Fixture JSON Feed";
export const JSON_ITEM_TITLE = "E2E Fixture JSON Item One";
export const SUMMARY_ITEM_TITLE = "E2E Summary Fixture Item";
export const YOUTUBE_ITEM_TITLE = "E2E YouTube Fixture Video";
export const REDDIT_HIGH_SCORE_ITEM_TITLE =
    "E2E Reddit Fixture High Score Post";
export const REDDIT_LOW_SCORE_ITEM_TITLE = "E2E Reddit Fixture Low Score Post";

function rssBody() {
    const pubDate = new Date().toUTCString();
    return `<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>${RSS_FEED_TITLE}</title>
    <link>https://fixtures.example/rss</link>
    <description>Offline e2e fixture feed (RSS 2.0)</description>
    <item>
      <title>${RSS_ITEM_TITLE}</title>
      <link>https://fixtures.example/rss/item-one</link>
      <guid>https://fixtures.example/rss/item-one</guid>
      <pubDate>${pubDate}</pubDate>
      <description><![CDATA[<p>First fixture item body.</p>]]></description>
    </item>
    <item>
      <title>E2E Fixture RSS Item Two</title>
      <link>https://fixtures.example/rss/item-two</link>
      <guid>https://fixtures.example/rss/item-two</guid>
      <pubDate>${pubDate}</pubDate>
      <description><![CDATA[<p>Second fixture item body.</p>]]></description>
    </item>
  </channel>
</rss>
`;
}

function atomBody() {
    const updated = new Date().toISOString();
    return `<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>${ATOM_FEED_TITLE}</title>
  <link href="https://fixtures.example/atom"/>
  <updated>${updated}</updated>
  <entry>
    <title>${ATOM_ITEM_TITLE}</title>
    <link href="https://fixtures.example/atom/item-one"/>
    <id>https://fixtures.example/atom/item-one</id>
    <updated>${updated}</updated>
    <content type="html"><![CDATA[<p>First fixture entry body.</p>]]></content>
  </entry>
  <entry>
    <title>E2E Fixture Atom Item Two</title>
    <link href="https://fixtures.example/atom/item-two"/>
    <id>https://fixtures.example/atom/item-two</id>
    <updated>${updated}</updated>
    <content type="html"><![CDATA[<p>Second fixture entry body.</p>]]></content>
  </entry>
</feed>
`;
}

function jsonFeedBody() {
    const datePublished = new Date().toISOString();
    return JSON.stringify(
        {
            version: "https://jsonfeed.org/version/1.1",
            title: JSON_FEED_TITLE,
            home_page_url: "https://fixtures.example/json",
            feed_url: "http://localhost:8098/feed.json",
            items: [
                {
                    id: "https://fixtures.example/json/item-one",
                    url: "https://fixtures.example/json/item-one",
                    title: JSON_ITEM_TITLE,
                    content_html: "<p>First fixture item body.</p>",
                    date_published: datePublished,
                },
                {
                    id: "https://fixtures.example/json/item-two",
                    url: "https://fixtures.example/json/item-two",
                    title: "E2E Fixture JSON Item Two",
                    content_html: "<p>Second fixture item body.</p>",
                    date_published: datePublished,
                },
            ],
        },
        null,
        2,
    );
}

function summaryFeedBody() {
    const datePublished = new Date().toISOString();
    return JSON.stringify({
        version: "https://jsonfeed.org/version/1.1",
        title: "E2E Summary Fixture Feed",
        items: [
            {
                id: "https://fixtures.example/summary/item",
                url: "https://fixtures.example/summary/item",
                title: SUMMARY_ITEM_TITLE,
                content_text:
                    "A plain-text fixture article that can be summarized.",
                date_published: datePublished,
            },
        ],
    });
}

// yt:videoId/media:group/media:thumbnail/media:community mirror a real YouTube channel feed's
// shape. NOTE (verified against backend/src/ingest/parse.rs + feed-rs 2.4.0): the `duration`
// attribute on <media:content> lands on feed_rs::model::MediaContent, not on the aggregate
// MediaObject the backend reads (`e.media.iter().find_map(|m| m.duration...)`); that field is
// only ever populated from an RSS2 <itunes:duration>, which the Atom parser never looks at. So
// this fixture cannot make duration_secs non-null through the real ingest pipeline - see the
// implementation report for this issue.
function youtubeFeedBody() {
    const updated = new Date().toISOString();
    return `<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:yt="http://www.youtube.com/xml/schemas/2015" xmlns:media="http://search.yahoo.com/mrss/">
  <id>yt:channel:e2e-fixture</id>
  <yt:channelId>e2e-fixture</yt:channelId>
  <title>E2E YouTube Fixture Feed</title>
  <link rel="alternate" href="https://www.youtube.com/channel/e2e-fixture"/>
  <author>
    <name>E2E Fixture Channel</name>
    <uri>https://www.youtube.com/channel/e2e-fixture</uri>
  </author>
  <published>${updated}</published>
  <entry>
    <id>yt:video:e2efixtur01</id>
    <yt:videoId>e2efixtur01</yt:videoId>
    <yt:channelId>e2e-fixture</yt:channelId>
    <title>${YOUTUBE_ITEM_TITLE}</title>
    <link rel="alternate" href="https://www.youtube.com/watch?v=e2efixtur01"/>
    <author>
      <name>E2E Fixture Channel</name>
      <uri>https://www.youtube.com/channel/e2e-fixture</uri>
    </author>
    <published>${updated}</published>
    <updated>${updated}</updated>
    <media:group>
      <media:title>${YOUTUBE_ITEM_TITLE}</media:title>
      <media:content url="https://www.youtube.com/v/e2efixtur01?version=3" type="application/x-shockwave-flash" width="640" height="390" duration="212"/>
      <media:thumbnail url="https://i.ytimg.com/vi/e2efixtur01/hqdefault.jpg" width="480" height="360"/>
      <media:description>Video description used by the local YouTube fixture.</media:description>
      <media:community>
        <media:starRating count="10" average="5.0" min="1" max="5"/>
        <media:statistics views="1000"/>
      </media:community>
    </media:group>
  </entry>
</feed>
`;
}

// A Reddit "top" JSON listing (matches backend/src/ingest/reddit.rs::parse_listing's
// data.children[].data shape), with one high-score and one low-score post so a spec can exercise
// the per-feed min_score filter. NOTE: see the implementation report - the real ingest scheduler
// (backend/src/ingest/scheduler.rs::process_reddit) always polls hardcoded reddit.com URLs for a
// `kind: reddit` feed, so this fixture cannot currently be reached through subscribe+ingest; it's
// reachable directly (curl, or a Rust unit test calling reddit::parse_listing).
function redditListingBody(minScore) {
    const createdUtc = Math.floor(Date.now() / 1000);
    const posts = [
        {
            id: "e2ehigh1",
            title: REDDIT_HIGH_SCORE_ITEM_TITLE,
            author: "e2e_fixture_user",
            permalink: "/r/e2efixture/comments/e2ehigh1/high_score_post/",
            is_self: true,
            selftext: "A high-scoring fixture post body.",
            score: 500,
            num_comments: 42,
            upvote_ratio: 0.97,
            created_utc: createdUtc,
            thumbnail: "https://fixtures.example/reddit/high.jpg",
        },
        {
            id: "e2elow1",
            title: REDDIT_LOW_SCORE_ITEM_TITLE,
            author: "e2e_fixture_user",
            permalink: "/r/e2efixture/comments/e2elow1/low_score_post/",
            is_self: true,
            selftext: "A low-scoring fixture post body.",
            score: 3,
            num_comments: 1,
            upvote_ratio: 0.55,
            created_utc: createdUtc,
            thumbnail: "https://fixtures.example/reddit/low.jpg",
        },
    ];
    const filtered =
        minScore === null ? posts : posts.filter((p) => p.score >= minScore);
    return JSON.stringify({
        data: {
            children: filtered.map((data) => ({ kind: "t3", data })),
        },
    });
}

// <link rel="alternate" type="..."> pages for the AddFeedModal discovery step
// (backend/src/ingest/discover.rs::sniff_alternate_links reads these tags directly, with no
// requirement that the hrefs themselves be independently fetched).
function discoverMultiBody() {
    return `<!doctype html>
<html>
<head>
  <title>E2E Discovery Fixture (multiple candidates)</title>
  <link rel="alternate" type="application/rss+xml" href="/rss.xml" title="${RSS_FEED_TITLE}">
  <link rel="alternate" type="application/atom+xml" href="/atom.xml" title="${ATOM_FEED_TITLE}">
  <link rel="alternate" type="application/json" href="/feed.json" title="${JSON_FEED_TITLE}">
</head>
<body><h1>Discovery fixture (multiple candidates)</h1></body>
</html>
`;
}

function discoverNoneBody() {
    return `<!doctype html>
<html>
<head><title>E2E Discovery Fixture (zero candidates)</title></head>
<body><h1>No feeds linked from this page.</h1></body>
</html>
`;
}

const ROUTES = {
    "/health": () => ({ status: 200, contentType: "text/plain", body: "ok" }),
    "/rss.xml": () => ({
        status: 200,
        contentType: "application/rss+xml",
        body: rssBody(),
    }),
    "/atom.xml": () => ({
        status: 200,
        contentType: "application/atom+xml",
        body: atomBody(),
    }),
    "/feed.json": () => ({
        status: 200,
        contentType: "application/feed+json",
        body: jsonFeedBody(),
    }),
    "/summary.json": () => ({
        status: 200,
        contentType: "application/feed+json",
        body: summaryFeedBody(),
    }),
    "/youtube.xml": () => ({
        status: 200,
        contentType: "application/atom+xml",
        body: youtubeFeedBody(),
    }),
    "/fail/500": () => ({
        status: 500,
        contentType: "text/plain",
        body: "fixture: forced 500",
    }),
    "/fail/404": () => ({
        status: 404,
        contentType: "text/plain",
        body: "fixture: forced 404",
    }),
    "/fail/malformed": () => ({
        status: 200,
        contentType: "application/rss+xml",
        // Well-formed HTTP, but not a feed feed-rs can parse.
        body: "this is not a feed, just plain text {{{",
    }),
    "/image/broken": () => ({
        status: 404,
        contentType: "text/plain",
        body: "fixture: broken image",
    }),
    "/discover/multi": () => ({
        status: 200,
        contentType: "text/html",
        body: discoverMultiBody(),
    }),
    "/discover/none": () => ({
        status: 200,
        contentType: "text/html",
        body: discoverNoneBody(),
    }),
};

// Mutable, process-local fixture state. Specs configure this through the helper endpoints below,
// so provider/feed/ntfy behavior stays deterministic and never leaves the e2e process.
let aiResponses = {};
let aiRequests = [];
let ntfyReceipts = [];
let ntfyForcedFailureStatus = null;
let flakyFailuresRemaining = DEFAULT_FLAKY_FAILURES;

function resetState() {
    aiResponses = {};
    aiRequests = [];
    ntfyReceipts = [];
    ntfyForcedFailureStatus = null;
    flakyFailuresRemaining = DEFAULT_FLAKY_FAILURES;
}

function sleep(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

function json(res, status, body) {
    res.writeHead(status, { "Content-Type": "application/json" });
    res.end(JSON.stringify(body));
}

function readJson(req) {
    return new Promise((resolve) => {
        let body = "";
        req.on("data", (chunk) => {
            body += chunk;
        });
        req.on("end", () => {
            try {
                resolve(body ? JSON.parse(body) : {});
            } catch {
                resolve({});
            }
        });
    });
}

function readRaw(req) {
    return new Promise((resolve) => {
        let body = "";
        req.on("data", (chunk) => {
            body += chunk;
        });
        req.on("end", () => resolve(body));
    });
}

function aiResponse(model) {
    return (
        aiResponses[model] ?? {
            status: 200,
            text: `AI fixture summary from ${model}.`,
        }
    );
}

async function respondOpenAi(res, model) {
    const response = aiResponse(model);
    if (response.delayMs) {
        await sleep(response.delayMs);
    }
    if (response.malformed) {
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end("{not valid json");
        return;
    }
    if (response.status !== 200) {
        json(res, response.status, {
            error: { message: response.error ?? `fixture error from ${model}` },
        });
        return;
    }
    json(res, 200, {
        choices: [{ message: { content: response.text } }],
        usage: { total_tokens: 12 },
    });
}

async function respondAnthropic(res, model) {
    const response = aiResponse(model);
    if (response.delayMs) {
        await sleep(response.delayMs);
    }
    if (response.malformed) {
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end("{not valid json");
        return;
    }
    if (response.status !== 200) {
        json(res, response.status, {
            error: { message: response.error ?? `fixture error from ${model}` },
        });
        return;
    }
    json(res, 200, {
        content: [{ type: "text", text: response.text }],
        usage: { input_tokens: 8, output_tokens: 12 },
    });
}

async function respondGemini(res, model) {
    const response = aiResponse(model);
    if (response.delayMs) {
        await sleep(response.delayMs);
    }
    if (response.malformed) {
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end("{not valid json");
        return;
    }
    if (response.status !== 200) {
        json(res, response.status, {
            error: { message: response.error ?? `fixture error from ${model}` },
        });
        return;
    }
    json(res, 200, {
        steps: [
            {
                type: "model_output",
                content: [{ type: "text", text: response.text }],
            },
        ],
        usage: { total_tokens: 12 },
    });
}

if (
    process.argv[1] &&
    fileURLToPath(import.meta.url) === resolve(process.argv[1])
) {
    const server = http.createServer(async (req, res) => {
        // A slow/timeout route's client can disconnect before we respond; don't let that surface
        // as an unhandled 'error' event and crash the fixture process.
        res.on("error", () => {});

        const { pathname, searchParams } = new URL(
            req.url ?? "/",
            `http://localhost:${PORT}`,
        );

        if (pathname === "/_control/reset" && req.method === "POST") {
            resetState();
            json(res, 200, { ok: true });
            return;
        }
        if (pathname === "/_control/flaky" && req.method === "POST") {
            const body = await readJson(req);
            flakyFailuresRemaining = Number(
                body.failures ?? DEFAULT_FLAKY_FAILURES,
            );
            json(res, 200, { ok: true, failures: flakyFailuresRemaining });
            return;
        }
        if (pathname === "/_control/ntfy/fail" && req.method === "POST") {
            const body = await readJson(req);
            ntfyForcedFailureStatus = Number(body.status ?? 500);
            json(res, 200, { ok: true, status: ntfyForcedFailureStatus });
            return;
        }

        if (pathname === "/ai-mock/reset" && req.method === "POST") {
            aiResponses = {};
            aiRequests = [];
            json(res, 200, { ok: true });
            return;
        }
        if (pathname === "/ai-mock/config" && req.method === "POST") {
            const body = await readJson(req);
            aiResponses = body.responses ?? {};
            aiRequests = [];
            json(res, 200, { ok: true });
            return;
        }
        if (pathname === "/ai-mock/requests" && req.method === "GET") {
            json(res, 200, { requests: aiRequests });
            return;
        }
        if (
            pathname === "/ai-mock/openai/chat/completions" &&
            req.method === "POST"
        ) {
            const body = await readJson(req);
            const model = body.model ?? "unknown";
            aiRequests.push({ kind: "openai", model });
            await respondOpenAi(res, model);
            return;
        }
        if (
            pathname === "/ai-mock/anthropic/messages" &&
            req.method === "POST"
        ) {
            const body = await readJson(req);
            const model = body.model ?? "unknown";
            aiRequests.push({ kind: "anthropic", model });
            await respondAnthropic(res, model);
            return;
        }
        if (pathname === "/ai-mock/interactions" && req.method === "POST") {
            const body = await readJson(req);
            const model = body.model ?? "unknown";
            aiRequests.push({ kind: "gemini", model });
            await respondGemini(res, model);
            return;
        }

        if (pathname === "/reddit.xml") {
            const minScore = searchParams.has("min_score")
                ? Number(searchParams.get("min_score"))
                : null;
            res.writeHead(200, { "Content-Type": "application/json" });
            res.end(redditListingBody(minScore));
            return;
        }

        if (pathname === "/fail/timeout") {
            const ms = Number(searchParams.get("ms") ?? 5000);
            await sleep(ms);
            if (!res.writableEnded && !res.destroyed) {
                res.writeHead(200, { "Content-Type": "application/rss+xml" });
                res.end(rssBody());
            }
            return;
        }
        if (pathname === "/slow") {
            const ms = Number(searchParams.get("ms") ?? 1000);
            await sleep(ms);
            if (!res.writableEnded && !res.destroyed) {
                res.writeHead(200, { "Content-Type": "application/rss+xml" });
                res.end(rssBody());
            }
            return;
        }
        if (pathname === "/flaky") {
            if (flakyFailuresRemaining > 0) {
                flakyFailuresRemaining -= 1;
                res.writeHead(500, { "Content-Type": "text/plain" });
                res.end("fixture: flaky failure");
                return;
            }
            res.writeHead(200, { "Content-Type": "application/rss+xml" });
            res.end(rssBody());
            return;
        }

        if (pathname.startsWith("/ntfy/") && pathname !== "/ntfy/_received") {
            if (req.method !== "POST") {
                res.writeHead(405, { "Content-Type": "text/plain" });
                res.end("method not allowed");
                return;
            }
            const topic = pathname.slice("/ntfy/".length);
            const body = await readRaw(req);
            ntfyReceipts.push({
                topic,
                title: req.headers.title ?? null,
                priority: req.headers.priority ?? null,
                tags: req.headers.tags ?? null,
                click: req.headers.click ?? null,
                hasAuth: Boolean(req.headers.authorization),
                body,
            });
            if (ntfyForcedFailureStatus !== null) {
                res.writeHead(ntfyForcedFailureStatus, {
                    "Content-Type": "text/plain",
                });
                res.end("fixture: forced ntfy failure");
                return;
            }
            res.writeHead(200, { "Content-Type": "text/plain" });
            res.end("ok");
            return;
        }
        if (pathname === "/ntfy/_received" && req.method === "GET") {
            json(res, 200, { receipts: ntfyReceipts });
            return;
        }

        const route = ROUTES[pathname];
        if (!route) {
            res.writeHead(404, { "Content-Type": "text/plain" });
            res.end("not found");
            return;
        }
        const { status, contentType, body } = route();
        res.writeHead(status, { "Content-Type": contentType });
        res.end(body);
    });

    server.listen(PORT, () => {
        console.log(`fixture server listening on http://localhost:${PORT}`);
    });
}
