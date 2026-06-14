CREATE VIRTUAL TABLE IF NOT EXISTS item_search USING fts5(
    item_id UNINDEXED,
    title,
    author,
    summary,
    content,
    feed_title,
    link
);

INSERT INTO item_search (item_id, title, author, summary, content, feed_title, link)
SELECT
    i.id,
    COALESCE(i.title, ''),
    COALESCE(i.author, ''),
    COALESCE(i.summary, ''),
    COALESCE(i.content, ''),
    COALESCE(f.title, f.url, ''),
    COALESCE(i.link, '')
FROM items i
JOIN feeds f ON f.id = i.feed_id
WHERE NOT EXISTS (
    SELECT 1 FROM item_search s WHERE s.item_id = i.id
);
