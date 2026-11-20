//! Size limits enforced by the server and respected by the client.

/// Largest binary WebSocket frame in either direction.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
/// Largest encrypted update (one envelope).
pub const MAX_UPDATE_PAYLOAD: usize = 256 * 1024;
/// Largest encrypted snapshot. Snapshots live in a SQLite row of the Durable
/// Object, whose rows are limited to 2 MB, and must fit one frame.
pub const MAX_SNAPSHOT_PAYLOAD: usize = 960 * 1024;
/// Most entries in one push.
pub const MAX_PUSH_ENTRIES: usize = 100;
/// Total payload bytes a client puts in one push (leaves room for headers).
pub const MAX_PUSH_BYTES: usize = 768 * 1024;
/// Most entries the server returns in one page.
pub const MAX_PAGE_ENTRIES: usize = 200;
/// Longest document, device or workspace identifier.
pub const MAX_ID_LEN: usize = 64;
/// Largest JSON request body of the HTTP API.
pub const MAX_JSON_BODY: usize = 64 * 1024;
/// Largest attachment object the server accepts regardless of configuration.
/// Stays under the 100 MB request body limit of Cloudflare's free plan.
pub const MAX_BLOB_CEILING: u64 = 90 * 1024 * 1024;
/// Default attachment object cap.
pub const DEFAULT_BLOB_MAX: u64 = 64 * 1024 * 1024;
/// Largest sealed part of a large object. Parts live in rows of the Durable
/// Object (limit 2 MB per row) and travel as one HTTP body each.
pub const MAX_PART_PAYLOAD: usize = 1024 * 1024;
/// Most parts in one large object, so a large object is at most 64 MiB sealed.
pub const MAX_PARTS: usize = 64;
/// Plaintext bytes carried by one part. The part's seal adds a header and the
/// envelope, which together stay far below the gap to [`MAX_PART_PAYLOAD`].
pub const PART_CHUNK: usize = MAX_PART_PAYLOAD - 1024;
/// Largest update or snapshot state (plaintext) the client accepts. Fits
/// `MAX_PARTS` parts with room for the headers.
pub const MAX_LARGE_PLAINTEXT: usize = 48 * 1024 * 1024;
