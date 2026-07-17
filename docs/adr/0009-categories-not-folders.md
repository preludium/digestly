# Categories are the single grouping concept, not a folder hierarchy

**Status: accepted.**

Every [[Feed]] subscription belongs to exactly one category. Categories serve two roles
simultaneously: the grouping label in the reader UI (the filter bar, per-category views) and the
bucket used to organize [[Digest]] output. One concept handling both keeps the model simple and
makes the digest grouping obvious - there is nothing to configure separately.

Each account starts with a single mandatory category: "Other" (the non-deletable catch-all). The
onboarding flow offers optional starter feeds whose categories ("Software Engineering", "AI") are
created on demand when the user subscribes to them - they are not force-created at account
registration. The subscription dialog creates new categories inline; deleting any category
reassigns its feeds to "Other".

## Considered options

**Folder hierarchy (feeds nested in folders).** More expressive, but adds a tree data model, tree
UI, and the question of where a feed in multiple folders lands in the digest. A nested model also
doesn't map cleanly to a digest's linear section list. Rejected.

**Tags (multi-label).** A feed could belong to several tags; the digest would need to decide
which tag to group it under. The extra flexibility buys complexity in exchange for a use case that
hasn't been requested. Rejected.

## Consequences

The `categories` table has a per-user unique name and position. The `subscriptions` table has a
non-nullable `category_id`. Every items-list query includes a category filter. The digest engine
groups by category and emits one AI prompt per non-empty category section. Deleting a category
cascades via the "move to Other" path in `routes/categories.rs`, not a database cascade.
