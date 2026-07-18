// feed-cards.spec.ts — issue #45 (E2E: feed reading)
// ItemCard.tsx: read (muted) vs unread, the starred corner badge, the video play overlay, and the
// 3-step thumbnail fallback (item image → feed icon → site favicon → Reddit/generic), walked
// using the F1 fixture's /image/broken endpoint.
//
// YouTube duration_secs can never be populated through real ingest (feed-rs only fills it from an
// RSS2 <itunes:duration>, which the Atom/YouTube parser never reads - see fixture-server.mjs's
// youtubeFeedBody comment and the epic's KNOWN BACKEND LIMITATIONS). So the duration badge is
// intentionally not asserted here - only the play overlay, which real ingest does produce.
//
// Reddit score/comments/min-score filtering is out of scope for the same reason (Reddit ingest is
// not fixture-reachable - process_reddit always polls hardcoded reddit.com). The Reddit *logo*
// fallback step below only needs a `kind: "reddit"` item with no image candidates, which this
// spec fabricates client-side via a route mock - it does not need a real scored Reddit item.
import { expect, test } from "@playwright/test";
import {
    FIXTURE,
    itemIdByTitle,
    registerUser,
    seedFeedWithItems,
    seedItems,
    seedYoutubeFeed,
} from "./support/api";

