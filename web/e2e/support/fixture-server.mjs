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

// Stable, known titles the specs assert on (must match web/e2e/support/api.ts FIXTURE.*).
export const RSS_FEED_TITLE = "E2E Fixture RSS Feed";
export const RSS_ITEM_TITLE = "E2E Fixture RSS Item One";
export const ATOM_FEED_TITLE = "E2E Fixture Atom Feed";
export const ATOM_ITEM_TITLE = "E2E Fixture Atom Item One";
export const JSON_FEED_TITLE = "E2E Fixture JSON Feed";
export const JSON_ITEM_TITLE = "E2E Fixture JSON Item One";
export const SUMMARY_ITEM_TITLE = "E2E Summary Fixture Item";
export const YOUTUBE_ITEM_TITLE = "E2E YouTube Fixture Video";

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

function youtubeFeedBody() {
    const updated = new Date().toISOString();
    return `<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>E2E YouTube Fixture Feed</title>
  <link href="https://www.youtube.com/channel/e2e-fixture"/>
  <updated>${updated}</updated>
  <entry>
    <title>${YOUTUBE_ITEM_TITLE}</title>
    <link href="https://www.youtube.com/watch?v=e2efixtur01"/>
    <id>yt:e2efixtur01</id>
    <updated>${updated}</updated>
    <summary>Video description used by the local YouTube fixture.</summary>
  </entry>
</feed>
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
};

// Mutable, process-local AI fixture state. Specs configure this through the helper endpoints
// below, so provider calls remain deterministic and never leave the e2e process.
let aiResponses = {};
let aiRequests = [];

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

function aiResponse(model) {
    return (
        aiResponses[model] ?? {
            status: 200,
            text: `AI fixture summary from ${model}.`,
        }
    );
}

function respondOpenAi(res, model) {
    const response = aiResponse(model);
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

function respondGemini(res, model) {
    const response = aiResponse(model);
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
        const { pathname } = new URL(
            req.url ?? "/",
            `http://localhost:${PORT}`,
        );
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
            respondOpenAi(res, model);
            return;
        }
        if (pathname === "/ai-mock/interactions" && req.method === "POST") {
            const body = await readJson(req);
            const model = body.model ?? "unknown";
            aiRequests.push({ kind: "gemini", model });
            respondGemini(res, model);
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