test.describe("feed cards", () => {
    test("read items render muted and starred items show the corner star", async ({
        page,
    }) => {
        await registerUser(page.request);
        await seedFeedWithItems(page.request);
        const readId = await itemIdByTitle(page.request, FIXTURE.rssItemTitle);
        const starredId = await itemIdByTitle(
            page.request,
            "E2E Fixture RSS Item Two",
        );
        await seedItems(page.request, [
            { itemId: readId, isRead: true },
            { itemId: starredId, isStarred: true },
        ]);

        await page.goto("/");
        const readCard = page
            .getByTestId("item-card")
            .filter({ hasText: FIXTURE.rssItemTitle });
        const starredCard = page
            .getByTestId("item-card")
            .filter({ hasText: "E2E Fixture RSS Item Two" });

        await expect(readCard).toHaveClass(/opacity-60/);
        await expect(readCard.locator("h3")).toHaveClass(
            /text-muted-foreground/,
        );
        await expect(readCard.locator("svg.lucide-star")).toHaveCount(0);

        await expect(starredCard).not.toHaveClass(/opacity-60/);
        await expect(starredCard.locator("svg.lucide-star")).toBeVisible();
    });

    test("video items show the play overlay (duration badge has no data through real ingest)", async ({
        page,
    }) => {
        await registerUser(page.request);
        // The fixture's media:thumbnail points at a real ytimg.com URL - intercept it so the
        // card's own image fallback never reaches the public internet, regardless of what it
        // resolves to.
        await page.route("https://i.ytimg.com/**", (route) =>
            route.fulfill({
                status: 404,
                contentType: "text/plain",
                body: "stub",
            }),
        );
        await seedYoutubeFeed(page.request);

        await page.goto("/");
        const card = page.getByTestId("item-card");
        await expect(card).toBeVisible();
        await expect(card.locator("svg.lucide-play")).toBeVisible();
    });

    test("thumbnail fallback walks image -> feed icon -> favicon -> generic, exercising /image/broken at each step", async ({
        page,
    }) => {
        await registerUser(page.request);
        await seedFeedWithItems(page.request);
        const itemId = await itemIdByTitle(page.request, FIXTURE.rssItemTitle);

        const listResponse = await page.request.get("/api/items");
        const body = (await listResponse.json()) as {
            items: Array<Record<string, unknown>>;
        };
        const target = body.items.find((i) => i.id === itemId);
        if (!target)
            throw new Error("expected the seeded rss item in the list");

        const imageUrl = "https://e2e-thumb-fixture.invalid/cover.jpg";
        const feedIconUrl = "https://e2e-thumb-fixture.invalid/icon.png";
        const siteUrl = "https://e2e-thumb-fixture.invalid";
        const faviconUrl = `${siteUrl}/favicon.ico`;
        target.image_url = imageUrl;
        target.feed_icon_url = feedIconUrl;
        target.site_url = siteUrl;

        await page.route(
            (url) => url.pathname === "/api/items",
            (route) => route.fulfill({ json: body }),
        );

        const hits = { cover: 0, icon: 0 };
        const relayToBrokenImage = async (
            route: import("@playwright/test").Route,
        ) => {
            const broken = await page.request.get(
                "http://localhost:8098/image/broken",
            );
            await route.fulfill({
                status: broken.status(),
                contentType: broken.headers()["content-type"] ?? "text/plain",
                body: await broken.text(),
            });
        };
        await page.route(imageUrl, async (route) => {
            hits.cover += 1;
            await relayToBrokenImage(route);
        });
        await page.route(feedIconUrl, async (route) => {
            hits.icon += 1;
            await relayToBrokenImage(route);
        });
        // NOTE: a route registered for the favicon URL itself is deliberately NOT asserted on for
        // a hit count. Verified by direct reproduction (an isolated `<img src=".../favicon.ico">`
        // outside this app entirely): Chromium/CDP never surfaces a request for a path literally
        // named "favicon.ico" through the normal Network domain that `page.on("request")`/
        // `page.route()` observe - it's diverted through the browser's own internal favicon
        // fetcher. That's a browser/tooling quirk, not an app bug: the <img> element still exists
        // and still eventually errors (proven below by the chain completing to the generic
        // fallback, which cannot happen unless the favicon candidate was reached and also
        // failed), it's just unobservable at this layer.
        await page.route(faviconUrl, (route) =>
            route.fulfill({
                status: 404,
                contentType: "text/plain",
                body: "x",
            }),
        );

        await page.goto("/");
        const card = page
            .getByTestId("item-card")
            .filter({ hasText: FIXTURE.rssItemTitle });
        await expect(card).toBeVisible();

        // Every candidate 404s in turn; once exhausted (image, feed icon, and - per the note
        // above - favicon too), Thumb falls to the generic icon and no <img> remains in the card.
        await expect(card.locator("img")).toHaveCount(0, { timeout: 10_000 });
        await expect(card.locator("svg.lucide-image")).toBeVisible();

        expect(hits.cover).toBeGreaterThan(0);
        expect(hits.icon).toBeGreaterThan(0);
    });

    test("a Reddit-kind item with no image candidates falls back to the Reddit logo without crashing", async ({
        page,
    }) => {
        const pageErrors: Error[] = [];
        page.on("pageerror", (err) => pageErrors.push(err));

        await registerUser(page.request);
        await seedFeedWithItems(page.request);
        const itemId = await itemIdByTitle(page.request, FIXTURE.rssItemTitle);

        const listResponse = await page.request.get("/api/items");
        const body = (await listResponse.json()) as {
            items: Array<Record<string, unknown>>;
        };
        const target = body.items.find((i) => i.id === itemId);
        if (!target)
            throw new Error("expected the seeded rss item in the list");
        // No image_url/feed_icon_url and kind "reddit" (faviconOf excludes reddit/youtube), so
        // Thumb has zero candidates and renders <RedditLogo/> (ItemCard.tsx:72-78).
        target.kind = "reddit";

        await page.route(
            (url) => url.pathname === "/api/items",
            (route) => route.fulfill({ json: body }),
        );

        await page.goto("/");
        const card = page
            .getByTestId("item-card")
            .filter({ hasText: FIXTURE.rssItemTitle });
        await expect(card).toBeVisible();
        // RedditLogo.tsx renders a plain inline <svg viewBox="0 0 256 256">, not an image import -
        // this is what confirms the previously-fixed `.svg?react` latent bug does not reproduce.
        await expect(card.locator('svg[viewBox="0 0 256 256"]')).toBeVisible();
        expect(pageErrors).toHaveLength(0);
    });
});
